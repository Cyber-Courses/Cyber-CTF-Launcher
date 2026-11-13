//! The attack box shell inside the app: a pseudo-terminal running the very command the runner
//! builds for the attack box, a machine of the lab (`docker compose … exec … bash`, or
//! `vagrant ssh …`), streamed to an xterm.js view. No Terminal.app window with a title we can't control, and no
//! macOS Automation prompt for driving Terminal. Closing the window ends the session, which ends
//! the `exec`/`ssh` it holds — the box itself is taken out by detaching it, through the runner.

use std::collections::HashMap;
use std::future::Future;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::runtime::{Runtime, ShellCommand};

/// Where a lab's attack box shell runs, as the runtime knows it. The command adapter answers it
/// over the app, so the shell itself never touches Tauri.
pub trait ShellTargets {
    /// The command the runner builds to open a shell into lab `id`'s attack box on `runtime`.
    fn command(&self, id: &str, runtime: Runtime) -> impl Future<Output = Result<ShellCommand>> + Send;
}

struct Session {
    /// Where its input goes: an [`Input`] (never the PTY itself, whose write can block).
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    /// The window showing it: closing that window ends it (the page's own cleanup never runs
    /// when its webview is destroyed).
    window: String,
}

/// Kills a session's shell and reaps it, off the calling thread (sync commands run on the main
/// thread): a killed child that is never waited on stays a zombie while the app runs. The shell's
/// `exec`/`ssh` ends with it; the box stays up until the learner detaches it.
fn end(mut sess: Session) {
    kill_shell(sess.child.as_mut());
    std::thread::spawn(move || {
        if let Err(e) = sess.child.wait() {
            log::warn!("couldn't reap an attack-box shell ({:?}): {e}", sess.child.process_id());
        }
    });
}

/// The exit code of a shell that ended (`waited`: its reap); none when it can't be reaped, which
/// is logged as [`end`]'s is.
fn reaped(pid: Option<u32>, waited: std::io::Result<portable_pty::ExitStatus>) -> Option<u32> {
    match waited {
        Ok(status) => Some(status.exit_code()),
        Err(e) => {
            log::warn!("couldn't reap an attack-box shell ({pid:?}): {e}");
            None
        }
    }
}

/// Kills a session's shell; one that can't be killed is logged: its `docker exec` or ssh is left
/// running, and its terminal with it.
fn kill_shell(child: &mut (dyn Child + Send + Sync)) {
    if let Err(e) = child.kill() {
        log::warn!("couldn't end an attack-box shell ({:?}): {e}", child.process_id());
    }
}

/// A session's input, written to its PTY by a thread of its own. A shell that isn't reading (a
/// wedged `docker exec`, a dead ssh) blocks a write once the PTY's buffer (about 4 KB) is full:
/// written directly, a paste then held the sessions lock on the main thread (a sync command),
/// and every window and every other terminal froze with it.
struct Input(std::sync::mpsc::SyncSender<Vec<u8>>);

/// Writes (keystrokes, pastes) a session holds while its shell doesn't read; past that its input
/// is refused rather than held.
const INPUT_QUEUE: usize = 1024;

impl Input {
    fn spawn(mut pty: Box<dyn Write + Send>) -> Input {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(INPUT_QUEUE);
        // Ends when the session is gone (its sender dropped), or its PTY with it.
        std::thread::spawn(move || {
            for chunk in rx {
                if pty.write_all(&chunk).and_then(|()| pty.flush()).is_err() {
                    break;
                }
            }
        });
        Input(tx)
    }
}

impl Write for Input {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        use std::sync::mpsc::TrySendError;
        match self.0.try_send(buf.to_vec()) {
            Ok(()) => Ok(buf.len()),
            Err(TrySendError::Full(_)) => Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "the shell isn't reading its input")),
            Err(TrySendError::Disconnected(_)) => Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "the shell has ended")),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The app's open shells, by session. Made once, where the app is put together (`app::services`),
/// and reached through the app's state: a cheap handle, which each shell's threads keep a copy of.
#[derive(Clone, Default)]
pub struct Terminals(Arc<Open>);

/// What [`Terminals`] holds.
#[derive(Default)]
struct Open {
    sessions: Mutex<HashMap<u32, Session>>,
    /// The last session's number: the next one is one more (the first is 1).
    last: AtomicU32,
}

impl Terminals {
    /// A new session's number.
    fn next_id(&self) -> u32 {
        self.0.last.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn with_sessions<T>(&self, f: impl FnOnce(&mut HashMap<u32, Session>) -> T) -> T {
        f(&mut self.0.sessions.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}

/// What an attack box shell sends the webview.
#[derive(Clone, Serialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TermEvent {
    /// Output, as text (split multi-byte characters are held back until complete).
    Data {
        /// The shell's output since the last event, as UTF-8.
        data: String,
    },
    /// The shell ended.
    Exit {
        /// Its exit code, when it has one.
        code: Option<u32>,
    },
}

/// Splits `buf` into the text that is complete (invalid bytes replaced) and the bytes of a
/// character cut off at its end, kept for the next read whatever came before them.
fn take_text(buf: &mut Vec<u8>) -> String {
    let complete = buf.len() - incomplete_tail(buf);
    let out = String::from_utf8_lossy(&buf[..complete]).into_owned();
    buf.drain(..complete);
    out
}

/// How many bytes at the end of `buf` begin a UTF-8 character that isn't whole yet (0 to 3):
/// a lead byte announcing more bytes than follow it.
fn incomplete_tail(buf: &[u8]) -> usize {
    for back in 1..=buf.len().min(3) {
        let need = match buf[buf.len() - back] {
            0x80..=0xBF => continue, // a continuation byte: look further back for its lead
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => return 0,
        };
        return if need > back { back } else { 0 };
    }
    0
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize { rows: rows.max(2), cols: cols.max(10), pixel_width: 0, pixel_height: 0 }
}

/// The shell a window asks for: lab `id`'s attack box, on `runtime`, at `cols` x `rows`.
pub struct Shell {
    /// The lab's id, its folder under `<app data>/labs/`.
    pub id: String,
    /// Where the lab runs.
    pub runtime: Runtime,
    /// The terminal's width in characters.
    pub cols: u16,
    /// The terminal's height in lines.
    pub rows: u16,
}

impl Terminals {
    /// Starts `shell` in a pseudo-terminal for the window labelled `window`; its output goes to
    /// `send`, which answers false once nobody reads it. Returns the session.
    pub async fn open(&self, targets: &impl ShellTargets, window: &str, shell: Shell, send: impl FnMut(TermEvent) -> bool + Send + 'static) -> Result<u32> {
        let Shell { id, runtime, cols, rows } = shell;
        let session = self.next_id();
        let command = targets.command(&id, runtime).await?;
        let pair = native_pty_system().openpty(pty_size(cols, rows)).map_err(|e| Error::Invalid(format!("couldn't open a terminal: {e}")))?;
        let mut cmd = pty_command(&command);
        cmd.env("TERM", "xterm-256color");
        // Docker's "What's next" ad on exit.
        cmd.env("DOCKER_CLI_HINTS", "false");
        let child = pair.slave.spawn_command(cmd).map_err(|e| Error::Invalid(format!("couldn't start the shell: {e}")))?;
        drop(pair.slave);
        let reader = pair.master.try_clone_reader().map_err(|e| Error::Invalid(e.to_string()))?;
        let writer = Box::new(Input::spawn(pair.master.take_writer().map_err(|e| Error::Invalid(e.to_string()))?));
        let window = window.to_string();
        self.with_sessions(|s| s.insert(session, Session { writer, master: pair.master, child, window }));
        self.follow(session, reader, send);
        Ok(session)
    }

    /// Streams session `session`'s output to `send` until its shell ends, then its exit code.
    fn follow(&self, session: u32, mut reader: Box<dyn Read + Send>, send: impl FnMut(TermEvent) -> bool + Send + 'static) {
        // The code of a shell whose end was seen by the Windows watcher (which closed its session).
        let ended = std::sync::Arc::new(Mutex::new(None::<u32>));
        #[cfg(windows)]
        self.watch_exit(session, ended.clone());
        let terminals = self.clone();
        std::thread::spawn(move || {
            forward(&mut reader, send, |view_gone| {
                let Some(mut sess) = terminals.with_sessions(|s| s.remove(&session)) else {
                    return ended.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
                };
                if view_gone {
                    // Nobody reads it any more, and the shell may never end on its own: waiting
                    // for it would leak it (and this thread) for as long as the app runs.
                    end(sess);
                    return None;
                }
                reaped(sess.child.process_id(), sess.child.wait())
            });
        });
    }

    /// Windows: `ConPTY` keeps the output open until the pseudo-console is closed, not when the shell
    /// ends, so the reader would wait (and the view show a live shell) forever. Watch the shell
    /// itself; once it has ended, keep its code in `ended` and close its session, which ends the
    /// output. Stops when the session is closed some other way (the window went).
    #[cfg(windows)]
    fn watch_exit(&self, session: u32, ended: std::sync::Arc<Mutex<Option<u32>>>) {
        let terminals = self.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let status = terminals.with_sessions(|s| s.get_mut(&session).map(|sess| sess.child.try_wait()));
                let code = match status {
                    None => return,
                    Some(Ok(None)) => continue,
                    Some(Ok(Some(st))) => Some(st.exit_code()),
                    Some(Err(e)) => {
                        log::warn!("couldn't reap an attack-box shell (session {session}): {e}");
                        None
                    }
                };
                *ended.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = code;
                // Dropped outside the sessions lock: closing the pseudo-console can take a moment.
                let closed = terminals.with_sessions(|s| s.remove(&session));
                drop(closed);
                return;
            }
        });
    }
}

/// What the pseudo-terminal runs: the runner's command (`docker`/`vagrant`) with its arguments,
/// in the run's folder, with the variables it needs (a Vagrant run's state folder) — the same
/// program the system terminal would run, no shell wrapping.
fn pty_command(command: &ShellCommand) -> CommandBuilder {
    #[cfg(windows)]
    let program = {
        let path =
            command.env.iter().find(|(k, _)| k.eq_ignore_ascii_case("PATH")).map(|(_, v)| std::ffi::OsString::from(v)).or_else(|| std::env::var_os("PATH"));
        let exts = std::env::var("PATHEXT").unwrap_or_else(|_| DEFAULT_PATHEXT.to_owned());
        let dirs: Vec<std::path::PathBuf> = path.map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        windows_program(&command.program, &dirs, &exts, std::path::Path::is_file)
    };
    #[cfg(not(windows))]
    let program = std::path::PathBuf::from(&command.program);
    let mut c = CommandBuilder::new(program);
    c.args(&command.args);
    c.cwd(&command.cwd);
    for (name, value) in &command.env {
        c.env(name, value);
    }
    c
}

/// Windows' own `PATHEXT` when the variable isn't set.
#[cfg(any(windows, test))]
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// The file Windows runs for a bare `program` name: the first of `dirs` (PATH, in order) holding
/// `program` with one of `exts` (`PATHEXT`, `;`-separated), as cmd.exe looks it up. A name with a
/// folder or an extension of its own, or one found nowhere, is kept as it is. `portable-pty` took
/// the first file named `program` exactly, and Docker Desktop ships an extensionless `docker` (a
/// script for WSL and Git Bash) beside `docker.exe` in its `bin` folder: the attack box shell then
/// failed with "%1 is not a valid Win32 application".
#[cfg(any(windows, test))]
fn windows_program(program: &str, dirs: &[std::path::PathBuf], exts: &str, exists: impl Fn(&std::path::Path) -> bool) -> std::path::PathBuf {
    let bare = std::path::Path::new(program);
    if bare.extension().is_some() || bare.components().count() > 1 {
        return bare.to_path_buf();
    }
    let exts: Vec<&str> = exts.split(';').map(str::trim).filter(|e| !e.is_empty()).collect();
    dirs.iter()
        .flat_map(|dir| exts.iter().map(move |ext| dir.join(format!("{program}{}", ext.to_ascii_lowercase()))))
        .find(|candidate| exists(candidate))
        .unwrap_or_else(|| bare.to_path_buf())
}

/// Streams the shell's output to `send` as text until it ends (or `send` refuses more), then
/// sends its exit code from `exit`, told whether the view went away (`send` refused).
fn forward(reader: &mut dyn Read, mut send: impl FnMut(TermEvent) -> bool, exit: impl FnOnce(bool) -> Option<u32>) {
    let mut chunk = [0u8; 8192];
    let mut pending = Vec::new();
    let mut view_gone = false;
    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                pending.extend_from_slice(&chunk[..n]);
                let data = take_text(&mut pending);
                if !data.is_empty() && !send(TermEvent::Data { data }) {
                    view_gone = true;
                    break;
                }
            }
        }
    }
    let code = exit(view_gone);
    send(TermEvent::Exit { code });
}

impl Terminals {
    /// Sends keystrokes (or pasted text) to session `session`; only the window labelled `window`,
    /// which shows it, may. Queued for the session's input thread, so no write waits on the shell
    /// (nor holds the sessions lock while it does).
    pub fn write(&self, window: &str, session: u32, data: &[u8]) -> Result<()> {
        self.with_sessions(|s| match s.get_mut(&session).filter(|sess| sess.window == window) {
            Some(sess) => sess.writer.write_all(data).and_then(|()| sess.writer.flush()).map_err(Error::Io),
            None => Err(Error::Invalid("the shell has ended".into())),
        })
    }

    /// Follows the view's size, so full-screen tools (vim, less, htop) lay out right.
    pub fn resize(&self, window: &str, session: u32, cols: u16, rows: u16) -> Result<()> {
        self.with_sessions(|s| match s.get(&session).filter(|sess| sess.window == window) {
            Some(sess) => sess.master.resize(pty_size(cols, rows)).map_err(|e| Error::Invalid(e.to_string())),
            None => Ok(()),
        })
    }

    /// Ends the shell (the view was closed). Another window's shell is left alone.
    pub fn close(&self, window: &str, session: u32) {
        let owned = self.with_sessions(|s| if s.get(&session).is_some_and(|sess| sess.window == window) { s.remove(&session) } else { None });
        if let Some(sess) = owned {
            end(sess);
        }
    }

    /// Ends the shells a window was showing, when it is closed.
    pub fn close_window(&self, label: &str) {
        let ids: Vec<u32> = self.with_sessions(|s| s.iter().filter(|(_, v)| v.window == label).map(|(k, _)| *k).collect());
        for id in ids {
            if let Some(sess) = self.with_sessions(|s| s.remove(&id)) {
                end(sess);
            }
        }
    }

    /// Ends every shell, when the app exits.
    pub fn close_all(&self) {
        let all: Vec<Session> = self.with_sessions(|s| s.drain().map(|(_, v)| v).collect());
        for mut sess in all {
            kill_shell(sess.child.as_mut());
        }
    }
}

/// A page-supplied title, safe for a window's title bar: no control characters, kept short.
pub(crate) fn window_title(title: &str) -> String {
    title.chars().filter(|c| !c.is_control()).take(80).collect()
}

/// A lab shell window's label and page.
pub(crate) fn shell_window(id: &str, runtime: Runtime) -> (String, String) {
    (format!("shell-{id}"), format!("shell?id={id}&runtime={}", runtime.tag()))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod proptests;
