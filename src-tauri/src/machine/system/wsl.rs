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
//!   pending file renames, a weak sign (many installers leave some);
//! - whether Cyber CTF's own "Turn on WSL 2" ran since this boot.
//!
//! Every probe is headless and timed: a hung `wsl.exe` costs the check a few seconds, never the
//! setup.

// Off Windows only the types (for the UI) and the tests use most of this.
#![cfg_attr(not(windows), allow(dead_code))]

use serde::Serialize;

use crate::AppHandle;
use crate::error::{Error, Result};

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
    // meanwhile (they can read as on already), they only start after it.
    if p.installed_this_boot && pending {
        return WslState::NeedsReboot;
    }
    if wsl && !platform_off && !not_running {
        // The platform is on and its hypervisor runs. A restart Windows waits for may be an
        // update's; then WSL has to answer too.
        if platform_on && running && (!pending || p.status == Run::Ok) {
            return WslState::Ready;
        }
        // WSL answers and nothing waits for a restart (the features couldn't be read).
        if p.status == Run::Ok && !pending && p.vm_platform.is_none() {
            return WslState::Ready;
        }
    }
    if p.installed_this_boot {
        return WslState::NeedsReboot;
    }
    if (wsl || platform_on) && pending {
        return WslState::NeedsReboot;
    }
    if platform_on && not_running {
        // On, yet no hypervisor: a restart still to come, or virtualization off in the firmware.
        // Only file renames hint at the first.
        return if p.renames_pending == Some(true) { WslState::NeedsReboot } else { WslState::Unknown };
    }
    let wsl_failed = matches!(p.status, Run::Failed | Run::Missing);
    let no_services = p.lxss_service == Some(false) && p.wsl_service == Some(false);
    // Nothing of WSL, as both the services and wsl.exe say; or WSL 1 only, without the platform.
    if (!wsl && wsl_failed && (no_services || platform_off)) || (wsl && platform_off && !pending) {
        return WslState::Missing;
    }
    WslState::Unknown
}

/// The features and the hypervisor, from the output of `windows::CIM_SCRIPT`: `features=ok` once
/// the feature query answered, then `feature:<name>=<InstallState>` for each feature it lists
/// (an unlisted one is absent), and `hypervisor=True|False`.
pub fn parse_cim(out: &str) -> (Option<Feature>, Option<Feature>, Option<bool>) {
    let lines: Vec<&str> = out.lines().map(str::trim).collect();
    let answered = lines.contains(&"features=ok");
    let feature = |name: &str| {
        if !answered {
            return None;
        }
        let prefix = format!("feature:{name}=");
        match lines.iter().find_map(|l| l.strip_prefix(prefix.as_str())) {
            None => Some(Feature::Absent),
            Some("1") => Some(Feature::Enabled),
            Some("2") => Some(Feature::Disabled),
            Some("3") => Some(Feature::Absent),
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

// ---------- Cyber CTF's own install, this boot ----------

/// The app data file holding the boot time at which "Turn on WSL 2" last ran.
const MARKER: &str = "wsl-install-boot";

/// Boot times read a second or so apart within one boot (they're derived from the uptime);
/// two boots are minutes apart.
fn same_boot(a: u64, b: u64) -> bool {
    a.abs_diff(b) <= 30
}

fn marker(app: &AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    app.path().app_data_dir().ok().map(|d| d.join(MARKER))
}

/// Remembers that "Turn on WSL 2" ran in this boot, so the check asks for the restart even
/// after the app was closed and opened again before it.
pub fn mark_installed(app: &AppHandle) {
    let Some(f) = marker(app) else { return };
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(f, sysinfo::System::boot_time().to_string());
}

/// Whether "Turn on WSL 2" ran in the current boot (a mark from an earlier boot is removed).
fn installed_this_boot(app: &AppHandle, boot: u64) -> bool {
    let Some(f) = marker(app) else { return false };
    let Some(marked) = std::fs::read_to_string(&f).ok().and_then(|s| s.trim().parse::<u64>().ok()) else { return false };
    if same_boot(marked, boot) {
        return true;
    }
    let _ = std::fs::remove_file(f);
    false
}

/// Where WSL 2 stands on this machine: Windows only (`None` elsewhere).
pub async fn report(app: &AppHandle) -> Option<WslReport> {
    #[cfg(windows)]
    {
        let probes = windows::probe(installed_this_boot(app, sysinfo::System::boot_time())).await;
        let state = classify(&probes);
        log::info!("WSL check: {probes:?} -> {state:?}");
        Some(WslReport { state, reopens_after_restart: windows::reopen_command().is_some() })
    }
    #[cfg(not(windows))]
    {
        let _ = app;
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

/// Restarts Windows in a few seconds (`shutdown /r`), to finish turning WSL 2 on. Before that,
/// asks Windows to open Cyber CTF once when the player signs back in (`RunOnce`). Windows only.
#[tauri::command]
pub async fn machine_restart(reason: String) -> Result<()> {
    let reason = self::reason(&reason)?;
    #[cfg(windows)]
    {
        // A unit test must never restart the machine it runs on: only scripted tools here.
        #[cfg(test)]
        if !crate::exec::fake::scripted() {
            return Err(Error::Invalid("a test restarted without scripting shutdown".into()));
        }
        let reopen = if cfg!(test) { None } else { windows::reopen_command() };
        let registered = reopen.as_deref().is_some_and(windows::register_reopen);
        let restart = crate::exec::run("shutdown", &["/r", "/t", "5", "/c", &reason], None).await;
        if restart.is_err() && registered {
            windows::unregister_reopen();
        }
        restart.map(|_| ())
    }
    #[cfg(not(windows))]
    {
        let _ = reason;
        Err(Error::Invalid("Restarting from Cyber CTF is only available on Windows.".into()))
    }
}

/// The command that opens this app again, for `RunOnce`: the executable itself, or for the
/// Store (MSIX) build, whose executable can't be started by path, its app through the shell.
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
    use std::process::Stdio;
    use std::time::Duration;

    use base64::Engine;
    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, RRF_RT_ANY, RegCloseKey, RegDeleteKeyValueW, RegGetValueW, RegOpenKeyExW,
        RegSetKeyValueW,
    };

    use super::{Probes, Run, parse_cim};

    const WSL_TIMEOUT: Duration = Duration::from_secs(10);
    const CIM_TIMEOUT: Duration = Duration::from_secs(20);

    /// The features and the hypervisor, through WMI (no admin rights needed), printed as
    /// language-neutral `key=value` lines (see [`parse_cim`]). Each query fails on its own.
    pub const CIM_SCRIPT: &str = "$ProgressPreference = 'SilentlyContinue'
try {
  $f = @(Get-CimInstance -ClassName Win32_OptionalFeature -Filter \"Name='VirtualMachinePlatform' OR Name='Microsoft-Windows-Subsystem-Linux'\" -ErrorAction Stop)
  'features=ok'
  foreach ($x in $f) { 'feature:' + $x.Name + '=' + [int]$x.InstallState }
} catch {}
try { 'hypervisor=' + [bool](Get-CimInstance -ClassName Win32_ComputerSystem -ErrorAction Stop).HypervisorPresent } catch {}
";

    /// Every signal, the commands side by side.
    pub async fn probe(installed_this_boot: bool) -> Probes {
        let encoded = encode(CIM_SCRIPT);
        let cim_args = ["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded.as_str()];
        let status = run("wsl", &["--status"], WSL_TIMEOUT);
        let version = run("wsl", &["--version"], WSL_TIMEOUT);
        let (status, version, cim) = tokio::join!(status, version, run("powershell", &cim_args, CIM_TIMEOUT));
        let (vm_platform, wsl_feature, hypervisor) = match cim {
            (Run::Ok, out) => parse_cim(&String::from_utf8_lossy(&out)),
            _ => (None, None, None),
        };
        let services = r"SYSTEM\CurrentControlSet\Services";
        Probes {
            status: status.0,
            version: version.0,
            lxss_service: key_exists(HKEY_LOCAL_MACHINE, &format!(r"{services}\LxssManager")),
            wsl_service: key_exists(HKEY_LOCAL_MACHINE, &format!(r"{services}\WslService")),
            vm_platform,
            wsl_feature,
            hypervisor,
            reboot_pending: key_exists(HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending"),
            renames_pending: value_exists(HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control\Session Manager", "PendingFileRenameOperations"),
            installed_this_boot,
        }
    }

    /// A script for `powershell -EncodedCommand`: base64 of its UTF-16LE text (no quoting to get
    /// wrong on the command line).
    fn encode(script: &str) -> String {
        let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// Runs a probe headless, with no input, killed after `timeout`: how it went, and its stdout.
    async fn run(program: &str, args: &[&str], timeout: Duration) -> (Run, Vec<u8>) {
        let mut cmd = tokio::process::Command::new(program);
        // wsl.exe writes UTF-16 unless told otherwise; only exit codes are read, but a log of it
        // stays readable.
        cmd.args(args).env("WSL_UTF8", "1").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
        crate::exec::headless(&mut cmd);
        let child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Run::Missing, Vec::new()),
            Err(_) => return (Run::Failed, Vec::new()),
        };
        // On a timeout the child is dropped, which kills it.
        match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Err(_) => (Run::TimedOut, Vec::new()),
            Ok(Ok(out)) => (if out.status.success() { Run::Ok } else { Run::Failed }, out.stdout),
            Ok(Err(_)) => (Run::Failed, Vec::new()),
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// Whether a registry key exists. A key this account may not read still exists.
    pub fn key_exists(root: HKEY, path: &str) -> Option<bool> {
        let path = wide(path);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: a NUL-terminated key name; `key` receives the handle, closed right after.
        let status = unsafe { RegOpenKeyExW(root, path.as_ptr(), 0, KEY_READ, &mut key) };
        match status {
            ERROR_SUCCESS => {
                // SAFETY: the handle RegOpenKeyExW just opened.
                unsafe { RegCloseKey(key) };
                Some(true)
            }
            ERROR_ACCESS_DENIED => Some(true),
            ERROR_FILE_NOT_FOUND => Some(false),
            _ => None,
        }
    }

    /// Whether a registry value exists (its data isn't read).
    pub fn value_exists(root: HKEY, path: &str, name: &str) -> Option<bool> {
        let (path, name) = (wide(path), wide(name));
        // SAFETY: NUL-terminated names; no type or data asked for (all three pointers null).
        let status = unsafe { RegGetValueW(root, path.as_ptr(), name.as_ptr(), RRF_RT_ANY, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
        match status {
            ERROR_SUCCESS | ERROR_MORE_DATA => Some(true),
            ERROR_FILE_NOT_FOUND => Some(false),
            _ => None,
        }
    }

    const RUN_ONCE: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";
    const RUN_ONCE_NAME: &str = "CyberCTF";

    /// The command line that opens this app again, when there is one.
    pub fn reopen_command() -> Option<String> {
        super::reopen_line(std::env::current_exe().ok()?.to_str()?)
    }

    /// Opens `line` once at the player's next sign-in (their own `RunOnce`: no admin rights).
    pub fn register_reopen(line: &str) -> bool {
        set_run_once(RUN_ONCE_NAME, line)
    }

    pub fn unregister_reopen() {
        delete_run_once(RUN_ONCE_NAME);
    }

    pub(super) fn set_run_once(name: &str, line: &str) -> bool {
        let (key, name, data) = (wide(RUN_ONCE), wide(name), wide(line));
        let Ok(bytes) = u32::try_from(data.len() * 2) else { return false };
        // SAFETY: NUL-terminated names; `data` is a NUL-terminated REG_SZ of `bytes` bytes.
        let status = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ, data.as_ptr().cast(), bytes) };
        status == ERROR_SUCCESS
    }

    pub(super) fn delete_run_once(name: &str) {
        let (key, name) = (wide(RUN_ONCE), wide(name));
        // SAFETY: NUL-terminated names.
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
    }

    #[cfg(test)]
    pub(super) fn run_once_exists(name: &str) -> Option<bool> {
        value_exists(HKEY_CURRENT_USER, RUN_ONCE, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        }
    }

    #[test]
    fn a_machine_without_wsl_is_missing() {
        assert_eq!(classify(&win11_without_wsl()), Missing);
        // Windows 10, whose wsl.exe may not exist at all.
        assert_eq!(classify(&Probes { status: Run::Missing, version: Run::Missing, ..win11_without_wsl() }), Missing);
        // The features couldn't be read: the services and wsl.exe still agree.
        assert_eq!(classify(&Probes { vm_platform: None, wsl_feature: None, hypervisor: None, ..win11_without_wsl() }), Missing);
        // A restart for an update is no sign of WSL.
        assert_eq!(classify(&Probes { reboot_pending: Some(true), ..win11_without_wsl() }), Missing);
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
        // On, not running, nothing pending in servicing, but files wait for the restart.
        assert_eq!(classify(&Probes { reboot_pending: Some(false), renames_pending: Some(true), ..by_hand }), NeedsReboot);
    }

    #[test]
    fn our_install_that_changed_nothing_is_judged_on_the_rest() {
        // It ran this boot, yet no restart is pending: WSL was ready already.
        assert_eq!(classify(&Probes { installed_this_boot: true, ..win11_store_ready() }), Ready);
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
    fn the_state_reaches_the_ui_in_camel_case() {
        let report = WslReport { state: NeedsReboot, reopens_after_restart: true };
        assert_eq!(serde_json::to_value(report).unwrap(), serde_json::json!({ "state": "needsReboot", "reopensAfterRestart": true }));
    }

    #[test]
    fn a_mark_counts_for_the_boot_it_was_made_in_only() {
        let t = crate::test_app::TestApp::new();
        assert!(!installed_this_boot(t.handle(), 1_000));
        mark_installed(t.handle());
        let boot = sysinfo::System::boot_time();
        assert!(installed_this_boot(t.handle(), boot));
        assert!(installed_this_boot(t.handle(), boot + 2), "the boot time jitters");
        // After a restart the mark is gone for good.
        assert!(!installed_this_boot(t.handle(), boot + 600));
        assert!(!t.data().join(MARKER).exists());
        assert!(!installed_this_boot(t.handle(), boot));
        std::fs::write(t.data().join(MARKER), "garbage").unwrap();
        assert!(!installed_this_boot(t.handle(), boot));
        assert!(same_boot(10, 40) && !same_boot(10, 41));
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

    #[cfg(not(windows))]
    #[tokio::test]
    async fn restarting_is_refused_off_windows_and_the_check_has_no_wsl() {
        let err = machine_restart(String::new()).await.unwrap_err();
        assert_eq!(err.to_string(), "Restarting from Cyber CTF is only available on Windows.");
        let t = crate::test_app::TestApp::new();
        assert!(report(t.handle()).await.is_none());
    }

    #[tokio::test]
    async fn a_bad_restart_note_is_refused_before_anything_runs() {
        let tools = crate::exec::fake::Tools::rules(&[]);
        assert!(machine_restart("a\nb".into()).await.is_err());
        assert!(tools.calls().is_empty());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn the_restart_runs_shutdown_with_the_note() {
        use crate::exec::fake::{Tools, fail, ok};
        let tools = Tools::rules(&[("shutdown", ok(""))]);
        machine_restart("Cyber CTF: finishing WSL 2".into()).await.unwrap();
        assert_eq!(tools.calls(), ["shutdown /r /t 5 /c Cyber CTF: finishing WSL 2"]);
        drop(tools);
        let _tools = Tools::rules(&[("shutdown", fail("Access is denied.(5)"))]);
        assert!(machine_restart(String::new()).await.unwrap_err().to_string().contains("Access is denied"));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_test_never_restarts_for_real() {
        assert!(machine_restart(String::new()).await.is_err());
    }

    /// The real probes on this Windows: they finish in time, read the registry, and give a state.
    #[cfg(windows)]
    #[tokio::test]
    async fn the_probes_answer_on_this_machine() {
        let started = std::time::Instant::now();
        let probes = windows::probe(false).await;
        println!("{probes:?} -> {:?}", classify(&probes));
        assert!(started.elapsed() < std::time::Duration::from_secs(45));
        assert!(probes.reboot_pending.is_some() && probes.lxss_service.is_some() && probes.wsl_service.is_some(), "{probes:?}");
        let t = crate::test_app::TestApp::new();
        assert!(report(t.handle()).await.is_some());
    }

    #[cfg(windows)]
    #[test]
    fn run_once_is_written_and_removed() {
        let name = format!("CyberCTF-test-{}", rand::random::<u32>());
        assert!(windows::set_run_once(&name, r#""C:\nowhere\app.exe""#));
        assert_eq!(windows::run_once_exists(&name), Some(true));
        windows::delete_run_once(&name);
        assert_eq!(windows::run_once_exists(&name), Some(false));
        assert!(windows::reopen_command().is_some());
    }
}
