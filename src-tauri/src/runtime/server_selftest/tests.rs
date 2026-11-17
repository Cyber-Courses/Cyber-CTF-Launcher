//! What stays the launcher's in the server self-test: the server as a launch would use it, the
//! steps in the player's words and order (the plan the Server screen lists), and the failures
//! people hit said with what to do. The test itself (its steps, its teardown, what an earlier
//! test left) is isothrum-machine's, tested there.

use std::sync::{Arc, Mutex};

use isothrum_runner::ErrorCode;

use super::*;

fn report(message: &str) -> Box<ErrorReport> {
    Box::new(ErrorReport::new(ErrorCode::ToolFailed, message))
}

/// A reporter and the events it sent.
fn reporter() -> (Reporter, Arc<Mutex<Vec<Event>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    (Reporter::new(move |e| sink.lock().unwrap().push(e)), seen)
}

/// Each step's last state, in the order steps were first reported.
fn ends(seen: &Mutex<Vec<Event>>) -> Vec<String> {
    let mut out: Vec<(String, String)> = Vec::new();
    for e in seen.lock().unwrap().iter() {
        match out.iter_mut().find(|(s, _)| s == e.step) {
            Some(found) => e.state.clone_into(&mut found.1),
            None => out.push((e.step.to_owned(), e.state.to_owned())),
        }
    }
    out.into_iter().map(|(s, st)| format!("{s}:{st}")).collect()
}

/// The detail of step `step`'s last event in `state`.
fn detail(seen: &Mutex<Vec<Event>>, step: &str, state: &str) -> String {
    seen.lock().unwrap().iter().rev().find(|e| e.step == step && e.state == state).and_then(|e| e.detail.clone()).unwrap_or_default()
}

/// A Proxmox server as a token setup with the node's key saves it, and a launch places it
/// (`launch::server_place`: the runner's key, the node discovered, the jump through it).
fn proxmox_place() -> ServerPlace {
    let vars = [
        ("proxmox_endpoint", "https://10.0.0.5:8006/"),
        ("proxmox_username", "root@pam"),
        ("proxmox_insecure", "true"),
        ("proxmox_ssh_address", "10.0.0.5"),
        ("proxmox_api_token", "root@pam!cyberctf=s3cret"),
        ("proxmox_ssh_username", "root"),
        ("proxmox_ssh_private_key_file", "/keys/node"),
        ("uplink_bridge", "vmbr1"),
        ("image_datastore", "isos"),
        ("ssh_public_key", "ssh-ed25519 AAAA runner"),
        ("ssh_private_key_file", "/runner/ssh/id_ed25519"),
    ];
    ServerPlace {
        label: "pve1".into(),
        name: "Lab server".into(),
        vars: vars.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
        node: Some("pve-a".into()),
        jump: Some("root@10.0.0.5".into()),
    }
}

fn proxmox() -> Target {
    proxmox_target("Lab server", &proxmox_place(), &vm_name("ab12")).unwrap()
}

fn esxi() -> Target {
    let env = [(HOSTNAME, "esxi.lan"), (PASSWORD, "pw"), (DATASTORE, "ssd1"), (VIRTUAL_NETWORK, "Test Net, Other")];
    let env: Vec<(String, String)> = env.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    esxi_target(&env, "VMware ovftool 5.1.0", &vm_name("ab12")).unwrap()
}

/// Every step of the test, in the order the test says them.
const EVERY_STEP: [Step; 9] = [Step::Host, Step::Prepare, Step::Image, Step::Network, Step::Vm, Step::Address, Step::Ssh, Step::Exec, Step::Down];

#[test]
fn the_steps_are_the_plan_the_server_screen_lists_in_its_order() {
    for (target, provider) in [(proxmox(), Provider::Proxmox), (esxi(), Provider::VmwareEsxi)] {
        let plan = PLANS.iter().find(|(p, _)| *p == provider).unwrap().1;
        // ESXi has no network of the test's own.
        let steps = EVERY_STEP.iter().filter(|s| provider == Provider::Proxmox || **s != Step::Network);
        assert_eq!(steps.map(|s| named(&target.words.kind, *s).0).collect::<Vec<_>>(), plan, "{provider:?}");
    }
}

#[test]
fn a_proxmox_server_is_tested_where_a_launch_would_put_its_lab() {
    let Target { server: Server::Proxmox(p), words } = proxmox() else { panic!("a Proxmox server") };
    assert_eq!((p.endpoint.as_str(), p.node.as_str(), p.bridge.as_str()), ("https://10.0.0.5:8006/", "pve-a", "vmbr1"));
    assert!(matches!(&p.sign_in, SignIn::Token(t) if t.expose() == "root@pam!cyberctf=s3cret"));
    assert!(p.insecure);
    assert_eq!(p.image_datastore.as_deref(), Some("isos"));
    assert_eq!(p.datastore, None, "the module's default");
    assert_eq!(p.node_ssh_key_file.as_deref(), Some(Path::new("/keys/node")));
    assert_eq!(p.node_address.as_deref(), Some("10.0.0.5"));
    assert_eq!(p.jump.as_deref(), Some("root@10.0.0.5"));
    let key = p.key.as_ref().expect("the runner's key");
    assert_eq!((key.private_file.as_path(), key.public.as_str()), (Path::new("/runner/ssh/id_ed25519"), "ssh-ed25519 AAAA runner"));
    assert_eq!((words.reached.as_str(), words.prepared.as_deref()), ("Lab server", Some("on node pve-a")));
}

#[test]
fn a_password_setup_signs_in_with_it_on_the_profiles_own_node() {
    // A password setup, its own node named in the profile (which wins over the one discovered).
    let mut place = proxmox_place();
    place.vars.retain(|(k, _)| k != "proxmox_api_token");
    place.vars.push(("proxmox_password".into(), "pw".into()));
    place.vars.push(("node".into(), "pve-own".into()));
    let Target { server: Server::Proxmox(p), .. } = proxmox_target("s", &place, "vm").unwrap() else { panic!() };
    assert!(matches!(&p.sign_in, SignIn::Password { user, password } if user == "root@pam" && password.expose() == "pw"));
    assert_eq!(p.node, "pve-own");
    place.vars.clear();
    assert_eq!(proxmox_target("s", &place, "vm").err().map(|e| e.to_string()).as_deref(), Some("the server's address isn't set"));
}

#[test]
fn an_esxi_host_is_tested_with_its_settings_on_its_first_port_group() {
    let Target { server: Server::Esxi(e), words } = esxi() else { panic!("an ESXi host") };
    assert_eq!((e.host.as_str(), e.port, e.username.as_str(), e.password.expose()), ("esxi.lan", 22, "root", "pw"));
    assert_eq!((e.datastore.as_deref(), e.network.as_deref()), (Some("ssd1"), Some("Test Net")));
    assert_eq!(words.reached, "esxi.lan:22 open; vagrant-vmware-esxi and VMware ovftool 5.1.0 ready");
    assert_eq!(words.prepared.as_deref(), Some("VM cyberctf-server-selftest-ab12, on port group Test Net"));
    assert_eq!(esxi_target(&[], "v", "vm").err().map(|e| e.to_string()).as_deref(), Some("the host's address isn't set"));
}

fn tools(plugin: std::result::Result<(), &str>, ovftool: std::result::Result<&str, &str>) -> Tools {
    Tools { plugin: plugin.map_err(str::to_owned), ovftool: ovftool.map(str::to_owned).map_err(str::to_owned) }
}

#[test]
fn a_missing_plugin_or_ovftool_fails_up_front_before_the_host_is_reached() {
    let err = tools_ready(tools(Err("Vagrant plugin `vagrant-vmware-esxi` is not installed"), Ok("v"))).unwrap_err().to_string();
    assert!(err.starts_with("Labs on ESXi run from this computer") && err.contains("`vagrant-vmware-esxi` is not installed"), "{err}");
    let err = tools_ready(tools(Err("no plugin"), Err("no ovftool"))).unwrap_err().to_string();
    assert!(err.contains("no plugin; no ovftool"), "{err}");
    assert_eq!(tools_ready(tools(Ok(()), Ok("VMware ovftool 5.1.0"))).unwrap(), "VMware ovftool 5.1.0");
}

#[test]
fn a_run_is_said_step_by_step_in_the_players_words() {
    let (r, seen) = reporter();
    let target = esxi();
    let work = Path::new("/data/selftest/server/ab12");
    let box_found = Found::Box { name: "bento/debian-12".into(), version: Some("202510.26.0".into()), arch: Some("amd64".into()) };
    let said: Vec<StepProgress> = vec![
        Progress::Running(Step::Host),
        Progress::Passed(Step::Host, Found::Answers("esxi.lan:22".into())),
        Progress::Running(Step::Prepare),
        Progress::Passed(Step::Prepare, Found::Removing),
        Progress::Passed(Step::Prepare, Found::Nothing),
        Progress::Running(Step::Image),
        Progress::Passed(Step::Image, Found::Line { text: "Transfer Completed".into(), bytes: Some(3000) }),
        Progress::Passed(Step::Image, box_found),
        Progress::Passed(Step::Vm, Found::Nothing),
        Progress::Passed(Step::Address, Found::Nothing),
        Progress::Passed(Step::Ssh, Found::Nothing),
        Progress::Passed(Step::Exec, Found::Uname("Linux x86_64".into())),
        Progress::Passed(Step::Down, Found::Nothing),
    ];
    for p in said {
        say(&r, &target.words, work, p);
    }
    assert_eq!(ends(&seen), ["connect:ok", "prepare:ok", "image:ok", "vm:ok", "address:ok", "ssh:ok", "command:ok", "cleanup:ok"]);
    assert_eq!(detail(&seen, "connect", "ok"), "esxi.lan:22 open; vagrant-vmware-esxi and VMware ovftool 5.1.0 ready");
    assert_eq!(detail(&seen, "prepare", "running"), "Removing the VM a previous test left on the host…");
    assert_eq!(detail(&seen, "image", "ok"), "Box bento/debian-12 v202510.26.0 (amd64), uploaded to datastore ssd1");
    let upload = seen.lock().unwrap().iter().find(|e| e.detail.as_deref() == Some("Transfer Completed")).map(|e| e.size);
    assert_eq!(upload, Some(Some(3000)), "the upload's line carries its size");
    assert_eq!(detail(&seen, "address", "ok"), "On port group Test Net");
    assert_eq!(detail(&seen, "command", "ok"), "Linux x86_64");
    assert_eq!(detail(&seen, "cleanup", "ok"), "Nothing of the test is left on the server.");
}

#[test]
fn the_failures_people_hit_are_said_with_what_to_do() {
    let (pve, esxi) = (proxmox().words, esxi().words);
    let work = Path::new("/data/selftest/server/ab12");
    let said = |words: &Words, why: Failure| error(words, work, why).to_string();
    assert_eq!(
        said(&pve, Failure::NoAddress(report("postcondition failed"))),
        "The test VM got no address on bridge vmbr1: the bridge has no DHCP server reachable from the VMs (or no internet). Use an uplink bridge with DHCP and internet."
    );
    assert!(
        said(&esxi, Failure::NoAddress(report("timeout waiting for \"running\"")))
            .starts_with("The test VM got no address on port group Test Net within 4 minutes")
    );
    assert!(said(&pve, Failure::NoBridge(report("bridge 'sn1' does not exist"))).contains("source /etc/network/interfaces.d/*"));
    assert!(said(&pve, Failure::ImageDownload(report("404"))).ends_with("Details: 404"));
    assert!(said(&esxi, Failure::NameTaken).starts_with("A VM named cyberctf-server-selftest-ab12 is already on the host"));
    let upload = said(&esxi, Failure::Upload("Error: No space left on device".into()));
    assert!(upload.contains("to datastore ssd1") && upload.ends_with("Details: Error: No space left on device"), "{upload}");
    assert!(said(&pve, Failure::NoSsh(report("refused"))).contains("at its address on bridge vmbr1"));
    assert!(said(&esxi, Failure::NoSsh(report("refused"))).contains("at its address on port group Test Net"));
    assert_eq!(said(&pve, Failure::Unreachable("10.0.0.5:8006".into())), "no answer on 10.0.0.5:8006");
    assert_eq!(said(&pve, Failure::Command { exit: 1, output: "nope\n".into() }), "the command inside the test VM failed: nope");
    // An earlier test's VM that couldn't be removed: where its state is kept.
    let left = said(&pve, Failure::Leftover(report("the VM is locked")));
    assert!(left.contains("the VM is locked; the test VM couldn't be removed from the host. Its state is kept in /data/selftest/server/ab12"), "{left}");
}

#[test]
fn a_failed_teardown_says_what_is_left_and_where_its_state_is_kept() {
    let (pve, esxi) = (proxmox().words, esxi().words);
    let why = Failure::Runner(report("terraform destroy failed: timeout"));
    let left_pve = left(&pve, &why);
    // The runner's failure as the app's error mapping says it.
    assert!(
        left_pve.starts_with("Couldn't remove everything the test created on the server (") && left_pve.contains("terraform destroy failed: timeout)"),
        "{left_pve}"
    );
    assert!(left_pve.contains("Its VM (cyberctf-server-selftest-ab12) and its lab network may still be there"), "{left_pve}");
    assert!(left(&esxi, &why).contains("Its VM, cyberctf-server-selftest-ab12, may still be there"));
    // The test's result: its own, or the teardown's, then the note.
    let work = Path::new("/w");
    let err = kept(work, Ok(())).unwrap_err().to_string();
    assert!(err.starts_with("the test VM couldn't be removed from the host. Its state is kept in /w"), "{err}");
    assert!(kept(work, Err(Error::Invalid("no address".into()))).unwrap_err().to_string().starts_with("no address; the test VM couldn't be removed"));
    let (r, seen) = reporter();
    say(&r, &pve, work, Progress::Failed(Step::Down, why));
    assert_eq!(detail(&seen, "cleanup", "fail"), left_pve);
}
