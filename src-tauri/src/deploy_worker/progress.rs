//! Following a worker from the app: its log line by line, the step it is at, and its verdict.

use std::time::Duration;

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

/// The verdict in a status file: `Ok` for `ok`, else the error the worker wrote, with its code
/// (`error: {"code":…,"message":…}`), or a bare message (a worker of an older version).
pub fn verdict(status: &str) -> Result<()> {
    let s = status.trim();
    if s == "ok" {
        return Ok(());
    }
    let body = s.strip_prefix("error: ").unwrap_or(s);
    Err(serde_json::from_str::<crate::error::Wire>(body).map_or_else(|_| Error::Invalid(body.to_string()), Error::from))
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
    loop {
        // The status first, then the log: the worker writes its last lines before its status, so
        // a status seen here means the log read next holds every line. (Read the other way
        // round, lines written between the two reads were dropped.)
        let status = std::fs::read_to_string(&f.status);
        if let Ok(bytes) = std::fs::read(&f.log)
            && bytes.len() > offset
        {
            lines.push(&bytes[offset..], |line| {
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
                return Err(Error::Invalid("the deploy worker stopped without a result (see its log)".into()));
            }
        }
        tokio::time::sleep(POLL).await;
    }
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
        let long = format!("==> ws01: {}", "a".repeat(200));
        let step = last_step(&long).unwrap();
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
        text.push_str(&"x".repeat(3 * STEP_TAIL as usize));
        text.push_str("\n==> dc01: Waiting for the VM to boot...\n  dc01: \n");
        std::fs::write(&log, &text).unwrap();
        assert_eq!(step_of(&log).as_deref(), Some("dc01: Waiting for the VM to boot..."));
        std::fs::write(&log, [&"y".repeat(2 * STEP_TAIL as usize)[..], "\nPulling images\n"].concat()).unwrap();
        assert_eq!(step_of(&log).as_deref(), Some("Pulling images"));
        // A log that isn't all UTF-8 (a tool's codepage) still has a step.
        std::fs::write(&log, b"Cr\xe9ation du r\xe9seau\n").unwrap();
        assert_eq!(step_of(&log).as_deref(), Some("Cr\u{fffd}ation du r\u{fffd}seau"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn verdict_reads_ok_and_errors() {
        assert!(verdict("ok\n").is_ok());
        let err = verdict("error: Docker isn't running").unwrap_err();
        assert!(format!("{err:?}").contains("Docker isn't running"));
        // The worker's error keeps its code across the process boundary.
        let err = verdict(r#"error: {"code":"portInUse","args":{"port":8080},"message":"Host port 8080 is taken"}"#).unwrap_err();
        assert_eq!(err.code(), crate::error::Code::PortInUse { port: 8080 });
        assert_eq!(err.to_string(), "Host port 8080 is taken");
        // A bare message is still an error, kept whole.
        assert!(verdict("something else").is_err());
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
        let dir = crate::deploy_worker::files::tests::temp_dir(tag);
        let files = crate::deploy_worker::files::Files::of(&dir, "lab-1");
        (dir, Spawned { files, worker: crate::deploy_worker::process::Worker { pid: 4_000_000_000, started: None } })
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
        let files = crate::deploy_worker::files::Files::of(&dir, "lab-1");
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
        assert!(err.to_string().contains("without a result"));
        std::fs::remove_dir_all(dir).ok();
    }
}

#[cfg(test)]
mod proptests;
