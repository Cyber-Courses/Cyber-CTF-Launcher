//! The hypervisors, servers and cloud accounts labs run on ([`Provider`]), and which local
//! hypervisors this machine can run a lab's VMs on. Detection is isothrum-machine's
//! ([`isothrum_machine::providers`]): what is installed, what Vagrant still needs, and why a
//! hypervisor can't run VMs yet (an [`Unavailable`] code, which the webview says as
//! `machine.unavailable.*`). The launcher keeps its places (Proxmox and the clouds beside the
//! Vagrant providers), the player's preferred hypervisor ([`pick`]), and the player's words for a
//! reason ([`said`]).

mod provider;

pub use isothrum_machine::providers::{Need, ProviderStatus, Unavailable, UtilityState};
pub use provider::Provider;

use crate::platform::host::Os;

/// What isothrum-machine detects with: this machine's tools, run by the runner's processes, and
/// its devices. A test's own when it set one (`testing::Machine`), else every tool missing:
/// a test never reads the machine running it.
fn machine() -> isothrum_machine::Context {
    #[cfg(test)]
    {
        testing::context()
    }
    #[cfg(not(test))]
    {
        let os = Os::HERE.isothrum();
        let host = isothrum_machine::Host {
            os,
            arch: crate::platform::host::Arch::HERE.isothrum(),
            // Detection doesn't read it; the setup's own installs look it up.
            packages: None,
            devices: isothrum_machine::Devices::here(os),
        };
        isothrum_machine::Context::new(std::sync::Arc::new(isothrum_runner::SystemProcesses::default()), host)
    }
}

/// The local hypervisors ready for a lab's VMs on this machine: Vagrant, the hypervisor and its
/// plugin (what the Machine page reports as ready), in [`Provider::ALL`]'s order.
pub async fn ready_here() -> Vec<Provider> {
    if crate::platform::exec::run_read("vagrant", &["--version"], None).await.is_err() {
        return Vec::new();
    }
    ready(&detect(true).await, true)
}

/// Of `statuses` ([`detect`]'s), the local hypervisors ready for a lab's VMs: none without
/// Vagrant (`vagrant_installed`). The machine report carries them, so the webview never works
/// this out again.
pub fn ready(statuses: &[ProviderStatus], vagrant_installed: bool) -> Vec<Provider> {
    if !vagrant_installed {
        return Vec::new();
    }
    // `available` holds only where the hypervisor is there: one that isn't has a reason.
    statuses.iter().filter(|p| !p.remote && p.available).map(|p| Provider::from(p.provider)).collect()
}

/// The hypervisor VM labs run on, of the `ready` ones ([`ready`]): `preferred` (the player's
/// Settings choice) when it is ready, else the first ready one. The one rule the VM self-test and
/// a lab's default place use.
pub fn pick(preferred: Option<Provider>, ready: &[Provider]) -> Option<Provider> {
    preferred.filter(|p| ready.contains(p)).or_else(|| ready.first().copied())
}

/// What the machine check finds for each Vagrant provider this machine can run (its OS and CPU),
/// the Vagrant plugins read only when Vagrant is installed (`vagrant_installed`).
pub async fn detect(vagrant_installed: bool) -> Vec<ProviderStatus> {
    isothrum_machine::providers::detect(&machine(), vagrant_installed).await
}

/// `provider`'s status as the machine report gives it ([`detect`]), checked alone: none when it
/// isn't offered on this machine, or isn't a Vagrant provider. What the server self-test reads
/// before it runs a lab on ESXi.
pub async fn status(provider: Provider) -> Option<ProviderStatus> {
    isothrum_machine::providers::status(&machine(), provider.vagrant()?).await
}

/// `reason` in the player's words, as the English UI says it (the catalogue's
/// `machine.unavailable.*`): for the log and the core's own messages; the webview says the code.
pub fn said(reason: &Unavailable) -> String {
    let section = &crate::english::MACHINE["unavailable"];
    match reason {
        Unavailable::NotInstalled { program: None } => crate::english::sentence(section, "notInstalledNoProgram", &serde_json::Value::Null),
        _ => crate::english::coded(section, reason),
    }
}

/// The groups a player joins to use `provider` on `os`, which apply from their next login
/// (libvirt's and `/dev/kvm`'s, as this machine names `/dev/kvm`'s): none for a provider that
/// isn't a Vagrant one.
pub fn login_groups(provider: Provider, os: Os) -> Vec<String> {
    let Some(vagrant) = provider.vagrant() else { return Vec::new() };
    let mut host = machine().host;
    host.os = os.isothrum();
    isothrum_machine::providers::login_groups(vagrant, &host)
}

/// The group a player joins to open `/dev/kvm` on this machine: the one it belongs to, else
/// `kvm` (the setup's shapes name it).
#[cfg(test)]
pub fn kvm_group() -> String {
    isothrum_machine::providers::kvm_group(&machine().host)
}

/// Where HashiCorp's Vagrant VMware Utility stands on this machine.
pub async fn vmware_utility() -> UtilityState {
    isothrum_machine::providers::vmware_utility(&machine().host).await
}

/// UTM's command line tool, which ships inside its app bundle, on no PATH: in the bundle the
/// machine check finds (the app doesn't move while it runs).
pub(crate) fn utmctl() -> &'static str {
    static UTMCTL: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        isothrum_machine::Devices::here(isothrum_runner::HostOs::Macos).utm_app.join("Contents/MacOS/utmctl").to_string_lossy().into_owned()
    });
    UTMCTL.as_str()
}

/// Whether this computer may share a folder with a VM on `provider`: refused up front for libvirt
/// or QEMU on Linux without the virtiofsd it shares the folder through (isothrum-machine's
/// [`virtiofsd`](isothrum_machine::providers::virtiofsd)), whose start would otherwise fail in
/// its Vagrant plugin.
pub(crate) fn can_share(provider: Provider) -> crate::error::Result<()> {
    let cx = machine();
    share_on(provider, Os::HERE, |p| isothrum_machine::providers::virtiofsd(p, &cx.host.devices).is_some())
}

/// [`can_share`] on `os`, `found` telling whether the provider finds a virtiofsd (asked only of
/// the providers that need one).
fn share_on(provider: Provider, os: Os, found: impl FnOnce(isothrum_runner::Provider) -> bool) -> crate::error::Result<()> {
    if let Some(vagrant) = provider.vagrant()
        && isothrum_machine::providers::needs_virtiofsd(vagrant, os.isothrum())
        && !found(vagrant)
    {
        return Err(crate::error::Error::coded(crate::error::Code::VirtiofsdMissing { hypervisor: provider.label().to_owned() }));
    }
    Ok(())
}

/// The machine a test's detection reads: its tools scripted (the runner's fake), its devices
/// under a folder of the test's.
#[cfg(test)]
pub(crate) mod testing {
    use std::cell::RefCell;
    use std::sync::Arc;

    use isothrum_runner::ProcessRunner;
    use isothrum_runner::fake::{FakeProcesses, Reply};

    thread_local! {
        static MACHINE: RefCell<Option<(Arc<dyn ProcessRunner>, isothrum_machine::Host)>> = const { RefCell::new(None) };
    }

    /// The machine detection reads on this test's thread, until dropped.
    pub(crate) struct Machine;

    impl Machine {
        /// `runner` runs the tools, `host` is the machine (its devices a test's own).
        pub(crate) fn set(runner: Arc<dyn ProcessRunner>, host: isothrum_machine::Host) -> Machine {
            MACHINE.with(|m| *m.borrow_mut() = Some((runner, host)));
            Machine
        }
    }

    impl Drop for Machine {
        fn drop(&mut self) {
            MACHINE.with(|m| *m.borrow_mut() = None);
        }
    }

    /// This OS and CPU, its devices under `root` (none there until a test makes them).
    pub(crate) fn host(root: &std::path::Path) -> isothrum_machine::Host {
        isothrum_machine::Host {
            os: crate::platform::host::Os::HERE.isothrum(),
            arch: crate::platform::host::Arch::HERE.isothrum(),
            packages: None,
            devices: isothrum_machine::Devices::under(root),
        }
    }

    /// The test's machine, else one where every tool is missing and no device is there.
    pub(super) fn context() -> isothrum_machine::Context {
        let (runner, host) = MACHINE.with(|m| m.borrow().clone()).unwrap_or_else(|| {
            let nothing: Arc<dyn ProcessRunner> = Arc::new(FakeProcesses::model(|_| Reply::missing()));
            (nothing, host(std::path::Path::new("/nonexistent/cyberctf-test-machine")))
        });
        isothrum_machine::Context::new(runner, host)
    }
}

#[cfg(test)]
mod tests;
