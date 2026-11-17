//! WSL 2 on Windows, as the launcher keeps it: isothrum-machine says where WSL 2 stands
//! ([`isothrum_machine::wsl`], its readiness classifier over `wsl.exe`, the registry and WMI); the
//! launcher remembers what it did itself, by boot (its own "Turn on WSL 2", and the restart it made
//! for it, handed to the check as [`Marks`]), and restarts Windows to finish turning WSL 2 or
//! Hyper-V on.

#![cfg_attr(not(windows), allow(dead_code, reason = "off Windows only the UI's types and the tests use most of this"))]

use std::sync::atomic::{AtomicU8, Ordering};

pub use isothrum_machine::wsl::WslState;
use isothrum_machine::wsl::{Marks, Wsl};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::machine::setup::WindowsFeature;
use crate::platform::AppDirs;

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

/// What Cyber CTF did itself in this boot and the ones before, for isothrum-machine's check.
pub fn marks(dirs: &AppDirs) -> Marks {
    let boot = sysinfo::System::boot_time();
    Marks { installed_this_boot: installed_this_boot(dirs, boot), restarted_for_wsl: restarted_for_wsl(dirs, boot) }
}

/// What the check found, as the launcher reports it, and what it remembers: once WSL 2 is ready,
/// the restart done for it is forgotten.
pub fn report(dirs: &AppDirs, found: &Wsl) -> WslReport {
    remember(dirs, found.state);
    log_if_changed(&format!("WSL check: {:?} -> {:?}", found.probes, found.state));
    WslReport { state: found.state, reopens_after_restart: reopen_command().is_some(), virtualization_off: found.virtualization_off }
}

/// Forgets the restart made for WSL once it is ready (`state`).
fn remember(dirs: &AppDirs, state: WslState) {
    if state == WslState::Ready {
        unmark(dirs, RESTART_MARK);
    }
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

/// `shutdown.exe` refusing because one is already scheduled (`ERROR_SHUTDOWN_IS_SCHEDULED`: its
/// message ends with the code in every language).
fn already_scheduled(e: &Error) -> bool {
    matches!(e, Error::CommandFailed { stderr, .. } if win32_error(stderr) == Some(ERROR_SHUTDOWN_IS_SCHEDULED))
}

/// `shutdown.exe`'s refusal because a shutdown is already scheduled.
const ERROR_SHUTDOWN_IS_SCHEDULED: u32 = 1190;

/// The Win32 error `text` ends with (its last `(<number>)`), if it ends with one: Windows' own
/// tools end their message with it in every display language (`A system shutdown has already
/// been scheduled.(1190)`), so the number, not the words, says which error it was.
fn win32_error(text: &str) -> Option<u32> {
    let text = text.trim_end().trim_end_matches('.');
    let (_, number) = text.strip_suffix(')')?.rsplit_once('(')?;
    number.trim().parse().ok()
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
        if !crate::testing::tools::scripted() {
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

/// The command line that opens this app again at the next sign-in, when there is one (Windows).
fn reopen_command() -> Option<String> {
    #[cfg(windows)]
    {
        windows::reopen_command()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
mod windows {
    #[cfg(test)]
    use crate::platform::sys::{Hive, registry_value_exists};
    use crate::platform::sys::{delete_user_registry_value, set_user_registry_string};

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
