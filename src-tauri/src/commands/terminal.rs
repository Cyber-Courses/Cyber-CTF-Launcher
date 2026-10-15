//! The in-app shell's commands: one pseudo-terminal per session, owned by its window.

use tauri::Manager;
use tauri::ipc::Channel;

use crate::app::window;
use crate::error::Result;
use crate::runtime::{Runtime, ShellKind};
use crate::terminal::{self, TermEvent};
use crate::{AppHandle, WebviewWindow};

/// Starts a lab's shell in a pseudo-terminal, its output sent on `events`; the session.
#[tauri::command]
#[allow(clippy::too_many_arguments, reason = "the webview sends each of the shell's fields as an argument")]
pub async fn terminal_open(
    window: WebviewWindow,
    id: String,
    kind: ShellKind,
    runtime: Runtime,
    cols: u16,
    rows: u16,
    events: Channel<TermEvent>,
) -> Result<u32> {
    let shell = terminal::Shell { id, kind, runtime, cols, rows };
    terminal::open(window.app_handle(), window.label(), shell, move |e| events.send(e).is_ok()).await
}

/// Types `data` into session `session` of this window.
#[tauri::command]
pub fn terminal_write(window: WebviewWindow, session: u32, data: &str) -> Result<()> {
    terminal::write(window.label(), session, data.as_bytes())
}

/// Resizes session `session` of this window.
#[tauri::command]
pub fn terminal_resize(window: WebviewWindow, session: u32, cols: u16, rows: u16) -> Result<()> {
    terminal::resize(window.label(), session, cols, rows)
}

/// Ends session `session` of this window.
#[tauri::command]
pub fn terminal_close(window: WebviewWindow, session: u32) {
    terminal::close(window.label(), session);
}

/// Opens a shell window for lab `id`, titled `title`.
#[tauri::command]
pub fn terminal_window(app: AppHandle, id: &str, kind: ShellKind, runtime: Runtime, title: &str) -> Result<()> {
    window::open_shell(&app, id, kind, runtime, title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::test_app::{Sink, TestApp};

    #[test]
    fn a_window_reaches_no_session_it_does_not_show() {
        let t = TestApp::new();
        let window = window::builder(t.handle(), "shell-lab-unknown", "").build().unwrap();
        assert_eq!(terminal_write(window.clone(), u32::MAX, "x").unwrap_err().to_string(), "the shell has ended");
        assert!(terminal_resize(window.clone(), u32::MAX, 80, 24).is_ok());
        terminal_close(window, u32::MAX);
    }

    #[tokio::test]
    async fn a_shell_for_a_lab_that_is_not_installed_starts_nothing() {
        let t = TestApp::new();
        let window = window::builder(t.handle(), "shell-lab-gone", "").build().unwrap();
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        let events = Sink::default();
        let err = terminal_open(window, "gone".into(), ShellKind::Lab, Runtime::Docker, 80, 24, events.channel()).await.unwrap_err();
        assert_eq!(err.to_string(), "lab `gone` is not installed");
        assert_eq!((events.messages(), tools.calls()), (Vec::new(), Vec::new()));
    }

    #[test]
    fn a_lab_s_shell_window_opens_once() {
        let t = TestApp::new();
        terminal_window(t.handle().clone(), "sqli", ShellKind::Lab, Runtime::Docker, "SQLi").unwrap();
        terminal_window(t.handle().clone(), "sqli", ShellKind::Lab, Runtime::Docker, "SQLi").unwrap();
        assert_eq!(t.handle().webview_windows().len(), 1);
    }
}
