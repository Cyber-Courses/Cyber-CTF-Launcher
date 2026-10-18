//! A batch's log, turned back into the events and outcomes a step-by-step run reports, and the
//! batch's own folder.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::platform::exec::vagrant::{Event, Outcome, Step};

/// A question Vagrant waits for an answer to (on Windows its `ask` reads the console even without
/// a terminal, so it would wait forever), told by a line printed just before it, and what to
/// say instead.
const PROMPTS: &[(&str, &str)] = &[
    (
        "Please choose a switch to attach to your Hyper-V instance",
        "Vagrant stopped to ask which Hyper-V virtual switch to use (this machine has several), which the launcher can't answer. \
Set ISOLOOM_HYPERV_SWITCH to the name of one (Get-VMSwitch lists them), or keep a single virtual switch in Hyper-V Manager, then try again.",
    ),
    (
        "You will be asked for the username and password to use for the SMB",
        "Vagrant stopped to ask for a Windows username and password, to share a folder with a Hyper-V VM over SMB, which the launcher can't answer. \
Turn the shared folder off for this lab, then try again.",
    ),
];

/// What to say instead of a question Vagrant asks on `line`, if it is one.
fn prompt(line: &str) -> Option<&'static str> {
    PROMPTS.iter().find(|(needle, _)| line.contains(needle)).map(|(_, say)| *say)
}

/// Why the watcher ended a batch, from its `reason` file, as a sentence.
fn stopped(reason: &str) -> String {
    match reason.trim() {
        "timeout" => format!("Vagrant was stopped: it ran for more than {} hours.", super::script::DEADLINE.as_secs() / 3600),
        "gone" => "Vagrant was stopped: the launcher that started it exited.".into(),
        "" | "cancelled" => "Vagrant was stopped: the operation was cancelled.".into(),
        other => other.to_string(),
    }
}

/// Lines kept for a failed step's error, as for a plain stream.
const ERROR_LINES: usize = crate::platform::exec::STREAM_ERROR_LINES;

/// Turns a batch's log back into its events and outcomes.
struct Collector<'s> {
    marker: String,
    steps: &'s [Step],
    current: usize,
    text: String,
    tail: std::collections::VecDeque<String>,
    missing: bool,
    outcomes: Vec<Outcome>,
    skipped: Vec<bool>,
    error: Option<String>,
}

impl<'s> Collector<'s> {
    fn new(marker: &str, steps: &'s [Step]) -> Collector<'s> {
        Collector {
            marker: format!("{marker} "),
            steps,
            current: 0,
            text: String::new(),
            tail: std::collections::VecDeque::new(),
            missing: false,
            outcomes: steps.iter().map(|_| None).collect(),
            skipped: vec![false; steps.len()],
            error: None,
        }
    }

    fn line(&mut self, line: &str, on_event: &mut impl FnMut(Event)) {
        let Some(rest) = line.strip_prefix(&self.marker) else {
            self.text.push_str(line);
            self.text.push('\n');
            if self.tail.len() == ERROR_LINES {
                self.tail.pop_front();
            }
            self.tail.push_back(line.to_string());
            on_event(Event::Line(self.current, line.to_string()));
            return;
        };
        let mut words = rest.splitn(3, ' ');
        let (kind, index) = (words.next().unwrap_or(""), words.next().and_then(|i| i.parse::<usize>().ok()));
        match (kind, index) {
            ("start", Some(i)) if i < self.steps.len() => {
                self.current = i;
                self.text.clear();
                self.tail.clear();
                self.missing = false;
                on_event(Event::Start);
            }
            ("missing", Some(_)) => self.missing = true,
            ("skip", Some(i)) if i < self.steps.len() => self.skipped[i] = true,
            ("end", Some(i)) if i < self.steps.len() => {
                let code = words.next().unwrap_or("").trim().to_string();
                let result = self.result(i, &code);
                on_event(Event::End);
                self.outcomes[i] = Some(result);
            }
            ("error", _) => self.error = Some(rest.trim_start_matches("error").trim().to_string()),
            _ => {}
        }
    }

    /// Step `i`'s result from its exit code.
    fn result(&mut self, i: usize, code: &str) -> Result<String> {
        if code == "0" {
            return Ok(std::mem::take(&mut self.text));
        }
        if self.missing {
            return Err(Error::ToolMissing { tool: "vagrant" });
        }
        let args: Vec<&str> = self.steps[i].args.iter().map(String::as_str).collect();
        let tail: Vec<String> = self.tail.iter().cloned().collect();
        Err(crate::platform::exec::failed("vagrant", &args, crate::platform::exec::stream_failure(&tail, format!("exit code: {code}"))))
    }

    /// The outcomes, once the script exited with `exit_code`; `reason`: why the watcher ended it,
    /// if it did. A step that neither ended nor was skipped (the script was cut short) fails the
    /// whole batch: the steps after it never ran.
    fn finish(self, exit_code: u32, reason: Option<&str>) -> Result<Vec<Outcome>> {
        if let Some(e) = self.error {
            return Err(Error::Invalid(format!("the Vagrant batch failed: {e}")));
        }
        let incomplete = self.outcomes.iter().zip(&self.skipped).any(|(o, skipped)| o.is_none() && !skipped);
        if incomplete || exit_code != 0 && self.outcomes.iter().all(Option::is_none) {
            return Err(Error::Invalid(match reason {
                Some(r) => stopped(r),
                None => format!("the Vagrant batch ended before it finished (exit code {exit_code})"),
            }));
        }
        Ok(self.outcomes)
    }
}

/// Follows the log of a batch's script, started in `work` and running as `script` (its exit code
/// once it ended): each line becomes the batch's events, and a question Vagrant would wait on
/// forever asks the watcher to end the batch at once. The batch's outcomes.
pub(super) async fn follow_batch(
    work: &mut WorkDir,
    marker: &str,
    steps: &[Step],
    script: impl std::future::Future<Output = Result<u32>>,
    mut on_event: impl FnMut(Event),
) -> Result<Vec<Outcome>> {
    let mut collector = Collector::new(marker, steps);
    let code = follow(&work.log(), script, |l| {
        if let Some(say) = prompt(l) {
            work.stop(say);
        }
        collector.line(l, &mut on_event);
    })
    .await??;
    let reason = std::fs::read_to_string(work.reason()).ok();
    work.finished();
    collector.finish(code, reason.as_deref())
}

/// How often a followed log is read for new lines.
const FOLLOW_EVERY: Duration = Duration::from_millis(200);

/// Follows the log at `path` while `done` runs, handing each complete line (without its line
/// ending) to `on_line`, then reads what was left once it finished. `done`'s output.
async fn follow<T>(path: &Path, done: impl std::future::Future<Output = T>, mut on_line: impl FnMut(&str)) -> std::io::Result<T> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path).await?;
    let mut pending: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 16 * 1024];
    tokio::pin!(done);
    loop {
        // Everything written so far.
        loop {
            let n = file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            pending.extend_from_slice(&buf[..n]);
            drain(&mut pending, false, &mut on_line);
        }
        tokio::select! {
            out = &mut done => {
                loop {
                    let n = file.read(&mut buf).await?;
                    if n == 0 {
                        break;
                    }
                    pending.extend_from_slice(&buf[..n]);
                }
                drain(&mut pending, true, &mut on_line);
                return Ok(out);
            }
            () = tokio::time::sleep(FOLLOW_EVERY) => {}
        }
    }
}

/// Hands each complete line in `pending` to `on_line` (and, when `last`, what is left).
fn drain(pending: &mut Vec<u8>, last: bool, on_line: &mut dyn FnMut(&str)) {
    while let Some(at) = pending.iter().position(|b| *b == b'\n') {
        let line: Vec<u8> = pending.drain(..=at).collect();
        on_line(String::from_utf8_lossy(&line).trim_end_matches(['\n', '\r']));
    }
    if last && !pending.is_empty() {
        on_line(String::from_utf8_lossy(pending).trim_end_matches('\r'));
        pending.clear();
    }
}

/// A batch's own folder (its log, environment file, and the watcher's `cancel` and `reason`
/// files). Dropped before the batch finished (its caller gave up), it asks the watcher to end
/// the batch, and is removed once the script has let go of it.
pub(super) struct WorkDir {
    dir: PathBuf,
    finished: bool,
}

impl WorkDir {
    pub(super) fn new() -> Result<WorkDir> {
        let dir = std::env::temp_dir().join(format!("cyberctf-vagrant-{:016x}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir)?;
        Ok(WorkDir { dir, finished: false })
    }

    pub(super) fn log(&self) -> PathBuf {
        self.dir.join("vagrant.log")
    }

    pub(super) fn env(&self) -> PathBuf {
        self.dir.join("env.json")
    }

    pub(super) fn cancel(&self) -> PathBuf {
        self.dir.join("cancel")
    }

    pub(super) fn reason(&self) -> PathBuf {
        self.dir.join("reason")
    }

    /// The batch is over: the folder can go once dropped.
    fn finished(&mut self) {
        self.finished = true;
    }

    /// A batch folder at `dir`, for the script's tests.
    #[cfg(test)]
    pub(super) fn at(dir: PathBuf) -> WorkDir {
        WorkDir { dir, finished: true }
    }

    /// Asks the watcher to end the batch, saying `why` (kept when asked already).
    fn stop(&self, why: &str) {
        if !self.cancel().exists() {
            let _ = std::fs::write(self.cancel(), why);
        }
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        if self.finished {
            let _ = std::fs::remove_dir_all(&self.dir);
            return;
        }
        self.stop("cancelled");
        // The script may still hold the log open for a moment (or not have started, the prompt
        // still up): try again for a while.
        let dir = self.dir.clone();
        std::thread::spawn(move || {
            for _ in 0..300 {
                if std::fs::remove_dir_all(&dir).is_ok() || !dir.exists() {
                    return;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::script::WATCH_EVERY;
    use super::*;

    #[test]
    fn questions_vagrant_waits_on_are_told_and_explained() {
        let switch = prompt("    default: Please choose a switch to attach to your Hyper-V instance.").unwrap();
        assert!(switch.contains("virtual switch") && switch.contains("try again"));
        assert!(prompt("You will be asked for the username and password to use for the SMB").unwrap().contains("shared folder"));
        assert!(prompt("==> default: Booting VM...").is_none());
        assert!(stopped("timeout").contains("6 hours"));
        assert!(stopped("cancelled").contains("cancelled") && stopped("").contains("cancelled"));
        assert_eq!(stopped(switch), switch);
    }

    fn collect(steps: &[Step], log: &[&str], exit: u32, reason: Option<&str>) -> (Vec<String>, Result<Vec<Outcome>>) {
        let mut events = Vec::new();
        let mut c = Collector::new("##m##", steps);
        for l in log {
            c.line(l, &mut |e: Event| {
                events.push(e.describe());
            });
        }
        (events, c.finish(exit, reason))
    }

    #[test]
    fn skipped_missing_and_broken_runs() {
        let steps = [Step::new(&["up"]), Step::new(&["ssh", "-c", "uname"])];
        let (_, outcomes) = collect(&steps, &["##m## start 0", "##m## missing 0", "##m## end 0 127", "##m## skip 1"], 0, None);
        let outcomes = outcomes.unwrap();
        assert!(matches!(&outcomes[0], Some(Err(Error::ToolMissing { tool: "vagrant" }))));
        assert!(outcomes[1].is_none(), "skipped");
        let (_, broken) = collect(&steps, &["##m## error Cannot find path 'C:\\lab' because it does not exist."], 1, None);
        assert!(broken.unwrap_err().to_string().contains("Cannot find path"));
        let (_, early) = collect(&steps, &[], 3, None);
        assert!(early.unwrap_err().to_string().contains("exit code 3"));
    }

    /// The batch's script as the test plays it: step 0 prints the line Vagrant prints before a
    /// question it would wait on forever, then the watcher (polling as the real one does) waits
    /// for the launcher to ask it to stop, writes why, and ends the script.
    async fn a_script_stuck_on_a_question(log: &Path, cancel: &Path, reason: &Path) -> Result<u32> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(log)?;
        f.write_all(b"##m## start 0\n    default: Please choose a switch to attach to your Hyper-V instance.\n")?;
        while !cancel.exists() {
            tokio::time::sleep(WATCH_EVERY).await;
        }
        std::fs::write(reason, std::fs::read_to_string(cancel)?)?;
        Ok(1)
    }

    #[tokio::test(start_paused = true)]
    async fn a_question_vagrant_waits_on_ends_the_batch_at_once_with_a_plain_message() {
        // Vagrant on Windows waits forever on its question: the launcher sees the line before it
        // and asks the watcher to end the run. On a fake clock: within one read of the log and one
        // look of the watcher, however loaded the machine.
        let mut work = WorkDir::new().unwrap();
        std::fs::File::create(work.log()).unwrap();
        let (log, cancel, reason) = (work.log(), work.cancel(), work.reason());
        let steps = [Step::new(&["up"])];
        let started = tokio::time::Instant::now();
        let result = follow_batch(&mut work, "##m##", &steps, a_script_stuck_on_a_question(&log, &cancel, &reason), |_| {}).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("which Hyper-V virtual switch") && err.contains("ISOLOOM_HYPERV_SWITCH"), "{err}");
        let ended = started.elapsed();
        assert!(ended <= FOLLOW_EVERY + WATCH_EVERY, "ended {ended:?} after the question");
    }

    #[tokio::test]
    async fn a_followed_log_yields_its_lines_as_they_are_written() {
        let path = std::env::temp_dir().join(format!("cyberctf-follow-{}.log", rand::random::<u32>()));
        std::fs::write(&path, "").unwrap();
        let writer = {
            let path = path.clone();
            async move {
                use std::io::Write;
                let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
                f.write_all(b"one\r\ntw").unwrap();
                tokio::time::sleep(Duration::from_millis(450)).await;
                f.write_all("o\nthr\u{e9}e\nno newline".as_bytes()).unwrap();
                7u32
            }
        };
        let mut lines = Vec::new();
        let code = follow(&path, writer, |l| lines.push(l.to_string())).await.unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(code, 7);
        assert_eq!(lines, ["one", "two", "thr\u{e9}e", "no newline"]);
    }

    #[test]
    fn an_abandoned_batch_asks_its_watcher_to_stop_and_goes_away() {
        let w = WorkDir::new().unwrap();
        let dir = w.dir.clone();
        w.stop("Vagrant stopped to ask something");
        w.stop("cancelled");
        assert_eq!(std::fs::read_to_string(w.cancel()).unwrap(), "Vagrant stopped to ask something", "the first reason stays");
        drop(w);
        for _ in 0..50 {
            if !dir.exists() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("{} left behind", dir.display());
    }
}
