//! The in-app shell's commands: one pseudo-terminal per session, owned by its window.

use tauri::ipc::Channel;

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
    terminal::terminal_open(window, terminal::Shell { id, kind, runtime, cols, rows }, events).await
}

/// Types `data` into session `session` of this window.
#[tauri::command]
pub fn terminal_write(window: WebviewWindow, session: u32, data: String) -> Result<()> {
    terminal::terminal_write(window, session, data)
}

/// Resizes session `session` of this window.
#[tauri::command]
pub fn terminal_resize(window: WebviewWindow, session: u32, cols: u16, rows: u16) -> Result<()> {
    terminal::terminal_resize(window, session, cols, rows)
}

/// Ends session `session` of this window.
#[tauri::command]
pub fn terminal_close(window: WebviewWindow, session: u32) -> Result<()> {
    terminal::terminal_close(window, session)
}

/// Opens a shell window for lab `id`, titled `title`.
#[tauri::command]
pub fn terminal_window(app: AppHandle, id: String, kind: ShellKind, runtime: Runtime, title: String) -> Result<()> {
    terminal::terminal_window(app, id, kind, runtime, title)
}
