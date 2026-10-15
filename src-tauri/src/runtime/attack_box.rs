//! The player's attacker beside a lab: the attack-box container next to a container lab that
//! runs on a lab host (Terraform host, or a local or ESXi lab VM), and the attack VM beside a VM
//! lab on this machine. The attack box on local Docker is `exegol`'s.

use std::path::Path;

use crate::AppHandle;

use super::paths::{lab_dir, state_dir};
use super::providers::Provider;
use super::{Runtime, attack_vm, lab, remote, server, ssh, vm};
use crate::error::{Error, Result};

/// The run env key carrying the player's pick: an attack-box image for a container lab, or the
/// attack VM's Vagrant box (`owner/name`) for a VM lab.
const ATTACKER_ENV: &str = "CYBERCTF_ATTACKBOX_IMAGE";

fn attacker_pick(env: &[(String, String)]) -> Option<&str> {
    env.iter().find(|(k, _)| k == ATTACKER_ENV).map(|(_, v)| v.as_str())
}

/// The attack-box image the player picked, when one was (passed in the run env).
fn attack_box_image(env: &[(String, String)]) -> Option<&str> {
    attacker_pick(env).filter(|i| crate::platform::exec::valid_image(i))
}

/// The attack VM's Vagrant box the player picked for a VM lab, when one was.
pub(super) fn attack_vm_box(env: &[(String, String)]) -> Option<&str> {
    attacker_pick(env).filter(|b| attack_vm::valid_box(b))
}

/// Starts the attack box on a lab host running the lab's Compose file: a container named
/// `attacker` on every network of the Compose project (the spec's name), so segmented labs
/// are reachable from it. The lab network isn't reachable from this machine, so the player's
/// shell goes over SSH into this container.
fn attack_box_script(project: &str, image: &str) -> String {
    format!(
        "set -euo pipefail\ndocker rm -f attacker >/dev/null 2>&1 || true\nmapfile -t networks < <(docker network ls -q --filter label=com.docker.compose.project={project})\n[ -n \"${{networks[*]:-}}\" ] || {{ echo 'the lab has no network' >&2; exit 1; }}\ndocker pull -q {image} >/dev/null\ndocker run -d --name attacker --hostname attacker --network \"${{networks[0]}}\" {image} sleep infinity >/dev/null\nfor n in \"${{networks[@]:1}}\"; do docker network connect \"$n\" attacker; done\n",
        project = ssh::sh_quote(project),
        image = ssh::sh_quote(image),
    )
}

const STARTING: &str = "Starting the attack box next to the lab (this pulls its image the first time)…";
const READY: &str = "Attack box ready.";

/// The attack box on a Terraform lab host (server or cloud), over SSH with the launcher's key.
async fn on_terraform_host(app: &AppHandle, id: &str, spec: &lab::Spec, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
    let Some(image) = attack_box_image(env) else { return Ok(()) };
    let Some(conn) = server::lab_connection(app, &lab_dir(app, id)?)? else { return Ok(()) };
    let tf = server::terraform_target(conn.provider).unwrap_or_default();
    let state = state_dir(app, id, tf)?;
    let target = remote::terraform_host(app, &state).await?.ok_or_else(|| Error::Invalid("the lab host has no address".into()))?;
    log(STARTING.into());
    remote::run_on_terraform_host(&target, &state, &attack_box_script(&spec.name, image)).await?;
    log(READY.into());
    Ok(())
}

/// The attack box in a local or ESXi lab VM (Docker on one VM), through `vagrant ssh`.
async fn in_lab_vm(vagrant: &Path, spec: &lab::Spec, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
    let Some(image) = attack_box_image(env) else { return Ok(()) };
    log(STARTING.into());
    let command = remote::sudo_bash(&attack_box_script(&spec.name, image));
    crate::platform::exec::run_env("vagrant", &["ssh", "-c", &command], Some(vagrant), env).await?;
    log(READY.into());
    Ok(())
}

/// [`in_lab_vm`] once the lab is up: a failing attack box shouldn't read as a failed deploy, so
/// it is noted and the lab stays usable (the shell can retry it).
pub(super) async fn in_lab_vm_or_note(vagrant: &Path, spec: &lab::Spec, env: &[(String, String)], log: &mut impl FnMut(String)) {
    let started = in_lab_vm(vagrant, spec, env, log).await;
    note_failure(started, log);
}

/// [`on_terraform_host`] once the lab is up, noted rather than failing the deploy (see
/// [`in_lab_vm_or_note`]).
pub(super) async fn on_terraform_host_or_note(app: &AppHandle, id: &str, spec: &lab::Spec, env: &[(String, String)], log: &mut impl FnMut(String)) {
    let started = on_terraform_host(app, id, spec, env, log).await;
    note_failure(started, log);
}

/// An attack box that didn't start beside a lab that is up: a line in the log, not an error.
fn note_failure(started: Result<()>, log: &mut impl FnMut(String)) {
    if let Err(e) = started {
        log(format!("The lab is running, but its attack box didn't start: {e}. Open the lab shell to retry it."));
    }
}

/// Starts the attack VM beside a VM lab on this machine, on the hypervisor the lab runs on.
pub async fn start_attack_vm(app: &AppHandle, id: &str, box_name: &str, log: impl FnMut(String)) -> Result<()> {
    let dir = lab_dir(app, id)?;
    // The attack VM lives beside a lab on this machine only (stop and park never reach one on a
    // server): a lab on a host must not get one started here on a remote provider.
    if crate::runtime::server::lab_connection(app, &dir)?.is_some() {
        return Err(Error::Invalid(
            "This lab runs on a server host: the attack VM is only for labs on this machine. Attack it from your own machine or a VM on that host.".into(),
        ));
    }
    let vagrant = lab::vagrant_dir(&dir, Runtime::Vm);
    let status = vm::status(&vagrant, &[]).await?;
    // The lab's internal networks exist as soon as one of its VMs is up; the attacker needn't
    // wait for the last machine to finish booting (and the UI may ask while that happens).
    if !status.machines.iter().any(|m| m.state == "running") {
        return Err(Error::Invalid("start the lab first: the attack VM joins its networks".into()));
    }
    let provider =
        status.provider.as_deref().and_then(Provider::from_id).ok_or_else(|| Error::Invalid("couldn't tell which hypervisor the lab runs on".into()))?;
    if provider.is_remote() {
        return Err(Error::Invalid("This lab's machines run on a remote host: the attack VM is only for labs on this machine.".into()));
    }
    crate::machine::storage::owned::tracked(app, &[], &[box_name.to_string()], attack_vm::start(&dir, &vagrant, provider, box_name, log)).await
}

#[cfg(test)]
mod tests {
    use super::{attack_box_image, attack_box_script, attack_vm_box, note_failure};

    #[test]
    fn a_failed_attack_box_is_a_note_not_a_failed_deploy() {
        // Regression: on a Terraform host the attack box error failed the whole launch after a
        // successful apply, so the lab was never recorded and its welcome never shown.
        let mut lines = Vec::new();
        note_failure(Ok(()), &mut |l| lines.push(l));
        assert!(lines.is_empty());
        note_failure(Err(crate::error::Error::Invalid("no route to host".into())), &mut |l| lines.push(l));
        assert_eq!(lines, ["The lab is running, but its attack box didn't start: no route to host. Open the lab shell to retry it."]);
    }

    fn env(pick: &str) -> Vec<(String, String)> {
        vec![("OTHER".into(), "x".into()), ("CYBERCTF_ATTACKBOX_IMAGE".into(), pick.into())]
    }

    #[test]
    fn the_pick_is_read_as_an_image_or_a_box_by_shape() {
        assert_eq!(attack_box_image(&[]), None);
        assert_eq!(attack_vm_box(&[]), None);
        let image = env("nwodtuhs/exegol:free");
        assert_eq!(attack_box_image(&image), Some("nwodtuhs/exegol:free"));
        let bad = env("x; rm -rf /");
        assert_eq!(attack_box_image(&bad), None);
        assert_eq!(attack_vm_box(&bad), None);
    }

    #[test]
    fn script_quotes_the_project_and_image() {
        let s = attack_box_script("my lab", "repo/img:tag");
        assert!(s.starts_with("set -euo pipefail\n"));
        assert!(s.contains("label=com.docker.compose.project='my lab'"), "{s}");
        assert!(s.contains("docker pull -q 'repo/img:tag'"), "{s}");
        assert!(s.contains("--network \"${networks[0]}\" 'repo/img:tag' sleep infinity"), "{s}");
    }

    /// A lab's generated Vagrantfile with one machine on one network, as the attack VM plans from.
    const LAB_VAGRANTFILE: &str = "config.vm.define \"dc01\" do |m|\n  m.vm.network \"private_network\", ip: \"192.168.56.30\", netmask: \"255.255.255.0\", virtualbox__intnet: \"isoloom-ad-lab\"\n  m.vm.provider \"virtualbox\" do |v|\n    v.name = \"ad · dc01\"\n  end\nend\n";

    #[tokio::test]
    async fn the_attack_vm_needs_a_running_machine_and_a_hypervisor_it_knows() {
        use crate::platform::exec::fake::{Tools, ok};
        let t = crate::testing::test_app::TestApp::new();
        let dir = t.lab("ad", &[]);
        let vagrant = crate::runtime::lab::vagrant_dir(&dir, crate::runtime::Runtime::Vm);
        std::fs::create_dir_all(&vagrant).unwrap();
        std::fs::write(vagrant.join("Vagrantfile"), LAB_VAGRANTFILE).unwrap();
        let cases = [
            ("1,dc01,state,poweroff\n1,dc01,provider-name,virtualbox\n", "start the lab first: the attack VM joins its networks"),
            // No `provider-name` line at all (an odd Vagrant answer), or one for a provider the
            // launcher doesn't drive: refused rather than started on a guess.
            ("1,dc01,state,running\n", "couldn't tell which hypervisor the lab runs on"),
            ("1,dc01,state,running\n1,dc01,provider-name,\n", "couldn't tell which hypervisor the lab runs on"),
            ("1,dc01,state,running\n1,dc01,provider-name,aws\n", "couldn't tell which hypervisor the lab runs on"),
        ];
        for (status, expected) in cases {
            let tools = Tools::rules(&[("vagrant status", ok(status))]);
            let err = super::start_attack_vm(t.handle(), "ad", "kalilinux/rolling", |_| {}).await.unwrap_err().to_string();
            assert_eq!(err, expected, "{status:?}");
            assert!(!tools.called("vagrant up"), "{:?}", tools.calls());
        }
    }

    #[tokio::test]
    async fn an_attack_vm_is_refused_beside_a_lab_running_on_an_esxi_host() {
        use crate::platform::exec::fake::{Tools, ok};
        let t = crate::testing::test_app::TestApp::new();
        let dir = t.lab("ad", &[]);
        let vagrant = crate::runtime::lab::vagrant_dir(&dir, crate::runtime::Runtime::Vm);
        std::fs::create_dir_all(&vagrant).unwrap();
        std::fs::write(vagrant.join("Vagrantfile"), LAB_VAGRANTFILE).unwrap();
        // The lab runs on an ESXi host (its marker says so, and Vagrant tracks it there).
        std::fs::write(dir.join(crate::runtime::server::HOST_MARKER), "esx1").unwrap();
        let tools = Tools::rules(&[("vagrant status", ok("1,dc01,state,running\n1,dc01,provider-name,vmware_esxi\n"))]);
        let r = super::start_attack_vm(t.handle(), "ad", "kalilinux/rolling", |_| {}).await;
        // The attack VM lives beside a lab on this machine only (stop and park never reach one on
        // a server): a lab elsewhere must be refused, not started on a remote provider here.
        assert!(r.is_err(), "an attack VM beside a lab on a server host is refused");
        assert!(!tools.called("--provider vmware_esxi"), "{:?}", tools.calls());
    }

    #[tokio::test]
    async fn no_attack_box_picked_means_nothing_to_start_in_the_lab_vm() {
        let spec: crate::runtime::lab::Spec = serde_yaml_ng::from_str("version: 1\nname: t\nmachines: {}\n").unwrap();
        let mut lines = Vec::new();
        // No pick, or a pick that is a Vagrant box shape but not a valid image: nothing runs.
        super::in_lab_vm_or_note(std::path::Path::new("lab"), &spec, &[], &mut |l| lines.push(l)).await;
        super::in_lab_vm_or_note(std::path::Path::new("lab"), &spec, &env("x; rm"), &mut |l| lines.push(l)).await;
        assert!(lines.is_empty());
    }
}
