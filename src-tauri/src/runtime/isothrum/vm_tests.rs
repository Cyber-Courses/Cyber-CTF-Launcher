//! A VM lab through the runner, against a scripted Vagrant (the runner's fake processes): no real
//! hypervisor runs. The lab's run manifest is the one Isoloom generates for the `vagrant` target.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use isothrum_runner::process::fake::{Call, FakeProcesses, Reply};
use isothrum_runner::{FakePorts, ProcessRunner};

use super::*;
use crate::runtime::Park;

const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.31.0.0/24 }\ninputs: [CTF_FLAG_MAIN]\nmachines:\n  web:\n    os: debian@12\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http, publish: 8086 } }\n    files: [{ path: /srv/flag, input: CTF_FLAG_MAIN }]\n    app: { steps: [{ run: web.sh }] }\nchecks:\n  - { http: \"http://web/\" }\nmessage: |\n  Open http://localhost:{{ machines.web.ports.web.published }}/\n";

const VMS: [&str; 2] = ["web", "isoloom-controller"];

/// An installed VM lab (`<labs>/web`) with its files generated, a runner home, and a model of its
/// VMs' states.
struct Fixture {
    root: std::path::PathBuf,
    dir: std::path::PathBuf,
    states: Arc<Mutex<BTreeMap<String, String>>>,
    fake: Arc<FakeProcesses>,
}

impl Fixture {
    fn new() -> Fixture {
        let root = std::env::temp_dir().join(format!("cyberctf-isothrum-vm-{}", rand::random::<u32>()));
        let dir = root.join("labs").join("web");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("isoloom.yml"), SPEC).unwrap();
        std::fs::write(dir.join("web.sh"), "true\n").unwrap();
        lab::prepare(&dir, lab::Target::Vagrant).unwrap();
        let states = Arc::new(Mutex::new(BTreeMap::new()));
        let script = Arc::clone(&states);
        let fake = Arc::new(FakeProcesses::new(move |call| answer(&script, call)));
        Fixture { root, dir, states, fake }
    }

    fn run(&self) -> LabRun {
        let config = RunnerConfig::new(&self.root).with_processes(Arc::clone(&self.fake) as Arc<dyn ProcessRunner>).with_ports(Arc::new(FakePorts::new([])));
        LabRun::with(config, &self.dir, "web", Kind::Vm).unwrap()
    }

    fn state(&self, vm: &str) -> String {
        self.states.lock().unwrap_or_else(PoisonError::into_inner).get(vm).cloned().unwrap_or_else(|| "not_created".into())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The fake Vagrant's answer to `call`.
fn answer(states: &Mutex<BTreeMap<String, String>>, call: &Call) -> Reply {
    let mut s = states.lock().unwrap_or_else(PoisonError::into_inner);
    let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
    let set = |s: &mut BTreeMap<String, String>, vms: &[&str], state: &str| vms.iter().for_each(|vm| drop(s.insert((*vm).to_owned(), state.to_owned())));
    match (call.program.as_str(), args.as_slice()) {
        ("vagrant", ["status", ..]) => {
            Reply::ok(&VMS.iter().map(|vm| format!("1,{vm},state,{}", s.get(*vm).map_or("not_created", String::as_str))).collect::<Vec<_>>().join("\n"))
        }
        ("vagrant", ["up", "--provider", _]) => {
            set(&mut s, &VMS, "running");
            Reply::ok(&format!("==> web: Machine booted and ready!\nflag {}", call.env("CTF_FLAG_MAIN").unwrap_or("none")))
        }
        ("vagrant", ["up", "--no-provision", "--provider", _, vms @ ..] | ["up", vms @ .., "--no-provision"]) => {
            set(&mut s, vms, "running");
            Reply::ok("")
        }
        ("vagrant", ["halt", vm]) => {
            set(&mut s, &[vm], "poweroff");
            Reply::ok("")
        }
        ("vagrant", [verb @ ("halt" | "suspend")]) => {
            let to = if *verb == "halt" { "poweroff" } else { "saved" };
            s.values_mut().filter(|v| *v == "running").for_each(|v| *v = to.into());
            Reply::ok("")
        }
        ("vagrant", ["destroy", "--force"]) => {
            s.clear();
            Reply::ok("")
        }
        ("vagrant", ["provision", "isoloom-controller", "--provision-with", "checks"]) => {
            Reply::ok("isoloom-check: PASS http://web/\nisoloom-check: END 1 passed, 0 failed")
        }
        ("vagrant", ["ssh", ..]) => Reply::fail(1, ""),
        _ => Reply::ok(""),
    }
}

#[tokio::test]
async fn a_vm_lab_goes_up_on_its_hypervisor_through_the_runner() {
    let f = Fixture::new();
    let run = f.run();
    let mut log = Vec::new();
    run.up(&[("CTF_FLAG_MAIN".into(), "flag{v}".into())], Some("virtualbox"), &mut |l| log.push(l)).await.unwrap();
    let up = f.fake.last("vagrant up --provider virtualbox").unwrap();
    assert_eq!(up.env("CTF_FLAG_MAIN"), Some("flag{v}"), "the launch's input reaches its slot");
    assert!(up.env("VAGRANT_DOTFILE_PATH").is_some_and(|p| p.starts_with(&f.root.join("runs").display().to_string())), "Vagrant's record in the run's state");
    assert!(log.iter().any(|l| l == "flag ***"), "masked: {log:?}");
    assert_eq!(log.last().map(String::as_str), Some("Open http://localhost:8086/"));
    assert_eq!(f.state("isoloom-controller"), "poweroff", "the controller is halted once the lab is built");
    let s = run.status().await.unwrap();
    assert!(s.running && matches!(s.place, Some(Place::LocalVm)));
    assert_eq!(s.url.as_deref(), Some("http://127.0.0.1:8086/"));
    assert!(s.machines.iter().find(|m| m.name == "isoloom-controller").is_some_and(|m| m.infra));
}

#[tokio::test]
async fn a_vm_lab_is_checked_parked_resumed_reprovisioned_and_stopped() {
    let f = Fixture::new();
    let run = f.run();
    run.up(&[], Some("virtualbox"), &mut |_| {}).await.unwrap();
    let c = run.check().await.unwrap();
    assert!(c.available && c.ok, "{}", c.output);
    run.park(Park::Pause, &mut |_| {}).await.unwrap();
    assert_eq!(f.state("web"), "saved", "paused: its state saved");
    paths::mark_parked(&f.dir, Some(Park::Pause)).unwrap();
    assert_eq!(run.status().await.unwrap().parked, Some(Park::Pause));
    run.resume(&mut |_| {}).await.unwrap();
    assert_eq!((f.state("web").as_str(), f.state("isoloom-controller").as_str()), ("running", "poweroff"));
    run.park(Park::Shutdown, &mut |_| {}).await.unwrap();
    assert_eq!(f.state("web"), "poweroff");
    run.resume(&mut |_| {}).await.unwrap();
    run.provision(Some("web"), &mut |_| {}).await.unwrap();
    assert!(f.fake.last("vagrant provision web").is_some());
    run.down(&mut |_| {}).await.unwrap();
    assert_eq!(f.state("web"), "not_created");
    assert!(!run.status().await.unwrap().running);
}
