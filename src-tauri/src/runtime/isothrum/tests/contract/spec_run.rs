//! The launcher's join of a lab's spec with its run: Isoloom's own example labs, read from an
//! Isoloom checkout and generated with Isoloom's library for Compose and for Vagrant, run by
//! Isothrum's runner (through its service, as the app runs them) on fake tools that do what the
//! generated files say, and the lab page the launcher draws from the two (`view`, `described`).
//! Isoloom and the runner know nothing of each other; the launcher alone knows both, so only here
//! does a change on either side show: the page's entry is where the run published the lab's web
//! port, each port is where the runner says it is, a VM run's kept host ports reach the
//! Vagrantfile under the names it reads (`HOST_PORT_*`), the lab's message is filled, and the page
//! offers the parks the run can do.
//!
//! The examples come from `CYBERCTF_TEST_ISOLOOM_DIR`, else the Isoloom checkout beside this one
//! (`../isoloom`, the workspace's and `./ws`'s layout); without either the tests fail, saying so,
//! rather than pass on nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use isothrum_runner::fake::FakeProcesses;
use isothrum_runner::{FakePorts, ProcessRunner, RunnerConfig};
use serde_json::{Value, json};

use super::super::*;
use crate::runtime::LabStatus;

/// Isoloom's examples these tests run as containers: a web page and a cache (a message naming the
/// page's URL), an API on a fixed host port with launch inputs, a DNS server published over UDP
/// (a message naming its published port), and one web server with no message.
const EXAMPLES: [&str; 4] = ["hello-stack", "invoice-portal-api", "udp-dns", "solo-web"];

/// The ones run as VMs. `udp-dns` is left out, a known gap: `vagrant port` doesn't say a
/// forward's protocol, so the runner reads its UDP forward as TCP (the page shows its TCP port 53
/// published) and gives its host port back as `HOST_PORT_DNS_53_TCP`, while the Vagrantfile reads
/// `HOST_PORT_DNS_53_UDP`: a UDP port Vagrant moved moves again at the next boot.
const VM_EXAMPLES: [&str; 3] = ["hello-stack", "invoice-portal-api", "solo-web"];

/// The free host ports the runner picks from for a publish that names none.
const FREE: std::ops::Range<u16> = 41000..41064;

/// How far a fake Vagrant moves a forward started again without its host port given: Vagrant's
/// providers re-pick a port `auto_correct` moved at each boot.
const MOVED_BY: u16 = 10000;

/// Isoloom's examples folder.
fn examples() -> PathBuf {
    let dir = std::env::var_os("CYBERCTF_TEST_ISOLOOM_DIR")
        .map_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("isoloom"), PathBuf::from)
        .join("examples");
    assert!(
        dir.is_dir(),
        "Isoloom's examples aren't at {}: set CYBERCTF_TEST_ISOLOOM_DIR to an Isoloom checkout (the workspace's is ../isoloom)",
        dir.display()
    );
    dir
}

/// One example installed as a lab (its own folder copied, its files generated for `target`) under
/// a fresh app data folder, and the fake tools its run is answered by.
struct Installed {
    _root: crate::testing::TempDir,
    data: PathBuf,
    dir: PathBuf,
    id: &'static str,
    tools: Arc<FakeProcesses>,
    /// The fake Vagrant's VMs, for a VM lab.
    vms: Arc<Mutex<Vms>>,
}

impl Installed {
    fn new(id: &'static str, target: lab::Target) -> Installed {
        let root = crate::testing::temp_dir("spec-run");
        let data = root.join("data");
        let dir = data.join("labs").join(id);
        lab::copy_tree(&examples().join(id), &dir).unwrap();
        lab::prepare(&dir, target).unwrap_or_else(|e| panic!("{id}: Isoloom generates {target:?}: {e}"));
        let vms = Arc::new(Mutex::new(if target == lab::Target::Vagrant { Vms::of(&lab::vagrant_dir(&dir).join("Vagrantfile")) } else { Vms::default() }));
        let tools = Arc::new(if target == lab::Target::Vagrant {
            let vms = Arc::clone(&vms);
            FakeProcesses::model(move |call| test_lab::tool_version(call).unwrap_or_else(|| locked(&vms).answer(call)))
        } else {
            let engine = Mutex::new(Docker::default());
            FakeProcesses::model(move |call| test_lab::tool_version(call).unwrap_or_else(|| locked(&engine).answer(call)))
        });
        Installed { _root: root, data, dir, id, tools, vms }
    }

    /// The lab's run, on a runner (served in this process) whose tools are the fake ones.
    fn run(&self, kind: Kind) -> Deployment {
        let mut config = RunnerConfig::new(crate::runtime::paths::runner_home_in(&self.data));
        config.processes = Arc::clone(&self.tools) as Arc<dyn ProcessRunner>;
        config.ports = Arc::new(FakePorts::new(FREE.collect::<Vec<_>>()));
        Deployment::here_with(config, &self.dir, self.id, kind).unwrap()
    }

    /// The lab's message as the start's deploy log ends with it.
    fn message(&self, target: lab::Target) -> String {
        described::message(&self.dir, target).unwrap_or_default()
    }
}

/// A port the tool published: its machine, its port inside it, whether it is TCP, and its host
/// port.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    machine: String,
    port: u16,
    tcp: bool,
    host: u16,
}

/// The host port each of the lab page's machines' TCP ports is published on, by machine and port.
fn published(status: &LabStatus) -> BTreeMap<(String, u16), u16> {
    status.machines.iter().flat_map(|m| m.ports.iter().filter_map(|p| Some(((m.name.clone(), p.target.get()), p.published?.get())))).collect()
}

/// The host ports a message names (`127.0.0.1:<port>`, `localhost:<port>`, `-p <port>`).
fn ports_named(message: &str) -> Vec<u16> {
    let re = regex::Regex::new(r"(?:127\.0\.0\.1:|localhost:|-p )(\d+)").unwrap();
    re.captures_iter(message).filter_map(|c| c[1].parse().ok()).collect()
}

/// What the page must show of a run of `lab` that is up: the entry at its first web port's host
/// port, every port it publishes where the run says, the parks `parks`, and, given, the message
/// filled in and naming only ports the run published.
fn joined(lab: &Installed, status: &LabStatus, seen: &[Seen], message: Option<&str>, parks: &[Park]) {
    let id = lab.id;
    assert!(status.running, "{id}: running");
    let shown = published(status);
    for ((machine, port), host) in &shown {
        let at = seen.iter().find(|s| s.machine == *machine && s.port == *port && s.tcp).map(|s| s.host);
        assert_eq!(at, Some(*host), "{id}: the page shows {machine}'s port {port} where the tool published it: {seen:?}");
    }
    assert!(!shown.is_empty() || seen.iter().all(|s| !s.tcp), "{id}: the page shows the ports the tool published: {seen:?}");
    let web = status.machines.iter().flat_map(|m| m.services.iter().filter(|s| s.kind == "web").map(move |s| (m.name.clone(), s.ports[0]))).next();
    match web {
        Some((machine, port)) => {
            let host = shown.get(&(machine.clone(), port)).unwrap_or_else(|| panic!("{id}: {machine}'s web port {port} published: {shown:?}"));
            assert_eq!(status.url.as_deref(), Some(format!("http://127.0.0.1:{host}/").as_str()), "{id}: the entry is the web port's host port");
        }
        None => assert_eq!(status.url, None, "{id}: no web port, no entry"),
    }
    assert_eq!(status.parks, parks, "{id}: the parks the page offers");
    let Some(message) = message else { return };
    assert!(!message.contains("{{"), "{id}: the message is filled in: {message}");
    let published: Vec<u16> = seen.iter().map(|s| s.host).collect();
    for port in ports_named(message) {
        assert!(published.contains(&port), "{id}: the message names port {port}, which the run published ({published:?}): {message}");
    }
}

#[tokio::test]
async fn isoloom_examples_as_containers_join_their_run_on_the_lab_page() {
    let _tools = no_tools();
    for id in EXAMPLES {
        let lab = Installed::new(id, lab::Target::Docker);
        let run = lab.run(Kind::Docker);
        let mut log = Vec::new();
        run.up(Inputs { values: &[], plain: &[], default_ports: true, docker_context: None }, None, &mut |l| log.push(l)).await.unwrap();
        let message = lab.message(lab::Target::Docker);
        assert!(log.join("\n").ends_with(message.trim_end()), "{id}: the start ends with the lab's message: {log:?}");
        let status = run.status().await.unwrap();
        let seen = docker_published(&lab.tools);
        joined(&lab, &status, &seen, Some(&message), &[Park::Pause, Park::Shutdown]);
        run.park(Park::Pause, &mut |_| {}).await.unwrap();
        assert_eq!(run.status().await.unwrap().parked, Some(Park::Pause), "{id}");
        run.resume(&mut |_| {}).await.unwrap();
        assert_eq!(published(&run.status().await.unwrap()), published(&status), "{id}: a resumed run is where it was");
        run.down(&mut |_| {}).await.unwrap();
        // On ports picked at random, the runner pins each publish to a free port in an override of
        // its own (a port the spec keeps fixed stays), and the page follows it. The message isn't
        // checked there, a known gap: it is filled from the spec, so it names the lab's own ports,
        // not the run's.
        run.up(Inputs::default(), None, &mut |_| {}).await.unwrap();
        let pinned = docker_published(&lab.tools);
        assert!(pinned.iter().all(|p| FREE.contains(&p.host) || seen.contains(p)), "{id}: each publish pinned to a free port, or fixed: {pinned:?}");
        joined(&lab, &run.status().await.unwrap(), &pinned, None, &[Park::Pause, Park::Shutdown]);
        run.down(&mut |_| {}).await.unwrap();
    }
}

#[tokio::test]
async fn isoloom_examples_as_vms_join_their_run_on_the_lab_page_and_keep_their_ports() {
    let _tools = no_tools();
    for id in VM_EXAMPLES {
        let lab = Installed::new(id, lab::Target::Vagrant);
        let run = lab.run(Kind::Vm);
        let mut log = Vec::new();
        run.up(Inputs { values: &[], plain: &[], default_ports: true, docker_context: None }, Some("virtualbox"), &mut |l| log.push(l)).await.unwrap();
        let message = lab.message(lab::Target::Vagrant);
        assert!(log.join("\n").ends_with(message.trim_end()), "{id}: the start ends with the lab's message: {log:?}");
        let status = run.status().await.unwrap();
        let seen = locked(&lab.vms).seen();
        joined(&lab, &status, &seen, Some(&message), &[Park::Pause, Park::Shutdown]);
        // Parked and started again, the run gives each kept port back under the name the
        // Vagrantfile reads it from; one it didn't would move, as Vagrant re-picks it.
        run.park(Park::Shutdown, &mut |_| {}).await.unwrap();
        run.resume(&mut |_| {}).await.unwrap();
        let resumed = lab.tools.made("vagrant up").pop().expect("a resume starts the VMs");
        for (var, machine, port) in locked(&lab.vms).read() {
            let kept = seen.iter().find(|s| s.machine == machine && s.port == port).map(|s| s.host.to_string());
            assert_eq!(resumed.env(&var), kept.as_deref(), "{id}: the run gives {machine}'s port {port} back as {var}, which the Vagrantfile reads");
        }
        assert_eq!(locked(&lab.vms).seen(), seen, "{id}: no port moved");
        assert_eq!(published(&run.status().await.unwrap()), published(&status), "{id}: a resumed run is where it was");
        run.down(&mut |_| {}).await.unwrap();
    }
}

/// The host port each service's port got, as the fake engine last listed them.
fn docker_published(tools: &FakeProcesses) -> Vec<Seen> {
    let Some(ps) = tools.calls().into_iter().rev().find(|c| compose_verb(c).is_some_and(|(v, _)| v == "ps")) else { return Vec::new() };
    Docker::listing(&ps, true)
        .into_iter()
        .flat_map(|(machine, ports)| ports.into_iter().map(move |p| Seen { machine: machine.clone(), port: p.target, tcp: p.tcp, host: p.host }))
        .collect()
}

/// A Compose project as Docker runs it: what the runner asks of `docker compose` answered from
/// the files each call names (Isoloom's project and the runner's own override), so any example
/// runs on it.
#[derive(Default)]
struct Docker {
    containers: Containers,
}

/// One port a service publishes, as the engine lists it.
struct Published {
    target: u16,
    host: u16,
    tcp: bool,
}

impl Docker {
    fn answer(&mut self, call: &Call) -> Reply {
        compose_asked(call, self.containers).map_or_else(|answered| answered, |(verb, rest)| self.compose(call, verb, rest))
    }

    /// `docker compose … <verb> <rest>`.
    fn compose(&mut self, call: &Call, verb: &str, rest: &[String]) -> Reply {
        let has = |flag: &str| rest.iter().any(|a| a == flag);
        match verb {
            "config" if has("--services") => Reply::ok(&project(call).keys().cloned().collect::<Vec<_>>().join("\n")),
            "config" => Reply::ok(&config(call, rest).to_string()),
            "ps" => Reply::ok(&self.ps(call, has("--all"))),
            "up" | "start" | "unpause" => {
                self.containers = Containers::Up;
                Reply::ok("")
            }
            "stop" => {
                self.containers = Containers::Stopped;
                Reply::ok("")
            }
            "pause" => {
                self.containers = Containers::Paused;
                Reply::ok("")
            }
            "down" => {
                self.containers = Containers::None;
                Reply::ok("")
            }
            _ => Reply::ok(""),
        }
    }

    /// `ps --format json`: each running service, its ports where they are published.
    fn ps(&self, call: &Call, all: bool) -> String {
        let Some(state) = listed_state(self.containers, all) else { return String::new() };
        Self::listing(call, self.containers == Containers::Up)
            .into_iter()
            .map(|(service, ports)| {
                let publishers: Vec<Value> = ports
                    .iter()
                    .map(|p| json!({"URL": "127.0.0.1", "PublishedPort": p.host, "TargetPort": p.target, "Protocol": if p.tcp { "tcp" } else { "udp" }}))
                    .collect();
                json!({"Service": service, "State": state, "Health": "", "ExitCode": 0, "Publishers": publishers}).to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Each service the project starts, and its ports where they are published (none while it
    /// isn't running).
    fn listing(call: &Call, running: bool) -> Vec<(String, Vec<Published>)> {
        project(call)
            .into_iter()
            .map(|(name, service)| {
                let ports = service["ports"].as_array().into_iter().flatten().filter_map(|p| {
                    let target = u16::try_from(p["target"].as_u64()?).ok()?;
                    let host = p["published"].as_str().and_then(|h| h.parse().ok()).filter(|_| running)?;
                    Some(Published { target, host, tcp: p["protocol"].as_str() != Some("udp") })
                });
                (name, ports.collect())
            })
            .collect()
    }
}

/// The services a `docker compose` call's project starts (each file it names merged, the
/// profiles it names attached), as `config --format json` prints them: their ports in long form,
/// each variable filled from the call's environment.
fn project(call: &Call) -> serde_json::Map<String, Value> {
    let mut merged = Value::Null;
    let mut profiles = Vec::new();
    let mut args = call.args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-f" => merge(&mut merged, yaml(&std::fs::read_to_string(args.next().unwrap()).unwrap())),
            "--profile" => profiles.push(args.next().unwrap().clone()),
            _ => {}
        }
    }
    let services = merged["services"].as_object().cloned().unwrap_or_default();
    services
        .into_iter()
        .filter(|(_, s)| s["profiles"].as_array().is_none_or(|p| p.iter().any(|p| p.as_str().is_some_and(|p| profiles.iter().any(|q| q == p)))))
        .map(|(name, mut s)| {
            let ports: Vec<Value> = s["ports"].as_array().into_iter().flatten().map(|p| long_port(p, call)).collect();
            s["ports"] = Value::Array(ports);
            (name, s)
        })
        .collect()
}

/// `config --format json` of `call`'s project: its services, or the ones `rest` names.
fn config(call: &Call, rest: &[String]) -> Value {
    let named: Vec<&String> = rest.iter().filter(|a| !a.starts_with("--") && *a != "json").collect();
    let services: serde_json::Map<String, Value> = project(call).into_iter().filter(|(n, _)| named.is_empty() || named.contains(&n)).collect();
    json!({ "services": services })
}

/// A port as `config` prints it, from the file's short (`[ip:][host:]target[/proto]`) or long
/// form, its variables filled from `call`'s environment.
fn long_port(port: &Value, call: &Call) -> Value {
    if let Some(short) = port.as_str() {
        let short = interpolate(short, call);
        let (spec, protocol) = short.split_once('/').unwrap_or((&short, "tcp"));
        let parts: Vec<&str> = spec.rsplitn(3, ':').collect();
        let (target, published, ip) = (parts[0], parts.get(1).copied().unwrap_or(""), parts.get(2).copied().unwrap_or(""));
        return json!({"target": target.parse::<u16>().unwrap(), "published": published, "host_ip": ip, "protocol": protocol});
    }
    let field = |key: &str| port[key].as_str().map(|v| interpolate(v, call)).or_else(|| port[key].as_u64().map(|n| n.to_string())).unwrap_or_default();
    json!({"target": field("target").parse::<u16>().unwrap(), "published": field("published"), "host_ip": field("host_ip"), "protocol": port["protocol"].as_str().unwrap_or("tcp")})
}

/// `text` with Compose's variables (`${X}`, `${X:-default}`, `${X-default}`, `${X:+alt}`, `$$`)
/// filled from `call`'s environment.
fn interpolate(text: &str, call: &Call) -> String {
    let re = regex::Regex::new(r"\$\$|\$\{([A-Za-z_][A-Za-z0-9_]*)(?:(:?[-+])([^}]*))?\}").unwrap();
    re.replace_all(text, |c: &regex::Captures<'_>| {
        let Some(name) = c.get(1) else { return "$".to_owned() };
        let value = call.env(name.as_str()).filter(|v| !v.is_empty() || c.get(2).is_some_and(|o| !o.as_str().starts_with(':')));
        let other = c.get(3).map_or("", |m| m.as_str()).to_owned();
        match c.get(2).map(|o| o.as_str()) {
            Some(":+" | "+") => value.map(|_| other).unwrap_or_default(),
            Some(_) => value.map_or(other, str::to_owned),
            None => value.unwrap_or_default().to_owned(),
        }
    })
    .into_owned()
}

/// A YAML document as JSON, its tags (Compose's `!override`, `!reset`) dropped.
fn yaml(text: &str) -> Value {
    fn json_of(v: serde_yaml_ng::Value) -> Value {
        use serde_yaml_ng::Value as Y;
        match v {
            Y::Null => Value::Null,
            Y::Bool(b) => Value::Bool(b),
            Y::Number(n) => n.as_u64().map(Value::from).or_else(|| n.as_i64().map(Value::from)).unwrap_or_else(|| json!(n.as_f64())),
            Y::String(s) => Value::String(s),
            Y::Sequence(s) => Value::Array(s.into_iter().map(json_of).collect()),
            Y::Mapping(m) => Value::Object(m.into_iter().filter_map(|(k, v)| Some((k.as_str()?.to_owned(), json_of(v)))).collect()),
            Y::Tagged(t) => json_of(t.value),
        }
    }
    json_of(serde_yaml_ng::from_str(text).unwrap())
}

/// `over` merged into `base` as Compose merges files: maps key by key, anything else replaced
/// (the runner's override replaces a service's ports whole).
fn merge(base: &mut Value, over: Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                merge(b.entry(k).or_insert(Value::Null), v);
            }
        }
        (b, o) => *b = o,
    }
}

/// A Vagrant project as Vagrant runs it: its VMs and their forwarded ports read from the
/// Vagrantfile, each forward at its fixed host port, or at the one the Vagrantfile reads from the
/// call's environment, else at its default, moved at a later boot (`auto_correct`).
#[derive(Default)]
struct Vms {
    /// Each VM, whether a plain `vagrant up` starts it, and its state.
    machines: BTreeMap<String, (bool, String)>,
    forwards: Vec<Forward>,
    /// The host port each forward got, by VM and guest port.
    at: BTreeMap<(String, u16), u16>,
}

/// One forwarded port of the Vagrantfile.
struct Forward {
    machine: String,
    guest: u16,
    /// The variable the Vagrantfile reads its host port from (none: a fixed one), and its default.
    var: Option<String>,
    default: u16,
}

impl Forward {
    /// The host port the forward gets on `up`'s call: a fixed one's own; else the one the
    /// Vagrantfile reads from the call's environment, else its default the first time (`was`
    /// none) and another after.
    fn host(&self, up: &Call, was: Option<u16>) -> u16 {
        let Some(var) = &self.var else { return self.default };
        up.env(var).and_then(|p| p.parse().ok()).unwrap_or(if was.is_some() { self.default + MOVED_BY } else { self.default })
    }
}

impl Vms {
    fn of(vagrantfile: &Path) -> Vms {
        let text = std::fs::read_to_string(vagrantfile).unwrap();
        let define = regex::Regex::new(r#"^\s*config\.vm\.define "([^"]+)"(, autostart: false)?"#).unwrap();
        // `host: 8080`, `host: ISOLOOM_HOSTPORTS.fetch("web/web", 8080)` (a fixed one), or
        // `host: ENV.fetch("HOST_PORT_WEB_80_TCP", ISOLOOM_HOSTPORTS.fetch("web/web", 8080)).to_i`.
        let forward =
            regex::Regex::new(r#"forwarded_port", guest: (\d+), host: (?:ENV\.fetch\("(HOST_PORT_\w+)", )?(?:ISOLOOM_HOSTPORTS\.fetch\("[^"]+", )?(\d+)"#)
                .unwrap();
        let (mut machines, mut forwards, mut current) = (BTreeMap::new(), Vec::new(), None::<String>);
        for line in text.lines() {
            if let Some(c) = define.captures(line) {
                machines.insert(c[1].to_owned(), (c.get(2).is_none(), "not_created".to_owned()));
                current = Some(c[1].to_owned());
            } else if line.contains(r#""forwarded_port""#) {
                // A forward this model can't read fails here, not as a run that forwards nothing.
                let (Some(c), Some(machine)) = (forward.captures(line), &current) else { panic!("a forward the fake Vagrant can't read: {line}") };
                assert!(c.get(2).is_some() || !line.contains("auto_correct: true"), "a forward Vagrant may move, its host port read from no variable: {line}");
                forwards.push(Forward {
                    machine: machine.clone(),
                    guest: c[1].parse().unwrap(),
                    var: c.get(2).map(|v| v.as_str().to_owned()),
                    default: c[3].parse().unwrap(),
                });
            }
        }
        Vms { machines, forwards, at: BTreeMap::new() }
    }

    /// Each forward whose host port the Vagrantfile reads from a variable: the variable, its VM
    /// and its guest port.
    fn read(&self) -> Vec<(String, String, u16)> {
        self.forwards.iter().filter_map(|f| Some((f.var.clone()?, f.machine.clone(), f.guest))).collect()
    }

    /// Each forward where Vagrant put it: Vagrant doesn't say a forward's protocol, so each
    /// reads as TCP.
    fn seen(&self) -> Vec<Seen> {
        self.at.iter().map(|((machine, port), host)| Seen { machine: machine.clone(), port: *port, tcp: true, host: *host }).collect()
    }

    fn answer(&mut self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        if call.program != "vagrant" {
            return Reply::ok("");
        }
        match args.as_slice() {
            ["status", ..] => Reply::ok(&self.machines.iter().map(|(vm, (_, state))| format!("1,{vm},state,{state}")).collect::<Vec<_>>().join("\n")),
            ["up", rest @ ..] => {
                let named: Vec<&str> = rest.iter().copied().filter(|a| !a.starts_with("--") && Some(*a) != vm_tests::provider_of(rest)).collect();
                let starting: Vec<String> = self
                    .machines
                    .iter()
                    .filter(|(vm, (auto, _))| if named.is_empty() { *auto } else { named.contains(&vm.as_str()) })
                    .map(|(vm, _)| vm.clone())
                    .collect();
                for vm in &starting {
                    self.machines.get_mut(vm).unwrap().1 = "running".into();
                    vm_tests::record_made(call, &[vm.as_str()], vm_tests::provider_of(rest));
                }
                for f in self.forwards.iter().filter(|f| starting.contains(&f.machine)) {
                    let key = (f.machine.clone(), f.guest);
                    let host = f.host(call, self.at.get(&key).copied());
                    self.at.insert(key, host);
                }
                Reply::ok("")
            }
            [verb @ ("halt" | "suspend"), ..] => {
                let to = if *verb == "halt" { "poweroff" } else { "saved" };
                self.machines.values_mut().filter(|(_, s)| s == "running").for_each(|(_, s)| *s = to.into());
                Reply::ok("")
            }
            ["resume", ..] => {
                self.machines.values_mut().filter(|(_, s)| s == "saved").for_each(|(_, s)| *s = "running".into());
                Reply::ok("")
            }
            ["destroy", ..] => {
                self.machines.values_mut().for_each(|(_, s)| *s = "not_created".into());
                Reply::ok("")
            }
            ["port", vm, ..] => Reply::ok(
                &self
                    .forwards
                    .iter()
                    .filter(|f| f.machine == *vm)
                    .filter_map(|f| Some(format!("1,{vm},forwarded_port,{},{}", f.guest, self.at.get(&(f.machine.clone(), f.guest))?)))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            ["ssh", ..] => Reply::fail(1, ""),
            _ => Reply::ok(""),
        }
    }
}
