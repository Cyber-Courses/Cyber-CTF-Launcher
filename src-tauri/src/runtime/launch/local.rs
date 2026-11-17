//! Getting this machine ready for a local start: the lab's slot cleared, enough memory, QEMU
//! for emulated machines, UTM open and answering, and a local VM start with one recovery from
//! leftover state.

use isothrum_machine::providers::ready::{self, NotReady};
use isothrum_runner::ProviderNeed;

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
/// one, before any VM is created. QEMU links a VM on one network only; UTM boots UTM-format boxes
/// only (Debian 12 and Ubuntu 24.04 built in, each network an isolated vmnet network Isoloom
/// attaches the VMs to); a TPM runs on VirtualBox, libvirt and Hyper-V only.
pub(super) fn check_provider(lab: &lab::Lab, target: lab::Target, provider: super::Provider) -> Result<()> {
    match lab::refusal_on(lab, target, provider) {
        Some(why) => Err(Error::Invalid(format!(
            "This lab can't run on {} yet: {why}. Run it on another hypervisor, a server or in your cloud account.",
            provider.label_on(isothrum_runner::this_host())
        ))),
        None => Ok(()),
    }
}

/// A lab on QEMU: a machine built for another CPU needs that CPU's emulator
/// (`qemu-system-x86_64` for an x86 lab on an Apple Silicon Mac), as isothrum-machine asks it
/// ([`ready::emulators`]). Its networks' fit (QEMU's two-VM links) is [`check_provider`]'s.
pub(super) async fn check_qemu(prepared: &lab::Prepared, log: &mut impl FnMut(String)) -> Result<()> {
    let cx = crate::runtime::providers::machine();
    let host = cx.host.arch;
    match ready::emulators(&cx, lab::emulated_arches(&prepared.spec, host)).await {
        Ok(emulated) => {
            for arch in emulated {
                log(deploy_log::emulating(crate::platform::cpu_name(arch), crate::platform::cpu_name(host)));
            }
            Ok(())
        }
        Err(NotReady::Emulator { arch, program }) => {
            // The webview knows a missing tool by its code, and points at the Machine page.
            log(format!("This lab's machines are {}: they need QEMU's {} emulator.", crate::platform::cpu_name(arch), crate::platform::cpu_name(arch)));
            Err(Error::ToolMissing { tool: program })
        }
        Err(other) => Err(Error::Invalid(other.to_string())),
    }
}

/// Why UTM refused the probe when macOS doesn't let this app drive it, with the way out.
pub(super) const UTM_NOT_ALLOWED: &str =
    "macOS doesn't let Cyber CTF control UTM: allow it under System Settings › Privacy & Security › Automation, then start the lab again.";

/// A lab on UTM: Vagrant's UTM plugin drives UTM through Apple events (`osascript`, `utmctl`),
/// reading the VM's state and its forwarded ports from the app, so a UTM that isn't running when
/// `vagrant up` asks, or that this app may not control, fails that read mid-start ("Application
/// isn't running (-600)", "Not authorized to send Apple events (-1743)"). UTM is opened first,
/// hidden and in the background, and waited for until it answers ([`ready::utm`]); the
/// automation refusal is named with its way out.
pub(crate) async fn check_utm(log: &mut impl FnMut(String)) -> Result<()> {
    let cx = crate::runtime::providers::machine();
    // UTM opening is said as the wait starts: the readiness check tells it through a channel,
    // `log` being this caller's alone.
    let (opening, mut said) = tokio::sync::mpsc::unbounded_channel();
    let ready = async move {
        let mut waiting = move || drop(opening.send(()));
        ready::utm(&cx, ready::UTM_WAIT, &mut waiting).await
    };
    let say = async {
        while said.recv().await.is_some() {
            log(deploy_log::OPENING_UTM.into());
        }
    };
    let (ready, ()) = tokio::join!(ready, say);
    ready.map_err(|e| match e {
        NotReady::NotOpened { .. } => Error::ToolMissing { tool: "UTM" },
        NotReady::Needs(ProviderNeed::Automation) => Error::Invalid(UTM_NOT_ALLOWED.into()),
        NotReady::NoAnswer { waited, said } => {
            Error::Invalid(format!("UTM didn't answer within {} s ({said}): open UTM, then start the lab again.", waited.as_secs()))
        }
        other => Error::Invalid(other.to_string()),
    })
}
