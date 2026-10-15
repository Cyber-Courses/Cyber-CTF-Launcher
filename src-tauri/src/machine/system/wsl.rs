//! Whether WSL 2 is ready on this Windows machine, and the restart that finishes turning it on.
//!
//! No single signal can be trusted: `wsl.exe` ships with Windows whether WSL is set up or not,
//! `wsl --version` only exists in the Store WSL, `wsl --status` may fail on a machine without a
//! distribution, and a feature turned on reads as on before the restart that really starts it.
//! So the check gathers several, none of which needs admin rights or reads localized text, and
//! [`classify`] (pure, tested on every OS) weighs them:
//! - `wsl --status` and `wsl --version`, by exit code only;
//! - the WSL services in the registry: `LxssManager` (the WSL feature) and `WslService` (the
//!   Store WSL);
//! - the optional features `VirtualMachinePlatform` and `Microsoft-Windows-Subsystem-Linux`
//!   (WMI `Win32_OptionalFeature`, readable without admin rights), and whether a hypervisor
//!   runs (`Win32_ComputerSystem.HypervisorPresent`: the Virtual Machine Platform only starts
//!   one after the restart);
//! - the restart Windows' servicing waits for (`Component Based Servicing\RebootPending`), and
//!   pending file renames, a weak sign (many installers, drivers and antivirus leave some at
//!   every boot), only heeded right after Cyber CTF's own install;
//! - whether Cyber CTF's own "Turn on WSL 2" ran since this boot, and whether Cyber CTF already
//!   restarted Windows for WSL (then it never asks for another restart).
//!
//! Every probe is headless and timed, its whole process tree killed on a timeout: a hung
//! `wsl.exe` costs the check a few seconds, never the setup.

#![cfg_attr(not(windows), allow(dead_code, reason = "off Windows only the UI's types and the tests use most of this"))]

use std::sync::atomic::{AtomicU8, Ordering};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::platform::AppDirs;

/// Where WSL 2 stands on this machine.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum WslState {
    /// On and running: Docker Desktop can use it.
    Ready,
    /// Turned on (or being turned on), but Windows has to restart to finish.
    NeedsReboot,
    /// Not set up.
    Missing,
    /// The signals don't agree, or couldn't be read.
    Unknown,
}

/// The WSL part of the system check (Windows only).
#[derive(Serialize, Clone, Debug, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct WslReport {
    pub state: WslState,
    /// Whether a restart from Cyber CTF opens it again once the player signs back in.
    pub reopens_after_restart: bool,
    /// The Virtual Machine Platform is on but no hypervisor runs, a restart having (likely)
    /// passed: virtualization is probably off in the firmware (BIOS/UEFI).
    pub virtualization_off: bool,
}

/// How a probe command went.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Run {
    /// Exited 0.
    Ok,
    /// Exited with another code (or couldn't be started for another reason than missing).
    Failed,
    /// Ran out of time and was killed.
    TimedOut,
    /// Not on this machine.
    Missing,
}

/// An optional feature's `Win32_OptionalFeature.InstallState`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feature {
    /// 1
    Enabled,
    /// 2
    Disabled,
    /// 3: not available on this edition, or not listed at all.
    Absent,
}

/// Every signal the check read. `None` is "couldn't tell" (unreadable, or the probe failed).
#[derive(Clone, Copy, Debug)]
pub struct Probes {
    /// `wsl.exe --status`.
    pub status: Run,
    /// `wsl.exe --version`: only the Store WSL knows this option.
    pub version: Run,
    /// The WSL feature's service (`LxssManager`) is registered.
    pub lxss_service: Option<bool>,
    /// The Store WSL's service (`WslService`) is registered.
    pub wsl_service: Option<bool>,
    pub vm_platform: Option<Feature>,
    pub wsl_feature: Option<Feature>,
    /// A hypervisor runs (what the Virtual Machine Platform starts at boot).
    pub hypervisor: Option<bool>,
    /// Windows' servicing waits for a restart.
    pub reboot_pending: Option<bool>,
    /// Files wait to be replaced at the next restart (weak: many installers leave some).
    pub renames_pending: Option<bool>,
    /// Cyber CTF's "Turn on WSL 2" ran since this boot.
    pub installed_this_boot: bool,
    /// Cyber CTF restarted Windows for WSL in an earlier boot, with no install since.
    pub restarted_for_wsl: bool,
}

/// Where WSL 2 stands, from what the probes found.
pub fn classify(p: &Probes) -> WslState {
    // WSL itself: the Store package, or the Windows feature.
    let store = p.version == Run::Ok || p.wsl_service == Some(true);
    let inbox = p.lxss_service == Some(true) || p.wsl_feature == Some(Feature::Enabled);
    let wsl = store || inbox;
    // WSL 2 runs its VM on the Virtual Machine Platform.
    let platform_on = p.vm_platform == Some(Feature::Enabled);
    let platform_off = matches!(p.vm_platform, Some(Feature::Disabled | Feature::Absent));
    let running = p.hypervisor == Some(true);
    let not_running = p.hypervisor == Some(false);
    let pending = p.reboot_pending == Some(true);

    // Our own install ran this boot and Windows waits for a restart: whatever the features read
    // meanwhile (they can read as on already), they only start after it. Renames count here
    // only: right after our install they are likely its own.
    if p.installed_this_boot && (pending || p.renames_pending == Some(true)) {
        return WslState::NeedsReboot;
    }
    if wsl && platform_on && running {
        // The platform is on and its hypervisor runs. A restart Windows waits for may be an
        // update's: then WSL has to answer too, or (without a distribution `wsl --status`
        // fails) be the Store WSL, which only installs once the platform is set up.
        if !pending || p.status == Run::Ok || (store && !p.installed_this_boot) {
            return WslState::Ready;
        }
    }
    // WSL answers and nothing waits for a restart; the features couldn't be read.
    if wsl && p.status == Run::Ok && p.reboot_pending == Some(false) && p.vm_platform.is_none() && !not_running {
        return WslState::Ready;
    }
    if p.installed_this_boot {
        return WslState::NeedsReboot;
    }
    // A restart is pending and WSL is partly there: that restart finishes it. Never a second
    // time once Cyber CTF restarted for it: what is left isn't a restart's to fix.
    if (wsl || platform_on) && pending && !p.restarted_for_wsl {
        return WslState::NeedsReboot;
    }
    let wsl_failed = matches!(p.status, Run::Failed | Run::Missing);
    let no_services = p.lxss_service == Some(false) && p.wsl_service == Some(false);
    // Nothing of WSL, as both the services and wsl.exe say; or WSL 1 only, without the platform.
    if (!wsl && !platform_on && wsl_failed && (no_services || platform_off)) || (wsl && platform_off && !pending) {
        return WslState::Missing;
    }
    // Among the rest: on, yet no hypervisor (virtualization off in the firmware, most likely).
    WslState::Unknown
}

/// Whether to tell the player virtualization looks off in the firmware: the platform is on, no
/// hypervisor runs, and no restart is still to come.
pub fn virtualization_off(p: &Probes, state: WslState) -> bool {
    state == WslState::Unknown && p.vm_platform == Some(Feature::Enabled) && p.hypervisor == Some(false)
}

/// The features and the hypervisor, from the output of `windows::CIM_SCRIPT`: `features=ok` once
/// the feature query answered, then `feature:<name>=<InstallState>` for each feature it lists
/// (an unlisted one is absent), and `hypervisor=True|False`. Partial output reads as far as it
/// goes.
pub fn parse_cim(out: &str) -> (Option<Feature>, Option<Feature>, Option<bool>) {
    let lines: Vec<&str> = out.lines().map(str::trim).collect();
    let answered = lines.contains(&"features=ok");
    let feature = |name: &str| {
        if !answered {
            return None;
        }
        let prefix = format!("feature:{name}=");
        match lines.iter().find_map(|l| l.strip_prefix(prefix.as_str())) {
            None | Some("3") => Some(Feature::Absent),
            Some("1") => Some(Feature::Enabled),
            Some("2") => Some(Feature::Disabled),
            Some(_) => None,
        }
    };
    let hypervisor = lines.iter().find_map(|l| l.strip_prefix("hypervisor=")).and_then(|v| match v {
        "True" => Some(true),
        "False" => Some(false),
        _ => None,
    });
    (feature("VirtualMachinePlatform"), feature("Microsoft-Windows-Subsystem-Linux"), hypervisor)
}

/// How a timed probe went, and its stdout.
fn outcome(result: &Result<String>) -> (Run, &str) {
    match result {
        Ok(out) => (Run::Ok, out.as_str()),
        Err(Error::ToolMissing { .. }) => (Run::Missing, ""),
        Err(e) if crate::platform::exec::timed_out(e) => (Run::TimedOut, ""),
        Err(_) => (Run::Failed, ""),
    }
}

// ---------- What Cyber CTF did, by boot ----------

/// The app data file holding the boot time at which "Turn on WSL 2" last ran.
const INSTALL_MARK: &str = "wsl-install-boot";
/// The app data file holding the boot time in which Cyber CTF restarted Windows for WSL.
const RESTART_MARK: &str = "wsl-restart-boot";

/// Boot times read a second or so apart within one boot (they're derived from the uptime);
/// two boots are minutes apart.
fn same_boot(a: u64, b: u64) -> bool {
    a.abs_diff(b) <= 30
}

fn mark(dirs: &AppDirs, name: &str, boot: u64) {
    let Some(f) = dirs.data_file(name) else { return };
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(f, boot.to_string());
}

fn marked_boot(dirs: &AppDirs, name: &str) -> Option<u64> {
    std::fs::read_to_string(dirs.data_file(name)?).ok()?.trim().parse().ok()
}

fn unmark(dirs: &AppDirs, name: &str) {
    if let Some(f) = dirs.data_file(name) {
        let _ = std::fs::remove_file(f);
    }
}

/// Remembers that "Turn on WSL 2" ran in this boot, so the check asks for the restart even
/// after the app was closed and opened again before it. A new install starts over: the next
/// restart is one to ask for again.
pub fn mark_installed(dirs: &AppDirs) {
    mark(dirs, INSTALL_MARK, sysinfo::System::boot_time());
    unmark(dirs, RESTART_MARK);
}

/// Whether "Turn on WSL 2" ran in boot `boot` (a mark from an earlier boot is removed).
fn installed_this_boot(dirs: &AppDirs, boot: u64) -> bool {
    match marked_boot(dirs, INSTALL_MARK) {
        Some(marked) if same_boot(marked, boot) => true,
        Some(_) => {
            unmark(dirs, INSTALL_MARK);
            false
        }
        None => false,
    }
}

/// Whether Cyber CTF restarted Windows for WSL in a boot before `boot`.
fn restarted_for_wsl(dirs: &AppDirs, boot: u64) -> bool {
    marked_boot(dirs, RESTART_MARK).is_some_and(|marked| !same_boot(marked, boot))
}

/// What the check makes of `probes`, and what it remembers: once WSL 2 is ready, the restart
/// done for it is forgotten.
fn assess(dirs: &AppDirs, probes: &Probes) -> (WslState, bool) {
    let state = classify(probes);
    if state == WslState::Ready {
        unmark(dirs, RESTART_MARK);
    }
    (state, virtualization_off(probes, state))
}

/// Where WSL 2 stands on this machine: Windows only (`None` elsewhere).
#[cfg_attr(not(windows), allow(clippy::unused_async, reason = "it awaits the probes on Windows"))]
pub async fn report(dirs: &AppDirs) -> Option<WslReport> {
    #[cfg(windows)]
    {
        let boot = sysinfo::System::boot_time();
        let probes = windows::probe(installed_this_boot(dirs, boot), restarted_for_wsl(dirs, boot)).await;
        let (state, virtualization_off) = assess(dirs, &probes);
        log::info!("WSL check: {probes:?} -> {state:?}");
        Some(WslReport { state, reopens_after_restart: windows::reopen_command().is_some(), virtualization_off })
    }
    #[cfg(not(windows))]
    {
        let _ = dirs;
        None
    }
}

// ---------- Restart ----------

/// The note Windows shows while it counts down, when the UI sends none.
const DEFAULT_REASON: &str = "Cyber CTF: restarting to finish turning on WSL 2.";

/// The restart note: one line, at most 200 characters (shutdown.exe takes 512).
fn reason(text: &str) -> Result<String> {
    let text = text.trim();
    if text.chars().count() > 200 || text.chars().any(char::is_control) {
        return Err(Error::Invalid("The restart note must be one line of at most 200 characters.".into()));
    }
    Ok(if text.is_empty() { DEFAULT_REASON.to_string() } else { text.to_string() })
}

/// No restart asked for in this run of the app.
const IDLE: u8 = 0;
/// One being asked for right now.
const ASKING: u8 = 1;
/// Windows counts down to it.
const SCHEDULED: u8 = 2;

/// The restart of this run of the app: each window has its own WSL step (and the onboarding
/// remounts it), but Windows restarts once and `RunOnce` is written once.
static RESTART: AtomicU8 = AtomicU8::new(IDLE);

/// Opening the app once at the next sign-in (shared across the command's await: `Sync`).
trait Reopen {
    /// Registers it; whether it was written.
    fn register(&self) -> bool;
    fn unregister(&self);
}

/// `shutdown.exe` refusing because one is already scheduled (`ERROR_SHUTDOWN_IS_SCHEDULED`, 1190:
/// its message ends with the code in every language).
fn already_scheduled(e: &Error) -> bool {
    matches!(e, Error::CommandFailed { stderr, .. } if stderr.contains("1190"))
}

/// Asks Windows to restart in a few seconds, once per run of the app: a second ask while it
/// counts down (another window, a remounted step) is the same restart. `RunOnce` is removed
/// again only if this call wrote it and the restart couldn't be scheduled.
async fn schedule_restart(dirs: &AppDirs, state: &AtomicU8, reason: &str, reopen: &(dyn Reopen + Sync)) -> Result<()> {
    if state.compare_exchange(IDLE, ASKING, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return Ok(());
    }
    let registered = reopen.register();
    let boot = sysinfo::System::boot_time();
    mark(dirs, RESTART_MARK, boot);
    match crate::platform::exec::run("shutdown", &["/r", "/t", "5", "/c", reason], None).await {
        Ok(_) => {
            state.store(SCHEDULED, Ordering::SeqCst);
            Ok(())
        }
        // Another restart is on its way (from outside the app): this one is it.
        Err(e) if already_scheduled(&e) => {
            state.store(SCHEDULED, Ordering::SeqCst);
            Ok(())
        }
        Err(e) => {
            if registered {
                reopen.unregister();
            }
            unmark(dirs, RESTART_MARK);
            state.store(IDLE, Ordering::SeqCst);
            Err(e)
        }
    }
}

/// Restarts Windows in a few seconds (`shutdown /r`), to finish turning WSL 2 on. Before that,
/// asks Windows to open Cyber CTF once when the player signs back in (`RunOnce`). Windows only.
#[cfg_attr(not(windows), allow(clippy::unused_async, reason = "it awaits the restart on Windows"))]
pub async fn machine_restart(dirs: AppDirs, reason: String) -> Result<()> {
    let reason = self::reason(&reason)?;
    #[cfg(windows)]
    {
        // A unit test must never restart the machine it runs on: only scripted tools here.
        #[cfg(test)]
        if !crate::platform::exec::fake::scripted() {
            return Err(Error::Invalid("a test restarted without scripting shutdown".into()));
        }
        let reopen = windows::UserRunOnce(if cfg!(test) { None } else { windows::reopen_command() });
        schedule_restart(&dirs, &RESTART, &reason, &reopen).await
    }
    #[cfg(not(windows))]
    {
        let _ = (dirs, reason);
        Err(Error::Invalid("Restarting from Cyber CTF is only available on Windows.".into()))
    }
}

/// The command that opens this app again, for `RunOnce`: the executable itself, or for the
/// Store (MSIX) build, whose executable can't be started by path, its app through the shell.
/// (The Store build writes the real `RunOnce`: its manifest turns registry virtualization off.)
fn reopen_line(exe: &str) -> Option<String> {
    if exe.is_empty() || exe.contains('"') {
        return None;
    }
    let parts: Vec<&str> = exe.split(['\\', '/']).collect();
    if let Some(i) = parts.iter().position(|p| p.eq_ignore_ascii_case("WindowsApps")) {
        // `<Name>_<Version>_<Arch>_<ResourceId>_<PublisherId>`: the family is `<Name>_<PublisherId>`.
        let folder: Vec<&str> = parts.get(i + 1)?.split('_').collect();
        let (name, publisher) = (folder.first()?, folder.last()?);
        let plain = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        if folder.len() < 5 || !plain(name) || !plain(publisher) {
            return None;
        }
        return Some(format!(r"explorer.exe shell:AppsFolder\{name}_{publisher}!CyberCTF"));
    }
    Some(format!("\"{exe}\""))
}

#[cfg(windows)]
mod windows {
    use std::time::Duration;

    use base64::Engine;

    use crate::platform::sys::{Hive, delete_user_registry_value, registry_key_exists, registry_value_exists, set_user_registry_string};

    use super::{Probes, outcome, parse_cim};

    const WSL_TIMEOUT: Duration = Duration::from_secs(10);
    const CIM_TIMEOUT: Duration = Duration::from_secs(20);

    /// The features and the hypervisor, through WMI (no admin rights needed), printed as
    /// language-neutral `key=value` lines (see [`parse_cim`]). Each query fails on its own, and
    /// the script exits 0 whatever happened, so what it printed is always read.
    pub const CIM_SCRIPT: &str = "$ProgressPreference = 'SilentlyContinue'
try {
  $f = @(Get-CimInstance -ClassName Win32_OptionalFeature -Filter \"Name='VirtualMachinePlatform' OR Name='Microsoft-Windows-Subsystem-Linux'\" -ErrorAction Stop)
  'features=ok'
  foreach ($x in $f) { 'feature:' + $x.Name + '=' + [int]$x.InstallState }
} catch {}
try { 'hypervisor=' + [bool](Get-CimInstance -ClassName Win32_ComputerSystem -ErrorAction Stop).HypervisorPresent } catch {}
exit 0
";

    /// Every signal, the commands side by side. They run like every timed tool (`exec`):
    /// headless, and on a timeout killed with whatever they started (the Store `wsl.exe` starts
    /// a child of its own).
    pub async fn probe(installed_this_boot: bool, restarted_for_wsl: bool) -> Probes {
        let encoded = encode(CIM_SCRIPT);
        let cim_args = ["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded.as_str()];
        // wsl.exe writes UTF-16 unless told otherwise; only exit codes are read, but its output
        // in a log stays readable.
        let utf8 = [("WSL_UTF8".to_string(), "1".to_string())];
        let status = crate::platform::exec::run_env_timed("wsl", &["--status"], None, &utf8, WSL_TIMEOUT);
        let version = crate::platform::exec::run_env_timed("wsl", &["--version"], None, &utf8, WSL_TIMEOUT);
        let cim = crate::platform::exec::run_env_timed("powershell", &cim_args, None, &[], CIM_TIMEOUT);
        let (status, version, cim) = tokio::join!(status, version, cim);
        let (vm_platform, wsl_feature, hypervisor) = parse_cim(outcome(&cim).1);
        let services = r"SYSTEM\CurrentControlSet\Services";
        Probes {
            status: outcome(&status).0,
            version: outcome(&version).0,
            lxss_service: registry_key_exists(Hive::LocalMachine, &format!(r"{services}\LxssManager")),
            wsl_service: registry_key_exists(Hive::LocalMachine, &format!(r"{services}\WslService")),
            vm_platform,
            wsl_feature,
            hypervisor,
            reboot_pending: registry_key_exists(Hive::LocalMachine, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending"),
            renames_pending: registry_value_exists(Hive::LocalMachine, r"SYSTEM\CurrentControlSet\Control\Session Manager", "PendingFileRenameOperations"),
            installed_this_boot,
            restarted_for_wsl,
        }
    }

    /// A script for `powershell -EncodedCommand`: base64 of its UTF-16LE text (no quoting to get
    /// wrong on the command line).
    fn encode(script: &str) -> String {
        let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    const RUN_ONCE: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";
    const RUN_ONCE_NAME: &str = "CyberCTF";

    /// The command line that opens this app again, when there is one.
    pub fn reopen_command() -> Option<String> {
        super::reopen_line(std::env::current_exe().ok()?.to_str()?)
    }

    /// The player's own `RunOnce` (no admin rights), opening `.0` at the next sign-in.
    pub struct UserRunOnce(pub Option<String>);

    impl super::Reopen for UserRunOnce {
        fn register(&self) -> bool {
            self.0.as_deref().is_some_and(|line| set_run_once(RUN_ONCE_NAME, line))
        }

        fn unregister(&self) {
            delete_run_once(RUN_ONCE_NAME);
        }
    }

    pub(super) fn set_run_once(name: &str, line: &str) -> bool {
        set_user_registry_string(RUN_ONCE, name, line)
    }

    pub(super) fn delete_run_once(name: &str) {
        delete_user_registry_value(RUN_ONCE, name);
    }

    #[cfg(test)]
    pub(super) fn run_once_exists(name: &str) -> Option<bool> {
        registry_value_exists(Hive::CurrentUser, RUN_ONCE, name)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU32};

    use super::*;
    use crate::platform::exec::fake::{Tools, fail, ok};
    use crate::testing::test_app::TestApp;
    use WslState::*;

    /// A machine where nothing could be read.
    fn blank() -> Probes {
        Probes {
            status: Run::Failed,
            version: Run::Failed,
            lxss_service: None,
            wsl_service: None,
            vm_platform: None,
            wsl_feature: None,
            hypervisor: None,
            reboot_pending: None,
            renames_pending: None,
            installed_this_boot: false,
            restarted_for_wsl: false,
        }
    }

    /// Windows 11 without WSL: wsl.exe is there, and only offers to install.
    fn win11_without_wsl() -> Probes {
        Probes {
            lxss_service: Some(false),
            wsl_service: Some(false),
            vm_platform: Some(Feature::Disabled),
            wsl_feature: Some(Feature::Disabled),
            hypervisor: Some(false),
            reboot_pending: Some(false),
            renames_pending: Some(false),
            ..blank()
        }
    }

    /// Windows 11 with the Store WSL and no distribution, after the restart.
    fn win11_store_ready() -> Probes {
        Probes {
            status: Run::Ok,
            version: Run::Ok,
            wsl_service: Some(true),
            vm_platform: Some(Feature::Enabled),
            hypervisor: Some(true),
            ..win11_without_wsl()
        }
    }

    /// Windows 10 21H2 with the WSL feature (no Store WSL: `--version` is unknown to it).
    fn win10_inbox_ready() -> Probes {
        Probes {
            status: Run::Ok,
            version: Run::Failed,
            lxss_service: Some(true),
            wsl_service: Some(false),
            vm_platform: Some(Feature::Enabled),
            wsl_feature: Some(Feature::Enabled),
            hypervisor: Some(true),
            reboot_pending: Some(false),
            renames_pending: Some(false),
            installed_this_boot: false,
            restarted_for_wsl: false,
        }
    }

    /// The Store WSL turned on, virtualization off in the firmware: the platform reads as on,
    /// no hypervisor ever starts, and something re-adds file renames at every boot.
    fn firmware_off() -> Probes {
        Probes { status: Run::Failed, hypervisor: Some(false), renames_pending: Some(true), ..win11_store_ready() }
    }

    #[test]
    fn a_machine_without_wsl_is_missing() {
        assert_eq!(classify(&win11_without_wsl()), Missing);
        // Windows 10, whose wsl.exe may not exist at all.
        assert_eq!(classify(&Probes { status: Run::Missing, version: Run::Missing, ..win11_without_wsl() }), Missing);
        // The features couldn't be read: the services and wsl.exe still agree.
        assert_eq!(classify(&Probes { vm_platform: None, wsl_feature: None, hypervisor: None, ..win11_without_wsl() }), Missing);
        // A restart for an update, or renames, are no sign of WSL.
        assert_eq!(classify(&Probes { reboot_pending: Some(true), renames_pending: Some(true), ..win11_without_wsl() }), Missing);
    }

    #[test]
    fn wsl_on_and_running_is_ready() {
        assert_eq!(classify(&win11_store_ready()), Ready);
        assert_eq!(classify(&win10_inbox_ready()), Ready);
        // Without a distribution `wsl --status` may fail: the platform running is enough.
        assert_eq!(classify(&Probes { status: Run::Failed, ..win11_store_ready() }), Ready);
        // A hung wsl.exe doesn't hide the rest.
        assert_eq!(classify(&Probes { status: Run::TimedOut, version: Run::TimedOut, ..win11_store_ready() }), Ready);
        // WMI unavailable: WSL answering, with nothing waiting for a restart, is enough.
        assert_eq!(classify(&Probes { vm_platform: None, wsl_feature: None, hypervisor: None, ..win11_store_ready() }), Ready);
        // An update waiting for a restart, while WSL answers.
        assert_eq!(classify(&Probes { reboot_pending: Some(true), renames_pending: Some(true), ..win11_store_ready() }), Ready);
        // Renames left by others never hold a working WSL back.
        assert_eq!(classify(&Probes { renames_pending: Some(true), ..win10_inbox_ready() }), Ready);
    }

    #[test]
    fn a_pending_update_doesnt_hide_a_store_wsl_without_a_distribution() {
        // The usual state after `wsl --install --no-distribution` and its restart: `--status`
        // fails (no distribution), and Windows Update waits for a restart of its own.
        let store = Probes { status: Run::Failed, reboot_pending: Some(true), ..win11_store_ready() };
        assert_eq!(classify(&store), Ready);
        // Known by its service alone (`--version` timed out).
        assert_eq!(classify(&Probes { version: Run::TimedOut, ..store }), Ready);
        // The WSL feature (no Store WSL) gives no such proof: the restart may be its own.
        let inbox = Probes { status: Run::Failed, reboot_pending: Some(true), ..win10_inbox_ready() };
        assert_eq!(classify(&inbox), NeedsReboot);
    }

    #[test]
    fn unreadable_servicing_is_no_proof_nothing_is_pending() {
        let unread = Probes { vm_platform: None, wsl_feature: None, hypervisor: None, reboot_pending: None, ..win11_store_ready() };
        assert_eq!(classify(&unread), Unknown);
        assert_eq!(classify(&Probes { reboot_pending: Some(false), ..unread }), Ready);
    }

    #[test]
    fn turned_on_but_not_restarted_needs_a_reboot() {
        // Right after Cyber CTF's install: the Store WSL is in, the platform reads as on, no
        // hypervisor yet.
        let after_install = Probes {
            status: Run::Failed,
            version: Run::Ok,
            wsl_service: Some(true),
            vm_platform: Some(Feature::Enabled),
            hypervisor: Some(false),
            reboot_pending: Some(true),
            installed_this_boot: true,
            ..win11_without_wsl()
        };
        assert_eq!(classify(&after_install), NeedsReboot);
        // Hyper-V already ran a hypervisor, and WSL answers: still the restart first.
        assert_eq!(classify(&Probes { hypervisor: Some(true), status: Run::Ok, ..after_install }), NeedsReboot);
        // Servicing says nothing, but our install left renames.
        assert_eq!(classify(&Probes { hypervisor: Some(true), reboot_pending: Some(false), renames_pending: Some(true), ..after_install }), NeedsReboot);
        // The app was reopened before the restart, and WMI still reads the features as off.
        let reopened = Probes {
            vm_platform: Some(Feature::Disabled),
            wsl_feature: Some(Feature::Disabled),
            wsl_service: Some(false),
            version: Run::Failed,
            ..after_install
        };
        assert_eq!(classify(&reopened), NeedsReboot);
        // Our install ran this boot and the servicing state can't be read.
        assert_eq!(classify(&Probes { reboot_pending: None, ..reopened }), NeedsReboot);
        // Turned on by hand (`wsl --install` in PowerShell) on Windows 10: no mark of ours.
        let by_hand = Probes {
            status: Run::Failed,
            vm_platform: Some(Feature::Enabled),
            wsl_feature: Some(Feature::Enabled),
            lxss_service: Some(false),
            hypervisor: Some(false),
            reboot_pending: Some(true),
            ..win11_without_wsl()
        };
        assert_eq!(classify(&by_hand), NeedsReboot);
    }

    #[test]
    fn our_install_that_changed_nothing_is_judged_on_the_rest() {
        // It ran this boot, yet no restart or rename is pending: WSL was ready already.
        assert_eq!(classify(&Probes { installed_this_boot: true, ..win11_store_ready() }), Ready);
    }

    #[test]
    fn virtualization_off_in_the_firmware_never_loops_on_restarts() {
        // Renames re-added at every boot are no reason to restart (again).
        let off = firmware_off();
        assert_eq!(classify(&off), Unknown);
        assert!(virtualization_off(&off, classify(&off)));
        // After Cyber CTF's restart, even with servicing waiting for another one: no second ask.
        let after_ours = Probes { restarted_for_wsl: true, reboot_pending: Some(true), ..off };
        assert_eq!(classify(&after_ours), Unknown);
        assert!(virtualization_off(&after_ours, classify(&after_ours)));
        // The same pending restart without one of ours yet: that one is worth trying, once.
        let before_ours = Probes { reboot_pending: Some(true), ..off };
        assert_eq!(classify(&before_ours), NeedsReboot);
        assert!(!virtualization_off(&before_ours, NeedsReboot));
        // A new install of ours starts over.
        assert_eq!(classify(&Probes { installed_this_boot: true, ..after_ours }), NeedsReboot);
        // A working machine is never told virtualization is off.
        assert!(!virtualization_off(&win11_store_ready(), Ready));
    }

    #[test]
    fn what_the_signals_cant_settle_is_unknown() {
        assert_eq!(classify(&blank()), Unknown);
        // Everything timed out.
        assert_eq!(classify(&Probes { status: Run::TimedOut, version: Run::TimedOut, ..blank() }), Unknown);
        // A hung wsl.exe with no other sign of WSL: maybe it's there and stuck.
        assert_eq!(classify(&Probes { status: Run::TimedOut, ..win11_without_wsl() }), Unknown);
        // On but no hypervisor and nothing pending: perhaps virtualization is off in the firmware.
        let no_hypervisor = Probes { hypervisor: Some(false), ..win10_inbox_ready() };
        assert_eq!(classify(&no_hypervisor), Unknown);
        assert!(virtualization_off(&no_hypervisor, Unknown));
        // Only the platform on, nothing of WSL: neither ready nor plainly missing.
        assert_eq!(classify(&Probes { vm_platform: Some(Feature::Enabled), ..win11_without_wsl() }), Unknown);
    }

    #[test]
    fn wsl_1_without_the_platform_is_missing_wsl_2() {
        let wsl1 = Probes { vm_platform: Some(Feature::Disabled), hypervisor: Some(false), ..win10_inbox_ready() };
        assert_eq!(classify(&wsl1), Missing);
        // Unless the platform is being turned on.
        assert_eq!(classify(&Probes { reboot_pending: Some(true), ..wsl1 }), NeedsReboot);
    }

    #[test]
    fn the_wmi_output_is_read_by_key() {
        let out = "features=ok\r\nfeature:VirtualMachinePlatform=1\r\nfeature:Microsoft-Windows-Subsystem-Linux=2\r\nhypervisor=True\r\n";
        assert_eq!(parse_cim(out), (Some(Feature::Enabled), Some(Feature::Disabled), Some(true)));
        // An edition without the feature doesn't list it.
        assert_eq!(parse_cim("features=ok\nfeature:VirtualMachinePlatform=3\nhypervisor=False\n"), (Some(Feature::Absent), Some(Feature::Absent), Some(false)));
        // The feature query failed; the hypervisor one didn't.
        assert_eq!(parse_cim("hypervisor=False\n"), (None, None, Some(false)));
        // InstallState 4 (unknown), and nothing at all.
        assert_eq!(
            parse_cim("features=ok\nfeature:VirtualMachinePlatform=4\nfeature:Microsoft-Windows-Subsystem-Linux=1"),
            (None, Some(Feature::Enabled), None)
        );
        assert_eq!(parse_cim(""), (None, None, None));
    }

    #[test]
    fn a_probe_reads_as_ok_failed_timed_out_or_missing() {
        let failed = |stderr: &str| Err(Error::CommandFailed { command: "wsl --status".into(), stderr: stderr.into() });
        assert_eq!(outcome(&Ok("features=ok".into())), (Run::Ok, "features=ok"));
        assert_eq!(outcome(&failed("")).0, Run::Failed);
        assert_eq!(outcome(&failed("timed out after 10s (the tool or its backend may be stuck)")).0, Run::TimedOut);
        assert_eq!(outcome(&Err(Error::ToolMissing { tool: "wsl" })).0, Run::Missing);
    }

    #[test]
    fn the_state_reaches_the_ui_in_camel_case() {
        let report = WslReport { state: NeedsReboot, reopens_after_restart: true, virtualization_off: false };
        assert_eq!(
            serde_json::to_value(report).unwrap(),
            serde_json::json!({ "state": "needsReboot", "reopensAfterRestart": true, "virtualizationOff": false })
        );
    }

    #[test]
    fn an_install_mark_counts_for_the_boot_it_was_made_in_only() {
        let t = TestApp::new();
        assert!(!installed_this_boot(&t.dirs(), 1_000));
        mark_installed(&t.dirs());
        let boot = sysinfo::System::boot_time();
        assert!(installed_this_boot(&t.dirs(), boot));
        assert!(installed_this_boot(&t.dirs(), boot + 2), "the boot time jitters");
        // After a restart the mark is gone for good.
        assert!(!installed_this_boot(&t.dirs(), boot + 600));
        assert!(!t.data().join(INSTALL_MARK).exists());
        assert!(!installed_this_boot(&t.dirs(), boot));
        std::fs::write(t.data().join(INSTALL_MARK), "garbage").unwrap();
        assert!(!installed_this_boot(&t.dirs(), boot));
        assert!(same_boot(10, 40) && !same_boot(10, 41));
    }

    #[test]
    fn a_restart_mark_counts_from_the_next_boot_until_wsl_is_ready_or_installed_again() {
        let t = TestApp::new();
        mark(&t.dirs(), RESTART_MARK, 1_000);
        // Still counting down in the same boot: not restarted yet.
        assert!(!restarted_for_wsl(&t.dirs(), 1_000));
        assert!(restarted_for_wsl(&t.dirs(), 2_000));
        // Not ready: remembered.
        assert_eq!(assess(&t.dirs(), &Probes { restarted_for_wsl: true, ..firmware_off() }), (Unknown, true));
        assert!(restarted_for_wsl(&t.dirs(), 2_000));
        // Ready: forgotten.
        assert_eq!(assess(&t.dirs(), &win11_store_ready()), (Ready, false));
        assert!(!restarted_for_wsl(&t.dirs(), 2_000));
        // A new install forgets it too.
        mark(&t.dirs(), RESTART_MARK, 1_000);
        mark_installed(&t.dirs());
        assert!(!restarted_for_wsl(&t.dirs(), 2_000));
    }

    #[test]
    fn the_restart_note_is_one_short_line() {
        assert_eq!(reason("").unwrap(), DEFAULT_REASON);
        assert_eq!(reason("  Cyber CTF: finishing WSL 2  ").unwrap(), "Cyber CTF: finishing WSL 2");
        assert!(reason("two\nlines").is_err());
        assert!(reason(&"a".repeat(201)).is_err());
        assert_eq!(reason(&"é".repeat(200)).unwrap().chars().count(), 200);
    }

    #[test]
    fn the_app_reopens_by_its_path_or_its_store_package() {
        assert_eq!(reopen_line(r"C:\Program Files\Cyber CTF\cyberctf-desktop.exe").as_deref(), Some(r#""C:\Program Files\Cyber CTF\cyberctf-desktop.exe""#));
        assert_eq!(
            reopen_line(r"C:\Program Files\WindowsApps\F4K.CyberCTF_1.4.1.0_x64__8wekyb3d8bbwe\CyberCTF.exe").as_deref(),
            Some(r"explorer.exe shell:AppsFolder\F4K.CyberCTF_8wekyb3d8bbwe!CyberCTF")
        );
        // Anything it can't read as a package, or put in one command line, isn't registered.
        assert_eq!(reopen_line(r"C:\Program Files\WindowsApps\Odd\CyberCTF.exe"), None);
        assert_eq!(reopen_line(r"C:\Program Files\WindowsApps\A&B_1_x64__p\CyberCTF.exe"), None);
        assert_eq!(reopen_line(r#"C:\a"b\app.exe"#), None);
        assert_eq!(reopen_line(""), None);
    }

    /// A `RunOnce` stand-in: whether it was written, and whether writing works.
    #[derive(Default)]
    struct FakeReopen {
        refuses: bool,
        written: AtomicBool,
        writes: AtomicU32,
    }

    impl FakeReopen {
        fn written(&self) -> bool {
            self.written.load(Ordering::SeqCst)
        }
    }

    impl Reopen for FakeReopen {
        fn register(&self) -> bool {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.written.store(!self.refuses, Ordering::SeqCst);
            !self.refuses
        }
        fn unregister(&self) {
            self.written.store(false, Ordering::SeqCst);
        }
    }

    const SHUTDOWN: &str = "shutdown /r /t 5 /c Cyber CTF: finishing WSL 2";

    #[tokio::test]
    async fn a_restart_is_scheduled_once_whoever_asks() {
        let t = TestApp::new();
        let tools = Tools::rules(&[("shutdown", ok(""))]);
        let (state, reopen) = (AtomicU8::new(IDLE), FakeReopen::default());
        schedule_restart(&t.dirs(), &state, "Cyber CTF: finishing WSL 2", &reopen).await.unwrap();
        assert_eq!(tools.calls(), [SHUTDOWN]);
        assert!(reopen.written());
        assert_eq!(marked_boot(&t.dirs(), RESTART_MARK), Some(sysinfo::System::boot_time()));
        // The other window, or the remounted step, asks again during the countdown: the same
        // restart, no second shutdown, RunOnce left as it is.
        schedule_restart(&t.dirs(), &state, "Cyber CTF: finishing WSL 2", &reopen).await.unwrap();
        assert_eq!(tools.calls().len(), 1);
        assert_eq!(reopen.writes.load(Ordering::SeqCst), 1);
        assert!(reopen.written());
    }

    #[test]
    fn the_restart_can_run_as_a_tauri_command() {
        // Tauri runs a command's future on its thread pool: it must be `Send` on every OS, though
        // only Windows awaits it from the command.
        fn send<T: Send>(_: &T) {}
        let t = TestApp::new();
        let (state, reopen) = (AtomicU8::new(IDLE), FakeReopen::default());
        send(&schedule_restart(&t.dirs(), &state, "x", &reopen));
        send(&machine_restart(t.dirs(), String::new()));
    }

    #[tokio::test]
    async fn a_restart_already_scheduled_by_windows_counts_as_this_one() {
        let t = TestApp::new();
        let _tools = Tools::rules(&[("shutdown", fail("A system shutdown has already been scheduled.(1190)"))]);
        let (state, reopen) = (AtomicU8::new(IDLE), FakeReopen::default());
        schedule_restart(&t.dirs(), &state, "x", &reopen).await.unwrap();
        assert!(reopen.written(), "the app still reopens after that restart");
        assert_eq!(state.load(Ordering::SeqCst), SCHEDULED);
        assert!(marked_boot(&t.dirs(), RESTART_MARK).is_some());
    }

    #[tokio::test]
    async fn a_failed_restart_takes_back_what_it_wrote_and_can_be_tried_again() {
        let t = TestApp::new();
        let tools = Tools::rules(&[("shutdown", fail("Access is denied.(5)"))]);
        let (state, reopen) = (AtomicU8::new(IDLE), FakeReopen::default());
        let err = schedule_restart(&t.dirs(), &state, "Cyber CTF: finishing WSL 2", &reopen).await.unwrap_err();
        assert!(err.to_string().contains("Access is denied"), "{err}");
        assert!(!reopen.written(), "RunOnce removed");
        assert!(marked_boot(&t.dirs(), RESTART_MARK).is_none());
        drop(tools);
        let tools = Tools::rules(&[("shutdown", ok(""))]);
        schedule_restart(&t.dirs(), &state, "Cyber CTF: finishing WSL 2", &reopen).await.unwrap();
        assert_eq!(tools.calls(), [SHUTDOWN]);
        assert!(reopen.written());
    }

    #[tokio::test]
    async fn a_failed_restart_leaves_a_runonce_it_didnt_write() {
        // Writing RunOnce failed: nothing of ours to remove, and the unregister isn't called.
        struct Untouchable;
        impl Reopen for Untouchable {
            fn register(&self) -> bool {
                false
            }
            fn unregister(&self) {
                panic!("removed a RunOnce entry this call didn't write");
            }
        }
        let t = TestApp::new();
        let _tools = Tools::rules(&[("shutdown", fail("Access is denied.(5)"))]);
        assert!(schedule_restart(&t.dirs(), &AtomicU8::new(IDLE), "x", &Untouchable).await.is_err());
        let refused = FakeReopen { refuses: true, ..FakeReopen::default() };
        let _tools = Tools::rules(&[("shutdown", ok(""))]);
        schedule_restart(&t.dirs(), &AtomicU8::new(IDLE), "x", &refused).await.unwrap();
        assert!(!refused.written());
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn restarting_is_refused_off_windows_and_the_check_has_no_wsl() {
        let t = TestApp::new();
        let tools = Tools::rules(&[]);
        let err = machine_restart(t.dirs(), String::new()).await.unwrap_err();
        assert_eq!(err.to_string(), "Restarting from Cyber CTF is only available on Windows.");
        assert_eq!(tools.calls(), [] as [&str; 0]);
        assert!(report(&t.dirs()).await.is_none());
    }

    #[tokio::test]
    async fn a_bad_restart_note_is_refused_whatever_the_os() {
        let t = TestApp::new();
        let err = machine_restart(t.dirs(), "a\nb".into()).await.unwrap_err();
        assert_eq!(err.to_string(), "The restart note must be one line of at most 200 characters.");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn the_restart_command_runs_shutdown_with_the_note() {
        let t = TestApp::new();
        let tools = Tools::rules(&[("shutdown", ok(""))]);
        machine_restart(t.dirs(), "Cyber CTF: finishing WSL 2".into()).await.unwrap();
        assert_eq!(tools.calls(), [SHUTDOWN]);
        // The process-wide state: later asks in this test process are the same restart.
        RESTART.store(IDLE, Ordering::SeqCst);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_test_never_restarts_for_real() {
        let t = TestApp::new();
        assert!(machine_restart(t.dirs(), String::new()).await.is_err());
    }

    /// The real probes on this Windows: they finish in time, read the registry, and give a state.
    #[cfg(windows)]
    #[tokio::test]
    async fn the_probes_answer_on_this_machine() {
        let started = std::time::Instant::now();
        let probes = windows::probe(false, false).await;
        println!("{probes:?} -> {:?}", classify(&probes));
        assert!(started.elapsed() < std::time::Duration::from_secs(45));
        assert!(probes.reboot_pending.is_some() && probes.lxss_service.is_some() && probes.wsl_service.is_some(), "{probes:?}");
        let t = TestApp::new();
        assert!(report(&t.dirs()).await.is_some());
    }

    /// Removes the test's RunOnce value however the test ends.
    #[cfg(windows)]
    struct RunOnceValue(String);

    #[cfg(windows)]
    impl Drop for RunOnceValue {
        fn drop(&mut self) {
            windows::delete_run_once(&self.0);
        }
    }

    #[cfg(windows)]
    #[test]
    fn run_once_is_written_and_removed() {
        // A name of its own: the app's `CyberCTF` value is never touched by a test.
        let value = RunOnceValue(format!("CyberCTF-unit-test-{}", rand::random::<u32>()));
        assert!(windows::set_run_once(&value.0, r#""C:\nowhere\app.exe""#));
        assert_eq!(windows::run_once_exists(&value.0), Some(true));
        windows::delete_run_once(&value.0);
        assert_eq!(windows::run_once_exists(&value.0), Some(false));
        assert!(windows::reopen_command().is_some());
        // Nothing to write: nothing written.
        assert!(!Reopen::register(&windows::UserRunOnce(None)));
    }
}
