//! Running the tools labs need (docker, vagrant, terraform, cloud CLIs): never through a shell,
//! headless on Windows, killed with their children when cancelled or timed out.

mod docker;
#[cfg(test)]
pub mod fake;
mod group;

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

/// Tools that may ship as a `.cmd`/`.bat` wrapper on Windows (not a real `.exe`). `CreateProcessW`
/// only appends `.exe` and does not consult `PATHEXT`, so `Command::new("az")` can't find
/// `az.cmd`; these go through `cmd.exe` (which also resolves a plain `.exe`, so it's safe for a
/// tool whose Windows form varies, like `oci`). `aws`/`terraform`/`docker`/`vagrant` are real
/// `.exe` and resolve directly.
#[cfg(windows)]
const WINDOWS_CMD_SHIM: &[&str] = &["az", "gcloud", "oci"];

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

/// Builds the Command: headless, Windows batch-wrapper tools routed through `cmd /C` so they
/// resolve, and Docker kept on the engine the user picked.
fn build(program: &str, args: &[&str]) -> Command {
    #[cfg(windows)]
    if WINDOWS_CMD_SHIM.contains(&program) {
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
            let group = GroupGuard::new(child.id());
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
