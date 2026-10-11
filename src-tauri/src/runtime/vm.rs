use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::model::Park;
use super::providers::Provider;
use super::{LabStatus, Machine};
use crate::error::Result;
use crate::exec::{run, run_env_timed, run_read, stream};

/// A status read must never hang the status poll: a wedged VirtualBox (its global lock held by
/// a stuck VBoxManage) would otherwise pile up one blocked `vagrant status` per poll tick.
const STATUS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

/// How long a best-effort cleanup `vagrant destroy` may run before we give up waiting and fall
/// back to hypervisor-level teardown: a wedged VirtualBox (a stuck VBoxManage) must not hang a
/// start indefinitely.
const CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

/// A cleanup `vagrant destroy`, bounded so it can't hang the caller. Errors and timeouts are
/// ignored: the hypervisor-level cleanup that follows handles whatever the destroy didn't.
async fn stop_bounded(dir: &Path, env: &[(String, String)]) {
    let _ = tokio::time::timeout(CLEANUP_TIMEOUT, stop(dir, env, |_l: String| {})).await;
}

/// Isoloom's provisioning controller: the machine that runs Ansible against the others. Not a
/// target, so it is powered off once the lab is built, and booted again to provision or check.
pub const CONTROLLER: &str = "isoloom-controller";

fn has_controller(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join("Vagrantfile")).is_ok_and(|t| t.contains(&format!("config.vm.define \"{CONTROLLER}\"")))
}

/// Whether this folder's VMs run on QEMU (vagrant-qemu). Its private networks link exactly two
/// VMs, so there is no room for an attack VM: the controller, on the lab network already, stays
/// on as the player's attack position.
pub fn on_qemu(dir: &Path) -> bool {
    std::fs::read_dir(dir.join(".vagrant").join("machines")).is_ok_and(|it| it.flatten().any(|m| m.path().join(Provider::Qemu.id()).is_dir()))
}

/// Powers the controller off (state kept) once the lab is built: it has no part in the attack
/// surface and would only cost memory. Best effort. On QEMU it stays on: it is the attacker.
async fn park_controller(dir: &Path, env: &[(String, String)], log: &mut impl FnMut(String)) {
    if !has_controller(dir) || on_qemu(dir) {
        return;
    }
    log("Powering off the setup machine (it comes back for provisioning and checks)…".into());
    if let Err(e) = vagrant(dir, &["halt", CONTROLLER], env, &mut *log).await {
        log(format!("The setup machine stays on: {e}"));
    }
}

/// A VirtualBox VM whose saved state was cut off (a suspend killed midway, a host crash) is
/// left `aborted-saved`, and VirtualBox then refuses every change to it ("the machine is not
/// mutable"), which makes `vagrant up` fail on this folder's VMs for good. The saved state is
/// worthless at that point: discard it, so the VM boots fresh from its disk. Best effort.
pub(super) async fn discard_aborted_saved(dir: &Path, log: &mut impl FnMut(String)) {
    for name in vm_names(dir) {
        let Ok(info) = run_read("VBoxManage", &["showvminfo", &name, "--machinereadable"], None).await else { continue };
        if info.lines().any(|l| l.trim() == "VMState=\"aborted-saved\"") {
            log(format!("{name}: its saved state is unusable (it was cut off); discarding it so the VM boots fresh."));
            let _ = run("VBoxManage", &["discardstate", &name], None).await;
        }
    }
}

/// `env` reaches the Vagrantfile and its provisioners (e.g. the evidence claim).
pub async fn start(dir: &Path, provider: Provider, env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    discard_aborted_saved(dir, &mut log).await;
    vagrant(dir, &["up", "--provider", provider.id()], env, &mut log).await?;
    park_controller(dir, env, &mut log).await;
    Ok(())
}

/// Runs the lab's setup again on its running machines, without rebuilding it: the repair for a
/// machine whose setup was cut short (a domain join that timed out). With a controller, the
/// setup is its Ansible play against every machine, so the controller is booted, the play rerun
/// (after `machine`'s own steps, when one is named) and the controller powered off again.
/// Without one, it is each machine's own provisioners (`vagrant provision [machine]`).
pub async fn provision(dir: &Path, machine: Option<&str>, env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    if has_controller(dir) {
        discard_aborted_saved(dir, &mut log).await;
        log("Booting the setup machine…".into());
        vagrant(dir, &["up", CONTROLLER, "--no-provision"], env, &mut log).await?;
        if let Some(m) = machine.filter(|m| *m != CONTROLLER) {
            vagrant(dir, &["provision", m], env, &mut log).await?;
        }
        log("Running the lab's setup play…".into());
        let result = vagrant(dir, &["provision", CONTROLLER, "--provision-with", "ansible"], env, &mut log).await;
        park_controller(dir, env, &mut log).await;
        return result;
    }
    let mut args = vec!["provision"];
    if let Some(m) = machine {
        args.push(m);
    }
    vagrant(dir, &args, env, &mut log).await
}

/// A streamed Vagrant command whose failure leads with Ansible's own verdict.
async fn vagrant(dir: &Path, args: &[&str], env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    // When a provisioner's Ansible run fails, Vagrant only says "the SSH command responded with a
    // non-zero exit status"; the reason (`fatal: [ws01]: UNREACHABLE! ... winrm ... timed out`)
    // is hundreds of lines up in the stream. Keep Ansible's last verdict and lead the error with it.
    let mut verdict: Option<String> = None;
    let result = stream("vagrant", args, Some(dir), env, |line: String| {
        if let Some(v) = ansible_verdict(&line) {
            verdict = Some(v);
        }
        log(line);
    })
    .await;
    lead_with_verdict(result, verdict)
}

/// A failed command's error, led by Ansible's verdict when the stream had one.
fn lead_with_verdict(result: Result<()>, verdict: Option<String>) -> Result<()> {
    match (result, verdict) {
        (Err(crate::error::Error::CommandFailed { command, stderr }), Some(v)) => {
            Err(crate::error::Error::CommandFailed { command, stderr: format!("{v}\n{stderr}") })
        }
        (result, _) => result,
    }
}

/// Parks the lab's VMs without destroying them: `Pause` saves their state to disk (`vagrant
/// suspend`, resumes in seconds), `Shutdown` powers them off cleanly (`vagrant halt`).
pub async fn park(dir: &Path, mode: Park, env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    stream("vagrant", &[park_verb(mode)], Some(dir), env, &mut log).await
}

/// The Vagrant command that parks VMs in `mode`.
fn park_verb(mode: Park) -> &'static str {
    match mode {
        Park::Pause => "suspend",
        Park::Shutdown => "halt",
    }
}

/// Brings parked VMs back: a saved VM resumes where it was, a powered-off one boots. Never
/// provisions again (the lab was built already; a second Ansible run could change it), and
/// stays on the provider the VMs were created with.
pub async fn resume(dir: &Path, env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    discard_aborted_saved(dir, &mut log).await;
    let provider = status(dir, env).await.ok().and_then(|s| s.provider);
    stream("vagrant", &resume_args(provider.as_deref()), Some(dir), env, &mut log).await?;
    park_controller(dir, env, &mut log).await;
    Ok(())
}

/// `vagrant up` that brings parked VMs back on the provider they were created with.
fn resume_args(provider: Option<&str>) -> Vec<&str> {
    let mut args = vec!["up", "--no-provision"];
    if let Some(p) = provider {
        args.extend(["--provider", p]);
    }
    args
}

/// Ansible's own account of a failure in a streamed Vagrant line (the `<machine>: ` prefix
/// stripped): a `fatal: [host]: UNREACHABLE!/FAILED!` line, trimmed to a readable length.
fn ansible_verdict(line: &str) -> Option<String> {
    let body = line.trim();
    // Vagrant prefixes guest output with "<machine>: "; a bare line has no prefix to strip.
    let verdict = if body.starts_with("fatal: [") {
        body
    } else {
        let (machine, rest) = body.split_once(": ")?;
        if machine.contains(' ') || !rest.starts_with("fatal: [") {
            return None;
        }
        rest
    };
    let mut v = verdict.to_string();
    if v.len() > 300 {
        // Cut on a character boundary: the message may carry non-ASCII text.
        let cut = (0..=297).rev().find(|&i| v.is_char_boundary(i)).unwrap_or(0);
        v.truncate(cut);
        v.push_str("...");
    }
    Some(v)
}

/// Destroys the VMs so the next start restores the lab's initial state. `env` must carry
/// the same server connection as `start`: Vagrant re-evaluates the Vagrantfile.
pub async fn stop(dir: &Path, env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    let destroyed = stream("vagrant", &["destroy", "--force"], Some(dir), env, &mut log).await;
    // Vagrant only destroys what it still tracks. A lab re-installed from its registered commit
    // loses `.vagrant/` (Vagrant's machine index), so the VMs of an earlier run become orphans
    // Vagrant reports as `not_created` and leaves behind — "Stop & clean up" then looked like it
    // did nothing. Finish at the hypervisor, scoped to this lab's exact VM names, for every
    // hypervisor we know how to clean (each is a no-op where the names don't exist).
    let names: HashSet<String> = vm_names(dir).into_iter().collect();
    if !names.is_empty() {
        recover_virtualbox(&names, &mut log).await;
        recover_parallels(&names, &mut log).await;
    }
    destroyed
}

/// A `vagrant up` line that means the start tripped over leftover state from a crashed or
/// interrupted previous run (an orphaned hypervisor VM or its folder, a stale Vagrant lock)
/// rather than a genuine provisioning failure. Such a start can't succeed until the leftovers
/// are cleared, so the caller clears them (`recover_local`) and tries once more.
pub fn is_stale_state_error(line: &str) -> bool {
    // Only hypervisor/Vagrant *startup* errors that a cleanup+retry can actually fix. Deliberately
    // NOT the bare "already exists" / "already registered" / "name is already in use", which also
    // appear in ordinary successful provisioner output ("group docker already exists", a user or
    // network that already exists): matching those would destroy and re-provision a healthy VM.
    let l = line.to_ascii_lowercase();
    l.contains("verr_already_exists") // VirtualBox: the target VM folder already exists
        || l.contains("could not rename") // the import can't rename its folder to the taken name
        || l.contains("is locked") // Vagrant: "the machine is locked" (a dead process's lock)
        || l.contains("different provider") // "an active machine was found with a different provider"
        || l.contains("single provider at a time")
}

mod recover;
mod status;

#[cfg(test)]
use recover::{folder_candidates, ours_in_prlctl, parse_machine_folder, parse_vbox_vms, recorded_on_other_provider, remove_leftover_folders};
pub(super) use recover::{has_machines, recover_parallels, recover_virtualbox, vm_names, vm_names_from};
pub use recover::{reconcile_provider, recover_local};
pub use status::status;
pub(super) use status::vagrant_topology;
#[cfg(test)]
use status::{cidr, field, parse_status, quoted, status_from};

#[cfg(test)]
mod proptests;
#[cfg(test)]
mod tests;
