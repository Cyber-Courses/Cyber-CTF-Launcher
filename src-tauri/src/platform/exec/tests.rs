use super::*;

#[tokio::test]
async fn a_run_given_up_on_reads_as_timed_out() {
    let err = within("docker compose exec", Duration::from_millis(10), std::future::pending::<Result<()>>()).await.unwrap_err();
    assert!(timed_out(&err), "{err}");
    assert!(err.to_string().starts_with("`docker compose exec` failed: timed out after"), "{err}");
    assert_eq!(within("x", Duration::from_secs(1), async { Ok(7) }).await.unwrap(), 7);
}

#[cfg(unix)]
#[tokio::test]
async fn timed_reads_capture_the_tools_output() {
    // Regression: the timed path spawned without piping stdout, so `wait_with_output`
    // returned "" for every status probe (docker ps, compose ps, vagrant status). Labs read
    // as not running and a successful launch looked like it reset. A timed read must return
    // what the tool printed, exactly like the untimed one.
    let timed = run_read("sh", &["-c", "printf hi"], None).await.unwrap();
    assert_eq!(timed, "hi");
    let untimed = run("sh", &["-c", "printf hi"], None).await.unwrap();
    assert_eq!(timed, untimed);
}

#[cfg(unix)]
#[tokio::test]
async fn timed_reads_surface_stderr_on_failure() {
    // A failing tool's message is piped too, so it reaches the error instead of vanishing.
    let err = run_read("sh", &["-c", "echo boom >&2; exit 3"], None).await.unwrap_err();
    assert!(matches!(err, Error::CommandFailed { ref stderr, .. } if stderr.contains("boom")), "{err:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn a_run_whose_tool_left_a_child_holding_its_output_returns_after_the_drain_cap() {
    // Regression: the plain `run`/`run_env` path read stdout to EOF with no bound, so a process the
    // tool left behind (a `compose down` leaving a plugin, an ssh ControlMaster) that held the pipe
    // open hung the call — and the lab lock a teardown holds — for good. It now returns within the
    // post-exit drain cap, with what the tool itself printed.
    let started = std::time::Instant::now();
    let out = run("sh", &["-c", "sleep 30 & printf done"], None).await.unwrap();
    assert_eq!(out, "done");
    assert!(started.elapsed() < Duration::from_secs(10), "returned after the drain cap, not after the leftover: {:?}", started.elapsed());
}

#[cfg(unix)]
#[tokio::test]
async fn a_timed_out_tool_is_an_error_naming_the_command() {
    let err = run_env_timed("sh", &["-c", "sleep 5"], None, &[], Duration::from_millis(200)).await.unwrap_err();
    assert!(
        matches!(err, Error::CommandFailed { ref command, ref stderr } if command == "sh -c sleep 5" && stderr.starts_with("timed out after 0s")),
        "{err:?}"
    );
    assert!(timed_out(&err));
    assert!(!timed_out(&failed("sh", &[], "exited 1".into())) && !timed_out(&Error::ToolMissing { tool: "sh" }));
}

/// A tool that starts a long child of its own and writes its pid to `file`, then waits: the
/// program and its arguments. The child outlives the tool unless the tool's whole tree is
/// ended with it, which is what the tree tests check.
fn parent_of_a_child(file: &std::path::Path) -> (&'static str, Vec<String>) {
    let path = file.display();
    if cfg!(windows) {
        let script = format!(
            "$p = Start-Process -FilePath ping -ArgumentList '-n','120','127.0.0.1' -PassThru -NoNewWindow; Set-Content -Path '{path}' -Value $p.Id; Start-Sleep 120"
        );
        ("powershell", ["-NoProfile", "-NonInteractive", "-Command", &script].map(String::from).to_vec())
    } else {
        ("sh", vec!["-c".to_string(), format!("sleep 120 & echo $! > '{path}'; wait")])
    }
}

/// The pid `parent_of_a_child` wrote once its child was up, waited for rather than read once:
/// a tool's first line may not have run yet on a loaded runner, and reading the file too early
/// (before the tool was cancelled) was the race that made these tests flaky under CI load.
/// Bounded (~15s), so a tool that never starts its child fails loudly instead of hanging.
async fn wait_for_child_pid(file: &std::path::Path) -> u32 {
    for _ in 0..300 {
        if let Some(pid) = std::fs::read_to_string(file).ok().and_then(|t| t.trim().parse::<u32>().ok()) {
            return pid;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the tool never started its child (no pid at {})", file.display());
}

/// Runs `tool` (a tool's future that starts the child whose pid lands in `file`) until that
/// child is up, then cancels it by dropping it and asserts its whole tree — the child included
/// — went with it. Waiting for the child first means it is always known-started before the tool
/// is ended: no read-before-write race, whatever the runner's load.
async fn assert_cancel_takes_the_tree(file: &std::path::Path, tool: impl std::future::Future) {
    let pid = {
        tokio::pin!(tool);
        tokio::select! {
            _ = &mut tool => panic!("the tool ended before it started its child"),
            pid = wait_for_child_pid(file) => pid,
        }
        // `tool` is dropped as this block ends: `kill_on_drop` and the armed `GroupGuard` end
        // its whole tree (its process group on unix, its job object on Windows).
    };
    let _ = std::fs::remove_file(file);
    assert!(fake::gone(pid).await, "the tool's child {pid} was left running");
}

#[tokio::test]
async fn a_timed_out_tool_takes_what_it_started_with_it() {
    // Regression (Windows): a timeout killed the tool alone, and what it had started (a
    // wrapper's real program) ran on. A tool's whole tree ends with it now (a job object on
    // Windows, its process group on unix). The timed path's tree-kill is its `GroupGuard`
    // dropped on give-up; the error of a timeout itself is `a_timed_out_tool_is_an_error...`.
    // The generous limit is a safety net only: the child is confirmed up and the tool is
    // cancelled long before it, so the test never waits on wall-clock and can't race.
    let file = std::env::temp_dir().join(format!("cyberctf-tree-{}", rand::random::<u32>()));
    let (program, args) = parent_of_a_child(&file);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let timed = run_env_timed(program, &args, None, &[], Duration::from_secs(120));
    assert_cancel_takes_the_tree(&file, timed).await;
}

/// Runs, the app's way (`run_env_timed`), a stand-in tool whose long child's pid lands in
/// `CYBERCTF_TREE_FILE`, and waits on it until it is killed.
fn run_a_parent_of_a_child() {
    let file = std::path::PathBuf::from(std::env::var("CYBERCTF_TREE_FILE").unwrap());
    let (program, args) = parent_of_a_child(&file);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let _ = runtime.block_on(run_env_timed(program, &args, None, &[], Duration::from_secs(300)));
}

/// The app's stand-in, run as a child process by the test below: it starts a tool the app's
/// way (`run_env_timed`, so in a job object of its own), a tool whose long child's pid lands
/// in `CYBERCTF_TREE_FILE`, and waits on it until it is killed.
#[cfg(windows)]
#[test]
#[ignore = "run as a child process by a_killed_apps_tools_end_with_it"]
fn tool_owner_helper() {
    run_a_parent_of_a_child();
}

/// What a test left running is ended however the test ends: the stand-in's tree, and the
/// pids it names, should the job have let them live on.
#[cfg(windows)]
struct Ends(std::process::Child, Vec<u32>);

#[cfg(windows)]
impl Drop for Ends {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        for pid in &self.1 {
            crate::platform::sys::terminate(*pid);
        }
    }
}

/// Regression: `docker` processes the app started outlived it, their parent gone. A tool's job ends
/// its tree when the job's last handle closes, which the app's own death does: the stand-in is
/// killed as Task Manager's End task or a crash would (`TerminateProcess`, no exit of its own), and
/// the tool and its child go with it.
#[cfg(windows)]
#[tokio::test]
async fn a_killed_apps_tools_end_with_it() {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let file = std::env::temp_dir().join(format!("cyberctf-owner-tree-{}", rand::random::<u32>()));
    let owner = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "platform::exec::tests::tool_owner_helper", "--ignored", "--nocapture", "--test-threads", "1"])
        .env("CYBERCTF_TREE_FILE", &file)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut ends = Ends(owner, Vec::new());
    let child = wait_for_child_pid(&file).await;
    let _ = std::fs::remove_file(&file);
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[Pid::from_u32(child)]), true, ProcessRefreshKind::nothing());
    let tool = sys.process(Pid::from_u32(child)).and_then(sysinfo::Process::parent).map(Pid::as_u32).expect("the tool's child has its parent");
    ends.1 = vec![tool, child];
    ends.0.kill().unwrap();
    ends.0.wait().unwrap();
    assert!(fake::gone(tool).await, "the tool {tool} outlived the app that started it");
    assert!(fake::gone(child).await, "the tool's child {child} outlived the app that started it");
}

/// The app's stand-in on unix, run as a child process by the test below: it records its tools
/// in the ledger `CYBERCTF_LEDGER` names, starts one the app's way (`run_env_timed`, so in a
/// process group of its own), a tool whose long child's pid lands in `CYBERCTF_TREE_FILE`,
/// and waits on it until it is killed.
#[cfg(unix)]
#[test]
#[ignore = "run as a child process by a_killed_apps_tools_end_at_the_next_start"]
fn ledger_owner_helper() {
    record_tools_in(std::path::PathBuf::from(std::env::var("CYBERCTF_LEDGER").unwrap()));
    run_a_parent_of_a_child();
}

/// Regression (#301): on unix, the tools of an app killed outright (SIGKILL, a crash, Force
/// Quit) ran on in their own process groups. They do until the next start, which ends them,
/// what they started with them, from the ledger the killed app kept.
#[cfg(unix)]
#[tokio::test]
async fn a_killed_apps_tools_end_at_the_next_start() {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let ledger = tempfile::tempdir().unwrap();
    let file = ledger.path().join("tree");
    let mut owner = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "platform::exec::tests::ledger_owner_helper", "--ignored", "--nocapture", "--test-threads", "1"])
        .env("CYBERCTF_LEDGER", ledger.path())
        .env("CYBERCTF_TREE_FILE", &file)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let child = wait_for_child_pid(&file).await;
    std::fs::remove_file(&file).unwrap();
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[Pid::from_u32(child)]), true, ProcessRefreshKind::nothing());
    let tool = sys.process(Pid::from_u32(child)).and_then(sysinfo::Process::parent).map(Pid::as_u32).expect("the tool's child has its parent");
    owner.kill().unwrap();
    owner.wait().unwrap();
    let alive = crate::platform::sys::process_exists(tool) && crate::platform::sys::process_exists(child);
    let ended = end_orphaned_tools(ledger.path()).await;
    assert!(alive, "the killed app's tool and its child ran on until the next start");
    assert_eq!(ended, [tool]);
    assert!(fake::gone(tool).await, "the tool {tool} outlived the next start");
    assert!(fake::gone(child).await, "the tool's child {child} outlived the next start");
    assert_eq!(std::fs::read_dir(ledger.path()).unwrap().count(), 0, "its record is gone");
}

/// The app's stand-in for the test below, its output piped to the test: it keeps its standard
/// handles as the app does at start, starts a long tool outside any job (as one started as
/// the app died is, #299), its pid to `CYBERCTF_TREE_FILE`, and exits without waiting on it.
#[cfg(windows)]
#[test]
#[ignore = "run as a child process by a_tool_left_running_doesnt_hold_the_apps_output"]
fn std_handles_owner_helper() {
    use std::os::windows::process::CommandExt;
    crate::platform::sys::keep_std_handles();
    let tool = std::process::Command::new("ping")
        .args(["-n", "60", "127.0.0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .unwrap();
    std::fs::write(std::env::var("CYBERCTF_TREE_FILE").unwrap(), tool.id().to_string()).unwrap();
    // Left running on purpose, never waited on: the test ends it.
    std::mem::forget(tool);
}

/// Regression: a `docker` the app left running held the pipes of what started it, and the E2E
/// harness waited on them until the docker was killed by hand. std hands a child every inheritable
/// handle of its parent; the app keeps its own standard handles to itself, so its output ends when
/// it does, whatever it left running.
#[cfg(windows)]
#[test]
fn a_tool_left_running_doesnt_hold_the_apps_output() {
    use std::io::Read;
    let file = std::env::temp_dir().join(format!("cyberctf-std-handles-{}", rand::random::<u32>()));
    let mut owner = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "platform::exec::tests::std_handles_owner_helper", "--ignored", "--nocapture", "--test-threads", "1"])
        .env("CYBERCTF_TREE_FILE", &file)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let (tx, ended) = std::sync::mpsc::channel();
    for mut pipe in [Box::new(owner.stdout.take().unwrap()) as Box<dyn Read + Send>, Box::new(owner.stderr.take().unwrap())] {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let _ = pipe.read_to_end(&mut Vec::new());
            let _ = tx.send(());
        });
    }
    let exited = owner.wait().unwrap();
    let tool = std::fs::read_to_string(&file).ok().and_then(|t| t.trim().parse::<u32>().ok());
    let _ = std::fs::remove_file(&file);
    let closed = (0..2).all(|_| ended.recv_timeout(Duration::from_secs(10)).is_ok());
    if let Some(pid) = tool {
        crate::platform::sys::terminate(pid);
    }
    assert!(exited.success() && tool.is_some(), "the stand-in failed ({exited}) or started no tool");
    assert!(closed, "the app's output stayed open after it exited: the tool it left running held it");
}

#[cfg(unix)]
#[tokio::test]
async fn output_that_isnt_utf8_is_read_lossily_and_the_tool_runs_on() {
    // Regression: the first byte that wasn't UTF-8 (VBoxManage in a French Windows' codepage)
    // failed the stream, which dropped the child and killed it: a `terraform apply` cut off
    // mid-run. The line comes through with a replacement character, and the tool finishes.
    let mut lines = Vec::new();
    let script = r"printf 'Cr\351ation\r\n'; printf 'err \377\n' >&2; sleep 0.2; printf 'done'";
    stream("sh", &["-c", script], None, &[], |l| lines.push(l)).await.expect("the tool ran to its end");
    // stdout and stderr are separate pipes: their lines interleave in no set order.
    let at = |l: &str| lines.iter().position(|x| x == l);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(at("err \u{fffd}").is_some(), "{lines:?}");
    assert!(at("Cr\u{fffd}ation") < at("done") && at("Cr\u{fffd}ation").is_some(), "{lines:?}");
}

#[test]
fn a_line_loses_its_ending_only() {
    let mut buf = b"a\r\r\n".to_vec();
    assert_eq!(take_line(&mut buf), "a\r");
    assert_eq!(buf, b"");
    assert_eq!(take_line(&mut b"last".to_vec()), "last");
    assert_eq!(take_line(&mut b"\n".to_vec()), "");
}

#[tokio::test]
async fn a_cancelled_stream_takes_what_it_started_with_it() {
    // A streamed `vagrant` is a wrapper around `ruby`: cancelled, the wrapper alone was
    // killed, and its ruby ran on holding Vagrant's lock. The tool's tree ends with it now.
    // The child is waited for before the stream is cancelled (dropped), so a loaded runner
    // can't read the pid file before the tool wrote it.
    let file = std::env::temp_dir().join(format!("cyberctf-stream-tree-{}", rand::random::<u32>()));
    let (program, args) = parent_of_a_child(&file);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    assert_cancel_takes_the_tree(&file, stream(program, &args, None, &[], |_| {})).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_tool_that_exited_is_not_waited_for_past_what_it_left_holding_its_output() {
    // Regression: the stream read until both pipes closed, and a process the tool left behind
    // (an ssh ControlMaster, a provider plugin) held them open: a finished `terraform destroy`
    // then hung the auto-stop sweep, and the lab's lock, for good.
    let started = std::time::Instant::now();
    let mut lines = Vec::new();
    let run = stream("sh", &["-c", "sleep 20 & printf 'done'"], None, &[], |l| lines.push(l));
    tokio::time::timeout(Duration::from_secs(10), run).await.expect("not held up by the leftover").unwrap();
    assert_eq!(lines, ["done"]);
    assert!(started.elapsed() < Duration::from_secs(10));
    // And a failure is still read as one, with its last line.
    let err = stream("sh", &["-c", "sleep 20 & echo gone >&2; exit 4"], None, &[], |_| {}).await.unwrap_err();
    assert!(matches!(err, Error::CommandFailed { ref stderr, .. } if stderr.starts_with("gone (exited with")), "{err:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn a_failed_stream_keeps_its_last_stderr_lines() {
    let err = stream("sh", &["-c", "for i in $(seq 1 12); do echo e$i >&2; done; exit 2"], None, &[], |_| {}).await.unwrap_err();
    let Error::CommandFailed { stderr, .. } = err else { panic!("{err:?}") };
    assert!(stderr.starts_with("e3\n") && stderr.contains("e12 (exited with"), "{stderr}");
}

#[test]
fn a_silent_failure_reports_its_exit_status() {
    assert_eq!(stream_failure(&[], "exit status: 1"), "exited with exit status: 1");
    assert_eq!(stream_failure(&[" ".into()], "exit status: 1"), "exited with exit status: 1");
    assert_eq!(stream_failure(&["no space left".into()], "exit status: 1"), "no space left (exited with exit status: 1)");
}

/// A tool that exits non-zero and prints nothing (a `docker` whose daemon just went away)
/// must still leave the player something to read: the command it ran, with its (empty)
/// stderr after it, in the same shape as a failure that explained itself.
#[cfg(unix)]
#[tokio::test]
async fn a_tool_failing_without_a_word_still_names_its_command() {
    let err = run_read("sh", &["-c", "exit 3"], None).await.unwrap_err();
    assert!(matches!(err, Error::CommandFailed { ref command, ref stderr } if command == "sh -c exit 3" && stderr.is_empty()), "{err:?}");
    assert_eq!(err.to_string(), "`sh -c exit 3` failed: ");
    assert_eq!(err.code(), crate::error::Code::CommandFailed { command: "sh -c exit 3".into(), stderr: String::new() });
    assert!(!timed_out(&err));
}

#[test]
fn a_missing_tool_is_named() {
    let err = spawn_error("vagrant")(std::io::Error::from(std::io::ErrorKind::NotFound));
    assert!(matches!(err, Error::ToolMissing { tool: "vagrant" }));
    assert!(matches!(spawn_error("vagrant")(std::io::Error::from(std::io::ErrorKind::PermissionDenied)), Error::Io(_)));
}

#[tokio::test]
async fn lines_are_read_lossily_whatever_the_read_boundaries() {
    // A two-byte buffer makes every multi-byte character span reads: `read_until` gathers the
    // whole line first, so a character is never cut at a chunk boundary. CRLF is stripped, a
    // lone CR inside a line is kept, a line that isn't UTF-8 is decoded whole (in the system's
    // code page on Windows, as its console tools print: 0xFF is `ÿ` in Windows-1252; replaced
    // elsewhere), and the last line needs no newline.
    let mut input = "a\r\nb\rc\n\u{20ac}\u{20ac}\n".as_bytes().to_vec();
    input.extend_from_slice(&[0xff, b'd']);
    let mut reader = BufReader::with_capacity(2, &input[..]);
    let mut buf = Vec::new();
    let mut lines = Vec::new();
    while reader.read_until(b'\n', &mut buf).await.unwrap() > 0 || !buf.is_empty() {
        lines.push(take_line(&mut buf));
    }
    let not_utf8 = if cfg!(windows) { crate::platform::tool_output::text::decode(&[0xff, b'd']) } else { "\u{fffd}d".to_owned() };
    assert_eq!(lines, ["a", "b\rc", "\u{20ac}\u{20ac}", not_utf8.as_str()]);
    assert_eq!(reader.read_until(b'\n', &mut buf).await.unwrap(), 0, "still at the end");
}

#[cfg(unix)]
#[tokio::test]
async fn a_streamed_tool_printing_invalid_utf8_is_forwarded_not_cut() {
    // A provisioning script echoing Latin-1 (0xE9 alone) used to end the stream with an
    // error and kill the tool; now the byte is replaced and the run goes on to its end.
    let mut lines = Vec::new();
    stream("sh", &["-c", "printf 'caf\\351 ok\\n'; printf 'done\\n'"], None, &[], |l| lines.push(l)).await.unwrap();
    assert_eq!(lines, ["caf\u{fffd} ok", "done"]);
}
