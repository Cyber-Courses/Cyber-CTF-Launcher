//! A tool left running beside the app's work, which nothing waits on (an SSH tunnel while
//! Terraform uses it): started as every timed tool is ([`super::spawn_group`]), its whole tree
//! ended when its handle drops, whichever way the work around it ends (done, failed, cancelled).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};

use super::group::GroupGuard;
use super::tool::Tool;
use super::{DRAIN_AFTER_EXIT, STREAM_ERROR_LINES, failed, piped_command, spawn_error, spawn_group};
use crate::error::{Error, Result};
use crate::platform::tool_output::text::decode;

/// A tool running in the background until dropped, which ends it with all it started.
pub struct Background {
    /// Its command, for what its failure says.
    program: &'static str,
    args: Vec<String>,
    running: Running,
}

/// How a background tool runs.
enum Running {
    /// A process, its group ended when this drops (the guard, then the tool killed on drop).
    Process {
        /// Boxed: a scripted tool's variant is small beside it.
        tool: Box<Tool>,
        _group: GroupGuard,
        /// Its last stderr lines, read as they come so a talkative tool never blocks on a full
        /// pipe, kept for what its failure says.
        said: Arc<Mutex<VecDeque<String>>>,
        /// The task reading them, done once the pipe closes.
        reading: tokio::task::JoinHandle<()>,
    },
    /// A test's scripted tool: running on (`None`), or stopped saying this on stderr.
    #[cfg(test)]
    Scripted(Option<String>),
}

/// Starts `program args` in the background, its input the null device: see [`Background`].
pub fn background(program: &'static str, args: &[&str]) -> Result<Background> {
    super::shim::check_args(program, args)?;
    let owned = args.iter().map(ToString::to_string).collect();
    #[cfg(test)]
    if let Some(answer) = super::fake::answer(program, args, &[]) {
        let stopped = match answer {
            super::fake::FakeOutcome::Hang | super::fake::FakeOutcome::Done(Ok(_)) => None,
            super::fake::FakeOutcome::Done(Err(Error::CommandFailed { stderr, .. })) => Some(stderr),
            super::fake::FakeOutcome::Done(Err(e)) => return Err(e),
        };
        return Ok(Background { program, args: owned, running: Running::Scripted(stopped) });
    }
    let mut cmd = piped_command(program, args, None, &[], std::process::Stdio::null());
    let (mut tool, group) = spawn_group(&mut cmd, false).map_err(spawn_error(program))?;
    let said = Arc::new(Mutex::new(VecDeque::new()));
    let reading = tokio::spawn(keep_tail(tool.stderr.take(), Arc::clone(&said)));
    // Its output isn't read: a tool run this way prints nothing there (ssh -N), and the pipe is
    // closed so a stray line can't block it.
    drop(tool.stdout.take());
    Ok(Background { program, args: owned, running: Running::Process { tool: Box::new(tool), _group: group, said, reading } })
}

/// Reads `pipe` to its end, keeping its last [`STREAM_ERROR_LINES`] lines in `said`.
async fn keep_tail(pipe: Option<tokio::process::ChildStderr>, said: Arc<Mutex<VecDeque<String>>>) {
    let Some(pipe) = pipe else { return };
    let mut lines = BufReader::new(pipe);
    let mut line = Vec::new();
    while lines.read_until(b'\n', &mut line).await.is_ok_and(|n| n > 0) {
        let mut kept = said.lock().unwrap_or_else(PoisonError::into_inner);
        kept.push_back(decode(&line).trim_end().to_owned());
        if kept.len() > STREAM_ERROR_LINES {
            kept.pop_front();
        }
        line.clear();
    }
}

impl Background {
    /// Waits up to `limit` for the tool to stop: its failure then (what it said on stderr, else
    /// its exit status), None while it runs on.
    pub async fn stopped_within(&mut self, limit: Duration) -> Option<Error> {
        let args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        match &mut self.running {
            Running::Process { tool, said, reading, .. } => {
                let status = tokio::time::timeout(limit, tool.wait()).await.ok()?;
                // Its last lines are still in the pipe for a moment after it exits.
                let _ = tokio::time::timeout(DRAIN_AFTER_EXIT, reading).await;
                let tail = said.lock().unwrap_or_else(PoisonError::into_inner).iter().cloned().collect::<Vec<_>>().join("\n");
                let why = match status {
                    Ok(status) if tail.trim().is_empty() => format!("exited with {status}"),
                    Ok(_) => tail,
                    Err(e) => e.to_string(),
                };
                Some(failed(self.program, &args, why))
            }
            #[cfg(test)]
            Running::Scripted(Some(stderr)) => Some(failed(self.program, &args, stderr.clone())),
            #[cfg(test)]
            Running::Scripted(None) => {
                tokio::time::sleep(limit).await;
                None
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_background_tool_runs_until_its_handle_drops_and_its_group_goes_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let script = format!("sleep 60 & echo $! > '{}'; wait", pid_file.display());
        let mut tool = background("sh", &["-c", &script]).unwrap();
        assert!(tool.stopped_within(Duration::from_millis(300)).await.is_none(), "it runs on");
        let pid: u32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
        drop(tool);
        assert!(super::super::fake::gone(pid).await, "dropped, what it started ended with it");
    }

    #[tokio::test]
    async fn a_background_tool_that_stops_says_why() {
        let mut tool = background("sh", &["-c", "echo 'Permission denied (publickey).' >&2; exit 255"]).unwrap();
        let why = tool.stopped_within(Duration::from_secs(10)).await.unwrap().to_string();
        assert!(why.contains("Permission denied (publickey)."), "{why}");
    }
}
