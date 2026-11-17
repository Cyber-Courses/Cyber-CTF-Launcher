//! The launcher's runs through the runner, end to end against a scripted Compose engine (the
//! runner's fake processes) and scripted launcher tools (the attack box's `docker` calls): no real
//! tool runs. The lab's project is the one Isoloom generates from the spec.

use isothrum_runner::{EventKind, Format, Status, StepState, WarningCode};

use super::test_lab::prelude::*;
use super::*;
use crate::error::Error;
use crate::runtime::providers::testing::{Faked, no_tools};
use crate::runtime::{Park, Place, Runtime, lab};
use isothrum_runner::HostPort;

mod contract;

const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\ninputs: { CTF_LAUNCH_TOKEN: { default: dev-launch-token } }\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http, publish: 8080 } }\n    files: [{ path: /run/lab/token, input: CTF_LAUNCH_TOKEN }]\n    app: { image: nginx:1.27 }\nchecks:\n  - { http: \"http://web/\" }\nmessage: |\n  Open http://localhost:{{ machines.web.ports.web.published }}/\n";

/// Where the fake project's containers are, and the host port its web port got.
#[derive(Debug, Default)]
struct Engine {
    containers: Containers,
    host_port: u16,
    /// `compose up` fails (Compose's own error) from now on.
    failing_up: bool,
    /// `compose pull` fails (the registry's refusal) from now on.
    failing_pull: bool,
    /// `compose up` is told here as it begins, then runs until it is cut short (a long download).
    held_up: Option<std::sync::Arc<tokio::sync::Notify>>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Containers {
    #[default]
    None,
    Up,
    Stopped,
    /// Frozen by `compose pause`: there, and listed without `--all`.
    Paused,
}

/// An installed lab (`<labs>/web`) with its files generated, a runner home, and the fake engine.
type Fixture = Scripted<Engine>;

impl Fixture {
    fn new() -> Fixture {
        Scripted::answered_by(Home::new("", SPEC, &[], lab::Target::Docker), answer)
    }

    /// The lab's run, on a runner whose home is the fixture's root.
    fn run(&self) -> Deployment {
        Deployment::here_with(self.home.config(&self.fake, [41873]), &self.home.dir, "web", Kind::Docker).unwrap()
    }

    /// The lab's run, started with [`launch_env`] (the attack box's tools scripted by the caller).
    async fn up(&self) -> Deployment {
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
    let mut e = locked(engine);
    let (verb, rest) = match compose_asked(call, e.containers) {
        Ok(asked) => asked,
        Err(answered) => return answered,
    };
    let has = |flag: &str| rest.iter().any(|a| a == flag);
    match verb {
        "ps" => Reply::ok(&ps(e.containers, has("--all"), e.host_port)),
        // The services the project declares (the runner reads a service its listing lacks as gone).
        "config" if has("--services") => Reply::ok("web\n"),
        "config" => Reply::ok(
            r#"{"services":{"web":{"ports":[{"target":80,"host_ip":"127.0.0.1","published":"","protocol":"tcp"}]}},"networks":{"lab":{"name":"cyberctf-web_lab"}}}"#,
        ),
        // Compose pulls what it lacks inside its `up`.
        "up" if e.failing_pull && !has("--no-recreate") => {
            Reply::fail(18, "Error response from daemon: toomanyrequests: You have reached your pull rate limit")
        }
        // A resume: what exists is started as it is.
        "up" if has("--no-recreate") => {
            if e.containers == Containers::Stopped {
                e.containers = Containers::Up;
            }
            Reply::ok("")
        }
        "up" if e.failing_up => Reply::fail(1, "Error response from daemon: pull access denied"),
        "up" if e.held_up.is_some() => {
            e.held_up.iter().for_each(|began| began.notify_one());
            let mut held = Reply::ok("");
            held.delay = std::time::Duration::from_secs(3600);
            held
        }
        "up" => {
            e.containers = Containers::Up;
            e.host_port = if call.env("ISOLOOM_PUBLISH_FIXED").is_some() { 8080 } else { 41873 };
            Reply::ok(" Container cyberctf-web-web-1  Started")
        }
        "run" if rest.iter().any(|a| a == "isoloom-check") => Reply::ok("isoloom-check: PASS http://web/\nisoloom-check: END 1 passed, 0 failed"),
        "stop" => {
            if e.containers == Containers::Up {
                e.containers = Containers::Stopped;
            }
            Reply::ok("")
        }
        "pause" => {
            if e.containers == Containers::Up {
                e.containers = Containers::Paused;
            }
            Reply::ok("")
        }
        "unpause" => {
            if e.containers == Containers::Paused {
                e.containers = Containers::Up;
            }
            Reply::ok("")
        }
        "down" => {
            e.containers = Containers::None;
            Reply::ok("")
        }
        _ => Reply::ok(""),
    }
}

/// The Compose verb of `call` and its arguments, past the project's `-p`, `-f` and `--profile`
/// options; none for a call that isn't `docker compose`.
fn compose_verb(call: &Call) -> Option<(&str, &[String])> {
    // A run pinned to an engine names its context first (`docker --context <it> compose …`).
    let start = if call.args.first().is_some_and(|a| a == "--context") { 2 } else { 0 };
    if call.program != "docker" || call.args.get(start).map(String::as_str) != Some("compose") {
        return None;
    }
    let mut i = start + 1;
    while matches!(call.args.get(i).map(String::as_str), Some("-p" | "-f" | "--profile")) {
        i += 2;
    }
    call.args.get(i).map(|verb| (verb.as_str(), &call.args[i + 1..]))
}

/// Whether `call` is `docker compose … <verb>` with `flag` among its arguments (none: any).
fn is_compose(call: &Call, verb: &str, flag: Option<&str>) -> bool {
    compose_verb(call).is_some_and(|(v, rest)| v == verb && flag.is_none_or(|f| rest.iter().any(|a| a == f)))
}

/// The last `docker compose … <verb>` call (with `flag`, when given).
fn last_compose(f: &Fixture, verb: &str, flag: Option<&str>) -> Option<Call> {
    f.fake.calls().into_iter().rev().find(|c| is_compose(c, verb, flag))
}

/// The Compose verb `call` asks and its arguments ([`compose_verb`]); else its answer: `docker ps
/// -aq …` (whether any container of the project is there) answered from `containers`, nothing for
/// any other call.
fn compose_asked(call: &Call, containers: Containers) -> std::result::Result<(&str, &[String]), Reply> {
    if call.program == "docker" && call.args.first().is_some_and(|a| a == "ps") && call.args.iter().any(|a| a == "-aq") {
        return Err(Reply::ok(if containers == Containers::None { "" } else { "c1" }));
    }
    compose_verb(call).ok_or_else(|| Reply::ok(""))
}

/// The state `compose ps` lists the project's containers in (`--all`: stopped ones too); none
/// when it lists none.
fn listed_state(containers: Containers, all: bool) -> Option<&'static str> {
    match containers {
        Containers::None => None,
        Containers::Stopped if !all => None,
        Containers::Up => Some("running"),
        Containers::Stopped => Some("exited"),
        Containers::Paused => Some("paused"),
    }
}

/// `compose ps --format json` for the lab's web machine and Isoloom's probe.
fn ps(containers: Containers, all: bool, host_port: u16) -> String {
    let Some(state) = listed_state(containers, all) else { return String::new() };
    let publisher = format!(r#"{{"URL":"127.0.0.1","PublishedPort":{host_port},"TargetPort":80,"Protocol":"tcp"}}"#);
    [("web", publisher.as_str()), ("isoloom-probe", "")]
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
    Inputs { values, plain: &[], default_ports: true, docker_context: None }
}

/// A fixture whose lab is up and then parked with `Park::Shutdown`, the launcher tools scripted
/// empty — the shared start of the park/resume tests. The caller keeps the `Fixture` and the
/// tools' guard, which hold the lab's folder and the fake tools for the rest of the test.
async fn up_then_parked() -> (Fixture, Deployment, Faked) {
    let f = Fixture::new();
    let tools = no_tools();
    let run = f.up().await;
    run.park(Park::Shutdown, &mut |_| {}).await.unwrap();
    (f, run, tools)
}

#[tokio::test]
async fn a_container_lab_goes_up_through_the_runner_as_cyberctf_lab_id() {
    let f = Fixture::new();
    let _tools = no_tools();
    let mut log = Vec::new();
    f.run().up(on_own_ports(&launch_env()), None, &mut |l| log.push(l)).await.unwrap();
    let up = last_compose(&f, "up", None).unwrap();
    assert_eq!(&up.args[..3], ["compose", "-p", "cyberctf-web"], "{}", up.line());
    assert_eq!(up.env("CTF_LAUNCH_TOKEN"), Some("t0k"), "the launch's input reaches its slot");
    assert_eq!(up.env("ISOLOOM_PUBLISH_FIXED"), Some("1"), "the player chose the lab's own ports");
    assert!(log.iter().any(|l| l == " Container cyberctf-web-web-1  Started"), "Compose's own lines reach the deploy log: {log:?}");
    assert_eq!(log.last().map(String::as_str), Some("Open http://localhost:8080/"), "the lab's message, its port filled");
    assert!(crate::testing::runner::is_recorded(&f.home.runner(), "cyberctf-web"), "the run is kept by the runner whose home is the app data folder");
    assert!(!f.home.dir.join(".cyberctf-ports.yml").exists(), "the runner pins ports in its own state, not the lab folder");
}

#[tokio::test]
async fn a_container_lab_started_on_an_engine_stays_on_it_for_its_page_and_its_stop() {
    // The player chose an engine other than the current one: the runner records it with the run,
    // so the page's reads and the stop, which name none, go to it too.
    let f = Fixture::new();
    let _tools = no_tools();
    let run = f.run();
    let env = launch_env();
    let on_orbstack = Inputs { docker_context: Some("orbstack"), ..on_own_ports(&env) };
    run.up(on_orbstack, None, &mut |_| {}).await.unwrap();
    run.status().await.unwrap();
    run.down(&mut |_| {}).await.unwrap();
    let docker: Vec<_> = f.fake.calls().into_iter().filter(|c| c.program == "docker" && c.args.iter().any(|a| a == "compose")).collect();
    assert!(docker.iter().any(|c| c.args.iter().any(|a| a == "down")), "it was taken down");
    for call in &docker {
        assert_eq!(call.args[..2], ["--context", "orbstack"], "{}", call.line());
    }
}

#[tokio::test]
async fn the_lab_page_reads_the_runners_status() {
    let f = Fixture::new();
    let _tools = no_tools();
    let run = f.up().await;
    let s = run.status().await.unwrap();
    assert!(s.running && s.parked.is_none());
    assert!(matches!(s.place, Some(Place::Container)));
    assert_eq!(s.url.as_deref(), Some("http://127.0.0.1:8080/"));
    // Where the website opens it after a launch: the same port, from the runner's status.
    assert_eq!(run.local_url().await.unwrap().as_deref(), Some("http://127.0.0.1:8080"));
    let web = s.machines.iter().find(|m| m.name == "web").unwrap();
    assert_eq!((web.state.as_str(), web.ip.as_str(), web.infra), ("running", "10.30.0.10", false));
    assert_eq!(web.ports.iter().map(|p| (p.target.get(), p.published.map(HostPort::get))).collect::<Vec<_>>(), [(80, Some(8080))]);
    assert_eq!(web.services.iter().map(|s| (s.name.as_str(), s.kind.as_str(), s.ports.clone())).collect::<Vec<_>>(), [("web", "web", vec![80])]);
    // Isoloom 2.1.0 names the probe job of the computer's own architecture `isoloom-probe`.
    assert!(s.machines.iter().find(|m| m.name == "isoloom-probe").is_some_and(|m| m.infra), "Isoloom's probe is plumbing");
    assert!(s.machines.iter().all(|m| m.name != "isoloom-check"), "a check runner that never ran isn't listed");
    assert_eq!(s.networks.iter().map(|n| (n.name.as_str(), n.subnet.as_str(), n.internal)).collect::<Vec<_>>(), [("lab", "10.30.0.0/24", false)]);
}

#[tokio::test]
async fn a_lab_parks_and_resumes() {
    let (f, run, _tools) = up_then_parked().await;
    assert!(last_compose(&f, "stop", None).is_some(), "a park stops the lab's containers");
    let parked = run.status().await.unwrap();
    assert_eq!((parked.running, parked.parked), (false, Some(Park::Shutdown)), "as the runner recorded it");
    run.resume(&mut |_| {}).await.unwrap();
    assert!(run.status().await.unwrap().running);
}

/// Paused, a container lab's containers are frozen, not stopped, and the page reads it paused, as
/// the runner recorded it; a resume unpauses them, as they were.
#[tokio::test]
async fn a_container_lab_pauses_and_resumes_as_it_was() {
    let f = Fixture::new();
    let _tools = no_tools();
    let run = f.up().await;
    run.park(Park::Pause, &mut |_| {}).await.unwrap();
    assert!(last_compose(&f, "pause", None).is_some(), "a Pause freezes the lab's containers");
    assert!(last_compose(&f, "stop", None).is_none(), "and stops none");
    let paused = run.status().await.unwrap();
    assert_eq!((paused.running, paused.parked), (false, Some(Park::Pause)), "as the runner recorded it");
    run.resume(&mut |_| {}).await.unwrap();
    assert!(last_compose(&f, "unpause", None).is_some(), "a resume unpauses them");
    let back = run.status().await.unwrap();
    assert_eq!((back.running, back.parked), (true, None));
    assert_eq!(back.url.as_deref(), Some("http://127.0.0.1:8080/"), "at the same address");
}

/// A resume has no launch values (they aren't kept), and it starts the containers as they are
/// (`up --no-recreate`): each keeps the file its input was written to at the start, so the
/// player's value survives a park, and nothing else is ever written in its place.
#[tokio::test]
async fn a_resumed_lab_keeps_the_files_its_inputs_were_written_to() {
    let f = Fixture::new();
    let _tools = no_tools();
    let run = f.up().await;
    assert_eq!(last_compose(&f, "up", None).and_then(|c| c.env("CTF_LAUNCH_TOKEN").map(str::to_owned)).as_deref(), Some("t0k"));
    run.park(Park::Shutdown, &mut |_| {}).await.unwrap();
    run.resume(&mut |_| {}).await.unwrap();
    let start = last_compose(&f, "up", Some("--no-recreate")).expect("a resume starts what exists as it is");
    assert_eq!(start.env("CTF_LAUNCH_TOKEN"), None, "no launch value reaches a resume, not even an empty one");
}

/// A lab imported from a folder starts with no launch values: the launcher gives it none, not
/// even empty ones. The lab's spec gives its inputs a default, which its generated files fall
/// back to (a Compose secret's variable can't be left unset, so the token's file is a config
/// filled from it or from the default).
#[tokio::test]
async fn a_lab_started_without_launch_values_gets_none_and_falls_back_to_its_defaults() {
    let f = Fixture::new();
    let _tools = no_tools();
    f.run().up(Inputs::default(), None, &mut |_| {}).await.unwrap();
    let up = last_compose(&f, "up", None).expect("the lab goes up");
    assert_eq!(up.env("CTF_LAUNCH_TOKEN"), None, "{}", up.line());
    assert_eq!(up.env("CTF_API_URL"), None, "{}", up.line());
    let compose = std::fs::read_to_string(lab::compose_file(&f.home.dir)).unwrap();
    assert!(compose.contains("content: ${CTF_LAUNCH_TOKEN:-dev-launch-token}"), "{compose}");
}

/// Regression (docs/contributing/testing/resilience.md M4): a container stopped outside the app (`docker stop`,
/// Docker Desktop's Stop) while its lab ran could only be reset; a resume now starts it again
/// with Compose, as it is, and the lab reads running.
#[tokio::test]
async fn a_container_stopped_outside_the_app_is_resumed_as_it_is() {
    let f = Fixture::new();
    let _tools = no_tools();
    let run = f.up().await;
    f.set(Containers::Stopped);
    let down = run.status().await.unwrap();
    let web = down.machines.iter().find(|m| m.name == "web").unwrap();
    assert_ne!(web.state, "running");
    run.resume(&mut |_| {}).await.unwrap();
    assert!(last_compose(&f, "up", Some("--no-recreate")).is_some(), "Compose starts the stopped services as they are");
    assert_eq!(f.fake.calls().iter().filter(|c| is_compose(c, "up", None) && !is_compose(c, "up", Some("--no-recreate"))).count(), 1, "nothing is rebuilt");
    assert!(run.status().await.unwrap().running);
}

#[tokio::test]
async fn a_parked_lab_whose_containers_are_gone_offers_a_fresh_start() {
    let (f, run, _tools) = up_then_parked().await;
    f.set(Containers::None);
    let err = run.resume(&mut |_| {}).await.unwrap_err().to_string();
    assert!(err.starts_with("Nothing is left of this lab to resume"), "{err}");
    assert_eq!(run.status().await.unwrap().parked, None, "no longer read as parked");
}

/// Regression: a lab whose containers were removed outside the app (`docker compose down` by hand,
/// a clean-up tool) stayed `ready` in the runner's record: its page said it wasn't running, but a
/// re-import, an update or a removal was refused as still running until Stop. Before those, a run
/// of which nothing is left is now taken down through the runner, which clears its record; one with
/// anything left, even stopped, still holds them back.
#[tokio::test]
async fn a_lab_taken_down_outside_the_app_is_forgotten_before_a_reimport() {
    use crate::runtime::status::{forget_gone_with, refuse_replace, still_up};
    let f = Fixture::new();
    let _tools = no_tools();
    let run = f.up().await;
    let dirs = crate::platform::AppDirs::new(Some(f.home.root.clone()), None);
    let mut quiet = |_: String| {};
    for (left, what) in [(Containers::Up, "running"), (Containers::Stopped, "stopped outside the app")] {
        f.set(left);
        assert!(!run.is_gone().await.unwrap(), "{what}");
        forget_gone_with(&dirs, "web", |_| std::future::ready(Ok(Some(f.run()))), &mut quiet).await.unwrap();
        assert!(refuse_replace(&dirs, &f.home.dir, "web").await.is_err(), "{what}: still there, still refused");
    }
    f.set(Containers::None);
    assert!(!run.status().await.unwrap().running, "its page says it isn't running");
    // The runner records what its status read observed: the run is gone, and nothing of it holds
    // a re-import, an update or a removal back any more.
    assert!(run.is_gone().await.unwrap());
    forget_gone_with(&dirs, "web", |_| std::future::ready(Ok(Some(f.run()))), &mut quiet).await.unwrap();
    assert_eq!(still_up(&dirs, "web").await.unwrap(), None);
    refuse_replace(&dirs, &f.home.dir, "web").await.unwrap();
    run.up(on_own_ports(&launch_env()), None, &mut |_| {}).await.unwrap();
    assert!(run.status().await.unwrap().running, "and it starts again");
}

#[tokio::test]
async fn a_second_start_of_a_running_lab_is_refused_and_a_down_removes_it_all() {
    let f = Fixture::new();
    let _tools = no_tools();
    let run = f.up().await;
    let again = run.up(on_own_ports(&launch_env()), None, &mut |_| {}).await.unwrap_err();
    assert!(matches!(&again, Error::Refused(m) if m.starts_with("This lab is already running on this machine")), "{again}");
    run.down(&mut |_| {}).await.unwrap();
    assert!(last_compose(&f, "down", Some("--volumes")).is_some());
    assert!(!run.status().await.unwrap().running);
}

#[tokio::test]
async fn the_labs_checks_run_through_the_runner() {
    let f = Fixture::new();
    let _tools = no_tools();
    let run = f.run();
    let not_up = run.check().await.unwrap_err().to_string();
    assert!(not_up.contains("isn't running"), "{not_up}");
    run.up(on_own_ports(&launch_env()), None, &mut |_| {}).await.unwrap();
    let c = run.check().await.unwrap();
    assert!(c.available && c.ok, "{}", c.output);
    assert_eq!(c.results.iter().map(|r| (r.name.as_str(), r.ok)).collect::<Vec<_>>(), [("http://web/", true)]);
    assert!(c.output.contains("isoloom-check: END 1 passed, 0 failed"), "{}", c.output);
    // The checks' run starts what they depend on: no launch value reaches it, the lab's defaults
    // stand in.
    let checks = last_compose(&f, "run", None).expect("the checks run in Compose");
    assert_eq!(checks.env("CTF_LAUNCH_TOKEN"), None, "{}", checks.line());
}

#[tokio::test]
async fn a_lab_never_generated_reads_as_not_running() {
    let root = std::env::temp_dir().join(format!("cyberctf-isothrum-{}", rand::random::<u32>()));
    let dir = root.join("labs").join("web");
    std::fs::create_dir_all(&dir).unwrap();
    let s = Deployment::here_with(isothrum_runner::RunnerConfig::new(&root), &dir, "web", Kind::Docker).unwrap().status().await.unwrap();
    assert!(!s.running && s.machines.is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn tearing_down_a_lab_never_generated_is_a_clean_no_op() {
    // A start that fails before generation (for example, a lab the bundled engine can't read)
    // leaves no project. Stopping it must not error on the missing Compose file: it created
    // nothing, so there is nothing to remove, as the launcher's own Docker path tolerates.
    let root = std::env::temp_dir().join(format!("cyberctf-isothrum-{}", rand::random::<u32>()));
    let dir = root.join("labs").join("web");
    std::fs::create_dir_all(&dir).unwrap();
    let run = Deployment::here_with(isothrum_runner::RunnerConfig::new(&root), &dir, "web", Kind::Docker).unwrap();
    let mut lines = Vec::new();
    run.down(&mut |l| lines.push(l)).await.unwrap();
    assert_eq!(lines, ["Nothing to remove: the run has no files."]);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_deploy_log_shows_tool_lines_and_warnings_only() {
    let step = EventKind::Step { step: "images".into(), state: StepState::Started, label: None, count: None };
    assert_eq!(progress::line(&step), None, "a step is a step, not a line");
    let log = EventKind::Log { step: "up".into(), stream: isothrum_runner::Stream::Stderr, line: " Container web  Healthy".into() };
    assert_eq!(progress::line(&log).as_deref(), Some(" Container web  Healthy"));
    let warning = |code: WarningCode, message: &str| progress::line(&EventKind::Warning { code, message: message.into() });
    assert_eq!(warning(WarningCode::LeftoversRemoved, "removing what an earlier run left").as_deref(), Some("Clearing a previous, stopped run…"));
    assert_eq!(warning(WarningCode::StepTolerated, "x"), None);
    assert_eq!(warning(WarningCode::OrphanRemoved, "Removing a VM an earlier run left").as_deref(), Some("Removing a VM an earlier run left"));
}

#[test]
fn the_specs_sensitive_flag_alone_decides_value_or_secret() {
    // A spec with both kinds: a sensitive token (by its name alone), a URL marked not sensitive,
    // and a value it doesn't declare (never trusted as plain).
    let spec = crate::runtime::lab::parse(
        "version: 1\nname: t\nnetworks: { lab: { cidr: 10.30.0.0/24 } }\ninputs:\n  CTF_LAUNCH_TOKEN: {}\n  CTF_API_URL: { sensitive: false }\nmachines:\n  web: { networks: { lab: 10 }, app: { image: nginx:1.27 } }\n",
    )
    .unwrap();
    let plain = crate::runtime::lab::plain_inputs(&spec);
    assert_eq!(plain, ["CTF_API_URL"]);
    let values =
        [("CTF_LAUNCH_TOKEN", "t0ken"), ("CTF_API_URL", "https://api.example"), ("CTF_FLAG_MAIN", "flag{x}")].map(|(n, v)| (n.to_owned(), v.to_owned()));
    let opts = start_options(Inputs { values: &values, plain: &plain, ..Inputs::default() }, None);
    assert_eq!(opts.env.get("CTF_API_URL").map(String::as_str), Some("https://api.example"), "plain: a value the run keeps");
    let secrets: Vec<&str> = opts.secrets.iter().map(|(name, _)| name).collect();
    assert_eq!(secrets, ["CTF_FLAG_MAIN", "CTF_LAUNCH_TOKEN"], "sensitive, or undeclared: secrets");
    assert!(!opts.env.contains_key("CTF_LAUNCH_TOKEN") && !opts.env.contains_key("CTF_FLAG_MAIN"));
}

#[test]
fn the_runners_step_events_are_the_deploy_logs_steps_and_nothing_else_is() {
    use crate::runtime::deploy_log::{DeployStep, DeployStepState, StepCount};
    let failed = EventKind::Step { step: "up".into(), state: StepState::Failed, label: Some("dc01".into()), count: None };
    assert_eq!(progress::step(&failed), Some(DeployStep { step: "up".into(), state: DeployStepState::Failed, label: Some("dc01".into()), count: None }));
    let counted = EventKind::Step { step: "images".into(), state: StepState::Started, label: None, count: Some(isothrum_runner::StepCount::new(1, Some(3))) };
    assert_eq!(progress::step(&counted).and_then(|s| s.count), Some(StepCount { done: 1, total: Some(3) }));
    let log = EventKind::Log { step: "up".into(), stream: isothrum_runner::Stream::Stdout, line: "x".into() };
    assert_eq!(progress::step(&log), None);
}

#[test]
fn a_start_gives_the_launch_values_as_secrets_and_the_ports_choice_as_a_value() {
    let values = [("CTF_FLAG_MAIN".to_owned(), "flag{x}".to_owned())];
    let opts = start_options(Inputs::values(&values), None);
    assert_eq!(opts.secrets.get("CTF_FLAG_MAIN").map(SecretValue::expose), Some("flag{x}"));
    assert_eq!(opts.secrets.iter().count(), 1, "the launch's values only");
    assert!(opts.env.is_empty(), "random ports: no fixed publish");
    let fixed = start_options(Inputs { values: &[], plain: &[], default_ports: true, docker_context: None }, None);
    assert_eq!(fixed.env.get("ISOLOOM_PUBLISH_FIXED").map(String::as_str), Some("1"));
    assert_eq!(fixed.secrets.iter().count(), 0, "the ports choice is no lab input");
    assert_eq!(opts.provider, None);
    let hypervisor = runner_provider("virtualbox").unwrap();
    assert_eq!(start_options(Inputs::default(), Some(hypervisor)).provider, Some(isothrum_runner::Provider::Virtualbox), "a VM lab's hypervisor");
    // Every Vagrant provider the launcher starts a lab on has the runner's name (Proxmox is a
    // target of its own, never one); anything else is refused.
    for p in crate::runtime::providers::Provider::ALL.into_iter().filter(|p| *p != crate::runtime::providers::Provider::Proxmox) {
        assert!(runner_provider(p.id()).is_ok(), "{}", p.id());
    }
    assert!(runner_provider("nonsense").is_err());
}

/// The lab's attack box as the launcher places it on this machine: a container added to the
/// lab, the run brought up to date with it.
fn placed_box(f: &Fixture) -> crate::runtime::attack_box::Placed {
    use crate::runtime::attack_box::Placed;
    use crate::runtime::host::Host;
    Placed { host: Host::here(f.run()), dir: f.home.dir.clone(), target: lab::Target::Docker, vm: false }
}

#[tokio::test]
async fn an_attack_box_refused_by_a_lab_that_is_not_up_leaves_the_lab_as_published() {
    let f = Fixture::new();
    let _tools = no_tools();
    let dirs = crate::platform::AppDirs::new(Some(f.home.root.clone()), None);
    let compose = std::fs::read_to_string(lab::compose_file(&f.home.dir)).unwrap();
    let err =
        placed_box(&f).add(&dirs, crate::runtime::attack_box::Pick { image: "kalilinux/kali-rolling", desktop: false }, None, &mut |_| {}).await.unwrap_err();
    assert!(matches!(&err, crate::error::Error::RefusedBeforeActing(m) if m == "This lab isn't running. Start it first."), "{err}");
    assert!(!crate::runtime::attack_box::added(&f.home.dir).unwrap(), "the box is out of the lab's spec again");
    assert_eq!(std::fs::read_to_string(lab::compose_file(&f.home.dir)).unwrap(), compose, "and its files as they were");
}

#[tokio::test]
async fn an_attack_box_whose_start_failed_in_the_run_stays_in_the_lab_for_stop_to_take_out() {
    let f = Fixture::new();
    let _tools = no_tools();
    let dirs = crate::platform::AppDirs::new(Some(f.home.root.clone()), None);
    f.up().await;
    f.model().failing_up = true;
    let placed = placed_box(&f);
    assert!(placed.add(&dirs, crate::runtime::attack_box::Pick { image: "kalilinux/kali-rolling", desktop: false }, None, &mut |_| {}).await.is_err());
    assert!(crate::runtime::attack_box::added(&f.home.dir).unwrap(), "the run may hold some of it: kept, for a stop to retire");
    // The run failed: the runner brings it up to date with nothing until it is started again or
    // stopped (a failed run takes only those), so the box stays in the lab for a stop to take out.
    f.model().failing_up = false;
    assert!(placed.remove(&dirs, &mut |_| {}).await.is_err(), "a failed run isn't updated");
    assert!(crate::runtime::attack_box::added(&f.home.dir).unwrap(), "kept for the stop");
}

#[tokio::test]
async fn an_attack_box_whose_image_cant_be_downloaded_says_so_after_its_pull() {
    // Regression: the box's image downloading inside the start's `up` was read as the lab's `up`
    // running out of time, and the player was told to check Docker's resources.
    let f = Fixture::new();
    let _tools = no_tools();
    let dirs = crate::platform::AppDirs::new(Some(f.home.root.clone()), None);
    f.up().await;
    f.model().failing_pull = true;
    let ups = || f.fake.calls().iter().filter(|c| is_compose(c, "up", None)).count();
    let before = ups();
    let err =
        placed_box(&f).add(&dirs, crate::runtime::attack_box::Pick { image: "kalilinux/kali-rolling", desktop: false }, None, &mut |_| {}).await.unwrap_err();
    assert_eq!(
        err.code(),
        crate::error::Code::DownloadFailed {
            reason: "Error response from daemon: toomanyrequests: You have reached your pull rate limit (exited with code 18)".into()
        }
    );
    assert!(err.to_string().starts_with("Couldn't download the lab's images: Error response from daemon: toomanyrequests"), "{err}");
    assert_eq!(ups(), before + 1, "the one update, cut short by its pull");
}

#[tokio::test]
async fn an_attack_box_goes_in_and_out_of_the_run_with_the_labs_files() {
    let f = Fixture::new();
    let _tools = no_tools();
    let dirs = crate::platform::AppDirs::new(Some(f.home.root.clone()), None);
    f.up().await;
    let placed = placed_box(&f);
    let compose = || std::fs::read_to_string(lab::compose_file(&f.home.dir)).unwrap();
    let updates = || f.fake.calls().iter().filter(|c| is_compose(c, "up", None)).count();
    let before = updates();
    placed.add(&dirs, crate::runtime::attack_box::Pick { image: "kalilinux/kali-rolling", desktop: false }, None, &mut |_| {}).await.unwrap();
    // The lab's files regenerated with the box, then the run brought up to date with them.
    assert!(compose().contains("attackbox") && compose().contains("kalilinux/kali-rolling"));
    let added = updates();
    assert!(added > before, "the run brought up to date");
    placed.remove(&dirs, &mut |_| {}).await.unwrap();
    // Out of the files, and the run brought up to date again (Compose drops the orphan).
    assert!(!compose().contains("attackbox"));
    assert!(updates() > added, "and again");
    assert!(!crate::runtime::attack_box::added(&f.home.dir).unwrap());
}

#[test]
fn a_lab_here_is_run_in_the_form_it_was_started_in() {
    let dir = std::env::temp_dir().join(format!("cyberctf-local-form-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    let home = dir.join("home");
    let recorded = std::cell::RefCell::new(crate::runtime::recorded::Recorded::default());
    let form = |runtime| Deployment::local_of(&home, &dir, "t", runtime, &recorded.borrow()).unwrap().map(|r| (r.served.format, r.target()));
    let record = |target: &str| {
        let json =
            serde_json::json!({ "version": 1, "name": "cyberctf-t", "project": dir.join(".isoloom").join(target), "format": "vagrant", "state": "ready" });
        *recorded.borrow_mut() = Some(serde_json::from_value::<isothrum_runner::RunRecord>(json).unwrap()).into();
    };
    // As declared: its containers on Docker, its VMs, and no run here for cloud services.
    assert_eq!(form(Runtime::Docker), Some((Format::Compose, Some(lab::Target::Docker))));
    assert_eq!(form(Runtime::Vm), Some((Format::Vagrant, Some(lab::Target::Vagrant))));
    assert_eq!(form(Runtime::Cloud), None);
    // One started as one VM per machine, as VMs. A run from a folder Isoloom no longer generates
    // (1.9's helper VM, `docker-vm`) is no form of its own: the lab reads as declared.
    record("docker-vm");
    assert_eq!(form(Runtime::Docker), Some((Format::Compose, Some(lab::Target::Docker))));
    record("vagrant");
    assert_eq!(form(Runtime::Docker), Some((Format::Vagrant, Some(lab::Target::Vagrant))));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Two machines on one network, both the lab's own: one can go while the other runs.
const TWO: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    app: { image: nginx:1.27 }\n  db:\n    networks: { lab: 11 }\n    app: { image: redis:7-alpine }\n";

#[test]
fn a_machine_removed_while_the_lab_runs_is_shown_down_not_left_out() {
    // Regression: a container removed outside the app (`docker rm -f`) vanished from the lab page,
    // so the page drew a healthy lab without it and offered no way back.
    let home = Home::new("gone", TWO, &[], lab::Target::Docker);
    let manifest = described::Described::of(&home.dir, described::Form::Containers).unwrap();
    let status = |run: &str, web: &str, db: &str| -> Status {
        serde_json::from_value(serde_json::json!({
            "state": run, "provider": null, "outputs": [],
            "machines": [
                { "name": "web", "state": web, "published": [] },
                { "name": "db", "state": db, "published": [] },
            ],
        }))
        .unwrap()
    };
    let shown = view::lab_status(&manifest, &status("failed", "absent", "running"), None);
    assert!(shown.running);
    let words: Vec<(&str, &str)> = shown.machines.iter().map(|m| (m.name.as_str(), m.state.as_str())).collect();
    assert_eq!(words, [("web", "absent"), ("db", "running")], "the gone machine is there, down");
    // A lab taken down or parked shows only what is there.
    assert_eq!(view::lab_status(&manifest, &status("absent", "absent", "absent"), None).machines, [] as [crate::runtime::Machine; 0]);
    let stopped = view::lab_status(&manifest, &status("parked", "absent", "stopped"), None);
    assert_eq!(stopped.machines.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["db"]);
}

/// The ways the page offers to park a run are the runner's (`Status::parks`): a QEMU run lists no
/// pause (vagrant-qemu has no suspend), so none is offered.
#[test]
fn a_run_parks_as_the_runner_says_it_can() {
    let home = Home::new("parks", TWO, &[], lab::Target::Docker);
    let manifest = described::Described::of(&home.dir, described::Form::Containers).unwrap();
    let parks = |listed: serde_json::Value| {
        let status: Status =
            serde_json::from_value(serde_json::json!({ "state": "ready", "provider": null, "outputs": [], "machines": [], "parks": listed })).unwrap();
        view::lab_status(&manifest, &status, None).parks
    };
    assert_eq!(parks(serde_json::json!(["pause", "shutdown"])), [Park::Pause, Park::Shutdown]);
    assert_eq!(parks(serde_json::json!(["shutdown"])), [Park::Shutdown]);
}

#[test]
fn a_one_vm_labs_vm_destroyed_outside_the_app_is_shown_gone() {
    // Regression (resilience-vm-destroyed): a gone machine was shown only while another of the
    // lab's ran. With its one VM destroyed, the runner reads the run failed and the VM absent; the
    // page drew no machine at all, so "web is shown gone" read None, and offered no way back.
    let home = Home::new(
        "one-gone",
        "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.31.0.0/24 }\nmachines:\n  web:\n    os: debian@12\n    networks: { lab: 10 }\n",
        &[],
        lab::Target::Vagrant,
    );
    let manifest = described::Described::of(&home.dir, described::Form::Vms).unwrap();
    let status = |run: &str| -> Status {
        serde_json::from_value(serde_json::json!({
            "state": run, "provider": "virtualbox", "outputs": [],
            "machines": [{ "name": "web", "state": "absent", "published": [] }],
        }))
        .unwrap()
    };
    let shown = view::lab_status(&manifest, &status("failed"), None);
    assert!(!shown.running);
    let words: Vec<(&str, &str)> = shown.machines.iter().map(|m| (m.name.as_str(), m.state.as_str())).collect();
    assert_eq!(words, [("web", "absent")], "the destroyed VM is there, gone");
    // Taken down by the app, it is gone without a word.
    assert_eq!(view::lab_status(&manifest, &status("absent"), None).machines, [] as [crate::runtime::Machine; 0]);
}

#[test]
fn a_server_labs_entry_is_where_its_module_publishes_it() {
    // Regression (a real Proxmox run): the lab ran with no link. A server's module forwards a
    // published port at its router's uplink (its `published` output); the runner reports no host
    // port for a Terraform module's VM, and no address for one without a guest agent.
    let home = Home::new(
        "pve-entry",
        "version: 1\nname: t\nnetworks:\n  app: { cidr: 10.80.0.0/24 }\nmachines:\n  web:\n    os: debian@12\n    networks: { app: 10 }\n    ports: { site: { port: 80, protocol: http, publish: 8080 } }\n",
        &[],
        lab::Target::Proxmox,
    );
    let manifest = described::Described::of(&home.dir, described::Form::Vms).unwrap();
    let status = |published: Option<&str>, address: Option<&str>| -> Status {
        let outputs = published.map(|p| serde_json::json!([{ "name": "published", "sensitive": false, "value": p }])).unwrap_or(serde_json::json!([]));
        serde_json::from_value(serde_json::json!({
            "state": "ready", "provider": null, "outputs": outputs,
            "machines": [
                { "name": "isoloom_router", "state": "running", "published": [], "address": "192.0.2.10" },
                { "name": "web", "state": "running", "published": [], "address": address },
            ],
        }))
        .unwrap()
    };
    let url = |published, address| view::lab_status(&manifest, &status(published, address), None).url;
    assert_eq!(url(Some(r#"{"web/site":"192.0.2.10:8080"}"#), None).as_deref(), Some("http://192.0.2.10:8080/"));
    // The module's forward wins over the VM's own address (a cloud VM's public one, whose
    // firewall opens the published port only).
    assert_eq!(url(Some(r#"{"web/site":"203.0.113.7:8080"}"#), Some("203.0.113.7")).as_deref(), Some("http://203.0.113.7:8080/"));
    // The router's address not known yet: no link rather than a broken one.
    assert_eq!(url(Some(r#"{"web/site":":8080"}"#), None), None);
    // No such output: the machine's own address, as before.
    assert_eq!(url(None, Some("172.16.1.53")).as_deref(), Some("http://172.16.1.53:80/"));
    assert_eq!(url(None, None), None);
}

#[tokio::test]
async fn a_running_labs_files_deleted_are_made_again_so_its_page_and_its_stop_still_reach_it() {
    // Regression: the player deleted the lab's generated folder while it ran. Its page read it as
    // stopped, and its Stop said "Nothing to remove" and left its containers running.
    let f = Fixture::new();
    let _tools = no_tools();
    let run = f.up().await;
    std::fs::remove_file(lab::compose_file(&f.home.dir)).unwrap();
    assert!(run.status().await.unwrap().running, "the page still sees it");
    let mut lines = Vec::new();
    run.down(&mut |l| lines.push(l)).await.unwrap();
    assert!(!lines.iter().any(|l| l.starts_with("Nothing to remove")), "{lines:?}");
    assert!(last_compose(&f, "down", Some("--volumes")).is_some(), "its containers were taken down");
    assert!(!run.status().await.unwrap().running);
}

#[tokio::test]
async fn a_running_lab_whose_whole_folder_is_gone_says_what_is_left() {
    let f = Fixture::new();
    let _tools = no_tools();
    let run = f.up().await;
    std::fs::remove_dir_all(&f.home.dir).unwrap();
    let err = run.down(&mut |_| {}).await.unwrap_err().to_string();
    assert!(err.starts_with("This lab's files are gone (") && err.contains("cyberctf-web"), "{err}");
    assert!(last_compose(&f, "down", Some("--volumes")).is_none(), "nothing could be run on it");
}
