//! Getting this machine ready for a local start: the lab's slot cleared, enough memory, QEMU
//! for emulated machines, and a local VM start with one recovery from leftover state.

use super::*;

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
pub(super) async fn check_qemu(prepared: &lab::Prepared, log: &mut impl FnMut(String)) -> Result<()> {
    if let Some(why) = lab::qemu_refusal(&prepared.lab) {
        return Err(Error::Invalid(format!("This lab can't run on QEMU yet: {why}. Run it on a server or in your cloud account.")));
    }
    let host = std::env::consts::ARCH;
    for arch in emulated_arches(&prepared.spec, host) {
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
