use super::*;

/// A shell whose kill fails (`EPERM`, say).
#[derive(Debug)]
struct Unkillable;

impl portable_pty::ChildKiller for Unkillable {
    fn kill(&mut self) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
    }
    fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
        Box::new(Unkillable)
    }
}

impl Child for Unkillable {
    fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
        Ok(None)
    }
    fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
        Ok(portable_pty::ExitStatus::with_exit_code(0))
    }
    fn process_id(&self) -> Option<u32> {
        Some(4242)
    }
    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }
}

/// Regression: a shell that couldn't be killed was dropped silently, its `docker exec` or ssh
/// left running.
#[test]
fn a_shell_that_cant_be_killed_is_logged() {
    crate::testing::logs::take_warnings();
    kill_shell(&mut Unkillable);
    let warned = crate::testing::logs::take_warnings();
    assert!(warned.len() == 1 && warned[0].starts_with("couldn't end an attack-box shell (Some(4242)): "), "{warned:?}");
}

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
    let terms = Terminals::default();
    let session = terms.next_id();
    terms.with_sessions(|s| s.insert(session, Session { writer, master: pair.master, child, window: "shell-test".into() }));
    let (tx, rx) = std::sync::mpsc::channel();
    terms.follow(session, reader, move |e| tx.send(e).is_ok());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut out = String::new();
    let code = loop {
        match rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
            Ok(TermEvent::Data { data }) => {
                // ConPTY asks where the cursor is (DSR) and holds all output until answered;
                // in the app xterm.js answers it.
                if data.contains("\x1b[6n") {
                    terms.write("shell-test", session, b"\x1b[1;1R").unwrap();
                }
                out.push_str(&data);
            }
            Ok(TermEvent::Exit { code }) => break code,
            Err(e) => panic!("no Exit within 15 s ({e}); output so far: {out:?}"),
        }
    };
    assert!(out.contains("hi"), "{out:?}");
    assert_eq!(code, Some(3));
    assert!(terms.with_sessions(|s| !s.contains_key(&session)), "the ended session is closed");
}

fn argv(c: &CommandBuilder) -> Vec<String> {
    c.get_argv().iter().map(|a| a.to_string_lossy().into_owned()).collect()
}

/// `c`'s argv with its program as the name it was asked for: on Windows the PTY runs the file
/// PATH and PATHEXT find for a bare name (`C:\Windows\system32\docker.exe` for `docker`, see
/// [`windows_program`]), on other platforms the name itself. Compared by file stem, ignoring
/// case as Windows does.
fn argv_by_name(c: &CommandBuilder) -> Vec<String> {
    let mut argv = argv(c);
    if let Some(program) = argv.first_mut() {
        let stem = std::path::Path::new(program.as_str()).file_stem().map(|s| s.to_string_lossy().to_lowercase());
        *program = stem.unwrap_or_default();
    }
    argv
}

#[test]
fn the_pty_runs_the_runners_command_in_its_folder() {
    // The runner builds the command (program + args + the run's folder); the PTY runs it as
    // is, no shell wrapping, on every platform.
    let dir = std::env::temp_dir();
    let env = vec![("VAGRANT_DOTFILE_PATH".to_owned(), "/runs/x/vagrant".to_owned())];
    let c = pty_command(&ShellCommand::new("vagrant", ["ssh", "isoloom-controller"], dir.clone()).with_env(env));
    assert_eq!(argv_by_name(&c), ["vagrant", "ssh", "isoloom-controller"]);
    assert_eq!(c.get_cwd().map(std::path::PathBuf::from), Some(dir.clone()));
    assert_eq!(c.get_env("VAGRANT_DOTFILE_PATH").map(|v| v.to_string_lossy().into_owned()).as_deref(), Some("/runs/x/vagrant"), "Vagrant finds the VM");
    let d = pty_command(&ShellCommand::new("docker", ["compose", "exec", "kali", "bash"], dir.clone()));
    assert_eq!(argv_by_name(&d), ["docker", "compose", "exec", "kali", "bash"]);
}

#[test]
fn a_bare_program_runs_as_windows_finds_it_through_pathext_not_an_extensionless_file() {
    // Regression (Windows): Docker Desktop's bin folder holds `docker` (a script) and
    // `docker.exe`; the shell was started on the script and Windows refused it.
    let dir = crate::testing::temp_dir("pathext");
    std::fs::write(dir.join("docker"), "#!/bin/sh").unwrap();
    std::fs::write(dir.join("docker.exe"), "MZ").unwrap();
    std::fs::write(dir.join("az.cmd"), "@echo off").unwrap();
    let dirs = [dir.join("missing"), dir.to_path_buf()];
    let find = |p: &str| windows_program(p, &dirs, DEFAULT_PATHEXT, std::path::Path::is_file);
    assert_eq!(find("docker"), dir.join("docker.exe"));
    assert_eq!(find("az"), dir.join("az.cmd"), "a .cmd wrapper, as PATHEXT lists it");
    assert_eq!(find("vagrant"), std::path::PathBuf::from("vagrant"), "found nowhere: kept, for the spawn to say so");
    assert_eq!(find("docker.exe"), std::path::PathBuf::from("docker.exe"), "an extension of its own is kept");
    let full = dir.join("docker").display().to_string();
    assert_eq!(find(&full), dir.join("docker"), "a path is kept");
    std::fs::remove_dir_all(dir).ok();
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
    let terms = Terminals::default();
    assert_eq!(terms.write("shell-lab-unknown", u32::MAX, b"x").unwrap_err().to_string(), "the shell has ended");
    assert!(terms.resize("shell-lab-unknown", u32::MAX, 80, 24).is_ok());
    terms.close("shell-lab-unknown", u32::MAX);
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
fn session_writing_to(terms: &Terminals, window: &str, writer: Box<dyn Write + Send>) -> u32 {
    let pair = native_pty_system().openpty(pty_size(80, 24)).unwrap();
    let child = pair.slave.spawn_command(CommandBuilder::new("cat")).unwrap();
    drop(pair.slave);
    let id = terms.next_id();
    let writer = Box::new(Input::spawn(writer));
    terms.with_sessions(|s| s.insert(id, Session { writer, master: pair.master, child, window: window.into() }));
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
    let terms = Terminals::default();
    let (release, stalled) = std::sync::mpsc::channel();
    let (sink, written) = std::sync::mpsc::channel::<Vec<u8>>();
    let stuck = session_writing_to(&terms, "shell-stalled", Box::new(Stalled(stalled)));
    let other = session_writing_to(&terms, "shell-other", Box::new(Sink(sink)));
    let started = std::time::Instant::now();
    // More than a PTY buffer, in many writes, then more: none waits on the shell.
    for _ in 0..64 {
        terms.write("shell-stalled", stuck, &[b'x'; 4096]).unwrap();
    }
    // A write that waited on the stalled shell would block forever (it never reads), so the
    // bound only has to tell "returned" from "hung" — generous, so a loaded runner descheduling
    // this thread for a moment can't false-fail it.
    assert!(started.elapsed() < std::time::Duration::from_secs(20), "writes to a stalled shell returned without waiting");
    // Another session's write goes through meanwhile (the sessions lock isn't held).
    terms.write("shell-other", other, b"ls\n").unwrap();
    assert_eq!(written.recv_timeout(std::time::Duration::from_secs(5)).unwrap(), b"ls\n");
    // A shell that never reads has its input refused once its queue is full, not held.
    let refused = (0..=INPUT_QUEUE).find_map(|_| terms.write("shell-stalled", stuck, b"y").err());
    assert_eq!(refused.unwrap().to_string(), "the shell isn't reading its input");
    drop(release);
    terms.close("shell-stalled", stuck);
    terms.close("shell-other", other);
    assert!(terms.with_sessions(|s| !s.contains_key(&stuck) && !s.contains_key(&other)));
}

#[cfg(unix)]
#[test]
fn a_window_reaches_its_own_shells_only() {
    let terms = Terminals::default();
    let (_release, stalled) = std::sync::mpsc::channel();
    let id = session_writing_to(&terms, "shell-mine", Box::new(Stalled(stalled)));
    assert_eq!(terms.write("shell-theirs", id, b"rm -rf ~\n").unwrap_err().to_string(), "the shell has ended");
    assert!(terms.resize("shell-theirs", id, 100, 30).is_ok());
    terms.close("shell-theirs", id);
    assert!(terms.with_sessions(|s| s.contains_key(&id)), "another window can't end it");
    terms.write("shell-mine", id, b"ok\n").unwrap();
    terms.resize("shell-mine", id, 100, 30).unwrap();
    terms.close("shell-mine", id);
    assert!(terms.with_sessions(|s| !s.contains_key(&id)));
}

#[cfg(unix)]
#[test]
fn a_session_takes_input_resizes_and_ends_with_its_window() {
    let terms = Terminals::default();
    let pair = native_pty_system().openpty(pty_size(80, 24)).unwrap();
    let mut cmd = CommandBuilder::new("sh");
    cmd.args(["-c", "cat"]);
    let child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let writer = Box::new(Input::spawn(pair.master.take_writer().unwrap()));
    let id = terms.next_id();
    let label = format!("shell-test-{id}");
    terms.with_sessions(|s| s.insert(id, Session { writer, master: pair.master, child, window: label.clone() }));
    terms.write(&label, id, b"hi\n").unwrap();
    terms.resize(&label, id, 100, 30).unwrap();
    terms.close_window("some-other-window");
    assert!(terms.with_sessions(|s| s.contains_key(&id)));
    terms.close_window(&label);
    assert!(!terms.with_sessions(|s| s.contains_key(&id)));
    assert!(terms.write(&label, id, b"x").is_err());
    terms.close(&label, id);
}

#[test]
fn a_shell_that_ends_but_cant_be_reaped_is_logged() {
    // Regression: its reap error was dropped, as `end`'s was before.
    crate::testing::logs::take_warnings();
    assert_eq!(super::reaped(Some(42), Err(std::io::Error::other("no child process"))), None);
    let warned = crate::testing::logs::take_warnings();
    assert!(warned.iter().any(|w| w == "couldn't reap an attack-box shell (Some(42)): no child process"), "{warned:?}");
    assert_eq!(super::reaped(Some(42), Ok(portable_pty::ExitStatus::with_exit_code(3))), Some(3));
}
