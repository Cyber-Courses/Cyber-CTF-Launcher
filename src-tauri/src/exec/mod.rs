//! Running the tools labs need (docker, vagrant, terraform, cloud CLIs): never through a shell,
//! headless on Windows, killed with their children when cancelled or timed out.

mod docker;
#[cfg(test)]
pub mod fake;
mod group;
mod shim;

use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[cfg(target_os = "linux")]
pub use docker::login_name;
pub use docker::{docker_denied, docker_denied_message, valid_image};
pub use group::kill_live_tools;

use crate::error::{Error, Result};
use group::GroupGuard;

/// On Windows, every child of a windowed app gets a console window of its own unless told not
/// to: the status polls (docker, vagrant, VBoxManage) would flash one on screen every few
/// seconds. `CREATE_NO_WINDOW` keeps them headless; their output is piped anyway.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// How long a read-only status probe (docker/vagrant inspect, ps, network ls) may take before
/// it's treated as a stuck tool. Polled paths use this so a wedged daemon can't hang the UI.
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(25);

/// stderr lines kept for a streamed tool's error.
const STREAM_ERROR_LINES: usize = 10;

/// Keeps a child process off the screen on Windows (no-op elsewhere).
pub fn headless(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// `headless` for a blocking `std::process::Command`.
pub fn headless_std(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Runs a blocking command, giving up (and killing it) after `timeout`. Its exit status, or
/// `None` when it couldn't start or ran out of time.
pub fn status_within(cmd: &mut std::process::Command, timeout: Duration) -> Option<ExitStatus> {
    let mut child = cmd.spawn().ok()?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Builds the Command: headless, Windows batch-wrapper tools routed through `cmd /C` so they
/// resolve, and Docker kept on the engine the user picked.
fn build(program: &str, args: &[&str]) -> Command {
    #[cfg(windows)]
    if shim::SHIMMED.contains(&program) {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(program).args(args);
        headless(&mut cmd);
        return cmd;
    }
    let mut cmd = Command::new(program);
    cmd.args(args);
    headless(&mut cmd);
    if docker::uses_engine(program) && docker::strips_engine_overrides(docker::cli_config().as_deref()) {
        for var in docker::ENGINE_OVERRIDES {
            cmd.env_remove(var);
        }
    }
    cmd
}

/// The error a tool that couldn't start gives: missing, or the OS's own reason.
fn spawn_error(program: &'static str) -> impl Fn(std::io::Error) -> Error {
    move |e| match e.kind() {
        std::io::ErrorKind::NotFound => Error::ToolMissing { tool: program },
        _ => Error::Io(e),
    }
}

/// A tool's failure, with the command it ran.
fn failed(program: &str, args: &[&str], stderr: String) -> Error {
    Error::CommandFailed { command: format!("{program} {}", args.join(" ")), stderr }
}

/// What a streamed tool's failure says: its last stderr lines, else just its exit status.
fn stream_failure(stderr_tail: &[String], status: impl std::fmt::Display) -> String {
    let cause = stderr_tail.join("\n");
    if cause.trim().is_empty() { format!("exited with {status}") } else { format!("{} (exited with {status})", cause.trim()) }
}

/// Runs a program without a shell (arguments are never interpolated) and returns stdout.
pub async fn run(program: &'static str, args: &[&str], cwd: Option<&Path>) -> Result<String> {
    run_env(program, args, cwd, &[]).await
}

/// `run` with the shared status timeout, for polled read-only probes.
pub async fn run_read(program: &'static str, args: &[&str], cwd: Option<&Path>) -> Result<String> {
    run_inner(program, args, cwd, &[], Some(STATUS_TIMEOUT)).await
}

/// `run` with extra environment variables.
pub async fn run_env(program: &'static str, args: &[&str], cwd: Option<&Path>, env: &[(String, String)]) -> Result<String> {
    run_inner(program, args, cwd, env, None).await
}

/// `run_env` that gives up after `timeout`, killing the process, instead of waiting forever.
/// For read-only probes (status, inspect) where a wedged tool must not hang the caller: a
/// stuck VirtualBox or Docker daemon then surfaces as an error rather than piling up.
pub async fn run_env_timed(program: &'static str, args: &[&str], cwd: Option<&Path>, env: &[(String, String)], timeout: Duration) -> Result<String> {
    run_inner(program, args, cwd, env, Some(timeout)).await
}

async fn run_inner(program: &'static str, args: &[&str], cwd: Option<&Path>, env: &[(String, String)], timeout: Option<Duration>) -> Result<String> {
    shim::check_args(program, args)?;
    #[cfg(test)]
    if let Some(answer) = fake::answer(program, args) {
        return answer;
    }
    // Kept alive until the tool exits (see lan_relay).
    let relayed = crate::lan_relay::prepare(program, env).await;
    let env = relayed.env.as_slice();
    let mut cmd = build(program, args);
    cmd.stdin(Stdio::null());
    cmd.envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    // Kill the child if this future is dropped (cancellation), so a long tool (vagrant/docker/
    // cloud CLI) isn't left orphaned.
    cmd.kill_on_drop(true);
    let output = match timeout {
        None => cmd.output().await.map_err(spawn_error(program))?,
        Some(dur) => {
            // `spawn` (unlike `output`) inherits the parent's stdio by default, so without
            // explicit pipes `wait_with_output` returns an empty stdout even though the tool
            // printed plenty: every timed status probe then read as "nothing running".
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
            // Its own process group, so a timeout or cancellation kills the whole tree: `vagrant`
            // is a wrapper that starts `ruby` as a child, and killing only the wrapper orphaned
            // one ruby per timed-out poll until they all deadlocked on Vagrant's lock.
            #[cfg(unix)]
            cmd.process_group(0);
            let child = cmd.spawn().map_err(spawn_error(program))?;
            let group = GroupGuard::new(&child);
            let Ok(waited) = tokio::time::timeout(dur, child.wait_with_output()).await else {
                return Err(failed(program, args, format!("timed out after {}s (the tool or its backend may be stuck)", dur.as_secs())));
            };
            group.disarm();
            waited.map_err(spawn_error(program))?
        }
    };
    if !output.status.success() {
        return Err(failed(program, args, String::from_utf8_lossy(&output.stderr).trim().to_string()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Like `run`, but adds `env` to the process environment and forwards every
/// stdout/stderr line to `on_line` as it arrives.
pub async fn stream(program: &'static str, args: &[&str], cwd: Option<&Path>, env: &[(String, String)], on_line: impl FnMut(String)) -> Result<()> {
    stream_from(program, args, cwd, env, None, on_line).await
}

/// Like `stream`, with the file `stdin` as the command's standard input.
pub async fn stream_stdin(program: &'static str, args: &[&str], cwd: Option<&Path>, stdin: &Path, on_line: impl FnMut(String)) -> Result<()> {
    stream_from(program, args, cwd, &[], Some(stdin), on_line).await
}

async fn stream_from(
    program: &'static str,
    args: &[&str],
    cwd: Option<&Path>,
    env: &[(String, String)],
    stdin: Option<&Path>,
    on_line: impl FnMut(String),
) -> Result<()> {
    shim::check_args(program, args)?;
    #[cfg(test)]
    let mut on_line = on_line;
    #[cfg(test)]
    if let Some(answer) = fake::answer_stream(program, args, &mut on_line) {
        let _ = stdin;
        return answer;
    }
    let relayed = crate::lan_relay::prepare(program, env).await;
    let env = relayed.env.as_slice();
    let mut cmd = build(program, args);
    let input = match stdin {
        Some(f) => Stdio::from(std::fs::File::open(f)?),
        None => Stdio::null(),
    };
    cmd.stdin(input).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    // Kill the child (and, with it, the long-lived process we stream) if this future is dropped,
    // rather than orphaning a vagrant/docker/terraform run.
    cmd.kill_on_drop(true);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().map_err(spawn_error(program))?;
    // Keep the last few stderr lines so a failure's error carries the actual cause, not just the
    // exit status (callers that surface the returned error, not the log channel, rely on this).
    let (status, tail) = forward_lines(&mut child, STREAM_ERROR_LINES, on_line, |_| {}).await?;
    if !status.success() {
        return Err(failed(program, args, stream_failure(&tail, status)));
    }
    Ok(())
}

/// Forwards a piped child's stdout and stderr lines to `on_line` as they arrive, until both
/// close, then waits for it. `on_stderr` sees each stderr line first. Returns its exit status
/// and its last `keep` stderr lines.
pub async fn forward_lines(
    child: &mut tokio::process::Child,
    keep: usize,
    mut on_line: impl FnMut(String),
    mut on_stderr: impl FnMut(&str),
) -> Result<(ExitStatus, Vec<String>)> {
    let mut stdout = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
    let mut stderr = BufReader::new(child.stderr.take().expect("piped stderr")).lines();
    let (mut out_done, mut err_done) = (false, false);
    let mut tail: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    while !(out_done && err_done) {
        tokio::select! {
            line = stdout.next_line(), if !out_done => match line? { Some(l) => on_line(l), None => out_done = true },
            line = stderr.next_line(), if !err_done => match line? {
                Some(l) => {
                    on_stderr(&l);
                    if keep > 0 {
                        if tail.len() == keep { tail.pop_front(); }
                        tail.push_back(l.clone());
                    }
                    on_line(l);
                }
                None => err_done = true,
            },
        }
    }
    let status = child.wait().await?;
    Ok((status, tail.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn streams_take_stdin_from_a_file() {
        // Isoloom's exec check runners are piped into `docker compose exec -T … sh -s` this way.
        let f = std::env::temp_dir().join(format!("cyberctf-stdin-{}.sh", std::process::id()));
        std::fs::write(&f, "echo one\necho two\n").unwrap();
        let mut lines = Vec::new();
        stream_stdin("sh", &["-s"], None, &f, |l| lines.push(l)).await.unwrap();
        let _ = std::fs::remove_file(&f);
        assert_eq!(lines, ["one", "two"]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timed_reads_capture_the_tools_output() {
        // Regression: the timed path spawned without piping stdout, so `wait_with_output`
        // returned "" for every status probe (docker ps, compose ps, vagrant status). Labs read
        // as not running and a successful launch looked like it reset. A timed read must return
        // what the tool printed, exactly like the untimed one.
        let timed = run_read("sh", &["-c", "printf hi"], None).await.unwrap();
        assert_eq!(timed, "hi");
        let untimed = run("sh", &["-c", "printf hi"], None).await.unwrap();
        assert_eq!(timed, untimed);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timed_reads_surface_stderr_on_failure() {
        // A failing tool's message is piped too, so it reaches the error instead of vanishing.
        let err = run_read("sh", &["-c", "echo boom >&2; exit 3"], None).await.unwrap_err();
        assert!(matches!(err, Error::CommandFailed { ref stderr, .. } if stderr.contains("boom")), "{err:?}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_timed_out_tool_is_an_error_naming_the_command() {
        let err = run_env_timed("sh", &["-c", "sleep 5"], None, &[], Duration::from_millis(200)).await.unwrap_err();
        assert!(
            matches!(err, Error::CommandFailed { ref command, ref stderr } if command == "sh -c sleep 5" && stderr.starts_with("timed out after 0s")),
            "{err:?}"
        );
    }

    /// Whether process `pid` is gone (or a zombie) within a few seconds.
    async fn gone(pid: u32) -> bool {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};
        for _ in 0..60 {
            let mut sys = System::new();
            sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true, ProcessRefreshKind::nothing());
            if sys.process(Pid::from_u32(pid)).is_none_or(|p| matches!(p.status(), ProcessStatus::Zombie | ProcessStatus::Dead)) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    #[test]
    fn a_blocking_command_is_given_up_after_its_time() {
        // Regression: ending each shell session at quit ran `docker exec` with no time limit, so
        // a Docker that didn't answer held the quit up.
        let (mut quick, mut stuck) = if cfg!(windows) {
            let mut quick = std::process::Command::new("cmd");
            quick.args(["/C", "exit 0"]);
            let mut stuck = std::process::Command::new("ping");
            stuck.args(["-n", "30", "127.0.0.1"]).stdout(Stdio::null());
            (quick, stuck)
        } else {
            let mut stuck = std::process::Command::new("sleep");
            stuck.arg("30");
            (std::process::Command::new("true"), stuck)
        };
        assert!(status_within(&mut quick, Duration::from_secs(10)).is_some_and(|s| s.success()));
        let started = std::time::Instant::now();
        assert!(status_within(&mut stuck, Duration::from_millis(500)).is_none());
        assert!(started.elapsed() < Duration::from_secs(10), "gave up in time");
    }

    #[tokio::test]
    async fn a_timed_out_tool_takes_what_it_started_with_it() {
        // Regression (Windows): a timeout killed the tool alone, and what it had started (a
        // wrapper's real program) ran on. A tool's whole tree ends with it now (a job object on
        // Windows, its process group on unix).
        let file = std::env::temp_dir().join(format!("cyberctf-tree-{}", rand::random::<u32>()));
        let path = file.display().to_string();
        let err = if cfg!(windows) {
            let script = format!(
                "$p = Start-Process -FilePath ping -ArgumentList '-n','120','127.0.0.1' -PassThru -NoNewWindow; Set-Content -Path '{path}' -Value $p.Id; Start-Sleep 120"
            );
            run_env_timed("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script], None, &[], Duration::from_secs(10)).await
        } else {
            run_env_timed("sh", &["-c", &format!("sleep 120 & echo $! > '{path}'; wait")], None, &[], Duration::from_secs(2)).await
        };
        assert!(err.is_err(), "timed out");
        let pid: u32 = std::fs::read_to_string(&file).expect("the tool started its child").trim().parse().unwrap();
        let _ = std::fs::remove_file(&file);
        assert!(gone(pid).await, "the tool's child {pid} was left running");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failed_stream_keeps_its_last_stderr_lines() {
        let err = stream("sh", &["-c", "for i in $(seq 1 12); do echo e$i >&2; done; exit 2"], None, &[], |_| {}).await.unwrap_err();
        let Error::CommandFailed { stderr, .. } = err else { panic!("{err:?}") };
        assert!(stderr.starts_with("e3\n") && stderr.contains("e12 (exited with"), "{stderr}");
    }

    #[test]
    fn a_silent_failure_reports_its_exit_status() {
        assert_eq!(stream_failure(&[], "exit status: 1"), "exited with exit status: 1");
        assert_eq!(stream_failure(&[" ".into()], "exit status: 1"), "exited with exit status: 1");
        assert_eq!(stream_failure(&["no space left".into()], "exit status: 1"), "no space left (exited with exit status: 1)");
    }

    #[test]
    fn a_missing_tool_is_named() {
        let err = spawn_error("vagrant")(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert!(matches!(err, Error::ToolMissing { tool: "vagrant" }));
        assert!(matches!(spawn_error("vagrant")(std::io::Error::from(std::io::ErrorKind::PermissionDenied)), Error::Io(_)));
    }
}
