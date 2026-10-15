//! Unit tests' stand-in for the tools: while a test holds a [`Tools`] guard, every `run*` and
//! `stream*` on its thread answers from the test's script instead of starting a process, so code
//! that drives docker, vagrant or terraform runs without them (and never touches real ones). The
//! install and removal steps (`platform::steps`) answer from it too: an install test never runs
//! a package manager.
//! Async tests run on one thread (`#[tokio::test]`), so the script is per thread: parallel tests
//! don't see each other's.

use std::cell::RefCell;
use std::rc::Rc;

use crate::error::{Error, Result};

/// A scripted tool's answer: its stdout (each line streamed, for `stream`), or its failure.
#[derive(Clone)]
pub enum Reply {
    Out(String),
    Fail(String),
}

/// A script's answer for a tool that printed `stdout`.
#[allow(clippy::unnecessary_wraps, reason = "a script answers an Option, None being a missing tool")]
pub fn ok(stdout: impl Into<String>) -> Option<Reply> {
    Some(Reply::Out(stdout.into()))
}

/// A script's answer for a tool that failed, printing `stderr`.
#[allow(clippy::unnecessary_wraps, reason = "a script answers an Option, None being a missing tool")]
pub fn fail(stderr: impl Into<String>) -> Option<Reply> {
    Some(Reply::Fail(stderr.into()))
}

/// What a scripted call got: the script's reply, or nothing (the tool is missing).
enum Scripted {
    Reply(Reply),
    Missing,
}

type Script = Box<dyn FnMut(&str, &[&str]) -> Option<Reply>>;

struct State {
    script: Script,
    calls: Vec<String>,
    /// The environment each call was given, beside `calls`.
    envs: Vec<Vec<(String, String)>>,
}

thread_local! {
    static STATE: RefCell<Option<Rc<RefCell<State>>>> = const { RefCell::new(None) };
}

/// The scripted tools of the running test, until dropped. A call the script doesn't answer
/// (`None`) is a missing tool.
pub struct Tools(Rc<RefCell<State>>);

impl Tools {
    pub fn new(script: impl FnMut(&str, &[&str]) -> Option<Reply> + 'static) -> Tools {
        let state = Rc::new(RefCell::new(State { script: Box::new(script), calls: Vec::new(), envs: Vec::new() }));
        STATE.with(|s| *s.borrow_mut() = Some(state.clone()));
        Tools(state)
    }

    /// Tools answering by rule: the first rule whose text the call (`program arg arg…`) contains
    /// answers; any other call succeeds with no output.
    pub fn rules(rules: &[(&str, Option<Reply>)]) -> Tools {
        let rules: Vec<(String, Option<Reply>)> = rules.iter().map(|(k, r)| (k.to_string(), r.clone())).collect();
        Tools::new(move |program, args| {
            let line = std::iter::once(program).chain(args.iter().copied()).collect::<Vec<_>>().join(" ");
            match rules.iter().find(|(k, _)| line.contains(k.as_str())) {
                Some((_, r)) => r.clone(),
                None => ok(""),
            }
        })
    }

    /// Every call so far, as `program arg arg…`.
    pub fn calls(&self) -> Vec<String> {
        self.0.borrow().calls.clone()
    }

    /// Whether a call containing `text` was made.
    pub fn called(&self, text: &str) -> bool {
        self.calls().iter().any(|c| c.contains(text))
    }

    /// The value of `name` in the environment of the last call containing `text`.
    pub fn env_of(&self, text: &str, name: &str) -> Option<String> {
        let st = self.0.borrow();
        let at = st.calls.iter().rposition(|c| c.contains(text))?;
        st.envs[at].iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }
}

impl Drop for Tools {
    fn drop(&mut self) {
        STATE.with(|s| *s.borrow_mut() = None);
    }
}

/// Whether a test scripts the tools on this thread (so nothing real would run).
#[cfg(windows)]
pub fn scripted() -> bool {
    STATE.with(|s| s.borrow().is_some())
}

/// The scripted answer to `program args`, when a test scripts the tools on this thread.
pub(super) fn answer(program: &'static str, args: &[&str], env: &[(String, String)]) -> Option<Result<String>> {
    Some(match reply(program, args, env)? {
        Scripted::Reply(Reply::Out(out)) => Ok(out),
        Scripted::Reply(Reply::Fail(stderr)) => Err(super::failed(program, args, stderr)),
        Scripted::Missing => Err(Error::ToolMissing { tool: program }),
    })
}

/// [`answer`] for a streamed tool: its output lines (stderr too, on a failure) go to `on_line`.
pub(super) fn answer_stream(program: &'static str, args: &[&str], env: &[(String, String)], mut on_line: impl FnMut(String)) -> Option<Result<()>> {
    let scripted = reply(program, args, env)?;
    Some(scripted.streamed(&mut on_line, |stderr| super::failed(program, args, stderr), || Error::ToolMissing { tool: program }))
}

/// [`answer_stream`] for an install or removal step (`platform::steps`), whose program is any
/// path: a missing tool and a failure say what `run_step` says for them.
pub fn answer_step(program: &str, args: &[String], on_line: &mut impl FnMut(String)) -> Option<Result<()>> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let scripted = reply(program, &args, &[])?;
    Some(scripted.streamed(
        on_line,
        |_| Error::Invalid(format!("`{program}` exited with exit status: 1")),
        || Error::Invalid(format!("{program} is not available on this machine.")),
    ))
}

impl Scripted {
    /// The scripted run as a stream: its output lines (stderr too, on a failure) go to
    /// `on_line`, then its end; a failure as `failed` says it, a missing tool as `missing` does.
    fn streamed(self, on_line: &mut impl FnMut(String), failed: impl FnOnce(String) -> Error, missing: impl FnOnce() -> Error) -> Result<()> {
        match self {
            Scripted::Reply(Reply::Out(text)) => {
                text.lines().for_each(|l| on_line(l.to_string()));
                Ok(())
            }
            Scripted::Reply(Reply::Fail(text)) => {
                text.lines().for_each(|l| on_line(l.to_string()));
                Err(failed(text))
            }
            Scripted::Missing => Err(missing()),
        }
    }
}

/// The script's answer to `program args`, recorded with its environment; None when no test
/// scripts the tools on this thread.
fn reply(program: &str, args: &[&str], env: &[(String, String)]) -> Option<Scripted> {
    let state = STATE.with(|s| s.borrow().clone())?;
    let mut st = state.borrow_mut();
    st.calls.push(std::iter::once(program).chain(args.iter().copied()).collect::<Vec<_>>().join(" "));
    st.envs.push(env.to_vec());
    Some((st.script)(program, args).map_or(Scripted::Missing, Scripted::Reply))
}

/// Whether process `pid` is gone (or a zombie) within a few seconds.
pub async fn gone(pid: u32) -> bool {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};
    for _ in 0..60 {
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true, ProcessRefreshKind::nothing());
        if sys.process(Pid::from_u32(pid)).is_none_or(|p| matches!(p.status(), ProcessStatus::Zombie | ProcessStatus::Dead)) {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    false
}
