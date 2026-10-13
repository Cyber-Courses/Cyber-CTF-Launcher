//! Vagrant elevated on Windows, for Hyper-V (issue #185).
//!
//! Hyper-V management needs an administrator token, and Vagrant's Hyper-V provider refuses to
//! run without one. A non-elevated launcher asks Windows for it (UAC, `ShellExecuteEx` with the
//! `runas` verb) once per batch of Vagrant commands, never for status reads.
//!
//! An elevated process can't inherit pipes (or the environment) from a non-elevated parent, so
//! the batch runs as a generated Windows PowerShell script (passed with `-EncodedCommand`, so no
//! script file and no execution policy is involved):
//! - it sets the folder and the environment the commands need (the caller's, plus `PATH` and
//!   the `VAGRANT_*` variables, `VAGRANT_HOME` included so boxes are shared even when another
//!   administrator account answers the prompt);
//! - it runs each step, appending its stdout and stderr lines to a log file (UTF-8), framed by
//!   marker lines carrying a random token (`start i`, `end i <exit code>`, `skip i`);
//! - the launcher follows that log as it grows ([`follow`]), turns it back into the same events
//!   a non-elevated batch reports ([`Collector`]), and waits on the process handle.
//!
//! Declining the prompt (`ERROR_CANCELLED`) gives a plain-language error ([`launch_error`]). An
//! elevated run can't be cancelled from the non-elevated app: if the caller gives up, the
//! commands still run to their end.

#![cfg_attr(not(windows), allow(dead_code))]

use std::path::Path;
use std::time::Duration;

use super::vagrant::{Event, Outcome, Step, When};
use crate::error::{Error, Result};

/// Whether a Vagrant command on Hyper-V machines (`hyperv`) must ask for elevation here.
pub(super) fn needed(hyperv: bool) -> bool {
    // Never in unit tests: they answer from the scripted tools, and must never raise a prompt.
    decide(cfg!(windows) && !cfg!(test), hyperv, is_elevated)
}

/// The decision itself: only Hyper-V, only on Windows, only when this process isn't elevated
/// already (it then runs Vagrant directly, as on any other provider).
fn decide(windows: bool, hyperv: bool, elevated: impl FnOnce() -> bool) -> bool {
    windows && hyperv && !elevated()
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

/// Refuses an argument Windows PowerShell 5.1 can't pass through to a native program intact:
/// it drops empty ones, mangles embedded double quotes, and stops parsing at `--%`.
fn check_arg(arg: &str) -> Result<()> {
    if arg.is_empty() || arg.contains(['"', '\0', '\n', '\r']) || arg == "--%" {
        return Err(Error::Invalid(format!("refusing to pass `{}` to an elevated Vagrant: Windows PowerShell can't pass it intact", arg.escape_debug())));
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

/// The environment the elevated commands get: an elevated process starts from the account's
/// stored environment, not this one. `PATH` (tools found here are found there) and every
/// `VAGRANT_*` variable carry over, `VAGRANT_HOME` is pinned to this user's (so the boxes are
/// the ones this user downloaded, whichever administrator approved), then the caller's `env`.
fn forwarded_env(own: impl IntoIterator<Item = (String, String)>, env: &[(String, String)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut profile = None;
    for (k, v) in own {
        let upper = k.to_ascii_uppercase();
        if upper == "USERPROFILE" {
            profile = Some(v.clone());
        }
        if upper == "PATH" || upper.starts_with("VAGRANT_") {
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

/// The PowerShell script that runs `steps` with `program` in `dir`, logging to `log` with the
/// marker lines of [`Collector`].
fn script(program: &str, dir: &Path, env: &[(String, String)], steps: &[Step], log: &Path, marker: &str) -> Result<String> {
    for arg in steps.iter().flat_map(|s| &s.args) {
        check_arg(arg)?;
    }
    let mut s = String::new();
    // Native stderr lines must stay plain lines (with `Stop`, the first one would end the script).
    s.push_str("$ErrorActionPreference = 'Continue'\n$ProgressPreference = 'SilentlyContinue'\n");
    s.push_str("try { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding -ArgumentList $false } catch {}\n");
    s.push_str(&format!(
        "$cyberctfFile = [System.IO.File]::Open({}, [System.IO.FileMode]::Append, [System.IO.FileAccess]::Write, [System.IO.FileShare]::ReadWrite)\n",
        ps_quote(&log.display().to_string())
    ));
    s.push_str("$cyberctfLog = New-Object System.IO.StreamWriter -ArgumentList $cyberctfFile, (New-Object System.Text.UTF8Encoding -ArgumentList $false)\n");
    s.push_str("$cyberctfLog.AutoFlush = $true\n");
    s.push_str("function Write-CyberctfLine([string]$t) { $cyberctfLog.WriteLine($t) }\n");
    s.push_str("$cyberctfFailed = $false\n$cyberctfCode = 0\ntry {\n");
    s.push_str(&format!("  Set-Location -LiteralPath {}\n", ps_quote(&dir.display().to_string())));
    for (k, v) in env {
        s.push_str(&format!("  [System.Environment]::SetEnvironmentVariable({}, {}, 'Process')\n", ps_quote(k), ps_quote(v)));
    }
    let m = ps_quote(marker);
    for (i, step) in steps.iter().enumerate() {
        let guard = match step.when {
            When::IfOk => "-not $cyberctfFailed",
            When::Always => "$true",
        };
        let args: Vec<String> = step.args.iter().map(|a| ps_quote(a)).collect();
        s.push_str(&format!(
            "  if ({guard}) {{\n    Write-CyberctfLine ({m} + ' start {i}')\n    $global:LASTEXITCODE = 0\n    try {{\n      & {} {} 2>&1 | ForEach-Object {{ Write-CyberctfLine ([string]$_) }}\n      $rc = $global:LASTEXITCODE\n    }} catch [System.Management.Automation.CommandNotFoundException] {{\n      Write-CyberctfLine ({m} + ' missing {i}')\n      $rc = 127\n    }}\n    if ($null -eq $rc) {{ $rc = 0 }}\n    Write-CyberctfLine ({m} + ' end {i} ' + $rc)\n    if ($rc -ne 0) {{ $cyberctfFailed = $true }}\n  }} else {{\n    Write-CyberctfLine ({m} + ' skip {i}')\n  }}\n",
            ps_quote(program),
            args.join(" ")
        ));
    }
    s.push_str(&format!("}} catch {{\n  Write-CyberctfLine ({m} + ' error ' + $_.Exception.Message)\n  $cyberctfCode = 1\n}} finally {{\n  $cyberctfLog.Close()\n}}\nexit $cyberctfCode\n"));
    Ok(s)
}

/// The arguments of `powershell.exe` that run `script`.
fn powershell_args(script: &str) -> Result<String> {
    use base64::Engine;
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let args = format!(
        "-NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -EncodedCommand {}",
        base64::engine::general_purpose::STANDARD.encode(utf16)
    );
    if args.len() > MAX_COMMAND_LINE {
        return Err(Error::Invalid("the elevated Vagrant command is too long for Windows (too many or too large environment variables)".into()));
    }
    Ok(args)
}

/// Lines kept for a failed step's error, as for a plain stream.
const ERROR_LINES: usize = super::STREAM_ERROR_LINES;

/// Turns the elevated batch's log back into a batch's events and outcomes.
struct Collector<'s> {
    marker: String,
    steps: &'s [Step],
    current: usize,
    text: String,
    tail: std::collections::VecDeque<String>,
    missing: bool,
    outcomes: Vec<Outcome>,
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

    /// The outcomes, once the script exited with `exit_code`.
    fn finish(self, exit_code: u32) -> Result<Vec<Outcome>> {
        if let Some(e) = self.error {
            return Err(Error::Invalid(format!("the elevated Vagrant run failed: {e}")));
        }
        if exit_code != 0 && self.outcomes.iter().all(Option::is_none) {
            return Err(Error::Invalid(format!("the elevated Vagrant run ended early (exit code {exit_code})")));
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

/// A folder of its own for one elevated batch's log, removed when done.
struct WorkDir(std::path::PathBuf);

impl WorkDir {
    fn new() -> Result<WorkDir> {
        let dir = std::env::temp_dir().join(format!("cyberctf-elevated-{:016x}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir)?;
        Ok(WorkDir(dir))
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs `steps` as `program` through `launch`, which starts PowerShell with the given arguments
/// and resolves to its exit code once it ended. Shared by the elevated path and its tests.
async fn run_script<L, F>(
    program: &str,
    dir: &Path,
    env: &[(String, String)],
    steps: &[Step],
    launch: L,
    mut on_event: impl FnMut(Event<'_>),
) -> Result<Vec<Outcome>>
where
    L: FnOnce(String) -> F,
    F: std::future::Future<Output = Result<u32>>,
{
    let work = WorkDir::new()?;
    let log = work.0.join("vagrant.log");
    std::fs::File::create(&log)?;
    let marker = format!("##cyberctf-{:016x}##", rand::random::<u64>());
    let env = forwarded_env(std::env::vars(), env);
    let args = powershell_args(&script(program, dir, &env, steps, &log, &marker)?)?;
    let mut collector = Collector::new(&marker, steps);
    let code = follow(&log, launch(args), |l| collector.line(l, &mut on_event)).await??;
    collector.finish(code)
}

/// Runs `steps` with Vagrant in `dir`, elevated: one UAC prompt for the whole batch.
#[cfg(windows)]
pub(super) async fn batch(dir: &Path, env: &[(String, String)], steps: &[Step], on_event: impl FnMut(Event<'_>)) -> Result<Vec<Outcome>> {
    let directory = dir.to_path_buf();
    let launch = move |args: String| async move {
        // The prompt blocks until it is answered; waiting blocks until the commands end.
        let process =
            tokio::task::spawn_blocking(move || win::launch(&args, &directory)).await.map_err(|e| Error::Invalid(e.to_string()))?.map_err(launch_error)?;
        tokio::task::spawn_blocking(move || process.wait()).await.map_err(|e| Error::Invalid(e.to_string()))
    };
    run_script("vagrant", dir, env, steps, launch, on_event).await
}

#[cfg(windows)]
mod win {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetExitCodeProcess, INFINITE, OpenProcessToken, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};

    /// `SW_HIDE`: the elevated PowerShell keeps its console window hidden.
    const SW_HIDE: i32 = 0;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
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

    /// The elevated PowerShell process, until it is waited for.
    pub(super) struct Process(usize);

    impl Process {
        /// Waits for it to end; its exit code.
        pub(super) fn wait(self) -> u32 {
            let handle = self.0 as HANDLE;
            let mut code = 1u32;
            // SAFETY: the process handle ShellExecuteExW returned, waited on and closed once.
            unsafe {
                WaitForSingleObject(handle, INFINITE);
                if GetExitCodeProcess(handle, &mut code) == 0 {
                    code = 1;
                }
                CloseHandle(handle);
            }
            code
        }
    }

    /// Starts Windows PowerShell elevated with `args` (UAC asks first). The OS error code when it
    /// couldn't (1223: the prompt was declined).
    pub(super) fn launch(args: &str, dir: &Path) -> std::result::Result<Process, i32> {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let exe = Path::new(&root).join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe");
        let (verb, file, params, directory) = (wide("runas".as_ref()), wide(exe.as_os_str()), wide(args.as_ref()), wide(dir.as_os_str()));
        // SAFETY: a zeroed SHELLEXECUTEINFOW is valid (null pointers, no flags); every string
        // it points to outlives the call.
        unsafe {
            let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
            info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
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
    fn only_hyperv_on_windows_without_elevation_is_elevated() {
        assert!(decide(true, true, || false));
        assert!(!decide(true, true, || true), "an elevated app runs Vagrant directly");
        assert!(!decide(true, false, || false), "other providers never need it");
        assert!(!decide(false, true, || false), "never off Windows");
        assert!(!needed(true), "never in unit tests");
    }

    #[test]
    fn a_declined_prompt_says_so_in_plain_words() {
        let e = launch_error(1223).to_string();
        assert!(e.contains("declined") && e.contains("administrator") && !e.contains("1223"), "{e}");
        assert!(matches!(launch_error(2), Error::ToolMissing { tool: "powershell" }));
        assert!(launch_error(5).to_string().contains("administrator rights"));
    }

    #[test]
    fn powershell_quoting_doubles_every_single_quote() {
        assert_eq!(ps_quote("plain"), "'plain'");
        assert_eq!(ps_quote("it's"), "'it''s'");
        assert_eq!(ps_quote("a\u{2019}b"), "'a\u{2019}\u{2019}b'");
        assert_eq!(ps_quote("$env:X `n"), "'$env:X `n'", "nothing expands in single quotes");
    }

    #[test]
    fn arguments_powershell_would_mangle_are_refused() {
        for bad in ["", "say \"hi\"", "--%", "a\nb"] {
            assert!(check_arg(bad).is_err(), "{bad:?}");
        }
        for good in ["up", "--provider", "hyperv", "uname -srm", "isoloom-controller", "a&b|c"] {
            assert!(check_arg(good).is_ok(), "{good:?}");
        }
        let steps = [Step::new(&["ssh", "-c", "echo \"x\""])];
        assert!(script("vagrant", Path::new("C:\\lab"), &[], &steps, Path::new("C:\\t\\l.log"), "##m##").is_err());
    }

    #[test]
    fn the_elevated_environment_carries_path_vagrant_and_the_callers() {
        let own = [
            ("Path".to_string(), "C:\\HashiCorp\\Vagrant\\bin".to_string()),
            ("USERPROFILE".into(), "C:\\Users\\Amélie\\".into()),
            ("VAGRANT_DEFAULT_PROVIDER".into(), "hyperv".into()),
            ("TEMP".into(), "C:\\Users\\Amélie\\AppData\\Local\\Temp".into()),
        ];
        let env = forwarded_env(own.clone(), &[("CLAIM".into(), "x".into()), ("vagrant_default_provider".into(), "virtualbox".into())]);
        assert_eq!(
            env,
            [
                ("Path".to_string(), "C:\\HashiCorp\\Vagrant\\bin".to_string()),
                ("VAGRANT_HOME".into(), "C:\\Users\\Amélie\\.vagrant.d".into()),
                ("CLAIM".into(), "x".into()),
                ("vagrant_default_provider".into(), "virtualbox".into()),
            ]
        );
        // A VAGRANT_HOME of the user's own is kept as it is.
        let mut own = own.to_vec();
        own.push(("VAGRANT_HOME".into(), "D:\\vagrant".into()));
        let env = forwarded_env(own, &[]);
        assert_eq!(env.iter().filter(|(k, _)| k == "VAGRANT_HOME").map(|(_, v)| v.as_str()).collect::<Vec<_>>(), ["D:\\vagrant"]);
    }

    #[test]
    fn the_script_runs_every_step_quoted_and_framed_by_markers() {
        let steps = [Step::new(&["up", "--provider", "hyperv"]), Step::new(&["halt", "o'brien"]).always()];
        let env = [("CLAIM".to_string(), "a'b".to_string())];
        let s = script("vagrant", Path::new("C:\\Users\\Amélie\\lab"), &env, &steps, Path::new("C:\\Temp\\x\\vagrant.log"), "##m##").unwrap();
        assert!(s.contains("Set-Location -LiteralPath 'C:\\Users\\Amélie\\lab'"), "{s}");
        assert!(s.contains("SetEnvironmentVariable('CLAIM', 'a''b', 'Process')"), "{s}");
        assert!(s.contains("& 'vagrant' 'up' '--provider' 'hyperv' 2>&1"), "{s}");
        assert!(s.contains("& 'vagrant' 'halt' 'o''brien' 2>&1"), "{s}");
        assert!(s.contains("if (-not $cyberctfFailed) {\n    Write-CyberctfLine ('##m##' + ' start 0')"), "{s}");
        assert!(s.contains("if ($true) {\n    Write-CyberctfLine ('##m##' + ' start 1')"), "a clean-up step always runs: {s}");
        assert!(s.contains("[System.IO.File]::Open('C:\\Temp\\x\\vagrant.log'"), "{s}");
        assert!(s.trim_end().ends_with("exit $cyberctfCode"));
    }

    #[test]
    fn powershell_gets_the_script_utf16_encoded() {
        use base64::Engine;
        let args = powershell_args("Write-Output 'é'").unwrap();
        let encoded = args.rsplit(' ').next().unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).unwrap();
        let units: Vec<u16> = bytes.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(String::from_utf16(&units).unwrap(), "Write-Output 'é'");
        assert!(args.starts_with("-NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -EncodedCommand "));
        assert!(powershell_args(&"x".repeat(MAX_COMMAND_LINE)).is_err(), "too long for a command line");
    }

    fn collect(steps: &[Step], log: &[&str], exit: u32) -> (Vec<String>, Result<Vec<Outcome>>) {
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
        (events, c.finish(exit))
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
        let (events, outcomes) = collect(&steps, &log, 0);
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
        let (_, outcomes) = collect(&steps, &["##m## start 0", "##m## missing 0", "##m## end 0 127", "##m## skip 1"], 0);
        let outcomes = outcomes.unwrap();
        assert!(matches!(&outcomes[0], Some(Err(Error::ToolMissing { tool: "vagrant" }))));
        assert!(outcomes[1].is_none(), "skipped");
        let (_, broken) = collect(&steps, &["##m## error Cannot find path 'C:\\lab' because it does not exist."], 1);
        assert!(broken.unwrap_err().to_string().contains("Cannot find path"));
        let (_, early) = collect(&steps, &[], 3);
        assert!(early.unwrap_err().to_string().contains("exit code 3"));
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

    /// The PowerShell that runs the scripts here: Windows PowerShell on Windows; elsewhere
    /// PowerShell 7 when `CYBERCTF_PWSH` names it (the script is the same).
    fn powershell() -> Option<String> {
        if cfg!(windows) { Some("powershell".into()) } else { std::env::var("CYBERCTF_PWSH").ok() }
    }

    #[tokio::test]
    async fn a_real_powershell_runs_the_batch_as_planned() {
        // The generated script, run by a real PowerShell (not elevated): steps, skips, clean-ups,
        // exit codes, stderr and non-ASCII output come back through the log as they would from
        // an elevated run.
        let Some(ps) = powershell() else { return };
        let (program, steps) = if cfg!(windows) {
            (
                "cmd.exe",
                vec![
                    Step::new(&["/c", "echo", "hello"]),
                    Step::new(&["/c", "echo oops 1>&2 & exit 3"]),
                    Step::new(&["/c", "echo", "skipped"]),
                    Step::new(&["/c", "echo", "cleanup"]).always(),
                ],
            )
        } else {
            (
                "sh",
                vec![
                    Step::new(&["-c", "echo hello; echo caf\u{e9}"]),
                    Step::new(&["-c", "echo oops >&2; exit 3"]),
                    Step::new(&["-c", "echo skipped"]),
                    Step::new(&["-c", "echo cleanup"]).always(),
                ],
            )
        };
        let dir = std::env::temp_dir();
        let launch = |args: String| async move {
            let mut cmd = tokio::process::Command::new(&ps);
            // `powershell_args` is one command line; split it back for a plain spawn.
            cmd.args(args.split(' ').filter(|a| *a != "-WindowStyle" && *a != "Hidden"));
            let status = cmd.status().await?;
            Ok(status.code().unwrap_or(1) as u32)
        };
        let mut lines = Vec::new();
        let outcomes = run_script(program, &dir, &[("CYBERCTF_X".into(), "it's".into())], &steps, launch, |e| {
            if let Event::Line(i, l) = e {
                lines.push(format!("{i} {}", l.trim()));
            }
        })
        .await
        .unwrap();
        assert!(matches!(&outcomes[0], Some(Ok(out)) if out.contains("hello")), "{outcomes:?}");
        let Some(Err(Error::CommandFailed { stderr, .. })) = &outcomes[1] else { panic!("{outcomes:?}") };
        assert!(stderr.contains("oops") && stderr.contains("exit code: 3"), "{stderr}");
        assert!(outcomes[2].is_none(), "skipped after the failure");
        assert!(matches!(&outcomes[3], Some(Ok(out)) if out.contains("cleanup")), "{outcomes:?}");
        assert!(lines.contains(&"0 hello".to_string()) && lines.contains(&"1 oops".to_string()), "{lines:?}");
        if !cfg!(windows) {
            assert!(lines.contains(&"0 caf\u{e9}".to_string()), "{lines:?}");
        }
    }
}
