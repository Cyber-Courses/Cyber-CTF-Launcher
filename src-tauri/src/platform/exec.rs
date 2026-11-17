//! Running the tools labs need (docker, vagrant, terraform, cloud CLIs): never through a shell,
//! headless on Windows, killed with their children when cancelled or timed out.

#![allow(clippy::disallowed_methods, reason = "the adapter: every tool the app runs starts here")]

mod background;
mod docker;
mod elevate;
#[cfg(test)]
pub mod fake;
mod group;
mod ledger;
pub(crate) mod shim;
mod tool;
#[cfg(any(windows, test))]
mod win_command;

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

pub use background::{Background, background};
/// The image pattern, for the bindings the tests write (`testing::ts_bindings`).
#[cfg(test)]
pub use docker::IMAGE_PATTERN;
pub use docker::login_name;
pub use docker::{ImageRef, docker_denied, docker_denied_error, valid_image};
pub use elevate::join_hyperv_admins;
#[cfg(windows)]
pub use elevate::set_owner_window;
pub(crate) use group::GroupGuard;
pub use group::kill_live_tools;
pub use ledger::{DIR as TOOLS_DIR, end_orphans as end_orphaned_tools, record_tools_in};
pub(crate) use tool::Tool;

use crate::error::{Error, Result};
use crate::platform::tool_output::text::{decode, locale};

/// Which tools answer this thread's calls: the machine's own, or a unit test's scripted ones
/// (`fake::Tools`, each with a set of its own). A cache of the tools' answers keys by it, so one
/// test's scripted answer never stands for another's while the tests run the cache the app runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ToolSet(u64);

impl ToolSet {
    /// Whether these are the machine's own tools, not a test's scripted ones: only they may
    /// reach the machine beyond a tool's answer (a restart's `RunOnce`).
    #[cfg(windows)]
    pub fn are_the_machines(self) -> bool {
        self == ToolSet(0)
    }
}

/// The [`ToolSet`] answering this thread's calls.
pub fn tools_in_use() -> ToolSet {
    #[cfg(test)]
    if let Some(set) = fake::set() {
        return ToolSet(set);
    }
    ToolSet(0)
}

/// On Windows, every child of a windowed app gets a console window of its own unless told not
/// to: the status polls (docker, vagrant, `VBoxManage`) would flash one on screen every few
/// seconds. `CREATE_NO_WINDOW` keeps them headless; their output is piped anyway.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// How long a read-only status probe (docker/vagrant inspect, ps, network ls) may take before
/// it's treated as a stuck tool. Polled paths use this so a wedged daemon can't hang the UI.
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(25);

/// The bound on `run`/`run_env` when the caller named none. A read probe or a cloud CLI sets its
/// own shorter limit, but teardown (`docker compose down`, `docker rm -f`) runs through the plain
/// path while holding the lab lock, and a wedged daemon there must fail rather than hang the lab
/// for good. Generous, since a real teardown can be slow; the post-exit drain cap
/// ([`DRAIN_AFTER_EXIT`]) returns a finished-but-leftover-held run far sooner.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

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

/// Starts a desktop app and returns at once, the app on its own: no console window, none of this
/// process's standard handles (a child given them keeps them open as long as it runs, and whoever
/// waits for this process's output then waits for the app: Docker Desktop, started by the guided
/// setup under a test harness whose output a script redirected, held the script until the
/// harness's budget ended it), its own process group. Windows only: macOS opens apps with
/// `open -a`, which returns by itself.
#[cfg(windows)]
pub fn launch_detached(program: &str) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    std::process::Command::new(program)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map(drop)
        .map_err(|e| Error::Invalid(format!("couldn't start {program}: {e}")))
}

/// Builds the Command: headless, Windows batch-wrapper tools routed through `cmd /C` so they
/// resolve, its messages in the C locale ([`locale`]), and Docker kept on the engine the user
/// picked.
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
    in_c_locale(&mut cmd);
    if docker::uses_engine(program) && docker::strips_engine_overrides(docker::cli_config().as_deref()) {
        for var in docker::ENGINE_OVERRIDES {
            cmd.env_remove(var);
        }
    }
    cmd
}

/// Sets `cmd`'s messages, numbers and times to the C locale, its character set left as the user's
/// ([`locale`]): what the launcher reads of a tool is then in the words its readers know.
pub fn in_c_locale(cmd: &mut Command) {
    for (var, value) in locale(|name| std::env::var_os(name)) {
        match value {
            Some(value) => cmd.env(var, value),
            None => cmd.env_remove(var),
        };
    }
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

/// What a tool given up on after `limit` failed with (`timed_out` reads it).
fn timed_out_after(limit: Duration) -> String {
    format!("timed out after {}s (the tool or its backend may be stuck)", limit.as_secs())
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
/// group on unix; on Windows a kill-on-close job object the kernel creates it in, so nothing it
/// starts ever runs outside it, and the job ends its tree however the app ends. Dropped armed (a
/// timeout, a cancellation), the guard ends the whole tree, and so does the app's exit
/// ([`kill_live_tools`]). `stream`: a long-running tool, stopped gently on unix, and recorded
/// there in the tools' ledger while it runs, for the next start to end it should the app be
/// killed outright ([`ledger`]). Its input is the null device and its output piped, whatever
/// `cmd` says (every caller asks for that).
pub(crate) fn spawn_group(cmd: &mut Command, stream: bool) -> std::io::Result<(Tool, GroupGuard)> {
    #[cfg(not(windows))]
    {
        cmd.kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = cmd.spawn()?;
        let guard = GroupGuard::track(child.id(), stream);
        Ok((Tool::from_child(child), guard))
    }
    #[cfg(windows)]
    {
        let job = crate::platform::sys::Job::new()?;
        let created = create_in_job(cmd.as_std(), &job)?;
        let guard = GroupGuard::in_job(created.pid, stream, job);
        Ok((Tool::from_created(created)?, guard))
    }
}

/// Creates the tool `cmd` describes (its program found on its `PATH`, its arguments, environment
/// changes and working folder) inside `job`.
#[cfg(windows)]
fn create_in_job(cmd: &std::process::Command, job: &crate::platform::sys::Job) -> std::io::Result<crate::platform::sys::Created> {
    use std::ffi::{OsStr, OsString};
    let changes: Vec<(OsString, Option<OsString>)> = cmd.get_envs().map(|(k, v)| (k.to_owned(), v.map(OsStr::to_owned))).collect();
    let vars = win_command::environment(std::env::vars_os(), &changes);
    let program = cmd.get_program().to_string_lossy();
    let application = win_command::resolve(&program, win_command::path_of(&vars), Path::is_file)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, format!("{program} isn't on PATH")))?;
    let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    let line = win_command::command_line(&application.to_string_lossy(), &args);
    crate::platform::sys::create_in_job(application.as_os_str(), OsStr::new(&line), &vars, cmd.get_current_dir().map(Path::as_os_str), job)
}

/// Whether `e` is a timed tool that ran out of time (and was killed), not one that failed.
#[cfg(test)]
pub fn timed_out(e: &Error) -> bool {
    matches!(e, Error::CommandFailed { stderr, .. } if stderr.starts_with("timed out after"))
}

/// Runs a program without a shell (arguments are never interpolated) and returns stdout.
pub async fn run(program: &'static str, args: &[&str], cwd: Option<&Path>) -> Result<String> {
    run_env(program, args, cwd, &[]).await
}

/// `run` with the shared status timeout, for polled read-only probes.
pub async fn run_read(program: &'static str, args: &[&str], cwd: Option<&Path>) -> Result<String> {
    run_inner(program, args, cwd, &[], Some(STATUS_TIMEOUT)).await
}

/// `run_read` with extra environment variables (a Docker context to probe, say).
pub async fn run_read_env(program: &'static str, args: &[&str], cwd: Option<&Path>, env: &[(String, String)]) -> Result<String> {
    run_inner(program, args, cwd, env, Some(STATUS_TIMEOUT)).await
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
        return match answer {
            fake::FakeOutcome::Done(result) => result,
            fake::FakeOutcome::Hang => std::future::pending::<Result<String>>().await,
        };
    }
    let mut cmd = piped_command(program, args, cwd, env, Stdio::null());
    // Its own group, so a timeout or cancellation ends the whole tree: `vagrant` is a wrapper
    // around `ruby` (an orphaned ruby per timed-out poll deadlocked on Vagrant's lock), and
    // `cmd /C az` around `python` (a cancelled `az login` left it running).
    let (child, group) = spawn_group(&mut cmd, false).map_err(spawn_error(program))?;
    // The plain `run`/`run_env` path names no timeout, but it is what teardown runs through while
    // holding the lab lock: a generous default ([`DEFAULT_TIMEOUT`]) bounds a wedged daemon so it
    // can't hang the lab for good. A read/CLI caller's own, shorter, limit is used as given.
    let limit = timeout.unwrap_or(DEFAULT_TIMEOUT);
    let waited = tokio::time::timeout(limit, wait_output_drained(child)).await.map_err(|_| failed(program, args, timed_out_after(limit)))?;
    group.disarm();
    let output = waited.map_err(spawn_error(program))?;
    if !output.status.success() {
        return Err(exit_error(program, args, output.status.code(), decode(&output.stderr).trim().to_string()));
    }
    Ok(decode(&output.stdout))
}

/// Waits for a piped child to exit and returns its output, but once it has exited waits at most
/// [`DRAIN_AFTER_EXIT`] for its pipes to close. `wait_with_output` reads them to EOF, and a
/// process the tool left running (an ssh `ControlMaster`, a Terraform provider plugin) can hold
/// them open for good: that hung a `run`/`run_env` teardown, and the lab lock it holds, forever.
/// The bytes are returned raw and decoded by the caller, exactly as `wait_with_output`.
async fn wait_output_drained(mut child: Tool) -> std::io::Result<std::process::Output> {
    let mut out_pipe = child.stdout.take().ok_or_else(|| std::io::Error::other("the tool's output wasn't piped"))?;
    let mut err_pipe = child.stderr.take().ok_or_else(|| std::io::Error::other("the tool's error output wasn't piped"))?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let status;
    {
        // Both pipes read to EOF while the exit is awaited, so the child never blocks on a full
        // one. If it exits before they close (a leftover holding them open), they get only the
        // bounded grace, never an unbounded wait.
        let read_both = async {
            tokio::try_join!(out_pipe.read_to_end(&mut stdout), err_pipe.read_to_end(&mut stderr))?;
            std::io::Result::Ok(())
        };
        tokio::pin!(read_both);
        tokio::select! {
            read = &mut read_both => {
                read?;
                status = child.wait().await?;
            }
            exit = child.wait() => {
                status = exit?;
                let _ = tokio::time::timeout(DRAIN_AFTER_EXIT, &mut read_both).await;
            }
        }
    }
    Ok(std::process::Output { status, stdout, stderr })
}

#[cfg(test)]
mod tests;
