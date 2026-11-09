use super::*;
use crate::runtime::test_support::record_run;
use crate::testing::test_app::TestApp;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

/// A VM lab of one Linux machine built for `arch` (`amd64`, `arm64`).
fn vms_on(arch: &str) -> String {
    format!(
        "version: 1\nname: ad\nnetworks: {{ lan: {{ cidr: 10.20.0.0/24 }} }}\nmachines:\n  dc: {{ os: debian@12, hardware: {{ arch: {arch} }}, networks: {{ lan: 10 }} }}\n"
    )
}

/// This machine's CPU, and another, as a spec names them.
fn arches() -> (&'static str, &'static str) {
    if std::env::consts::ARCH == "aarch64" { ("arm64", "amd64") } else { ("amd64", "arm64") }
}

/// A container lab with VM recipes too.
const DUAL: &str = "version: 1\nname: t\nnetworks: { lab: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { os: debian@12, app: { steps: [{ run: web.sh }] }, networks: { lab: 10 } }\n";

/// Lab `id` installed at [`SHA`] with `spec`, so its spec is read from the installed copy.
fn installed(t: &TestApp, id: &str, spec: &str) {
    let dir = t.lab(id, &[("isoloom.yml", spec), ("web.sh", "true\n")]);
    std::fs::write(dir.join(".cyberctf-commit"), SHA).unwrap();
}

/// `id` saved as the default server.
fn default_server(t: &TestApp, id: &str) {
    let id = id.to_owned();
    crate::runtime::server::store::update(&t.dirs(), |s| {
        s.default = Some(id);
        Ok(())
    })
    .unwrap();
}

/// Where lab `id`, declared `runtime`, starts with `ready` the hypervisors ready here.
async fn place(t: &TestApp, id: &str, runtime: Runtime, ready: &[Provider]) -> StartPlace {
    preferring(t, id, runtime, ready, None).await
}

/// [`place`], the player preferring hypervisor `preferred`.
async fn preferring(t: &TestApp, id: &str, runtime: Runtime, ready: &[Provider], preferred: Option<Provider>) -> StartPlace {
    let ready = ready.to_vec();
    Box::pin(default_place_with(&t.dirs(), (id, runtime), ("CyberCTF/lab", SHA), preferred, async move || ready)).await.unwrap()
}

/// On saved server `id`.
fn host(id: &str) -> StartPlace {
    StartPlace { host: Some(id.into()), ..StartPlace::default() }
}

/// A VM lab here, on hypervisor `p`.
fn here_on(p: Provider) -> StartPlace {
    StartPlace { provider: Some(p), hypervisor: Some(p), ..StartPlace::default() }
}

/// A container lab here in a VM on hypervisor `p`.
fn in_vm_on(p: Provider) -> StartPlace {
    StartPlace { provider: Some(p), ..StartPlace::default() }
}

#[tokio::test]
async fn a_vm_lab_runs_here_when_a_hypervisor_here_runs_it_else_on_the_default_server() {
    // Regression: the backend sent a VM lab to the default server whenever there was one (Home's
    // "start again", the website), while the lab page kept it here when a hypervisor was ready:
    // two rules for one question.
    let t = TestApp::new();
    let (native, foreign) = arches();
    installed(&t, "ad", &vms_on(native));
    assert_eq!(place(&t, "ad", Runtime::Vm, &[]).await, StartPlace::default(), "nowhere to go: here, which says so at start");
    let pve = t.host("pve", "proxmox", serde_json::json!({}));
    default_server(&t, &pve);
    assert_eq!(place(&t, "ad", Runtime::Vm, &[Provider::Virtualbox]).await, here_on(Provider::Virtualbox));
    // A hypervisor that can't run it (UTM: no network for its VMs) doesn't count.
    assert_eq!(place(&t, "ad", Runtime::Vm, &[Provider::Utm]).await, host("pve"));
    assert_eq!(place(&t, "ad", Runtime::Vm, &[]).await, host("pve"));
    // VMs built for another CPU run here only emulated.
    installed(&t, "other", &vms_on(foreign));
    assert_eq!(place(&t, "other", Runtime::Vm, &[Provider::Virtualbox]).await, host("pve"));
    assert_eq!(place(&t, "other", Runtime::Vm, &[Provider::Virtualbox, Provider::Qemu]).await, here_on(Provider::Qemu));
    // A lab whose targets leave Proxmox out stays here; ESXi runs it through Vagrant.
    installed(&t, "ad2", &format!("{}targets: [vagrant]\n", vms_on(native)));
    assert_eq!(place(&t, "ad2", Runtime::Vm, &[]).await, StartPlace::default());
    let esx = t.host("esx", "vmware_esxi", serde_json::json!({}));
    default_server(&t, &esx);
    assert_eq!(place(&t, "ad2", Runtime::Vm, &[]).await, host("esx"));
    // A cloud account is never the implicit place.
    let aws = t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true }));
    default_server(&t, &aws);
    assert_eq!(place(&t, "ad", Runtime::Vm, &[]).await, StartPlace::default());
}

#[tokio::test]
async fn a_lab_starts_again_where_it_last_ran() {
    // Regression: Home's "start again" sent a lab wherever the default rule said, not where it
    // last ran (as the lab page's Reset does).
    let t = TestApp::new();
    installed(&t, "ad", &vms_on(arches().0));
    let pve = t.host("pve", "proxmox", serde_json::json!({}));
    let esx = t.host("esx", "vmware_esxi", serde_json::json!({}));
    default_server(&t, &esx);
    // Last on Proxmox: there again, a hypervisor ready here notwithstanding (the one it would run
    // on if the player picks this machine).
    record_run(t.data(), "ad", Some(&pve), "proxmox", "absent");
    assert_eq!(place(&t, "ad", Runtime::Vm, &[Provider::Virtualbox]).await, StartPlace { hypervisor: Some(Provider::Virtualbox), ..host("pve") });
    // Last in a cloud account: never there unasked; the rule as if it never ran.
    record_run(t.data(), "ad", Some("aws1"), "cloud-vm", "absent");
    assert_eq!(place(&t, "ad", Runtime::Vm, &[]).await, host("esx"));
    // Last here as VMs: here on that hypervisor.
    record_run(t.data(), "ad", None, "vagrant", "absent");
    set_provider(&t, "ad", "libvirt");
    assert_eq!(place(&t, "ad", Runtime::Vm, &[]).await, here_on(Provider::Libvirt));
    // A container lab: in its helper VM on UTM, or as one VM per machine.
    installed(&t, "web", DUAL);
    assert_eq!(place(&t, "web", Runtime::Docker, &[]).await, StartPlace::default(), "never ran: on Docker here");
    record_run(t.data(), "web", None, "docker-vm", "absent");
    set_provider(&t, "web", "utm");
    assert_eq!(place(&t, "web", Runtime::Docker, &[]).await, in_vm_on(Provider::Utm));
    record_run(t.data(), "web", None, "vagrant", "parked");
    set_provider(&t, "web", "virtualbox");
    assert_eq!(place(&t, "web", Runtime::Docker, &[]).await, StartPlace { per_machine_vm: true, ..in_vm_on(Provider::Virtualbox) });
    // A server no longer saved: as if it never ran there.
    record_run(t.data(), "web", Some("gone"), "docker-vm", "absent");
    assert_eq!(place(&t, "web", Runtime::Docker, &[]).await, StartPlace::default());
    // A cloud-services lab is picked on its page.
    assert_eq!(place(&t, "goat", Runtime::Cloud, &[]).await, StartPlace::default());
}

#[tokio::test]
async fn a_vm_lab_starts_here_on_the_preferred_hypervisor_when_it_is_ready_and_runs_it() {
    // Regression: the webview picked a VM lab's hypervisor itself (the Settings choice first),
    // dropping the backend's: two rules for one question, and a test that couldn't tell them apart.
    let t = TestApp::new();
    installed(&t, "ad", &vms_on(arches().0));
    let ready = [Provider::Virtualbox, Provider::Libvirt];
    assert_eq!(preferring(&t, "ad", Runtime::Vm, &ready, Some(Provider::Libvirt)).await, here_on(Provider::Libvirt));
    assert_eq!(preferring(&t, "ad", Runtime::Vm, &ready, None).await, here_on(Provider::Virtualbox), "no choice: the first ready");
    // Preferred but not ready, or ready but unable to run it (UTM: no network for its VMs): not it.
    assert_eq!(preferring(&t, "ad", Runtime::Vm, &ready, Some(Provider::Parallels)).await, here_on(Provider::Virtualbox));
    let utm = [Provider::Utm, Provider::Virtualbox];
    assert_eq!(preferring(&t, "ad", Runtime::Vm, &utm, Some(Provider::Utm)).await, here_on(Provider::Virtualbox));
    // Where it last ran here comes first: its VMs are there.
    record_run(t.data(), "ad", None, "vagrant", "absent");
    set_provider(&t, "ad", "virtualbox");
    assert_eq!(preferring(&t, "ad", Runtime::Vm, &ready, Some(Provider::Libvirt)).await, here_on(Provider::Virtualbox));
}

#[tokio::test]
async fn a_lab_last_on_a_server_is_an_error_when_the_saved_servers_cant_be_read() {
    // Regression: an unreadable server.json read as "no such server", so a lab last run on
    // Proxmox started here instead.
    let t = TestApp::new();
    installed(&t, "ad", &vms_on(arches().0));
    let pve = t.host("pve", "proxmox", serde_json::json!({}));
    record_run(t.data(), "ad", Some(&pve), "proxmox", "absent");
    std::fs::write(t.data().join("server.json"), "{ not json").unwrap();
    let ready = vec![Provider::Virtualbox];
    let place = Box::pin(default_place_with(&t.dirs(), ("ad", Runtime::Vm), ("CyberCTF/lab", SHA), None, async move || ready)).await;
    assert!(place.is_err(), "{place:?}");
}

/// Lab `id`'s recorded run used Vagrant's `provider`.
fn set_provider(t: &TestApp, id: &str, provider: &str) {
    let path = t.data().join("runs").join(format!("cyberctf-{id}")).join("record.json");
    let mut record: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    record["provider"] = provider.into();
    std::fs::write(path, record.to_string()).unwrap();
}
