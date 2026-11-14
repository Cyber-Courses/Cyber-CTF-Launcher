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
use crate::machine::setup::WindowsFeature;
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
    /// Whether WSL 2 is on, waits for a restart, or is missing.
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
    /// The Virtual Machine Platform feature's state.
    pub vm_platform: Option<Feature>,
    /// The Windows Subsystem for Linux feature's state.
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
    let hypervisor_running = p.hypervisor != Some(false);
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
    let answers_with_features_unread = wsl && p.status == Run::Ok && p.reboot_pending == Some(false) && p.vm_platform.is_none();
    if answers_with_features_unread && hypervisor_running {
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
/// The app data file holding the boot time at which Cyber CTF turned Hyper-V on.
const HYPERV_MARK: &str = "hyperv-on-boot";

/// Boot times read a second or so apart within one boot (they're derived from the uptime);
/// two boots are minutes apart.
fn same_boot(a: u64, b: u64) -> bool {
    a.abs_diff(b) <= 30
}

/// Remembers boot `boot` in the mark `name`. One that can't be written is logged: the check then
/// can't tell, after the app is opened again, that a restart is due for WSL or was made for it.
fn mark(dirs: &AppDirs, name: &str, boot: u64) {
    let written = dirs.data().and_then(|data| crate::platform::atomic_file::write(&data.join(name), boot.to_string()).map_err(Error::from));
    if let Err(e) = written {
        log::warn!("couldn't keep the WSL mark {name}: {e}");
    }
}

fn marked_boot(dirs: &AppDirs, name: &str) -> Option<u64> {
    std::fs::read_to_string(dirs.data_file(name)?).ok()?.trim().parse().ok()
}

/// Forgets the mark `name`; one that isn't there is forgotten already. One that can't be removed
/// is logged: the check would go on reading it.
fn unmark(dirs: &AppDirs, name: &str) {
    let Some(f) = dirs.data_file(name) else { return };
    if let Err(e) = crate::platform::fs::remove_if_present(&f) {
        log::warn!("couldn't remove the WSL mark {}: {e}", f.display());
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
    marked_this_boot(dirs, INSTALL_MARK, boot)
}

/// Whether the mark `name` is of boot `boot` (one from an earlier boot is removed).
fn marked_this_boot(dirs: &AppDirs, name: &str, boot: u64) -> bool {
    match marked_boot(dirs, name) {
        Some(marked) if same_boot(marked, boot) => true,
        Some(_) => {
            unmark(dirs, name);
            false
        }
        None => false,
    }
}

/// Remembers that Cyber CTF turned Hyper-V on in this boot: the feature starts at the next
/// restart, so the setup asks for one until then (Hyper-V reads as off meanwhile).
pub fn mark_hyperv_on(dirs: &AppDirs) {
    mark(dirs, HYPERV_MARK, sysinfo::System::boot_time());
}

/// Whether Cyber CTF turned Hyper-V on in this boot, so a restart is due for it.
pub fn hyperv_on_this_boot(dirs: &AppDirs) -> bool {
    marked_this_boot(dirs, HYPERV_MARK, sysinfo::System::boot_time())
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

/// Logs a WSL check's `outcome` at `info` when it differs from the last one, else at `debug`, so
/// a Machine page left open doesn't fill the log with the same line; whether it was new (the
/// name says it logs).
fn log_if_changed(outcome: &str) -> bool {
    static LAST: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
    let mut last = LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let new = *last != outcome;
    if new {
        log::info!("{outcome}");
        outcome.clone_into(&mut last);
    } else {
        log::debug!("{outcome}");
    }
    new
}

/// Where WSL 2 stands on this machine: Windows only (`None` elsewhere).
#[cfg_attr(not(windows), allow(clippy::unused_async, reason = "it awaits the probes on Windows"))]
pub async fn report(dirs: &AppDirs) -> Option<WslReport> {
    #[cfg(windows)]
    {
        let boot = sysinfo::System::boot_time();
        let probes = windows::probe(installed_this_boot(dirs, boot), restarted_for_wsl(dirs, boot)).await;
        let (state, virtualization_off) = assess(dirs, &probes);
        log_if_changed(&format!("WSL check: {probes:?} -> {state:?}"));
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
async fn schedule_restart(dirs: &AppDirs, state: &AtomicU8, reason: &str, reopen: &(dyn Reopen + Sync), feature: WindowsFeature) -> Result<()> {
    if state.compare_exchange(IDLE, ASKING, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return Ok(());
    }
    let registered = reopen.register();
    // WSL's own mark: Hyper-V's restart is no restart for WSL (its turned-on mark expires with
    // this boot by itself).
    let for_wsl = feature == WindowsFeature::Wsl;
    if for_wsl {
        mark(dirs, RESTART_MARK, sysinfo::System::boot_time());
    }
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
            if for_wsl {
                unmark(dirs, RESTART_MARK);
            }
            state.store(IDLE, Ordering::SeqCst);
            Err(e)
        }
    }
}

/// Restarts Windows in a few seconds (`shutdown /r`), to finish turning WSL 2 (or Hyper-V) on. Before that,
/// asks Windows to open Cyber CTF once when the player signs back in (`RunOnce`). Windows only.
#[cfg_attr(not(windows), allow(clippy::unused_async, reason = "it awaits the restart on Windows"))]
pub async fn machine_restart(dirs: AppDirs, reason: String, feature: WindowsFeature) -> Result<()> {
    let reason = self::reason(&reason)?;
    #[cfg(windows)]
    {
        // A unit test must never restart the machine it runs on: only scripted tools here.
        #[cfg(test)]
        if !crate::platform::exec::fake::scripted() {
            return Err(Error::Invalid("a test restarted without scripting shutdown".into()));
        }
        let machines = crate::platform::exec::tools_in_use().are_the_machines();
        let reopen = windows::UserRunOnce(if machines { windows::reopen_command() } else { None });
        schedule_restart(&dirs, &RESTART, &reason, &reopen, feature).await
    }
    #[cfg(not(windows))]
    {
        let _ = (dirs, reason, feature);
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
mod tests;
