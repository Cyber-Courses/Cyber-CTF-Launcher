//! A VM lab through the runner, against a scripted Vagrant (the runner's fake processes): no real
//! hypervisor runs. The lab's run manifest is the one Isoloom generates for the `vagrant` target.

use std::collections::BTreeMap;

use super::test_lab::prelude::*;
use super::*;

const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.31.0.0/24 }\ninputs: [CTF_FLAG_MAIN]\nmachines:\n  web:\n    os: debian@12\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http, publish: 8086 } }\n    files: [{ path: /srv/flag, input: CTF_FLAG_MAIN }]\n    app: { steps: [{ run: web.sh }] }\nchecks:\n  - { http: \"http://web/\" }\nmessage: |\n  Open http://localhost:{{ machines.web.ports.web.published }}/\n";

const VMS: [&str; 2] = ["web", "isoloom-controller"];

/// An installed VM lab (`<labs>/web`) with its files generated, a runner home, and a model of its
/// VMs' states.
type Fixture = Scripted<BTreeMap<String, String>>;

impl Fixture {
    fn new() -> Fixture {
        Scripted::answered_by(Home::new("-vm", SPEC, &[("web.sh", "true\n")], lab::Target::Vagrant), answer)
    }

    fn run(&self) -> LabRun {
        LabRun::with(self.home.config(&self.fake, []), &self.home.dir, "web", Kind::Vm).unwrap()
    }

    /// The lab on an ESXi host, `place`, run from a Linux host (no relay: the runner's own tests
    /// cover macOS's).
    fn esxi_run(&self, place: EsxiPlace) -> LabRun {
        let mut config = self.home.config(&self.fake, []);
        config.host = isoloom_manifest::HostOs::Linux;
        LabRun::with(config, &self.home.dir, "web", Kind::Vm).unwrap().on(place)
    }

    fn state(&self, vm: &str) -> String {
        self.model().get(vm).cloned().unwrap_or_else(|| "not_created".into())
    }
}

/// The fake Vagrant's answer to `call`.
fn answer(states: &Mutex<BTreeMap<String, String>>, call: &Call) -> Reply {
    vagrant(&mut locked(states), call)
}

/// The fake Vagrant's answer to `call`, from the VMs' `states`.
fn vagrant(s: &mut BTreeMap<String, String>, call: &Call) -> Reply {
    let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
    let set = |s: &mut BTreeMap<String, String>, vms: &[&str], state: &str| vms.iter().for_each(|vm| drop(s.insert((*vm).to_owned(), state.to_owned())));
    match (call.program.as_str(), args.as_slice()) {
        ("vagrant", ["status", ..]) => {
            Reply::ok(&VMS.iter().map(|vm| format!("1,{vm},state,{}", s.get(*vm).map_or("not_created", String::as_str))).collect::<Vec<_>>().join("\n"))
        }
        ("vagrant", ["up", "--provider", _]) => {
            set(s, &VMS, "running");
            Reply::ok(&format!("==> web: Machine booted and ready!\nflag {}", call.env("CTF_FLAG_MAIN").unwrap_or("none")))
        }
        ("vagrant", ["up", "--no-provision", "--provider", _, vms @ ..] | ["up", vms @ .., "--no-provision"]) => {
            set(s, vms, "running");
            Reply::ok("")
        }
        // Every running VM, or the ones named (a pause suspends only the lab's own VMs).
        ("vagrant", [verb @ ("halt" | "suspend"), vms @ ..]) => {
            let to = if *verb == "halt" { "poweroff" } else { "saved" };
            s.iter_mut().filter(|(vm, v)| *v == "running" && (vms.is_empty() || vms.contains(&vm.as_str()))).for_each(|(_, v)| *v = to.into());
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
        // ESXi: its plugin, and each VM at the address its VMware Tools report.
        ("vagrant", ["plugin", "list", ..]) => Reply::ok("1,,plugin-name,vagrant-vmware-esxi"),
        ("vagrant", ["ssh-config", vm]) => Reply::ok(&format!("Host {vm}\n  HostName 172.16.1.53\n  User vagrant\n")),
        _ => Reply::ok(""),
    }
}

/// The VMs' states, and until when the start's `vagrant up` holds their locks (booting and
/// provisioning them).
#[derive(Default)]
struct Provisioning {
    states: BTreeMap<String, String>,
    held_until: Option<tokio::time::Instant>,
    /// The `vagrant` calls made while the start's `vagrant up` held the VMs.
    while_held: Vec<String>,
}

/// How long the start's `vagrant up` takes.
const UP_TAKES: std::time::Duration = std::time::Duration::from_secs(600);

/// The fake Vagrant's answer to `call`, refusing `vagrant status` as Vagrant does while the
/// start's `vagrant up` provisions the VMs (`MachineActionLockedError`).
fn provisioning(model: &Mutex<Provisioning>, call: &Call) -> Reply {
    let mut m = locked(model);
    let held = m.held_until.is_some_and(|until| tokio::time::Instant::now() < until);
    if held {
        m.while_held.push(format!("{} {}", call.program, call.args.join(" ")));
    }
    match call.args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["status", ..] if held => {
            let mut refused = Reply::fail(1, "");
            refused.stdout = vec![
                "1794331884,,ui,error,An action 'read_state' was attempted on the machine 'web'%!(VAGRANT_COMMA)\\nbut another process is already executing an action on the machine.".into(),
                "1794331884,,error-exit,Vagrant::Errors::MachineActionLockedError,An action 'read_state' was attempted on the machine 'web'".into(),
            ];
            refused
        }
        ["up", "--provider", _] => {
            m.held_until = Some(tokio::time::Instant::now() + UP_TAKES);
            let mut up = vagrant(&mut m.states, call);
            up.delay = UP_TAKES;
            up
        }
        _ => vagrant(&mut m.states, call),
    }
}

/// Regression: the lab page polls the lab's status from the app while its start runs in the
/// deploy worker, another process. Two minutes into the start's provisioning, Vagrant refused the
/// status read (the start's `vagrant up` held the VMs' locks), the runner's retries ran out, and
/// the page showed "Couldn't read the state of" the lab. The read now says it isn't running yet,
/// as the start's own progress does. Regression (friday, libvirt): the app's reads ran `vagrant
/// status` on the VMs the start was provisioning, and took their locks in Vagrant's index from
/// under the start's own Vagrant calls: neither the lab page's nor the attack box's read runs any
/// Vagrant while the start holds the run, only the start does.
#[tokio::test(start_paused = true)]
async fn the_lab_page_reads_a_vm_lab_while_its_start_provisions_it_in_another_process() {
    let f: Scripted<Provisioning> = Scripted::answered_by(Home::new("-vm-busy", SPEC, &[("web.sh", "true\n")], lab::Target::Vagrant), provisioning);
    // The deploy worker's runner, which starts the lab, and the app's, which the page reads it
    // through: the lab's status, and the attack box's (the run's status, the box one of its machines).
    let worker = LabRun::with(f.home.config(&f.fake, []), &f.home.dir, "web", Kind::Vm).unwrap();
    let app = LabRun::with(f.home.config(&f.fake, []), &f.home.dir, "web", Kind::Vm).unwrap();
    let page = async {
        tokio::time::sleep(std::time::Duration::from_secs(148)).await;
        tokio::join!(app.status(), app.run_status())
    };
    let mut log = |_| {};
    let (started, (read, boxed)) = tokio::join!(worker.up(Inputs { values: &[], default_ports: true }, Some("virtualbox"), &mut log), page);
    started.unwrap();
    let read = read.unwrap();
    assert!(!read.running, "not up yet");
    assert!(matches!(read.place, Some(Place::LocalVm)));
    let boxed = boxed.unwrap().expect("the lab's files exist");
    assert!(!boxed.live, "as last read, not asked of Vagrant");
    let held = f.model().while_held.clone();
    assert!(held.is_empty(), "no Vagrant ran while the start's `vagrant up` held the VMs: {held:?}");
    // Once the start is done, the page reads it running.
    assert!(app.status().await.unwrap().running);
}

/// The VMs' states, and the `vagrant status` under way, and the most ever at once.
#[derive(Default)]
struct Reads {
    states: BTreeMap<String, String>,
    /// When each `vagrant status` under way ends.
    ending: Vec<tokio::time::Instant>,
    most: usize,
}

/// How long a `vagrant status` takes (fog's libvirt connection, a busy VirtualBox).
const READ_TAKES: std::time::Duration = std::time::Duration::from_secs(3);

/// The fake Vagrant's answer to `call`, each `vagrant status` taking [`READ_TAKES`], counted with
/// those under way.
fn timed_reads(model: &Mutex<Reads>, call: &Call) -> Reply {
    let mut m = locked(model);
    if call.args.first().is_some_and(|a| a == "status") {
        let now = tokio::time::Instant::now();
        m.ending.retain(|end| *end > now);
        m.ending.push(now + READ_TAKES);
        m.most = m.most.max(m.ending.len());
        let mut read = vagrant(&mut m.states, call);
        read.delay = READ_TAKES;
        return read;
    }
    vagrant(&mut m.states, call)
}

/// Regression (friday, libvirt): at the end of a start, the lab page read the lab's status and
/// the attack box's auto-start read it too; each asked the runner on its own, two `vagrant
/// status` of the same VMs ran at once, Vagrant refused one (`MachineLocked`: each read takes the
/// VMs' locks in its index without waiting), and the box "couldn't be added with the lab". The
/// app's reads of one run take turns: never two Vagrant reads of it at once.
#[tokio::test(start_paused = true)]
async fn the_apps_reads_of_a_vm_lab_never_ask_vagrant_at_once() {
    let f: Scripted<Reads> = Scripted::answered_by(Home::new("-vm-reads", SPEC, &[("web.sh", "true\n")], lab::Target::Vagrant), timed_reads);
    let worker = LabRun::with(f.home.config(&f.fake, []), &f.home.dir, "web", Kind::Vm).unwrap();
    worker.up(Inputs { values: &[], default_ports: true }, Some("virtualbox"), &mut |_| {}).await.unwrap();
    f.model().most = 0;
    // The lab page's poll, its read after the start, the auto-start's read, and the attack box's:
    // each command opens the run on its own.
    let opened = || LabRun::with(f.home.config(&f.fake, []), &f.home.dir, "web", Kind::Vm).unwrap();
    let (poll, after, auto, boxed) = (opened(), opened(), opened(), opened());
    let (polled, read_after, auto_read, box_read) = tokio::join!(poll.status(), after.status(), auto.status(), boxed.run_status());
    assert!(polled.unwrap().running && read_after.unwrap().running && auto_read.unwrap().running);
    assert!(box_read.unwrap().is_some_and(|s| s.live));
    assert_eq!(f.model().most, 1, "never two `vagrant status` of the run at once");
}

/// The attack box's shared folder needs the lab's hypervisor: the run's record says it, no
/// `vagrant status` asked for it.
#[tokio::test]
async fn a_vm_labs_hypervisor_is_read_from_its_record_without_vagrant() {
    let f = Fixture::new();
    let run = f.run();
    assert_eq!(run.recorded_provider().unwrap(), None, "never started");
    run.up(Inputs { values: &[], default_ports: true }, Some("virtualbox"), &mut |_| {}).await.unwrap();
    let before = f.fake.calls().len();
    assert_eq!(run.recorded_provider().unwrap().as_deref(), Some("virtualbox"));
    assert_eq!(f.fake.calls().len(), before, "no tool ran");
}

#[tokio::test]
async fn a_vm_lab_goes_up_on_its_hypervisor_through_the_runner() {
    let f = Fixture::new();
    let run = f.run();
    let mut log = Vec::new();
    // On the lab's own ports: the fixture's host has no free port to pick instead.
    let values = [("CTF_FLAG_MAIN".into(), "flag{v}".into())];
    run.up(Inputs { values: &values, default_ports: true }, Some("virtualbox"), &mut |l| log.push(l)).await.unwrap();
    let up = f.fake.last("vagrant up --provider virtualbox").unwrap();
    assert_eq!(up.env("CTF_FLAG_MAIN"), Some("flag{v}"), "the launch's input reaches its slot");
    assert!(
        up.env("VAGRANT_DOTFILE_PATH").is_some_and(|p| p.starts_with(&f.home.root.join("runs").display().to_string())),
        "Vagrant's record in the run's state"
    );
    assert!(log.iter().any(|l| l == "flag ***"), "masked: {log:?}");
    assert_eq!(log.last().map(String::as_str), Some("Open http://localhost:8086/"));
    assert_eq!(f.state("isoloom-controller"), "poweroff", "the controller is halted once the lab is built");
    let s = run.status().await.unwrap();
    assert!(s.running && matches!(s.place, Some(Place::LocalVm)));
    assert_eq!(s.provider.as_deref(), Some("virtualbox"), "the page knows the hypervisor (Pause hangs on it)");
    assert_eq!(s.url.as_deref(), Some("http://127.0.0.1:8086/"));
    assert!(s.machines.iter().find(|m| m.name == "isoloom-controller").is_some_and(|m| m.infra));
}

#[tokio::test]
async fn a_vm_lab_is_checked_parked_resumed_reprovisioned_and_stopped() {
    let f = Fixture::new();
    let run = f.run();
    run.up(Inputs { values: &[], default_ports: true }, Some("virtualbox"), &mut |_| {}).await.unwrap();
    let c = run.check().await.unwrap();
    assert!(c.available && c.ok, "{}", c.output);
    run.park(Park::Pause, &mut |_| {}).await.unwrap();
    assert_eq!(f.state("web"), "saved", "paused: its state saved");
    paths::mark_parked(&f.home.dir, Some(Park::Pause)).unwrap();
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

/// An ESXi server's settings as the launcher keeps them for a lab (`lab::esxi_env`'s names, and
/// the launcher's own).
fn esxi_env() -> Vec<(String, String)> {
    [
        ("CYBERCTF_REMOTE_HOST", "172.16.1.20"),
        ("CYBERCTF_ESXI_PASSWORD", "pw"),
        ("ESXI_HOSTNAME", "172.16.1.20"),
        ("ESXI_HOSTPORT", "22"),
        ("ESXI_USERNAME", "root"),
        ("ESXI_PASSWORD", "pw"),
        ("ESXI_DATASTORE", "datastore1"),
        ("ESXI_VIRTUAL_NETWORK", "VM Network"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
}

#[test]
fn an_esxi_place_takes_the_hosts_settings_as_params_and_its_password_as_the_connection() {
    let place = EsxiPlace::from_env("esxi-lab", "ESXi lab", &esxi_env());
    assert_eq!(place.label, "esxi-lab");
    let names: Vec<&str> = place.params.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(names, ["ESXI_HOSTNAME", "ESXI_HOSTPORT", "ESXI_USERNAME", "ESXI_DATASTORE", "ESXI_VIRTUAL_NETWORK"]);
    assert_eq!(place.connection().get("ESXI_PASSWORD").map(SecretValue::expose), Some("pw"));
    assert!(!format!("{place:?}").contains("pw\""), "never shown: {place:?}");
    // A server with no datastore or network set: those params are left to their defaults.
    let bare: Vec<(String, String)> = esxi_env().into_iter().filter(|(k, _)| k != "ESXI_DATASTORE" && k != "ESXI_VIRTUAL_NETWORK").collect();
    assert_eq!(EsxiPlace::from_env("esxi-lab", "ESXi lab", &bare).params.len(), 3);
}

#[tokio::test]
async fn a_vm_lab_runs_on_an_esxi_host_through_the_runner() {
    let f = Fixture::new();
    let run = f.esxi_run(EsxiPlace::from_env("esxi-lab", "ESXi lab", &esxi_env()));
    let mut log = Vec::new();
    run.up(Inputs::values(&[("CTF_FLAG_MAIN".into(), "flag{v}".into())]), Some("vmware_esxi"), &mut |l| log.push(l)).await.unwrap();
    let up = f.fake.last("vagrant up --provider vmware_esxi").unwrap();
    for (name, value) in [("ESXI_HOSTNAME", "172.16.1.20"), ("ESXI_DATASTORE", "datastore1"), ("ESXI_VIRTUAL_NETWORK", "VM Network"), ("ESXI_PASSWORD", "pw")] {
        assert_eq!(up.env(name), Some(value), "{name}");
    }
    assert_eq!(up.env("CYBERCTF_ESXI_PASSWORD"), None, "only what the manifest's slots take");
    let s = run.status().await.unwrap();
    assert!(s.running, "{:?}", s.machines.iter().map(|m| (&m.name, &m.state)).collect::<Vec<_>>());
    assert_eq!(s.url.as_deref(), Some("http://172.16.1.53:80/"), "ESXi forwards nothing: the VM's own address");
    let c = run.check().await.unwrap();
    assert!(c.available && c.ok, "{}", c.output);
    run.park(Park::Shutdown, &mut |_| {}).await.unwrap();
    assert_eq!(f.state("web"), "poweroff");
    run.resume(&mut |_| {}).await.unwrap();
    assert_eq!(f.state("web"), "running");
    run.down(&mut |_| {}).await.unwrap();
    assert_eq!(f.state("web"), "not_created");
    // The password reached every Vagrant command, the status reads and the park included.
    for call in f.fake.calls().iter().filter(|c| c.program == "vagrant" && c.args != ["--version"]) {
        assert_eq!(call.env("ESXI_PASSWORD"), Some("pw"), "{}", call.line());
    }
}

/// The fake Vagrant, whose ESXi host refuses its password once the model holds `password changed`.
fn esxi_refusing(states: &Mutex<BTreeMap<String, String>>, call: &Call) -> Reply {
    let mut s = locked(states);
    if s.contains_key("password changed") && call.program == "vagrant" && call.args.first().is_some_and(|a| a != "--version") {
        return Reply::fail(
            1,
            "ESXi host access : Verify password in Vagrantfile is correct.\nUnable to connect to ESXi host! Error: Authentication failed for user root@172.16.1.20",
        );
    }
    vagrant(&mut s, call)
}

/// Regression (tests/resilience.md N4): an ESXi host's password changed showed as Vagrant's raw
/// failure; the lab page now says to update it in Servers, naming the server as Servers does.
#[tokio::test]
async fn an_esxi_password_changed_says_to_update_it_in_servers() {
    let f: Fixture = Scripted::answered_by(Home::new("-vm", SPEC, &[("web.sh", "true\n")], lab::Target::Vagrant), esxi_refusing);
    let run = f.esxi_run(EsxiPlace::from_env("esxi-lab", "ESXi lab", &esxi_env()));
    run.up(Inputs::values(&[("CTF_FLAG_MAIN".into(), "flag{v}".into())]), Some("vmware_esxi"), &mut |_| {}).await.unwrap();
    f.model().insert("password changed".into(), String::new());
    let err = run.status().await.err().unwrap();
    assert_eq!(err.code(), crate::error::Code::ServerAuthFailed { server: "ESXi lab".into() });
    assert_eq!(err.to_string(), "Cyber CTF can't sign in to ESXi lab: its token was revoked or its password changed. Update it in Servers.");
    let stop = run.down(&mut |_| {}).await.unwrap_err();
    assert_eq!(stop.code(), crate::error::Code::ServerAuthFailed { server: "ESXi lab".into() });
}
