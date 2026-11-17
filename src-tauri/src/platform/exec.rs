//! Running the tools labs need (docker, vagrant, terraform, cloud CLIs), never through a shell:
//! a thin layer over Isothrum's runner ([`crate::platform::processes`]), which keeps each tool
//! headless on Windows and ends its whole tree when its call is cut short (a timeout, a
//! cancellation). In a unit test the tools are the test's machine's (`testing::tools::runner`):
//! scripted, never the real ones.

mod background;
mod docker;
mod elevate;

use std::path::Path;
use std::time::Duration;

pub use background::{Background, background};
/// The image pattern, for the bindings the tests write (`testing::ts_bindings`).
#[cfg(test)]
pub use docker::IMAGE_PATTERN;
pub use docker::login_name;
pub use docker::{ImageRef, docker_denied, docker_denied_error, valid_image};
pub use elevate::join_hyperv_admins;
#[cfg(windows)]
pub use elevate::set_owner_window;

use crate::error::{Error, Result};

/// Which tools answer this thread's calls: the machine's own, or a unit test's scripted ones
/// (`testing::tools::Tools`, each with a set of its own). A cache of the tools' answers keys by
/// it, so one test's scripted answer never stands for another's while the tests run the cache the
/// app runs.
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
    if let Some(set) = crate::testing::tools::set() {
        return ToolSet(set);
    }
    ToolSet(0)
}

/// How long a read-only status probe (docker/vagrant inspect, ps, network ls) may take before
/// it's treated as a stuck tool. Polled paths use this so a wedged daemon can't hang the UI.
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(25);

/// The bound on `run`/`run_env` when the caller named none. A read probe or a cloud CLI sets its
/// own shorter limit, but teardown (`docker compose down`, `docker rm -f`) runs through the plain
/// path while holding the lab lock, and a wedged daemon there must fail rather than hang the lab
/// for good. Generous, since a real teardown can be slow; the runner returns a finished run whose
/// leftovers hold its output open far sooner.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// A tool's failure, with the command it ran.
fn failed(program: &str, args: &[&str], stderr: String) -> Error {
    Error::CommandFailed { command: format!("{program} {}", args.join(" ")), stderr }
}

/// What a tool given up on after `limit` failed with (`timed_out` reads it).
fn timed_out_after(limit: Duration) -> String {
    format!("timed out after {}s (the tool or its backend may be stuck)", limit.as_secs())
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

async fn run_inner(program: &'static str, args: &[&str], cwd: Option<&Path>, env: &[(String, String)], timeout: Option<Duration>) -> Result<String> {
    let mut spec = isothrum_runner::ProcessSpec::new(program, args.iter().copied());
    spec.cwd = cwd.map(Path::to_path_buf);
    spec.env = env.to_vec();
    let runner = runner();
    // The plain `run`/`run_env` path names no timeout, but it is what teardown runs through while
    // holding the lab lock: a generous default ([`DEFAULT_TIMEOUT`]) bounds a wedged daemon so it
    // can't hang the lab for good. A read/CLI caller's own, shorter, limit is used as given. Given
    // up on, the call's future is dropped, and the runner ends the tool's whole tree.
    let limit = timeout.unwrap_or(DEFAULT_TIMEOUT);
    let captured = tokio::time::timeout(limit, isothrum_runner::capture(runner.as_ref(), &spec))
        .await
        .map_err(|_| failed(program, args, timed_out_after(limit)))?
        .map_err(|e| not_run(program, e))?;
    if !captured.success() {
        return Err(failed(program, args, captured.stderr_tail.join("\n").trim().to_owned()));
    }
    Ok(captured.stdout.join("\n"))
}

/// The runner the app's tools start through; a test's machine's in a unit test.
fn runner() -> std::sync::Arc<dyn isothrum_runner::ProcessRunner> {
    #[cfg(test)]
    {
        crate::testing::tools::runner()
    }
    #[cfg(not(test))]
    {
        crate::platform::processes::system()
    }
}

/// The error of a tool the runner couldn't run to its end: missing, refused, or the OS's reason.
fn not_run(program: &'static str, e: isothrum_runner::ProcessError) -> Error {
    match e {
        isothrum_runner::ProcessError::Missing { .. } => Error::ToolMissing { tool: program },
        isothrum_runner::ProcessError::Io { source, .. } => Error::Io(source),
        other => Error::Invalid(other.to_string()),
    }
}
