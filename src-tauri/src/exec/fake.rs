//! Unit tests' stand-in for the tools: while a test holds a [`Tools`] guard, every `run*` and
//! `stream*` on its thread answers from the test's script instead of starting a process, so code
//! that drives docker, vagrant or terraform runs without them (and never touches real ones). The
//! install and removal steps (`platform::steps`) answer from it too: an install test never runs
//! a package manager.
//! Async tests run on one thread (`#[tokio::test]`), so the script is per thread: parallel tests
//! don't see each other's.

use std::cell::RefCell;

use crate::error::{Error, Result};

/// A scripted tool's answer: its stdout (each line streamed, for `stream`), or its failure.
#[derive(Clone)]
pub enum Reply {
    Out(String),
    Fail(String),
}

pub fn ok(stdout: impl Into<String>) -> Option<Reply> {
    Some(Reply::Out(stdout.into()))
}

pub fn fail(stderr: impl Into<String>) -> Option<Reply> {
    Some(Reply::Fail(stderr.into()))
}

type Script = Box<dyn FnMut(&str, &[&str]) -> Option<Reply>>;

struct State {
    script: Script,
    calls: Vec<String>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// The scripted tools of the running test, until dropped. A call the script doesn't answer
/// (`None`) is a missing tool.
pub struct Tools;

impl Tools {
    pub fn new(script: impl FnMut(&str, &[&str]) -> Option<Reply> + 'static) -> Tools {
        STATE.with(|s| *s.borrow_mut() = Some(State { script: Box::new(script), calls: Vec::new() }));
        Tools
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
        STATE.with(|s| s.borrow().as_ref().map(|st| st.calls.clone()).unwrap_or_default())
    }

    /// Whether a call containing `text` was made.
    pub fn called(&self, text: &str) -> bool {
        self.calls().iter().any(|c| c.contains(text))
    }
}

impl Drop for Tools {
    fn drop(&mut self) {
        STATE.with(|s| *s.borrow_mut() = None);
    }
}

/// Whether a test scripts the tools on this thread (so nothing real would run).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn scripted() -> bool {
    STATE.with(|s| s.borrow().is_some())
}

/// The scripted answer to `program args`, when a test scripts the tools on this thread.
pub(super) fn answer(program: &'static str, args: &[&str]) -> Option<Result<String>> {
    Some(match reply(program, args)? {
        Some(Reply::Out(out)) => Ok(out),
        Some(Reply::Fail(stderr)) => Err(super::failed(program, args, stderr)),
        None => Err(Error::ToolMissing { tool: program }),
    })
}

/// [`answer`] for a streamed tool: its output lines (stderr too, on a failure) go to `on_line`.
pub(super) fn answer_stream(program: &'static str, args: &[&str], mut on_line: impl FnMut(String)) -> Option<Result<()>> {
    let reply = reply(program, args)?;
    if let Some(Reply::Out(text) | Reply::Fail(text)) = &reply {
        text.lines().for_each(|l| on_line(l.to_string()));
    }
    Some(match reply {
        Some(Reply::Out(_)) => Ok(()),
        Some(Reply::Fail(stderr)) => Err(super::failed(program, args, stderr)),
        None => Err(Error::ToolMissing { tool: program }),
    })
}

/// [`answer_stream`] for an install or removal step (`platform::steps`), whose program is any
/// path: a missing tool and a failure say what `run_step` says for them.
pub fn answer_step(program: &str, args: &[String], on_line: &mut impl FnMut(String)) -> Option<Result<()>> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let reply = reply(program, &args)?;
    if let Some(Reply::Out(text) | Reply::Fail(text)) = &reply {
        text.lines().for_each(|l| on_line(l.to_string()));
    }
    Some(match reply {
        Some(Reply::Out(_)) => Ok(()),
        Some(Reply::Fail(_)) => Err(Error::Invalid(format!("`{program}` exited with exit status: 1"))),
        None => Err(Error::Invalid(format!("{program} is not available on this machine."))),
    })
}

fn reply(program: &str, args: &[&str]) -> Option<Option<Reply>> {
    STATE.with(|s| {
        let mut guard = s.borrow_mut();
        let st = guard.as_mut()?;
        st.calls.push(std::iter::once(program).chain(args.iter().copied()).collect::<Vec<_>>().join(" "));
        Some((st.script)(program, args))
    })
}
