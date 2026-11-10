//! Following a worker from the app: its log line by line, the step it is at, and its verdict.

use std::time::Duration;

use super::files::verdict;
use super::process::{Spawned, alive};
use crate::error::{Error, Result};

/// How a failed run's last log line starts (the UI marks a run failed by it).
pub const FAILED_MARK: &str = "✗";

/// How often the log and status files are read while following a worker.
const POLL: Duration = Duration::from_millis(300);
/// Polls a vanished worker gets for its status file to land before it counts as lost.
const STATUS_GRACE: u8 = 4;
/// Longest step shown in the sidebar, in characters.
const STEP_MAX: usize = 72;

/// The step a worker's log is at: the last Vagrant action line (`==> dc01: Booting VM...`) as
/// `dc01: Booting VM...`, else the last non-empty line, kept short. Vagrant's own progress
/// chatter (download percentages, blank spacers) never counts as a step.
pub fn last_step(log: &str) -> Option<String> {
    let mut last_action: Option<String> = None;
    let mut last_line: Option<String> = None;
    for raw in log.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with('✓') || l.starts_with('✗') {
            continue;
        }
        if let Some(rest) = l.strip_prefix("==>") {
            let rest = rest.trim();
            // "dc01: Booting VM..." keeps the machine; "dc01: " alone (a spacer) does not count.
            if rest.split_once(':').is_some_and(|(_, text)| !text.trim().is_empty()) {
                last_action = Some(rest.to_string());
            }
            continue;
        }
        last_line = Some(l.to_string());
    }
    let step = last_action.or(last_line)?;
    Some(if step.chars().count() > STEP_MAX { format!("{}…", step.chars().take(STEP_MAX - 1).collect::<String>()) } else { step })
}

/// How much of a log's end [`step_of`] reads: a VM start's log runs to megabytes, and the
/// sidebar polls every worker's step every 2 s.
const STEP_TAIL: u64 = 64 * 1024;

/// [`last_step`] of the log file at `path`, from its last [`STEP_TAIL`] bytes only (bytes that
/// aren't UTF-8 replaced). `None` when it can't be read.
pub fn step_of(path: &std::path::Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let from = len.saturating_sub(STEP_TAIL);
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    file.take(STEP_TAIL).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    // Started mid-line: that line's start is cut off, so it's left out.
    let whole = if from > 0 { text.split_once('\n').map_or("", |(_, rest)| rest) } else { &text };
    last_step(whole)
}

/// What the worker wrote between reads: whole lines go out, a cut-off last line waits for the
/// rest. Bytes, not text: a read can also cut a character in two (`…`, `✓`), which decoded on
/// its own would come out as replacement characters.
#[derive(Default)]
struct LineBuffer {
    carry: Vec<u8>,
}

impl LineBuffer {
    fn push(&mut self, bytes: &[u8], mut line: impl FnMut(String)) {
        self.carry.extend_from_slice(bytes);
        while let Some(i) = self.carry.iter().position(|b| *b == b'\n') {
            let whole: Vec<u8> = self.carry.drain(..=i).collect();
            line(String::from_utf8_lossy(&whole[..i]).into_owned());
        }
    }

    fn rest(&mut self) -> Option<String> {
        (!self.carry.is_empty()).then(|| String::from_utf8_lossy(&std::mem::take(&mut self.carry)).into_owned())
    }
}

/// Follows a worker's log line by line into `log` until it finishes; its status file is the
/// result. A worker that vanishes without a status is an error (its log says what happened).
pub async fn tail(spawned: &Spawned, mut log: impl FnMut(String)) -> Result<()> {
    let f = &spawned.files;
    let mut offset = 0usize;
    let mut lines = LineBuffer::default();
    let mut grace = 0u8;
    // What it said last: why it stopped, when it ends without a verdict.
    let mut last: Option<String> = None;
    loop {
        // The status first, then the log: the worker writes its last lines before its status, so
        // a status seen here means the log read next holds every line. (Read the other way
        // round, lines written between the two reads were dropped.)
        let status = std::fs::read_to_string(&f.status);
        if let Ok(bytes) = std::fs::read(&f.log)
            && bytes.len() > offset
        {
            lines.push(&bytes[offset..], |line| {
                if !line.trim().is_empty() {
                    last = Some(line.clone());
                }
                // The worker's closing ✗ line restates its verdict, which the caller reports.
                if !line.starts_with(FAILED_MARK) {
                    log(line);
                }
            });
            offset = bytes.len();
        }
        if let Ok(status) = status {
            if let Some(rest) = lines.rest() {
                log(rest);
            }
            return verdict(&status);
        }
        if !alive(&spawned.worker) {
            // The status is written just before the worker exits: give it a moment to land.
            grace += 1;
            if grace > STATUS_GRACE {
                return Err(lost(spawned, last.as_deref()));
            }
        }
        tokio::time::sleep(POLL).await;
    }
}

/// The error of a worker that ended without a verdict: how its process ended and the last line
/// it wrote (a crash's message lands there, on its stderr), also in the app's log with where its
/// own log is. It said only "stopped without a result", and nothing in the app's log said more.
fn lost(spawned: &Spawned, last_line: Option<&str>) -> Error {
    let how = ended(spawned.exit.get());
    log::warn!("deploy worker {} ended without a verdict: it {how}; its output is in {}", spawned.worker.pid, spawned.files.log.display());
    let said = last_line.map(|l| format!(" Its last line: {}", l.trim())).unwrap_or_default();
    Error::Invalid(format!("the deploy worker stopped without a result: it {how}.{said}"))
}

/// How a worker process ended (`None`: not known, it wasn't reaped by this app).
fn ended(status: Option<&std::process::ExitStatus>) -> String {
    #[cfg(unix)]
    if let Some(signal) = status.and_then(std::os::unix::process::ExitStatusExt::signal) {
        return format!("was killed by signal {signal}");
    }
    status.and_then(std::process::ExitStatus::code).map_or_else(|| "ended, how isn't known".into(), exit_code)
}

/// `exited with code N`, with what the code means when it is one a crash or a kill leaves (the
/// Windows ones are NTSTATUS codes, shown in hex as Windows tools show them).
fn exit_code(code: i32) -> String {
    let raw = code.cast_unsigned();
    let meaning = match raw {
        101 => Some("a panic"),
        0xC000_0005 => Some("an access violation"),
        0xC000_00FD => Some("a stack overflow"),
        0xC000_0374 => Some("a heap corruption"),
        0xC000_0409 => Some("an abort"),
        0xC000_013A => Some("a Ctrl+C, or its console closed"),
        _ => None,
    };
    let shown = if raw >= 0x8000_0000 { format!("0x{raw:08X}") } else { code.to_string() };
    meaning.map_or_else(|| format!("exited with code {shown}"), |m| format!("exited with code {shown} ({m})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_step_prefers_vagrants_action_lines_and_skips_spacers() {
        let log = "Starting the lab…\n==> dc01: Importing base box 'x'...\n    dc01: 42%\n==> dc01: Booting VM...\n==> dc01: \n\n";
        assert_eq!(last_step(log).as_deref(), Some("dc01: Booting VM..."));
        // No Vagrant lines yet: the launcher's own last line.
        assert_eq!(last_step("Downloading CyberCTF/x@abc\nLab installed\n").as_deref(), Some("Lab installed"));
        assert_eq!(last_step("\n\n"), None);
        // Long lines are kept readable.
        let long_line = format!("==> ws01: {}", "a".repeat(200));
        let step = last_step(&long_line).unwrap();
        assert!(step.ends_with('…'));
        assert_eq!(step.chars().count(), STEP_MAX);
    }

    #[test]
    fn a_step_is_read_from_the_end_of_a_long_log() {
        let dir = std::env::temp_dir().join(format!("cyberctf-step-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("lab.log");
        assert_eq!(step_of(&log), None, "no log yet");
        // Megabytes of chatter before the step: only the end is read, and the line cut at the
        // start of that end doesn't count.
        let mut text = "==> dc01: Booting VM...\n".repeat(2);
        let tail = usize::try_from(STEP_TAIL).unwrap();
        text.push_str(&"x".repeat(3 * tail));
        text.push_str("\n==> dc01: Waiting for the VM to boot...\n  dc01: \n");
        std::fs::write(&log, &text).unwrap();
        assert_eq!(step_of(&log).as_deref(), Some("dc01: Waiting for the VM to boot..."));
        std::fs::write(&log, [&"y".repeat(2 * tail)[..], "\nPulling images\n"].concat()).unwrap();
        assert_eq!(step_of(&log).as_deref(), Some("Pulling images"));
        // A log that isn't all UTF-8 (a tool's codepage) still has a step.
        std::fs::write(&log, b"Cr\xe9ation du r\xe9seau\n").unwrap();
        assert_eq!(step_of(&log).as_deref(), Some("Cr\u{fffd}ation du r\u{fffd}seau"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_line_cut_between_reads_comes_out_whole() {
        let mut buf = LineBuffer::default();
        let mut out = Vec::new();
        buf.push(b"one\ntw", |l| out.push(l));
        buf.push(b"o\nthr", |l| out.push(l));
        assert_eq!(out, ["one", "two"]);
        assert_eq!(buf.rest().as_deref(), Some("thr"));
        assert_eq!(buf.rest(), None);
    }

    fn spawned(tag: &str) -> (std::path::PathBuf, Spawned) {
        let dir = crate::testing::temp_dir(tag);
        let files = crate::app::deploy_worker::files::Files::of(&dir, "lab-1");
        let worker = crate::app::deploy_worker::process::Worker { pid: 4_000_000_000, started: None };
        (dir, Spawned { files, worker, exit: crate::app::deploy_worker::process::Exit::default() })
    }

    #[tokio::test]
    async fn tail_follows_the_log_to_the_verdict() {
        let (dir, s) = spawned("tail-ok");
        std::fs::write(&s.files.log, "one\ntwo\n✗ failed\ncut").unwrap();
        std::fs::write(&s.files.status, "error: it broke").unwrap();
        let mut lines = Vec::new();
        let err = tail(&s, |l| lines.push(l)).await.unwrap_err();
        assert_eq!(err.to_string(), "it broke");
        assert_eq!(lines, ["one", "two", "cut"]);
        std::fs::write(&s.files.status, "ok").unwrap();
        assert!(tail(&s, |_| {}).await.is_ok());
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn lines_written_just_before_the_status_are_not_dropped() {
        // Regression: the log was read before the status, so when the worker wrote its last
        // lines and its status between the two reads, those lines never reached the app. The
        // worker finishing while the first line is passed on stands in for that moment.
        let (dir, s) = spawned("tail-race");
        std::fs::write(&s.files.log, "one\n").unwrap();
        let mut lines = Vec::new();
        let files = crate::app::deploy_worker::files::Files::of(&dir, "lab-1");
        tail(&s, |l| {
            if l == "one" {
                use std::io::Write as _;
                let mut log = std::fs::OpenOptions::new().append(true).open(&files.log).unwrap();
                writeln!(log, "Lab is ready at http://localhost:8080").unwrap();
                std::fs::write(&files.status, "ok").unwrap();
            }
            lines.push(l);
        })
        .await
        .unwrap();
        assert_eq!(lines, ["one", "Lab is ready at http://localhost:8080"]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn a_worker_gone_without_a_status_is_an_error() {
        let (dir, s) = spawned("tail-lost");
        let err = tail(&s, |_| {}).await.unwrap_err();
        assert_eq!(err.to_string(), "the deploy worker stopped without a result: it ended, how isn't known.");
        std::fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_gone_without_a_status_says_how_it_ended_and_its_last_line() {
        // Regression (karen, Windows): a worker that died mid-download left the app saying only
        // "stopped without a result (see its log)"; the next start then emptied that log.
        use std::os::unix::process::ExitStatusExt;
        let (dir, s) = spawned("tail-crashed");
        std::fs::write(&s.files.log, "Downloading CyberCTF/web@abc\n\nthread 'tokio-runtime-worker' has overflowed its stack\n\n").unwrap();
        s.exit.set(std::process::ExitStatus::from_raw(9)).unwrap();
        let err = tail(&s, |_| {}).await.unwrap_err().to_string();
        assert_eq!(
            err,
            "the deploy worker stopped without a result: it was killed by signal 9. Its last line: thread 'tokio-runtime-worker' has overflowed its stack"
        );
        let (dir2, s) = spawned("tail-exited");
        s.exit.set(std::process::ExitStatus::from_raw(101 << 8)).unwrap();
        let err = tail(&s, |_| {}).await.unwrap_err().to_string();
        assert_eq!(err, "the deploy worker stopped without a result: it exited with code 101 (a panic).");
        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(dir2).ok();
    }

    #[test]
    fn exit_codes_a_crash_leaves_say_what_they_mean() {
        assert_eq!(exit_code(2), "exited with code 2");
        assert_eq!(exit_code(1), "exited with code 1");
        assert_eq!(exit_code(0xC000_00FD_u32.cast_signed()), "exited with code 0xC00000FD (a stack overflow)");
        assert_eq!(exit_code(0xC000_0409_u32.cast_signed()), "exited with code 0xC0000409 (an abort)");
        assert_eq!(exit_code(0xC000_013A_u32.cast_signed()), "exited with code 0xC000013A (a Ctrl+C, or its console closed)");
        assert_eq!(exit_code(0xE000_0001_u32.cast_signed()), "exited with code 0xE0000001");
        assert_eq!(ended(None), "ended, how isn't known");
    }
}

#[cfg(test)]
mod proptests;
