//! Vagrant on Hyper-V, on Windows: run by a generated PowerShell script, elevated when needed
//! (issue #185).
//!
//! Hyper-V management needs an administrator (or a member of "Hyper-V Administrators"), and
//! Vagrant's Hyper-V provider refuses to run otherwise. A launcher that is neither asks Windows
//! for elevation (UAC, `ShellExecuteEx` with the `runas` verb) once per batch of Vagrant
//! commands, never for status reads.
//!
//! An elevated process can't inherit pipes (or the environment) from a non-elevated parent, so a
//! batch on Hyper-V runs as a Windows PowerShell script (passed with `-EncodedCommand`: no script
//! file, no execution policy), elevated or not:
//! - it reads the environment the commands need from a file in the batch's own folder (under the
//!   user's temp folder) and deletes it, so values (tokens) never sit on a command line;
//! - it picks the Hyper-V virtual switch the VMs' first adapter goes on, unless the user named
//!   one ([`super::vagrant::HYPERV_SWITCH_ENV`], read by the lab's Isoloom Vagrantfile and the
//!   launcher's own), so Vagrant doesn't stop to ask;
//! - it runs each step, appending its stdout and stderr lines to a log file (UTF-8), framed by
//!   marker lines carrying a random token (`start i`, `end i <exit code>`, `skip i`);
//! - a watcher (a runspace of the script's own process) ends the whole process tree when the
//!   batch folder gets a `cancel` file (the caller gave up, or Vagrant asked a question nobody can
//!   answer), when the launcher process is gone, or after [`DEADLINE`], and says why in a `reason`
//!   file.
//!
//! The launcher follows that log as it grows ([`follow_batch`]), turns it back into the same events
//! a batch run step by step reports, and waits for the process. Declining the UAC prompt
//! (`ERROR_CANCELLED`) gives a plain-language error ([`launch_error`]).

#[cfg(any(windows, test))]
mod collect;
#[cfg(any(windows, test))]
mod script;

#[cfg(any(windows, test))]
use std::path::Path;

#[cfg(any(windows, test))]
use self::collect::{WorkDir, follow_batch};
#[cfg(any(windows, test))]
use self::script::{Plan, env_json, forwarded_env, own_env, powershell_args, ps_quote, script};
#[cfg(any(windows, test))]
use super::vagrant::{Event, Outcome, Step};
use crate::error::{Error, Result};

/// How Vagrant runs for a batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    /// As any other tool, one process per step.
    Direct,
    /// Through the PowerShell script; `runas`: elevated through a UAC prompt.
    Script { runas: bool },
}

/// How Vagrant runs for machines on Hyper-V (`hyperv`) or not.
pub(super) fn mode(hyperv: bool) -> Mode {
    // Never in unit tests: they answer from the scripted tools, and must never raise a prompt.
    decide(cfg!(windows) && !cfg!(test), hyperv, may_manage_hyperv)
}

/// The decision itself: only Hyper-V on Windows goes through the script, elevated only when this
/// process may not manage Hyper-V already (`may_manage`: elevated, or a Hyper-V administrator,
/// which Vagrant accepts too).
fn decide(windows: bool, hyperv: bool, may_manage: impl FnOnce() -> bool) -> Mode {
    if !(windows && hyperv) {
        return Mode::Direct;
    }
    Mode::Script { runas: !may_manage() }
}

/// Whether this process runs with an administrator (elevated) token.
#[cfg(windows)]
fn is_elevated() -> bool {
    static ELEVATED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ELEVATED.get_or_init(crate::platform::sys::is_elevated)
}

#[cfg(not(windows))]
fn is_elevated() -> bool {
    false
}

/// Whether this user is in "Hyper-V Administrators" (S-1-5-32-578).
#[cfg(windows)]
fn in_hyperv_admins() -> bool {
    static MEMBER: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *MEMBER.get_or_init(crate::platform::sys::in_hyperv_admins)
}

#[cfg(not(windows))]
fn in_hyperv_admins() -> bool {
    false
}

/// Whether this process may manage Hyper-V itself, as Vagrant's Hyper-V provider requires for
/// every command on its machines (`vagrant status` included): elevated, or a member of "Hyper-V
/// Administrators". Never off Windows.
pub fn may_manage_hyperv() -> bool {
    cfg!(windows) && (is_elevated() || in_hyperv_admins())
}

/// The well-known SID of BUILTIN\Hyper-V Administrators (its name is translated).
#[cfg(any(windows, test))]
const HYPERV_ADMINS_SID: &str = "S-1-5-32-578";

/// The PowerShell script adding `account` (`DOMAIN\user`) to Hyper-V Administrators; one already
/// in it is fine. Any other failure ends it with a non-zero exit code.
#[cfg(any(windows, test))]
fn join_hyperv_admins_script(account: &str) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'\ntry {{ Add-LocalGroupMember -SID {} -Member {} }} catch [Microsoft.PowerShell.Commands.MemberExistsException] {{ }}\n",
        ps_quote(HYPERV_ADMINS_SID),
        ps_quote(account)
    )
}

/// What declining the prompt (or another refusal) to join Hyper-V Administrators means.
#[cfg(any(windows, test))]
fn join_error(code: i32) -> Error {
    const ERROR_CANCELLED: i32 = 1223;
    match code {
        ERROR_CANCELLED => Error::Invalid("The Windows permission prompt was declined, so nothing was changed. Try again and choose Yes.".into()),
        _ => Error::Invalid(format!("Windows couldn't add your account to Hyper-V Administrators ({}).", std::io::Error::from_raw_os_error(code))),
    }
}

/// Adds this user to "Hyper-V Administrators", through one UAC prompt (#185). From the next
/// sign-in on, Vagrant drives Hyper-V for this account without administrator rights, status
/// reads included, so the app needn't run as administrator nor ask again for each lab.
#[cfg(windows)]
pub async fn join_hyperv_admins() -> Result<()> {
    let account = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
        (Ok(domain), Ok(user)) => format!("{domain}\\{user}"),
        (_, Ok(user)) => user,
        _ => return Err(Error::Invalid("couldn't tell which Windows account this is".into())),
    };
    let args = powershell_args(&join_hyperv_admins_script(&account))?.join(" ");
    let exe = powershell();
    let dir = std::env::temp_dir();
    let owner = owner_window().unwrap_or(0);
    let code =
        tokio::task::spawn_blocking(move || crate::platform::sys::launch_elevated(&exe, &args, &dir, owner).map(crate::platform::sys::ElevatedProcess::wait))
            .await
            .map_err(|e| Error::Invalid(e.to_string()))?
            .map_err(join_error)?;
    if code != 0 {
        return Err(Error::Invalid(format!("Windows couldn't add your account to Hyper-V Administrators (PowerShell exited with {code}).")));
    }
    Ok(())
}

#[cfg(not(windows))]
pub async fn join_hyperv_admins() -> Result<()> {
    Err(Error::Invalid("Hyper-V is only on Windows.".into()))
}

/// The environment variable a deploy worker gets the app's main window in (see
/// [`set_owner_window`]).
#[cfg(any(windows, test))]
pub const OWNER_WINDOW_ENV: &str = "CYBERCTF_OWNER_WINDOW";

#[cfg(any(windows, test))]
static OWNER_WINDOW: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// The app's main window (a Windows `HWND`): the UAC prompt is raised for it, so it comes to the
/// front instead of waiting as a flashing taskbar button behind a windowless process.
#[cfg(any(windows, test))]
pub fn set_owner_window(hwnd: isize) {
    OWNER_WINDOW.store(hwnd, std::sync::atomic::Ordering::Relaxed);
}

/// The window UAC prompts are raised for: the one set here, else the one a deploy worker was
/// started with.
#[cfg(any(windows, test))]
pub fn owner_window() -> Option<isize> {
    let set = OWNER_WINDOW.load(std::sync::atomic::Ordering::Relaxed);
    if set != 0 {
        return Some(set);
    }
    std::env::var(OWNER_WINDOW_ENV).ok().and_then(|v| v.parse().ok()).filter(|h| *h != 0)
}

/// What Windows' answer to the elevation request means, as an error.
#[cfg(any(windows, test))]
fn launch_error(code: i32) -> Error {
    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_CANCELLED: i32 = 1223;
    match code {
        ERROR_CANCELLED => Error::Invalid(DECLINED.into()),
        ERROR_FILE_NOT_FOUND => Error::ToolMissing { tool: "powershell" },
        _ => Error::Invalid(format!(
            "Windows could not start Vagrant with administrator rights for Hyper-V ({}). Start Cyber CTF as administrator and try again.",
            std::io::Error::from_raw_os_error(code)
        )),
    }
}

/// The error when the user answers No to the UAC prompt.
#[cfg(any(windows, test))]
const DECLINED: &str = "Hyper-V needs administrator rights, and the Windows permission prompt was declined, so Vagrant did not run. \
Try again and choose Yes, or start Cyber CTF as administrator.";

/// What a batch runs: `program` (Vagrant) in `dir`, with `env` added, step by step.
#[cfg(any(windows, test))]
struct Batch<'a> {
    program: &'a str,
    dir: &'a Path,
    env: &'a [(String, String)],
    steps: &'a [Step],
}

/// Runs `batch` through `launch`, which starts PowerShell with the given arguments and resolves
/// to its exit code once it ended.
#[cfg(any(windows, test))]
async fn run_script<L, F>(batch: &Batch<'_>, launch: L, on_event: impl FnMut(Event)) -> Result<Vec<Outcome>>
where
    L: FnOnce(Vec<String>) -> F,
    F: std::future::Future<Output = Result<u32>>,
{
    let mut work = WorkDir::new()?;
    std::fs::File::create(work.log())?;
    std::fs::write(work.env(), env_json(&forwarded_env(own_env(), batch.env)))?;
    let marker = format!("##cyberctf-{:016x}##", rand::random::<u64>());
    let plan = Plan { batch, work: &work, marker: &marker, launcher: std::process::id() };
    let args = powershell_args(&script(&plan)?)?;
    follow_batch(&mut work, &marker, batch.steps, launch(args), on_event).await
}

/// Starts PowerShell (`exe`) with `args` as a plain child; its exit code once it ended.
#[cfg(any(windows, test))]
async fn launch_plain(exe: &std::ffi::OsStr, args: Vec<String>) -> Result<u32> {
    let mut cmd = tokio::process::Command::new(exe);
    // `-WindowStyle` is Windows only (PowerShell 7 elsewhere refuses it).
    let windows_only = |a: &&String| cfg!(windows) || (*a != "-WindowStyle" && *a != "Hidden");
    cmd.args(args.iter().filter(windows_only)).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    super::headless(&mut cmd);
    let status = cmd.status().await.map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Error::ToolMissing { tool: "powershell" },
        _ => Error::Io(e),
    })?;
    Ok(status.code().map_or(1, i32::cast_unsigned))
}

/// Runs `steps` with Vagrant in `dir` through the script: elevated (`runas`, one UAC prompt for
/// the whole batch) or not.
#[cfg(windows)]
pub(super) async fn batch(dir: &Path, env: &[(String, String)], steps: &[Step], runas: bool, on_event: impl FnMut(Event)) -> Result<Vec<Outcome>> {
    let program = script::resolve("vagrant.exe", std::env::var_os("PATH").as_deref()).map_or_else(|| "vagrant".into(), |p| p.display().to_string());
    let batch = Batch { program: &program, dir, env, steps };
    let exe = powershell();
    if !runas {
        return run_script(&batch, move |args| async move { launch_plain(exe.as_os_str(), args).await }, on_event).await;
    }
    let directory = dir.to_path_buf();
    let owner = owner_window().unwrap_or(0);
    let launch = move |args: Vec<String>| async move {
        // The prompt blocks until it is answered; waiting blocks until the commands end. A
        // process given up on is closed when dropped (its watcher ends it).
        let process = tokio::task::spawn_blocking(move || crate::platform::sys::launch_elevated(&exe, &args.join(" "), &directory, owner))
            .await
            .map_err(|e| Error::Invalid(e.to_string()))?
            .map_err(launch_error)?;
        tokio::task::spawn_blocking(move || process.wait()).await.map_err(|e| Error::Invalid(e.to_string()))
    };
    run_script(&batch, launch, on_event).await
}

/// Windows PowerShell, by its full path.
#[cfg(windows)]
fn powershell() -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    Path::new(&root).join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_hyperv_on_windows_goes_through_the_script_elevated_when_it_must() {
        assert_eq!(decide(true, true, || false), Mode::Script { runas: true });
        assert_eq!(decide(true, true, || true), Mode::Script { runas: false }, "an admin (or Hyper-V admin) runs it without a prompt");
        assert_eq!(decide(true, false, || false), Mode::Direct, "other providers never need it");
        assert_eq!(decide(false, true, || false), Mode::Direct, "never off Windows");
        assert_eq!(mode(true), Mode::Direct, "never in unit tests");
    }

    #[test]
    fn joining_hyperv_administrators_adds_this_account_by_the_groups_sid() {
        // #185: the group's name is translated (Administrateurs Hyper-V), its SID isn't.
        let script = join_hyperv_admins_script(r"PC\Jo O'Brien");
        assert!(script.contains(r"Add-LocalGroupMember -SID 'S-1-5-32-578' -Member 'PC\Jo O''Brien'"), "{script}");
        assert!(script.contains("MemberExistsException") && script.starts_with("$ErrorActionPreference = 'Stop'"), "{script}");
        assert!(join_error(1223).to_string().contains("declined"));
        assert!(join_error(5).to_string().contains("Hyper-V Administrators"));
        assert!(!may_manage_hyperv() || cfg!(windows), "never off Windows");
    }

    #[test]
    fn a_declined_prompt_says_so_in_plain_words() {
        let e = launch_error(1223).to_string();
        assert!(e.contains("declined") && e.contains("administrator") && !e.contains("1223"), "{e}");
        assert!(matches!(launch_error(2), Error::ToolMissing { tool: "powershell" }));
        assert!(launch_error(5).to_string().contains("administrator rights"));
    }

    #[test]
    fn the_owner_window_comes_from_the_app_or_the_workers_environment() {
        // No window set in this test process, nor in its environment.
        assert_eq!(owner_window(), None);
        set_owner_window(42);
        assert_eq!(owner_window(), Some(42));
        set_owner_window(0);
    }

    /// The PowerShell that runs the scripts here: Windows PowerShell on Windows; elsewhere
    /// PowerShell 7 when `CYBERCTF_PWSH` names it (the script is the same).
    fn powershell() -> Option<std::ffi::OsString> {
        if cfg!(windows) { Some("powershell".into()) } else { std::env::var_os("CYBERCTF_PWSH") }
    }

    /// Runs `steps` with `program` through a real PowerShell (not elevated); its lines too.
    async fn real(ps: std::ffi::OsString, program: &str, dir: &Path, steps: &[Step]) -> (Vec<String>, Result<Vec<Outcome>>) {
        let mut lines = Vec::new();
        let env = [("CYBERCTF_X".to_string(), "it's".to_string())];
        let batch = Batch { program, dir, env: &env, steps };
        let result = run_script(
            &batch,
            move |args| async move { launch_plain(&ps, args).await },
            |e| {
                if let Event::Line(i, l) = e {
                    lines.push(format!("{i} {}", l.trim()));
                }
            },
        )
        .await;
        (lines, result)
    }

    #[tokio::test]
    async fn a_real_powershell_stops_on_a_missing_folder() {
        let Some(ps) = powershell() else { return };
        let program = if cfg!(windows) { "cmd.exe" } else { "sh" };
        let missing = std::env::temp_dir().join(format!("cyberctf-missing-{}", rand::random::<u32>()));
        let (_, result) = real(ps, program, &missing, &[Step::new(&["-c", "echo ran"])]).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("the Vagrant batch failed") && err.contains("cyberctf-missing-"), "{err}");
    }

    #[tokio::test]
    async fn a_real_powershell_watcher_ends_the_whole_batch_stuck_on_a_question() {
        // The generated watcher, run by a real PowerShell, ends the run's whole tree once asked
        // (the step would wait a minute). How soon it is asked is proven on a fake clock
        // (`collect::tests`); this proves it acts, so it asserts no time.
        let Some(ps) = powershell() else { return };
        let (program, step) = if cfg!(windows) {
            ("cmd.exe", Step::new(&["/c", "echo Please choose a switch to attach to your Hyper-V instance.& ping -n 60 127.0.0.1 >nul"]))
        } else {
            ("sh", Step::new(&["-c", "echo 'Please choose a switch to attach to your Hyper-V instance.'; sleep 60"]))
        };
        let (_, result) = real(ps, program, &std::env::temp_dir(), &[step]).await;
        let err = result.expect_err("the watcher ended the step before it finished").to_string();
        assert!(err.contains("which Hyper-V virtual switch"), "{err}");
    }
}
