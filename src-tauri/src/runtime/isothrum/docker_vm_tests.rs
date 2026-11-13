//! A container lab in a local VM (Isoloom's docker-vm) through the runner, against a scripted
//! Vagrant on UTM and a scripted Docker inside the VM (the runner's fake processes): no real
//! hypervisor runs. The lab's run manifest is the one Isoloom generates for the `docker-vm`
//! target. What the lab page relies on: the lab reads as running once UTM's plugin says
//! `started`. A lab that declares no attack machine of its own gets no box from the runner: the
//! launcher adds its box as a machine of the lab (`attack_machine`).

use super::test_lab::prelude::*;
use super::*;

const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http, publish: 8080 } }\n    app: { image: nginx:1.27 }\nchecks:\n  - { http: \"http://web/\" }\n";

/// What UTM's Vagrant plugin calls a running VM.
const STARTED: &str = "started";

/// `docker compose ps --all --format json` of the started lab, one object a line.
const PS: &str = r#"{"Service":"web","State":"running","Health":"healthy","Publishers":[{"PublishedPort":8080,"TargetPort":80,"Protocol":"tcp"}],"Labels":""}"#;

/// The helper VM and the lab's containers in it.
#[derive(Debug, Default)]
struct Engine {
    /// The VM's state label (absent: `not_created`).
    vm: Option<&'static str>,
    /// Whether Compose started the lab in it.
    up: bool,
    /// How many of the next VM starts (an `up` or a `reload`) UTM refuses as it does right after
    /// an import or a restart.
    refusals: u32,
    /// Whether a refused `reload` had shut the VM down first, as Vagrant's does before it boots
    /// the VM again with its new settings.
    reload_stops: bool,
    /// The lab's services Compose starts, beside `web` (a machine added to the lab).
    added: Option<&'static str>,
}

/// UTM's passing refusal, as Vagrant's UTM plugin reports it.
const UTM_REFUSAL: &str = "read_forwarded_ports.applescript: execution error: UTM got an error: Application isn\u{2019}t running. (-600)";

/// An installed container lab (`<labs>/web`) with its docker-vm files generated, a runner home,
/// and a model of its VM and containers.
type Fixture = Scripted<Engine>;

impl Fixture {
    fn new() -> Fixture {
        Scripted::answered_by(Home::new("-dvm", SPEC, &[], lab::vagrant_target(Runtime::Docker)), answer)
    }

    /// The lab's run in a VM, on a macOS host (where UTM is).
    fn run(&self) -> LabRun {
        // Free host ports for the runner to pin the lab's published ports to: Isothrum 2.1.3 and
        // later pass over a preferred number the computer holds, and pick a free one otherwise.
        let mut config = self.home.config(&self.fake, 41000..41064);
        config.host = isoloom_manifest::HostOs::Macos;
        LabRun::with(config, &self.home.dir, "web", Kind::DockerVm).unwrap()
    }
}

/// The fake Vagrant's answer to `call`: UTM's plugin, the helper VM, and the way into it.
fn answer(engine: &Mutex<Engine>, call: &Call) -> Reply {
    let mut e = locked(engine);
    let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
    if call.program == "vagrant" && matches!(args.first(), Some(&("up" | "reload"))) && e.refusals > 0 {
        e.refusals -= 1;
        if args.first() == Some(&"reload") && e.reload_stops {
            (e.vm, e.up) = (Some("stopped"), false);
        }
        return Reply::fail(1, UTM_REFUSAL);
    }
    match (call.program.as_str(), args.as_slice()) {
        ("vagrant", ["--version"]) => Reply::ok("Vagrant 2.4.9"),
        ("vagrant", ["plugin", "list", "--machine-readable"]) => Reply::ok("1,,plugin-name,vagrant_utm\n"),
        ("vagrant", ["status", ..]) => Reply::ok(&format!("1,default,state,{}", e.vm.unwrap_or("not_created"))),
        ("vagrant", ["up", ..]) => {
            e.vm = Some(STARTED);
            Reply::ok("==> default: Machine booted and ready!")
        }
        ("vagrant", ["suspend"]) => {
            e.vm = Some("suspended");
            Reply::ok("")
        }
        ("vagrant", ["destroy", ..]) => {
            // The VM goes; UTM's mood and the lab's files don't.
            *e = Engine { refusals: e.refusals, reload_stops: e.reload_stops, added: e.added, ..Engine::default() };
            Reply::ok("")
        }
        ("vagrant", ["ssh", "--no-tty", "--command", command, "default"]) => {
            let command = (*command).to_owned();
            in_vm(&mut e, &command, call)
        }
        _ => Reply::ok(""),
    }
}

/// What the VM answers to `command` over `vagrant ssh`: Compose for the lab.
fn in_vm(e: &mut Engine, command: &str, call: &Call) -> Reply {
    if e.vm != Some(STARTED) {
        return Reply::fail(255, "ssh: connect to host: Connection refused");
    }
    if command.contains(" up --detach ") {
        e.up = true;
        return Reply::ok(&format!("Container t-web-1  Healthy ({})", call.line()));
    }
    if command.contains("ps --all --format json") {
        let added = e.added.map(|name| format!("\n{}", PS.replace("\"web\"", &format!("\"{name}\"")).replace("8080", "8090")));
        return Reply::ok(&if e.up { format!("{PS}{}", added.unwrap_or_default()) } else { String::new() });
    }
    assert!(!command.starts_with("sudo docker "), "no container of the runner's own: {command}");
    Reply::ok("")
}

#[tokio::test]
async fn a_container_lab_in_a_utm_vm_runs_with_nothing_of_the_runners_own_in_it() {
    let f = Fixture::new();
    let run = f.run();
    run.up(Inputs::default(), Some("utm"), &mut |_| {}).await.unwrap();
    let s = run.status().await.unwrap();
    assert!(s.running && matches!(s.place, Some(Place::LocalVm)), "UTM's `started` reads as running (running={})", s.running);
    assert!(f.fake.calls().iter().all(|c| !c.line().contains("docker run")), "nothing but the lab went into the VM");
    run.down(&mut |_| {}).await.unwrap();
    assert!(f.model().vm.is_none());
}

/// The lab up in its UTM VM, then changed so that bringing the run up to date restarts the VM
/// (another host port), with UTM refusing the next `refusals` VM starts: what the update said, and
/// how it ended.
async fn update_through_utm_refusals(refusals: u32) -> (Vec<String>, Result<()>, usize) {
    let f = Fixture::new();
    let run = f.run();
    run.up(Inputs::default(), Some("utm"), &mut |_| {}).await.unwrap();
    std::fs::write(f.home.dir.join("isoloom.yml"), SPEC.replace("publish: 8080", "publish: 8081")).unwrap();
    lab::prepare(&f.home.dir, lab::vagrant_target(Runtime::Docker)).unwrap();
    f.model().refusals = refusals;
    let before = f.fake.calls().len();
    let mut said = Vec::new();
    let updated = run.update(&mut |l| said.push(l)).await;
    let reloads = f.fake.calls().iter().skip(before).filter(|c| c.line().starts_with("vagrant reload")).count();
    (said, updated, reloads)
}

#[tokio::test]
async fn an_update_tries_again_once_after_utms_passing_refusal() {
    // Regression: restarting the helper VM for its new settings booted into UTM's passing "-600"
    // and the update failed, where a second try goes through.
    let (said, updated, reloads) = update_through_utm_refusals(1).await;
    assert!(updated.is_ok(), "{updated:?}");
    assert_eq!(reloads, 2, "the restart, tried again once");
    assert_eq!(said.iter().filter(|l| l.contains("a passing UTM scripting error") && l.ends_with("trying again.")).count(), 1, "{said:?}");
    // The VM still ran after the refusal: the same update again, not a start.
    assert!(!said.iter().any(|l| l.starts_with("The run isn't running after that")), "{said:?}");
    // Refused again: that is the error, not another try.
    let (said, updated, reloads) = update_through_utm_refusals(2).await;
    assert!(updated.as_ref().is_err_and(|e| e.to_string().contains("(-600)")), "{updated:?}");
    assert_eq!(reloads, 2, "one try again, no more");
    assert_eq!(said.iter().filter(|l| l.ends_with("trying again.")).count(), 1, "{said:?}");
}

/// A start of the lab in its UTM VM (the launcher's own, [`launch::up_in_vm`]) with UTM refusing
/// its first `refusals` VM starts: what it said, how it ended, and how many `vagrant up` it ran.
async fn start_through_utm_refusals(refusals: u32) -> (Vec<String>, Result<()>, usize) {
    let f = Fixture::new();
    f.model().refusals = refusals;
    let mut said = Vec::new();
    let started =
        crate::runtime::launch::up_in_vm(&f.home.dir, crate::runtime::providers::Provider::Utm, Inputs::default(), || Ok(f.run()), &mut |l| said.push(l)).await;
    let ups = f.fake.calls().iter().filter(|c| c.line().starts_with("vagrant up")).count();
    (said, started, ups)
}

#[tokio::test]
async fn a_start_in_a_utm_vm_tries_again_once_after_utms_passing_refusal() {
    // Regression: a first boot right after UTM imported the VM answered "-600" and the start
    // failed, where a second try goes through.
    let (said, started, ups) = start_through_utm_refusals(1).await;
    assert!(started.is_ok(), "{started:?}");
    assert_eq!(ups, 2, "the start, tried again once");
    assert_eq!(said.iter().filter(|l| l.ends_with("trying again.")).count(), 1, "{said:?}");
    // Refused again: that is the error, not another try.
    let (said, started, ups) = start_through_utm_refusals(2).await;
    assert!(started.as_ref().is_err_and(|e| e.to_string().contains("(-600)")), "{started:?}");
    assert_eq!(ups, 2, "one try again, no more");
    assert_eq!(said.iter().filter(|l| l.ends_with("trying again.")).count(), 1, "{said:?}");
}

#[tokio::test]
async fn an_update_whose_restart_utm_refused_after_stopping_the_vm_starts_the_run_again_with_its_new_machine() {
    // Regression (a real UTM host): adding the attack box restarted the helper VM for its new
    // settings; the reload shut the VM down, then UTM refused its boot with "-600". Trying the
    // update again was refused ("This lab isn't running. Start it first."), so the box was never
    // added and the lab stayed down.
    let f = Fixture::new();
    let run = f.run();
    run.up(Inputs::default(), Some("utm"), &mut |_| {}).await.unwrap();
    let with_box = SPEC.replace("publish: 8080", "publish: 8081").replace(
        "checks:",
        "  box:\n    networks: { lab: 11 }\n    ports: { web: { port: 80, protocol: http, publish: 8090 } }\n    app: { image: nginx:1.27 }\nchecks:",
    );
    std::fs::write(f.home.dir.join("isoloom.yml"), with_box).unwrap();
    lab::prepare(&f.home.dir, lab::vagrant_target(Runtime::Docker)).unwrap();
    {
        let mut m = f.model();
        (m.refusals, m.reload_stops, m.added) = (1, true, Some("box"));
    }
    let before = f.fake.calls().len();
    let mut said = Vec::new();
    let updated = run.update(&mut |l| said.push(l)).await;
    assert!(updated.is_ok(), "{updated:?} {said:?}");
    let after: Vec<String> = f.fake.calls().iter().skip(before).map(Call::line).collect();
    assert_eq!(after.iter().filter(|c| c.starts_with("vagrant reload")).count(), 1, "the update's restart, not tried again: {after:?}");
    assert!(after.iter().any(|c| c.starts_with("vagrant up")), "the run started again: {after:?}");
    assert_eq!(said.iter().filter(|l| l.ends_with("trying again.")).count(), 1, "{said:?}");
    assert!(said.iter().any(|l| l == "The run isn't running after that: starting it again on its current files."), "{said:?}");
    let s = run.status().await.unwrap();
    assert!(
        s.running && s.machines.iter().any(|m| m.name == "box" && m.state == "running"),
        "the new machine is up: {:?}",
        s.machines.iter().map(|m| (&m.name, &m.state)).collect::<Vec<_>>()
    );
}
