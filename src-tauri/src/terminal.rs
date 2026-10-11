//! The attack box shell inside the app: a pseudo-terminal running the same command line the
//! system terminal would (`docker exec -it …`, `ssh -t …`, `vagrant ssh`), streamed to an
//! xterm.js view. No Terminal.app window with a title we can't control, and no macOS
//! Automation prompt for driving Terminal.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Mutex;

use crate::AppHandle;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use tauri::ipc::Channel;

use crate::error::{Error, Result};
use crate::runtime::{Runtime, ShellKind, attack_vm_shell_target, end_shell_session, end_shell_session_blocking, shell_command_for};

struct Session {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    /// The window showing it: closing that window ends it (the page's own cleanup never runs
    /// when its webview is destroyed).
    window: String,
    /// The attack box container and the mark on this shell's processes in it: killing
    /// `docker exec` here leaves them running there.
    remote: Option<(String, String)>,
}

/// Kills a session's shell and reaps it, off the calling thread (sync commands run on the main
/// thread): a killed child that is never waited on stays a zombie while the app runs.
fn end(mut sess: Session) {
    let _ = sess.child.kill();
    if let Some((container, tag)) = sess.remote.take() {
        tauri::async_runtime::spawn(async move { end_shell_session(&container, &tag).await });
    }
    std::thread::spawn(move || {
        let _ = sess.child.wait();
    });
}

static SESSIONS: Mutex<Option<HashMap<u32, Session>>> = Mutex::new(None);
static NEXT_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

fn with_sessions<T>(f: impl FnOnce(&mut HashMap<u32, Session>) -> T) -> T {
    let mut guard = SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TermEvent {
    /// Output, as text (split multi-byte characters are held back until complete).
    Data { data: String },
    /// The shell ended.
    Exit { code: Option<u32> },
}

/// Splits `buf` into the text that is complete and the bytes of a character cut off at its end.
fn take_text(buf: &mut Vec<u8>) -> String {
    match std::str::from_utf8(buf) {
        Ok(s) => {
            let out = s.to_string();
            buf.clear();
            out
        }
        Err(e) if e.error_len().is_none() => {
            // An incomplete character at the end: keep it for the next read.
            let valid = e.valid_up_to();
            let out = String::from_utf8_lossy(&buf[..valid]).into_owned();
            buf.drain(..valid);
            out
        }
        Err(_) => {
            let out = String::from_utf8_lossy(buf).into_owned();
            buf.clear();
            out
        }
    }
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize { rows: rows.max(2), cols: cols.max(10), pixel_width: 0, pixel_height: 0 }
}

/// Starts the shell for a lab in a pseudo-terminal; output goes to `events`. Returns the session.
#[tauri::command]
pub async fn terminal_open(
    window: crate::WebviewWindow,
    id: String,
    kind: ShellKind,
    runtime: Runtime,
    cols: u16,
    rows: u16,
    events: Channel<TermEvent>,
) -> Result<u32> {
    use tauri::Manager;
    let session = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // Unique across app runs too, so a later run never ends an earlier run's shell.
    let tag = format!("{}-{session}", std::process::id());
    let (line, container) = shell_command_for(window.app_handle(), &id, kind, runtime, &tag).await?;
    let remote = container.map(|c| (c, tag));
    // Windows has no `sh` for the attack VM's `cd … && vagrant ssh`: vagrant runs directly there,
    // in the attack VM's folder.
    let direct = if cfg!(windows) && matches!(kind, ShellKind::AttackVm) { Some(attack_vm_shell_target(window.app_handle(), &id)?) } else { None };
    let pair = native_pty_system().openpty(pty_size(cols, rows)).map_err(|e| Error::Invalid(format!("couldn't open a terminal: {e}")))?;
    let mut cmd = pty_command(&line, direct.as_ref().map(|(d, m)| (d.as_path(), *m)));
    cmd.env("TERM", "xterm-256color");
    // Docker's "What's next" ad on exit.
    cmd.env("DOCKER_CLI_HINTS", "false");
    let child = pair.slave.spawn_command(cmd).map_err(|e| Error::Invalid(format!("couldn't start the shell: {e}")))?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| Error::Invalid(e.to_string()))?;
    let writer = pair.master.take_writer().map_err(|e| Error::Invalid(e.to_string()))?;
    let window = window.label().to_string();
    with_sessions(|s| s.insert(session, Session { writer, master: pair.master, child, window, remote }));

    std::thread::spawn(move || {
        forward(
            &mut reader,
            |e| events.send(e).is_ok(),
            |view_gone| {
                let mut sess = with_sessions(|s| s.remove(&session))?;
                if view_gone {
                    // Nobody reads it any more, and the shell may never end on its own: waiting
                    // for it would leak it (and this thread) for as long as the app runs.
                    end(sess);
                    return None;
                }
                sess.child.wait().ok().map(|st| st.exit_code())
            },
        );
    });
    Ok(session)
}

/// What the pseudo-terminal runs: `vagrant ssh [machine]` in a folder (`direct`), or `line`
/// through the platform's shell.
fn pty_command(line: &str, direct: Option<(&std::path::Path, Option<&str>)>) -> CommandBuilder {
    if let Some((dir, machine)) = direct {
        let mut c = CommandBuilder::new("vagrant");
        c.arg("ssh");
        if let Some(m) = machine {
            c.arg(m);
        }
        c.cwd(dir);
        return c;
    }
    #[cfg(unix)]
    let c = {
        let mut c = CommandBuilder::new("sh");
        c.args(["-c", &format!("exec {line}")]);
        c
    };
    #[cfg(windows)]
    let c = {
        let mut c = CommandBuilder::new("cmd");
        c.args(["/c", &line.replace('\'', "\"")]);
        c
    };
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

/// Sends keystrokes (or pasted text) to the shell.
#[tauri::command]
pub fn terminal_write(session: u32, data: String) -> Result<()> {
    with_sessions(|s| match s.get_mut(&session) {
        Some(sess) => sess.writer.write_all(data.as_bytes()).and_then(|_| sess.writer.flush()).map_err(Error::Io),
        None => Err(Error::Invalid("the shell has ended".into())),
    })
}

/// Follows the view's size, so full-screen tools (vim, less, htop) lay out right.
#[tauri::command]
pub fn terminal_resize(session: u32, cols: u16, rows: u16) -> Result<()> {
    with_sessions(|s| match s.get(&session) {
        Some(sess) => sess.master.resize(pty_size(cols, rows)).map_err(|e| Error::Invalid(e.to_string())),
        None => Ok(()),
    })
}

/// Ends the shell (the view was closed).
#[tauri::command]
pub fn terminal_close(session: u32) -> Result<()> {
    if let Some(sess) = with_sessions(|s| s.remove(&session)) {
        end(sess);
    }
    Ok(())
}

/// Ends the shells a window was showing, when it is closed.
fn close_window(label: &str) {
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
        if let Some((container, tag)) = sess.remote.take() {
            end_shell_session_blocking(&container, &tag);
        }
    }
}

/// A page-supplied title, safe for a window's title bar: no control characters, kept short.
fn window_title(title: &str) -> String {
    title.chars().filter(|c| !c.is_control()).take(80).collect()
}

/// A lab shell window's label and page.
fn shell_window(id: &str, kind: ShellKind, runtime: Runtime) -> (String, String) {
    let k = match kind {
        ShellKind::Lab => "lab",
        ShellKind::AttackVm => "attackVm",
    };
    let rt = match runtime {
        Runtime::Docker => "DOCKER",
        Runtime::Vm => "VM",
    };
    (format!("shell-{k}-{id}"), format!("shell?id={id}&kind={k}&runtime={rt}"))
}

/// Opens (or focuses) a lab's shell window.
#[tauri::command]
pub fn terminal_window(app: AppHandle, id: String, kind: ShellKind, runtime: Runtime, title: String) -> Result<()> {
    crate::runtime::validate_id(&id)?;
    let (label, page) = shell_window(&id, kind, runtime);
    if crate::window::focus_existing(&app, &label) {
        return Ok(());
    }
    let window = crate::window::builder(&app, &label, page)
        .title(format!("{} · attack box", window_title(&title)))
        .inner_size(900.0, 560.0)
        .min_inner_size(480.0, 280.0)
        .resizable(true)
        .build()
        .map_err(|e| Error::Invalid(format!("could not open the shell window: {e}")))?;
    window.on_window_event(move |e| {
        if matches!(e, tauri::WindowEvent::Destroyed) {
            close_window(&label);
        }
    });
    Ok(())
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
        assert!(buf.is_empty());
        let mut bad = vec![b'x', 0xff, b'y'];
        assert_eq!(take_text(&mut bad), "x\u{fffd}y");
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

    fn argv(c: &CommandBuilder) -> Vec<String> {
        c.get_argv().iter().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn the_attack_vm_runs_vagrant_ssh_directly_in_its_folder() {
        // Windows (regression): no `sh` for `cd … && exec vagrant ssh`; vagrant runs in the folder.
        let dir = std::env::temp_dir();
        let c = pty_command("ignored", Some((dir.as_path(), Some("isoloom-controller"))));
        assert_eq!(argv(&c), ["vagrant", "ssh", "isoloom-controller"]);
        assert_eq!(c.get_cwd().map(std::path::PathBuf::from), Some(dir.clone()));
        assert_eq!(argv(&pty_command("ignored", Some((dir.as_path(), None)))), ["vagrant", "ssh"]);
        // Otherwise the line goes through the platform's shell, as before.
        let line = pty_command("docker exec -it 'x' bash", None);
        #[cfg(unix)]
        assert_eq!(argv(&line), ["sh", "-c", "exec docker exec -it 'x' bash"]);
        #[cfg(windows)]
        assert_eq!(argv(&line), ["cmd", "/c", "docker exec -it \"x\" bash"]);
    }

    #[test]
    fn shell_windows_are_named_by_kind_and_lab() {
        assert_eq!(shell_window("sqli", ShellKind::Lab, Runtime::Docker), ("shell-lab-sqli".into(), "shell?id=sqli&kind=lab&runtime=DOCKER".into()));
        assert_eq!(shell_window("goad", ShellKind::AttackVm, Runtime::Vm), ("shell-attackVm-goad".into(), "shell?id=goad&kind=attackVm&runtime=VM".into()));
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
        assert!(events_and_end(&[b'a'; 20000], 0).1);
    }

    #[test]
    fn unknown_sessions_are_ended_or_ignored() {
        assert_eq!(terminal_write(u32::MAX, "x".into()).unwrap_err().to_string(), "the shell has ended");
        assert!(terminal_resize(u32::MAX, 80, 24).is_ok());
        assert!(terminal_close(u32::MAX).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn a_session_takes_input_resizes_and_ends_with_its_window() {
        let pair = native_pty_system().openpty(pty_size(80, 24)).unwrap();
        let mut cmd = CommandBuilder::new("sh");
        cmd.args(["-c", "cat"]);
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let writer = pair.master.take_writer().unwrap();
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let window = format!("test-window-{id}");
        with_sessions(|s| s.insert(id, Session { writer, master: pair.master, child, window: window.clone(), remote: None }));
        terminal_write(id, "hi\n".into()).unwrap();
        terminal_resize(id, 100, 30).unwrap();
        close_window("some-other-window");
        assert!(with_sessions(|s| s.contains_key(&id)));
        close_window(&window);
        assert!(!with_sessions(|s| s.contains_key(&id)));
        assert!(terminal_write(id, "x".into()).is_err());
    }
}
