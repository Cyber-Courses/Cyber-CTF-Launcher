//! The launcher's guided machine setup on this machine, end to end and without a window: what the
//! setup window would install, start or restart, in its order ([`crate::machine::setup::plan`],
//! the plan the window renders), run through the functions the window's commands call, then the
//! self-tests and an example lab. A caller runs it pass after pass on a bare Windows or Linux
//! machine (a throwaway one), restarting it or logging in again between passes as an item says.
//!
//! It installs real tools on this machine, as an administrator: never run it on a machine of
//! yours, unless check-only (below). It returns at once without its knobs:
//!
//! - `CYBERCTF_TEST_SETUP_PATHS`: the lab types to set up, `docker,vm`;
//! - `CYBERCTF_TEST_SETUP_HYPERVISORS`: the hypervisors to try, in order (`libvirt,virtualbox`):
//!   the first the setup can set up is set up, each other one reported (`skipped` and why);
//! - `CYBERCTF_TEST_SETUP_LOGS`: a folder for each action's log, and the machine reports before,
//!   after each action, and after;
//! - `CYBERCTF_TEST_SETUP_LAB`: an Isoloom example lab, started and stopped once the machine is
//!   ready (optional);
//! - `CYBERCTF_TEST_RESULTS`: the file each item's record is appended to, as it goes (a crash
//!   still leaves what was done), in the format of every test runner's `--results`
//!   (tests/README.md, "Results"; [`support::results`](super::support::results));
//! - `CYBERCTF_TEST_SETUP_REPORT`: the same items as `@@setup` lines, the report's format before
//!   the records, while the test rig reads it (remove once it reads the records);
//! - `CYBERCTF_TEST_SETUP_CHECK_ONLY`: `1` checks only, see below.
//!
//! Check-only (`CYBERCTF_TEST_SETUP_CHECK_ONLY=1`), safe on any machine: the machine check and the
//! plan run exactly as the app runs them, and the report, the records and the `@@setup` lines are
//! written as in a pass, but nothing is installed, started, removed or restarted, and no
//! self-test or lab runs. An item the setup finds in place is `present` (`pass`), one it can't
//! set up `failed` (`fail`), one it leaves to the player `skipped`, and anything it would do is
//! `check-only` (`skip`, reason `check-only: would <action>`): each step of the plan, a tool that
//! isn't ready yet but would be (`ready:<tool>`), the self-tests and the lab.
//!
//! An item's status is `installed` (done in this pass), `present` (there already, or ready),
//! `failed` (the note names its log), `skipped` (the note says why) or `again` (note `reboot` or
//! `relogin`: the pass ends there, passing, and the next one picks up after the restart or the new
//! login). Its record: suite `setup`, case the item, provider the hypervisor the pass sets up,
//! result `pass` (installed, present), `fail` or `skip` (skipped, again), reason
//! `<status>: <note>`. Its `@@setup` line: `@@setup<TAB>item<TAB>status<TAB>seconds<TAB>note`. The
//! items: `wsl`, `docker`, `engine:<id>`, `vagrant`, `<hypervisor>`, `plugin:<name>`,
//! `vmware-utility`, then `ready:<tool>`, `selftest:docker`, `selftest:vm`, `lab:<name>`.
//!
//! It never waits on a prompt of its own: the installs elevate the way the app does (UAC through
//! winget and PowerShell, pkexec on Linux), which the rig answers (an elevated scheduled task, a
//! polkit rule); without pkexec, an install's line says so. Built once with the library's tests,
//! it runs from that test binary on a machine without the sources:
//!
//! ```text
//! CYBERCTF_TEST_SETUP_PATHS=docker,vm CYBERCTF_TEST_SETUP_HYPERVISORS=libvirt,virtualbox \
//! CYBERCTF_TEST_RESULTS=setup.jsonl CYBERCTF_TEST_SETUP_LOGS=setup-logs \
//! CYBERCTF_TEST_SETUP_LAB=<isoloom checkout>/examples/hello-stack \
//! cargo test --lib -- --ignored --exact integration_tests::setup::the_guided_setup_readies_this_machine --nocapture --test-threads=1
//! ```

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::support::lab::install_copy;
use super::support::results::{self, Record, Verdict};
use super::support::{check, scenario};
use crate::machine::selftest::{self, Kind};
use crate::machine::setup::{Action, ManualReason, Plan, SetupPath, Step, Wanted, WindowsFeature, plan};
use crate::machine::system::{self, SystemReport, docker};
use crate::machine::tools::install;
use crate::platform::AppDirs;
use crate::runtime::providers::{Provider, UtilityState};
use crate::runtime::{isothrum, launch, lifecycle};

/// What each installed item leaves on the machine, for a caller's leftover audit.
mod leaves;

/// The test's knobs.
struct Knobs {
    wanted: Wanted,
    /// Where the records go (`CYBERCTF_TEST_RESULTS`), and the `@@setup` lines (the old report).
    results: Option<PathBuf>,
    report: Option<PathBuf>,
    logs: PathBuf,
    lab: Option<PathBuf>,
    /// Detect and plan only (`CYBERCTF_TEST_SETUP_CHECK_ONLY=1`): nothing is run.
    check_only: bool,
}

impl Knobs {
    /// The knobs from the environment; none without the paths and the logs.
    fn from_env() -> Option<Knobs> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let paths = super::support::knob("CYBERCTF_TEST_SETUP_PATHS")?;
        let paths: Vec<SetupPath> = paths
            .split(',')
            .map(|p| serde_json::from_value(serde_json::json!(p.trim())).unwrap_or_else(|_| panic!("CYBERCTF_TEST_SETUP_PATHS: `{p}` is not docker or vm")))
            .collect();
        let hypervisors = var("CYBERCTF_TEST_SETUP_HYPERVISORS")
            .unwrap_or_default()
            .split(',')
            .filter(|h| !h.trim().is_empty())
            .map(|h| Provider::from_id(h.trim()).unwrap_or_else(|| panic!("CYBERCTF_TEST_SETUP_HYPERVISORS: `{h}` is not a hypervisor")))
            .collect();
        Some(Knobs {
            wanted: Wanted { paths, engine: None, hypervisors },
            results: var(results::RESULTS).map(PathBuf::from),
            report: var("CYBERCTF_TEST_SETUP_REPORT").map(PathBuf::from),
            logs: PathBuf::from(var("CYBERCTF_TEST_SETUP_LOGS")?),
            lab: var("CYBERCTF_TEST_SETUP_LAB").map(PathBuf::from),
            check_only: var("CYBERCTF_TEST_SETUP_CHECK_ONLY").is_some_and(|v| v.trim() == "1"),
        })
    }
}

/// An item's status on its line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Status {
    Installed,
    Present,
    Failed,
    Skipped,
    Again,
    /// What a check-only pass would do, not done.
    CheckOnly,
}

impl Status {
    /// The status's verdict in a record: a pass that ends for a restart is a skip, saying so.
    fn verdict(self) -> Verdict {
        match self {
            Status::Installed | Status::Present => Verdict::Pass,
            Status::Failed => Verdict::Fail,
            Status::Skipped | Status::Again | Status::CheckOnly => Verdict::Skip,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Status::Installed => "installed",
            Status::Present => "present",
            Status::Failed => "failed",
            Status::Skipped => "skipped",
            Status::Again => "again",
            Status::CheckOnly => "check-only",
        }
    }
}

/// The note on one line.
fn one_line(note: &str) -> String {
    let note: String = note.chars().map(|c| if matches!(c, '\t' | '\r' | '\n') { ' ' } else { c }).collect();
    note.trim().to_owned()
}

/// One item's line in the old report: `@@setup<TAB>item<TAB>status<TAB>seconds<TAB>note`.
fn line(item: &str, status: Status, seconds: u64, note: &str) -> String {
    format!("@@setup\t{item}\t{}\t{seconds}\t{}", status.word(), one_line(note))
}

/// Where the pass writes: its records (and the old report), item by item, and its logs folder.
struct Out {
    results: Option<PathBuf>,
    report: Option<PathBuf>,
    /// The hypervisor the pass sets up, its records' provider ("" for none).
    hypervisor: String,
    logs: PathBuf,
    /// The entry's own folder (`setup_root`), which an installed item's record names among what
    /// it leaves.
    root: PathBuf,
    /// The items that failed, for the test's verdict.
    failed: Vec<String>,
    /// How many items this pass installed: none, and the entry's folder is dropped at the end
    /// ([`end_of_pass`]).
    installed: usize,
    /// How many logs and reports were written, for their order in the folder.
    written: usize,
}

impl Out {
    /// Where `knobs` say the pass writes, nothing written yet.
    fn new(knobs: &Knobs) -> Out {
        Out {
            results: knobs.results.clone(),
            report: knobs.report.clone(),
            hypervisor: String::new(),
            logs: knobs.logs.clone(),
            root: setup_root(),
            failed: Vec::new(),
            installed: 0,
            written: 0,
        }
    }

    /// One item's record: on the pass's hypervisor, the reason its status and note, its log the
    /// logs folder, and for an item installed in this pass, what the install leaves on the
    /// machine on purpose ([`leaves`]), in the leftover audit's words.
    fn record(&self, item: &str, status: Status, seconds: u64, note: &str) -> Record {
        Record {
            suite: "setup".to_owned(),
            case: item.to_owned(),
            provider: self.hypervisor.clone(),
            form: String::new(),
            result: status.verdict(),
            reason: format!("{}: {}", status.word(), one_line(note)),
            seconds,
            log: self.logs.display().to_string(),
            leaves: if status == Status::Installed { leaves::of(item, std::env::consts::OS, &self.root) } else { Vec::new() },
        }
    }

    /// Appends an item's record (and its old report line) and prints it, at once: a crash keeps it.
    fn line(&mut self, item: &str, status: Status, seconds: u64, note: &str) {
        let record = self.record(item, status, seconds, note);
        println!("{}", record.line());
        if let Some(file) = &self.results {
            results::append(file, &record);
        }
        if let Some(report) = &self.report {
            let text = line(item, status, seconds, note);
            let appended = std::fs::OpenOptions::new().create(true).append(true).open(report).and_then(|mut f| writeln!(f, "{text}"));
            if let Err(e) = appended {
                panic!("appending to {}: {e}", report.display());
            }
        }
        if status == Status::Installed {
            self.installed += 1;
        }
        if status == Status::Failed {
            self.failed.push(format!("{item}: {note}"));
        }
    }

    /// The next file of the logs folder, named after `what`, numbered in order.
    fn file(&mut self, what: &str, extension: &str) -> PathBuf {
        self.written += 1;
        let what: String = what.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect();
        self.logs.join(format!("{:02}-{what}.{extension}", self.written))
    }

    /// Saves the machine report `r` as `<what>.json`.
    fn save_report(&mut self, what: &str, r: &SystemReport) {
        let file = self.file(what, "json");
        if let Err(e) = std::fs::write(&file, serde_json::to_string_pretty(r).unwrap_or_default()) {
            println!("couldn't save {}: {e}", file.display());
        }
    }
}

/// A log file, written line by line as an action runs.
struct Log {
    file: Option<std::fs::File>,
    name: String,
}

impl Log {
    fn create(path: &Path) -> Log {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Log { file: std::fs::File::create(path).ok(), name }
    }

    fn write(&mut self, l: &str) {
        println!("  {l}");
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "{l}");
        }
    }
}

/// The entry's own folder, under the system's temporary folder: the app's folders for the pass
/// (`dirs`); kept from pass to pass, and named among what an installed item leaves.
fn setup_root() -> PathBuf {
    std::env::temp_dir().join("cyberctf-guided-setup")
}

/// The end of a pass that doesn't stop at a restart: the entry's folder is dropped when the pass
/// installed nothing, since only an installed item's record names it among what is left on the
/// machine (the leftover audit reported it after every check-only or nothing-to-install pass:
/// fedora45-bare's setup shards, where the guided setup installs nothing, 2026-11-12). A pass
/// that installed something keeps it: its marks are the next pass's.
fn end_of_pass(installed: usize, root: &Path) {
    if installed == 0 {
        let _ = std::fs::remove_dir_all(root);
    }
}

/// The app's folders for the pass: kept from pass to pass (the WSL and Hyper-V marks, what the
/// setup installed), as a player's app keeps them.
fn dirs() -> AppDirs {
    let root = setup_root();
    let (data, cache) = (root.join("data"), root.join("cache"));
    for dir in [&data, &cache] {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("creating {}: {e}", dir.display()));
    }
    AppDirs::new(Some(data), Some(cache))
}

/// What `step` does, by the function the window's command for it calls, into `log`: its status
/// and note.
async fn run_step(dirs: &AppDirs, step: &Step, log: &mut Log) -> (Status, String) {
    if let Some(left) = left_alone(&step.action) {
        return left;
    }
    let mut on_line = |l: String| log.write(&l);
    let done = match &step.action {
        Action::Install { dependency } => {
            install::install_dependency(dirs.clone(), *dependency, &mut on_line).await.map(|()| format!("{} installed", dependency.name()))
        }
        Action::Plugin { plugin } => install::install_vagrant_plugin(plugin.clone(), &mut on_line).await.map(|()| format!("{plugin} installed")),
        Action::StartEngine { engine } => docker::docker_start_engine(engine.id().to_owned()).await.map(|()| format!("{} started", engine.id())),
        Action::UseEngine { engine } => docker::docker_use_engine(engine.id().to_owned()).await.map(|()| format!("Docker uses {}", engine.id())),
        Action::Manual { .. } | Action::Blocked { .. } => unreachable!("left alone above"),
        Action::Restart { .. } | Action::Relogin => unreachable!("a pass ends at a restart or a new login"),
    };
    match done {
        Ok(note) => (Status::Installed, note),
        Err(e) => {
            log.write(&format!("failed: {e}"));
            (Status::Failed, format!("{e}; see {}", log.name))
        }
    }
}

/// A step the setup doesn't run itself, as a pass records it: the player's to take (skipped) or one
/// that can't run here (failed). None for a step the setup runs.
fn left_alone(action: &Action) -> Option<(Status, String)> {
    Some(match action {
        Action::Manual { reason: ManualReason::Distribution, dependency, .. } => {
            let what = dependency.map_or("it", |d| d.name());
            (Status::Skipped, format!("this distribution isn't supported by the guided setup yet: install {what} yourself"))
        }
        Action::Manual { reason: ManualReason::Vendor, url, .. } => (Status::Skipped, format!("links out: {}", url.as_deref().unwrap_or("its vendor's site"))),
        Action::Blocked { reason } => (Status::Failed, format!("can't run here: {reason}")),
        _ => return None,
    })
}

/// What the setup would do for a step it runs, said by a check-only pass (`would …`); None for
/// one it leaves alone ([`left_alone`]).
fn would(action: &Action) -> Option<String> {
    Some(match action {
        Action::Install { dependency } => format!("would install {}", dependency.name()),
        Action::Plugin { plugin } => format!("would install the Vagrant plugin {plugin}"),
        Action::StartEngine { engine } => format!("would start {}", engine.id()),
        Action::UseEngine { engine } => format!("would point Docker at {}", engine.id()),
        Action::Restart { feature } => format!(
            "would restart the computer to turn {} on",
            match feature {
                WindowsFeature::Wsl => "WSL 2",
                WindowsFeature::Hyperv => "Hyper-V",
            }
        ),
        Action::Relogin => "would sign out and back in".to_owned(),
        Action::Blocked { reason } if action.ends_pass() => format!("would restart the computer: {reason}"),
        Action::Manual { .. } | Action::Blocked { .. } => return None,
    })
}

/// Whether a step about `item` sets up `tool` (a `ready:` line's): its own item, or for Docker and
/// Compose, Docker's install, its engine or WSL.
fn sets_up(item: &str, tool: &str) -> bool {
    item == tool || (matches!(tool, "docker" | "compose") && (item == "docker" || item == "wsl" || item.starts_with("engine:")))
}

/// The example lab's item: `lab:<its folder's name>`.
fn lab_item(lab: &Path) -> String {
    format!("lab:{}", lab.file_name().map_or_else(|| "lab".to_owned(), |n| n.to_string_lossy().into_owned()))
}

/// A check-only pass: what the machine check `before` found and what the setup would do about it
/// (`first`, the plan for the knobs' wants), recorded item by item as a pass records them, and
/// nothing run: no install, engine, restart, self-test or lab, nor any tool at all.
fn check_only(out: &mut Out, before: &SystemReport, first: &Plan, knobs: &Knobs) {
    // The plan a pass acts on: the hypervisor the first plan picked, as `act` plans it.
    let wanted = Wanted { hypervisors: first.hypervisor.into_iter().collect(), ..knobs.wanted.clone() };
    let steps = plan(before, &before.setup, &wanted).steps;
    for step in &steps {
        match (would(&step.action), left_alone(&step.action)) {
            (Some(what), _) => out.line(&step.item, Status::CheckOnly, 0, &what),
            (None, Some((status, note))) => out.line(&step.item, status, 0, &note),
            (None, None) => unreachable!("a step is run or left alone"),
        }
    }
    for item in first.items.iter().filter(|i| !steps.iter().any(|s| &s.item == *i)) {
        out.line(item, Status::Present, 0, "nothing to do");
    }
    let acts_on = |tool: &str| steps.iter().any(|s| would(&s.action).is_some() && sets_up(&s.item, tool));
    let ready = readiness(before, &knobs.wanted, first.hypervisor);
    for (tool, ok, note) in &ready {
        let item = format!("ready:{tool}");
        match (*ok, acts_on(tool)) {
            (true, _) => out.line(&item, Status::Present, 0, note),
            (false, true) => out.line(&item, Status::CheckOnly, 0, &format!("would be ready once set up (now: {note})")),
            (false, false) => out.line(&item, Status::Failed, 0, note),
        }
    }
    let will_be = |tool: &str| ready.iter().any(|(t, ok, _)| t == tool && (*ok || acts_on(t)));
    let docker_ready = knobs.wanted.paths.contains(&SetupPath::Docker) && will_be("docker");
    let vm_ready = first.hypervisor.filter(|h| will_be(h.id()) && will_be("vagrant"));
    if knobs.wanted.paths.contains(&SetupPath::Docker) {
        if docker_ready {
            out.line("selftest:docker", Status::CheckOnly, 0, "would run the Docker self-test");
        } else {
            out.line("selftest:docker", Status::Skipped, 0, "Docker isn't ready");
        }
    }
    if knobs.wanted.paths.contains(&SetupPath::Vm) {
        match vm_ready {
            Some(h) => out.line("selftest:vm", Status::CheckOnly, 0, &format!("would run the VM self-test on {}", h.id())),
            None => out.line("selftest:vm", Status::Skipped, 0, "no hypervisor is ready"),
        }
    }
    if let Some(lab) = &knobs.lab {
        match (docker_ready, vm_ready) {
            (true, _) => out.line(&lab_item(lab), Status::CheckOnly, 0, "would start and stop it in containers"),
            (false, Some(h)) => out.line(&lab_item(lab), Status::CheckOnly, 0, &format!("would start and stop it in a VM on {}", h.id())),
            (false, None) => out.line(&lab_item(lab), Status::Skipped, 0, "nothing to run it on"),
        }
    }
}

/// The setup's actions on this machine, one at a time, the report read again after each (an
/// install changes what comes next: Docker installed asks to start it, or for a new login). The
/// hypervisor is the one the first plan picked: one that can't run once installed is a failure,
/// not a reason to set up another. Returns the restart or the new login that ends the pass, if
/// one does, and the machine as it is then.
async fn act(dirs: &AppDirs, out: &mut Out, wanted: &Wanted, mut report: SystemReport) -> (Option<Step>, SystemReport, Vec<String>) {
    let mut done: Vec<Step> = Vec::new();
    loop {
        let p = plan(&report, &report.setup, wanted);
        let Some(step) = p.steps.iter().find(|s| !s.action.ends_pass() && !done.contains(s)).cloned() else {
            let acted = done.into_iter().map(|s| s.item).collect();
            return (p.steps.into_iter().find(|s| s.action.ends_pass()), report, acted);
        };
        let started = Instant::now();
        let mut log = Log::create(&out.file(&format!("{}-{}", step.item, kind(&step.action)), "log"));
        log.write(&format!("{}: {:?}", step.item, step.action));
        let (status, note) = run_step(dirs, &step, &mut log).await;
        out.line(&step.item, status, started.elapsed().as_secs(), &note);
        done.push(step.clone());
        report = system::system_check(dirs.clone()).await;
        out.save_report(&format!("report-after-{}", step.item), &report);
    }
}

/// An action's kind, for its log's name.
fn kind(action: &Action) -> &'static str {
    match action {
        Action::Install { .. } => "install",
        Action::Plugin { .. } => "plugin",
        Action::StartEngine { .. } => "start",
        Action::UseEngine { .. } => "use",
        Action::Restart { .. } => "restart",
        Action::Relogin => "relogin",
        Action::Manual { .. } => "manual",
        Action::Blocked { .. } => "blocked",
    }
}

/// Whether each wanted tool is ready by the machine report `r`: Docker installed and answering,
/// Compose, WSL 2 (Windows: nothing left for it in the plan), Vagrant, the hypervisor installed and
/// available, its plugin, VMware's utility running.
fn readiness(r: &SystemReport, wanted: &Wanted, hypervisor: Option<Provider>) -> Vec<(String, bool, String)> {
    let mut ready = Vec::new();
    if wanted.paths.contains(&SetupPath::Docker) {
        if r.os == "windows" {
            let state = r.wsl.as_ref().map_or_else(|| "not reported".to_owned(), |w| format!("{:?}", w.state));
            ready.push(("wsl".to_owned(), r.setup.wsl.is_empty(), format!("WSL 2: {state}")));
        }
        let engine = r.docker_engine.map_or("none", docker::Engine::id);
        ready.push((
            "docker".to_owned(),
            crate::machine::setup::docker_ready(r),
            format!("{} (engine {engine})", r.docker.version.as_deref().unwrap_or("no Docker")),
        ));
        ready.push(("compose".to_owned(), r.docker_compose.installed, r.docker_compose.version.clone().unwrap_or_else(|| "no Compose".into())));
    }
    if wanted.paths.contains(&SetupPath::Vm) {
        ready.push(("vagrant".to_owned(), r.vagrant.installed, r.vagrant.version.clone().unwrap_or_else(|| "no Vagrant".into())));
        if let Some(p) = hypervisor.and_then(|h| r.vm_providers.iter().find(|p| p.provider == h)) {
            let why = p.reason.as_ref().map_or_else(|| "available".into(), crate::runtime::providers::Unavailable::message);
            ready.push((p.provider.id().to_owned(), p.hypervisor == Some(true) && p.available, why));
            if let Some(plugin) = &p.plugin {
                ready.push((format!("plugin:{plugin}"), p.plugin_installed, plugin.clone()));
            }
            if let Some(utility) = p.utility {
                ready.push(("vmware-utility".to_owned(), utility == UtilityState::Running, format!("{utility:?}")));
            }
        }
    }
    ready
}

/// Runs the self-test of `kind` (on `provider` for a VM) as the window does, its steps into its log.
async fn self_test(dirs: &AppDirs, out: &mut Out, kind: Kind, provider: Option<Provider>) {
    let item = match kind {
        Kind::Docker => "selftest:docker",
        Kind::Vm => "selftest:vm",
    };
    let started = Instant::now();
    let path = out.file(item, "log");
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let log = std::sync::Arc::new(std::sync::Mutex::new(Log::create(&path)));
    let events = {
        let log = log.clone();
        move |e: crate::runtime::selftest_report::Event| {
            let text = serde_json::to_string(&e).unwrap_or_default();
            log.lock().unwrap_or_else(std::sync::PoisonError::into_inner).write(&text);
        }
    };
    let result = selftest::machine_selftest(dirs.clone(), kind, provider, None, events).await;
    let seconds = started.elapsed().as_secs();
    match result {
        Ok(()) => out.line(item, Status::Installed, seconds, &format!("passed{}", provider.map(|p| format!(" on {}", p.id())).unwrap_or_default())),
        Err(e) => out.line(item, Status::Failed, seconds, &format!("{e}; see {name}")),
    }
}

/// Where the example lab runs: in containers here, else (VM labs only) in a VM on `hypervisor`.
#[derive(Clone, Copy)]
enum LabPlace {
    Docker,
    Vm(Provider),
}

/// Starts the example lab at `src` as the lab page does, then stops it, in `place`.
async fn example_lab(out: &mut Out, src: &Path, place: LabPlace) {
    let name = src.file_name().map_or_else(|| "lab".to_owned(), |n| n.to_string_lossy().into_owned());
    let item = lab_item(src);
    let started = Instant::now();
    let id = format!("setup-{name}-{}", rand::random::<u16>());
    let dir = install_copy(src, &id);
    let home = std::env::temp_dir().join(format!("cyberctf-{id}-home"));
    let log_path = out.file(&item, "log");
    let log_name = log_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let log = std::cell::RefCell::new(Log::create(&log_path));
    let mut failures = Vec::new();
    scenario(
        &mut failures,
        async |f: &mut Vec<String>| {
            let mut on_line = |l: String| log.borrow_mut().write(&l);
            let up = match place {
                LabPlace::Docker => launch::start_docker_run(&home, &dir, &id, launch::Inputs::default(), &mut on_line).await,
                LabPlace::Vm(provider) => {
                    launch::start_docker_in_vm_run(&home, &dir, &id, launch::VmStart { provider, inputs: launch::Inputs::default() }, &mut on_line).await
                }
            };
            check(up.is_ok(), format!("it starts ({:?})", up.err()), f);
        },
        async |f: &mut Vec<String>| {
            let on_line = |l: String| log.borrow_mut().write(&l);
            let run = match place {
                LabPlace::Docker => isothrum::LabRun::docker(&home, &dir, &id),
                LabPlace::Vm(_) => isothrum::LabRun::docker_in_vm(&home, &dir, &id),
            };
            let stopped = match run {
                Ok(run) => lifecycle::stop_run(run, &dir, on_line).await,
                Err(e) => Err(e),
            };
            check(stopped.is_ok(), format!("it stops ({:?})", stopped.err()), f);
        },
    )
    .await;
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&home);
    let seconds = started.elapsed().as_secs();
    let place = match place {
        LabPlace::Docker => "in containers".to_owned(),
        LabPlace::Vm(p) => format!("in a VM on {}", p.id()),
    };
    if failures.is_empty() {
        out.line(&item, Status::Installed, seconds, &format!("started and stopped {place}"));
    } else {
        out.line(&item, Status::Failed, seconds, &format!("{place}: {}; see {log_name}", failures.join("; ")));
    }
}

/// The first plan's hypervisor, and why each other one wanted isn't set up.
fn report_hypervisors(out: &mut Out, first: &Plan, wanted: &Wanted) {
    for (provider, why) in &first.passed_over {
        out.line(provider.id(), Status::Skipped, 0, why);
    }
    if wanted.paths.contains(&SetupPath::Vm) && first.hypervisor.is_none() {
        let tried: Vec<&str> = wanted.hypervisors.iter().map(|p| p.id()).collect();
        out.line(
            "hypervisor",
            Status::Failed,
            0,
            &format!("none of {} can be set up here", if tried.is_empty() { "the offered ones".into() } else { tried.join(", ") }),
        );
    }
}

#[tokio::test]
#[ignore = "installs real tools on this machine, as an administrator: the test rig's bare guests only, or CYBERCTF_TEST_SETUP_CHECK_ONLY=1 (CYBERCTF_TEST_SETUP_*)"]
async fn the_guided_setup_readies_this_machine() {
    let Some(knobs) = Knobs::from_env() else { return };
    std::fs::create_dir_all(&knobs.logs).unwrap_or_else(|e| panic!("creating {}: {e}", knobs.logs.display()));
    let mut out = Out::new(&knobs);
    let dirs = dirs();

    let before = system::system_check(dirs.clone()).await;
    out.save_report("report-before", &before);
    // The hypervisor this pass sets up: the first wanted one the setup can set up here.
    let first = plan(&before, &before.setup, &knobs.wanted);
    out.hypervisor = first.hypervisor.map_or_else(String::new, |h| h.id().to_owned());
    report_hypervisors(&mut out, &first, &knobs.wanted);
    if knobs.check_only {
        check_only(&mut out, &before, &first, &knobs);
        end_of_pass(out.installed, &out.root);
        assert!(out.failed.is_empty(), "the setup can't ready this machine:\n{}", out.failed.join("\n"));
        return;
    }
    let wanted = Wanted { hypervisors: first.hypervisor.into_iter().collect(), ..knobs.wanted.clone() };
    let items = first.items.clone();

    let (ends, after, acted) = act(&dirs, &mut out, &wanted, before).await;
    if let Some(end) = ends {
        // A restart or a new login: the next pass picks up there.
        out.line(&end.item, Status::Again, 0, if matches!(end.action, Action::Relogin) { "relogin" } else { "reboot" });
        out.save_report("report-after", &after);
        assert!(out.failed.is_empty(), "the setup failed:\n{}", out.failed.join("\n"));
        return;
    }
    for item in items.iter().filter(|i| !acted.contains(i)) {
        out.line(item, Status::Present, 0, "nothing to do");
    }

    let ready = readiness(&after, &knobs.wanted, first.hypervisor);
    for (tool, ok, note) in &ready {
        out.line(&format!("ready:{tool}"), if *ok { Status::Present } else { Status::Failed }, 0, note);
    }
    let is_ready = |tool: &str| ready.iter().any(|(t, ok, _)| t == tool && *ok);
    let docker_ready = knobs.wanted.paths.contains(&SetupPath::Docker) && is_ready("docker");
    let vm_ready = first.hypervisor.filter(|h| is_ready(h.id()) && is_ready("vagrant"));

    if knobs.wanted.paths.contains(&SetupPath::Docker) {
        if docker_ready {
            self_test(&dirs, &mut out, Kind::Docker, None).await;
        } else {
            out.line("selftest:docker", Status::Skipped, 0, "Docker isn't ready");
        }
    }
    if knobs.wanted.paths.contains(&SetupPath::Vm) {
        match vm_ready {
            Some(hypervisor) => self_test(&dirs, &mut out, Kind::Vm, Some(hypervisor)).await,
            None => out.line("selftest:vm", Status::Skipped, 0, "no hypervisor is ready"),
        }
    }
    if let Some(lab) = &knobs.lab {
        match (docker_ready, vm_ready) {
            (true, _) => example_lab(&mut out, lab, LabPlace::Docker).await,
            (false, Some(hypervisor)) => example_lab(&mut out, lab, LabPlace::Vm(hypervisor)).await,
            (false, None) => out.line(&lab_item(lab), Status::Skipped, 0, "nothing to run it on"),
        }
    }

    let after = system::system_check(dirs.clone()).await;
    out.save_report("report-after", &after);
    end_of_pass(out.installed, &out.root);
    assert!(out.failed.is_empty(), "the setup failed:\n{}", out.failed.join("\n"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_items_record_says_its_status_and_the_pass_hypervisor() {
        let knobs = Knobs { wanted: Wanted::default(), results: None, report: None, logs: PathBuf::from("setup-logs"), lab: None, check_only: false };
        let mut out = Out::new(&knobs);
        out.hypervisor = "libvirt".into();
        let r = out.record("docker", Status::Installed, 42, "Docker installed");
        let line = r.line();
        assert!(
            line.starts_with(r#"{"suite":"setup","case":"docker","provider":"libvirt","form":"","result":"pass","reason":"installed: Docker installed","seconds":42,"log":"setup-logs","leaves":["#),
            "{line}"
        );
        // An installed item says what it leaves, on this system, the entry's own folder last.
        assert_eq!(r.leaves, leaves::of("docker", std::env::consts::OS, &setup_root()));
        assert_eq!(r.leaves.last().map(|l| (l.kind.as_str(), l.name.clone())), Some(("temp-folder", setup_root().display().to_string())));
        if std::env::consts::OS == "linux" {
            assert!(r.leaves.iter().any(|l| l.kind == "route" && l.name == "172.17.0.0/16 dev docker0"), "{:?}", r.leaves);
        }
        // Anything else leaves nothing: present already, failed, skipped, a pass that ends.
        for (status, note) in
            [(Status::Present, "29.1"), (Status::Failed, "exited"), (Status::Skipped, "why"), (Status::Again, "reboot"), (Status::CheckOnly, "would install")]
        {
            let r = out.record("docker", status, 0, note);
            assert!(r.leaves.is_empty() && !r.line().contains("leaves"), "{status:?}: {}", r.line());
        }
        assert_eq!(
            (out.record("wsl", Status::Again, 0, "reboot").result, out.record("wsl", Status::Again, 0, "reboot").reason),
            (Verdict::Skip, "again: reboot".into())
        );
        assert_eq!(out.record("vagrant", Status::Failed, 3, "exited with 1:\n\tE: no\r\n").reason, "failed: exited with 1:  E: no");
        assert_eq!(out.record("ready:docker", Status::Present, 0, "29.1").result, Verdict::Pass);
        assert_eq!(out.record("selftest:vm", Status::Skipped, 0, "no hypervisor is ready").result, Verdict::Skip);
    }

    #[test]
    fn a_pass_that_installed_nothing_drops_the_entrys_folder_and_one_that_did_keeps_it() {
        let root = std::env::temp_dir().join(format!("cyberctf-guided-setup-test-{}-{}", std::process::id(), rand::random::<u32>()));
        std::fs::create_dir_all(root.join("data")).unwrap();
        end_of_pass(1, &root);
        assert!(root.is_dir(), "an install's marks stay for the next pass");
        end_of_pass(0, &root);
        assert!(!root.exists(), "nothing installed: nothing of the entry's left");
        end_of_pass(0, &root);
    }

    #[test]
    fn a_line_is_tab_separated_on_one_line() {
        assert_eq!(line("docker", Status::Installed, 42, "Docker installed"), "@@setup\tdocker\tinstalled\t42\tDocker installed");
        assert_eq!(line("wsl", Status::Again, 0, "reboot"), "@@setup\twsl\tagain\t0\treboot");
        assert_eq!(line("vagrant", Status::Failed, 3, "exited with 1:\n\tE: no\r\n"), "@@setup\tvagrant\tfailed\t3\texited with 1:  E: no");
    }

    #[test]
    fn the_knobs_name_the_paths_and_the_hypervisors_in_order() {
        let paths: Vec<SetupPath> = serde_json::from_value(serde_json::json!(["docker", "vm"])).unwrap();
        assert_eq!(paths, [SetupPath::Docker, SetupPath::Vm]);
        assert_eq!(Provider::from_id("vmware_desktop"), Some(Provider::VmwareDesktop));
        assert_eq!(Provider::from_id("libvirt"), Some(Provider::Libvirt));
    }

    /// The records a check-only pass wrote on the machine `report`, for docker and vm on libvirt
    /// with a lab, and every tool call it made.
    fn checked_only(report: &SystemReport) -> (Vec<serde_json::Value>, Vec<String>) {
        let dir = std::env::temp_dir().join(format!("cyberctf-setup-check-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        let knobs = Knobs {
            wanted: Wanted { paths: vec![SetupPath::Docker, SetupPath::Vm], engine: None, hypervisors: vec![Provider::Libvirt, Provider::Virtualbox] },
            results: Some(dir.join("setup.jsonl")),
            report: Some(dir.join("setup.report")),
            logs: dir.clone(),
            lab: Some(PathBuf::from("examples").join("hello-stack")),
            check_only: true,
        };
        // Every tool, installer and command runner answers from here, and says it was called.
        let tools = crate::platform::exec::fake::Tools::new(|_, _| None);
        let mut out = Out::new(&knobs);
        let first = plan(report, &report.setup, &knobs.wanted);
        report_hypervisors(&mut out, &first, &knobs.wanted);
        check_only(&mut out, report, &first, &knobs);
        let records = std::fs::read_to_string(dir.join("setup.jsonl")).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        let lines = std::fs::read_to_string(dir.join("setup.report")).unwrap();
        assert_eq!(lines.lines().count(), std::fs::read_to_string(dir.join("setup.jsonl")).unwrap().lines().count(), "the @@setup lines say the same items");
        std::fs::remove_dir_all(&dir).ok();
        (records, tools.calls())
    }

    #[test]
    fn a_check_only_pass_records_what_the_setup_would_do_and_runs_nothing() {
        for (name, report) in crate::machine::setup::shapes::all() {
            let (records, calls) = checked_only(&report);
            assert_eq!(calls, [] as [String; 0], "{name}: no installer, engine, self-test, lab or other tool runs");
            assert!(!records.is_empty(), "{name}");
            for r in &records {
                let (result, reason) = (r["result"].as_str().unwrap(), r["reason"].as_str().unwrap());
                let done = matches!((result, reason.split(':').next().unwrap()), ("pass", "present") | ("fail", "failed") | ("skip", "skipped" | "check-only"));
                assert!(done, "{name}: {r}");
                assert!(!reason.starts_with("installed") && !reason.starts_with("again"), "{name}: nothing is done in check-only: {r}");
                if reason.starts_with("check-only:") {
                    assert!(reason.starts_with("check-only: would "), "{name}: {r}");
                }
            }
        }
    }

    #[test]
    fn a_check_only_pass_on_a_bare_debian_says_each_install_it_would_make() {
        let (records, _) = checked_only(&crate::machine::setup::shapes::bare_debian());
        let reason = |case: &str| {
            records.iter().find(|r| r["case"] == case).map(|r| (r["result"].as_str().unwrap().to_owned(), r["reason"].as_str().unwrap().to_owned()))
        };
        assert_eq!(reason("docker").unwrap().0, "skip");
        assert!(reason("docker").unwrap().1.starts_with("check-only: would install"), "{:?}", reason("docker"));
        assert_eq!(reason("vagrant").unwrap().1, format!("check-only: would install {}", crate::machine::tools::install::Dependency::Vagrant.name()));
        assert!(reason("ready:docker").unwrap().1.starts_with("check-only: would be ready once set up"), "{:?}", reason("ready:docker"));
        assert_eq!(reason("selftest:docker").unwrap(), ("skip".to_owned(), "check-only: would run the Docker self-test".to_owned()));
        assert_eq!(reason("lab:hello-stack").unwrap(), ("skip".to_owned(), "check-only: would start and stop it in containers".to_owned()));
    }

    #[test]
    fn readiness_names_each_wanted_tool_as_the_rig_reads_it() {
        let wanted = Wanted { paths: vec![SetupPath::Docker, SetupPath::Vm], engine: None, hypervisors: vec![Provider::Libvirt] };
        let r = crate::machine::setup::shapes::debian_libvirt_without_plugin();
        let ready: Vec<(String, bool)> = readiness(&r, &wanted, Some(Provider::Libvirt)).into_iter().map(|(t, ok, _)| (t, ok)).collect();
        assert_eq!(
            ready,
            [
                ("docker".to_owned(), false),
                ("compose".to_owned(), false),
                ("vagrant".to_owned(), true),
                ("libvirt".to_owned(), false),
                ("plugin:vagrant-libvirt".to_owned(), false),
            ]
        );
        let r = crate::machine::setup::shapes::bare_windows();
        assert_eq!(readiness(&r, &Wanted { paths: vec![SetupPath::Docker], ..Wanted::default() }, None)[0].0, "wsl");
    }
}
