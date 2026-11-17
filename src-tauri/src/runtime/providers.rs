//! The hypervisors, servers and cloud accounts labs run on ([`Provider`]), and which local
//! hypervisors this machine can run a lab's VMs on. Detection is isothrum-machine's
//! ([`isothrum_machine::providers`]): what is installed, what Vagrant still needs, and why a
//! hypervisor can't run VMs yet (an [`Unavailable`] code, which the webview says as
//! `machine.unavailable.*`). The launcher keeps its places (Proxmox and the clouds beside the
//! Vagrant providers), the player's preferred hypervisor ([`pick`]), and the player's words for a
//! reason ([`said`]).

mod provider;

#[cfg(test)]
pub use isothrum_machine::providers::UtilityState;
pub use isothrum_machine::providers::{ProviderStatus, Unavailable};
pub use provider::Provider;

use isothrum_machine::HostOs;

/// What isothrum-machine detects with: this machine's ([`this_machine`]). In a test, the test's
/// own when it set one (`testing::Machine`), this machine's in a real-lab scenario
/// (`testing::read_this_machine`), else every tool missing: a unit test never reads the machine
/// running it.
pub(crate) fn machine() -> isothrum_machine::Context {
    #[cfg(test)]
    {
        testing::context()
    }
    #[cfg(not(test))]
    {
        this_machine()
    }
}

/// What the machine check reads with ([`crate::machine::system::system_check`]): [`machine`]'s,
/// and outside the tests the version probes that rarely change (`vagrant --version`...) kept a
/// while ([`crate::platform::remembered::Remembering`]). Its host has no package manager:
/// [`machine_setup`]'s has.
pub(crate) fn machine_check() -> isothrum_machine::Context {
    #[cfg(test)]
    {
        machine()
    }
    #[cfg(not(test))]
    {
        let mut cx = machine();
        cx.runner = std::sync::Arc::new(crate::platform::remembered::Remembering::new(cx.runner));
        cx
    }
}

/// [`machine_check`]'s, with this machine's package manager, looked up through its runner
/// (isothrum-machine's [`Host::here`](isothrum_machine::Host::here)): what the machine check
/// reports and the guided setup installs through.
pub(crate) async fn machine_setup() -> isothrum_machine::Context {
    let mut cx = machine_check();
    cx.host.packages = isothrum_machine::Host::here(cx.runner.as_ref()).await.packages;
    cx
}

/// This machine as the guided setup reads it: [`machine_setup`]'s host.
pub(crate) async fn host() -> isothrum_machine::Host {
    machine_setup().await.host
}

/// What the self-tests run with: [`machine`]'s.
pub(crate) fn machine_selftest() -> isothrum_machine::Context {
    machine()
}

/// What the cloud CLIs are asked with ([`isothrum_machine::cloud`]): [`machine`]'s.
pub(crate) fn machine_cloud() -> isothrum_machine::Context {
    machine()
}

/// This machine: its tools, run by the app's processes ([`crate::platform::processes`]), and its
/// devices.
fn this_machine() -> isothrum_machine::Context {
    let os = isothrum_runner::this_host();
    let host = isothrum_machine::Host {
        os,
        arch: isothrum_runner::this_arch(),
        // Detection doesn't read it; the setup's own installs look it up.
        packages: None,
        devices: isothrum_machine::Devices::here(os),
    };
    isothrum_machine::Context::new(crate::platform::processes::system(), host)
}

/// The local hypervisors ready for a lab's VMs on this machine: Vagrant, the hypervisor and its
/// plugin (what the Machine page reports as ready), in [`Provider::ALL`]'s order.
pub async fn ready_here() -> Vec<Provider> {
    let cx = machine();
    if !isothrum_machine::tools::version(&cx, isothrum_machine::tools::probes::VAGRANT).await.installed {
        return Vec::new();
    }
    ready(&isothrum_machine::providers::detect(&cx, true).await, true)
}

/// Of `statuses` ([`detect`]'s), the local hypervisors ready for a lab's VMs: none without
/// Vagrant (`vagrant_installed`). The machine report carries them, so the webview never works
/// this out again.
pub fn ready(statuses: &[ProviderStatus], vagrant_installed: bool) -> Vec<Provider> {
    isothrum_machine::check::ready(statuses, vagrant_installed).into_iter().map(Provider::from).collect()
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

/// The group a player joins to open `/dev/kvm` on this machine: the one it belongs to, else
/// `kvm` (the setup's shapes name it).
#[cfg(test)]
pub fn kvm_group() -> String {
    isothrum_machine::providers::kvm_group(&machine().host)
}

/// UTM's command line tool, which ships inside its app bundle, on no PATH: in the bundle the
/// machine check finds (the app doesn't move while it runs). The integration tests read UTM's VMs
/// with it; the app itself never runs it, as it would open UTM with its window in front.
#[cfg(test)]
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
    share_on(provider, isothrum_runner::this_host(), |p| isothrum_machine::providers::virtiofsd(p, &cx.host.devices).is_some())
}

/// [`can_share`] on `os`, `found` telling whether the provider finds a virtiofsd (asked only of
/// the providers that need one).
fn share_on(provider: Provider, os: HostOs, found: impl FnOnce(isothrum_runner::Provider) -> bool) -> crate::error::Result<()> {
    if let Some(vagrant) = provider.vagrant()
        && isothrum_machine::providers::needs_virtiofsd(vagrant, os)
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
    use std::sync::atomic::{AtomicBool, Ordering};

    use isothrum_runner::ProcessRunner;
    use isothrum_runner::fake::{FakeProcesses, Reply};

    thread_local! {
        static MACHINE: RefCell<Option<(Arc<dyn ProcessRunner>, isothrum_machine::Host)>> = const { RefCell::new(None) };
    }

    /// Whether this process's detection reads this machine ([`read_this_machine`]).
    static THIS_MACHINE: AtomicBool = AtomicBool::new(false);

    /// Detection reads this machine from now on, on every thread of this process, where no test
    /// set its own ([`Machine`]): a real-lab scenario's (`integration_tests`), which runs alone
    /// in its process on the real tools. Without it, a scenario's lab was checked against a
    /// machine with nothing installed: the attack box's shared folder on libvirt was refused for
    /// want of the virtiofsd this machine has.
    pub(crate) fn read_this_machine() {
        THIS_MACHINE.store(true, Ordering::Relaxed);
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
            os: isothrum_runner::this_host(),
            arch: isothrum_runner::this_arch(),
            packages: None,
            devices: isothrum_machine::Devices::under(root),
        }
    }

    /// A computer on `os` and `arch` going through `packages`, its devices nowhere: a rule's
    /// test host.
    pub(crate) fn host_on(os: isothrum_machine::HostOs, arch: isothrum_machine::Arch, packages: Option<isothrum_machine::Packages>) -> isothrum_machine::Host {
        isothrum_machine::Host { os, arch, packages, devices: isothrum_machine::Devices::under(std::path::Path::new("/nonexistent/cyberctf-test-machine")) }
    }

    /// The test's machine, else this one in a real-lab scenario ([`read_this_machine`]), else one
    /// where every tool is missing and no device is there.
    pub(super) fn context() -> isothrum_machine::Context {
        let set = MACHINE.with(|m| m.borrow().clone());
        if set.is_none() && THIS_MACHINE.load(Ordering::Relaxed) {
            return super::this_machine();
        }
        let (runner, host) = set.unwrap_or_else(|| {
            // A test scripting the launcher's tools answers isothrum-machine's calls too.
            let nothing: Arc<dyn ProcessRunner> = if crate::testing::tools::scripted() {
                Arc::new(crate::testing::tools::AsRunner)
            } else {
                Arc::new(FakeProcesses::model(|_| Reply::missing()))
            };
            (nothing, host(std::path::Path::new("/nonexistent/cyberctf-test-machine")))
        });
        isothrum_machine::Context::new(runner, host)
    }
}

#[cfg(test)]
mod tests;
