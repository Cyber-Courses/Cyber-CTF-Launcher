use super::*;
use crate::runtime::lab::{Refusal, RefusalCause};

use Provider::{Aws, Gcp, Parallels, Proxmox, Qemu, Utm, Virtualbox, VmwareEsxi};

/// One way to start on `provider` (Isoloom's target isn't the rule's).
fn path(provider: Provider) -> RunPath {
    RunPath { provider: Some(provider), target: String::new(), variant: None }
}

/// The paths on each of `providers`.
fn paths(providers: &[Provider]) -> Vec<RunPath> {
    providers.iter().copied().map(path).collect()
}

/// A container lab with VM recipes too: containers here, its VMs on VirtualBox, UTM and QEMU,
/// one VM or one VM per machine on AWS (one VM only on GCP).
fn dual() -> RunOptions {
    RunOptions {
        docker: Some(RunPath { provider: None, target: String::new(), variant: None }),
        vms: paths(&[Virtualbox, Utm, Qemu]),
        cloud_docker: paths(&[Aws, Gcp]),
        cloud_vm: paths(&[Aws]),
        ..RunOptions::default()
    }
}

/// A VM lab: on VirtualBox and UTM here, on Proxmox (not ESXi), one VM per machine on AWS.
fn vms() -> RunOptions {
    RunOptions { vms: paths(&[Virtualbox, Utm]), servers: paths(&[Proxmox]), cloud_vm: paths(&[Aws]), ..RunOptions::default() }
}

/// A cloud-services lab on AWS.
fn cloud() -> RunOptions {
    RunOptions { cloud_services: paths(&[Aws]), ..RunOptions::default() }
}

/// Every saved host: a Proxmox server, an ESXi one, AWS and GCP accounts.
fn kind(id: &str) -> Result<Option<Provider>> {
    Ok(match id {
        "pve" => Some(Proxmox),
        "esx" => Some(VmwareEsxi),
        "aws1" => Some(Aws),
        "gcp1" => Some(Gcp),
        _ => None,
    })
}

/// Parallels (which runs neither lab's VMs), QEMU and UTM ready; OrbStack answering.
fn ready() -> Ready {
    Ready { hypervisors: vec![Parallels, Qemu, Utm], engines: vec![Engine::Orbstack] }
}

/// The rule's answer.
fn placed(runtime: Runtime, options: &RunOptions, ready: &Ready, choice: &Choice, saved: &Saved) -> Result<Placement> {
    choose(runtime, options, ready, choice, saved, kind)
}

fn docker(engine: Option<Engine>, default_ports: bool) -> Placement {
    Placement::Docker { engine, default_ports }
}

fn on(provider: Provider) -> Placement {
    Placement::Vms { provider }
}

fn server(host: &str) -> Placement {
    Placement::Server { host: host.into() }
}

fn account(account: &str, per_machine: bool) -> Placement {
    Placement::Cloud { account: account.into(), per_machine }
}

fn choice() -> Choice {
    Choice::default()
}

fn host(id: &str) -> Choice {
    Choice { host: Some(id.into()), ..Choice::default() }
}

fn provider(p: Provider) -> Choice {
    Choice { provider: Some(p), ..Choice::default() }
}

/// Possibilities × readiness × choice × preference → placement: what the player picks always
/// wins, a saved preference fills what they didn't pick, and the rest is the first that Isoloom
/// allows and is ready here.
#[test]
fn the_choice_then_the_saved_preference_then_what_isoloom_allows_and_is_ready_decide() {
    let none = Saved::default();
    let likes = Saved {
        vm_provider: Some(Utm),
        docker_engine: Some(Engine::Orbstack),
        port_mode: Some(PortMode::Default),
        default_server: Some(("pve".into(), Proxmox)),
    };
    let per_machine = Choice { per_machine_vm: true, ..Choice::default() };
    let here = Choice { here: true, ..Choice::default() };
    let colima_random = Choice { engine: Some(Engine::Colima), default_ports: Some(false), ..Choice::default() };
    let nothing_ready = Ready::default();
    #[rustfmt::skip]
    let table: Vec<(&str, Runtime, RunOptions, Ready, Choice, &Saved, Placement)> = vec![
        // A container lab: containers here, on the current context, on free ports…
        ("container lab, nothing picked or saved", Runtime::Docker, dual(), ready(), choice(), &none, docker(None, false)),
        // …or on the saved engine (while it answers) and ports.
        ("container lab, saved engine and ports", Runtime::Docker, dual(), ready(), choice(), &likes, docker(Some(Engine::Orbstack), true)),
        ("container lab, saved engine not answering", Runtime::Docker, dual(), nothing_ready.clone(), choice(), &likes, docker(None, true)),
        ("container lab, engine and ports picked", Runtime::Docker, dual(), ready(), colima_random.clone(), &likes, docker(Some(Engine::Colima), false)),
        // Its VM form: the hypervisor picked, else the saved one, else the first ready that runs it.
        ("one VM per machine, hypervisor picked", Runtime::Docker, dual(), ready(), Choice { provider: Some(Virtualbox), per_machine_vm: true, ..Choice::default() }, &likes, on(Virtualbox)),
        ("one VM per machine, saved hypervisor", Runtime::Docker, dual(), ready(), per_machine.clone(), &likes, on(Utm)),
        ("one VM per machine, first ready", Runtime::Docker, dual(), ready(), per_machine.clone(), &none, on(Qemu)),
        // Never containers on a server; cloud forms on an account.
        ("container lab on an account", Runtime::Docker, dual(), ready(), host("aws1"), &likes, account("aws1", false)),
        ("container lab per machine on an account", Runtime::Docker, dual(), ready(), Choice { host: Some("aws1".into()), per_machine_vm: true, ..Choice::default() }, &none, account("aws1", true)),
        // A VM lab: the saved hypervisor when ready and it runs the lab, else the first that does.
        ("VM lab, saved hypervisor ready", Runtime::Vm, vms(), ready(), choice(), &likes, on(Utm)),
        ("VM lab, saved hypervisor doesn't run it", Runtime::Vm, vms(), ready(), choice(), &Saved { vm_provider: Some(Parallels), ..Saved::default() }, on(Utm)),
        ("VM lab, picked hypervisor", Runtime::Vm, vms(), nothing_ready.clone(), provider(Virtualbox), &likes, on(Virtualbox)),
        // None ready here: the default server, unless this machine was picked.
        ("VM lab, nothing ready, default server", Runtime::Vm, vms(), nothing_ready.clone(), choice(), &likes, server("pve")),
        ("VM lab, server picked", Runtime::Vm, vms(), ready(), host("pve"), &likes, server("pve")),
        ("VM lab on an account", Runtime::Vm, vms(), ready(), host("aws1"), &likes, account("aws1", true)),
        ("cloud lab on its account", Runtime::Cloud, cloud(), ready(), host("aws1"), &likes, account("aws1", false)),
        ("VM lab, this machine picked, saved hypervisor", Runtime::Vm, vms(), ready(), here.clone(), &likes, on(Utm)),
    ];
    for (case, runtime, options, ready, choice, saved, expected) in table {
        assert_eq!(placed(runtime, &options, &ready, &choice, saved).unwrap(), expected, "{case}");
    }
}

/// What Isoloom says the spec can't do, what isn't ready, and what the policy turns off are
/// refused with the codes a start says; nothing is ever picked in their place.
#[test]
fn what_the_spec_cant_do_or_isnt_ready_is_refused_never_replaced() {
    let none = Saved::default();
    let likes = Saved { default_server: Some(("pve".into(), Proxmox)), ..Saved::default() };
    let code = |r: Result<Placement>| r.unwrap_err().code();
    let said = |r: Result<Placement>| r.unwrap_err().to_string();
    // No hypervisor ready that runs a VM lab, and this machine picked (or no default server).
    let here = Choice { here: true, ..Choice::default() };
    assert_eq!(code(placed(Runtime::Vm, &vms(), &Ready::default(), &here, &likes)), Code::NoHypervisor);
    assert_eq!(code(placed(Runtime::Vm, &vms(), &Ready { hypervisors: vec![Parallels], ..Ready::default() }, &choice(), &none)), Code::NoHypervisor);
    let per_machine = Choice { per_machine_vm: true, ..Choice::default() };
    assert_eq!(code(placed(Runtime::Docker, &dual(), &Ready::default(), &per_machine, &none)), Code::NoHypervisor);
    // A cloud-services lab with no account picked; a cloud picked as a hypervisor.
    assert_eq!(code(placed(Runtime::Cloud, &cloud(), &ready(), &choice(), &likes)), Code::PickCloudAccount);
    assert_eq!(code(placed(Runtime::Vm, &vms(), &ready(), &provider(Aws), &none)), Code::PickCloudAccount);
    // A server picked as a hypervisor here.
    assert!(said(placed(Runtime::Vm, &vms(), &ready(), &provider(Proxmox), &none)).contains("pick a server host"));
    // Containers on a server, one VM per machine on a server.
    assert_eq!(said(placed(Runtime::Docker, &dual(), &ready(), &host("pve"), &none)), crate::runtime::launch::CONTAINERS_NOT_ON_SERVERS);
    let pve_per_machine = Choice { host: Some("pve".into()), per_machine_vm: true, ..Choice::default() };
    assert_eq!(said(placed(Runtime::Docker, &dual(), &ready(), &pve_per_machine, &none)), crate::runtime::launch::PER_MACHINE_NOT_ON_SERVERS);
    // Forms Isoloom doesn't generate there: ESXi, a hypervisor or a cloud form it lacks.
    assert_eq!(said(placed(Runtime::Vm, &vms(), &ready(), &host("esx"), &none)), "This lab doesn't run as ESXi.");
    assert_eq!(said(placed(Runtime::Vm, &vms(), &ready(), &provider(Qemu), &none)), "This lab doesn't run as QEMU.");
    let gcp_per_machine = Choice { host: Some("gcp1".into()), per_machine_vm: true, ..Choice::default() };
    assert!(said(placed(Runtime::Docker, &dual(), &ready(), &gcp_per_machine, &none)).starts_with("This lab doesn't run as one VM per machine on"));
    // Isoloom's own reason, and the policy's code, when they refuse it.
    let refusing = RunOptions {
        refused: vec![
            Refusal { provider: Some(Qemu), target: String::new(), cause: RefusalCause::Isoloom { reason: "QEMU links a VM on one network only".into() } },
            Refusal { provider: Some(Aws), target: String::new(), cause: RefusalCause::Policy { setting: "DisableCloudLaunches".into() } },
        ],
        ..vms()
    };
    assert_eq!(said(placed(Runtime::Vm, &refusing, &ready(), &provider(Qemu), &none)), "This lab can't run as QEMU: QEMU links a VM on one network only.");
    let no_clouds = RunOptions { cloud_vm: Vec::new(), ..refusing };
    assert_eq!(code(placed(Runtime::Vm, &no_clouds, &ready(), &host("aws1"), &none)), Code::TurnedOffByPolicy { setting: "DisableCloudLaunches".into() });
    // A container lab whose containers Isoloom doesn't generate.
    let vm_recipes_only = RunOptions { docker: None, ..dual() };
    assert_eq!(said(placed(Runtime::Docker, &vm_recipes_only, &ready(), &choice(), &none)), "This lab doesn't run as containers on this machine.");
    // A host that isn't saved.
    assert_eq!(said(placed(Runtime::Vm, &vms(), &ready(), &host("gone"), &none)), "that host is no longer set up in the launcher");
}

/// The webview reads the placement as it is sent: its kind, then its fields in camelCase.
#[test]
fn a_placement_reads_as_the_webview_writes_it() {
    let read = |json: serde_json::Value| serde_json::from_value::<Placement>(json).unwrap();
    assert_eq!(read(serde_json::json!({ "kind": "docker", "engine": "orbstack", "defaultPorts": true })), docker(Some(Engine::Orbstack), true));
    assert_eq!(read(serde_json::json!({ "kind": "vms", "provider": "utm" })), on(Utm));
    assert_eq!(read(serde_json::json!({ "kind": "server", "host": "pve" })), server("pve"));
    assert_eq!(read(serde_json::json!({ "kind": "cloud", "account": "aws1", "perMachine": true })), account("aws1", true));
    let choice: Choice = serde_json::from_value(serde_json::json!({ "here": true, "perMachineVm": true, "defaultPorts": false })).unwrap();
    assert_eq!(choice, Choice { here: true, per_machine_vm: true, default_ports: Some(false), ..Choice::default() });
}
