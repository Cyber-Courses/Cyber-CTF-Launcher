//! Getting this machine ready for a local start: the lab's slot cleared, enough memory, QEMU
//! for emulated machines, UTM open and answering, and a local VM start with one recovery from
//! leftover state.

use std::time::Duration;

use super::super::deploy_log;
use super::{Error, Result, lab};

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
    (available_mb > 0 && needed_mb > available_mb).then(|| deploy_log::low_memory(needed_mb, available_mb))
}

/// A lab run as `target` on `provider` (its VMs, or the helper VM a container lab runs in):
/// Isoloom's own refusal of the pair, if it has one, before any VM is created. QEMU links a VM on
/// one network only; UTM's Vagrant plugin gives a VM no private network, so a lab with a network
/// (every lab) can't run as VMs on UTM; a TPM runs on VirtualBox, libvirt and Hyper-V only; a
/// container's host folders aren't shared into a helper VM everywhere.
pub(super) fn check_provider(prepared: &lab::Prepared, target: lab::Target, provider: super::Provider) -> Result<()> {
    match lab::refusal_on(&prepared.lab, target, provider) {
        Some(why) => Err(Error::Invalid(format!(
            "This lab can't run on {} yet: {why}. Run it on another hypervisor, a server or in your cloud account.",
            provider.label_on(std::env::consts::OS)
        ))),
        None => Ok(()),
    }
}

/// A lab on QEMU: a machine built for another CPU needs that CPU's emulator
/// (`qemu-system-x86_64` for an x86 lab on an Apple Silicon Mac). Its networks' fit (QEMU's
/// two-VM links) is [`check_provider`]'s.
pub(super) async fn check_qemu(prepared: &lab::Prepared, log: &mut impl FnMut(String)) -> Result<()> {
    let host = std::env::consts::ARCH;
    for arch in lab::emulated_arches(&prepared.spec, host) {
        let bin = if arch == "x86_64" { "qemu-system-x86_64" } else { "qemu-system-aarch64" };
        if crate::platform::exec::run(bin, &["--version"], None).await.is_err() {
            // The webview knows a missing tool by its code, and points at the Machine page.
            log(format!("This lab's machines are {arch}: they need QEMU's {arch} emulator."));
            return Err(Error::ToolMissing { tool: bin });
        }
        log(deploy_log::emulating(arch, host));
    }
    Ok(())
}

/// How long UTM may take to open and answer Apple events.
const UTM_WAIT: Duration = Duration::from_secs(60);

/// What UTM is asked, once it runs: its virtual machines. Not its name: UTM answers that while
/// it is still loading, then refuses the VM import a moment later with "UTM is not ready to
/// accept commands. (-2700)"; it lists its VMs only once it takes commands.
const UTM_PROBE: &str = "tell application \"UTM\" to get id of every virtual machine";

/// Why UTM refused the probe when macOS doesn't let this app drive it, with the way out.
pub(super) const UTM_NOT_ALLOWED: &str =
    "macOS doesn't let Cyber CTF control UTM: allow it under System Settings › Privacy & Security › Automation, then start the lab again.";

/// A lab on UTM: Vagrant's UTM plugin drives UTM through Apple events (`osascript`, `utmctl`),
/// reading the VM's state and its forwarded ports from the app, so a UTM that isn't running when
/// `vagrant up` asks, or that this app may not control, fails that read mid-start ("Application
/// isn't running (-600)", "Not authorized to send Apple events (-1743)"). UTM is opened first, in
/// the background (its first-launch dialogs then face the player instead of failing a launch from
/// behind), and waited for until it answers; the automation refusal is named with its way out.
pub(super) async fn check_utm(log: &mut impl FnMut(String)) -> Result<()> {
    if crate::platform::exec::run("open", &["-g", "-a", "UTM"], None).await.is_err() {
        return Err(Error::ToolMissing { tool: "UTM" });
    }
    let started = std::time::Instant::now();
    let mut said = false;
    loop {
        let probe = crate::platform::exec::run_env_timed("osascript", &["-e", UTM_PROBE], None, &[], Duration::from_secs(15)).await;
        match probe {
            Ok(_) => return Ok(()),
            Err(Error::CommandFailed { stderr, .. }) if utm_not_allowed(&stderr) => return Err(Error::Invalid(UTM_NOT_ALLOWED.into())),
            Err(e) if started.elapsed() >= UTM_WAIT => {
                return Err(Error::Invalid(format!("UTM didn't answer within {} s ({e}): open UTM, then start the lab again.", UTM_WAIT.as_secs())));
            }
            Err(_) => {
                if !said {
                    log(deploy_log::OPENING_UTM.into());
                    said = true;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

/// Whether `osascript`'s complaint is macOS refusing this app Apple events to UTM (the
/// Automation permission), which no wait fixes: `Not authorized to send Apple events to UTM.
/// (-1743)`.
pub(super) fn utm_not_allowed(stderr: &str) -> bool {
    stderr.contains("-1743") || stderr.contains("Not authorized to send Apple events")
}
