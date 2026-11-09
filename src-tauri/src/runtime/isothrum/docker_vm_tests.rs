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
}

/// An installed container lab (`<labs>/web`) with its docker-vm files generated, a runner home,
/// and a model of its VM and containers.
type Fixture = Scripted<Engine>;

impl Fixture {
    fn new() -> Fixture {
        Scripted::answered_by(Home::new("-dvm", SPEC, &[], lab::vagrant_target(Runtime::Docker)), answer)
    }

    /// The lab's run in a VM, on a macOS host (where UTM is).
    fn run(&self) -> LabRun {
        let mut config = self.home.config(&self.fake, []);
        config.host = isothrum_runner::isoloom_manifest::HostOs::Macos;
        LabRun::with(config, &self.home.dir, "web", Kind::DockerVm).unwrap()
    }
}

/// The fake Vagrant's answer to `call`: UTM's plugin, the helper VM, and the way into it.
fn answer(engine: &Mutex<Engine>, call: &Call) -> Reply {
    let mut e = locked(engine);
    let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
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
            *e = Engine::default();
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
        return Reply::ok(if e.up { PS } else { "" });
    }
    assert!(!command.starts_with("sudo docker "), "no container of the runner's own: {command}");
    Reply::ok("")
}

#[tokio::test]
async fn a_container_lab_in_a_utm_vm_runs_and_the_runner_adds_no_attack_box_of_its_own() {
    let f = Fixture::new();
    let run = f.run();
    run.up(&[], Some("utm"), &mut |_| {}).await.unwrap();
    let s = run.status().await.unwrap();
    assert!(s.running && matches!(s.place, Some(Place::LocalVm)), "UTM's `started` reads as running (running={})", s.running);

    let attack = run.attack_status(None).await.unwrap();
    assert!(!attack.running && attack.ip.is_empty(), "{attack:?}");
    assert_eq!(attack.networks, ["lab"], "the lab's networks are known");
    assert_eq!(attack.shell_cmd, "", "no box of the runner's own to open");
    let err = run.attach(None, Some("kalilinux/kali-rolling"), &mut |_| {}).await.unwrap_err().to_string();
    assert!(err.contains("this lab declares no access machine"), "{err}");
    assert!(f.fake.calls().iter().all(|c| !c.line().contains("docker run")), "nothing went into the VM");
    run.down(&mut |_| {}).await.unwrap();
    assert!(f.model().vm.is_none());
}
