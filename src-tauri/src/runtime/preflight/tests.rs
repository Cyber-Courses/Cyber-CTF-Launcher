use super::*;
use crate::machine::system::docker::Engine;
use crate::runtime::providers::testing::{Faked, fake};
use crate::testing::test_app::TestApp;
use isothrum_runner::fake::{Call, FakeProcesses, Reply};
use std::net::{Ipv4Addr, TcpListener};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

/// A container lab of one web machine, its port 80 published on `publish` (`fixed`: that host
/// port or nothing).
fn web(publish: u16, fixed: bool) -> String {
    let publish = if fixed { format!("{{ host: {publish}, fixed: true }}") } else { publish.to_string() };
    format!(
        "version: 1\nname: web\nnetworks: {{ lab: {{ cidr: 10.70.0.0/24 }} }}\nmachines:\n  web:\n    networks: {{ lab: 10 }}\n    ports: {{ site: {{ port: 80, protocol: http, publish: {publish} }} }}\n    app: {{ image: nginx:1.27 }}\n"
    )
}

/// A VM lab of one Debian machine built for this machine's CPU (any hypervisor here runs it).
fn vms() -> String {
    let arch = if std::env::consts::ARCH == "aarch64" { "arm64" } else { "amd64" };
    format!(
        "version: 1\nname: ad\nnetworks: {{ lan: {{ cidr: 10.20.0.0/24 }} }}\nmachines:\n  dc: {{ os: debian@12, hardware: {{ arch: {arch} }}, networks: {{ lan: 10 }} }}\n"
    )
}

/// Lab `id` installed at [`SHA`] with `spec`: its spec is read from there, nothing downloaded.
fn installed(t: &TestApp, id: &str, spec: &str) {
    let dir = t.lab(id, &[("isoloom.yml", spec)]);
    std::fs::write(dir.join(crate::runtime::lab::COMMIT_MARKER), SHA).unwrap();
}

/// What a launch of `id`, declared `runtime`, at `placement` finds wrong.
async fn checked(t: &TestApp, id: &str, runtime: Runtime, placement: &Placement) -> Result<()> {
    let source = lab::Source { repository: "CyberCTF/lab", commit: SHA, check: |_, _| Ok(()) };
    Box::pin(check(&t.dirs(), id, runtime, source, placement)).await
}

/// This machine's tools: Docker answering on its current context and `colima-dev`, no Vagrant
/// (so no hypervisor is ready); `docker` false: Docker's daemon not answering.
fn machine(docker: bool) -> Faked {
    fake(FakeProcesses::model(move |c: &Call| match c.program.as_str() {
        "docker" if c.args.first().is_some_and(|a| a == "context") => Reply::ok("default *\ncolima-dev\n"),
        "docker" if docker => Reply::ok("27.3.1\n"),
        "docker" => Reply::fail(1, "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?"),
        _ => Reply::missing(),
    }))
}

const HERE: Placement = Placement::Docker { engine: None, default_ports: false };

#[tokio::test]
async fn a_launch_this_machine_can_run_passes() {
    let t = TestApp::new();
    installed(&t, "web", &web(18080, false));
    let _tools = machine(true);
    checked(&t, "web", Runtime::Docker, &HERE).await.unwrap();
}

#[tokio::test]
async fn the_policy_is_said_before_the_launch_is_registered() {
    use crate::platform::policy::{Key, Override};
    let t = TestApp::new();
    installed(&t, "priv", crate::runtime::test_support::PRIVILEGED_LAB);
    let _tools = machine(true);
    let _policy = Override::with(&[Key::RefusePrivilegedLabs]);
    let err = checked(&t, "priv", Runtime::Docker, &HERE).await.unwrap_err();
    assert_eq!(err.code(), Code::TurnedOffByPolicy { setting: "RefusePrivilegedLabs".into() });
}

#[tokio::test]
async fn a_placement_isoloom_doesnt_allow_is_refused_as_it_is() {
    let t = TestApp::new();
    installed(&t, "ad", &vms());
    t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true }));
    let _tools = machine(true);
    // A VM lab as containers here; an account sent as a server.
    assert_eq!(checked(&t, "ad", Runtime::Vm, &HERE).await.unwrap_err().code(), Code::NoHypervisor);
    let err = checked(&t, "ad", Runtime::Vm, &Placement::Server { host: "aws1".into() }).await.unwrap_err();
    assert_eq!(err.to_string(), "this host is a cloud account, not a server");
}

#[tokio::test]
async fn a_hypervisor_not_ready_here_is_said_for_every_provider() {
    let t = TestApp::new();
    installed(&t, "ad", &vms());
    let tools = machine(true);
    // No Vagrant: no hypervisor is ready, whichever one (not only QEMU and UTM, as the start checked).
    for provider in [Provider::Virtualbox, Provider::Libvirt, Provider::Qemu] {
        // (Isoloom may refuse the lab on one of them here too: then both are said.)
        let err = checked(&t, "ad", Runtime::Vm, &Placement::Vms { provider }).await.unwrap_err();
        assert!(matches!(err, Error::ToolMissing { tool: "Vagrant" }) || err.to_string().contains("`Vagrant` is not installed"), "{provider:?}: {err}");
    }
    assert!(!tools.made("vagrant").is_empty(), "isothrum-machine's own check asked: {:?}", tools.lines());
}

#[tokio::test]
async fn an_engine_with_no_context_or_a_daemon_not_answering_is_said() {
    let t = TestApp::new();
    installed(&t, "web", &web(18081, false));
    {
        let _tools = machine(true);
        let orbstack = Placement::Docker { engine: Some(Engine::Orbstack), default_ports: false };
        assert_eq!(checked(&t, "web", Runtime::Docker, &orbstack).await.unwrap_err().to_string(), "no Docker context for orbstack");
        let colima = Placement::Docker { engine: Some(Engine::Colima), default_ports: false };
        checked(&t, "web", Runtime::Docker, &colima).await.unwrap();
    }
    let _tools = machine(false);
    let err = checked(&t, "web", Runtime::Docker, &HERE).await.unwrap_err();
    assert!(err.to_string().contains("Cannot connect to the Docker daemon"), "Docker's own words: {err}");
}

#[tokio::test]
async fn a_fixed_host_port_already_taken_is_said_unless_the_lab_holds_it_already() {
    let t = TestApp::new();
    let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = held.local_addr().unwrap().port();
    let _tools = machine(true);
    let taken = Code::PortInUse { port: isothrum_runner::HostPort::new(port), holder: None };
    // Kept fixed by the spec: taken whatever the ports picked.
    installed(&t, "fixed", &web(port, true));
    assert_eq!(checked(&t, "fixed", Runtime::Docker, &HERE).await.unwrap_err().code(), taken);
    let own_ports = Placement::Docker { engine: None, default_ports: true };
    assert_eq!(checked(&t, "fixed", Runtime::Docker, &own_ports).await.unwrap_err().code(), taken);
    // A published port the spec doesn't keep fixed is the runner's to pick, as before.
    installed(&t, "free", &web(port, false));
    checked(&t, "free", Runtime::Docker, &own_ports).await.unwrap();
    // Something of the lab here already (a parked run keeps its ports): its own, not a clash.
    crate::runtime::test_support::record_run(t.data(), "fixed", None, "docker", "parked");
    checked(&t, "fixed", Runtime::Docker, &HERE).await.unwrap();
}

/// A taken fixed port names what holds it, as the system's tools say, through the same words a
/// run's taken port is said with ("nginx (pid 4123)", Docker's processes as Docker). `lsof`'s
/// answer here; Windows asks PowerShell.
#[cfg(not(windows))]
#[tokio::test]
async fn a_taken_fixed_port_names_what_holds_it() {
    let t = TestApp::new();
    let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = held.local_addr().unwrap().port();
    installed(&t, "fixed", &web(port, true));
    for (lsof, named) in [("p4123\ncnginx\n", "nginx (pid 4123)"), ("p77\nccom.docke\n", "Docker (pid 77)")] {
        let _tools = fake(FakeProcesses::model(move |c: &Call| match c.program.as_str() {
            "docker" if c.args.first().is_some_and(|a| a == "context") => Reply::ok("default *\n"),
            "docker" => Reply::ok("27.3.1\n"),
            "lsof" if c.args.iter().any(|a| a == &format!("-iTCP:{port}")) => Reply::ok(lsof),
            _ => Reply::missing(),
        }));
        let err = checked(&t, "fixed", Runtime::Docker, &HERE).await.unwrap_err();
        assert_eq!(err.code(), Code::PortInUse { port: isothrum_runner::HostPort::new(port), holder: Some(named.into()) });
        assert!(err.to_string().contains(&format!("Held by {named}.")), "{err}");
    }
}

#[tokio::test]
async fn every_problem_is_said_at_once() {
    let t = TestApp::new();
    let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = held.local_addr().unwrap().port();
    installed(&t, "web", &web(port, true));
    let _tools = machine(false);
    let err = checked(&t, "web", Runtime::Docker, &HERE).await.unwrap_err();
    let said = err.to_string();
    assert!(said.starts_with("This lab can't start here yet:\n• "), "{said}");
    assert_eq!(said.lines().count(), 3, "the daemon and the port: {said}");
    assert!(said.contains(&port.to_string()), "{said}");
}

#[test]
fn the_mapper_says_nothing_one_problem_or_each_of_several() {
    assert!(verdict(Vec::new()).is_ok());
    assert_eq!(verdict(vec![Error::coded(Code::NoHypervisor)]).unwrap_err().code(), Code::NoHypervisor);
    let several = verdict(vec![Error::coded(Code::NoHypervisor), Error::Refused("one more".into())]).unwrap_err();
    assert!(several.is_refusal());
    assert!(several.to_string().ends_with("\n• one more"), "{several}");
}
