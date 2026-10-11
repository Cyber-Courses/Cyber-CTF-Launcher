use std::path::{Path, PathBuf};

use super::{
    CLOUD_NEEDS_ACCOUNT, LocalRun, Provider, Runtime, budget_verdict, check_qemu, emulated_arches, local_run, log_lines, low_memory_warning, parse_ipv4,
    start_on_docker, terraform_vars,
};
use super::{server, warn_if_low_memory, welcome};
use crate::exec::fake::{Tools, fail, ok};
use crate::runtime::paths::{local_vm, mark_parked, parked};
use crate::runtime::{Park, start};
use crate::test_app::TestApp;

const RUNNING: &str = r#"{"Service":"web","State":"running","Name":"cyberctf-web-web-1"}"#;
const EXITED: &str = r#"{"Service":"web","State":"exited","Name":"cyberctf-web-web-1"}"#;

async fn start_here(t: &TestApp, id: &str, runtime: Runtime, provider: Option<Provider>, env: &[(String, String)]) -> (crate::error::Result<()>, Vec<String>) {
    let dir = t.data().join("labs").join(id);
    let mut log = Vec::new();
    let r = start(t.handle(), &dir, id, runtime, provider, None, env, |l| log.push(l)).await;
    (r, log)
}

#[tokio::test]
async fn a_container_lab_starts_on_this_machines_docker() {
    let t = TestApp::new();
    let dir = t.lab("web", &[("isoloom.yml", CONTAINERS)]);
    let tools = Tools::rules(&[]);
    let (r, log) = start_here(&t, "web", Runtime::Docker, None, &[]).await;
    r.unwrap();
    assert!(tools.called("docker info") && tools.called("up -d --pull missing --wait"), "{:?}", tools.calls());
    assert_eq!(log.last().map(String::as_str), Some("Then look around."), "{log:?}");
    assert_eq!(local_vm(&dir), None);
}

#[tokio::test]
async fn a_container_lab_already_up_is_refused_and_leftovers_are_cleared() {
    let t = TestApp::new();
    t.lab("web", &[("isoloom.yml", CONTAINERS)]);
    let tools = Tools::rules(&[("ps --all", ok(RUNNING))]);
    let (r, _) = start_here(&t, "web", Runtime::Docker, None, &[]).await;
    assert!(r.unwrap_err().to_string().starts_with("This lab is already running on this machine"));
    assert!(!tools.called("up -d"));
    drop(tools);
    let tools = Tools::rules(&[("ps --all", ok(EXITED))]);
    let (r, log) = start_here(&t, "web", Runtime::Docker, None, &[]).await;
    r.unwrap();
    assert!(log.contains(&"Clearing a previous, stopped run…".to_string()), "{log:?}");
    assert!(tools.called("down --volumes"));
}

#[tokio::test]
async fn a_parked_lab_resumes_unless_nothing_is_left_of_it() {
    let t = TestApp::new();
    let dir = t.lab("web", &[("isoloom.yml", CONTAINERS)]);
    mark_parked(&dir, Some(Park::Shutdown)).unwrap();
    let tools = Tools::rules(&[("ps --all", ok(EXITED))]);
    let (r, log) = start_here(&t, "web", Runtime::Docker, None, &[]).await;
    r.unwrap();
    assert!(log[0].starts_with("This lab is parked on this machine: resuming it"), "{log:?}");
    assert!(tools.called(" start") && !tools.called("up -d"));
    assert_eq!(parked(&dir), None);
    drop(tools);
    // Its containers pruned: a fresh start instead of a resume that can only fail.
    mark_parked(&dir, Some(Park::Shutdown)).unwrap();
    let tools = Tools::rules(&[]);
    let (r, log) = start_here(&t, "web", Runtime::Docker, None, &[]).await;
    r.unwrap();
    assert!(log[0].contains("its stopped containers are gone: starting it fresh"), "{log:?}");
    assert!(tools.called("up -d"));
}

#[tokio::test]
async fn a_vm_lab_starts_on_a_local_hypervisor() {
    let t = TestApp::new();
    t.lab("ad", &[("isoloom.yml", VM_LAB), ("setup.sh", "true\n")]);
    let tools = Tools::rules(&[]);
    let (r, _) = start_here(&t, "ad", Runtime::Vm, None, &[]).await;
    assert_eq!(r.unwrap_err().to_string(), "VM labs need a provider");
    let env = [("CYBERCTF_ATTACKBOX_IMAGE".to_string(), "kalilinux/rolling".to_string())];
    let (r, log) = start_here(&t, "ad", Runtime::Vm, Some(Provider::Virtualbox), &env).await;
    r.unwrap();
    assert!(tools.called("vagrant up --provider virtualbox"), "{:?}", tools.calls());
    // The attack VM is started beside it, or its failure noted without failing the lab.
    assert!(log.iter().any(|l| l.contains("attack VM")) || tools.called(".cyberctf-attack-vm") || tools.calls().len() > 2, "{log:?}");
}

#[tokio::test]
async fn a_vm_start_that_trips_on_leftovers_is_cleared_and_retried_once() {
    let t = TestApp::new();
    t.lab("ad", &[("isoloom.yml", VM_LAB), ("setup.sh", "true\n")]);
    let tools = Tools::rules(&[("vagrant up", fail("VBoxManage: error: VERR_ALREADY_EXISTS"))]);
    let (r, log) = start_here(&t, "ad", Runtime::Vm, Some(Provider::Virtualbox), &[]).await;
    assert!(r.is_err());
    assert!(log.iter().any(|l| l.starts_with("A previous run left VM state behind")), "{log:?}");
    assert_eq!(tools.calls().iter().filter(|c| c.starts_with("vagrant up")).count(), 2);
    drop(tools);
    // A genuine failure is not retried.
    let tools = Tools::rules(&[("vagrant up", fail("provisioning failed"))]);
    let (r, _) = start_here(&t, "ad", Runtime::Vm, Some(Provider::Virtualbox), &[]).await;
    assert!(r.is_err());
    assert_eq!(tools.calls().iter().filter(|c| c.starts_with("vagrant up")).count(), 1);
}

#[tokio::test]
async fn a_container_lab_runs_in_a_local_vm_or_is_refused_on_a_missing_vagrant() {
    let t = TestApp::new();
    let dir = t.lab("web", &[("isoloom.yml", CONTAINERS)]);
    let tools = Tools::rules(&[]);
    let (r, log) = start_here(&t, "web", Runtime::Docker, Some(Provider::Virtualbox), &[]).await;
    r.unwrap();
    assert_eq!(log[0], "Running in a virtualbox VM on this machine");
    assert_eq!(local_vm(&dir).as_deref(), Some("virtualbox"));
    assert!(tools.called("vagrant up --provider virtualbox"));
    drop(tools);
    let _tools = Tools::rules(&[("vagrant --version", None)]);
    let (r, _) = start_here(&t, "web", Runtime::Docker, Some(Provider::Virtualbox), &[]).await;
    assert!(r.unwrap_err().to_string().starts_with("Vagrant isn't installed"));
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

/// The launcher's SSH key, as `ensure_key` leaves it.
fn key(t: &TestApp) {
    let ssh = t.data().join("ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    std::fs::write(ssh.join("id_ed25519"), "key").unwrap();
    std::fs::write(ssh.join("id_ed25519.pub"), "ssh-ed25519 AAAA cyberctf-launcher\n").unwrap();
}

async fn start_on(t: &TestApp, id: &str, runtime: Runtime, host: &str) -> (crate::error::Result<()>, Vec<String>) {
    let dir = t.data().join("labs").join(id);
    let mut log = Vec::new();
    let r = start(t.handle(), &dir, id, runtime, None, Some(host), &[], |l| log.push(l)).await;
    (r, log)
}

#[tokio::test]
async fn a_lab_on_a_cloud_account_is_applied_with_terraform_there() {
    let t = TestApp::new();
    let dir = t.lab("web", &[("isoloom.yml", CONTAINERS)]);
    key(&t);
    let host = t.host("az1", "azure", serde_json::json!({}));
    let tools = Tools::rules(&[]);
    let (r, log) = start_on(&t, "web", Runtime::Docker, &host).await;
    r.unwrap();
    assert!(log.iter().any(|l| l.contains("billed there until you stop it")), "{log:?}");
    assert!(tools.called("terraform") && tools.called("apply"), "{:?}", tools.calls());
    assert_eq!(std::fs::read_to_string(dir.join(server::HOST_MARKER)).unwrap(), "az1");
    let state = t.data().join("deployments").join("web").join("azure");
    assert_eq!(std::fs::read_to_string(state.join("runtime")).unwrap(), "DOCKER");
    // The run record keeps the firewall locked to this machine.
    assert!(std::fs::read_to_string(state.join("run.json")).unwrap().contains("203.0.113.7/32"));
    // An unknown host is refused before anything runs.
    assert!(start_on(&t, "web", Runtime::Docker, "nope").await.0.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn an_aws_launch_checks_the_budget_and_a_gcp_launch_its_project() {
    let t = TestApp::new();
    t.lab("web", &[("isoloom.yml", CONTAINERS)]);
    key(&t);
    let aws = t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true, "monthlyLimit": 50.0 }));
    let tools = Tools::rules(&[]);
    let (_, log) = start_on(&t, "web", Runtime::Docker, &aws).await;
    assert!(log.iter().any(|l| l.starts_with("Monthly budget not checked")) || tools.called("aws"), "{log:?}");
    drop(tools);
    let gcp = t.host("gcp1", "gcp", serde_json::json!({}));
    let tools = Tools::rules(&[]);
    let (_, log) = start_on(&t, "web", Runtime::Docker, &gcp).await;
    assert_eq!(log[0], "Checking the Cyber CTF labs project on Google Cloud…");
    assert!(tools.called("gcloud"));
}

#[tokio::test]
async fn the_public_ip_comes_from_the_first_resolver_that_gives_one() {
    use crate::http::tests::serve_once;
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap();
    let garbage = serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nblocked".to_vec()).await;
    let good = serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\n198.51.100.4\n".to_vec()).await;
    let closed = format!("http://{closed}/");
    assert_eq!(super::public_ip_from(&[closed.as_str(), garbage.as_str(), good.as_str()]).await.unwrap(), "198.51.100.4");
    let err = super::public_ip_from(&[closed.as_str()]).await.unwrap_err().to_string();
    assert!(err.starts_with("Couldn't determine your public IP"), "{err}");
    assert_eq!(super::public_ip().await.unwrap(), "203.0.113.7", "tests never reach the internet");
}

fn lab(spec: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cyberctf-launch-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("isoloom.yml"), spec).unwrap();
    dir
}

const CONTAINERS: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    services: [{ port: 80, http: true }]\n    docker: { image: nginx:1.27 }\nmessage: |\n  Open the web at {{ machines.web.addresses.lab }}.\n  Then look around.\n";

/// A VM lab that prepares: each machine installed by a script of the lab's (`setup.sh`).
const VM_LAB: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  a:\n    networks: { lab: 10 }\n    resources: { memory_mb: 1024 }\n    vm: { os: debian-12, provision: [setup.sh] }\n  b:\n    arch: arm64\n    networks: { lab: 11 }\n    resources: { memory_mb: 2048 }\n    vm: { os: debian-12, provision: [setup.sh] }\n";

const VMS: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  a:\n    networks: { lab: 10 }\n    resources: { memory_mb: 1024 }\n    vm: { os: debian-12 }\n  b:\n    arch: arm64\n    networks: { lab: 11 }\n    resources: { memory_mb: 2048 }\n    vm: { os: debian-12 }\n";

#[test]
fn a_local_run_follows_the_runtime_and_the_hypervisor() {
    assert_eq!(local_run(Runtime::Docker, None).unwrap(), LocalRun::Docker);
    assert_eq!(local_run(Runtime::Docker, Some(Provider::Virtualbox)).unwrap(), LocalRun::DockerInVm(Provider::Virtualbox));
    // A server provider is never a local VM: the containers run on this machine's Docker.
    assert_eq!(local_run(Runtime::Docker, Some(Provider::Proxmox)).unwrap(), LocalRun::Docker);
    assert_eq!(local_run(Runtime::Vm, Some(Provider::Qemu)).unwrap(), LocalRun::Vm(Provider::Qemu));
    assert_eq!(local_run(Runtime::Vm, None).unwrap_err().to_string(), "VM labs need a provider");
    assert_eq!(local_run(Runtime::Vm, Some(Provider::VmwareEsxi)).unwrap_err().to_string(), "pick a server host to run on ESXi or Proxmox");
    assert_eq!(local_run(Runtime::Cloud, None).unwrap_err().to_string(), CLOUD_NEEDS_ACCOUNT);
}

#[test]
fn terraform_gets_the_hosts_vars_then_the_launchers() {
    let key = Path::new("keys").join("id_ed25519");
    let host = vec![("node".to_string(), "pve".to_string())];
    let vars = terraform_vars(&host, Some("{}".into()), true, "ssh-ed25519 AAA".into(), &key, Some("203.0.113.7"));
    let names: Vec<&str> = vars.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(names, ["node", "inputs", "ssh_via_node", "ssh_public_key", "ssh_private_key_file", "allowed_cidr"]);
    assert_eq!(vars[4].1, key.to_string_lossy());
    assert_eq!(vars[5].1, "203.0.113.7/32");
    // No inputs, a routable bridge, not a cloud: only the key.
    let vars = terraform_vars(&[], None, false, "pk".into(), &key, None);
    assert_eq!(vars.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["ssh_public_key", "ssh_private_key_file"]);
}

#[test]
fn an_aws_budget_blocks_only_when_it_is_known_to_be_over() {
    let mut lines = Vec::new();
    assert!(budget_verdict(server::BudgetCheck::Ok, &mut |l| lines.push(l)).is_ok());
    let err = budget_verdict(server::BudgetCheck::Over(51.5, 50.0), &mut |l| lines.push(l)).unwrap_err().to_string();
    assert!(err.contains("$51.50 of $50.00"), "{err}");
    assert!(lines.is_empty());
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
    let spec = isoloom_core::load(&dir).unwrap();
    warn_if_low_memory(&spec, &mut |_l| {});
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn qemu_emulates_only_the_vms_built_for_another_cpu() {
    let dir = lab(VMS);
    let spec = isoloom_core::load(&dir).unwrap();
    assert_eq!(emulated_arches(&spec, "x86_64"), ["aarch64"]);
    assert_eq!(emulated_arches(&spec, "aarch64"), ["x86_64"]);
    let mut both = emulated_arches(&spec, "riscv64");
    both.sort();
    assert_eq!(both, ["aarch64", "x86_64"]);
    std::fs::remove_dir_all(dir).unwrap();
    // Containers have no CPU of their own to emulate.
    let dir = lab(CONTAINERS);
    let spec = isoloom_core::load(&dir).unwrap();
    assert!(emulated_arches(&spec, "riscv64").is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn a_lab_without_vms_needs_no_emulator() {
    let dir = lab(CONTAINERS);
    let spec = isoloom_core::load(&dir).unwrap();
    let mut lines = Vec::new();
    check_qemu(&spec, &mut |l| lines.push(l)).await.unwrap();
    assert!(lines.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_welcome_is_the_labs_message_line_by_line() {
    let dir = lab(CONTAINERS);
    let spec = isoloom_core::load(&dir).unwrap();
    let mut lines = Vec::new();
    welcome(&dir, &spec, isoloom_core::Target::Vagrant, &mut |l| lines.push(l));
    assert_eq!(lines, ["Open the web at 10.30.0.10.", "Then look around."]);
    let mut lines = Vec::new();
    log_lines("one\ntwo\n", &mut |l| lines.push(l));
    assert_eq!(lines, ["one", "two"]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn a_docker_start_without_a_lab_spec_fails_before_touching_docker() {
    let dir = std::env::temp_dir().join(format!("cyberctf-nolab-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut logged = Vec::new();
    let res = start_on_docker(&dir, "no-lab", &[], &mut |l| logged.push(l)).await;
    // Nothing generated, no marker written, nothing run.
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(res.is_err());
    assert!(logged.is_empty(), "{logged:?}");
    assert!(left.is_empty());
}

#[test]
fn a_lab_still_deployed_on_another_host_is_not_started_elsewhere() {
    // Regression: starting a lab here (or on another host) dropped the marker naming the host
    // its deployment still ran on, so Stop, the expiry reaper and removing that host all lost
    // it, and a cloud lab kept billing.
    let base = std::env::temp_dir().join(format!("cyberctf-elsewhere-{}", rand::random::<u32>()));
    let dir = base.join("labs").join("web-1");
    std::fs::create_dir_all(&dir).unwrap();
    // Never deployed on a server: free to start anywhere.
    assert_eq!(super::deployed_elsewhere(&base, &dir, "web-1", None), None);
    server::mark_lab(&dir, Some("aws1")).unwrap();
    // Marked, but nothing of it is left there (destroyed): free.
    assert_eq!(super::deployed_elsewhere(&base, &dir, "web-1", None), None);
    // A Terraform state still holding resources.
    let state = base.join("deployments").join("web-1").join("aws");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("terraform.tfstate"), r#"{"outputs":{"ip":{"value":"203.0.113.7"}},"resources":[{}]}"#).unwrap();
    assert_eq!(super::deployed_elsewhere(&base, &dir, "web-1", None).as_deref(), Some("aws1"), "here");
    assert_eq!(super::deployed_elsewhere(&base, &dir, "web-1", Some("pve1")).as_deref(), Some("aws1"), "another host");
    assert_eq!(super::deployed_elsewhere(&base, &dir, "web-1", Some("aws1")), None, "the same host again");
    std::fs::remove_dir_all(base.join("deployments")).unwrap();
    // Machines Vagrant still tracks on an ESXi host.
    server::mark_lab(&dir, Some("esxi1")).unwrap();
    let vagrant = crate::runtime::lab::vagrant_dir(&dir, Runtime::Vm);
    let machine = vagrant.join(".vagrant").join("machines").join("dc01").join("vmware_esxi");
    std::fs::create_dir_all(&machine).unwrap();
    std::fs::write(vagrant.join("Vagrantfile"), "").unwrap();
    assert_eq!(super::deployed_elsewhere(&base, &dir, "web-1", None), None, "no machine created yet");
    std::fs::write(machine.join("id"), "vm-42").unwrap();
    assert_eq!(super::deployed_elsewhere(&base, &dir, "web-1", None).as_deref(), Some("esxi1"));
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn resolver_answers_must_be_one_ipv4() {
    assert_eq!(parse_ipv4("203.0.113.7\n").as_deref(), Some("203.0.113.7"));
    assert_eq!(parse_ipv4("  198.51.100.1 ").as_deref(), Some("198.51.100.1"));
    assert_eq!(parse_ipv4("2001:db8::1"), None);
    assert_eq!(parse_ipv4("<html>blocked</html>"), None);
    assert_eq!(parse_ipv4(""), None);
}
