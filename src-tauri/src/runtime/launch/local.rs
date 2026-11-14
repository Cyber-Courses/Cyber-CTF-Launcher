//! Getting this machine ready for a local start: the lab's slot cleared, enough memory, QEMU
//! for emulated machines, and a local VM start with one recovery from leftover state.

use super::super::deploy_log;
use super::{Error, Result, lab};

/// Warns (without blocking) when the host likely can't fit all of a one-VM-per-machine lab's
/// memory, so an out-of-memory failure mid-boot isn't a surprise.
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
    (available_mb > 0 && needed_mb > available_mb).then(|| deploy_log::low_memory(needed_mb, available_mb))
}

/// A lab run as `target` on `provider` (its VMs): Isoloom's own refusal of the pair, if it has
/// one, before any VM is created. QEMU links a VM on one network only; UTM's Vagrant plugin gives
/// a VM no private network, so a lab with a network (every lab) can't run as VMs on UTM; a TPM
/// runs on VirtualBox, libvirt and Hyper-V only.
pub(super) fn check_provider(lab: &lab::Lab, target: lab::Target, provider: super::Provider) -> Result<()> {
    match lab::refusal_on(lab, target, provider) {
        Some(why) => Err(Error::Invalid(format!(
            "This lab can't run on {} yet: {why}. Run it on another hypervisor, a server or in your cloud account.",
            provider.label_on(crate::platform::host::Os::HERE)
        ))),
        None => Ok(()),
    }
}

/// A lab on QEMU: a machine built for another CPU needs that CPU's emulator
/// (`qemu-system-x86_64` for an x86 lab on an Apple Silicon Mac). Its networks' fit (QEMU's
/// two-VM links) is [`check_provider`]'s.
pub(super) async fn check_qemu(prepared: &lab::Prepared, log: &mut impl FnMut(String)) -> Result<()> {
    let host = crate::platform::host::Arch::HERE;
    for arch in lab::emulated_arches(&prepared.spec, host) {
        let bin = arch.qemu_system();
        if crate::platform::exec::run(bin, &["--version"], None).await.is_err() {
            // The webview knows a missing tool by its code, and points at the Machine page.
            log(format!("This lab's machines are {}: they need QEMU's {} emulator.", arch.id(), arch.id()));
            return Err(Error::ToolMissing { tool: bin });
        }
        log(deploy_log::emulating(arch.id(), host.id()));
    }
    Ok(())
}
