//! The launcher's runs through the runner, end to end against a scripted Compose engine (the
//! runner's fake processes) and scripted launcher tools (the attack box's `docker` calls): no real
//! tool runs. The lab's run manifest is the one Isoloom generates from the spec.

use std::sync::{Arc, Mutex, PoisonError};

use isothrum_runner::process::fake::{Call, FakeProcesses, Reply};
use isothrum_runner::{EventKind, FakePorts, ProcessRunner, ReadyState, StepState, WarningCode};

use super::*;
use crate::error::Error;
use crate::platform::exec::fake::Tools;
use crate::runtime::Park;

const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\ninputs: [CTF_LAUNCH_TOKEN]\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http, publish: 8080 } }\n    files: [{ path: /run/lab/token, input: CTF_LAUNCH_TOKEN }]\n    app: { image: nginx:1.27 }\nchecks:\n  - { http: \"http://web/\" }\nmessage: |\n  Open http://localhost:{{ machines.web.ports.web.published }}/\n";

/// Where the fake project's containers are, and the host port its web port got.
#[derive(Debug, Default)]
struct Engine {
    containers: Containers,
    host_port: u16,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Containers {
    #[default]
    None,
    Up,
    Stopped,
}

/// An installed lab (`<labs>/web`) with its files generated, a runner home, and the fake engine.
struct Fixture {
    root: std::path::PathBuf,
    dir: std::path::PathBuf,
    engine: Arc<Mutex<Engine>>,
    fake: Arc<FakeProcesses>,
}

impl Fixture {
    fn new() -> Fixture {
        let root = std::env::temp_dir().join(format!("cyberctf-isothrum-{}", rand::random::<u32>()));
        let dir = root.join("labs").join("web");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("isoloom.yml"), SPEC).unwrap();
        lab::prepare(&dir, lab::Target::Docker).unwrap();
        let engine = Arc::new(Mutex::new(Engine::default()));
        let script = Arc::clone(&engine);
        let fake = Arc::new(FakeProcesses::new(move |call| answer(&script, call)));
        Fixture { root, dir, engine, fake }
    }

    /// The lab's run, on a runner whose home is the fixture's root.
    fn run(&self) -> LabRun {
        let config =
            RunnerConfig::new(&self.root).with_processes(Arc::clone(&self.fake) as Arc<dyn ProcessRunner>).with_ports(Arc::new(FakePorts::new([41873])));
        LabRun::with(config, &self.dir, "web", Kind::Docker).unwrap()
    }

    /// The lab's run, started with [`launch_env`] (the attack box's tools scripted by the caller).
    async fn up(&self) -> LabRun {
        let run = self.run();
        run.up(&launch_env(), None, &mut |_| {}).await.unwrap();
        run
    }

    fn set(&self, containers: Containers) {
        self.engine.lock().unwrap_or_else(PoisonError::into_inner).containers = containers;
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The fake Compose engine's answer to `call`.
fn answer(engine: &Mutex<Engine>, call: &Call) -> Reply {
    let line = call.line();
    let mut e = engine.lock().unwrap_or_else(PoisonError::into_inner);
    if line.contains(" ps --all --format json") || line.contains(" ps --format json") {
        return Reply::ok(&ps(e.containers, line.contains("--all"), e.host_port));
    }
    if line.contains(" config --format json") {
        return Reply::ok(
            r#"{"services":{"web":{"ports":[{"target":80,"host_ip":"127.0.0.1","published":"","protocol":"tcp"}]}},"networks":{"lab":{"name":"cyberctf-web_lab"}}}"#,
        );
    }
    if line.contains(" up --detach ") {
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
    vec![("CTF_LAUNCH_TOKEN".into(), "t0k".into()), (PORTS_ENV.into(), "default".into())]
}

#[tokio::test]
async fn a_container_lab_goes_up_through_the_runner_as_cyberctf_lab_id() {
    let f = Fixture::new();
    let tools = Tools::rules(&[]);
    let mut log = Vec::new();
    f.run().up(&launch_env(), None, &mut |l| log.push(l)).await.unwrap();
    let up = f.fake.last(" up --detach").unwrap();
    assert!(up.line().contains("--project-name cyberctf-web"), "{}", up.line());
    assert_eq!(up.env("CTF_LAUNCH_TOKEN"), Some("t0k"), "the launch's input reaches its slot");
    assert_eq!(up.env("ISOLOOM_PUBLISH_FIXED"), Some("1"), "the player chose the lab's own ports");
    assert_eq!(up.env(PORTS_ENV), None, "a value no slot takes is never delivered");
    assert!(log.iter().any(|l| l == "Starting the lab's containers…"), "{log:?}");
    assert_eq!(log.last().map(String::as_str), Some("Open http://localhost:8080/"), "the lab's message, its port filled");
    assert!(tools.called("docker inspect -f {{.State.Running}} cyberctf-web-attacker"), "the attack box is asked to rejoin: {:?}", tools.calls());
    assert!(f.root.join("runs/cyberctf-web/record.json").is_file(), "the run is kept in the app data's runs/");
    assert!(!f.dir.join(".cyberctf-ports.yml").exists(), "the runner pins ports in its own state, not the lab folder");
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
async fn a_lab_parks_and_resumes_with_its_attack_box() {
    let f = Fixture::new();
    let tools = Tools::rules(&[]);
    let run = f.up().await;
    run.park(Park::Shutdown, &mut |_| {}).await.unwrap();
    assert!(tools.called("docker stop -t 1 cyberctf-web-attacker"), "the attack box stops with the lab");
    assert!(f.fake.last("compose").is_some_and(|c| c.line().ends_with(" stop")));
    let parked = run.status().await.unwrap();
    assert_eq!((parked.running, parked.parked), (false, Some(Park::Shutdown)));
    paths::mark_parked(&f.dir, Some(Park::Pause)).unwrap();
    assert_eq!(run.status().await.unwrap().parked, Some(Park::Pause), "as the player chose it");
    run.resume(&mut |_| {}).await.unwrap();
    assert!(tools.called("docker start cyberctf-web-attacker"), "the attack box starts again with the lab");
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

#[tokio::test]
async fn a_parked_lab_whose_containers_are_gone_offers_a_fresh_start() {
    let f = Fixture::new();
    let _tools = Tools::rules(&[]);
    let run = f.up().await;
    run.park(Park::Shutdown, &mut |_| {}).await.unwrap();
    paths::mark_parked(&f.dir, Some(Park::Shutdown)).unwrap();
    f.set(Containers::None);
    let err = run.resume(&mut |_| {}).await.unwrap_err().to_string();
    assert!(err.starts_with("Nothing is left of this lab to resume"), "{err}");
    assert_eq!(paths::parked(&f.dir), None, "the parked mark goes");
}

#[tokio::test]
async fn a_second_start_of_a_running_lab_is_refused_and_a_down_removes_it_all() {
    let f = Fixture::new();
    let tools = Tools::rules(&[]);
    let run = f.up().await;
    let again = run.up(&launch_env(), None, &mut |_| {}).await.unwrap_err();
    assert!(matches!(&again, Error::Refused(m) if m.starts_with("This lab is already running on this machine")), "{again}");
    run.down(&mut |_| {}).await.unwrap();
    assert!(tools.called("docker rm -f cyberctf-web-attacker"), "the attack box goes first, freeing the lab's networks");
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
    run.up(&launch_env(), None, &mut |_| {}).await.unwrap();
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
    assert_eq!(warning(WarningCode::HookFailed, "after_up: no network").as_deref(), Some("The lab is fine, but its attack box didn't follow: no network"));
    assert_eq!(warning(WarningCode::StepTolerated, "x"), None);
    assert_eq!(warning(WarningCode::OrphanRemoved, "Removing a VM an earlier run left").as_deref(), Some("Removing a VM an earlier run left"));
    assert_eq!(progress::line(&EventKind::Result { ok: true, data: serde_json::json!({}) }), None, "the caller says how it ended");
}

#[test]
fn a_start_gives_the_launch_values_to_the_slots_and_resumes_a_parked_run() {
    let opts = start_options(&[("CTF_FLAG_MAIN".into(), "flag{x}".into()), (PORTS_ENV.into(), "random".into())], None);
    assert_eq!(opts.secrets.get("CTF_FLAG_MAIN").map(SecretValue::expose), Some("flag{x}"));
    assert!(opts.secrets.get(PORTS_ENV).is_none());
    assert!(opts.params.is_empty(), "random ports: no fixed publish");
    assert_eq!(opts.occupied, Occupied::ResumeParked);
    let fixed = start_options(&[(PORTS_ENV.into(), "default".into())], None);
    assert_eq!(fixed.params.get("ISOLOOM_PUBLISH_FIXED").map(String::as_str), Some("1"));
    assert_eq!(opts.provider, None);
    assert_eq!(start_options(&[], Some("virtualbox")).provider.as_deref(), Some("virtualbox"), "a VM lab's hypervisor");
}
