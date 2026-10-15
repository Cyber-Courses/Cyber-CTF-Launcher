//! Getting this machine ready for a local start: the lab's slot cleared, enough memory, QEMU
//! for emulated machines, and a local VM start with one recovery from leftover state.

use super::*;

/// Guards a local start: refuses when the lab is already running on this machine (Docker or a
/// local VM), and otherwise tears down any stopped or half-created leftovers from a previous
/// start so the fresh start doesn't trip over them (a poweroff VM, dead containers).
pub(super) async fn ensure_local_slot_free(dir: &Path, id: &str, log: &mut impl FnMut(String)) -> Result<()> {
    // Docker containers of this lab on this machine (best effort: before the first start the
    // compose file may not exist yet, and status then errors, which just means nothing to clear).
    if let Ok(s) = docker::status(dir, id).await {
        if s.running {
            return Err(Error::Refused("This lab is already running on this machine. Stop it before starting it again.".into()));
        }
        if docker::containers(dir, id).await.is_ok_and(|n| n > 0) {
            log("Clearing a previous, stopped run…".into());
            let _ = docker::stop(dir, id, |_l: String| {}).await;
        }
    }
    // A local VM for this lab (Docker on one VM, or a VM lab), whichever vagrant folder holds one.
    for vdir in vagrant_dirs(dir) {
        let Ok(s) = vm::status(&vdir, &[]).await else { continue };
        if s.running {
            return Err(Error::Refused("This lab is already running in a VM on this machine. Stop it before starting it again.".into()));
        }
        // "not_created" is a clean slate; anything else (poweroff, aborted, saved) is a leftover.
        if s.machines.iter().any(|m| m.state != "not_created") {
            log("Clearing a previous, incomplete VM…".into());
            let _ = vm::stop(&vdir, &[], |_l: String| {}).await;
        }
    }
    Ok(())
}

/// Warns (without blocking) when the host likely can't fit all of a one-VM-per-machine lab's
/// memory, so an out-of-memory failure mid-boot isn't a surprise. Not used for a container lab
/// in a single VM, where the total would overcount.
pub(super) fn warn_if_low_memory(spec: &lab::Spec, log: &mut impl FnMut(String)) {
    use sysinfo::System;
    let needed_mb = u64::from(lab::memory_mb(spec));
    let mut sys = System::new();
    sys.refresh_memory();
    let available_mb = sys.available_memory() / (1024 * 1024);
    if let Some(warning) = low_memory_warning(needed_mb, available_mb) {
        log(warning);
    }
}

/// The warning for a lab needing `needed_mb` of memory with `available_mb` free (0: unknown).
pub(super) fn low_memory_warning(needed_mb: u64, available_mb: u64) -> Option<String> {
    (available_mb > 0 && needed_mb > available_mb).then(|| {
        format!(
            "This lab's VMs ask for about {needed_mb} MB, but only ~{available_mb} MB is free on this machine. It may run slowly or fail to boot; close other apps or stop other labs if it struggles."
        )
    })
}

/// The CPUs (`std::env::consts::ARCH` names) a lab's VMs are built for that differ from `host`,
/// i.e. the ones QEMU has to emulate.
pub(super) fn emulated_arches(spec: &lab::Spec, host: &str) -> Vec<&'static str> {
    let mut emulated: Vec<&'static str> = spec
        .machines
        .values()
        .filter(|m| m.vm.is_some())
        .map(|m| match m.arch {
            lab::Arch::Amd64 => "x86_64",
            lab::Arch::Arm64 => "aarch64",
        })
        .filter(|a| *a != host)
        .collect();
    emulated.dedup();
    emulated
}

/// A lab on QEMU: its networks must fit QEMU's two-VM links, and a machine built for another CPU
/// needs that CPU's emulator (`qemu-system-x86_64` for an x86 lab on an Apple Silicon Mac).
pub(super) async fn check_qemu(spec: &lab::Spec, log: &mut impl FnMut(String)) -> Result<()> {
    if let Some(why) = lab::qemu_refusal(spec) {
        return Err(Error::Invalid(format!("This lab can't run on QEMU yet: {why}. Run it on a server or in your cloud account.")));
    }
    let host = std::env::consts::ARCH;
    for arch in emulated_arches(spec, host) {
        let bin = if arch == "x86_64" { "qemu-system-x86_64" } else { "qemu-system-aarch64" };
        if crate::platform::exec::run(bin, &["--version"], None).await.is_err() {
            return Err(Error::Invalid(format!(
                "This lab's machines are {arch}, and QEMU's {arch} emulator ({bin}) isn't installed. Reinstall QEMU from the Machine page."
            )));
        }
        log(format!(
            "Emulating {arch} on this {host} machine with QEMU, many times slower than native: a Windows machine takes 15 to 40 minutes to boot, and the lab's setup can take hours."
        ));
    }
    Ok(())
}

/// Starts local VMs with one automatic recovery. A `vagrant up` can fail because a crashed or
/// interrupted previous run left state behind that `ensure_local_slot_free` couldn't see: an
/// orphaned hypervisor VM (VirtualBox "VERR_ALREADY_EXISTS") or a stale lock ("the machine is
/// locked"). When the failure is one of those, clear the leftovers and try once more; a genuine
/// provisioning failure is returned unchanged, never retried.
pub(super) async fn start_local_vm(dir: &Path, provider: Provider, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
    // A missing hypervisor or Vagrant plugin should say so plainly, not fail mid-boot with a
    // raw Vagrant error.
    crate::runtime::providers::ensure_usable(provider).await.map_err(Error::Invalid)?;
    // Switched target since last time (e.g. an ESXi run, now local): clear the old state so
    // `vagrant up` doesn't refuse with "an active machine was found with a different provider".
    vm::reconcile_provider(dir, provider, env, log).await;
    let mut stale = false;
    let first = vm::start(dir, provider, env, |l: String| {
        stale |= vm::is_stale_state_error(&l);
        log(l);
    })
    .await;
    match first {
        Ok(()) => Ok(()),
        Err(_) if stale => {
            log("A previous run left VM state behind. Clearing it, then starting again…".into());
            vm::recover_local(dir, provider, log).await;
            vm::start(dir, provider, env, log).await
        }
        Err(e) => Err(e),
    }
}
