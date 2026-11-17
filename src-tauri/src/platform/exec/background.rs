//! A tool left running beside the app's work, which nothing waits on (an SSH tunnel while
//! Terraform uses it): run by the runner as every tool is, on a task of its own, its whole tree
//! ended when its handle drops, whichever way the work around it ends (done, failed, cancelled).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use isothrum_runner::{Exit, ProcessError, ProcessSpec, Stream};

use super::{failed, not_run, runner};
use crate::error::Error;

/// The stderr lines kept for a background tool's failure.
const SAID_LINES: usize = 10;

/// A tool running in the background until dropped, which ends it with all it started.
pub struct Background {
    /// Its command, for what its failure says.
    program: &'static str,
    args: Vec<String>,
    /// The task running it, until it ended; dropping it ends the tool's tree.
    running: Option<tokio::task::JoinHandle<std::result::Result<Exit, ProcessError>>>,
    /// Its last stderr lines, for what its failure says.
    said: Arc<Mutex<VecDeque<String>>>,
    /// Why it stopped, once it did.
    stopped: Option<Error>,
}

/// Starts `program args` in the background, its input the null device: see [`Background`].
pub fn background(program: &'static str, args: &[&str]) -> Background {
    let spec = ProcessSpec::new(program, args.iter().copied());
    let runner = runner();
    let said = Arc::new(Mutex::new(VecDeque::new()));
    let keep = Arc::clone(&said);
    let running = tokio::spawn(async move {
        runner
            .run(&spec, &mut |stream, line| {
                if stream == Stream::Stderr {
                    let mut kept = keep.lock().unwrap_or_else(PoisonError::into_inner);
                    kept.push_back(line.trim_end().to_owned());
                    if kept.len() > SAID_LINES {
                        kept.pop_front();
                    }
                }
            })
            .await
    });
    Background { program, args: args.iter().map(ToString::to_string).collect(), running: Some(running), said, stopped: None }
}

impl Background {
    /// Waits up to `limit` for the tool to stop: its failure then (what it said on stderr, else
    /// its exit status), None while it runs on.
    pub async fn stopped_within(&mut self, limit: Duration) -> Option<Error> {
        if let Some(running) = self.running.as_mut() {
            let ended = tokio::time::timeout(limit, running).await.ok()?;
            self.running = None;
            let args: Vec<&str> = self.args.iter().map(String::as_str).collect();
            let tail = self.said.lock().unwrap_or_else(PoisonError::into_inner).iter().cloned().collect::<Vec<_>>().join("\n");
            self.stopped = Some(match ended {
                Ok(Ok(exit)) if tail.trim().is_empty() => failed(self.program, &args, exit.to_string()),
                Ok(Ok(_)) => failed(self.program, &args, tail),
                Ok(Err(e)) => not_run(self.program, e),
                Err(e) => failed(self.program, &args, e.to_string()),
            });
        }
        self.stopped.as_ref().map(clone_error)
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        // The runner's future dropped with its task: the tool's whole tree ends.
        if let Some(running) = self.running.take() {
            running.abort();
        }
    }
}

/// A copy of `e` as a background tool's caller gets it again.
fn clone_error(e: &Error) -> Error {
    match e {
        Error::CommandFailed { command, stderr } => Error::CommandFailed { command: command.clone(), stderr: stderr.clone() },
        Error::ToolMissing { tool } => Error::ToolMissing { tool },
        other => Error::Invalid(other.to_string()),
    }
}
