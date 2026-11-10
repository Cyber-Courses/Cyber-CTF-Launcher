//! The launcher's guided machine setup on this machine, end to end and without a window: what the
//! setup window would install, start or restart, in its order ([`crate::machine::setup::plan`],
//! the plan the window renders), run through the functions the window's commands call, then the
//! self-tests and an example lab. The test rig's `setup` suite runs it, pass after pass, on a bare
//! Windows or Linux guest (`rig/README.md`, "The setup suite", in the launcher's workspace).
//!
//! It installs real tools on this machine, as an administrator: never run it on a machine of
//! yours. It returns at once without its knobs:
//!
//! - `CYBERCTF_TEST_SETUP_PATHS`: the lab types to set up, `docker,vm`;
//! - `CYBERCTF_TEST_SETUP_HYPERVISORS`: the hypervisors to try, in order (`libvirt,virtualbox`):
//!   the first the setup can set up is set up, each other one reported (`skipped` and why);
//! - `CYBERCTF_TEST_SETUP_REPORT`: the file each item's line is appended to, as it goes (a crash
//!   still leaves what was done);
//! - `CYBERCTF_TEST_SETUP_LOGS`: a folder for each action's log, and the machine reports before,
//!   after each action, and after;
//! - `CYBERCTF_TEST_SETUP_LAB`: an Isoloom example lab, started and stopped once the machine is
//!   ready (optional).
//!
//! A line per item: `@@setup<TAB>item<TAB>status<TAB>seconds<TAB>note`, the status `installed`
//! (done in this pass), `present` (there already, or ready), `failed` (the note names its log),
//! `skipped` (the note says why) or `again` (note `reboot` or `relogin`: the pass ends there,
//! passing, and the next one picks up after the restart or the new login). The items: `wsl`,
//! `docker`, `engine:<id>`, `vagrant`, `<hypervisor>`, `plugin:<name>`, `vmware-utility`, then
//! `ready:<tool>`, `selftest:docker`, `selftest:vm`, `lab:<name>`.
//!
//! It never waits on a prompt of its own: the installs elevate the way the app does (UAC through
//! winget and PowerShell, pkexec on Linux), which the rig answers (an elevated scheduled task, a
//! polkit rule); without pkexec, an install's line says so. Built once with the library's tests,
//! it runs from that test binary on a machine without the sources:
//!
//! ```text
//! CYBERCTF_TEST_SETUP_PATHS=docker,vm CYBERCTF_TEST_SETUP_HYPERVISORS=libvirt,virtualbox \
//! CYBERCTF_TEST_SETUP_REPORT=setup.tsv CYBERCTF_TEST_SETUP_LOGS=setup-logs \
//! CYBERCTF_TEST_SETUP_LAB=<isoloom checkout>/examples/hello-stack \
//! cargo test --lib -- --ignored --exact integration_tests::setup::the_guided_setup_readies_this_machine --nocapture --test-threads=1
//! ```

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::support::{check, install_copy, scenario};
use crate::machine::selftest::{self, Kind};
use crate::machine::setup::{Action, ManualReason, Plan, SetupPath, Step, Wanted, plan};
use crate::machine::system::{self, SystemReport, docker};
use crate::machine::tools::install;
use crate::platform::AppDirs;
use crate::runtime::providers::{Provider, UtilityState};
use crate::runtime::{isothrum, launch, lifecycle};

/// The test's knobs.
struct Knobs {
    wanted: Wanted,
    report: PathBuf,
    logs: PathBuf,
    lab: Option<PathBuf>,
}

impl Knobs {
    /// The knobs from the environment; none without the paths, the report and the logs.
    fn from_env() -> Option<Knobs> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let paths = var("CYBERCTF_TEST_SETUP_PATHS")?;
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
            report: PathBuf::from(var("CYBERCTF_TEST_SETUP_REPORT")?),
            logs: PathBuf::from(var("CYBERCTF_TEST_SETUP_LOGS")?),
            lab: var("CYBERCTF_TEST_SETUP_LAB").map(PathBuf::from),
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
}

impl Status {
    fn word(self) -> &'static str {
        match self {
            Status::Installed => "installed",
            Status::Present => "present",
            Status::Failed => "failed",
            Status::Skipped => "skipped",
            Status::Again => "again",
        }
    }
}

/// One item's line: `@@setup<TAB>item<TAB>status<TAB>seconds<TAB>note`, the note on one line.
fn line(item: &str, status: Status, seconds: u64, note: &str) -> String {
    let note: String = note.chars().map(|c| if matches!(c, '\t' | '\r' | '\n') { ' ' } else { c }).collect();
    format!("@@setup\t{item}\t{}\t{seconds}\t{}", status.word(), note.trim())
}

/// Where the pass writes: its report, line by line, and its logs folder.
struct Out {
    report: PathBuf,
    logs: PathBuf,
    /// The items that failed, for the test's verdict.
    failed: Vec<String>,
    /// How many logs and reports were written, for their order in the folder.
    written: usize,
}

impl Out {
    /// Appends an item's line to the report (and prints it), at once: a crash keeps it.
    fn line(&mut self, item: &str, status: Status, seconds: u64, note: &str) {
        let text = line(item, status, seconds, note);
        println!("{text}");
        let appended = std::fs::OpenOptions::new().create(true).append(true).open(&self.report).and_then(|mut f| writeln!(f, "{text}"));
        if let Err(e) = appended {
            panic!("appending to {}: {e}", self.report.display());
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

/// The app's folders for the pass: kept from pass to pass (the WSL and Hyper-V marks, what the
/// setup installed), as a player's app keeps them.
fn dirs() -> AppDirs {
    let root = std::env::temp_dir().join("cyberctf-guided-setup");
    let (data, cache) = (root.join("data"), root.join("cache"));
    for dir in [&data, &cache] {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("creating {}: {e}", dir.display()));
    }
    AppDirs::new(Some(data), Some(cache))
}

/// What `step` does, by the function the window's command for it calls, into `log`: its status
/// and note.
async fn run_step(dirs: &AppDirs, step: &Step, log: &mut Log) -> (Status, String) {
    let mut on_line = |l: String| log.write(&l);
    let done = match &step.action {
        Action::Install { dependency } => {
            install::install_dependency(dirs.clone(), *dependency, &mut on_line).await.map(|()| format!("{} installed", dependency.name()))
        }
        Action::Plugin { plugin } => install::install_vagrant_plugin(plugin.clone(), &mut on_line).await.map(|()| format!("{plugin} installed")),
        Action::StartEngine { engine } => docker::docker_start_engine(engine.id().to_owned()).await.map(|()| format!("{} started", engine.id())),
        Action::UseEngine { engine } => docker::docker_use_engine(engine.id().to_owned()).await.map(|()| format!("Docker uses {}", engine.id())),
        Action::Manual { reason: ManualReason::Distribution, dependency, .. } => {
            let what = dependency.map_or("it", |d| d.name());
            return (Status::Skipped, format!("this distribution isn't supported by the guided setup yet: install {what} yourself"));
        }
        Action::Manual { reason: ManualReason::Vendor, url, .. } => {
            return (Status::Skipped, format!("links out: {}", url.as_deref().unwrap_or("its vendor's site")));
        }
        Action::Blocked { reason } => return (Status::Failed, format!("can't run here: {reason}")),
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
            let why = p.reason.clone().unwrap_or_else(|| "available".into());
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
    let item = format!("lab:{name}");
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
#[ignore = "installs real tools on this machine, as an administrator: the test rig's bare guests only (CYBERCTF_TEST_SETUP_*)"]
async fn the_guided_setup_readies_this_machine() {
    let Some(knobs) = Knobs::from_env() else { return };
    std::fs::create_dir_all(&knobs.logs).unwrap_or_else(|e| panic!("creating {}: {e}", knobs.logs.display()));
    let mut out = Out { report: knobs.report.clone(), logs: knobs.logs.clone(), failed: Vec::new(), written: 0 };
    let dirs = dirs();

    let before = system::system_check(dirs.clone()).await;
    out.save_report("report-before", &before);
    // The hypervisor this pass sets up: the first wanted one the setup can set up here.
    let first = plan(&before, &before.setup, &knobs.wanted);
    report_hypervisors(&mut out, &first, &knobs.wanted);
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
            (false, None) => out.line(
                &format!("lab:{}", lab.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())),
                Status::Skipped,
                0,
                "nothing to run it on",
            ),
        }
    }

    let after = system::system_check(dirs.clone()).await;
    out.save_report("report-after", &after);
    assert!(out.failed.is_empty(), "the setup failed:\n{}", out.failed.join("\n"));
}

#[cfg(test)]
mod tests {
    use super::*;

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
