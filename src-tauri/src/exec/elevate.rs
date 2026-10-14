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
//! - a watcher (a background job) ends the whole process tree when the batch folder gets a
//!   `cancel` file (the caller gave up, or Vagrant asked a question nobody can answer), when the
//!   launcher process is gone, or after [`DEADLINE`], and says why in a `reason` file.
//!
//! The launcher follows that log as it grows ([`follow`]), turns it back into the same events a
//! batch run step by step reports ([`Collector`]), and waits for the process. Declining the UAC
//! prompt (`ERROR_CANCELLED`) gives a plain-language error ([`launch_error`]).

#![cfg_attr(not(windows), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::vagrant::{Event, HYPERV_SWITCH_ENV, Outcome, Step, When};
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
    *ELEVATED.get_or_init(win::is_elevated)
}

#[cfg(not(windows))]
fn is_elevated() -> bool {
    false
}

/// Whether this user is in "Hyper-V Administrators" (S-1-5-32-578).
#[cfg(windows)]
fn in_hyperv_admins() -> bool {
    static MEMBER: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *MEMBER.get_or_init(win::in_hyperv_admins)
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
const HYPERV_ADMINS_SID: &str = "S-1-5-32-578";

/// The PowerShell script adding `account` (`DOMAIN\user`) to Hyper-V Administrators; one already
/// in it is fine. Any other failure ends it with a non-zero exit code.
fn join_hyperv_admins_script(account: &str) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'\ntry {{ Add-LocalGroupMember -SID {} -Member {} }} catch [Microsoft.PowerShell.Commands.MemberExistsException] {{ }}\n",
        ps_quote(HYPERV_ADMINS_SID),
        ps_quote(account)
    )
}

/// What declining the prompt (or another refusal) to join Hyper-V Administrators means.
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
    let exe = win::powershell();
    let dir = std::env::temp_dir();
    let owner = owner_window().unwrap_or(0);
    let code = tokio::task::spawn_blocking(move || win::launch(&exe, &args, &dir, owner).map(|p| p.wait()))
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
pub const OWNER_WINDOW_ENV: &str = "CYBERCTF_OWNER_WINDOW";

static OWNER_WINDOW: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// The app's main window (a Windows `HWND`): the UAC prompt is raised for it, so it comes to the
/// front instead of waiting as a flashing taskbar button behind a windowless process.
pub fn set_owner_window(hwnd: isize) {
    OWNER_WINDOW.store(hwnd, std::sync::atomic::Ordering::Relaxed);
}

/// The window UAC prompts are raised for: the one set here, else the one a deploy worker was
/// started with.
pub fn owner_window() -> Option<isize> {
    let set = OWNER_WINDOW.load(std::sync::atomic::Ordering::Relaxed);
    if set != 0 {
        return Some(set);
    }
    std::env::var(OWNER_WINDOW_ENV).ok().and_then(|v| v.parse().ok()).filter(|h| *h != 0)
}

/// What Windows' answer to the elevation request means, as an error.
pub(super) fn launch_error(code: i32) -> Error {
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
const DECLINED: &str = "Hyper-V needs administrator rights, and the Windows permission prompt was declined, so Vagrant did not run. \
Try again and choose Yes, or start Cyber CTF as administrator.";

/// The longest command line Windows accepts, in UTF-16 units.
const MAX_COMMAND_LINE: usize = 32_000;

/// The longest a batch may run before the watcher ends it: a big lab's first boot takes a couple
/// of hours at most; past this, it is stuck.
const DEADLINE: Duration = Duration::from_secs(6 * 3600);

/// A question Vagrant waits for an answer to (on Windows its `ask` reads the console even without
/// a terminal, so it would wait forever), told by a line printed just before it, and what to
/// say instead.
const PROMPTS: &[(&str, &str)] = &[
    (
        "Please choose a switch to attach to your Hyper-V instance",
        "Vagrant stopped to ask which Hyper-V virtual switch to use (this machine has several), which the launcher can't answer. \
Set ISOLOOM_HYPERV_SWITCH to the name of one (Get-VMSwitch lists them), or keep a single virtual switch in Hyper-V Manager, then try again.",
    ),
    (
        "You will be asked for the username and password to use for the SMB",
        "Vagrant stopped to ask for a Windows username and password, to share a folder with a Hyper-V VM over SMB, which the launcher can't answer. \
Turn the shared folder off for this lab, then try again.",
    ),
];

/// What to say instead of a question Vagrant asks on `line`, if it is one.
fn prompt(line: &str) -> Option<&'static str> {
    PROMPTS.iter().find(|(needle, _)| line.contains(needle)).map(|(_, say)| *say)
}

/// Why the watcher ended a batch, from its `reason` file, as a sentence.
fn stopped(reason: &str) -> String {
    match reason.trim() {
        "timeout" => format!("Vagrant was stopped: it ran for more than {} hours.", DEADLINE.as_secs() / 3600),
        "gone" => "Vagrant was stopped: the launcher that started it exited.".into(),
        "" | "cancelled" => "Vagrant was stopped: the operation was cancelled.".into(),
        other => other.to_string(),
    }
}

/// Refuses an argument Windows PowerShell 5.1 can't pass through to a native program intact:
/// it drops empty ones, mangles embedded double quotes, and stops parsing at `--%`.
fn check_arg(arg: &str) -> Result<()> {
    if arg.is_empty() || arg.contains(['"', '\0', '\n', '\r']) || arg == "--%" {
        return Err(Error::Invalid(format!("refusing to pass `{}` to Vagrant through Windows PowerShell: it can't pass it intact", arg.escape_debug())));
    }
    Ok(())
}

/// `s` as a PowerShell single-quoted string. PowerShell also takes the typographic single quotes
/// as quote characters, so every one of them is doubled too.
fn ps_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// The environment the commands get: an elevated process starts from the account's stored
/// environment, not this one. `PATH` (tools found here are found there) and every `VAGRANT_*`
/// variable carry over, `VAGRANT_HOME` is pinned to this user's (so the boxes are the ones this
/// user downloaded, whichever administrator approved), then the caller's `env`.
fn forwarded_env(own: impl IntoIterator<Item = (String, String)>, env: &[(String, String)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut profile = None;
    for (k, v) in own {
        let upper = k.to_ascii_uppercase();
        if upper == "USERPROFILE" {
            profile = Some(v.clone());
        }
        if upper == "PATH" || upper.starts_with("VAGRANT_") || upper == HYPERV_SWITCH_ENV {
            out.push((k, v));
        }
    }
    if !out.iter().any(|(k, _)| k.eq_ignore_ascii_case("VAGRANT_HOME"))
        && let Some(p) = profile
    {
        out.push(("VAGRANT_HOME".into(), format!("{}\\.vagrant.d", p.trim_end_matches(['\\', '/']))));
    }
    for (k, v) in env {
        out.retain(|(o, _)| !o.eq_ignore_ascii_case(k));
        out.push((k.clone(), v.clone()));
    }
    out
}

/// This process's environment, without the variables that aren't Unicode (none the commands
/// need).
fn own_env() -> impl Iterator<Item = (String, String)> {
    std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
}

/// The environment file the script reads: a JSON list of `{k, v}`.
fn env_json(env: &[(String, String)]) -> String {
    serde_json::Value::Array(env.iter().map(|(k, v)| serde_json::json!({ "k": k, "v": v })).collect()).to_string()
}

/// The first `name` in the folders of `path` (a `PATH` value): the program the script runs, by
/// its full path, so it doesn't depend on the elevated account's own `PATH`.
fn resolve(name: &str, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?).map(|d| d.join(name)).find(|p| p.is_file())
}

/// What a batch's script needs.
struct Plan<'a> {
    program: &'a str,
    dir: &'a Path,
    steps: &'a [Step],
    work: &'a WorkDir,
    marker: &'a str,
    /// The launcher's process id: its end ends the batch.
    launcher: u32,
}

/// The PowerShell script that runs a [`Plan`], logging with the marker lines of [`Collector`].
fn script(p: &Plan) -> Result<String> {
    for arg in p.steps.iter().flat_map(|s| &s.args) {
        check_arg(arg)?;
    }
    let m = ps_quote(p.marker);
    let mut s = script_log(p);
    s.push_str(&script_watcher(p));
    s.push_str("$script:cyberctfFailed = $false\n$cyberctfCode = 0\ntry {\n");
    s.push_str(&script_setup(p));
    s.push_str(&script_step_function(p, &m));
    s.push_str(&script_steps(p));
    s.push_str(&script_end(&m));
    Ok(s)
}

/// A path, quoted for PowerShell.
fn ps_path(path: &Path) -> String {
    ps_quote(&path.display().to_string())
}

/// The script's log: a UTF-8 file shared with the launcher, and `Write-CyberctfLine`.
fn script_log(p: &Plan) -> String {
    let mut s = String::new();
    // Native stderr lines must stay plain lines (with `Stop`, the first one would end the script).
    s.push_str("$ErrorActionPreference = 'Continue'\n$ProgressPreference = 'SilentlyContinue'\n");
    s.push_str("try { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding -ArgumentList $false } catch {}\n");
    s.push_str(&format!(
        "$cyberctfFile = [System.IO.File]::Open({}, [System.IO.FileMode]::Append, [System.IO.FileAccess]::Write, [System.IO.FileShare]::ReadWrite)\n",
        ps_path(&p.work.log())
    ));
    s.push_str("$cyberctfLog = New-Object System.IO.StreamWriter -ArgumentList $cyberctfFile, (New-Object System.Text.UTF8Encoding -ArgumentList $false)\n");
    s.push_str("$cyberctfLog.AutoFlush = $true\n");
    s.push_str("function Write-CyberctfLine([string]$t) { $cyberctfLog.WriteLine($t) }\n");
    s
}

/// The watcher: a background job (its own process) that ends this script's whole tree on cancel,
/// when the launcher is gone, or at the deadline.
fn script_watcher(p: &Plan) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "$cyberctfWatch = Start-Job -ArgumentList $PID, {}, {}, {}, {} -ScriptBlock {{\n\
         \x20 param($main, $launcherId, $cancel, $reason, $secs)\n\
         \x20 $launcher = Get-Process -Id $launcherId -ErrorAction SilentlyContinue\n\
         \x20 $deadline = (Get-Date).AddSeconds($secs)\n\
         \x20 while ($true) {{\n\
         \x20   $why = $null\n\
         \x20   if (Test-Path -LiteralPath $cancel) {{ $why = [System.IO.File]::ReadAllText($cancel) }}\n\
         \x20   elseif ($null -ne $launcher -and $launcher.HasExited) {{ $why = 'gone' }}\n\
         \x20   elseif ((Get-Date) -gt $deadline) {{ $why = 'timeout' }}\n\
         \x20   if ($null -ne $why) {{\n\
         \x20     [System.IO.File]::WriteAllText($reason, $why)\n\
         \x20     if ($IsLinux -or $IsMacOS) {{ Stop-Process -Id $main -Force }} else {{ & taskkill.exe /T /F /PID $main | Out-Null }}\n\
         \x20     return\n\
         \x20   }}\n\
         \x20   Start-Sleep -Milliseconds 500\n\
         \x20 }}\n\
         }}\n",
        p.launcher,
        ps_path(&p.work.cancel()),
        ps_path(&p.work.reason()),
        DEADLINE.as_secs()
    ));
    s
}

/// The environment, the working folder and the Hyper-V switch.
fn script_setup(p: &Plan) -> String {
    let mut s = String::new();
    // The environment, from a file read once and deleted, so no value sits on a command line.
    s.push_str(&format!(
        "  $cyberctfPairs = [System.IO.File]::ReadAllText({env}) | ConvertFrom-Json\n  Remove-Item -LiteralPath {env} -Force\n  foreach ($p in $cyberctfPairs) {{ [System.Environment]::SetEnvironmentVariable($p.k, $p.v, 'Process') }}\n",
        env = ps_path(&p.work.env())
    ));
    s.push_str(&format!("  Set-Location -LiteralPath {} -ErrorAction Stop\n", ps_path(p.dir)));
    // The switch the VMs' first adapter goes on, unless the user named one: the built-in "Default
    // Switch", else an external one, else the first. Isoloom's Vagrantfile would take the Default
    // Switch, and stop on a host without one (Windows Server); the launcher's attach to it.
    s.push_str(&format!(
        "  if (-not $env:{HYPERV_SWITCH_ENV}) {{\n    try {{\n      $sw = @(Get-VMSwitch -ErrorAction Stop)\n      $pick = @($sw | Where-Object {{ $_.Name -eq 'Default Switch' }}) + @($sw | Where-Object {{ \"$($_.SwitchType)\" -eq 'External' }}) + $sw | Select-Object -First 1\n      if ($pick) {{ $env:{HYPERV_SWITCH_ENV} = $pick.Name }}\n    }} catch {{}}\n  }}\n"
    ));
    s
}

/// `Invoke-CyberctfStep`, which runs one step between its marker lines.
fn script_step_function(p: &Plan, m: &str) -> String {
    let mut s = String::new();
    // One function runs a step, so each step is one short line (the whole script must fit on a
    // command line, base64-encoded).
    s.push_str(&format!(
        "  function Invoke-CyberctfStep([int]$i, [bool]$always, [string[]]$a) {{\n\
         \x20   if ($script:cyberctfFailed -and -not $always) {{ Write-CyberctfLine ({m} + ' skip ' + $i); return }}\n\
         \x20   Write-CyberctfLine ({m} + ' start ' + $i)\n\
         \x20   $global:LASTEXITCODE = 0\n\
         \x20   $rc = $null\n\
         \x20   try {{\n\
         \x20     & {} @a 2>&1 | ForEach-Object {{ Write-CyberctfLine ([string]$_) }}\n\
         \x20     $rc = $global:LASTEXITCODE\n\
         \x20   }} catch [System.Management.Automation.CommandNotFoundException] {{\n\
         \x20     Write-CyberctfLine ({m} + ' missing ' + $i)\n\
         \x20     $rc = 127\n\
         \x20   }}\n\
         \x20   if ($null -eq $rc) {{ $rc = 0 }}\n\
         \x20   Write-CyberctfLine ({m} + ' end ' + $i + ' ' + $rc)\n\
         \x20   if ($rc -ne 0) {{ $script:cyberctfFailed = $true }}\n\
         \x20 }}\n",
        ps_quote(p.program)
    ));
    s
}

/// One line per step.
fn script_steps(p: &Plan) -> String {
    let mut s = String::new();
    for (i, step) in p.steps.iter().enumerate() {
        let always = match step.when {
            When::IfOk => "$false",
            When::Always => "$true",
        };
        let args: Vec<String> = step.args.iter().map(|a| ps_quote(a)).collect();
        s.push_str(&format!("  Invoke-CyberctfStep {i} {always} @({})\n", args.join(", ")));
    }
    s
}

/// The end: the error marker, the watcher stopped, the log closed, the exit code.
fn script_end(m: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "}} catch {{\n  Write-CyberctfLine ({m} + ' error ' + $_.Exception.Message)\n  $cyberctfCode = 1\n}} finally {{\n  Stop-Job -Job $cyberctfWatch -ErrorAction SilentlyContinue\n  Remove-Job -Job $cyberctfWatch -Force -ErrorAction SilentlyContinue\n  $cyberctfLog.Close()\n}}\nexit $cyberctfCode\n"
    ));
    s
}

/// The arguments of `powershell.exe` that run `script`.
fn powershell_args(script: &str) -> Result<Vec<String>> {
    use base64::Engine;
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let args: Vec<String> = ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-EncodedCommand"]
        .into_iter()
        .map(String::from)
        .chain([base64::engine::general_purpose::STANDARD.encode(utf16)])
        .collect();
    if args.iter().map(|a| a.len() + 1).sum::<usize>() > MAX_COMMAND_LINE {
        return Err(Error::Invalid("the Vagrant batch is too long for a Windows command line".into()));
    }
    Ok(args)
}

/// Lines kept for a failed step's error, as for a plain stream.
const ERROR_LINES: usize = super::STREAM_ERROR_LINES;

/// Turns a batch's log back into its events and outcomes.
struct Collector<'s> {
    marker: String,
    steps: &'s [Step],
    current: usize,
    text: String,
    tail: std::collections::VecDeque<String>,
    missing: bool,
    outcomes: Vec<Outcome>,
    skipped: Vec<bool>,
    error: Option<String>,
}

impl<'s> Collector<'s> {
    fn new(marker: &str, steps: &'s [Step]) -> Collector<'s> {
        Collector {
            marker: format!("{marker} "),
            steps,
            current: 0,
            text: String::new(),
            tail: Default::default(),
            missing: false,
            outcomes: steps.iter().map(|_| None).collect(),
            skipped: vec![false; steps.len()],
            error: None,
        }
    }

    fn line(&mut self, line: &str, on_event: &mut impl FnMut(Event<'_>)) {
        let Some(rest) = line.strip_prefix(&self.marker) else {
            self.text.push_str(line);
            self.text.push('\n');
            if self.tail.len() == ERROR_LINES {
                self.tail.pop_front();
            }
            self.tail.push_back(line.to_string());
            on_event(Event::Line(self.current, line.to_string()));
            return;
        };
        let mut words = rest.splitn(3, ' ');
        let (kind, index) = (words.next().unwrap_or(""), words.next().and_then(|i| i.parse::<usize>().ok()));
        match (kind, index) {
            ("start", Some(i)) if i < self.steps.len() => {
                self.current = i;
                self.text.clear();
                self.tail.clear();
                self.missing = false;
                on_event(Event::Start(i));
            }
            ("missing", Some(_)) => self.missing = true,
            ("skip", Some(i)) if i < self.steps.len() => self.skipped[i] = true,
            ("end", Some(i)) if i < self.steps.len() => {
                let code = words.next().unwrap_or("").trim().to_string();
                let result = self.result(i, &code);
                on_event(Event::End(i, &result));
                self.outcomes[i] = Some(result);
            }
            ("error", _) => self.error = Some(rest.trim_start_matches("error").trim().to_string()),
            _ => {}
        }
    }

    /// Step `i`'s result from its exit code.
    fn result(&mut self, i: usize, code: &str) -> Result<String> {
        if code == "0" {
            return Ok(std::mem::take(&mut self.text));
        }
        if self.missing {
            return Err(Error::ToolMissing { tool: "vagrant" });
        }
        let args: Vec<&str> = self.steps[i].args.iter().map(String::as_str).collect();
        let tail: Vec<String> = self.tail.iter().cloned().collect();
        Err(super::failed("vagrant", &args, super::stream_failure(&tail, format!("exit code: {code}"))))
    }

    /// The outcomes, once the script exited with `exit_code`; `reason`: why the watcher ended it,
    /// if it did. A step that neither ended nor was skipped (the script was cut short) fails the
    /// whole batch: the steps after it never ran.
    fn finish(self, exit_code: u32, reason: Option<&str>) -> Result<Vec<Outcome>> {
        if let Some(e) = self.error {
            return Err(Error::Invalid(format!("the Vagrant batch failed: {e}")));
        }
        let incomplete = self.outcomes.iter().zip(&self.skipped).any(|(o, skipped)| o.is_none() && !skipped);
        if incomplete || exit_code != 0 && self.outcomes.iter().all(Option::is_none) {
            return Err(Error::Invalid(match reason {
                Some(r) => stopped(r),
                None => format!("the Vagrant batch ended before it finished (exit code {exit_code})"),
            }));
        }
        Ok(self.outcomes)
    }
}

/// How often a followed log is read for new lines.
const FOLLOW_EVERY: Duration = Duration::from_millis(200);

/// Follows the log at `path` while `done` runs, handing each complete line (without its line
/// ending) to `on_line`, then reads what was left once it finished. `done`'s output.
async fn follow<T>(path: &Path, done: impl std::future::Future<Output = T>, mut on_line: impl FnMut(&str)) -> std::io::Result<T> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path).await?;
    let mut pending: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 16 * 1024];
    tokio::pin!(done);
    loop {
        // Everything written so far.
        loop {
            let n = file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            pending.extend_from_slice(&buf[..n]);
            drain(&mut pending, false, &mut on_line);
        }
        tokio::select! {
            out = &mut done => {
                loop {
                    let n = file.read(&mut buf).await?;
                    if n == 0 {
                        break;
                    }
                    pending.extend_from_slice(&buf[..n]);
                }
                drain(&mut pending, true, &mut on_line);
                return Ok(out);
            }
            _ = tokio::time::sleep(FOLLOW_EVERY) => {}
        }
    }
}

/// Hands each complete line in `pending` to `on_line` (and, when `last`, what is left).
fn drain(pending: &mut Vec<u8>, last: bool, on_line: &mut dyn FnMut(&str)) {
    while let Some(at) = pending.iter().position(|b| *b == b'\n') {
        let line: Vec<u8> = pending.drain(..=at).collect();
        on_line(String::from_utf8_lossy(&line).trim_end_matches(['\n', '\r']));
    }
    if last && !pending.is_empty() {
        on_line(String::from_utf8_lossy(pending).trim_end_matches('\r'));
        pending.clear();
    }
}

/// A batch's own folder (its log, environment file, and the watcher's `cancel` and `reason`
/// files). Dropped before the batch finished (its caller gave up), it asks the watcher to end
/// the batch, and is removed once the script has let go of it.
struct WorkDir {
    dir: PathBuf,
    finished: bool,
}

impl WorkDir {
    fn new() -> Result<WorkDir> {
        let dir = std::env::temp_dir().join(format!("cyberctf-vagrant-{:016x}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir)?;
        Ok(WorkDir { dir, finished: false })
    }

    fn log(&self) -> PathBuf {
        self.dir.join("vagrant.log")
    }

    fn env(&self) -> PathBuf {
        self.dir.join("env.json")
    }

    fn cancel(&self) -> PathBuf {
        self.dir.join("cancel")
    }

    fn reason(&self) -> PathBuf {
        self.dir.join("reason")
    }

    /// Asks the watcher to end the batch, saying `why` (kept when asked already).
    fn stop(&self, why: &str) {
        if !self.cancel().exists() {
            let _ = std::fs::write(self.cancel(), why);
        }
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        if self.finished {
            let _ = std::fs::remove_dir_all(&self.dir);
            return;
        }
        self.stop("cancelled");
        // The script may still hold the log open for a moment (or not have started, the prompt
        // still up): try again for a while.
        let dir = self.dir.clone();
        std::thread::spawn(move || {
            for _ in 0..300 {
                if std::fs::remove_dir_all(&dir).is_ok() || !dir.exists() {
                    return;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
    }
}

/// Runs `steps` as `program` through `launch`, which starts PowerShell with the given arguments
/// and resolves to its exit code once it ended.
async fn run_script<L, F>(
    program: &str,
    dir: &Path,
    env: &[(String, String)],
    steps: &[Step],
    launch: L,
    mut on_event: impl FnMut(Event<'_>),
) -> Result<Vec<Outcome>>
where
    L: FnOnce(Vec<String>) -> F,
    F: std::future::Future<Output = Result<u32>>,
{
    let mut work = WorkDir::new()?;
    std::fs::File::create(work.log())?;
    std::fs::write(work.env(), env_json(&forwarded_env(own_env(), env)))?;
    let marker = format!("##cyberctf-{:016x}##", rand::random::<u64>());
    let plan = Plan { program, dir, steps, work: &work, marker: &marker, launcher: std::process::id() };
    let args = powershell_args(&script(&plan)?)?;
    let mut collector = Collector::new(&marker, steps);
    let code = follow(&work.log(), launch(args), |l| {
        if let Some(say) = prompt(l) {
            work.stop(say);
        }
        collector.line(l, &mut on_event)
    })
    .await??;
    let reason = std::fs::read_to_string(work.reason()).ok();
    work.finished = true;
    collector.finish(code, reason.as_deref())
}

/// Starts PowerShell (`exe`) with `args` as a plain child; its exit code once it ended.
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
    Ok(status.code().map(|c| c as u32).unwrap_or(1))
}

/// Runs `steps` with Vagrant in `dir` through the script: elevated (`runas`, one UAC prompt for
/// the whole batch) or not.
#[cfg(windows)]
pub(super) async fn batch(dir: &Path, env: &[(String, String)], steps: &[Step], runas: bool, on_event: impl FnMut(Event<'_>)) -> Result<Vec<Outcome>> {
    let program = resolve("vagrant.exe", std::env::var_os("PATH").as_deref()).map(|p| p.display().to_string()).unwrap_or_else(|| "vagrant".into());
    let exe = win::powershell();
    if !runas {
        return run_script(&program, dir, env, steps, move |args| async move { launch_plain(exe.as_os_str(), args).await }, on_event).await;
    }
    let directory = dir.to_path_buf();
    let owner = owner_window().unwrap_or(0);
    let launch = move |args: Vec<String>| async move {
        // The prompt blocks until it is answered; waiting blocks until the commands end. A
        // process given up on is closed when dropped (its watcher ends it).
        let process = tokio::task::spawn_blocking(move || win::launch(&exe, &args.join(" "), &directory, owner))
            .await
            .map_err(|e| Error::Invalid(e.to_string()))?
            .map_err(launch_error)?;
        tokio::task::spawn_blocking(move || process.wait()).await.map_err(|e| Error::Invalid(e.to_string()))
    };
    run_script(&program, dir, env, steps, launch, on_event).await
}

#[cfg(windows)]
mod win {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
    use windows_sys::Win32::Security::{
        AllocateAndInitializeSid, CheckTokenMembership, FreeSid, GetTokenInformation, PSID, SID_IDENTIFIER_AUTHORITY, TOKEN_ELEVATION, TOKEN_QUERY,
        TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetExitCodeProcess, INFINITE, OpenProcessToken, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};

    /// `SW_HIDE`: the PowerShell keeps its console window hidden.
    const SW_HIDE: i32 = 0;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Windows PowerShell, by its full path.
    pub(super) fn powershell() -> PathBuf {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        Path::new(&root).join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe")
    }

    pub(super) fn is_elevated() -> bool {
        // SAFETY: plain Win32 calls on a token handle owned (and closed) here, filling a struct
        // of the size given.
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut len = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                &mut elevation as *mut _ as *mut core::ffi::c_void,
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            ) != 0;
            CloseHandle(token);
            ok && elevation.TokenIsElevated != 0
        }
    }

    /// Whether this process's token is in BUILTIN\Hyper-V Administrators (S-1-5-32-578).
    pub(super) fn in_hyperv_admins() -> bool {
        const SECURITY_NT_AUTHORITY: SID_IDENTIFIER_AUTHORITY = SID_IDENTIFIER_AUTHORITY { Value: [0, 0, 0, 0, 0, 5] };
        const SECURITY_BUILTIN_DOMAIN_RID: u32 = 32;
        const DOMAIN_ALIAS_RID_HYPER_V_ADMINS: u32 = 578;
        // SAFETY: the SID is allocated, checked against this thread's (or process's) token, and
        // freed here.
        unsafe {
            let mut sid: PSID = std::ptr::null_mut();
            if AllocateAndInitializeSid(&SECURITY_NT_AUTHORITY, 2, SECURITY_BUILTIN_DOMAIN_RID, DOMAIN_ALIAS_RID_HYPER_V_ADMINS, 0, 0, 0, 0, 0, 0, &mut sid)
                == 0
            {
                return false;
            }
            let mut member = 0;
            let ok = CheckTokenMembership(std::ptr::null_mut(), sid, &mut member) != 0;
            FreeSid(sid);
            ok && member != 0
        }
    }

    /// The elevated PowerShell process; its handle is closed when dropped.
    pub(super) struct Process(usize);

    impl Process {
        /// Waits for it to end; its exit code.
        pub(super) fn wait(self) -> u32 {
            let mut code = 1u32;
            // SAFETY: the live process handle ShellExecuteExW returned (closed on drop).
            unsafe {
                WaitForSingleObject(self.0 as HANDLE, INFINITE);
                if GetExitCodeProcess(self.0 as HANDLE, &mut code) == 0 {
                    code = 1;
                }
            }
            code
        }
    }

    impl Drop for Process {
        fn drop(&mut self) {
            // SAFETY: the handle is ours and closed once.
            unsafe {
                CloseHandle(self.0 as HANDLE);
            }
        }
    }

    /// Starts PowerShell (`exe`) elevated with `args` (UAC asks first, for the window `owner` when
    /// not 0). The OS error code when it couldn't (1223: the prompt was declined).
    pub(super) fn launch(exe: &Path, args: &str, dir: &Path, owner: isize) -> std::result::Result<Process, i32> {
        let (verb, file, params, directory) = (wide("runas".as_ref()), wide(exe.as_os_str()), wide(args.as_ref()), wide(dir.as_os_str()));
        // SAFETY: a zeroed SHELLEXECUTEINFOW is valid (null pointers, no flags); every string
        // it points to outlives the call.
        unsafe {
            let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
            info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
            info.hwnd = owner as _;
            info.lpVerb = verb.as_ptr();
            info.lpFile = file.as_ptr();
            info.lpParameters = params.as_ptr();
            info.lpDirectory = directory.as_ptr();
            info.nShow = SW_HIDE;
            if ShellExecuteExW(&mut info) == 0 {
                return Err(GetLastError() as i32);
            }
            if info.hProcess.is_null() {
                return Err(1);
            }
            Ok(Process(info.hProcess as usize))
        }
    }
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

    #[test]
    fn questions_vagrant_waits_on_are_told_and_explained() {
        let switch = prompt("    default: Please choose a switch to attach to your Hyper-V instance.").unwrap();
        assert!(switch.contains("virtual switch") && switch.contains("try again"));
        assert!(prompt("You will be asked for the username and password to use for the SMB").unwrap().contains("shared folder"));
        assert!(prompt("==> default: Booting VM...").is_none());
        assert!(stopped("timeout").contains("6 hours"));
        assert!(stopped("cancelled").contains("cancelled") && stopped("").contains("cancelled"));
        assert_eq!(stopped(switch), switch);
    }

    #[test]
    fn powershell_quoting_doubles_every_single_quote() {
        assert_eq!(ps_quote("plain"), "'plain'");
        assert_eq!(ps_quote("it's"), "'it''s'");
        assert_eq!(ps_quote("a\u{2019}b"), "'a\u{2019}\u{2019}b'");
        assert_eq!(ps_quote("$env:X `n"), "'$env:X `n'", "nothing expands in single quotes");
    }

    fn work() -> WorkDir {
        WorkDir { dir: PathBuf::from("C:\\Temp\\w"), finished: true }
    }

    fn plan<'a>(steps: &'a [Step], work: &'a WorkDir) -> Plan<'a> {
        Plan { program: "C:\\HashiCorp\\Vagrant\\bin\\vagrant.exe", dir: Path::new("C:\\Users\\Amélie\\lab"), steps, work, marker: "##m##", launcher: 77 }
    }

    #[test]
    fn arguments_powershell_would_mangle_are_refused() {
        for bad in ["", "say \"hi\"", "--%", "a\nb"] {
            assert!(check_arg(bad).is_err(), "{bad:?}");
        }
        for good in ["up", "--provider", "hyperv", "uname -srm", "isoloom-controller", "a&b|c", "echo $(uname)"] {
            assert!(check_arg(good).is_ok(), "{good:?}");
        }
        let steps = [Step::new(&["ssh", "-c", "echo \"x\""])];
        let w = work();
        assert!(script(&plan(&steps, &w)).is_err());
    }

    #[test]
    fn the_environment_carries_path_vagrant_and_the_callers_and_stays_off_the_command_line() {
        let own = [
            ("Path".to_string(), "C:\\HashiCorp\\Vagrant\\bin".to_string()),
            ("USERPROFILE".into(), "C:\\Users\\Amélie\\".into()),
            ("VAGRANT_DEFAULT_PROVIDER".into(), "hyperv".into()),
            ("TEMP".into(), "C:\\Users\\Amélie\\AppData\\Local\\Temp".into()),
        ];
        let env = forwarded_env(own.clone(), &[("CTF_LAUNCH_TOKEN".into(), "s3cret".into()), ("vagrant_default_provider".into(), "virtualbox".into())]);
        assert_eq!(
            env,
            [
                ("Path".to_string(), "C:\\HashiCorp\\Vagrant\\bin".to_string()),
                ("VAGRANT_HOME".into(), "C:\\Users\\Amélie\\.vagrant.d".into()),
                ("CTF_LAUNCH_TOKEN".into(), "s3cret".into()),
                ("vagrant_default_provider".into(), "virtualbox".into()),
            ]
        );
        let json: serde_json::Value = serde_json::from_str(&env_json(&env)).unwrap();
        assert_eq!(json[2], serde_json::json!({ "k": "CTF_LAUNCH_TOKEN", "v": "s3cret" }));
        // A VAGRANT_HOME of the user's own is kept as it is.
        let mut own = own.to_vec();
        own.push(("VAGRANT_HOME".into(), "D:\\vagrant".into()));
        let env = forwarded_env(own, &[]);
        assert_eq!(env.iter().filter(|(k, _)| k == "VAGRANT_HOME").map(|(_, v)| v.as_str()).collect::<Vec<_>>(), ["D:\\vagrant"]);
        // A switch the user named for Isoloom is the one the VMs get.
        let env = forwarded_env([("isoloom_hyperv_switch".to_string(), "Lab NAT".to_string())], &[]);
        assert_eq!(env, [("isoloom_hyperv_switch".to_string(), "Lab NAT".to_string())]);
        // The script names the file, never a value.
        let steps = [Step::new(&["up"])];
        let w = work();
        let s = script(&plan(&steps, &w)).unwrap();
        assert!(s.contains("ReadAllText('C:\\Temp\\w") && s.contains("env.json") && !s.contains("s3cret"), "{s}");
    }

    #[test]
    fn the_program_is_found_by_its_full_path() {
        let d = std::env::temp_dir().join(format!("cyberctf-resolve-{}", rand::random::<u32>()));
        let (a, b) = (d.join("a"), d.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("vagrant.exe"), "").unwrap();
        let path = std::env::join_paths([&a, &b]).unwrap();
        assert_eq!(resolve("vagrant.exe", Some(&path)), Some(b.join("vagrant.exe")));
        assert_eq!(resolve("vagrant.exe", Some(&std::env::join_paths([&a]).unwrap())), None);
        assert_eq!(resolve("vagrant.exe", None), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_script_runs_every_step_quoted_and_framed_by_markers() {
        let steps = [Step::new(&["up", "--provider", "hyperv"]), Step::new(&["halt", "o'brien"]).always()];
        let w = work();
        let s = script(&plan(&steps, &w)).unwrap();
        assert!(s.contains("Set-Location -LiteralPath 'C:\\Users\\Amélie\\lab' -ErrorAction Stop"), "{s}");
        assert!(s.contains("& 'C:\\HashiCorp\\Vagrant\\bin\\vagrant.exe' @a 2>&1"), "{s}");
        assert!(s.contains("  Invoke-CyberctfStep 0 $false @('up', '--provider', 'hyperv')\n"), "{s}");
        assert!(s.contains("  Invoke-CyberctfStep 1 $true @('halt', 'o''brien')\n"), "a clean-up step always runs: {s}");
        assert!(s.contains("[System.IO.File]::Open('C:\\Temp\\w"), "{s}");
        assert!(s.contains("Start-Job -ArgumentList $PID, 77, 'C:\\Temp\\w") && s.contains("taskkill.exe /T /F /PID $main"), "the watcher: {s}");
        assert!(s.contains(&format!("$env:{HYPERV_SWITCH_ENV} = $pick.Name")), "{s}");
        assert!(s.trim_end().ends_with("exit $cyberctfCode"));
    }

    #[test]
    fn powershell_gets_the_script_utf16_encoded() {
        use base64::Engine;
        let args = powershell_args("Write-Output 'é'").unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(args.last().unwrap()).unwrap();
        let units: Vec<u16> = bytes.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(String::from_utf16(&units).unwrap(), "Write-Output 'é'");
        assert_eq!(args[..7], ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-EncodedCommand"]);
        assert!(powershell_args(&"x".repeat(MAX_COMMAND_LINE)).is_err(), "too long for a command line");
        // A real batch's script fits with room to spare (the environment is in a file).
        let steps: Vec<Step> = (0..12).map(|_| Step::new(&["provision", "isoloom-controller", "--provision-with", "ansible"])).collect();
        let w = work();
        let len: usize = powershell_args(&script(&plan(&steps, &w)).unwrap()).unwrap().iter().map(|a| a.len() + 1).sum();
        assert!(len < MAX_COMMAND_LINE / 2, "{len}");
    }

    fn collect(steps: &[Step], log: &[&str], exit: u32, reason: Option<&str>) -> (Vec<String>, Result<Vec<Outcome>>) {
        let mut events = Vec::new();
        let mut c = Collector::new("##m##", steps);
        for l in log {
            c.line(l, &mut |e: Event<'_>| {
                events.push(match e {
                    Event::Start(i) => format!("start {i}"),
                    Event::Line(i, l) => format!("line {i} {l}"),
                    Event::End(i, r) => format!(
                        "end {i} {}",
                        match r {
                            Ok(s) => s.clone(),
                            Err(e) => e.to_string(),
                        }
                    ),
                })
            });
        }
        (events, c.finish(exit, reason))
    }

    #[test]
    fn the_log_becomes_the_batchs_events_and_outcomes() {
        let steps = [Step::new(&["up"]), Step::new(&["provision"]), Step::new(&["halt"]).always()];
        let log = [
            "##m## start 0",
            "Bringing machine 'default' up with 'hyperv' provider...",
            "##m## end 0 0",
            "##m## start 1",
            "fatal: [dc01]: UNREACHABLE!",
            "##m## end 1 1",
            "##m## start 2",
            "##m## end 2 0",
            "##m##x start 2",
        ];
        let (events, outcomes) = collect(&steps, &log, 0, None);
        assert_eq!(
            events[..3],
            ["start 0", "line 0 Bringing machine 'default' up with 'hyperv' provider...", "end 0 Bringing machine 'default' up with 'hyperv' provider...\n"]
        );
        assert_eq!(events[3..5], ["start 1", "line 1 fatal: [dc01]: UNREACHABLE!"]);
        assert_eq!(events[5], "end 1 `vagrant provision` failed: fatal: [dc01]: UNREACHABLE! (exited with exit code: 1)");
        assert_eq!(events[6..8], ["start 2", "end 2 "]);
        assert_eq!(events[8], "line 2 ##m##x start 2", "only the exact marker counts");
        let outcomes = outcomes.unwrap();
        assert!(matches!(&outcomes[0], Some(Ok(_))) && matches!(&outcomes[1], Some(Err(_))) && matches!(&outcomes[2], Some(Ok(_))));
    }

    #[test]
    fn skipped_missing_and_broken_runs() {
        let steps = [Step::new(&["up"]), Step::new(&["ssh", "-c", "uname"])];
        let (_, outcomes) = collect(&steps, &["##m## start 0", "##m## missing 0", "##m## end 0 127", "##m## skip 1"], 0, None);
        let outcomes = outcomes.unwrap();
        assert!(matches!(&outcomes[0], Some(Err(Error::ToolMissing { tool: "vagrant" }))));
        assert!(outcomes[1].is_none(), "skipped");
        let (_, broken) = collect(&steps, &["##m## error Cannot find path 'C:\\lab' because it does not exist."], 1, None);
        assert!(broken.unwrap_err().to_string().contains("Cannot find path"));
        let (_, early) = collect(&steps, &[], 3, None);
        assert!(early.unwrap_err().to_string().contains("exit code 3"));
    }

    #[test]
    fn a_batch_cut_short_is_a_failure_never_a_success() {
        // Regression: up done, then the script killed during the next step: the steps that never
        // ended (or never ran) must not read as done.
        let steps = [Step::new(&["up"]), Step::new(&["provision"]), Step::new(&["halt"]).always()];
        let (_, cut) = collect(&steps, &["##m## start 0", "##m## end 0 0", "##m## start 1", "TASK [x]"], 1, None);
        assert!(cut.unwrap_err().to_string().contains("ended before it finished"));
        // Between two steps, with an exit code of 0 even.
        let (_, between) = collect(&steps, &["##m## start 0", "##m## end 0 0"], 0, None);
        assert!(between.is_err());
        // Ended by the watcher: its reason is the error.
        let (_, timed) = collect(&steps, &["##m## start 0"], 1, Some("timeout"));
        assert!(timed.unwrap_err().to_string().contains("more than 6 hours"));
    }

    #[tokio::test]
    async fn a_followed_log_yields_its_lines_as_they_are_written() {
        let path = std::env::temp_dir().join(format!("cyberctf-follow-{}.log", rand::random::<u32>()));
        std::fs::write(&path, "").unwrap();
        let writer = {
            let path = path.clone();
            async move {
                use std::io::Write;
                let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
                f.write_all(b"one\r\ntw").unwrap();
                tokio::time::sleep(Duration::from_millis(450)).await;
                f.write_all("o\nthr\u{e9}e\nno newline".as_bytes()).unwrap();
                7u32
            }
        };
        let mut lines = Vec::new();
        let code = follow(&path, writer, |l| lines.push(l.to_string())).await.unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(code, 7);
        assert_eq!(lines, ["one", "two", "thr\u{e9}e", "no newline"]);
    }

    #[test]
    fn an_abandoned_batch_asks_its_watcher_to_stop_and_goes_away() {
        let w = WorkDir::new().unwrap();
        let dir = w.dir.clone();
        w.stop("Vagrant stopped to ask something");
        w.stop("cancelled");
        assert_eq!(std::fs::read_to_string(w.cancel()).unwrap(), "Vagrant stopped to ask something", "the first reason stays");
        drop(w);
        for _ in 0..50 {
            if !dir.exists() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("{} left behind", dir.display());
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
        let result = run_script(
            program,
            dir,
            &env,
            steps,
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
    async fn a_real_powershell_runs_the_batch_as_planned() {
        // The generated script, run by a real PowerShell: steps, skips, clean-ups, exit codes,
        // stderr, the environment from its file, and non-ASCII output come back through the log.
        let Some(ps) = powershell() else { return };
        let (program, steps) = if cfg!(windows) {
            (
                "cmd.exe",
                vec![
                    Step::new(&["/c", "echo", "hello", "%CYBERCTF_X%"]),
                    Step::new(&["/c", "echo oops 1>&2 & exit 3"]),
                    Step::new(&["/c", "echo", "skipped"]),
                    Step::new(&["/c", "echo", "cleanup"]).always(),
                ],
            )
        } else {
            (
                "sh",
                vec![
                    Step::new(&["-c", "echo hello $CYBERCTF_X; echo caf\u{e9}"]),
                    Step::new(&["-c", "echo oops >&2; exit 3"]),
                    Step::new(&["-c", "echo skipped"]),
                    Step::new(&["-c", "echo cleanup"]).always(),
                ],
            )
        };
        let (lines, outcomes) = real(ps, program, &std::env::temp_dir(), &steps).await;
        let outcomes = outcomes.unwrap();
        assert!(matches!(&outcomes[0], Some(Ok(out)) if out.contains("hello it's")), "{outcomes:?}");
        let Some(Err(Error::CommandFailed { stderr, .. })) = &outcomes[1] else { panic!("{outcomes:?}") };
        assert!(stderr.contains("oops") && stderr.contains("exit code: 3"), "{stderr}");
        assert!(outcomes[2].is_none(), "skipped after the failure");
        assert!(matches!(&outcomes[3], Some(Ok(out)) if out.contains("cleanup")), "{outcomes:?}");
        assert!(lines.contains(&"1 oops".to_string()), "{lines:?}");
        if !cfg!(windows) {
            assert!(lines.contains(&"0 caf\u{e9}".to_string()), "{lines:?}");
        }
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
    async fn a_question_vagrant_waits_on_ends_the_batch_with_a_plain_message() {
        // Vagrant on Windows waits forever on its question: the launcher sees the line before it,
        // and the watcher ends the run (the whole tree) instead of hanging.
        let Some(ps) = powershell() else { return };
        let (program, step) = if cfg!(windows) {
            ("cmd.exe", Step::new(&["/c", "echo Please choose a switch to attach to your Hyper-V instance.& ping -n 60 127.0.0.1 >nul"]))
        } else {
            ("sh", Step::new(&["-c", "echo 'Please choose a switch to attach to your Hyper-V instance.'; sleep 60"]))
        };
        let started = std::time::Instant::now();
        let (_, result) = real(ps, program, &std::env::temp_dir(), &[step]).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("which Hyper-V virtual switch"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(45), "ended in {:?}", started.elapsed());
    }
}
