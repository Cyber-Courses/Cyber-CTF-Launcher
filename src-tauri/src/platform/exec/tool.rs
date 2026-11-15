//! A tool [`super::spawn_group`] started: its output and its exit, whatever the OS. On unix it is
//! tokio's child, in a process group of its own. On Windows it is a process the kernel created
//! inside its job object (`sys::create_in_job`, Windows only), which std can't do: so a tool is never outside
//! its job, not even suspended for a moment before it is put there.

use std::process::ExitStatus;

use tokio::process::{ChildStderr, ChildStdout};

#[cfg(windows)]
use crate::platform::sys;

/// A running tool: its piped output (taken by whoever reads it) and its process.
pub(crate) struct Tool {
    /// Its standard output, piped.
    pub(crate) stdout: Option<ChildStdout>,
    /// Its standard error, piped.
    pub(crate) stderr: Option<ChildStderr>,
    process: Process,
}

#[cfg(not(windows))]
type Process = tokio::process::Child;

/// A tool's process on Windows: killed when dropped before it was seen to exit, as tokio's
/// `kill_on_drop` child is.
#[cfg(windows)]
struct Process {
    handle: std::sync::Arc<std::os::windows::io::OwnedHandle>,
    exited: Option<ExitStatus>,
}

impl Tool {
    /// The tool tokio started, its output taken out of it.
    #[cfg(not(windows))]
    pub(super) fn from_child(mut child: tokio::process::Child) -> Tool {
        Tool { stdout: child.stdout.take(), stderr: child.stderr.take(), process: child }
    }

    /// The tool created in its job.
    #[cfg(windows)]
    pub(super) fn from_created(created: sys::Created) -> std::io::Result<Tool> {
        let pipe = |h: std::os::windows::io::OwnedHandle| std::process::ChildStdout::from(h);
        let err_pipe = |h: std::os::windows::io::OwnedHandle| std::process::ChildStderr::from(h);
        Ok(Tool {
            stdout: Some(ChildStdout::from_std(pipe(created.stdout))?),
            stderr: Some(ChildStderr::from_std(err_pipe(created.stderr))?),
            process: Process { handle: std::sync::Arc::new(created.process), exited: None },
        })
    }

    /// Waits for it to exit (cancel safe: its exit, once seen, is kept).
    pub(crate) async fn wait(&mut self) -> std::io::Result<ExitStatus> {
        #[cfg(not(windows))]
        return self.process.wait().await;
        #[cfg(windows)]
        {
            use std::os::windows::process::ExitStatusExt as _;
            if let Some(status) = self.process.exited {
                return Ok(status);
            }
            let handle = std::sync::Arc::clone(&self.process.handle);
            let code = tokio::task::spawn_blocking(move || sys::exit_code(&handle, u32::MAX)).await.map_err(std::io::Error::other)??;
            let status = ExitStatus::from_raw(code.ok_or_else(|| std::io::Error::other("the tool's wait returned before it exited"))?);
            self.process.exited = Some(status);
            Ok(status)
        }
    }
}

#[cfg(windows)]
impl Drop for Process {
    fn drop(&mut self) {
        if self.exited.is_none() {
            sys::terminate_process(&self.handle);
        }
    }
}
