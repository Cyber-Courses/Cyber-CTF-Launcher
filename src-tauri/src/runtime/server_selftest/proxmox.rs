//! The Proxmox self-test: a real throwaway lab on the server, through the exact path a lab takes.
//!
//! The test lab ([`lab_run::LAB`]: one small Debian VM on its own lab network, nothing to install)
//! is generated for the `proxmox` target and run by the Isothrum runner ([`ServerRun`]) with this
//! server's profile and placement (node, datastores, uplink bridge, the jump when the lab bridge
//! is internal), so it proves what a launch needs: the cloud image downloaded, the lab network
//! (an SDN vnet) created, the VMs started, the lab's router given an address on the uplink (DHCP
//! and internet), the machine reached over SSH through it and a command run inside, then
//! everything destroyed. The teardown always runs, even after a failure, and says what is left
//! when it fails. The run is named `cyberctf-server-selftest-<host id>` ([`lab_run::run_id`]).

use std::path::{Path, PathBuf};

use super::lab_run::{self, Course, SSH, Step, TestRun, run_id};
use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::runtime::docker::Check;
use crate::runtime::isothrum::{ServerPlace, ServerRun};
use crate::runtime::launch::server_place;
use crate::runtime::paths::runner_home;
use crate::runtime::{Runtime, lab, server};

/// The steps of its own the run reports, in order (the frontend shows the same plan).
const IMAGE: Step = ("image", "Download the Debian cloud image");
const NETWORK: Step = ("network", "Create the test's lab network");
const VM: Step = ("vm", "Start the test VMs");
const UPLINK: Step = ("uplink", "The lab's router gets an address on the uplink");

/// The steps the test reports, in order (the frontend lists the same: [`super::PLANS`]).
#[cfg(test)]
pub(super) const PLAN: [&str; 9] = [super::CONNECT, lab_run::PREPARE.0, IMAGE.0, NETWORK.0, VM.0, UPLINK.0, SSH.0, lab_run::COMMAND.0, lab_run::CLEANUP.0];

/// The test lab's run on the server, where a launch would put it.
pub(super) struct OnServer {
    run: ServerRun,
    place: ServerPlace,
}

impl TestRun for OnServer {
    async fn up(&self, log: &mut (dyn FnMut(String) + Send)) -> Result<()> {
        self.run.up(&self.place, &[], &mut |l| log(l)).await
    }
    async fn check(&self) -> Result<Check> {
        self.run.check().await
    }
    async fn down(&self, log: &mut (dyn FnMut(String) + Send)) -> Result<()> {
        self.run.down(&self.place, &mut |l| log(l)).await
    }
}

/// Writes the test lab into `work` and generates it for Proxmox, as a launch does.
fn write_lab(work: &Path) -> Result<PathBuf> {
    let dir = work.join("lab");
    lab_run::write_lab(work, lab::terraform(&dir, Runtime::Vm, "proxmox").1)
}

/// The test lab's run on server `id`, from its folder `dir`: also how a lab a previous test kept
/// is removed ([`lab_run::clear_kept`]).
pub(super) async fn open(dirs: &AppDirs, id: &str, conn: &server::Connection, dir: PathBuf) -> Result<OnServer> {
    let run = ServerRun::new(&runner_home(dirs)?, &dir, &run_id(id), Runtime::Vm)?;
    let place = Box::pin(server_place(dirs, id, conn, &run)).await?;
    Ok(OnServer { run, place })
}

/// The test lab written into `work`, generated for Proxmox, and its run opened where a launch
/// would put it, with the node it runs on.
pub(super) async fn prepare(dirs: &AppDirs, id: &str, conn: &server::Connection, work: &Path) -> Result<(OnServer, Option<String>)> {
    let on = Box::pin(open(dirs, id, conn, write_lab(work)?)).await?;
    let node = on.place.node.clone().map(|n| format!("on node {n}"));
    Ok((on, node))
}

/// How the test lab comes up on server `conn`: its router on the profile's uplink bridge.
pub(super) fn course(conn: &server::Connection) -> impl Course + use<> {
    Pve { bridge: bridge_of(conn) }
}

/// The uplink bridge the lab's router uses (the profile's, else the module's default).
fn bridge_of(conn: &server::Connection) -> String {
    conn.tf_vars.iter().find(|(k, _)| k == "uplink_bridge").map_or_else(|| "vmbr0".to_owned(), |(_, v)| v.clone())
}

/// How the test lab comes up on Proxmox: Terraform's lines for the image, the lab network and the
/// VM, then the runner waiting for the lab to be ready; `bridge` is the uplink its router uses.
struct Pve {
    bridge: String,
}

impl Course for Pve {
    const UP: &'static [Step] = &[IMAGE, NETWORK, VM, UPLINK, SSH];

    fn ends(&mut self, line: &str) -> Option<usize> {
        marker(line)
    }

    fn explain(&self, raw: &str) -> Option<(usize, String)> {
        explain(raw, &self.bridge)
    }

    fn left(&self, e: &Error) -> String {
        format!(
            "Couldn't remove everything the test created on the server ({e}). Its VMs (named iso<slot>-…, tagged cyberctf-server-selftest) and their lab network may still be there: the next test removes them first, or delete them in the Proxmox web UI."
        )
    }
}

/// The resources whose creation ends a step of [`Pve::UP`], by what of their Terraform address can't
/// be masked: the runner replaces a saved secret's value in every line, and a credential that is,
/// or contains, "proxmox" turns `proxmox_download_file.debian_12` into `***_download_file.debian_12`.
/// So the provider's prefix isn't matched, only the rest of the type and the resource's name.
const DOWNLOAD_FILE: &str = "_download_file.";
const SDN_APPLY: &str = "_sdn_applier.env";
const PROBE_VM: &str = "_virtual_environment_vm.probe";

/// The step a line of the run ends, as its index in [`Pve::UP`]: Terraform's line for the image, the
/// lab network's SDN apply, the lab machine's VM, then the runner waiting for the machines to be
/// ready (the apply, and so the router's uplink address, done).
fn marker(line: &str) -> Option<usize> {
    let complete = |resource: &str| line.split_once(": Creation complete").is_some_and(|(address, _)| address.contains(resource));
    if complete(DOWNLOAD_FILE) {
        Some(0)
    } else if complete(SDN_APPLY) {
        Some(1)
    } else if complete(PROBE_VM) {
        Some(2)
    } else if line.starts_with("Waiting for the lab to be ready") {
        Some(3)
    } else {
        None
    }
}

/// The failures people hit, as the step they belong to and what to do, in the player's terms.
fn explain(raw: &str, bridge: &str) -> Option<(usize, String)> {
    if raw.contains("got no IPv4 address on its uplink bridge") {
        return Some((
            3,
            format!(
                "The test VM got no address on bridge {bridge}: the bridge has no DHCP server reachable from the VMs (or no internet). Use an uplink bridge with DHCP and internet."
            ),
        ));
    }
    if raw.contains("does not exist") && raw.contains("bridge") {
        return Some((
            2,
            format!(
                "A test VM couldn't start because its network bridge doesn't exist on the node. If it is the lab network's, the node doesn't load its SDN configuration: add `source /etc/network/interfaces.d/*` at the end of /etc/network/interfaces, then run `ifreload -a`. Details: {raw}"
            ),
        ));
    }
    if raw.contains(DOWNLOAD_FILE.trim_end_matches('.')) {
        return Some((0, format!("The server couldn't download the Debian cloud image (it needs internet, and a storage with 'iso' content). Details: {raw}")));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::lab_run::scripted::*;
    use super::*;

    /// The test lab's course on Proxmox, its router on `bridge`.
    fn pve(bridge: &str) -> Pve {
        Pve { bridge: bridge.into() }
    }

    /// Terraform's and the runner's lines of a run that went all the way.
    const ALL_THE_WAY: [&str; 6] = [
        "Creating the lab's VMs on the server…",
        "proxmox_download_file.debian_12: Still creating... [10s elapsed]",
        "proxmox_download_file.debian_12: Creation complete after 41s [id=local:iso/debian-12.qcow2.img]",
        "proxmox_sdn_applier.env: Creation complete after 2s [id=env]",
        "proxmox_virtual_environment_vm.probe: Creation complete after 33s [id=105]",
        "Waiting for the lab to be ready: 10.254.250.10 running: cloud-init",
    ];

    #[tokio::test]
    async fn a_run_that_works_reports_every_step_and_tears_down() {
        let (r, seen) = reporter();
        let run = Scripted::new(&ALL_THE_WAY, None);
        let ran = lab_run::test(&r, &run, pve("vmbr1")).await;
        assert!(ran.result.is_ok() && ran.cleaned);
        assert_eq!(ends(&seen), ["image:ok", "network:ok", "vm:ok", "uplink:ok", "ssh:ok", "command:ok", "cleanup:ok"]);
        // The steps the plan lists after the connection and the preparation, in its order.
        assert_eq!(ends(&seen), PLAN[2..].iter().map(|s| format!("{s}:ok")).collect::<Vec<_>>());
        assert_eq!(run.calls(), ["up", "check", "down"]);
        // The running step shows the tool's latest line.
        assert!(
            seen.lock().unwrap().iter().any(|e| e.step == "image" && e.state == "running" && e.detail.as_deref().is_some_and(|d| d.contains("Still creating")))
        );
        assert_eq!(detail(&seen, "command", "ok"), "Linux x86_64");
    }

    #[tokio::test]
    async fn no_dhcp_on_the_uplink_fails_that_step_in_plain_words_and_still_tears_down() {
        let (r, seen) = reporter();
        let raw = "terraform apply failed: Error: Resource postcondition failed … VM `router` got no IPv4 address on its uplink bridge `vmbr1` within 8m: the bridge has no DHCP…";
        let run = Scripted::new(&ALL_THE_WAY[..4], Some(raw));
        let ran = lab_run::test(&r, &run, pve("vmbr1")).await;
        let err = ran.result.unwrap_err().to_string();
        assert_eq!(
            err,
            "The test VM got no address on bridge vmbr1: the bridge has no DHCP server reachable from the VMs (or no internet). Use an uplink bridge with DHCP and internet."
        );
        assert!(ran.cleaned);
        // The VM step had no line of its own: skipped, not claimed; nothing after the failure.
        assert_eq!(ends(&seen), ["image:ok", "network:ok", "vm:skip", "uplink:fail", "cleanup:ok"]);
        assert_eq!(detail(&seen, "uplink", "fail"), err);
        assert_eq!(run.calls(), ["up", "down"], "no command after a failed start, and always the teardown");
    }

    #[tokio::test]
    async fn an_unknown_failure_belongs_to_the_running_step() {
        let (r, seen) = reporter();
        let run = Scripted::new(&ALL_THE_WAY[..3], Some("401 Unauthorized"));
        let ran = lab_run::test(&r, &run, pve("vmbr0")).await;
        assert_eq!(ran.result.unwrap_err().to_string(), "401 Unauthorized");
        assert_eq!(ends(&seen), ["image:ok", "network:fail", "cleanup:ok"]);
    }

    #[tokio::test]
    async fn a_failed_teardown_says_what_is_left_and_keeps_the_runs_folder() {
        let (r, seen) = reporter();
        let mut run = Scripted::new(&ALL_THE_WAY, None);
        run.down = Some("terraform destroy failed: timeout");
        let ran = lab_run::test(&r, &run, pve("vmbr1")).await;
        assert!(!ran.cleaned, "the folder stays so the next test removes the lab first");
        let err = ran.result.unwrap_err().to_string();
        assert!(err.starts_with("Couldn't remove everything the test created on the server (terraform destroy failed: timeout)"), "{err}");
        assert!(err.contains("the next test removes them first"));
        assert_eq!(ends(&seen).last().map(String::as_str), Some("cleanup:fail"));
        // Also after a failed start: the teardown runs, and its failure is said too.
        let (r, seen) = reporter();
        let mut run = Scripted::new(&[], Some("boom"));
        run.down = Some("still there");
        let ran = lab_run::test(&r, &run, pve("vmbr1")).await;
        assert!(!ran.cleaned && ran.result.unwrap_err().to_string() == "boom");
        assert_eq!(ends(&seen), ["image:fail", "cleanup:fail"]);
    }

    #[tokio::test]
    async fn a_failed_command_fails_its_step() {
        let (r, seen) = reporter();
        let mut run = Scripted::new(&ALL_THE_WAY, None);
        run.check_ok = false;
        let ran = lab_run::test(&r, &run, pve("vmbr1")).await;
        assert!(ran.result.unwrap_err().to_string().starts_with("the command inside the test VM failed"));
        assert_eq!(ends(&seen)[5..], ["command:fail".to_owned(), "cleanup:ok".to_owned()]);
    }

    #[test]
    fn a_missing_bridge_and_a_failed_download_are_explained() {
        let (at, why) = explain("Error: bridge 'i1n0' does not exist", "vmbr1").unwrap();
        assert_eq!(at, 2);
        assert!(why.contains("source /etc/network/interfaces.d/*") && why.contains("ifreload -a"));
        assert_eq!(explain("proxmox_download_file.debian_12: 404", "vmbr1").unwrap().0, 0);
        assert_eq!(explain("***_download_file.debian_12: 404", "vmbr1").unwrap().0, 0, "the provider's name masked");
        assert!(explain("something else", "vmbr1").is_none());
        assert_eq!(run_id("ab12"), "server-selftest-ab12");
    }

    #[test]
    fn a_step_ends_on_its_resource_even_with_the_providers_name_masked() {
        // Regression: a saved credential containing "proxmox" had the runner mask the provider's
        // name in every line; no marker matched, and the test sat on its first step to the end.
        for prefix in ["proxmox", "***"] {
            assert_eq!(marker(&format!("{prefix}_download_file.debian_12: Creation complete after 3s [id=local:iso/debian.img]")), Some(0));
            assert_eq!(marker(&format!("{prefix}_sdn_applier.env: Creation complete after 1s")), Some(1));
            assert_eq!(marker(&format!("{prefix}_virtual_environment_vm.probe: Creation complete after 11s [id=101]")), Some(2));
            assert_eq!(marker(&format!("{prefix}_virtual_environment_vm.probe: Still creating... [10s elapsed]")), None);
            assert_eq!(marker(&format!("{prefix}_virtual_environment_vm.isoloom_router: Creation complete after 9s")), None);
        }
        assert_eq!(marker("Waiting for the lab to be ready"), Some(3));
        assert_eq!(marker("Apply complete! Resources: 5 added"), None);
    }

    /// The test lab compiles for Proxmox, and the resources the step markers watch are the ones
    /// Isoloom generates for it.
    #[test]
    fn the_test_lab_generates_for_proxmox_with_the_watched_resources() {
        let work = std::env::temp_dir().join(format!("cyberctf-pve-selftest-{}", rand::random::<u32>()));
        let dir = write_lab(&work).unwrap();
        let module = lab::terraform(&dir, Runtime::Vm, "proxmox").0;
        assert!(module.join("run.json").is_file());
        let tf: String = std::fs::read_dir(&module)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "tf"))
            .map(|e| std::fs::read_to_string(e.path()).unwrap())
            .collect();
        for resource in [
            format!("resource \"proxmox{}\"", DOWNLOAD_FILE.trim_end_matches('.')),
            format!("resource \"proxmox{}\" \"env\"", SDN_APPLY.trim_end_matches(".env")),
            format!("resource \"proxmox{}\" \"probe\"", PROBE_VM.trim_end_matches(".probe")),
            "resource \"proxmox_virtual_environment_vm\" \"isoloom_router\"".into(),
            "got no IPv4 address on its uplink bridge".into(),
        ] {
            assert!(tf.contains(&resource), "{resource} missing from the generated module");
        }
        assert!(isothrum_runner::RunName::new(&crate::runtime::docker::project(&run_id("0123456789abcdef"))).is_ok());
        std::fs::remove_dir_all(work).unwrap();
    }
}
