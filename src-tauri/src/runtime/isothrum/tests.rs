//! The launcher's runs through the runner, end to end against a scripted Compose engine (the
//! runner's fake processes) and scripted launcher tools (the attack box's `docker` calls): no real
//! tool runs. The lab's run manifest is the one Isoloom generates from the spec.

use isothrum_runner::{EventKind, ReadyState, StepState, WarningCode};

use super::test_lab::prelude::*;
use super::*;
use crate::error::Error;
use crate::platform::exec::fake::Tools;

const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\ninputs: [CTF_LAUNCH_TOKEN]\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http, publish: 8080 } }\n    files: [{ path: /run/lab/token, input: CTF_LAUNCH_TOKEN }]\n    app: { image: nginx:1.27 }\nchecks:\n  - { http: \"http://web/\" }\nmessage: |\n  Open http://localhost:{{ machines.web.ports.web.published }}/\n";

/// Where the fake project's containers are, and the host port its web port got.
#[derive(Debug, Default)]
struct Engine {
    containers: Containers,
    host_port: u16,
    /// `compose up` fails (Compose's own error) from now on.
    failing_up: bool,
    /// `compose pull` fails (the registry's refusal) from now on.
    failing_pull: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Containers {
    #[default]
    None,
    Up,
    Stopped,
}

/// An installed lab (`<labs>/web`) with its files generated, a runner home, and the fake engine.
type Fixture = Scripted<Engine>;

impl Fixture {
    fn new() -> Fixture {
        Scripted::answered_by(Home::new("", SPEC, &[], lab::Target::Docker), answer)
    }

    /// The lab's run, on a runner whose home is the fixture's root.
    fn run(&self) -> LabRun {
        LabRun::with(self.home.config(&self.fake, [41873]), &self.home.dir, "web", Kind::Docker).unwrap()
    }

    /// The lab's run, started with [`launch_env`] (the attack box's tools scripted by the caller).
    async fn up(&self) -> LabRun {
        let run = self.run();
        run.up(on_own_ports(&launch_env()), None, &mut |_| {}).await.unwrap();
        run
    }

    fn set(&self, containers: Containers) {
        self.model().containers = containers;
    }
}

/// The fake Compose engine's answer to `call`.
fn answer(engine: &Mutex<Engine>, call: &Call) -> Reply {
    let line = call.line();
    let mut e = locked(engine);
    if line.contains(" ps --all --format json") || line.contains(" ps --format json") {
        return Reply::ok(&ps(e.containers, line.contains("--all"), e.host_port));
    }
    if line.contains(" config --format json") {
        return Reply::ok(
            r#"{"services":{"web":{"ports":[{"target":80,"host_ip":"127.0.0.1","published":"","protocol":"tcp"}]}},"networks":{"lab":{"name":"cyberctf-web_lab"}}}"#,
        );
    }
    if line.contains(" pull --policy ") && e.failing_pull {
        return Reply::fail(18, "Error response from daemon: toomanyrequests: You have reached your pull rate limit");
    }
    if line.contains(" up --detach ") {
        if e.failing_up {
            return Reply::fail(1, "Error response from daemon: pull access denied");
        }
        e.containers = Containers::Up;
        e.host_port = if call.env("ISOLOOM_PUBLISH_FIXED").is_some() { 8080 } else { 41873 };
        return Reply::ok(" Container cyberctf-web-web-1  Started");
    }
    if line.contains("--profile check run") {
        return Reply::ok("isoloom-check: PASS http://web/\nisoloom-check: END 1 passed, 0 failed");
    }
    if line.starts_with("docker ps -aq") {
        return Reply::ok(if e.containers == Containers::None { "" } else { "c1" });
    }
    e.containers = match e.containers {
        Containers::Up if line.ends_with(" stop") => Containers::Stopped,
        Containers::Stopped if line.ends_with(" start") => Containers::Up,
        _ if line.contains(" down --volumes") => Containers::None,
        now => now,
    };
    Reply::ok("")
}

/// `compose ps --format json` for the lab's web machine and Isoloom's probe.
fn ps(containers: Containers, all: bool, host_port: u16) -> String {
    let state = match containers {
        Containers::None => return String::new(),
        Containers::Stopped if !all => return String::new(),
        Containers::Up => "running",
        Containers::Stopped => "exited",
    };
    let publisher = format!(r#"{{"URL":"127.0.0.1","PublishedPort":{host_port},"TargetPort":80,"Protocol":"tcp"}}"#);
    [("web", publisher.as_str()), ("isoloom-probe-amd64", "")]
        .iter()
        .map(|(service, publishers)| format!(r#"{{"Service":"{service}","State":"{state}","Health":"","ExitCode":0,"Publishers":[{publishers}]}}"#))
        .collect::<Vec<_>>()
        .join("\n")
}

fn launch_env() -> Vec<(String, String)> {
    vec![("CTF_LAUNCH_TOKEN".into(), "t0k".into())]
}

/// The launch's `values`, on the lab's own host ports (the player's choice).
fn on_own_ports(values: &[(String, String)]) -> Inputs<'_> {
    Inputs { values, default_ports: true }
}

/// A fixture whose lab is up and then parked with `Park::Shutdown`, the launcher tools scripted
/// empty — the shared start of the park/resume tests. The caller keeps the `Fixture` and the
/// `Tools` guard, which hold the lab's folder and the fake tools for the rest of the test.
async fn up_then_parked() -> (Fixture, LabRun, Tools) {
    let f = Fixture::new();
    let tools = Tools::rules(&[]);
    let run = f.up().await;
    run.park(Park::Shutdown, &mut |_| {}).await.unwrap();
    (f, run, tools)
}

#[tokio::test]
async fn a_container_lab_goes_up_through_the_runner_as_cyberctf_lab_id() {
    let f = Fixture::new();
    let _tools = Tools::rules(&[]);
    let mut log = Vec::new();
    f.run().up(on_own_ports(&launch_env()), None, &mut |l| log.push(l)).await.unwrap();
    let up = f.fake.last(" up --detach").unwrap();
    assert!(up.line().contains("--project-name cyberctf-web"), "{}", up.line());
    assert_eq!(up.env("CTF_LAUNCH_TOKEN"), Some("t0k"), "the launch's input reaches its slot");
    assert_eq!(up.env("ISOLOOM_PUBLISH_FIXED"), Some("1"), "the player chose the lab's own ports");
    assert!(log.iter().any(|l| l == "Starting the environment's containers…"), "{log:?}");
    assert_eq!(log.last().map(String::as_str), Some("Open http://localhost:8080/"), "the lab's message, its port filled");
    assert!(f.home.root.join("runs/cyberctf-web/record.json").is_file(), "the run is kept in the app data's runs/");
    assert!(!f.home.dir.join(".cyberctf-ports.yml").exists(), "the runner pins ports in its own state, not the lab folder");
}

#[tokio::test]
async fn the_lab_page_reads_the_runners_status() {
    let f = Fixture::new();
    let _tools = Tools::rules(&[]);
    let run = f.up().await;
    let s = run.status().await.unwrap();
    assert!(s.running && s.parked.is_none());
    assert!(matches!(s.place, Some(Place::Container)));
    assert_eq!(s.url.as_deref(), Some("http://127.0.0.1:8080/"));
    let web = s.machines.iter().find(|m| m.name == "web").unwrap();
    assert_eq!((web.state.as_str(), web.ip.as_str(), web.infra), ("running", "10.30.0.10", false));
    assert_eq!(web.ports.iter().map(|p| (p.target, p.published)).collect::<Vec<_>>(), [(80, 8080)]);
    assert_eq!(web.services.iter().map(|s| (s.name.as_str(), s.kind.as_str(), s.ports.clone())).collect::<Vec<_>>(), [("web", "web", vec![80])]);
    assert!(s.machines.iter().find(|m| m.name == "isoloom-probe-amd64").is_some_and(|m| m.infra), "Isoloom's probe is plumbing");
    assert!(s.machines.iter().all(|m| m.name != "isoloom-check"), "a check runner that never ran isn't listed");
    assert_eq!(s.networks.iter().map(|n| (n.name.as_str(), n.subnet.as_str(), n.internal)).collect::<Vec<_>>(), [("lab", "10.30.0.0/24", false)]);
}

#[tokio::test]
async fn a_lab_parks_and_resumes() {
    let (f, run, _tools) = up_then_parked().await;
    assert!(f.fake.last("compose").is_some_and(|c| c.line().ends_with(" stop")));
    let parked = run.status().await.unwrap();
    assert_eq!((parked.running, parked.parked), (false, Some(Park::Shutdown)));
    paths::mark_parked(&f.home.dir, Some(Park::Pause)).unwrap();
    assert_eq!(run.status().await.unwrap().parked, Some(Park::Pause), "as the player chose it");
    run.resume(&mut |_| {}).await.unwrap();
    assert!(run.status().await.unwrap().running);
}

/// A resume has no launch values (they aren't kept), so each input slot gets its default, empty,
/// which Compose reads as "keep the file the container was created with": the player's value
/// survives a park, and nothing else is ever written in its place.
#[tokio::test]
async fn a_resumed_lab_keeps_the_files_its_inputs_were_written_to() {
    let f = Fixture::new();
    let _tools = Tools::rules(&[]);
    let run = f.up().await;
    assert_eq!(f.fake.last(" up --detach").and_then(|c| c.env("CTF_LAUNCH_TOKEN").map(str::to_owned)).as_deref(), Some("t0k"));
    run.park(Park::Shutdown, &mut |_| {}).await.unwrap();
    run.resume(&mut |_| {}).await.unwrap();
    let start = f.fake.last(" start").unwrap();
    assert!(start.line().ends_with(" start"), "{}", start.line());
    assert_eq!(start.env("CTF_LAUNCH_TOKEN"), Some(""), "the slot's default, which keeps the file");
}

/// Regression (tests/resilience.md M4): a container stopped outside the app (`docker stop`,
/// Docker Desktop's Stop) while its lab ran could only be reset; a resume now starts it again
/// with Compose, as it is, and the lab reads running.
#[tokio::test]
async fn a_container_stopped_outside_the_app_is_resumed_as_it_is() {
    let f = Fixture::new();
    let _tools = Tools::rules(&[]);
    let run = f.up().await;
    f.set(Containers::Stopped);
    let down = run.status().await.unwrap();
    let web = down.machines.iter().find(|m| m.name == "web").unwrap();
    assert_ne!(web.state, "running");
    run.resume(&mut |_| {}).await.unwrap();
    assert!(f.fake.last(" start").is_some_and(|c| c.line().ends_with(" start")), "Compose starts the stopped services");
    assert_eq!(f.fake.calls().iter().filter(|c| c.line().contains(" up --detach")).count(), 1, "nothing is rebuilt");
    assert!(run.status().await.unwrap().running);
}

#[tokio::test]
async fn a_parked_lab_whose_containers_are_gone_offers_a_fresh_start() {
    let (f, run, _tools) = up_then_parked().await;
    paths::mark_parked(&f.home.dir, Some(Park::Shutdown)).unwrap();
    f.set(Containers::None);
    let err = run.resume(&mut |_| {}).await.unwrap_err().to_string();
    assert!(err.starts_with("Nothing is left of this lab to resume"), "{err}");
    assert_eq!(paths::parked(&f.home.dir), None, "the parked mark goes");
}

#[tokio::test]
async fn a_second_start_of_a_running_lab_is_refused_and_a_down_removes_it_all() {
    let f = Fixture::new();
    let _tools = Tools::rules(&[]);
    let run = f.up().await;
    let again = run.up(on_own_ports(&launch_env()), None, &mut |_| {}).await.unwrap_err();
    assert!(matches!(&again, Error::Refused(m) if m.starts_with("This lab is already running on this machine")), "{again}");
    run.down(&mut |_| {}).await.unwrap();
    assert!(f.fake.last(" down --volumes").is_some());
    assert!(!run.status().await.unwrap().running);
}

#[tokio::test]
async fn the_labs_checks_run_through_the_runner() {
    let f = Fixture::new();
    let _tools = Tools::rules(&[]);
    let run = f.run();
    let not_up = run.check().await.unwrap_err().to_string();
    assert!(not_up.contains("isn't running"), "{not_up}");
    run.up(on_own_ports(&launch_env()), None, &mut |_| {}).await.unwrap();
    let c = run.check().await.unwrap();
    assert!(c.available && c.ok, "{}", c.output);
    assert_eq!(c.results.iter().map(|r| (r.name.as_str(), r.ok)).collect::<Vec<_>>(), [("http://web/", true)]);
    assert!(c.output.contains("isoloom-check: END 1 passed, 0 failed"), "{}", c.output);
}

#[tokio::test]
async fn a_lab_never_generated_reads_as_not_running() {
    let root = std::env::temp_dir().join(format!("cyberctf-isothrum-{}", rand::random::<u32>()));
    let dir = root.join("labs").join("web");
    std::fs::create_dir_all(&dir).unwrap();
    let s = LabRun::with(RunnerConfig::new(&root), &dir, "web", Kind::Docker).unwrap().status().await.unwrap();
    assert!(!s.running && s.machines.is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn tearing_down_a_lab_never_generated_is_a_clean_no_op() {
    // A start that fails before generation (for example, a lab the bundled engine can't read)
    // leaves no run manifest. Stopping it must not error on the missing run.json: it created
    // nothing, so there is nothing to remove, as the launcher's own Docker path tolerates.
    let root = std::env::temp_dir().join(format!("cyberctf-isothrum-{}", rand::random::<u32>()));
    let dir = root.join("labs").join("web");
    std::fs::create_dir_all(&dir).unwrap();
    let run = LabRun::with(RunnerConfig::new(&root), &dir, "web", Kind::Docker).unwrap();
    let mut lines = Vec::new();
    run.down(&mut |l| lines.push(l)).await.unwrap();
    assert_eq!(lines, ["Nothing to remove: the run has no files."]);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_deploy_log_shows_steps_tool_lines_and_warnings_only() {
    let step = |state, note: Option<&str>| EventKind::Step { step: "up".into(), state, note: note.map(Into::into) };
    assert_eq!(progress::line(&step(StepState::Running, Some("Starting the lab's containers…"))).as_deref(), Some("Starting the lab's containers…"));
    assert_eq!(progress::line(&step(StepState::Running, None)), None, "a step without its line");
    assert_eq!(progress::line(&step(StepState::Done, None)), None);
    let log = EventKind::Log { step: "up".into(), stream: isothrum_runner::Stream::Stderr, line: " Container web  Healthy".into() };
    assert_eq!(progress::line(&log).as_deref(), Some(" Container web  Healthy"));
    let waiting = EventKind::Ready { condition: "tcp 127.0.0.1:8080".into(), state: ReadyState::Waiting, detail: "no answer yet".into() };
    assert_eq!(progress::line(&waiting).as_deref(), Some("Waiting for the lab to be ready: no answer yet"));
    let warning = |code: WarningCode, message: &str| progress::line(&EventKind::Warning { code, message: message.into() });
    assert_eq!(warning(WarningCode::LeftoversRemoved, "removing what an earlier run left").as_deref(), Some("Clearing a previous, stopped run…"));
    assert_eq!(warning(WarningCode::StepTolerated, "x"), None);
    assert_eq!(warning(WarningCode::OrphanRemoved, "Removing a VM an earlier run left").as_deref(), Some("Removing a VM an earlier run left"));
    assert_eq!(progress::line(&EventKind::Result { ok: true, data: serde_json::json!({}) }), None, "the caller says how it ended");
}

#[test]
fn a_start_gives_the_launch_values_to_the_slots_and_resumes_a_parked_run() {
    let values = [("CTF_FLAG_MAIN".to_owned(), "flag{x}".to_owned())];
    let opts = start_options(Inputs::values(&values), None);
    assert_eq!(opts.secrets.get("CTF_FLAG_MAIN").map(SecretValue::expose), Some("flag{x}"));
    assert_eq!(opts.secrets.iter().count(), 1, "the launch's values only");
    assert!(opts.params.is_empty(), "random ports: no fixed publish");
    assert_eq!(opts.occupied, Occupied::ResumeParked);
    let fixed = start_options(Inputs { values: &[], default_ports: true }, None);
    assert_eq!(fixed.params.get("ISOLOOM_PUBLISH_FIXED").map(String::as_str), Some("1"));
    assert_eq!(fixed.secrets.iter().count(), 0, "the ports choice is no lab input");
    assert_eq!(opts.provider, None);
    let hypervisor = runner_provider("virtualbox").unwrap();
    assert_eq!(start_options(Inputs::default(), Some(hypervisor)).provider, Some(isoloom_manifest::Provider::Virtualbox), "a VM lab's hypervisor");
    // Every Vagrant provider the launcher starts a lab on has the runner's name (Proxmox is a
    // target of its own, never one); anything else is refused.
    for p in crate::runtime::providers::Provider::ALL.into_iter().filter(|p| *p != crate::runtime::providers::Provider::Proxmox) {
        assert!(runner_provider(p.id()).is_ok(), "{}", p.id());
    }
    assert!(runner_provider("nonsense").is_err());
}

/// The lab's attack box as the launcher places it on this machine: a container added to the
/// lab, the run brought up to date with it.
fn placed_box(f: &Fixture) -> crate::runtime::attack_machine::Placed {
    use crate::runtime::attack_machine::Placed;
    use crate::runtime::host::Host;
    Placed { host: Host::Local(f.run()), dir: f.home.dir.clone(), target: lab::Target::Docker, vm: false }
}

#[tokio::test]
async fn an_attack_box_refused_by_a_lab_that_is_not_up_leaves_the_lab_as_published() {
    let f = Fixture::new();
    let _tools = Tools::rules(&[]);
    let dirs = crate::platform::AppDirs::new(Some(f.home.root.clone()), None);
    let compose = std::fs::read_to_string(lab::compose_file(&f.home.dir)).unwrap();
    let err = placed_box(&f)
        .add(&dirs, crate::runtime::attack_machine::Pick { image: "kalilinux/kali-rolling", desktop: false }, None, &mut |_| {})
        .await
        .unwrap_err();
    assert!(matches!(&err, crate::error::Error::RefusedBeforeActing(m) if m == "This lab isn't running. Start it first."), "{err}");
    assert!(!crate::runtime::attack_machine::added(&f.home.dir).unwrap(), "the box is out of the lab's spec again");
    assert_eq!(std::fs::read_to_string(lab::compose_file(&f.home.dir)).unwrap(), compose, "and its files as they were");
}

#[tokio::test]
async fn an_attack_box_whose_start_failed_in_the_run_stays_in_the_lab_for_stop_to_take_out() {
    let f = Fixture::new();
    let _tools = Tools::rules(&[]);
    let dirs = crate::platform::AppDirs::new(Some(f.home.root.clone()), None);
    f.up().await;
    f.model().failing_up = true;
    let placed = placed_box(&f);
    assert!(placed.add(&dirs, crate::runtime::attack_machine::Pick { image: "kalilinux/kali-rolling", desktop: false }, None, &mut |_| {}).await.is_err());
    assert!(crate::runtime::attack_machine::added(&f.home.dir).unwrap(), "the run may hold some of it: kept, for a stop to retire");
    f.model().failing_up = false;
    placed.remove(&dirs, &mut |_| {}).await.unwrap();
    assert!(!crate::runtime::attack_machine::added(&f.home.dir).unwrap());
}

#[tokio::test]
async fn an_attack_box_whose_image_cant_be_downloaded_says_so_after_its_pull() {
    // Regression: the box's image downloading inside the start's `up` was read as the lab's `up`
    // running out of time, and the player was told to check Docker's resources.
    let f = Fixture::new();
    let _tools = Tools::rules(&[]);
    let dirs = crate::platform::AppDirs::new(Some(f.home.root.clone()), None);
    f.up().await;
    f.model().failing_pull = true;
    let ups = || f.fake.calls().iter().filter(|c| c.line().contains(" up --detach ")).count();
    let before = ups();
    let mut lines = Vec::new();
    let err = placed_box(&f)
        .add(&dirs, crate::runtime::attack_machine::Pick { image: "kalilinux/kali-rolling", desktop: false }, None, &mut |l| lines.push(l))
        .await
        .unwrap_err();
    assert_eq!(
        err.code(),
        crate::error::Code::DownloadFailed {
            reason: "Error response from daemon: toomanyrequests: You have reached your pull rate limit (exited with code 18)".into()
        }
    );
    assert!(err.to_string().starts_with("Couldn't download the lab's images: Error response from daemon: toomanyrequests"), "{err}");
    // The pull is its own step in the deploy log, before the containers' (which never ran).
    assert!(lines.iter().any(|l| l == "Pulling the environment's images…"), "{lines:#?}");
    assert_eq!(ups(), before, "nothing started after the failed pull");
}

#[tokio::test]
async fn an_attack_box_goes_in_and_out_of_the_run_with_the_labs_files() {
    let f = Fixture::new();
    let _tools = Tools::rules(&[]);
    let dirs = crate::platform::AppDirs::new(Some(f.home.root.clone()), None);
    f.up().await;
    let placed = placed_box(&f);
    let compose = || std::fs::read_to_string(lab::compose_file(&f.home.dir)).unwrap();
    let updates = || f.fake.calls().iter().filter(|c| c.line().contains(" up --detach ")).count();
    let before = updates();
    placed.add(&dirs, crate::runtime::attack_machine::Pick { image: "kalilinux/kali-rolling", desktop: false }, None, &mut |_| {}).await.unwrap();
    // The lab's files regenerated with the box, then the run brought up to date with them.
    assert!(compose().contains("attackbox") && compose().contains("kalilinux/kali-rolling"));
    assert_eq!(updates(), before + 1);
    placed.remove(&dirs, &mut |_| {}).await.unwrap();
    // Out of the files, and the run brought up to date again (Compose drops the orphan).
    assert!(!compose().contains("attackbox"));
    assert_eq!(updates(), before + 2);
    assert!(!crate::runtime::attack_machine::added(&f.home.dir).unwrap());
}

#[test]
fn a_lab_here_is_run_in_the_form_it_was_started_in() {
    let dir = std::env::temp_dir().join(format!("cyberctf-local-form-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    let home = dir.join("home");
    let form = |runtime| LabRun::local(&home, &dir, "t", runtime).unwrap().map(|r| r.target());
    // As declared: its containers on Docker, its VMs, and no run here for cloud services.
    assert_eq!(form(Runtime::Docker), Some(lab::Target::Docker));
    assert_eq!(form(Runtime::Vm), Some(lab::Target::Vagrant));
    assert_eq!(form(Runtime::Cloud), None);
    // A container lab started in one local VM, then as one VM per machine (which wins).
    paths::mark_local_vm(&dir, Some(crate::runtime::providers::Provider::Qemu)).unwrap();
    assert_eq!(form(Runtime::Docker), Some(lab::Target::DockerVm));
    paths::mark_per_machine_vm(&dir, true).unwrap();
    assert_eq!(form(Runtime::Docker), Some(lab::Target::Vagrant));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The lab's run manifest at `manifest` as an earlier launcher left it: the version its Isoloom
/// wrote (0, every 1.x's).
fn as_written_before(manifest: &std::path::Path) {
    let mut written: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(manifest).unwrap()).unwrap();
    written["manifest_version"] = 0.into();
    std::fs::write(manifest, written.to_string()).unwrap();
}

/// A lab left running across an update of the launcher: its files, generated by the earlier
/// version, are generated again before its status is read or it is stopped, as its instance
/// (`.isoloom-<n>/`) with the launcher's own overrides (its Compose project).
#[tokio::test]
async fn a_lab_generated_by_an_earlier_version_is_generated_again_then_read_and_stopped() {
    let home = Home::new("-earlier", SPEC, &[(lab::INSTANCE_MARKER, "3")], lab::Target::Docker);
    let f: Fixture = Scripted::answered_by(home, answer);
    let _tools = Tools::rules(&[]);
    f.up().await;
    let manifest = lab::docker_manifest(&f.home.dir);
    assert!(manifest.starts_with(f.home.dir.join(".isoloom-3")), "{}", manifest.display());
    let generated = std::fs::read_to_string(&manifest).unwrap();
    as_written_before(&manifest);
    let compose = lab::compose_file(&f.home.dir);
    let project = std::fs::read_to_string(&compose).unwrap();
    assert!(project.contains("name: cyberctf-web"), "{project}");
    std::fs::write(&compose, project.replace("name: cyberctf-web", "name: t-3")).unwrap();
    let run = f.run();
    let s = run.status().await.unwrap();
    assert!(s.running, "the lab reads as running");
    assert_eq!(std::fs::read_to_string(&manifest).unwrap(), generated, "generated again as this version writes it");
    assert_eq!(std::fs::read_to_string(&compose).unwrap(), project, "with the launcher's Compose project");
    as_written_before(&manifest);
    run.down(&mut |_| {}).await.unwrap();
    let down = f.fake.last(" down --volumes").unwrap();
    assert!(down.line().contains("--project-name cyberctf-web"), "{}", down.line());
    assert!(!run.status().await.unwrap().running);
}

/// A lab generated by an earlier version whose spec is gone can't be generated again: an
/// operation says what to do, not the runner's refusal of the old manifest.
#[tokio::test]
async fn a_lab_generated_by_an_earlier_version_that_cant_be_generated_again_says_what_to_do() {
    let f = Fixture::new();
    let _tools = Tools::rules(&[]);
    as_written_before(&lab::docker_manifest(&f.home.dir));
    std::fs::remove_file(f.home.dir.join("isoloom.yml")).unwrap();
    let err = f.run().down(&mut |_| {}).await.unwrap_err().to_string();
    assert!(err.starts_with("This lab's files were made by another version of Cyber CTF, and they couldn't be made again for this one: "), "{err}");
    assert!(err.ends_with("Remove the lab and add it again from the catalogue; anything it still runs is on the Machine page."), "{err}");
    assert!(f.fake.calls().is_empty(), "nothing ran: {:?}", f.fake.calls());
}

/// A tool of `PROCESSES` cut short that ignores the request to stop (SIGINT trapped), its pid in
/// `pid` once it runs.
#[cfg(unix)]
async fn cut_short_stubborn_tool(pid: &std::path::Path) -> u32 {
    let script = format!("trap '' INT; echo $$ > '{}'; while :; do sleep 0.1; done", pid.display());
    let spec = ProcessSpec::new("sh", ["-c", script.as_str()]);
    let mut quiet = |_: isothrum_runner::Stream, _: &str| {};
    let run = super::PROCESSES.run(&spec, &mut quiet);
    assert!(tokio::time::timeout(std::time::Duration::from_secs(2), run).await.is_err(), "cut short");
    std::fs::read_to_string(pid).unwrap().trim().parse().unwrap()
}

/// Whether process `pid` is gone, within a few seconds.
#[cfg(unix)]
async fn gone(pid: u32) -> bool {
    for _ in 0..50 {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists()
            && std::process::Command::new("kill").args(["-0", &pid.to_string()]).status().is_ok_and(|s| !s.success())
        {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    false
}

#[cfg(unix)]
#[tokio::test]
async fn a_tool_cut_short_never_outlives_the_wait_for_it_nor_the_apps_exit() {
    // The runner asks a cut-short tool to stop and gives it 30 s: a process exiting meanwhile
    // left it running.
    let dir = std::env::temp_dir().join(format!("cyberctf-cut-short-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    let waited = cut_short_stubborn_tool(&dir.join("waited")).await;
    let started = std::time::Instant::now();
    super::stop_cut_short_tools(std::time::Duration::from_secs(1)).await;
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(gone(waited).await, "ended once the wait was over");
    let at_exit = cut_short_stubborn_tool(&dir.join("at-exit")).await;
    super::end_cut_short_tools();
    assert!(gone(at_exit).await, "ended at the app's exit");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_launchers_vagrant_providers_are_isoloom_s_by_the_same_name_and_plugin() {
    // The launcher's provider table and Isoloom's agree: each provider Isoloom's Vagrantfiles drive
    // is the launcher's of the same id, and its plugin is Isoloom's (`Provider::plugin`).
    use crate::runtime::providers::Provider;
    let mapped: Vec<(Provider, isoloom_manifest::Provider)> = Provider::ALL.iter().filter_map(|p| Some((*p, super::vagrant_provider(*p)?))).collect();
    for (launcher, isoloom) in &mapped {
        assert_eq!(serde_json::to_value(isoloom).unwrap(), launcher.id(), "{launcher:?}");
        assert_eq!(super::vagrant_plugin(*launcher), isoloom.plugin(), "{launcher:?}");
    }
    let unmapped: Vec<Provider> = Provider::ALL.into_iter().filter(|p| super::vagrant_provider(*p).is_none()).collect();
    assert_eq!(unmapped, [Provider::Proxmox], "Isoloom's Vagrant providers are all the launcher's but Proxmox");
    assert_eq!(super::vagrant_plugin(Provider::Utm), Some("vagrant_utm"));
    assert_eq!(super::vagrant_plugin(Provider::Virtualbox), None);
    for cloud in Provider::CLOUDS {
        assert_eq!(super::vagrant_plugin(cloud), None, "{cloud:?}");
    }
}

/// Two machines on one network, both the lab's own: one can go while the other runs.
const TWO: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    app: { image: nginx:1.27 }\n  db:\n    networks: { lab: 11 }\n    app: { image: redis:7-alpine }\n";

#[test]
fn a_machine_removed_while_the_lab_runs_is_shown_down_not_left_out() {
    // Regression: a container removed outside the app (`docker rm -f`) vanished from the lab page,
    // so the page drew a healthy lab without it and offered no way back.
    let home = Home::new("gone", TWO, &[], lab::Target::Docker);
    let manifest = isoloom_manifest::parse(&std::fs::read_to_string(lab::docker_manifest(&home.dir)).unwrap()).unwrap();
    let status = |web: &str, db: &str| -> Status {
        serde_json::from_value(serde_json::json!({
            "state": "failed", "provider": null, "networks": [], "outputs": [], "message": null,
            "machines": [
                { "name": "web", "role": "member", "state": web, "addresses": {}, "published": [] },
                { "name": "db", "role": "member", "state": db, "addresses": {}, "published": [] },
            ],
        }))
        .unwrap()
    };
    let shown = view::lab_status(&manifest, &status("absent", "running"), None);
    assert!(shown.running);
    let words: Vec<(&str, &str)> = shown.machines.iter().map(|m| (m.name.as_str(), m.state.as_str())).collect();
    assert_eq!(words, [("web", "absent"), ("db", "running")], "the gone machine is there, down");
    // A lab that doesn't run shows only what is there.
    assert!(view::lab_status(&manifest, &status("absent", "absent"), None).machines.is_empty());
    let stopped = view::lab_status(&manifest, &status("absent", "stopped"), None);
    assert_eq!(stopped.machines.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["db"]);
}

#[tokio::test]
async fn a_running_labs_files_deleted_are_made_again_so_its_page_and_its_stop_still_reach_it() {
    // Regression: the player deleted the lab's generated folder while it ran. Its page read it as
    // stopped, and its Stop said "Nothing to remove" and left its containers running.
    let f = Fixture::new();
    let _tools = Tools::rules(&[]);
    let run = f.up().await;
    std::fs::remove_file(lab::docker_manifest(&f.home.dir)).unwrap();
    assert!(run.status().await.unwrap().running, "the page still sees it");
    let mut lines = Vec::new();
    run.down(&mut |l| lines.push(l)).await.unwrap();
    assert!(!lines.iter().any(|l| l.starts_with("Nothing to remove")), "{lines:?}");
    assert!(f.fake.last(" down --volumes").is_some(), "its containers were taken down");
    assert!(!run.status().await.unwrap().running);
}

#[tokio::test]
async fn a_running_lab_whose_whole_folder_is_gone_says_what_is_left() {
    let f = Fixture::new();
    let _tools = Tools::rules(&[]);
    let run = f.up().await;
    std::fs::remove_dir_all(&f.home.dir).unwrap();
    let err = run.down(&mut |_| {}).await.unwrap_err().to_string();
    assert!(err.starts_with("This lab's files are gone (") && err.contains("cyberctf-web"), "{err}");
    assert!(f.fake.last(" down --volumes").is_none(), "nothing could be run on it");
}
