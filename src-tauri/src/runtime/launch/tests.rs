use std::path::PathBuf;

use super::local::{UTM_NOT_ALLOWED, check_utm, utm_not_allowed};
use super::{
    CLOUD_NEEDS_ACCOUNT, Destination, LabRef, LocalRun, Provider, Runtime, budget_verdict, emulated_arches, local_run, low_memory_warning, parse_ipv4,
    per_machine_runs_on, record_cloud_form,
};
use super::{server, warn_if_low_memory};
use crate::platform::exec::fake::Tools;
use crate::runtime::paths::{mark_parked, parked};
use crate::runtime::{Park, start};
use crate::testing::test_app::TestApp;

async fn start_here(t: &TestApp, id: &str, runtime: Runtime, provider: Option<Provider>, env: &[(String, String)]) -> (crate::error::Result<()>, Vec<String>) {
    start_at(t, id, runtime, Destination { provider, ..Destination::default() }, env).await
}

/// A start of installed lab `id` at `to`, with the lines it logged.
async fn start_at(t: &TestApp, id: &str, runtime: Runtime, to: Destination<'_>, env: &[(String, String)]) -> (crate::error::Result<()>, Vec<String>) {
    let dir = t.data().join("labs").join(id);
    let mut log = Vec::new();
    let r = start(&t.dirs(), LabRef { dir: &dir, id, runtime }, to, env, |l| log.push(l)).await;
    (r, log)
}

/// A start in one VM per machine here, on `provider`.
fn per_machine_on(provider: Provider) -> Destination<'static> {
    Destination { provider: Some(provider), host: None, per_machine_vm: true, docker_context: None }
}

#[tokio::test]
async fn a_lab_on_qemu_is_checked_for_its_emulator() {
    let t = TestApp::new();
    t.lab("ad", &[("isoloom.yml", VM_LAB), ("setup.sh", "true\n")]);
    let _tools = Tools::rules(&[("qemu-system", None)]);
    let (r, _) = start_here(&t, "ad", Runtime::Vm, Some(Provider::Qemu), &[]).await;
    // One of its machines is built for the other CPU, and its emulator is missing (or its
    // networks don't fit QEMU's two-VM links): refused before any VM is created.
    let err = r.unwrap_err().to_string();
    assert!(err.contains("emulator") || err.contains("can't run on QEMU"), "{err}");
}

async fn start_on(t: &TestApp, id: &str, runtime: Runtime, host: &str) -> (crate::error::Result<()>, Vec<String>) {
    start_at(t, id, runtime, Destination { host: Some(host), ..Destination::default() }, &[]).await
}

// Cloud VM labs (cloud-vm, cloud-docker) now launch through the Isothrum runner (`CloudRun`),
// whose Terraform runs on the runner's own process port — not the launcher's faked `exec` — so
// their execution is covered by the runner crate and the opt-in `real_cloud_labs` harness, not
// here. The launcher's own cloud policy is unit-tested where it lives: the AWS budget verdict
// (`on_host::budget_verdict`), this machine's public IP (`on_host::agreed_ip`), and the
// account-to-options mapping (`isothrum::cloud` tests).

/// A fake IP-lookup service answering `body` once.
async fn resolver(body: &str) -> String {
    crate::platform::http::tests::serve_once(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes()).await
}

#[tokio::test]
async fn the_public_ip_is_the_one_two_resolvers_agree_on() {
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap();
    let closed = format!("http://{closed}/");
    let (garbage, a, b) = (resolver("blocked").await, resolver("93.184.216.34\n").await, resolver(" 93.184.216.34").await);
    // One down, one answering a page: the two that answer agree.
    assert_eq!(super::public_ip_from(&[closed.as_str(), garbage.as_str(), a.as_str(), b.as_str()]).await.unwrap(), "93.184.216.34");
    // A single answer isn't enough to open the firewall to it.
    let (one, garbage) = (resolver("93.184.216.34").await, resolver("blocked").await);
    let err = super::public_ip_from(&[closed.as_str(), one.as_str(), garbage.as_str()]).await.unwrap_err().to_string();
    assert!(err.starts_with("Couldn't confirm your public IP"), "{err}");
    assert!(err.contains("an unexpected answer"), "{err}");
    // Two that disagree: refused, naming both.
    let (a, b, c) = (resolver("93.184.216.34").await, resolver("93.184.216.34").await, resolver("8.8.4.4").await);
    let err = super::public_ip_from(&[a.as_str(), b.as_str(), c.as_str()]).await.unwrap_err().to_string();
    assert!(err.starts_with("The services that tell Cyber CTF your public IP disagree"), "{err}");
    assert!(err.contains("93.184.216.34") && err.contains("8.8.4.4"), "{err}");
    // A private address (a proxy's own) counts as no answer.
    let (lan, a) = (resolver("192.168.1.20").await, resolver("93.184.216.34").await);
    let err = super::public_ip_from(&[lan.as_str(), a.as_str()]).await.unwrap_err().to_string();
    assert!(err.contains("192.168.1.20, not a public address"), "{err}");
    assert_eq!(super::public_ip().await.unwrap(), "203.0.113.7", "tests never reach the internet");
}

#[test]
fn the_verdict_needs_two_matching_public_answers() {
    use std::net::Ipv4Addr;
    let urls = ["https://a", "https://b", "https://c"];
    let ip = Ipv4Addr::new(93, 184, 216, 34);
    assert_eq!(super::agreed_ip(&urls, &[Ok(ip), Err("timeout".into()), Ok(ip)]).unwrap(), "93.184.216.34");
    assert!(super::agreed_ip(&urls, &[Ok(ip), Err("timeout".into()), Err("refused".into())]).is_err());
    assert!(super::agreed_ip(&urls, &[]).is_err());
    let err = super::agreed_ip(&urls, &[Ok(ip), Ok(ip), Ok(Ipv4Addr::new(1, 1, 1, 1))]).unwrap_err().to_string();
    assert!(err.contains("https://c: 1.1.1.1"), "{err}");
}

#[test]
fn only_internet_addresses_are_public() {
    use super::is_public_ipv4;
    for ip in ["93.184.216.34", "8.8.8.8", "1.1.1.1", "100.63.255.255", "100.128.0.1", "198.20.0.1", "223.255.255.254"] {
        assert!(is_public_ipv4(ip.parse().unwrap()), "{ip}");
    }
    for ip in [
        "10.1.2.3",
        "172.16.0.1",
        "192.168.1.1",
        "127.0.0.1",
        "169.254.1.1",
        "100.64.0.1",
        "100.127.255.255",
        "0.0.0.0",
        "0.1.2.3",
        "192.0.0.8",
        "192.0.2.1",
        "198.51.100.4",
        "203.0.113.7",
        "198.18.0.1",
        "198.19.255.255",
        "224.0.0.1",
        "240.0.0.1",
        "255.255.255.255",
    ] {
        assert!(!is_public_ipv4(ip.parse().unwrap()), "{ip}");
    }
}

fn lab(spec: &str) -> PathBuf {
    crate::runtime::test_support::write_lab(spec)
}

const CONTAINERS: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http } }\n    app: { image: nginx:1.27 }\nmessage: |\n  Open the web at {{ machines.web.addresses.lab }}.\n  Then look around.\n";

/// A VM lab that prepares: each machine installed by a script of the lab's (`setup.sh`).
const VM_LAB: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  a:\n    networks: { lab: 10 }\n    resources: { memory_mb: 1024 }\n    os: debian@12\n    app: { steps: [{ run: setup.sh }] }\n  b:\n    hardware: { arch: arm64 }\n    networks: { lab: 11 }\n    resources: { memory_mb: 2048 }\n    os: debian@12\n    app: { steps: [{ run: setup.sh }] }\n";

const VMS: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  a:\n    networks: { lab: 10 }\n    resources: { memory_mb: 1024 }\n    os: debian@12\n  b:\n    hardware: { arch: arm64 }\n    networks: { lab: 11 }\n    resources: { memory_mb: 2048 }\n    os: debian@12\n";

#[test]
fn a_local_run_follows_the_runtime_and_the_hypervisor() {
    assert_eq!(local_run(Runtime::Docker, None, false).unwrap(), LocalRun::Docker);
    assert_eq!(local_run(Runtime::Docker, Some(Provider::Virtualbox), false).unwrap(), LocalRun::DockerInVm(Provider::Virtualbox));
    // A server provider is never a local VM: the containers run on this machine's Docker.
    assert_eq!(local_run(Runtime::Docker, Some(Provider::Proxmox), false).unwrap(), LocalRun::Docker);
    assert_eq!(local_run(Runtime::Vm, Some(Provider::Qemu), false).unwrap(), LocalRun::Vm(Provider::Qemu));
    assert_eq!(local_run(Runtime::Vm, None, false).unwrap_err().to_string(), "VM labs need a provider");
    assert_eq!(local_run(Runtime::Vm, Some(Provider::VmwareEsxi), false).unwrap_err().to_string(), "pick a server host to run on ESXi or Proxmox");
    assert_eq!(local_run(Runtime::Cloud, None, false).unwrap_err().to_string(), CLOUD_NEEDS_ACCOUNT);
}

#[test]
fn a_container_lab_picked_as_one_vm_per_machine_runs_as_a_vm_lab() {
    // The override: the VM form on the hypervisor, as a VM lab would run.
    assert_eq!(local_run(Runtime::Docker, Some(Provider::Qemu), true).unwrap(), LocalRun::Vm(Provider::Qemu));
    // It needs a hypervisor on this machine: none, or a server's, is refused (never silently
    // containers, which is not what was picked).
    assert_eq!(local_run(Runtime::Docker, None, true).unwrap_err().to_string(), "One VM per machine needs a hypervisor on this machine");
    assert!(local_run(Runtime::Docker, Some(Provider::Proxmox), true).is_err());
    // A VM lab is already that form; a cloud lab has no machines to be VMs.
    assert_eq!(local_run(Runtime::Vm, Some(Provider::Virtualbox), true).unwrap(), LocalRun::Vm(Provider::Virtualbox));
    assert_eq!(local_run(Runtime::Cloud, None, true).unwrap_err().to_string(), CLOUD_NEEDS_ACCOUNT);
}

/// A lab with both forms: each machine has a container recipe (`app.image`) and the VM one
/// (`os`, `app.steps`), like `supplier-portal-api`. One machine is built for the other CPU
/// whatever this host is, so QEMU's emulator check always has something to check (as `VM_LAB`).
const BOTH_FORMS: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    os: debian@12\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http } }\n    resources: { memory_mb: 1024 }\n    app: { image: nginx:1.27, steps: [{ run: setup.sh }] }\n  db:\n    os: debian@12\n    hardware: { arch: arm64 }\n    networks: { lab: 11 }\n    resources: { memory_mb: 1024 }\n    app: { image: mysql:8.0, steps: [{ run: setup.sh }] }\n";

#[tokio::test]
async fn a_container_lab_started_as_vms_takes_the_vm_path_and_every_operation_follows() {
    use crate::runtime::lab::{Target, vagrant_dir};
    use crate::runtime::paths::{PER_MACHINE_MARKER, local_runtime, per_machine_vm};
    let t = TestApp::new();
    let dir = t.lab("both", &[("isoloom.yml", BOTH_FORMS), ("setup.sh", "true\n")]);
    // QEMU with no emulator for the other CPU: the start stops at the emulator check, after the
    // VM form's files are generated and the form is recorded, before any VM is created.
    let _tools = Tools::rules(&[("qemu-system", None)]);
    let (r, _) = start_at(&t, "both", Runtime::Docker, per_machine_on(Provider::Qemu), &[]).await;
    let err = r.unwrap_err().to_string();
    assert!(err.contains("emulator"), "{err}");
    // The vagrant target's files (one VM per machine), not docker-vm's, and the form marked.
    assert!(vagrant_dir(&dir, Runtime::Vm).join("Vagrantfile").is_file(), "the vagrant target was generated");
    assert!(!vagrant_dir(&dir, Runtime::Docker).join("Vagrantfile").exists(), "not docker-vm");
    assert!(per_machine_vm(&dir) && dir.join(PER_MACHINE_MARKER).is_file());
    assert_eq!(crate::runtime::paths::local_vm(&dir), None, "no docker-vm mark");
    // The webview keeps calling the lab a container lab: here it is a VM lab.
    assert_eq!(local_runtime(&dir, Runtime::Docker), Runtime::Vm);
    assert_eq!(crate::runtime::runtime_here(&t.dirs(), "both", false), Some(Runtime::Vm));
    assert_eq!(crate::runtime::primary_url(&dir, "both", Runtime::Docker).await, None, "a VM lab has no loopback URL");
    // Stopping it as a container lab goes to its VM run (the vagrant target's manifest) and
    // forgets the form. The manifest is removed first, so the runner has nothing to destroy and
    // no real `vagrant` runs here.
    let manifest = vagrant_dir(&dir, Runtime::Vm).join("run.json");
    assert!(manifest.is_file(), "the VM run's manifest was written");
    std::fs::remove_file(&manifest).unwrap();
    let mut log = Vec::new();
    crate::runtime::stop_lab(&t.dirs(), "both", Runtime::Docker, std::future::ready(()), |l| log.push(l)).await.unwrap();
    assert_eq!(log, ["Nothing to remove: the run has no files."], "the VM run, not a Docker one");
    assert!(!per_machine_vm(&dir));
    assert_eq!(local_runtime(&dir, Runtime::Docker), Runtime::Docker);
    let _ = Target::Vagrant;
}

#[tokio::test]
async fn a_plain_container_start_forgets_an_earlier_per_machine_form() {
    use crate::runtime::paths::{mark_per_machine_vm, per_machine_vm};
    let t = TestApp::new();
    // The same dual-form lab, started as containers this time. Its `setup.sh` is missing, so the
    // start stops at the spec check (the lab's files are looked at before anything is
    // generated or run), after the form was decided: no container is ever started here.
    let dir = t.lab("both", &[("isoloom.yml", BOTH_FORMS)]);
    mark_per_machine_vm(&dir, true).unwrap();
    let tools = Tools::rules(&[]);
    let (r, _) = start_here(&t, "both", Runtime::Docker, None, &[]).await;
    let err = r.unwrap_err().to_string();
    assert!(err.contains("isoloom.yml has mistakes") && err.contains("setup.sh"), "{err}");
    assert!(!per_machine_vm(&dir), "started as containers: the lab is a container lab again");
    assert!(!tools.calls().iter().any(|c| c.starts_with("docker compose")), "{:?}", tools.calls());
}

#[tokio::test]
async fn one_vm_per_machine_is_refused_on_a_server_and_on_utm() {
    use crate::runtime::paths::per_machine_vm;
    let t = TestApp::new();
    let dir = t.lab("both", &[("isoloom.yml", BOTH_FORMS), ("setup.sh", "true\n")]);
    t.host("pve1", "proxmox", serde_json::json!({}));
    let tools = Tools::rules(&[]);
    let to = Destination { host: Some("pve1"), per_machine_vm: true, ..Destination::default() };
    let err = start(&t.dirs(), LabRef { dir: &dir, id: "both", runtime: Runtime::Docker }, to, &[], |_| {}).await.unwrap_err().to_string();
    assert_eq!(err, super::PER_MACHINE_HERE_ONLY);
    assert!(tools.calls().is_empty() && !per_machine_vm(&dir), "{:?}", tools.calls());
    // UTM: Isoloom refuses the vagrant target there (vagrant_utm gives a VM no private network),
    // so the start is refused with its reason, before UTM is even opened.
    let (r, _) = start_at(&t, "both", Runtime::Docker, per_machine_on(Provider::Utm), &[]).await;
    let err = r.unwrap_err().to_string();
    assert!(err.starts_with("This lab can't run on UTM yet: ") && err.contains("vagrant_utm"), "{err}");
    assert!(!tools.called("open"), "{:?}", tools.calls());
}

#[test]
fn one_vm_per_machine_runs_on_every_cloud_and_on_no_server() {
    // One VM per machine is cloud-vm on any cloud account (which clouds a lab's module is written
    // for is its run options', `lab::run_options`, which the picker offers); never on a server.
    for cloud in [Provider::Aws, Provider::Azure, Provider::Gcp, Provider::DigitalOcean, Provider::Linode, Provider::Oci] {
        assert!(per_machine_runs_on(Some(cloud)).is_ok(), "{cloud:?}");
    }
    for server in [Some(Provider::Proxmox), Some(Provider::VmwareEsxi), None] {
        assert_eq!(per_machine_runs_on(server).unwrap_err().to_string(), super::PER_MACHINE_HERE_ONLY, "{server:?}");
    }
}

#[test]
fn the_cloud_form_sets_the_runtime_and_records_it() {
    use crate::runtime::paths::per_machine_vm;
    let t = TestApp::new();
    let dir = t.lab("both", &[("isoloom.yml", BOTH_FORMS)]);
    // A container lab picked as one VM per machine on a cloud: cloud-vm (the runtime that drives
    // the module), and the form recorded so every later cloud dispatch follows it.
    assert_eq!(record_cloud_form(&dir, Runtime::Docker, true).unwrap(), Runtime::Vm);
    assert!(per_machine_vm(&dir));
    // Its containers on one cloud VM: cloud-docker, and the mark cleared (a re-launch as the other
    // form must not keep the old one).
    assert_eq!(record_cloud_form(&dir, Runtime::Docker, false).unwrap(), Runtime::Docker);
    assert!(!per_machine_vm(&dir));
    // A VM lab is cloud-vm whatever the pick says; the mark is for container labs only, so it stays
    // clear (status, stop and the rest read the catalogue runtime for it).
    assert_eq!(record_cloud_form(&dir, Runtime::Vm, true).unwrap(), Runtime::Vm);
    assert!(!per_machine_vm(&dir));
}

#[test]
fn an_aws_budget_blocks_only_when_it_is_known_to_be_over() {
    let mut lines = Vec::new();
    assert!(budget_verdict(server::BudgetCheck::Ok, &mut |l| lines.push(l)).is_ok());
    let err = budget_verdict(server::BudgetCheck::Over(51.5, 50.0), &mut |l| lines.push(l)).unwrap_err().to_string();
    assert!(err.contains("$51.50 of $50.00"), "{err}");
    assert_eq!(lines, Vec::<String>::new());
    assert!(budget_verdict(server::BudgetCheck::Unverifiable("Cost Explorer is off.".into()), &mut |l| lines.push(l)).is_ok());
    assert_eq!(lines, ["Monthly budget not checked: Cost Explorer is off. Launching anyway; the lab still auto-stops."]);
}

#[test]
fn memory_is_a_warning_only_when_the_lab_needs_more_than_is_free() {
    assert_eq!(low_memory_warning(4096, 0), None, "unknown free memory");
    assert_eq!(low_memory_warning(2048, 4096), None);
    assert!(low_memory_warning(8192, 4096).unwrap().contains("about 8192 MB, but only ~4096 MB"));
    // The real probe: it never fails, it only may warn.
    let dir = lab(VMS);
    let spec = crate::runtime::lab::load(&dir).unwrap();
    warn_if_low_memory(&spec, &mut |_l| {});
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn qemu_emulates_only_the_vms_built_for_another_cpu() {
    let dir = lab(VMS);
    let spec = crate::runtime::lab::load(&dir).unwrap();
    assert_eq!(emulated_arches(&spec, "x86_64"), ["aarch64"]);
    assert_eq!(emulated_arches(&spec, "aarch64"), ["x86_64"]);
    let mut both = emulated_arches(&spec, "riscv64");
    both.sort_unstable();
    assert_eq!(both, ["aarch64", "x86_64"]);
    std::fs::remove_dir_all(dir).unwrap();
    // Containers have no CPU of their own to emulate.
    let dir = lab(CONTAINERS);
    let spec = crate::runtime::lab::load(&dir).unwrap();
    assert_eq!(emulated_arches(&spec, "riscv64"), Vec::<&str>::new());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_lab_without_vms_needs_no_emulator() {
    let dir = lab(CONTAINERS);
    let spec = crate::runtime::lab::load(&dir).unwrap();
    for host in ["x86_64", "aarch64"] {
        assert!(emulated_arches(&spec, host).is_empty(), "{host}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn a_start_refused_for_a_lab_still_up_elsewhere_changes_nothing() {
    // Regression: starting a lab here (or on another host) dropped the marker naming the host
    // its run was still up on, so Stop, the expiry reaper and removing that host all lost it,
    // and a cloud lab kept billing. Only the launcher's own Terraform states were asked, never
    // the runner's record. Refused before anything ran: the host marker (the way to stop it
    // there) and the parked mark stay, no tool is touched, and the error is a refusal, not a
    // failed deploy.
    let t = TestApp::new();
    let dir = t.lab("web", &[("isoloom.yml", CONTAINERS)]);
    t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true }));
    server::mark_lab(&dir, Some("aws1")).unwrap();
    mark_parked(&dir, Some(Park::Pause)).unwrap();
    crate::runtime::test_support::record_run(t.data(), "web", Some("aws1"), "cloud-docker", "ready");
    let tools = Tools::rules(&[]);
    let (r, log) = start_here(&t, "web", Runtime::Docker, None, &[]).await;
    let err = r.unwrap_err();
    assert!(err.is_refusal(), "{err}");
    assert_eq!(err.to_string(), "This lab is still running on Host aws1. Stop it there, then try again.");
    assert_eq!(server::marked_host(&dir).as_deref(), Some("aws1"));
    assert_eq!(parked(&dir), Some(Park::Pause));
    assert!(tools.calls().is_empty() && log.is_empty(), "{:?} {log:?}", tools.calls());
    // The same for another host; its own host again is not "elsewhere".
    let (r, _) = start_on(&t, "web", Runtime::Docker, "pve1").await;
    assert!(r.unwrap_err().is_refusal());
    // Up here (VMs the runner started on this machine): a start on a server is refused too.
    server::mark_lab(&dir, None).unwrap();
    crate::runtime::test_support::record_run(t.data(), "web", None, "vagrant", "ready");
    let (r, _) = start_on(&t, "web", Runtime::Docker, "aws1").await;
    assert_eq!(r.unwrap_err().code(), crate::error::Code::LabRunningHere);
    // A deployment of the launcher's own Terraform path not yet handed to the runner counts too.
    crate::runtime::test_support::record_run(t.data(), "web", None, "vagrant", "absent");
    server::mark_lab(&dir, Some("aws1")).unwrap();
    let state = t.data().join("deployments").join("web").join("aws");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("terraform.tfstate"), r#"{"outputs":{},"resources":[{"type":"aws_instance"}]}"#).unwrap();
    let (r, _) = start_here(&t, "web", Runtime::Docker, None, &[]).await;
    assert_eq!(r.unwrap_err().code(), crate::error::Code::LabRunningOn { host: "Host aws1".into() });
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
}

#[test]
fn resolver_answers_must_be_one_ipv4() {
    assert_eq!(parse_ipv4("203.0.113.7\n").as_deref(), Some("203.0.113.7"));
    assert_eq!(parse_ipv4("  198.51.100.1 ").as_deref(), Some("198.51.100.1"));
    assert_eq!(parse_ipv4("2001:db8::1"), None);
    assert_eq!(parse_ipv4("<html>blocked</html>"), None);
    assert_eq!(parse_ipv4(""), None);
}

#[test]
fn an_automation_refusal_is_told_apart_from_a_utm_still_opening() {
    // macOS refusing this app Apple events (no wait fixes it) against a UTM not answering yet.
    assert!(utm_not_allowed("54:63: execution error: Not authorized to send Apple events to UTM. (-1743)"));
    assert!(utm_not_allowed("UTM got an error: (-1743)"));
    assert!(!utm_not_allowed("execution error: UTM got an error: Application isn't running. (-600)"));
    assert!(!utm_not_allowed(""));
    assert!(UTM_NOT_ALLOWED.contains("Automation"));
}

/// A UTM that answers but isn't ready for commands yet (-2700, as it refused a VM import after
/// answering its name) is waited for like one still opening: the start goes on once it is.
#[tokio::test]
async fn a_utm_not_ready_for_commands_is_waited_for() {
    use crate::platform::exec::fake::{Tools, fail, ok};
    let mut probes = 0;
    let tools = Tools::new(move |program, args| match (program, args) {
        ("osascript", [_, probe]) if probe.contains("every virtual machine") => {
            probes += 1;
            if probes == 1 { fail("execution error: UTM got an error: UTM is not ready to accept commands. (-2700)") } else { ok("") }
        }
        _ => ok(""),
    });
    let mut lines = Vec::new();
    check_utm(&mut |l| lines.push(l)).await.unwrap();
    assert!(lines.iter().any(|l| l.contains("waiting for it to answer")), "{lines:?}");
    assert!(tools.called("every virtual machine"), "asks for its VMs, which only a ready UTM lists");
}

/// The real UTM, opened cold: run by hand before a release with UTM quit (`osascript -e 'tell
/// application "UTM" to quit'`), so the wait covers its launch.
#[tokio::test]
#[ignore = "opens the real UTM (macOS, UTM installed)"]
async fn utm_is_opened_and_answers_before_a_vm_starts() {
    let mut lines = Vec::new();
    check_utm(&mut |l| lines.push(l)).await.unwrap();
    println!("said: {lines:?}");
}

#[tokio::test]
async fn the_policy_refuses_a_start_it_turns_off_before_anything_runs() {
    use crate::error::Code;
    use crate::platform::policy::{Key, Override};
    let t = TestApp::new();
    t.lab("priv", &[("isoloom.yml", crate::runtime::test_support::PRIVILEGED_LAB)]);
    let aws = t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true }));
    let tools = Tools::rules(&[]);
    let refused = |r: crate::error::Result<()>, setting: &str| {
        let err = r.unwrap_err();
        assert_eq!(err.code(), Code::TurnedOffByPolicy { setting: setting.into() }, "{err}");
    };
    {
        let _policy = Override::with(&[Key::RefusePrivilegedLabs]);
        refused(start_here(&t, "priv", Runtime::Docker, None, &[]).await.0, "RefusePrivilegedLabs");
    }
    {
        let _policy = Override::with(&[Key::DisableCloudLaunches]);
        refused(start_on(&t, "priv", Runtime::Docker, &aws).await.0, "DisableCloudLaunches");
        // A cloud-services lab always runs on a cloud account.
        refused(start_here(&t, "priv", Runtime::Cloud, None, &[]).await.0, "DisableCloudLaunches");
    }
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
}
