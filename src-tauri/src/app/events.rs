//! The events the core emits to the webview, by name: the webview listens by the same names,
//! generated from these (`Events.ts`, `testing/ts_bindings.rs`). One that can't be emitted is
//! logged: a quit refused for a running deploy, say, then shows no prompt and nothing says why.

use tauri::Emitter;

/// A quit was held back while deploys run; its payload is how many.
pub const QUIT_BLOCKED: &str = "quit-blocked";
/// The machine-setup window, open already, is asked to go to a step; its payload is the step.
pub const MACHINE_SETUP_STEP: &str = "machine-setup-step";
/// The website asks to start a lab here; its payload is the launch to confirm.
pub const LAUNCH_CONFIRM: &str = "launch-confirm";

/// Every event, by the name the webview knows it by (camelCase) and its name on the wire.
#[cfg(test)]
pub const ALL: [(&str, &str); 3] = [("quitBlocked", QUIT_BLOCKED), ("machineSetupStep", MACHINE_SETUP_STEP), ("launchConfirm", LAUNCH_CONFIRM)];

/// Emits `event` with `payload` from `to` (the app, or one window), logging one that can't be.
pub fn emit<S: serde::Serialize + Clone>(to: &impl Emitter<crate::AppRuntime>, event: &str, payload: S) {
    emitted(event, to.emit(event, payload));
}

/// Logs event `event` that couldn't be emitted.
fn emitted(event: &str, result: tauri::Result<()>) {
    if let Err(e) = result {
        log::warn!("the event \"{event}\" couldn't reach the webview: {e}");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_event_that_cant_be_emitted_is_logged() {
        crate::testing::logs::take_warnings();
        super::emitted(super::QUIT_BLOCKED, Ok(()));
        super::emitted(super::QUIT_BLOCKED, Err(tauri::Error::WindowNotFound));
        let warned = crate::testing::logs::take_warnings();
        assert_eq!(warned.len(), 1, "{warned:?}");
        assert!(warned[0].starts_with("the event \"quit-blocked\" couldn't reach the webview: "), "{warned:?}");
    }
}
