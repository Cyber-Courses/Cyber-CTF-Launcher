//! The run events the webview is driven by: each operation's start, its log lines and its end,
//! numbered by a cursor. The log keeps what was emitted (up to [`KEPT`] events), so a window that
//! (re)loads replays what it missed ([`EventLog::since`]) and then follows the live ones, the
//! same Tauri event ([`RUN_EVENT`]) for every window.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;

use super::OperationId;
use crate::error::Wire;
use crate::runtime::{Operation, Runtime};

pub use crate::app::events::RUN_EVENT;

/// How many events are kept for a window that (re)loads: a VM start's log is some thousands of
/// lines, and a window wants the ones of the operations still running and the last ones ended.
const KEPT: usize = 50_000;

/// What happened to a lab operation.
#[derive(Serialize, Clone, Debug, PartialEq, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RunEvent {
    /// The operation began (or this app found it running, started by an earlier run of the app).
    Started {
        /// The operation.
        operation: OperationId,
        /// The lab it acts on.
        lab_id: String,
        /// What it is.
        op: Operation,
        /// The machine it targets, when one.
        machine: Option<String>,
        /// The runtime the lab is operated in, when the operation says.
        runtime: Option<Runtime>,
        /// Whether the UI shows the lab as deploying meanwhile.
        deploying: bool,
        /// When it began, in milliseconds since the Unix epoch.
        at: u64,
    },
    /// One line of the operation's log.
    Log {
        /// The operation.
        operation: OperationId,
        /// The lab it acts on.
        lab_id: String,
        /// The line.
        line: String,
    },
    /// The operation ended: done, or failed with `error`.
    Ended {
        /// The operation.
        operation: OperationId,
        /// The lab it acted on.
        lab_id: String,
        /// What it was.
        op: Operation,
        /// Why it failed; none when it went through.
        error: Option<Wire>,
    },
}

/// An event with its place among every event: a window reloading asks for the ones after the last
/// place it saw.
#[derive(Serialize, Clone, Debug, PartialEq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Cursored {
    /// Its place, from 1.
    pub cursor: u64,
    /// The event.
    pub event: RunEvent,
}

/// What a listener does with each event as it is emitted (the app: emit it to the webview).
pub type Listener = Box<dyn Fn(&Cursored) + Send + Sync>;

/// The events emitted so far, and who hears the next ones.
#[derive(Default)]
pub struct EventLog {
    /// The kept events and the next cursor.
    inner: Mutex<Kept>,
    /// Told of every event as it is emitted, in order.
    listeners: Mutex<Vec<Listener>>,
    /// Woken at every event, for [`EventLog::ended`].
    emitted: tokio::sync::Notify,
}

/// The kept events, oldest first, and the last cursor given.
#[derive(Default)]
struct Kept {
    /// The last cursor given (0: none yet).
    last: u64,
    /// The kept events.
    events: VecDeque<Cursored>,
}

impl EventLog {
    /// Emits `event`: kept, then told to every listener, in the order emitted.
    pub fn emit(&self, event: RunEvent) {
        let cursored = {
            let mut kept = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            kept.last += 1;
            let cursored = Cursored { cursor: kept.last, event };
            kept.events.push_back(cursored.clone());
            while kept.events.len() > KEPT {
                kept.events.pop_front();
            }
            cursored
        };
        for listener in self.listeners.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter() {
            listener(&cursored);
        }
        self.emitted.notify_waiters();
    }

    /// Waits for operation `operation`'s end: `Ok` when it went through, its error when it failed.
    ///
    /// # Errors
    ///
    /// The operation's failure.
    pub async fn ended(&self, operation: &OperationId) -> Result<(), Wire> {
        loop {
            let emitted = self.emitted.notified();
            tokio::pin!(emitted);
            emitted.as_mut().enable();
            if let Some(outcome) = self.end_of(operation) {
                return outcome;
            }
            emitted.await;
        }
    }

    /// Operation `operation`'s outcome among the kept events, once it ended.
    fn end_of(&self, operation: &OperationId) -> Option<Result<(), Wire>> {
        let kept = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        kept.events.iter().rev().find_map(|e| match &e.event {
            RunEvent::Ended { operation: o, error, .. } if o == operation => Some(error.clone().map_or(Ok(()), Err)),
            _ => None,
        })
    }

    /// Adds `listener`, told of every event from now on.
    pub fn listen(&self, listener: Listener) {
        self.listeners.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(listener);
    }

    /// The kept events after cursor `after` (all of them for none).
    pub fn since(&self, after: Option<u64>) -> Vec<Cursored> {
        let kept = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        kept.events.iter().filter(|e| after.is_none_or(|a| e.cursor > a)).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(operation: &str, line: &str) -> RunEvent {
        RunEvent::Log { operation: OperationId(operation.into()), lab_id: "web".into(), line: line.into() }
    }

    #[test]
    fn a_window_replays_what_it_missed_and_listeners_hear_each_event_in_order() {
        let events = EventLog::default();
        let heard = std::sync::Arc::new(Mutex::new(Vec::new()));
        let h = heard.clone();
        events.listen(Box::new(move |e| h.lock().unwrap().push(e.cursor)));
        for i in 0..3 {
            events.emit(log("op-1", &format!("line {i}")));
        }
        assert_eq!(*heard.lock().unwrap(), [1, 2, 3]);
        assert_eq!(events.since(Some(1)).iter().map(|e| e.cursor).collect::<Vec<_>>(), [2, 3]);
        assert_eq!(events.since(None).len(), 3);
        assert_eq!(events.since(Some(3)), []);
    }

    #[tokio::test]
    async fn a_waiter_hears_an_operations_end_and_how_it_ended() {
        let events = std::sync::Arc::new(EventLog::default());
        let op = OperationId("op-1".into());
        let waiter = {
            let (events, op) = (std::sync::Arc::clone(&events), op.clone());
            tokio::spawn(async move { events.ended(&op).await })
        };
        tokio::task::yield_now().await;
        let error = Wire { code: crate::error::Code::Other, message: "no".into() };
        events.emit(RunEvent::Ended { operation: op.clone(), lab_id: "web".into(), op: Operation::Launch, error: Some(error.clone()) });
        assert_eq!(waiter.await.unwrap(), Err(error));
        // Already ended: answered at once.
        assert!(events.ended(&op).await.is_err());
    }

    #[test]
    fn the_events_go_to_the_webview_in_its_shape() {
        let e = Cursored { cursor: 7, event: log("op-1", "==> Starting") };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({ "cursor": 7, "event": { "kind": "log", "operation": "op-1", "labId": "web", "line": "==> Starting" } })
        );
    }
}
