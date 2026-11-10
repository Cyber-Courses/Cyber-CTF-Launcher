//! What every server's VM test shares: the throwaway lab it runs, and that lab's run reported step
//! by step. Each server says how its run comes up ([`Course`]): the steps `up` goes through, the
//! lines of the run that end each, and the failures it explains in the player's terms. The run
//! is brought up, its command run inside the VM, and it is always torn down, the teardown saying
//! what is left when it fails.

use std::path::{Path, PathBuf};

use super::{Ran, Reporter};
use crate::error::{Error, Result};
use crate::runtime::docker::Check;
use crate::runtime::lab;

/// The test lab: one Debian VM on a lab network with internet (on Proxmox the module adds its
/// router on the uplink), a command run inside it as its check. The `/24` is the lab's own, so
/// it never meets the server's networks.
pub(super) const LAB: &str = "version: 1
name: cyberctf-server-selftest
networks:
  test: { cidr: 10.254.250.0/24, internet: true }
machines:
  probe:
    os: debian@12
    networks: { test: 10 }
checks:
  - { name: command inside the test VM, from: probe, exec: uname -sm, expect: Linux }
";

/// A step the test reports: its id and its label.
pub(super) type Step = (&'static str, &'static str);

/// The steps every server's test reports around its own (the frontend shows the same plan).
pub(super) const PREPARE: Step = ("prepare", "Prepare the test lab");
pub(super) const SSH: Step = ("ssh", "Reach the test VM over SSH");
pub(super) const COMMAND: Step = ("command", "Run a command inside the test VM");
pub(super) const CLEANUP: Step = ("cleanup", "Remove everything the test created");

/// The id the test's run is named after (`cyberctf-<id>`), which no catalogue lab takes.
pub(super) fn run_id(host: &str) -> String {
    format!("server-selftest-{host}")
}

/// Writes the test lab into `work` and generates it for `target`, as a launch does.
pub(super) fn write_lab(work: &Path, target: lab::Target) -> Result<PathBuf> {
    let dir = work.join("lab");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("isoloom.yml"), LAB)?;
    lab::prepare(&dir, target)?;
    Ok(dir)
}

/// The test lab a previous test kept in `work` (its teardown failed, or the app quit while it
/// ran): what to remove from the server before anything else. None when it never got as far as
/// generating its files, so created nothing.
pub(super) fn kept_lab(work: &Path) -> Option<PathBuf> {
    let dir = work.join("lab");
    dir.join(".isoloom").exists().then_some(dir)
}

/// Removes the test lab a previous test kept in `work` ([`kept_lab`]) from the server, its run
/// opened from that folder by `open`; nothing when none was kept.
pub(super) async fn clear_kept<R: TestRun, F: Future<Output = Result<R>>>(work: &Path, open: impl FnOnce(PathBuf) -> F) -> Result<()> {
    let Some(dir) = kept_lab(work) else { return Ok(()) };
    let run = Box::pin(open(dir)).await?;
    Box::pin(run.down(&mut |_| {})).await
}

/// The test proper: the lab prepared (`prepare`: written, its run opened, and what to say of it),
/// then [`test`] with `course`. A preparation that failed reached nothing on the server, so it
/// leaves nothing to remove.
pub(super) async fn prepared_test<R: TestRun>(r: &Reporter, prepare: impl Future<Output = Result<(R, Option<String>)>>, course: impl Course) -> Ran {
    match r.step(PREPARE.0, PREPARE.1, prepare).await {
        Err(e) => Ran { result: Err(e), cleaned: true },
        // Boxed: a whole lab's run on the server makes a large future.
        Ok(run) => Box::pin(test(r, &run, course)).await,
    }
}

/// What the test drives: the lab's run on the server (the runner's in the app; a scripted one in
/// tests).
pub(super) trait TestRun {
    async fn up(&self, log: &mut (dyn FnMut(String) + Send)) -> Result<()>;
    async fn check(&self) -> Result<Check>;
    async fn down(&self, log: &mut (dyn FnMut(String) + Send)) -> Result<()>;
}

/// How one server's test lab comes up, as its test reports it.
pub(super) trait Course: Send {
    /// The steps `up` goes through, in order: each ended by a line of the run ([`Course::ends`]),
    /// the last by `up` succeeding.
    const UP: &'static [Step];

    /// The step `line` of the run ends, as its index in [`Course::UP`]; what the line says may be
    /// noted for a step's detail or an explanation.
    fn ends(&mut self, line: &str) -> Option<usize>;

    /// How many bytes step `at` of [`Course::UP`] moves, once the run's lines say (the image it
    /// uploads): its live lines carry it.
    fn size(&self, _at: usize) -> Option<u64> {
        None
    }

    /// What step `at` of [`Course::UP`] says when it ends.
    fn detail(&self, _at: usize) -> Option<String> {
        None
    }

    /// A failure of `up`, `raw`, as the step it belongs to and what to do, in the player's terms.
    fn explain(&self, raw: &str) -> Option<(usize, String)>;

    /// What a teardown that failed with `e` leaves on the server, and how to remove it.
    fn left(&self, e: &Error) -> String;
}

/// Brings the test lab up, step by step, runs its command, and always tears it down.
pub(super) async fn test(r: &Reporter, run: &impl TestRun, course: impl Course) -> Ran {
    let mut steps = Steps::new(r, course);
    steps.start();
    let up = run.up(&mut |line| steps.line(&line)).await;
    let result = match up {
        Ok(()) => {
            steps.finish();
            command(r, run).await
        }
        Err(e) => Err(steps.fail(&e)),
    };
    let down = cleanup(r, run, &steps.course).await;
    let cleaned = down.is_ok();
    Ran { result: result.and(down), cleaned }
}

/// The command step: the lab's check, a command run inside the VM over SSH.
async fn command(r: &Reporter, run: &impl TestRun) -> Result<()> {
    r.step(COMMAND.0, COMMAND.1, async {
        let check = run.check().await?;
        if check.ok {
            Ok(((), Some(check.output.lines().find(|l| l.contains("Linux")).unwrap_or("Linux").trim().to_owned())))
        } else {
            Err(Error::Invalid(format!("the command inside the test VM failed: {}", check.output.trim())))
        }
    })
    .await
}

/// The teardown, reported whatever happened before; a failure says what is left.
async fn cleanup(r: &Reporter, run: &impl TestRun, course: &impl Course) -> Result<()> {
    r.send(CLEANUP.0, CLEANUP.1, "running", None);
    match run.down(&mut |line| r.progress(CLEANUP.0, CLEANUP.1, &line)).await {
        Ok(()) => {
            r.send(CLEANUP.0, CLEANUP.1, "ok", Some("Nothing of the test is left on the server.".into()));
            Ok(())
        }
        Err(e) => {
            let left = course.left(&e);
            r.send(CLEANUP.0, CLEANUP.1, "fail", Some(left.clone()));
            Err(Error::Invalid(left))
        }
    }
}

/// Where `up` is among its steps: the first not yet done, moved on by the lines that end each.
struct Steps<'a, C> {
    r: &'a Reporter,
    course: C,
    done: usize,
}

impl<'a, C: Course> Steps<'a, C> {
    fn new(r: &'a Reporter, course: C) -> Self {
        Steps { r, course, done: 0 }
    }

    fn start(&self) {
        if let Some((step, label)) = C::UP.get(self.done) {
            self.r.send(step, label, "running", None);
        }
    }

    /// Step `done` passed, with what it says.
    fn pass(&mut self) {
        let (step, label) = C::UP[self.done];
        self.r.send(step, label, "ok", self.course.detail(self.done));
        self.done += 1;
    }

    /// A line of the run: the end of a step (and of any before it), else the running step's
    /// latest detail.
    fn line(&mut self, line: &str) {
        match self.course.ends(line) {
            Some(at) if at >= self.done => {
                while self.done <= at {
                    self.pass();
                }
                self.start();
            }
            _ => {
                if let Some((step, label)) = C::UP.get(self.done) {
                    self.r.progress_sized(step, label, line, self.course.size(self.done));
                }
            }
        }
    }

    /// `up` succeeded: every step is done (SSH reached the machine for its ready check).
    fn finish(&mut self) {
        while self.done < C::UP.len() {
            self.pass();
        }
    }

    /// `up` failed with `e`: the step it failed at says why in the player's terms; the ones before
    /// it that no line ended are skipped. The error returned is that explanation.
    fn fail(&mut self, e: &Error) -> Error {
        let raw = e.to_string();
        let last = C::UP.len() - 1;
        let (at, why) = self.course.explain(&raw).map_or((self.done.min(last), raw.clone()), |(at, why)| (at.max(self.done).min(last), why));
        while self.done < at {
            let (step, label) = C::UP[self.done];
            self.r.send(step, label, "skip", None);
            self.done += 1;
        }
        let (step, label) = C::UP[at];
        self.r.send(step, label, "fail", Some(why.clone()));
        self.done = C::UP.len();
        Error::Invalid(why)
    }
}

/// A scripted run, and how to read what a test reported, for each server's tests.
#[cfg(test)]
pub(super) mod scripted {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::runtime::selftest_report::Event;

    /// A scripted run: the lines its `up` prints and how it ends, its check, its teardown, and
    /// what was called.
    pub(in crate::runtime::server_selftest) struct Scripted {
        lines: Vec<&'static str>,
        up: Option<&'static str>,
        pub(in crate::runtime::server_selftest) check_ok: bool,
        pub(in crate::runtime::server_selftest) down: Option<&'static str>,
        calls: Mutex<Vec<&'static str>>,
    }

    impl Scripted {
        pub(in crate::runtime::server_selftest) fn new(lines: &[&'static str], up: Option<&'static str>) -> Self {
            Scripted { lines: lines.to_vec(), up, check_ok: true, down: None, calls: Mutex::new(Vec::new()) }
        }
        pub(in crate::runtime::server_selftest) fn calls(&self) -> Vec<&'static str> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl TestRun for Scripted {
        // A scripted run answers at once: its steps are ready futures.
        fn up(&self, log: &mut (dyn FnMut(String) + Send)) -> impl Future<Output = Result<()>> {
            self.calls.lock().unwrap().push("up");
            self.lines.iter().for_each(|l| log((*l).to_owned()));
            std::future::ready(self.up.map_or(Ok(()), |e| Err(Error::Invalid(e.into()))))
        }
        fn check(&self) -> impl Future<Output = Result<Check>> {
            self.calls.lock().unwrap().push("check");
            std::future::ready(Ok(Check { available: true, ok: self.check_ok, output: "Linux x86_64".into(), results: Vec::new() }))
        }
        fn down(&self, _log: &mut (dyn FnMut(String) + Send)) -> impl Future<Output = Result<()>> {
            self.calls.lock().unwrap().push("down");
            std::future::ready(self.down.map_or(Ok(()), |e| Err(Error::Invalid(e.into()))))
        }
    }

    pub(in crate::runtime::server_selftest) type Seen = Arc<Mutex<Vec<Event>>>;

    pub(in crate::runtime::server_selftest) fn reporter() -> (Reporter, Seen) {
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        (Reporter::new(move |e| sink.lock().unwrap().push(e)), seen)
    }

    /// Each step's last state, in the order steps were first reported.
    pub(in crate::runtime::server_selftest) fn ends(seen: &Seen) -> Vec<String> {
        let mut out: Vec<(String, String)> = Vec::new();
        for e in seen.lock().unwrap().iter() {
            match out.iter_mut().find(|(s, _)| s == e.step) {
                Some(found) => found.1 = e.state.to_owned(),
                None => out.push((e.step.to_owned(), e.state.to_owned())),
            }
        }
        out.into_iter().map(|(s, st)| format!("{s}:{st}")).collect()
    }

    /// The detail of step `step`'s last event in `state`.
    pub(in crate::runtime::server_selftest) fn detail(seen: &Seen, step: &str, state: &str) -> String {
        seen.lock().unwrap().iter().rev().find(|e| e.step == step && e.state == state).and_then(|e| e.detail.clone()).unwrap_or_default()
    }

    /// The size step `step`'s last running event carried.
    pub(in crate::runtime::server_selftest) fn running_size(seen: &Seen, step: &str) -> Option<u64> {
        seen.lock().unwrap().iter().rev().find(|e| e.step == step && e.state == "running").and_then(|e| e.size)
    }

    /// Whether step `step` showed a running line containing `text`.
    pub(in crate::runtime::server_selftest) fn showed(seen: &Seen, step: &str, text: &str) -> bool {
        seen.lock().unwrap().iter().any(|e| e.step == step && e.state == "running" && e.detail.as_deref().is_some_and(|d| d.contains(text)))
    }
}

#[cfg(test)]
mod tests {
    use super::scripted::*;
    use super::*;

    /// A course of two steps: `one` ends on a line `one done` and says `first` when it does, a
    /// failure saying `bad two` belongs to `two`.
    struct Two;

    impl Course for Two {
        const UP: &'static [Step] = &[("one", "One"), ("two", "Two")];
        fn ends(&mut self, line: &str) -> Option<usize> {
            (line == "one done").then_some(0)
        }
        fn detail(&self, at: usize) -> Option<String> {
            (at == 0).then(|| "first".into())
        }
        fn size(&self, at: usize) -> Option<u64> {
            (at == 1).then_some(2_400_000_000)
        }
        fn explain(&self, raw: &str) -> Option<(usize, String)> {
            raw.contains("bad two").then(|| (1, "two went wrong".into()))
        }
        fn left(&self, e: &Error) -> String {
            format!("left behind ({e})")
        }
    }

    #[tokio::test]
    async fn each_step_ends_on_its_line_with_its_detail_then_the_command_and_the_teardown() {
        let (r, seen) = reporter();
        let run = Scripted::new(&["working", "one done", "still going"], None);
        let ran = test(&r, &run, Two).await;
        assert!(ran.result.is_ok() && ran.cleaned);
        assert_eq!(ends(&seen), ["one:ok", "two:ok", "command:ok", "cleanup:ok"]);
        assert_eq!(detail(&seen, "one", "ok"), "first");
        assert!(showed(&seen, "one", "working") && showed(&seen, "two", "still going"));
        assert_eq!(run.calls(), ["up", "check", "down"]);
    }

    #[tokio::test]
    async fn a_steps_live_lines_carry_the_size_its_course_knows_it_moves() {
        let (r, seen) = reporter();
        let run = Scripted::new(&["working", "one done", "Disk progress: 8%"], None);
        assert!(test(&r, &run, Two).await.result.is_ok());
        assert_eq!(running_size(&seen, "one"), None);
        assert_eq!(running_size(&seen, "two"), Some(2_400_000_000));
        assert_eq!(detail(&seen, "two", "running"), "Disk progress: 8%");
    }

    #[tokio::test]
    async fn an_explained_failure_skips_the_steps_no_line_ended_and_still_tears_down() {
        let (r, seen) = reporter();
        let run = Scripted::new(&[], Some("bad two"));
        let ran = test(&r, &run, Two).await;
        assert_eq!(ran.result.unwrap_err().to_string(), "two went wrong");
        assert!(ran.cleaned);
        assert_eq!(ends(&seen), ["one:skip", "two:fail", "cleanup:ok"]);
        assert_eq!(run.calls(), ["up", "down"]);
    }

    #[tokio::test]
    async fn a_failed_teardown_says_what_its_course_says_is_left() {
        let (r, seen) = reporter();
        let mut run = Scripted::new(&[], None);
        run.down = Some("host gone");
        let ran = test(&r, &run, Two).await;
        assert!(!ran.cleaned);
        assert_eq!(ran.result.unwrap_err().to_string(), "left behind (host gone)");
        assert_eq!(detail(&seen, "cleanup", "fail"), "left behind (host gone)");
    }

    #[tokio::test]
    async fn a_failed_preparation_reports_its_step_and_leaves_nothing_to_remove() {
        let (r, seen) = reporter();
        let ran = prepared_test(&r, async { Err::<(Scripted, _), _>(Error::Invalid("no runner".into())) }, Two).await;
        assert!(ran.cleaned && ran.result.unwrap_err().to_string() == "no runner");
        assert_eq!(ends(&seen), ["prepare:fail"]);
        let (r, seen) = reporter();
        let ran = prepared_test(&r, async { Ok((Scripted::new(&[], None), Some("on node pve".into()))) }, Two).await;
        assert!(ran.result.is_ok());
        assert_eq!(ends(&seen), ["prepare:ok", "one:ok", "two:ok", "command:ok", "cleanup:ok"]);
        assert_eq!(detail(&seen, "prepare", "ok"), "on node pve");
    }

    #[tokio::test]
    async fn a_kept_lab_is_torn_down_from_its_folder_before_the_next_test() {
        let work = std::env::temp_dir().join(format!("cyberctf-srv-clear-{}", rand::random::<u32>()));
        let opened = std::sync::Mutex::new(Vec::new());
        // A run opened from `dir`, its teardown ending as `down`.
        let open = |down: Option<&'static str>| {
            let opened = &opened;
            move |dir: PathBuf| {
                opened.lock().unwrap().push(dir);
                let mut run = Scripted::new(&[], None);
                run.down = down;
                std::future::ready(Ok(run))
            }
        };
        // Nothing kept: no run opened, nothing to remove.
        clear_kept(&work, open(None)).await.unwrap();
        assert!(opened.lock().unwrap().is_empty());
        // A lab kept (a failed teardown, or a test cut short): opened from its folder, torn down.
        std::fs::create_dir_all(work.join("lab").join(".isoloom")).unwrap();
        clear_kept(&work, open(None)).await.unwrap();
        assert_eq!(*opened.lock().unwrap(), [work.join("lab")]);
        // Its teardown failing says so.
        assert_eq!(clear_kept(&work, open(Some("still there"))).await.unwrap_err().to_string(), "still there");
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn a_kept_lab_is_one_whose_files_were_generated() {
        let work = std::env::temp_dir().join(format!("cyberctf-srv-kept-{}", rand::random::<u32>()));
        assert_eq!(kept_lab(&work), None, "no folder: nothing was created");
        std::fs::create_dir_all(work.join("lab")).unwrap();
        assert_eq!(kept_lab(&work), None, "a lab that never generated created nothing");
        std::fs::create_dir_all(work.join("lab").join(".isoloom")).unwrap();
        assert_eq!(kept_lab(&work), Some(work.join("lab")));
        std::fs::remove_dir_all(&work).unwrap();
        assert_eq!(run_id("ab12"), "server-selftest-ab12");
    }
}
