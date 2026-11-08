//! A container lab in a local VM (Isoloom's docker-vm) through the runner, against a scripted
//! Vagrant on UTM and a scripted Docker inside the VM (the runner's fake processes): no real
//! hypervisor runs. The lab's run manifest is the one Isoloom generates for the `docker-vm`
//! target. What the lab page relies on: the lab reads as running once UTM's plugin says
//! `started`, and its attack box is a container the runner stands up inside the VM.

use super::test_lab::prelude::*;
use super::*;

const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http, publish: 8080 } }\n    app: { image: nginx:1.27 }\nchecks:\n  - { http: \"http://web/\" }\n";

/// The address the fake Docker gives the attack box on the lab network.
const BOX_IP: &str = "10.30.0.77";

/// What UTM's Vagrant plugin calls a running VM.
const STARTED: &str = "started";

/// `docker compose ps --all --format json` of the started lab, one object a line.
const PS: &str = r#"{"Service":"web","State":"running","Health":"healthy","Publishers":[{"PublishedPort":8080,"TargetPort":80,"Protocol":"tcp"}],"Labels":""}"#;

/// The helper VM, the lab's containers and the attack box in it.
#[derive(Debug, Default)]
struct Engine {
    /// The VM's state label (absent: `not_created`).
    vm: Option<&'static str>,
    /// Whether Compose started the lab in it.
    up: bool,
    /// The attack box: its image, and the Docker networks it is on.
    attacker: Option<(String, Vec<String>)>,
    /// How many of the next `docker run`s fail with UTM's passing scripting error.
    run_fails: u32,
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

    /// The `docker run` calls into the VM so far.
    fn runs(&self) -> Vec<Call> {
        self.fake.calls().into_iter().filter(|c| c.line().contains("docker run -d")).collect()
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

/// What the VM answers to `command` over `vagrant ssh`: Compose for the lab, Docker for the box.
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
    if let Some(rest) = command.strip_prefix("sudo docker ") {
        return docker(e, rest);
    }
    Reply::ok("")
}

/// The single-quoted words of a `docker` command the runner wrote.
fn quoted(command: &str) -> Vec<&str> {
    command.split('\'').skip(1).step_by(2).collect()
}

/// What root's Docker in the VM answers to `docker <rest>`: the box's run, removal and inspects.
fn docker(e: &mut Engine, rest: &str) -> Reply {
    let words = quoted(rest);
    let networks_json = |nets: &[String]| {
        let each: Vec<String> = nets.iter().map(|n| format!(r#""{n}":{{"IPAddress":"{BOX_IP}"}}"#)).collect();
        format!("{{{}}}", each.join(","))
    };
    if rest.starts_with("run -d --name ") {
        if e.run_fails > 0 {
            e.run_fails -= 1;
            return Reply::fail(1, "execution error: UTM got an error: Application isn't running. (-600)");
        }
        let [_, net, image] = words[..] else { panic!("{rest}") };
        e.attacker = Some((image.to_owned(), vec![net.to_owned()]));
        return Reply::ok("0123456789ab");
    }
    if rest.starts_with("rm -f ") {
        return match e.attacker.take() {
            Some(_) => Reply::ok(""),
            None => Reply::fail(1, "Error response from daemon: No such container"),
        };
    }
    if rest.starts_with("inspect -f ") {
        let Some((_, nets)) = &e.attacker else {
            return Reply::fail(1, "Error: No such object");
        };
        let json = networks_json(nets);
        return Reply::ok(&if rest.contains("{{.State.Running}}|") { format!("true|{json}") } else { json });
    }
    panic!("a docker command the fake doesn't model: {rest}")
}

#[tokio::test]
async fn a_container_lab_in_a_utm_vm_gets_its_attack_box_inside_the_vm() {
    let f = Fixture::new();
    let run = f.run();
    run.up(&[], Some("utm"), &mut |_| {}).await.unwrap();
    let s = run.status().await.unwrap();
    assert!(s.running && matches!(s.place, Some(Place::LocalVm)), "UTM's `started` reads as running (running={})", s.running);

    let before = run.attack_status(None).await.unwrap();
    assert!(!before.running && before.ip.is_empty(), "{before:?}");
    assert_eq!(before.networks, ["lab"], "the lab network the box joins is known before it is up");
    assert!(before.shell_cmd.contains("VAGRANT_DOTFILE_PATH="), "the shell line carries the run's state folder: {}", before.shell_cmd);

    run.attach(None, Some("kalilinux/kali-rolling"), &mut |_| {}).await.unwrap();
    let started = f.runs().pop().unwrap();
    assert_eq!(started.program, "vagrant");
    assert_eq!(started.args[..3], ["ssh", "--no-tty", "--command"]);
    assert_eq!(started.args[4], "default", "into the lab's VM");
    let docker_run = &started.args[3];
    assert!(
        docker_run.starts_with("sudo docker run -d --name 'cyberctf-web-isoloom-access' --hostname attackbox --label isoloom.access=true --network '"),
        "{docker_run}"
    );
    assert!(docker_run.ends_with("_lab' 'kalilinux/kali-rolling' sleep infinity"), "on the lab's network there: {docker_run}");
    assert!(started.env("VAGRANT_DOTFILE_PATH").is_some_and(|p| p.starts_with(&f.home.root.join("runs").display().to_string())), "{}", started.line());

    let after = run.attack_status(None).await.unwrap();
    assert_eq!((after.running, after.ip.as_str(), after.lab_network.as_str()), (true, BOX_IP, "lab"));
    let shell = run.shell_command().unwrap();
    assert_eq!(shell.program, "vagrant");
    assert_eq!(shell.args[..3], ["ssh", "default", "-c"]);
    assert!(shell.args[3].starts_with("sudo docker exec -it 'cyberctf-web-isoloom-access' sh -c '"), "{}", shell.args[3]);
    assert!(shell.env.iter().any(|(k, v)| k == "VAGRANT_DOTFILE_PATH" && v.starts_with(&f.home.root.join("runs").display().to_string())), "{:?}", shell.env);

    run.detach(&mut |_| {}).await.unwrap();
    assert!(f.fake.last("docker rm -f 'cyberctf-web-isoloom-access'").is_some());
    assert!(!run.attack_status(None).await.unwrap().running, "the box is gone");
    run.down(&mut |_| {}).await.unwrap();
    assert!(f.model().vm.is_none());
}

#[tokio::test]
async fn an_attach_into_a_utm_vm_is_tried_once_more_on_utms_passing_scripting_error() {
    let f = Fixture::new();
    let run = f.run();
    run.up(&[], Some("utm"), &mut |_| {}).await.unwrap();
    f.model().run_fails = 1;
    let mut log = Vec::new();
    run.attach(None, None, &mut |l| log.push(l)).await.unwrap();
    assert_eq!(f.runs().len(), 2, "once more after the -600");
    assert!(log.iter().any(|l| l == UTM_TRANSIENT_RETRY), "{log:?}");
    assert!(run.attack_status(None).await.unwrap().running);
    // Twice is a failure of its own, reported as it is: no third try.
    run.detach(&mut |_| {}).await.unwrap();
    f.model().run_fails = 2;
    let runs = f.runs().len();
    let err = run.attach(None, None, &mut |_| {}).await.unwrap_err().to_string();
    assert!(err.contains("(-600)"), "{err}");
    assert_eq!(f.runs().len(), runs + 2, "two tries, no more");
}

#[tokio::test]
async fn the_attack_box_needs_the_lab_running_in_its_vm() {
    let f = Fixture::new();
    let run = f.run();
    let not_started = run.attach(None, None, &mut |_| {}).await.unwrap_err();
    assert_eq!(not_started.to_string(), "This lab isn't running. Start it first.");
    run.up(&[], Some("utm"), &mut |_| {}).await.unwrap();
    run.park(Park::Pause, &mut |_| {}).await.unwrap();
    let paused = run.attach(None, None, &mut |_| {}).await.unwrap_err();
    assert_eq!(paused.to_string(), "This lab isn't running. Start it first.");
    assert!(f.runs().is_empty(), "nothing went into the VM");
}
