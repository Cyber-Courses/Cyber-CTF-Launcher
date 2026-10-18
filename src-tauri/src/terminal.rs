//! The attack box shell inside the app: a pseudo-terminal running the very command the runner
//! builds for the on-demand access box (`docker compose … exec … bash`, or `vagrant ssh …`),
//! streamed to an xterm.js view. No Terminal.app window with a title we can't control, and no
//! macOS Automation prompt for driving Terminal. Closing the window ends the session, which ends
//! the `exec`/`ssh` it holds — the box itself is taken out by detaching it, through the runner.

use std::collections::HashMap;
use std::future::Future;
use std::io::{Read, Write};
use std::sync::Mutex;

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
    let _ = sess.child.kill();
    std::thread::spawn(move || {
        let _ = sess.child.wait();
    });
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

static SESSIONS: Mutex<Option<HashMap<u32, Session>>> = Mutex::new(None);
static NEXT_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

fn with_sessions<T>(f: impl FnOnce(&mut HashMap<u32, Session>) -> T) -> T {
    let mut guard = SESSIONS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(HashMap::new))
}

#[derive(Clone, Serialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TermEvent {
    /// Output, as text (split multi-byte characters are held back until complete).
    Data { data: String },
    /// The shell ended.
    Exit { code: Option<u32> },
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
    /// The lab.
    pub id: String,
    /// Where the lab runs.
    pub runtime: Runtime,
    /// The terminal's width in characters.
    pub cols: u16,
    /// The terminal's height in lines.
    pub rows: u16,
}

/// Starts `shell` in a pseudo-terminal for the window labelled `window`; its output goes to
/// `send`, which answers false once nobody reads it. Returns the session.
pub async fn open(targets: &impl ShellTargets, window: &str, shell: Shell, send: impl FnMut(TermEvent) -> bool + Send + 'static) -> Result<u32> {
    let Shell { id, runtime, cols, rows } = shell;
    let session = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
    with_sessions(|s| s.insert(session, Session { writer, master: pair.master, child, window }));
    follow(session, reader, send);
    Ok(session)
}

/// Streams session `session`'s output to `send` until its shell ends, then its exit code.
fn follow(session: u32, mut reader: Box<dyn Read + Send>, send: impl FnMut(TermEvent) -> bool + Send + 'static) {
    // The code of a shell whose end was seen by the Windows watcher (which closed its session).
    let ended = std::sync::Arc::new(Mutex::new(None::<u32>));
    #[cfg(windows)]
    watch_exit(session, ended.clone());
    std::thread::spawn(move || {
        forward(&mut reader, send, |view_gone| {
            let Some(mut sess) = with_sessions(|s| s.remove(&session)) else {
                return ended.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
            };
            if view_gone {
                // Nobody reads it any more, and the shell may never end on its own: waiting
                // for it would leak it (and this thread) for as long as the app runs.
                end(sess);
                return None;
            }
            sess.child.wait().ok().map(|st| st.exit_code())
        });
    });
}

/// Windows: `ConPTY` keeps the output open until the pseudo-console is closed, not when the shell
/// ends, so the reader would wait (and the view show a live shell) forever. Watch the shell
/// itself; once it has ended, keep its code in `ended` and close its session, which ends the
/// output. Stops when the session is closed some other way (the window went).
#[cfg(windows)]
fn watch_exit(session: u32, ended: std::sync::Arc<Mutex<Option<u32>>>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(250));
            let status = with_sessions(|s| s.get_mut(&session).map(|sess| sess.child.try_wait()));
            let code = match status {
                None => return,
                Some(Ok(None)) => continue,
                Some(Ok(Some(st))) => Some(st.exit_code()),
                Some(Err(_)) => None,
            };
            *ended.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = code;
            // Dropped outside the sessions lock: closing the pseudo-console can take a moment.
            let closed = with_sessions(|s| s.remove(&session));
            drop(closed);
            return;
        }
    });
}

/// What the pseudo-terminal runs: the runner's command (`docker`/`vagrant`) with its arguments,
/// in the run's folder — the same program the system terminal would run, no shell wrapping.
fn pty_command(command: &ShellCommand) -> CommandBuilder {
    let mut c = CommandBuilder::new(&command.program);
    c.args(&command.args);
    if let Some(cwd) = &command.cwd {
        c.cwd(cwd);
    }
    c
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

/// Sends keystrokes (or pasted text) to session `session`; only the window labelled `window`,
/// which shows it, may. Queued for the session's input thread, so no write waits on the shell
/// (nor holds the sessions lock while it does).
pub fn write(window: &str, session: u32, data: &[u8]) -> Result<()> {
    with_sessions(|s| match s.get_mut(&session).filter(|sess| sess.window == window) {
        Some(sess) => sess.writer.write_all(data).and_then(|()| sess.writer.flush()).map_err(Error::Io),
        None => Err(Error::Invalid("the shell has ended".into())),
    })
}

/// Follows the view's size, so full-screen tools (vim, less, htop) lay out right.
pub fn resize(window: &str, session: u32, cols: u16, rows: u16) -> Result<()> {
    with_sessions(|s| match s.get(&session).filter(|sess| sess.window == window) {
        Some(sess) => sess.master.resize(pty_size(cols, rows)).map_err(|e| Error::Invalid(e.to_string())),
        None => Ok(()),
    })
}

/// Ends the shell (the view was closed). Another window's shell is left alone.
pub fn close(window: &str, session: u32) {
    let owned = with_sessions(|s| if s.get(&session).is_some_and(|sess| sess.window == window) { s.remove(&session) } else { None });
    if let Some(sess) = owned {
        end(sess);
    }
}

/// Ends the shells a window was showing, when it is closed.
pub fn close_window(label: &str) {
    let ids: Vec<u32> = with_sessions(|s| s.iter().filter(|(_, v)| v.window == label).map(|(k, _)| *k).collect());
    for id in ids {
        if let Some(sess) = with_sessions(|s| s.remove(&id)) {
            end(sess);
        }
    }
}

/// Ends every shell, when the app exits.
pub fn close_all() {
    let all: Vec<Session> = with_sessions(|s| s.drain().map(|(_, v)| v).collect());
    for mut sess in all {
        let _ = sess.child.kill();
    }
}

/// A page-supplied title, safe for a window's title bar: no control characters, kept short.
pub(crate) fn window_title(title: &str) -> String {
    title.chars().filter(|c| !c.is_control()).take(80).collect()
}

/// A lab shell window's label and page.
pub(crate) fn shell_window(id: &str, runtime: Runtime) -> (String, String) {
    let rt = match runtime {
        Runtime::Docker => "DOCKER",
        Runtime::Vm => "VM",
        Runtime::Cloud => "CLOUD",
    };
    (format!("shell-{id}"), format!("shell?id={id}&runtime={rt}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_characters_wait_for_their_last_byte() {
        let euro = "€".as_bytes(); // 3 bytes
        let mut buf = vec![b'a', euro[0], euro[1]];
        assert_eq!(take_text(&mut buf), "a");
        assert_eq!(buf, vec![euro[0], euro[1]]);
        buf.push(euro[2]);
        assert_eq!(take_text(&mut buf), "€");
        assert_eq!(buf, b"");
        let mut bad = vec![b'x', 0xff, b'y'];
        assert_eq!(take_text(&mut bad), "x\u{fffd}y");
    }

    #[test]
    fn a_four_byte_character_arriving_one_byte_at_a_time_is_held_back_until_whole() {
        // A PTY hands out whatever is ready, so an emoji can arrive split three ways.
        let crab = "\u{1f980}".as_bytes();
        let mut buf = Vec::new();
        for b in &crab[..3] {
            buf.push(*b);
            assert_eq!(take_text(&mut buf), "", "nothing is sent until the character is whole");
        }
        assert_eq!(buf.len(), 3);
        buf.push(crab[3]);
        assert_eq!(take_text(&mut buf), "\u{1f980}");
        assert_eq!(buf, b"");
    }

    #[test]
    fn a_character_split_at_the_end_is_held_back_even_after_an_invalid_byte() {
        // `take_text` only holds back the tail when the *first* error is the incomplete one. With
        // a stray byte before it (Latin-1 from a tool, a corrupted read), `from_utf8` reports that
        // byte first, the `Err(_)` arm decodes everything lossily and clears the buffer: the two
        // bytes of the split character become one U+FFFD here and its last byte another on the
        // next read, so the character is lost. Fix: on any error, still check whether the buffer
        // ends in an incomplete sequence (e.g. decode `valid_up_to..` with `error_len`, or use
        // `Utf8Chunks` and keep the last chunk's `invalid()` when it could be the start of a
        // character).
        let euro = "\u{20ac}".as_bytes();
        let mut buf = vec![b'x', 0xff, euro[0], euro[1]];
        assert_eq!(take_text(&mut buf), "x\u{fffd}");
        assert_eq!(buf, vec![euro[0], euro[1]], "the split character waits for its last byte");
        buf.push(euro[2]);
        assert_eq!(take_text(&mut buf), "\u{20ac}");
    }

    #[test]
    fn window_titles_drop_control_characters_and_stay_short() {
        assert_eq!(window_title("SQLi\n lab\u{7}"), "SQLi lab");
        assert_eq!(window_title(&"x".repeat(200)).chars().count(), 80);
    }

    #[cfg(unix)]
    #[test]
    fn a_pty_runs_a_command_and_reports_its_exit() {
        let pair = native_pty_system().openpty(pty_size(80, 24)).unwrap();
        let mut cmd = CommandBuilder::new("sh");
        cmd.args(["-c", "printf 'hi\\n'; exit 3"]);
        let mut child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut out = Vec::new();
        let mut chunk = [0u8; 256];
        while let Ok(n) = reader.read(&mut chunk) {
            if n == 0 {
                break;
            }
            out.extend_from_slice(&chunk[..n]);
            if out.windows(2).any(|w| w == b"hi") {
                break;
            }
        }
        assert!(String::from_utf8_lossy(&out).contains("hi"));
        assert_eq!(child.wait().unwrap().exit_code(), 3);
    }

    /// Windows: a shell that ends while its session (the pseudo-console) is still open reaches
    /// the view as an Exit, with its code, and doesn't leave the reading thread waiting forever.
    #[cfg(windows)]
    #[test]
    fn a_windows_shell_that_exits_reports_its_exit() {
        let pair = native_pty_system().openpty(pty_size(80, 24)).unwrap();
        let mut cmd = CommandBuilder::new("cmd");
        cmd.args(["/c", "echo hi& exit 3"]);
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let reader = pair.master.try_clone_reader().unwrap();
        let writer = Box::new(Input::spawn(pair.master.take_writer().unwrap()));
        // A live session, as terminal_open makes it: its window still open.
        let session = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        with_sessions(|s| s.insert(session, Session { writer, master: pair.master, child, window: "shell-test".into() }));
        let (tx, rx) = std::sync::mpsc::channel();
        follow(session, reader, move |e| tx.send(e).is_ok());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut out = String::new();
        let code = loop {
            match rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
                Ok(TermEvent::Data { data }) => {
                    // ConPTY asks where the cursor is (DSR) and holds all output until answered;
                    // in the app xterm.js answers it.
                    if data.contains("\x1b[6n") {
                        write("shell-test", session, b"\x1b[1;1R").unwrap();
                    }
                    out.push_str(&data);
                }
                Ok(TermEvent::Exit { code }) => break code,
                Err(e) => panic!("no Exit within 15 s ({e}); output so far: {out:?}"),
            }
        };
        assert!(out.contains("hi"), "{out:?}");
        assert_eq!(code, Some(3));
        assert!(with_sessions(|s| !s.contains_key(&session)), "the ended session is closed");
    }

    fn argv(c: &CommandBuilder) -> Vec<String> {
        c.get_argv().iter().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn the_pty_runs_the_runners_command_in_its_folder() {
        // The runner builds the command (program + args + the run's folder); the PTY runs it as
        // is, no shell wrapping, on every platform.
        let dir = std::env::temp_dir();
        let c = pty_command(&ShellCommand { program: "vagrant".into(), args: vec!["ssh".into(), "isoloom-controller".into()], cwd: Some(dir.clone()) });
        assert_eq!(argv(&c), ["vagrant", "ssh", "isoloom-controller"]);
        assert_eq!(c.get_cwd().map(std::path::PathBuf::from), Some(dir));
        let d =
            pty_command(&ShellCommand { program: "docker".into(), args: ["compose", "exec", "kali", "bash"].iter().map(|s| (*s).into()).collect(), cwd: None });
        assert_eq!(argv(&d), ["docker", "compose", "exec", "kali", "bash"]);
    }

    #[test]
    fn shell_windows_are_named_by_lab() {
        assert_eq!(shell_window("sqli", Runtime::Docker), ("shell-sqli".into(), "shell?id=sqli&runtime=DOCKER".into()));
        assert_eq!(shell_window("goad", Runtime::Vm), ("shell-goad".into(), "shell?id=goad&runtime=VM".into()));
        let size = pty_size(1, 1);
        assert_eq!((size.cols, size.rows), (10, 2));
    }

    /// The events a view accepting `accept` of them gets, and whether the shell's end was told
    /// the view went away.
    fn events_and_end(input: &[u8], accept: usize) -> (Vec<String>, bool) {
        let mut out = Vec::new();
        let mut gone = None;
        let mut reader = std::io::Cursor::new(input.to_vec());
        forward(
            &mut reader,
            |e| {
                out.push(serde_json::to_string(&e).unwrap());
                out.len() <= accept
            },
            |view_gone| {
                gone = Some(view_gone);
                Some(7)
            },
        );
        (out, gone.expect("the end is always reached"))
    }

    fn events(input: &[u8], accept: usize) -> Vec<String> {
        events_and_end(input, accept).0
    }

    #[test]
    fn output_is_forwarded_then_the_exit_code() {
        assert_eq!(events(b"hello", 10), [r#"{"kind":"data","data":"hello"}"#, r#"{"kind":"exit","code":7}"#]);
        assert_eq!(events(b"", 10), [r#"{"kind":"exit","code":7}"#]);
        assert!(!events_and_end(b"hello", 10).1, "the shell ended on its own: waited for");
    }

    #[test]
    fn a_closed_view_stops_the_stream() {
        let big = vec![b'a'; 20000];
        let out = events(&big, 0);
        assert_eq!(out.len(), 2, "one data event refused, then the exit");
    }

    #[test]
    fn a_closed_view_ends_its_shell_instead_of_waiting_on_it() {
        // Regression: once the view refused output, the thread waited for a shell nobody would
        // end (its session already removed), leaking both for as long as the app ran.
        assert!(events_and_end(&vec![b'a'; 20000], 0).1);
    }

    #[test]
    fn unknown_sessions_are_ended_or_ignored() {
        assert_eq!(write("shell-lab-unknown", u32::MAX, b"x").unwrap_err().to_string(), "the shell has ended");
        assert!(resize("shell-lab-unknown", u32::MAX, 80, 24).is_ok());
        close("shell-lab-unknown", u32::MAX);
    }

    /// A writer that blocks until `release` is dropped, as a PTY whose shell doesn't read does.
    #[cfg(unix)]
    struct Stalled(std::sync::mpsc::Receiver<()>);

    #[cfg(unix)]
    impl Write for Stalled {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let _ = self.0.recv();
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// A session of window `window` whose input goes to `writer` (through its own thread), its
    /// shell a `cat` that never ends on its own.
    #[cfg(unix)]
    fn session_writing_to(window: &str, writer: Box<dyn Write + Send>) -> u32 {
        let pair = native_pty_system().openpty(pty_size(80, 24)).unwrap();
        let child = pair.slave.spawn_command(CommandBuilder::new("cat")).unwrap();
        drop(pair.slave);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let writer = Box::new(Input::spawn(writer));
        with_sessions(|s| s.insert(id, Session { writer, master: pair.master, child, window: window.into() }));
        id
    }

    #[cfg(unix)]
    #[test]
    fn a_shell_that_doesnt_read_blocks_no_other_session() {
        // Regression: a paste into a stalled shell (a wedged `docker exec`) blocked in the PTY
        // write, on the main thread and holding the sessions lock: every window froze.
        struct Sink(std::sync::mpsc::Sender<Vec<u8>>);
        impl Write for Sink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                let _ = self.0.send(buf.to_vec());
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (release, stalled) = std::sync::mpsc::channel();
        let (sink, written) = std::sync::mpsc::channel::<Vec<u8>>();
        let stuck = session_writing_to("shell-stalled", Box::new(Stalled(stalled)));
        let other = session_writing_to("shell-other", Box::new(Sink(sink)));
        let started = std::time::Instant::now();
        // More than a PTY buffer, in many writes, then more: none waits on the shell.
        for _ in 0..64 {
            write("shell-stalled", stuck, &[b'x'; 4096]).unwrap();
        }
        // A write that waited on the stalled shell would block forever (it never reads), so the
        // bound only has to tell "returned" from "hung" — generous, so a loaded runner descheduling
        // this thread for a moment can't false-fail it.
        assert!(started.elapsed() < std::time::Duration::from_secs(20), "writes to a stalled shell returned without waiting");
        // Another session's write goes through meanwhile (the sessions lock isn't held).
        write("shell-other", other, b"ls\n").unwrap();
        assert_eq!(written.recv_timeout(std::time::Duration::from_secs(5)).unwrap(), b"ls\n");
        // A shell that never reads has its input refused once its queue is full, not held.
        let refused = (0..=INPUT_QUEUE).find_map(|_| write("shell-stalled", stuck, b"y").err());
        assert_eq!(refused.unwrap().to_string(), "the shell isn't reading its input");
        drop(release);
        close("shell-stalled", stuck);
        close("shell-other", other);
        assert!(with_sessions(|s| !s.contains_key(&stuck) && !s.contains_key(&other)));
    }

    #[cfg(unix)]
    #[test]
    fn a_window_reaches_its_own_shells_only() {
        let (_release, stalled) = std::sync::mpsc::channel();
        let id = session_writing_to("shell-mine", Box::new(Stalled(stalled)));
        assert_eq!(write("shell-theirs", id, b"rm -rf ~\n").unwrap_err().to_string(), "the shell has ended");
        assert!(resize("shell-theirs", id, 100, 30).is_ok());
        close("shell-theirs", id);
        assert!(with_sessions(|s| s.contains_key(&id)), "another window can't end it");
        write("shell-mine", id, b"ok\n").unwrap();
        resize("shell-mine", id, 100, 30).unwrap();
        close("shell-mine", id);
        assert!(with_sessions(|s| !s.contains_key(&id)));
    }

    #[cfg(unix)]
    #[test]
    fn a_session_takes_input_resizes_and_ends_with_its_window() {
        let pair = native_pty_system().openpty(pty_size(80, 24)).unwrap();
        let mut cmd = CommandBuilder::new("sh");
        cmd.args(["-c", "cat"]);
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let writer = Box::new(Input::spawn(pair.master.take_writer().unwrap()));
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let label = format!("shell-test-{id}");
        with_sessions(|s| s.insert(id, Session { writer, master: pair.master, child, window: label.clone() }));
        write(&label, id, b"hi\n").unwrap();
        resize(&label, id, 100, 30).unwrap();
        close_window("some-other-window");
        assert!(with_sessions(|s| s.contains_key(&id)));
        close_window(&label);
        assert!(!with_sessions(|s| s.contains_key(&id)));
        assert!(write(&label, id, b"x").is_err());
        close(&label, id);
    }
}

#[cfg(test)]
mod proptests;
