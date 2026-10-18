//! The runner's events as the launcher's progress: the lines of a deploy log (the lab page shows
//! them as they come, and the sidebar takes its step from the last one). A step's own line (the
//! manifest's note for it), what its tool prints, and what the runner warns about are shown;
//! state moves, outputs and the final result or error are not (the caller says how it ended).

use isothrum_runner::{Event, EventKind, Events, ReadyState, StepState, WarningCode};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// An operation's event sink, and the lines it sends: the sink crosses threads (the runner's
/// contract), the lines are read on the caller's task ([`follow`]).
pub(super) fn channel() -> (Lines, UnboundedReceiver<String>) {
    let (tx, rx) = unbounded_channel();
    (Lines(tx), rx)
}

/// Runs `op` (an operation given the sink of [`channel`]), each line `rx` receives from it
/// reaching `log` as it comes, in order.
pub(super) async fn follow<T>(log: &mut impl FnMut(String), mut rx: UnboundedReceiver<String>, op: impl Future<Output = T>) -> T {
    let mut running = std::pin::pin!(op);
    loop {
        tokio::select! {
            biased;
            Some(line) = rx.recv() => log(line),
            out = &mut running => {
                while let Ok(line) = rx.try_recv() {
                    log(line);
                }
                return out;
            }
        }
    }
}

/// An event sink that discards every line: for an operation with no deploy log to show (the
/// auto-stop reaper).
pub(super) fn silent() -> Lines {
    channel().0
}

/// Sends each event's line, if it has one, to the caller's task.
pub(super) struct Lines(UnboundedSender<String>);

impl Events for Lines {
    fn emit(&self, event: &Event) {
        if let Some(text) = line(&event.kind) {
            // The receiver lives as long as the operation: a send can only fail once it's over.
            let _ = self.0.send(text);
        }
    }
}

/// The deploy log's line for an event, if it gets one.
pub(super) fn line(kind: &EventKind) -> Option<String> {
    match kind {
        EventKind::Step { state: StepState::Running, note: Some(note), .. } => Some(note.clone()),
        EventKind::Log { line, .. } => Some(line.clone()),
        EventKind::Ready { state: ReadyState::Waiting, detail, .. } if !detail.is_empty() => Some(format!("Waiting for the lab to be ready: {detail}")),
        EventKind::Warning { code, message } => warning(*code, message),
        _ => None,
    }
}

/// What the player reads for one of the runner's warnings (its codes; its message otherwise).
fn warning(code: WarningCode, message: &str) -> Option<String> {
    match code {
        WarningCode::LeftoversRemoved => Some("Clearing a previous, stopped run…".into()),
        WarningCode::Recovering => Some("Docker kept something of an earlier run: clearing it, then starting again…".into()),
        WarningCode::HookFailed => {
            Some(format!("The lab is fine, but its attack box didn't follow: {}", message.split_once(": ").map_or(message, |(_, why)| why)))
        }
        // A best-effort step (an old network left in place): nothing the player can act on.
        WarningCode::StepTolerated => None,
        _ => Some(message.to_owned()),
    }
}
