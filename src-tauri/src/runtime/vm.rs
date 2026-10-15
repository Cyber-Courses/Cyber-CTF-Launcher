use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::model::Park;
use super::providers::Provider;
use super::{LabStatus, Machine};
use crate::error::Result;
use crate::platform::exec::{Event, Step, Vagrant, run, run_env_timed, run_read};

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
pub const CONTROLLER: &str = super::lab::CONTROLLER;

fn has_controller(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join("Vagrantfile")).is_ok_and(|t| t.contains(&format!("config.vm.define \"{CONTROLLER}\"")))
}

/// Whether this folder's VMs run on QEMU (vagrant-qemu). Its private networks link exactly two
/// VMs, so there is no room for an attack VM: the controller, on the lab network already, stays
/// on as the player's attack position.
pub fn on_qemu(dir: &Path) -> bool {
    crate::platform::exec::machines_on(dir, Provider::Qemu.id())
}

/// One Vagrant command of a lab operation, as a step of a batch (one UAC prompt for the whole
/// operation on Hyper-V): what it logs when it starts, and how a failure is taken.
struct Part {
    step: Step,
    /// Logged when the step starts.
    note: Option<&'static str>,
    /// A best-effort step: its failure is logged after this, and doesn't fail the operation.
    tolerated: Option<&'static str>,
}

impl Part {
    fn new(args: &[&str]) -> Part {
        Part { step: Step::new(args), note: None, tolerated: None }
    }

    fn note(mut self, note: &'static str) -> Part {
        self.note = Some(note);
        self
    }
}

/// Powers the controller off (state kept) once the lab is built: it has no part in the attack
/// surface and would only cost memory. Best effort. None without a controller, or on QEMU (`qemu`),
/// where it stays on: it is the attacker.
fn park_controller(dir: &Path, qemu: bool) -> Option<Part> {
    if !has_controller(dir) || qemu {
        return None;
    }
    Some(Part {
        step: Step::new(&["halt", CONTROLLER]),
        note: Some("Powering off the setup machine (it comes back for provisioning and checks)…"),
        tolerated: Some("The setup machine stays on"),
    })
}

/// Runs an operation's parts in order with `vagrant`, logging their output; the first failure
/// (led by Ansible's verdict when its step had one) is the operation's.
async fn run_parts(vagrant: &Vagrant<'_>, parts: &[Part], log: &mut impl FnMut(String)) -> Result<()> {
    let steps: Vec<Step> = parts.iter().map(|p| p.step.clone()).collect();
    // When a provisioner's Ansible run fails, Vagrant only says "the SSH command responded with a
    // non-zero exit status"; the reason (`fatal: [ws01]: UNREACHABLE! ... winrm ... timed out`)
    // is hundreds of lines up in the stream. Keep each step's last verdict to lead its error with.
    let mut verdicts: Vec<Option<String>> = vec![None; parts.len()];
    let outcomes = vagrant
        .batch(&steps, |e| match e {
            Event::Start(i) => {
                if let Some(n) = parts[i].note {
                    log(n.into());
                }
            }
            Event::Line(i, line) => {
                if let Some(v) = ansible_verdict(&line) {
                    verdicts[i] = Some(v);
                }
                log(line);
            }
            Event::End(i, Err(e)) => {
                if let Some(t) = parts[i].tolerated {
                    log(format!("{t}: {e}"));
                }
            }
            Event::End(_, Ok(_)) => {}
        })
        .await?;
    for (i, outcome) in outcomes.into_iter().enumerate() {
        if let Some(Err(e)) = outcome
            && parts[i].tolerated.is_none()
        {
            return lead_with_verdict(Err(e), verdicts[i].take());
        }
    }
    Ok(())
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
    let mut parts = vec![Part::new(&["up", "--provider", provider.id()])];
    parts.extend(park_controller(dir, provider == Provider::Qemu));
    run_parts(&Vagrant::new(dir, env, provider == Provider::Hyperv), &parts, &mut log).await
}

/// Runs the lab's setup again on its running machines, without rebuilding it: the repair for a
/// machine whose setup was cut short (a domain join that timed out). With a controller, the
/// setup is its Ansible play against every machine, so the controller is booted, the play rerun
/// (after `machine`'s own steps, when one is named) and the controller powered off again.
/// Without one, it is each machine's own provisioners (`vagrant provision [machine]`).
pub async fn provision(dir: &Path, machine: Option<&str>, env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    if has_controller(dir) {
        // The controller is booted: an aborted saved state would make that fail for good.
        discard_aborted_saved(dir, &mut log).await;
    }
    run_parts(&Vagrant::of(dir, env), &provision_parts(dir, machine), &mut log).await
}

/// The steps of [`provision`]: with a controller, boot it, rerun `machine`'s own steps (when
/// named), rerun its play, then power it off again (even after a failure: it would only cost
/// memory). Without one, the machines' own provisioners.
fn provision_parts(dir: &Path, machine: Option<&str>) -> Vec<Part> {
    if !has_controller(dir) {
        let mut args = vec!["provision"];
        args.extend(machine);
        return vec![Part::new(&args)];
    }
    let mut parts = vec![Part::new(&["up", CONTROLLER, "--no-provision"]).note("Booting the setup machine…")];
    if let Some(m) = machine.filter(|m| *m != CONTROLLER) {
        parts.push(Part::new(&["provision", m]));
    }
    parts.push(Part::new(&["provision", CONTROLLER, "--provision-with", "ansible"]).note("Running the lab's setup play…"));
    if let Some(mut park) = park_controller(dir, on_qemu(dir)) {
        park.step = park.step.always();
        parts.push(park);
    }
    parts
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
pub async fn park(dir: &Path, mode: Park, env: &[(String, String)], log: impl FnMut(String)) -> Result<()> {
    Vagrant::of(dir, env).stream(&[park_verb(mode)], log).await
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
    // A status read is never elevated (it is polled): on a non-elevated Hyper-V it may fail,
    // and the machines' folders still tell the provider.
    let provider = status(dir, env).await.ok().and_then(|s| s.provider);
    let hyperv = provider.as_deref() == Some(Provider::Hyperv.id()) || crate::platform::exec::on_hyperv(dir);
    let mut parts = vec![Part::new(&resume_args(provider.as_deref()))];
    parts.extend(park_controller(dir, on_qemu(dir)));
    run_parts(&Vagrant::new(dir, env, hyperv), &parts, &mut log).await
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
    let destroyed = Vagrant::of(dir, env).stream(&["destroy", "--force"], &mut log).await;
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
use recover::{folder_candidates, machine_folder, ours_in_prlctl, parse_machine_folder, parse_vbox_vms, recorded_on_other_provider, remove_leftover_folders};
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
