//! What a self-test reports, this machine's (`machine::selftest`) and a server's
//! (`server_selftest`) alike, so both share one frontend type: one [`Event`] per step, `running`,
//! then `ok`, `fail` or `skip`, sent through a [`Reporter`].

use std::future::Future;

use serde::Serialize;

use crate::error::Result;

/// One step's state in a self-test, as the page that runs it shows it.
#[derive(Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SelfTestEvent")]
pub struct Event {
    /// The step's id, e.g. `network` or `cleanup`.
    pub step: &'static str,
    /// What the step does, as the page names it.
    pub label: &'static str,
    /// `running`, `ok`, `fail` or `skip`.
    #[ts(type = r#""running" | "ok" | "fail" | "skip""#)]
    pub state: &'static str,
    /// What the step found, its live output while it runs, or why it failed or was skipped; None
    /// when there is nothing to add.
    pub detail: Option<String>,
    /// How many bytes the step moves, when known (the image it uploads): its running events
    /// carry it once it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub size: Option<u64>,
}

/// Longest live output line shown under a running step, in characters.
const PROGRESS_MAX: usize = 160;

/// Where a test's events go (the page that runs it), and how a failed step's error is said.
pub(crate) struct Reporter {
    sink: Box<dyn Fn(Event) + Send + Sync>,
    explain: fn(&str) -> String,
}

impl Reporter {
    /// A reporter that hands each event to `sink`, a failed step saying its error as it is.
    pub(crate) fn new(sink: impl Fn(Event) + Send + Sync + 'static) -> Reporter {
        Reporter::explaining(sink, str::to_owned)
    }

    /// A reporter that hands each event to `sink`, a failed step saying its error through
    /// `explain` (a hint for the failures people hit, in front of the raw error).
    pub(crate) fn explaining(sink: impl Fn(Event) + Send + Sync + 'static, explain: fn(&str) -> String) -> Reporter {
        Reporter { sink: Box::new(sink), explain }
    }

    /// Sends step `step` (`label`) in `state`, with `detail`.
    pub(crate) fn send(&self, step: &'static str, label: &'static str, state: &'static str, detail: Option<String>) {
        (self.sink)(Event { step, label, state, detail, size: None });
    }

    /// Live output for a step that is still running, as its detail line: trimmed, at most
    /// [`PROGRESS_MAX`] characters; a blank line is not sent.
    pub(crate) fn progress(&self, step: &'static str, label: &'static str, line: &str) {
        self.progress_sized(step, label, line, None);
    }

    /// [`Reporter::progress`], with how many bytes the step moves when known (`size`).
    pub(crate) fn progress_sized(&self, step: &'static str, label: &'static str, line: &str, size: Option<u64>) {
        let line = line.trim();
        if !line.is_empty() {
            (self.sink)(Event { step, label, state: "running", detail: Some(line.chars().take(PROGRESS_MAX).collect()), size });
        }
    }

    /// Runs one step, reporting it: `running`, then `ok` with the future's detail or `fail` with
    /// its error; a failure stops the test with that step's error.
    pub(crate) async fn step<T>(&self, step: &'static str, label: &'static str, fut: impl Future<Output = Result<(T, Option<String>)>>) -> Result<T> {
        self.send(step, label, "running", None);
        let outcome = fut.await;
        let (state, detail) = match &outcome {
            Ok((_, detail)) => ("ok", detail.clone()),
            Err(e) => ("fail", Some((self.explain)(&e.to_string()))),
        };
        self.send(step, label, state, detail);
        outcome.map(|(v, _)| v)
    }

    /// The closing clean-up step, reported only when the test itself passed (a failed test
    /// already says what went wrong).
    pub(crate) fn cleanup<T>(&self, label: &'static str, test: &Result<()>, done: Result<T>, ok_detail: Option<String>) {
        match (test, done) {
            (Ok(()), Ok(_)) => self.send("cleanup", label, "ok", ok_detail),
            (Ok(()), Err(e)) => self.send("cleanup", label, "fail", Some(e.to_string())),
            (Err(_), _) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::error::Error;

    /// A reporter (failures said through `explain`) and the events it sent, as `step:state`
    /// and their details.
    fn reporter(explain: fn(&str) -> String) -> (Reporter, Arc<Mutex<Vec<Event>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        (Reporter::explaining(move |e| sink.lock().unwrap().push(e), explain), seen)
    }

    fn states(seen: &Mutex<Vec<Event>>) -> Vec<String> {
        seen.lock().unwrap().iter().map(|e| format!("{}:{}", e.step, e.state)).collect()
    }

    #[tokio::test]
    async fn a_step_reports_running_then_ok_or_fail_said_through_its_explanation() {
        let (r, seen) = reporter(|raw| format!("hint: {raw}"));
        let v = r.step("one", "First", async { Ok((7, Some("fine".into()))) }).await.unwrap();
        assert_eq!(v, 7);
        let err = r.step::<()>("two", "Second", async { Err(Error::Invalid("broke".into())) }).await.unwrap_err();
        assert_eq!(err.to_string(), "broke", "the step's own error is returned unexplained");
        assert_eq!(states(&seen), ["one:running", "one:ok", "two:running", "two:fail"]);
        let events = seen.lock().unwrap();
        assert_eq!(events[1].detail.as_deref(), Some("fine"));
        assert_eq!(events[3].detail.as_deref(), Some("hint: broke"));
    }

    #[test]
    fn progress_lines_are_trimmed_and_short_and_cleanup_reports_only_after_a_pass() {
        let (r, seen) = reporter(str::to_owned);
        r.progress("box", "Box", "   ");
        r.progress("box", "Box", &format!("  {}  ", "x".repeat(500)));
        r.cleanup::<()>("Clean", &Ok(()), Ok(()), Some("kept".into()));
        r.cleanup::<()>("Clean", &Ok(()), Err(Error::Invalid("left over".into())), None);
        r.cleanup::<()>("Clean", &Err(Error::Invalid("x".into())), Ok(()), None);
        assert_eq!(states(&seen), ["box:running", "cleanup:ok", "cleanup:fail"]);
        let events = seen.lock().unwrap();
        assert_eq!(events[0].detail.as_ref().unwrap().chars().count(), PROGRESS_MAX);
        assert_eq!(events[2].detail.as_deref(), Some("left over"));
    }
}
