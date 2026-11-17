use super::local::{UTM_NOT_ALLOWED, check_utm};
use super::{
    Destination, LabRef, LocalRun, Provider, Runtime, budget_verdict, cloud_form, containers_run_on, local_run, low_memory_warning, per_machine_runs_on,
};
use super::{server, warn_if_low_memory};
use crate::runtime::lab::emulated_arches;
use crate::runtime::providers::testing::{Faked, fake, no_tools};
use crate::runtime::recorded::Recorded;
use crate::runtime::start;
use crate::testing::test_app::TestApp;
use isothrum_machine::Arch;

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

/// Tools with no QEMU emulator, whichever CPU it would be for; nothing else may run.
fn no_emulator() -> Faked {
    use isothrum_runner::fake::{FakeProcesses, Reply};
    fake(FakeProcesses::script().allow("qemu-system-x86_64", Reply::missing()).allow("qemu-system-aarch64", Reply::missing()).build())
}

/// A start in one VM per machine here, on `provider`.
fn per_machine_on(provider: Provider) -> Destination<'static> {
    Destination { provider: Some(provider), per_machine_vm: true, ..Destination::default() }
}

#[tokio::test]
async fn a_lab_on_qemu_is_checked_for_its_emulator() {
    let t = TestApp::new();
    t.lab("ad", &[("isoloom.yml", VM_LAB), ("setup.sh", "true\n")]);
    let _tools = no_emulator();
    let (r, _) = start_here(&t, "ad", Runtime::Vm, Some(Provider::Qemu), &[]).await;
    // One of its machines is built for the other CPU, and its emulator is missing (or its
    // networks don't fit QEMU's two-VM links): refused before any VM is created.
    let err = r.unwrap_err().to_string();
    assert!(err.contains("qemu-system") || err.contains("can't run on QEMU"), "{err}");
}

async fn start_on(t: &TestApp, id: &str, runtime: Runtime, host: &str) -> (crate::error::Result<()>, Vec<String>) {
    start_at(t, id, runtime, Destination { host: Some(host), ..Destination::default() }, &[]).await
}

// Cloud VM labs (cloud-vm, cloud-docker) now launch through the Isothrum runner (`Deployment`),
// whose Terraform runs on the runner's own process port — not the launcher's faked `exec` — so
// their execution is covered by the runner crate and the opt-in `real_cloud_labs` harness, not
// here. The launcher's own cloud policy is unit-tested where it lives: the AWS budget verdict
// (`on_host::budget_verdict`), this machine's public IP said (`on_host::ip_unsure`), and the
// account-to-options mapping (`isothrum::cloud_runs` tests).

/// The resolvers' failures in the player's words; tests never reach the internet.
#[tokio::test]
async fn an_unsure_public_ip_is_said_as_the_launch_stops() {
    use isothrum_machine::net::PublicIpFailed;
    let ip = std::net::Ipv4Addr::new(93, 184, 216, 34);
    let err = super::ip_unsure(PublicIpFailed::Disagree(vec![("https://a".into(), ip), ("https://c".into(), std::net::Ipv4Addr::new(8, 8, 4, 4))])).to_string();
    assert!(err.starts_with("The services that tell Cyber CTF your public IP disagree (https://a: 93.184.216.34, https://c: 8.8.4.4)"), "{err}");
    let err = super::ip_unsure(PublicIpFailed::Unconfirmed(vec![("https://b".into(), "an unexpected answer".into())])).to_string();
    assert!(err.starts_with("Couldn't confirm your public IP") && err.ends_with("(https://b: an unexpected answer)"), "{err}");
    assert_eq!(super::public_ip().await.unwrap(), "203.0.113.7", "tests never reach the internet");
}

fn lab(spec: &str) -> crate::testing::TempDir {
    crate::runtime::test_support::write_lab(spec)
}

const CONTAINERS: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http } }\n    app: { image: nginx:1.27 }\nmessage: |\n  Open the web at {{ machines.web.addresses.lab }}.\n  Then look around.\n";

/// A VM lab that prepares: each machine installed by a script of the lab's (`setup.sh`).
const VM_LAB: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  a:\n    networks: { lab: 10 }\n    resources: { memory_mb: 1024 }\n    os: debian@12\n    app: { steps: [{ run: setup.sh }] }\n  b:\n    hardware: { arch: arm64 }\n    networks: { lab: 11 }\n    resources: { memory_mb: 2048 }\n    os: debian@12\n    app: { steps: [{ run: setup.sh }] }\n";

const VMS: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  a:\n    networks: { lab: 10 }\n    resources: { memory_mb: 1024 }\n    os: debian@12\n  b:\n    hardware: { arch: arm64 }\n    networks: { lab: 11 }\n    resources: { memory_mb: 2048 }\n    os: debian@12\n";

#[test]
fn a_local_run_follows_the_runtime_and_the_hypervisor() {
    assert_eq!(local_run(Runtime::Docker, None, false).unwrap(), LocalRun::Docker);
    // A container lab on a hypervisor here runs as one VM per machine there, never inside one VM.
    assert_eq!(local_run(Runtime::Docker, Some(Provider::Virtualbox), false).unwrap(), LocalRun::Vm(Provider::Virtualbox));
    assert_eq!(local_run(Runtime::Docker, Some(Provider::Virtualbox), true).unwrap(), LocalRun::Vm(Provider::Virtualbox));
    // A server provider is never a local VM: the containers run on this machine's Docker.
    assert_eq!(local_run(Runtime::Docker, Some(Provider::Proxmox), false).unwrap(), LocalRun::Docker);
    assert_eq!(local_run(Runtime::Vm, Some(Provider::Qemu), false).unwrap(), LocalRun::Vm(Provider::Qemu));
    assert_eq!(local_run(Runtime::Vm, None, false).unwrap_err().code(), crate::error::Code::NoHypervisor);
    assert_eq!(local_run(Runtime::Vm, Some(Provider::VmwareEsxi), false).unwrap_err().to_string(), "pick a server host to run on ESXi or Proxmox");
    assert_eq!(local_run(Runtime::Cloud, None, false).unwrap_err().code(), crate::error::Code::PickCloudAccount);
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
    assert_eq!(local_run(Runtime::Cloud, None, true).unwrap_err().code(), crate::error::Code::PickCloudAccount);
}

/// A lab with both forms: each machine has a container recipe (`app.image`) and the VM one
/// (`os`, `app.steps`), like `supplier-portal-api`. One machine is built for the other CPU
/// whatever this host is, so QEMU's emulator check always has something to check (as `VM_LAB`).
const BOTH_FORMS: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    os: debian@12\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http } }\n    resources: { memory_mb: 1024 }\n    app: { image: nginx:1.27, steps: [{ run: setup.sh }] }\n  db:\n    os: debian@12\n    hardware: { arch: arm64 }\n    networks: { lab: 11 }\n    resources: { memory_mb: 1024 }\n    app: { image: mysql:8.0, steps: [{ run: setup.sh }] }\n";

#[tokio::test]
async fn a_container_lab_started_as_vms_takes_the_vm_path_and_every_operation_follows() {
    use crate::runtime::lab::vagrant_dir;
    use crate::runtime::test_support::record_run;
    let t = TestApp::new();
    let dir = t.lab("both", &[("isoloom.yml", BOTH_FORMS), ("setup.sh", "true\n")]);
    // QEMU with no emulator for the other CPU: the start stops at the emulator check, after the
    // VM form's files are generated, before any VM is created.
    let _tools = no_emulator();
    let (r, _) = start_at(&t, "both", Runtime::Docker, per_machine_on(Provider::Qemu), &[]).await;
    let err = r.unwrap_err().to_string();
    assert!(err.contains("qemu-system"), "{err}");
    // The vagrant target's files (one VM per machine); nothing ran, so nothing is recorded: the
    // lab still reads as the container lab it is.
    assert!(vagrant_dir(&dir).join("Vagrantfile").is_file(), "the vagrant target was generated");
    assert_eq!(Recorded::of(&t.dirs(), "both").await.unwrap().form(Runtime::Docker), Runtime::Docker);
    // Once the runner records its run in the vagrant target's folder, the webview keeps calling
    // the lab a container lab: here it is a VM lab.
    record_run(t.data(), "both", None, "vagrant", "ready");
    assert_eq!(Recorded::of(&t.dirs(), "both").await.unwrap().form(Runtime::Docker), Runtime::Vm);
    assert_eq!(crate::runtime::runtime_here(&t.dirs(), "both", false).await, Some(Runtime::Vm));
    assert_eq!(crate::runtime::primary_url(&t.dirs(), &dir, "both", Runtime::Docker).await, None, "a VM lab has no loopback URL");
    // Taken down, it is a container lab again.
    record_run(t.data(), "both", None, "vagrant", "absent");
    assert_eq!(Recorded::of(&t.dirs(), "both").await.unwrap().form(Runtime::Docker), Runtime::Docker);
}

#[tokio::test]
async fn one_vm_per_machine_is_refused_on_a_server() {
    let t = TestApp::new();
    let dir = t.lab("both", &[("isoloom.yml", BOTH_FORMS), ("setup.sh", "true\n")]);
    t.host("pve1", "proxmox", serde_json::json!({}));
    let tools = no_tools();
    let to = Destination { host: Some("pve1"), per_machine_vm: true, ..Destination::default() };
    let err = start(&t.dirs(), LabRef { dir: &dir, id: "both", runtime: Runtime::Docker }, to, &[], |_| {}).await.unwrap_err().to_string();
    assert_eq!(err, super::PER_MACHINE_NOT_ON_SERVERS);
    assert!(tools.calls().is_empty() && Recorded::of(&t.dirs(), "both").await.unwrap().record().is_none(), "{:?}", tools.calls());
}

/// One VM per machine on UTM: Isoloom takes the lab there (Debian 12 has a UTM box), and the
/// start opens UTM, hidden, before Vagrant drives it; a Mac without UTM stops there, saying so.
#[tokio::test]
async fn one_vm_per_machine_on_utm_opens_utm_first() {
    let t = TestApp::new();
    t.lab("both", &[("isoloom.yml", BOTH_FORMS), ("setup.sh", "true\n")]);
    let tools = std::sync::Arc::new(isothrum_runner::fake::FakeProcesses::model(|_| isothrum_runner::fake::Reply::missing()));
    let mac = crate::runtime::providers::testing::host_on(isothrum_machine::HostOs::Macos, Arch::Arm64, None);
    let _machine = crate::runtime::providers::testing::Machine::set(std::sync::Arc::<isothrum_runner::fake::FakeProcesses>::clone(&tools), mac);
    let (r, _) = start_at(&t, "both", Runtime::Docker, per_machine_on(Provider::Utm), &[]).await;
    let err = r.unwrap_err();
    assert!(matches!(err, crate::error::Error::ToolMissing { tool: "UTM" }), "{err}");
    let opened = tools.calls().iter().any(|c| c.program == "open" && c.args.starts_with(&["-g".to_owned(), "-j".to_owned(), "-a".to_owned()]));
    assert!(opened, "UTM opened hidden, in the background");
}

#[test]
fn a_container_labs_containers_run_on_every_cloud_and_on_no_server() {
    for cloud in Provider::CLOUDS {
        assert!(containers_run_on(Some(cloud)).is_ok(), "{cloud:?}");
    }
    for server in [Some(Provider::Proxmox), Some(Provider::VmwareEsxi), None] {
        assert_eq!(containers_run_on(server).unwrap_err().to_string(), super::CONTAINERS_NOT_ON_SERVERS, "{server:?}");
    }
}

#[test]
fn one_vm_per_machine_runs_on_every_cloud_and_on_no_server() {
    // One VM per machine is cloud-vm on any cloud account (which clouds a lab's module is written
    // for is its run options', `lab::run_options`, which the picker offers); never on a server.
    for cloud in [Provider::Aws, Provider::Azure, Provider::Gcp, Provider::DigitalOcean, Provider::Linode, Provider::Oci] {
        assert!(per_machine_runs_on(Some(cloud)).is_ok(), "{cloud:?}");
    }
    for server in [Some(Provider::Proxmox), Some(Provider::VmwareEsxi), None] {
        assert_eq!(per_machine_runs_on(server).unwrap_err().to_string(), super::PER_MACHINE_NOT_ON_SERVERS, "{server:?}");
    }
}

#[test]
fn the_cloud_form_sets_the_runtime() {
    // A container lab picked as one VM per machine on a cloud: cloud-vm (the runtime that drives
    // the module); its containers on one cloud VM: cloud-docker. A VM lab is cloud-vm whatever
    // the pick says.
    assert_eq!(cloud_form(Runtime::Docker, true), Runtime::Vm);
    assert_eq!(cloud_form(Runtime::Docker, false), Runtime::Docker);
    assert_eq!(cloud_form(Runtime::Vm, true), Runtime::Vm);
    assert_eq!(cloud_form(Runtime::Vm, false), Runtime::Vm);
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
    assert_eq!(emulated_arches(&spec, Arch::Amd64), [Arch::Arm64]);
    assert_eq!(emulated_arches(&spec, Arch::Arm64), [Arch::Amd64]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_lab_without_vms_needs_no_emulator() {
    let dir = lab(CONTAINERS);
    let spec = crate::runtime::lab::load(&dir).unwrap();
    for host in isothrum_machine::host::ALL_ARCHES {
        assert!(emulated_arches(&spec, host).is_empty(), "{host:?}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn a_start_refused_for_a_lab_still_up_elsewhere_changes_nothing() {
    // Regression: starting a lab here (or on another host) dropped the marker naming the host
    // its run was still up on, so Stop, the expiry reaper and removing that host all lost it,
    // and a cloud lab kept billing. Only the launcher's own Terraform states were asked, never
    // the runner's record. Refused before anything ran: the record (the way to stop it there)
    // stays, no tool is touched, and the error is a refusal, not a failed deploy.
    let t = TestApp::new();
    t.lab("web", &[("isoloom.yml", CONTAINERS)]);
    t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true }));
    crate::runtime::test_support::record_run(t.data(), "web", Some("aws1"), "cloud-docker", "ready");
    let tools = no_tools();
    let (r, log) = start_here(&t, "web", Runtime::Docker, None, &[]).await;
    let err = r.unwrap_err();
    assert!(err.is_refusal(), "{err}");
    assert_eq!(err.to_string(), "This lab is still running on Host aws1. Stop it there, then try again.");
    assert_eq!(Recorded::of(&t.dirs(), "web").await.unwrap().host(), Some("aws1"));
    assert!(tools.calls().is_empty() && log.is_empty(), "{:?} {log:?}", tools.calls());
    // The same for another host; its own host again is not "elsewhere".
    let (r, _) = start_on(&t, "web", Runtime::Docker, "pve1").await;
    assert!(r.unwrap_err().is_refusal());
    // Up here (VMs the runner started on this machine): a start on a server is refused too.
    crate::runtime::test_support::record_run(t.data(), "web", None, "vagrant", "ready");
    let (r, _) = start_on(&t, "web", Runtime::Docker, "aws1").await;
    assert_eq!(r.unwrap_err().code(), crate::error::Code::LabRunningHere);
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
}

/// UTM on a Mac whose tools answer `osascript`'s ask for UTM's machines with `answers`, one
/// after the other (the last one kept): what `check_utm` says, and what it logged.
async fn utm_answering(answers: Vec<isothrum_runner::fake::Reply>) -> (crate::error::Result<()>, Vec<String>) {
    use isothrum_runner::fake::{FakeProcesses, Reply};
    let asked = std::sync::atomic::AtomicUsize::new(0);
    let tools = FakeProcesses::model(move |call| match call.program.as_str() {
        "osascript" => {
            let n = asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            answers[n.min(answers.len() - 1)].clone()
        }
        _ => Reply::ok(""),
    });
    let mac = crate::runtime::providers::testing::host_on(isothrum_machine::HostOs::Macos, Arch::Arm64, None);
    let _machine = crate::runtime::providers::testing::Machine::set(std::sync::Arc::new(tools), mac);
    let mut lines = Vec::new();
    let r = check_utm(&mut |l| lines.push(l)).await;
    (r, lines)
}

/// macOS refusing this app Apple events to UTM (no wait fixes it) is said with its way out, at
/// once.
#[tokio::test]
async fn an_automation_refusal_is_told_apart_from_a_utm_still_opening() {
    use isothrum_runner::fake::Reply;
    let (r, lines) = utm_answering(vec![Reply::fail(1, "54:63: execution error: Not authorized to send Apple events to UTM. (-1743)")]).await;
    assert_eq!(r.unwrap_err().to_string(), UTM_NOT_ALLOWED);
    assert!(lines.is_empty(), "{lines:?}");
    assert!(UTM_NOT_ALLOWED.contains("Automation"));
}

/// A UTM that answers but isn't ready for commands yet (-2700, as it refused a VM import after
/// answering its name) is waited for like one still opening: the start goes on once it is.
#[tokio::test]
async fn a_utm_not_ready_for_commands_is_waited_for() {
    use isothrum_runner::fake::Reply;
    let not_ready = Reply::fail(1, "execution error: UTM got an error: UTM is not ready to accept commands. (-2700)");
    let (r, lines) = utm_answering(vec![not_ready, Reply::ok("")]).await;
    r.unwrap();
    assert_eq!(lines, [crate::runtime::deploy_log::OPENING_UTM]);
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
    let tools = no_tools();
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

#[test]
fn a_start_with_nowhere_to_run_is_refused_by_its_code_before_anything_is_downloaded() {
    use crate::error::Code;
    use crate::platform::policy::{Key, Override};
    let t = TestApp::new();
    let dirs = t.dirs();
    t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true }));
    t.host("pve", "proxmox", serde_json::json!({}));
    let check = |runtime, to: Destination<'_>| super::check_destination(&dirs, runtime, &to);
    let here = |provider| Destination { provider, ..Destination::default() };
    let on = |host| Destination { host: Some(host), ..Destination::default() };
    let per_machine = |provider, host| Destination { provider, host, per_machine_vm: true, ..Destination::default() };
    assert_eq!(check(Runtime::Cloud, here(None)).unwrap_err().code(), Code::PickCloudAccount);
    assert_eq!(check(Runtime::Vm, here(None)).unwrap_err().code(), Code::NoHypervisor);
    assert!(check(Runtime::Cloud, on("aws1")).is_ok());
    assert!(check(Runtime::Vm, here(Some(Provider::Qemu))).is_ok());
    assert!(check(Runtime::Vm, on("pve")).is_ok());
    assert!(check(Runtime::Docker, here(None)).is_ok());
    // Regression: these were refused only once the lab was downloaded, at its start.
    // One VM per machine with no hypervisor here.
    assert!(check(Runtime::Docker, per_machine(None, None)).unwrap_err().to_string().contains("needs a hypervisor on this machine"));
    // ESXi or Proxmox picked with no host.
    assert!(check(Runtime::Vm, here(Some(Provider::Proxmox))).unwrap_err().to_string().contains("pick a server host"));
    // One VM per machine on a server rather than a cloud account.
    assert_eq!(check(Runtime::Docker, per_machine(None, Some("pve"))).unwrap_err().to_string(), super::PER_MACHINE_NOT_ON_SERVERS);
    assert!(check(Runtime::Docker, per_machine(None, Some("aws1"))).is_ok());
    // A container lab on a server (ESXi, Proxmox), which runs no containers.
    assert_eq!(check(Runtime::Docker, on("pve")).unwrap_err().to_string(), super::CONTAINERS_NOT_ON_SERVERS);
    assert!(check(Runtime::Docker, on("aws1")).is_ok());
    // A start on a cloud account under DisableCloudLaunches, which needs no spec.
    let _policy = Override::with(&[Key::DisableCloudLaunches]);
    assert_eq!(check(Runtime::Docker, on("aws1")).unwrap_err().code(), Code::TurnedOffByPolicy { setting: "DisableCloudLaunches".into() });
    assert!(check(Runtime::Vm, on("pve")).is_ok());
}
