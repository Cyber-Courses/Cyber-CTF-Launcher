//! Running the tools labs need (docker, vagrant, terraform, cloud CLIs): never through a shell,
//! headless on Windows, killed with their children when cancelled or timed out.

mod docker;
mod elevate;
#[cfg(test)]
pub mod fake;
mod group;
pub(crate) mod shim;
mod vagrant;

use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

#[cfg(target_os = "linux")]
pub use docker::login_name;
pub use docker::{docker_denied, docker_denied_error, image_repository, valid_image};
#[cfg(windows)]
#[cfg(windows)]
pub use elevate::{OWNER_WINDOW_ENV, owner_window, set_owner_window};
pub use elevate::{join_hyperv_admins, may_manage_hyperv};
pub(crate) use group::GroupGuard;
pub use group::kill_live_tools;
pub use vagrant::{Event, Step, Vagrant, hyperv_switch_line};

use crate::error::{Error, Result};

/// On Windows, every child of a windowed app gets a console window of its own unless told not
/// to: the status polls (docker, vagrant, `VBoxManage`) would flash one on screen every few
/// seconds. `CREATE_NO_WINDOW` keeps them headless; their output is piped anyway.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// How long a read-only status probe (docker/vagrant inspect, ps, network ls) may take before
/// it's treated as a stuck tool. Polled paths use this so a wedged daemon can't hang the UI.
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(25);

/// How long a cloud CLI call (`aws`, `az`, `gcloud`, `oci`) may take: one waiting for a
/// re-authentication nobody will give, or on a dead network, must not leave Test spinning or a
/// launch stalled with its lab locked.
pub const CLI_TIMEOUT: Duration = Duration::from_secs(60);

/// stderr lines kept for a streamed tool's error.
const STREAM_ERROR_LINES: usize = 10;

/// How long a tool's pipes are still read once it has exited: its last lines are in them, but a
/// process it left running (an ssh `ControlMaster`, a Terraform provider plugin) can hold them open
/// for good, and waiting for them to close hung the run (and the lab's lock) forever.
const DRAIN_AFTER_EXIT: Duration = Duration::from_secs(2);

/// Keeps a child process off the screen on Windows (no-op elsewhere).
pub fn headless(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// [`headless`] for a synchronous `std::process::Command`: keeps the short Windows-only helpers
/// (the Ctrl+C interrupter, `taskkill`) off the screen. Windows only, because every caller is.
#[cfg(windows)]
pub fn headless_std(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
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

/// What a tool given up on after `limit` failed with (see [`timed_out`]).
fn timed_out_after(limit: Duration) -> String {
    format!("timed out after {}s (the tool or its backend may be stuck)", limit.as_secs())
}

/// `run`, a tool run (or several) described as `what`, given up after `limit` like a timed tool:
/// dropping it kills what it started (`kill_on_drop`, and its process group).
pub async fn within<T>(what: &str, limit: Duration, run: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(limit, run).await.unwrap_or_else(|_| Err(Error::CommandFailed { command: what.to_string(), stderr: timed_out_after(limit) }))
}

/// The error of a tool that ran and exited with `code`: missing when cmd.exe, which runs it on
/// Windows, said it found no such command; else its failure.
fn exit_error(program: &'static str, args: &[&str], code: Option<i32>, stderr: String) -> Error {
    if shim::missing_behind_cmd(program, code) {
        return Error::ToolMissing { tool: program };
    }
    failed(program, args, stderr)
}

/// Starts `cmd` as a group of its own, tracked until the guard is disarmed: its own process
/// group on unix; on Windows a job object, the process created suspended and resumed only once
/// it is in the job, so nothing it starts in between escapes it. Dropped armed (a timeout, a
/// cancellation), the guard ends the whole tree, and so does the app's exit
/// ([`kill_live_tools`]). `stream`: a long-running tool, stopped gently on unix.
pub(crate) fn spawn_group(cmd: &mut Command, stream: bool) -> std::io::Result<(Child, GroupGuard)> {
    cmd.kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW | group::CREATE_SUSPENDED);
    let child = cmd.spawn()?;
    let guard = if stream { GroupGuard::stream(&child) } else { GroupGuard::new(&child) };
    #[cfg(windows)]
    group::resume(&child)?;
    Ok((child, guard))
}

/// Whether `e` is a timed tool that ran out of time (and was killed), not one that failed.
pub fn timed_out(e: &Error) -> bool {
    matches!(e, Error::CommandFailed { stderr, .. } if stderr.starts_with("timed out after"))
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

/// `run` given up after [`CLI_TIMEOUT`], for a cloud CLI.
pub async fn run_cli(program: &'static str, args: &[&str]) -> Result<String> {
    run_inner(program, args, None, &[], Some(CLI_TIMEOUT)).await
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

/// `program args` reading `stdin`, with `env` added, in `cwd` when given, its output piped.
/// Piped explicitly: `spawn` (unlike `output`) inherits the parent's stdio by default, and
/// `wait_with_output` then returned an empty stdout although the tool printed plenty (every
/// timed status probe read as "nothing running").
fn piped_command(program: &str, args: &[&str], cwd: Option<&Path>, env: &[(String, String)], stdin: Stdio) -> Command {
    let mut cmd = build(program, args);
    cmd.stdin(stdin).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd
}

async fn run_inner(program: &'static str, args: &[&str], cwd: Option<&Path>, env: &[(String, String)], timeout: Option<Duration>) -> Result<String> {
    shim::check_args(program, args)?;
    #[cfg(test)]
    if let Some(answer) = fake::answer(program, args, env) {
        return answer;
    }
    // Kept alive until the tool exits (see lan_relay).
    let relayed = crate::lan_relay::prepare(program, env).await;
    let env = relayed.env.as_slice();
    let mut cmd = piped_command(program, args, cwd, env, Stdio::null());
    // Its own group, so a timeout or cancellation ends the whole tree: `vagrant` is a wrapper
    // around `ruby` (an orphaned ruby per timed-out poll deadlocked on Vagrant's lock), and
    // `cmd /C az` around `python` (a cancelled `az login` left it running).
    let (child, group) = spawn_group(&mut cmd, false).map_err(spawn_error(program))?;
    let waiting = child.wait_with_output();
    let waited = match timeout {
        None => waiting.await,
        Some(dur) => tokio::time::timeout(dur, waiting).await.map_err(|_| failed(program, args, timed_out_after(dur)))?,
    };
    group.disarm();
    let output = waited.map_err(spawn_error(program))?;
    if !output.status.success() {
        return Err(exit_error(program, args, output.status.code(), String::from_utf8_lossy(&output.stderr).trim().to_string()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Like `run`, but adds `env` to the process environment and forwards every
/// stdout/stderr line to `on_line` as it arrives.
pub async fn stream(program: &'static str, args: &[&str], cwd: Option<&Path>, env: &[(String, String)], on_line: impl FnMut(String)) -> Result<()> {
    stream_from(program, args, cwd, env, on_line).await
}

async fn stream_from(program: &'static str, args: &[&str], cwd: Option<&Path>, env: &[(String, String)], on_line: impl FnMut(String)) -> Result<()> {
    shim::check_args(program, args)?;
    #[cfg(test)]
    let mut on_line = on_line;
    #[cfg(test)]
    if let Some(answer) = fake::answer_stream(program, args, env, &mut on_line) {
        return answer;
    }
    let relayed = crate::lan_relay::prepare(program, env).await;
    let env = relayed.env.as_slice();
    let mut cmd = piped_command(program, args, cwd, env, Stdio::null());
    // A cancellation ends the whole tree: `vagrant` is a wrapper whose `ruby` outlived the
    // wrapper killed alone, and kept Vagrant's lock. A deploy worker being stopped reaches it
    // through `kill_live_tools` (gently: SIGTERM), and Terraform's interrupt finds it as a group
    // under the worker (see deploy_worker).
    let (mut child, group) = spawn_group(&mut cmd, true).map_err(spawn_error(program))?;
    // Keep the last few stderr lines so a failure's error carries the actual cause, not just the
    // exit status (callers that surface the returned error, not the log channel, rely on this).
    let forwarded = forward_lines(&mut child, STREAM_ERROR_LINES, on_line, |_| {}).await;
    if forwarded.is_ok() {
        group.disarm();
    }
    let (status, tail) = forwarded?;
    if !status.success() {
        return Err(exit_error(program, args, status.code(), stream_failure(&tail, status)));
    }
    Ok(())
}

/// Forwards a piped child's stdout and stderr lines to `on_line` as they arrive, until both
/// close, or until [`DRAIN_AFTER_EXIT`] after the child itself exited (something it left running
/// may keep them open). `on_stderr` sees each stderr line first. Returns its exit status and its
/// last `keep` stderr lines.
///
/// Lines are read as bytes and decoded lossily: tools print in the console's codepage
/// (`VBoxManage` and Vagrant on a French Windows, WinRM output, a compose build's), and a line
/// that isn't UTF-8 once failed the stream, which dropped (and so killed) a `terraform apply`
/// mid-run.
pub async fn forward_lines(
    child: &mut tokio::process::Child,
    keep: usize,
    mut on_line: impl FnMut(String),
    mut on_stderr: impl FnMut(&str),
) -> Result<(ExitStatus, Vec<String>)> {
    let mut stdout = BufReader::new(piped(child.stdout.take(), "output")?);
    let mut stderr = BufReader::new(piped(child.stderr.take(), "error output")?);
    // Kept across the loop: `read_until` is cancel safe only because a partial line stays here.
    let (mut out_buf, mut err_buf) = (Vec::new(), Vec::new());
    let (mut out_done, mut err_done) = (false, false);
    let mut tail = StderrTail::new(keep);
    let mut exited: Option<ExitStatus> = None;
    // Armed once the child has exited; never polled before.
    let drain = tokio::time::sleep(DRAIN_AFTER_EXIT);
    tokio::pin!(drain);
    while !(out_done && err_done) {
        tokio::select! {
            // At the end, a last line with no `\n` may already be in the buffer: read by a call
            // the other stream's line cut short.
            read = stdout.read_until(b'\n', &mut out_buf), if !out_done => match read? {
                0 if out_buf.is_empty() => out_done = true,
                _ => on_line(take_line(&mut out_buf)),
            },
            read = stderr.read_until(b'\n', &mut err_buf), if !err_done => match read? {
                0 if err_buf.is_empty() => err_done = true,
                _ => tail.forward(take_line(&mut err_buf), &mut on_line, &mut on_stderr),
            },
            status = child.wait(), if exited.is_none() => {
                exited = Some(status?);
                drain.as_mut().reset(tokio::time::Instant::now() + DRAIN_AFTER_EXIT);
            },
            () = &mut drain, if exited.is_some() => break,
        }
    }
    // Cut off by the drain: what was half read is still a line.
    if !out_buf.is_empty() {
        on_line(take_line(&mut out_buf));
    }
    tail.flush(&mut err_buf, &mut on_line, &mut on_stderr);
    let status = match exited {
        Some(status) => status,
        None => child.wait().await?,
    };
    Ok((status, tail.lines.into()))
}

/// A child's stream, which the caller piped: an error, not a panic, if it didn't.
fn piped<T>(stream: Option<T>, what: &str) -> Result<T> {
    stream.ok_or_else(|| Error::Invalid(format!("the tool's {what} wasn't piped")))
}

/// A streamed tool's last stderr lines, kept for its error.
struct StderrTail {
    keep: usize,
    lines: std::collections::VecDeque<String>,
}

impl StderrTail {
    fn new(keep: usize) -> StderrTail {
        StderrTail { keep, lines: std::collections::VecDeque::new() }
    }

    /// Hands a stderr line to `on_stderr` then `on_line`, keeping it among the last `keep`.
    fn forward(&mut self, line: String, on_line: &mut impl FnMut(String), on_stderr: &mut impl FnMut(&str)) {
        on_stderr(&line);
        if self.keep > 0 {
            if self.lines.len() == self.keep {
                self.lines.pop_front();
            }
            self.lines.push_back(line.clone());
        }
        on_line(line);
    }

    /// Forwards the stderr line half read into `buf`, if any: cut off, it is still a line.
    fn flush(&mut self, buf: &mut Vec<u8>, on_line: &mut impl FnMut(String), on_stderr: &mut impl FnMut(&str)) {
        if !buf.is_empty() {
            self.forward(take_line(buf), on_line, on_stderr);
        }
    }
}

/// The line read into `buf` (its `\n` or `\r\n` cut off, bytes that aren't UTF-8 replaced),
/// leaving `buf` empty for the next one.
fn take_line(buf: &mut Vec<u8>) -> String {
    let mut end = buf.len();
    if buf[..end].ends_with(b"\n") {
        end -= 1;
    }
    if buf[..end].ends_with(b"\r") {
        end -= 1;
    }
    let line = String::from_utf8_lossy(&buf[..end]).into_owned();
    buf.clear();
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_run_given_up_on_reads_as_timed_out() {
        let err = within("docker compose exec", Duration::from_millis(10), std::future::pending::<Result<()>>()).await.unwrap_err();
        assert!(timed_out(&err), "{err}");
        assert!(err.to_string().starts_with("`docker compose exec` failed: timed out after"), "{err}");
        assert_eq!(within("x", Duration::from_secs(1), async { Ok(7) }).await.unwrap(), 7);
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
        assert!(timed_out(&err));
        assert!(!timed_out(&failed("sh", &[], "exited 1".into())) && !timed_out(&Error::ToolMissing { tool: "sh" }));
    }

    /// A tool that starts a long child of its own and writes its pid to `file`, then waits: the
    /// program and its arguments. The child outlives the tool unless the tool's whole tree is
    /// ended with it, which is what the tree tests check.
    fn parent_of_a_child(file: &std::path::Path) -> (&'static str, Vec<String>) {
        let path = file.display();
        if cfg!(windows) {
            let script = format!(
                "$p = Start-Process -FilePath ping -ArgumentList '-n','120','127.0.0.1' -PassThru -NoNewWindow; Set-Content -Path '{path}' -Value $p.Id; Start-Sleep 120"
            );
            ("powershell", ["-NoProfile", "-NonInteractive", "-Command", &script].map(String::from).to_vec())
        } else {
            ("sh", vec!["-c".to_string(), format!("sleep 120 & echo $! > '{path}'; wait")])
        }
    }

    /// The pid `parent_of_a_child` wrote once its child was up, waited for rather than read once:
    /// a tool's first line may not have run yet on a loaded runner, and reading the file too early
    /// (before the tool was cancelled) was the race that made these tests flaky under CI load.
    /// Bounded (~15s), so a tool that never starts its child fails loudly instead of hanging.
    async fn wait_for_child_pid(file: &std::path::Path) -> u32 {
        for _ in 0..300 {
            if let Some(pid) = std::fs::read_to_string(file).ok().and_then(|t| t.trim().parse::<u32>().ok()) {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the tool never started its child (no pid at {})", file.display());
    }

    /// Runs `tool` (a tool's future that starts the child whose pid lands in `file`) until that
    /// child is up, then cancels it by dropping it and asserts its whole tree — the child included
    /// — went with it. Waiting for the child first means it is always known-started before the tool
    /// is ended: no read-before-write race, whatever the runner's load.
    async fn assert_cancel_takes_the_tree(file: &std::path::Path, tool: impl std::future::Future) {
        let pid = {
            tokio::pin!(tool);
            tokio::select! {
                _ = &mut tool => panic!("the tool ended before it started its child"),
                pid = wait_for_child_pid(file) => pid,
            }
            // `tool` is dropped as this block ends: `kill_on_drop` and the armed `GroupGuard` end
            // its whole tree (its process group on unix, its job object on Windows).
        };
        let _ = std::fs::remove_file(file);
        assert!(fake::gone(pid).await, "the tool's child {pid} was left running");
    }

    #[tokio::test]
    async fn a_timed_out_tool_takes_what_it_started_with_it() {
        // Regression (Windows): a timeout killed the tool alone, and what it had started (a
        // wrapper's real program) ran on. A tool's whole tree ends with it now (a job object on
        // Windows, its process group on unix). The timed path's tree-kill is its `GroupGuard`
        // dropped on give-up; the error of a timeout itself is `a_timed_out_tool_is_an_error...`.
        // The generous limit is a safety net only: the child is confirmed up and the tool is
        // cancelled long before it, so the test never waits on wall-clock and can't race.
        let file = std::env::temp_dir().join(format!("cyberctf-tree-{}", rand::random::<u32>()));
        let (program, args) = parent_of_a_child(&file);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let timed = run_env_timed(program, &args, None, &[], Duration::from_secs(120));
        assert_cancel_takes_the_tree(&file, timed).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn output_that_isnt_utf8_is_read_lossily_and_the_tool_runs_on() {
        // Regression: the first byte that wasn't UTF-8 (VBoxManage in a French Windows' codepage)
        // failed the stream, which dropped the child and killed it: a `terraform apply` cut off
        // mid-run. The line comes through with a replacement character, and the tool finishes.
        let mut lines = Vec::new();
        let script = r"printf 'Cr\351ation\r\n'; printf 'err \377\n' >&2; sleep 0.2; printf 'done'";
        stream("sh", &["-c", script], None, &[], |l| lines.push(l)).await.expect("the tool ran to its end");
        // stdout and stderr are separate pipes: their lines interleave in no set order.
        let at = |l: &str| lines.iter().position(|x| x == l);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(at("err \u{fffd}").is_some(), "{lines:?}");
        assert!(at("Cr\u{fffd}ation") < at("done") && at("Cr\u{fffd}ation").is_some(), "{lines:?}");
    }

    #[test]
    fn a_line_loses_its_ending_only() {
        let mut buf = b"a\r\r\n".to_vec();
        assert_eq!(take_line(&mut buf), "a\r");
        assert_eq!(buf, b"");
        assert_eq!(take_line(&mut b"last".to_vec()), "last");
        assert_eq!(take_line(&mut b"\n".to_vec()), "");
    }

    #[tokio::test]
    async fn a_cancelled_stream_takes_what_it_started_with_it() {
        // A streamed `vagrant` is a wrapper around `ruby`: cancelled, the wrapper alone was
        // killed, and its ruby ran on holding Vagrant's lock. The tool's tree ends with it now.
        // The child is waited for before the stream is cancelled (dropped), so a loaded runner
        // can't read the pid file before the tool wrote it.
        let file = std::env::temp_dir().join(format!("cyberctf-stream-tree-{}", rand::random::<u32>()));
        let (program, args) = parent_of_a_child(&file);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        assert_cancel_takes_the_tree(&file, stream(program, &args, None, &[], |_| {})).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_tool_that_exited_is_not_waited_for_past_what_it_left_holding_its_output() {
        // Regression: the stream read until both pipes closed, and a process the tool left behind
        // (an ssh ControlMaster, a provider plugin) held them open: a finished `terraform destroy`
        // then hung the auto-stop sweep, and the lab's lock, for good.
        let started = std::time::Instant::now();
        let mut lines = Vec::new();
        let run = stream("sh", &["-c", "sleep 20 & printf 'done'"], None, &[], |l| lines.push(l));
        tokio::time::timeout(Duration::from_secs(10), run).await.expect("not held up by the leftover").unwrap();
        assert_eq!(lines, ["done"]);
        assert!(started.elapsed() < Duration::from_secs(10));
        // And a failure is still read as one, with its last line.
        let err = stream("sh", &["-c", "sleep 20 & echo gone >&2; exit 4"], None, &[], |_| {}).await.unwrap_err();
        assert!(matches!(err, Error::CommandFailed { ref stderr, .. } if stderr.starts_with("gone (exited with")), "{err:?}");
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

    #[tokio::test]
    async fn lines_are_read_lossily_whatever_the_read_boundaries() {
        // A two-byte buffer makes every multi-byte character span reads: `read_until` gathers the
        // whole line first, so a character is never cut at a chunk boundary. CRLF is stripped, a
        // lone CR inside a line is kept, an invalid byte is replaced, and the last line needs no
        // newline.
        let mut input = "a\r\nb\rc\n\u{20ac}\u{20ac}\n".as_bytes().to_vec();
        input.extend_from_slice(&[0xff, b'd']);
        let mut reader = BufReader::with_capacity(2, &input[..]);
        let mut buf = Vec::new();
        let mut lines = Vec::new();
        while reader.read_until(b'\n', &mut buf).await.unwrap() > 0 || !buf.is_empty() {
            lines.push(take_line(&mut buf));
        }
        assert_eq!(lines, ["a", "b\rc", "\u{20ac}\u{20ac}", "\u{fffd}d"]);
        assert_eq!(reader.read_until(b'\n', &mut buf).await.unwrap(), 0, "still at the end");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_streamed_tool_printing_invalid_utf8_is_forwarded_not_cut() {
        // A provisioning script echoing Latin-1 (0xE9 alone) used to end the stream with an
        // error and kill the tool; now the byte is replaced and the run goes on to its end.
        let mut lines = Vec::new();
        stream("sh", &["-c", "printf 'caf\\351 ok\\n'; printf 'done\\n'"], None, &[], |l| lines.push(l)).await.unwrap();
        assert_eq!(lines, ["caf\u{fffd} ok", "done"]);
    }
}

#[cfg(test)]
mod proptests;
