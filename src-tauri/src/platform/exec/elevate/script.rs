//! The PowerShell script a batch runs as: its environment from a file, the Hyper-V switch, each
//! step framed by marker lines in a log, and a watcher that ends the whole tree when asked.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::Batch;
use super::collect::WorkDir;
use crate::error::{Error, Result};
use crate::platform::exec::vagrant::{HYPERV_SWITCH_ENV, When};

/// The longest command line Windows accepts, in UTF-16 units.
pub(super) const MAX_COMMAND_LINE: usize = 32_000;

/// The longest a batch may run before the watcher ends it: a big lab's first boot takes a couple
/// of hours at most; past this, it is stuck.
pub(super) const DEADLINE: Duration = Duration::from_secs(6 * 3600);

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
pub(super) fn ps_quote(s: &str) -> String {
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
pub(super) fn forwarded_env(own: impl IntoIterator<Item = (String, String)>, env: &[(String, String)]) -> Vec<(String, String)> {
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
pub(super) fn own_env() -> impl Iterator<Item = (String, String)> {
    std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
}

/// The environment file the script reads: a JSON list of `{k, v}`.
pub(super) fn env_json(env: &[(String, String)]) -> String {
    serde_json::Value::Array(env.iter().map(|(k, v)| serde_json::json!({ "k": k, "v": v })).collect()).to_string()
}

/// The first `name` in the folders of `path` (a `PATH` value): the program the script runs, by
/// its full path, so it doesn't depend on the elevated account's own `PATH`.
pub(super) fn resolve(name: &str, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?).map(|d| d.join(name)).find(|p| p.is_file())
}

/// What a batch's script needs.
pub(super) struct Plan<'a> {
    pub(super) batch: &'a Batch<'a>,
    pub(super) work: &'a WorkDir,
    pub(super) marker: &'a str,
    /// The launcher's process id: its end ends the batch.
    pub(super) launcher: u32,
}

/// The PowerShell script that runs a [`Plan`], logging with the marker lines of [`Collector`].
pub(super) fn script(p: &Plan) -> Result<String> {
    for arg in p.batch.steps.iter().flat_map(|s| &s.args) {
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
    let _ = writeln!(
        s,
        "$cyberctfFile = [System.IO.File]::Open({}, [System.IO.FileMode]::Append, [System.IO.FileAccess]::Write, [System.IO.FileShare]::ReadWrite)",
        ps_path(&p.work.log())
    );
    s.push_str("$cyberctfLog = New-Object System.IO.StreamWriter -ArgumentList $cyberctfFile, (New-Object System.Text.UTF8Encoding -ArgumentList $false)\n");
    s.push_str("$cyberctfLog.AutoFlush = $true\n");
    s.push_str("function Write-CyberctfLine([string]$t) { $cyberctfLog.WriteLine($t) }\n");
    s
}

/// The watcher: a background job (its own process) that ends this script's whole tree on cancel,
/// when the launcher is gone, or at the deadline.
fn script_watcher(p: &Plan) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
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
    );
    s
}

/// The environment, the working folder and the Hyper-V switch.
fn script_setup(p: &Plan) -> String {
    let mut s = String::new();
    // The environment, from a file read once and deleted, so no value sits on a command line.
    let _ = write!(
        s,
        "  $cyberctfPairs = [System.IO.File]::ReadAllText({env}) | ConvertFrom-Json\n  Remove-Item -LiteralPath {env} -Force\n  foreach ($p in $cyberctfPairs) {{ [System.Environment]::SetEnvironmentVariable($p.k, $p.v, 'Process') }}\n",
        env = ps_path(&p.work.env())
    );
    let _ = writeln!(s, "  Set-Location -LiteralPath {} -ErrorAction Stop", ps_path(p.batch.dir));
    // The switch the VMs' first adapter goes on, unless the user named one: the built-in "Default
    // Switch", else an external one, else the first. Isoloom's Vagrantfile would take the Default
    // Switch, and stop on a host without one (Windows Server); the launcher's attach to it.
    let _ = write!(
        s,
        "  if (-not $env:{HYPERV_SWITCH_ENV}) {{\n    try {{\n      $sw = @(Get-VMSwitch -ErrorAction Stop)\n      $pick = @($sw | Where-Object {{ $_.Name -eq 'Default Switch' }}) + @($sw | Where-Object {{ \"$($_.SwitchType)\" -eq 'External' }}) + $sw | Select-Object -First 1\n      if ($pick) {{ $env:{HYPERV_SWITCH_ENV} = $pick.Name }}\n    }} catch {{}}\n  }}\n"
    );
    s
}

/// `Invoke-CyberctfStep`, which runs one step between its marker lines.
fn script_step_function(p: &Plan, m: &str) -> String {
    let mut s = String::new();
    // One function runs a step, so each step is one short line (the whole script must fit on a
    // command line, base64-encoded).
    let _ = write!(
        s,
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
        ps_quote(p.batch.program)
    );
    s
}

/// One line per step.
fn script_steps(p: &Plan) -> String {
    let mut s = String::new();
    for (i, step) in p.batch.steps.iter().enumerate() {
        let always = match step.when {
            When::IfOk => "$false",
            When::Always => "$true",
        };
        let args: Vec<String> = step.args.iter().map(|a| ps_quote(a)).collect();
        let _ = writeln!(s, "  Invoke-CyberctfStep {i} {always} @({})", args.join(", "));
    }
    s
}

/// The end: the error marker, the watcher stopped, the log closed, the exit code.
fn script_end(m: &str) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        "}} catch {{\n  Write-CyberctfLine ({m} + ' error ' + $_.Exception.Message)\n  $cyberctfCode = 1\n}} finally {{\n  Stop-Job -Job $cyberctfWatch -ErrorAction SilentlyContinue\n  Remove-Job -Job $cyberctfWatch -Force -ErrorAction SilentlyContinue\n  $cyberctfLog.Close()\n}}\nexit $cyberctfCode\n"
    );
    s
}

/// The arguments of `powershell.exe` that run `script`.
pub(super) fn powershell_args(script: &str) -> Result<Vec<String>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::exec::vagrant::Step;

    #[test]
    fn powershell_quoting_doubles_every_single_quote() {
        assert_eq!(ps_quote("plain"), "'plain'");
        assert_eq!(ps_quote("it's"), "'it''s'");
        assert_eq!(ps_quote("a\u{2019}b"), "'a\u{2019}\u{2019}b'");
        assert_eq!(ps_quote("$env:X `n"), "'$env:X `n'", "nothing expands in single quotes");
    }

    fn work() -> WorkDir {
        WorkDir::at(PathBuf::from("C:\\Temp\\w"))
    }

    fn plan<'a>(batch: &'a Batch<'a>, work: &'a WorkDir) -> Plan<'a> {
        Plan { batch, work, marker: "##m##", launcher: 77 }
    }

    fn batch(steps: &[Step]) -> Batch<'_> {
        Batch { program: "C:\\HashiCorp\\Vagrant\\bin\\vagrant.exe", dir: Path::new("C:\\Users\\Amélie\\lab"), env: &[], steps }
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
        assert!(script(&plan(&batch(&steps), &w)).is_err());
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
        let s = script(&plan(&batch(&steps), &w)).unwrap();
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
        let s = script(&plan(&batch(&steps), &w)).unwrap();
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
        let len: usize = powershell_args(&script(&plan(&batch(&steps), &w)).unwrap()).unwrap().iter().map(|a| a.len() + 1).sum();
        assert!(len < MAX_COMMAND_LINE / 2, "{len}");
    }
}
