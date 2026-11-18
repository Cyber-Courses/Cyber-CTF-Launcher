use super::*;

#[tokio::test]
async fn the_vm_test_picks_no_hypervisor_without_vagrant() {
    // Regression: the test's own copy of "ready" never asked whether Vagrant was there, and
    // picked a hypervisor whose every Vagrant call would then fail.
    use isothrum_runner::fake::{FakeProcesses, Reply};
    let tools = std::sync::Arc::new(FakeProcesses::model(|call| if call.program == "vagrant" { Reply::missing() } else { Reply::ok("") }));
    let runner: std::sync::Arc<dyn isothrum_runner::ProcessRunner> = tools.clone();
    let root = crate::testing::temp_dir("vm-test-no-vagrant");
    let _machine = crate::runtime::providers::testing::Machine::set(runner, crate::runtime::providers::testing::host(&root));
    assert_eq!(test_provider(Some(Provider::Virtualbox)).await, None);
    assert_eq!(tools.made("vagrant --version").len(), 1, "{:?}", tools.calls());
}

#[test]
fn no_hypervisor_ready_says_what_keeps_the_preferred_one_from_running_vms() {
    use crate::runtime::providers::{ProviderStatus, Unavailable, said};
    // An installed hypervisor, as the check sends it, kept from running VMs by `reason`.
    let status = |provider: Provider, reason: serde_json::Value| -> ProviderStatus {
        serde_json::from_value(serde_json::json!({
            "provider": provider, "remote": false, "available": false, "hypervisor": true, "plugin": null,
            "pluginInstalled": true, "utility": null, "needs": [], "reason": reason, "accountLacks": null,
        }))
        .unwrap()
    };
    let kvm = serde_json::json!({ "code": "kvmDenied", "args": { "group": "kvm" } });
    let statuses = [status(Provider::Virtualbox, serde_json::json!({ "code": "virtualboxStuck" })), status(Provider::Qemu, kvm.clone())];
    let kvm: Unavailable = serde_json::from_value(kvm).unwrap();
    assert_eq!(none_ready(Some(Provider::Qemu), &statuses), format!("QEMU can't run VMs here: {}", said(&kvm)));
    assert!(none_ready(None, &statuses).starts_with("VirtualBox can't run VMs here: "), "the first installed one");
    assert_eq!(none_ready(Some(Provider::Qemu), &[]), "no local hypervisor with its Vagrant plugin is installed");
}

#[test]
fn the_uninstaller_removes_the_test_vm_as_the_app_does() {
    let under = format!("\"$env:LOCALAPPDATA\\{}\\selftest\\vm\\\"", crate::platform::config::KEYCHAIN_SERVICE);
    let script = format!(
        "# Written by `cargo test` (machine/selftest/vm.rs) from the app's own removal: don't edit.\n# Run by installer-hooks.nsh before the uninstaller removes the app's folders.\n{}",
        isothrum_machine::selftest::hypervisor::hyperv_leftover_script(VM_NAME, &under)
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../packaging/windows/remove-test-vm.ps1");
    let crlf = script.replace('\n', "\r\n");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(crlf.as_str()) {
        std::fs::write(&path, &crlf).unwrap();
    }
    assert!(script.contains(r#"StartsWith("$env:LOCALAPPDATA\org.cyberctf.desktop\selftest\vm\", "#), "{script}");
}

#[test]
fn each_step_is_said_in_the_apps_words() {
    use isothrum_runner::Provider as V;
    assert_eq!(named(Step::Network), ("network", "VM reachable on a lab network"));
    assert_eq!(named(Step::Networks), ("networks", "Remove the networks the test made"));
    assert_eq!(detail(Found::Box { name: "generic/alpine319", held: true }).as_deref(), Some("generic/alpine319 (ready)"));
    assert_eq!(detail(Found::Booted { name: "utm/bookworm", provider: V::Utm }).as_deref(), Some("utm/bookworm on utm"));
    assert_eq!(detail(Found::Reached { address: ADDRESSES[1] }).as_deref(), Some("this machine → 192.168.56.249"));
    assert_eq!(detail(Found::AddressesTaken).as_deref(), Some("every test address (192.168.56.250…) is taken on this machine"));
    assert_eq!(detail(Found::NoPrivateNetwork(V::Qemu)).as_deref(), Some("qemu has no private networks"));
    assert_eq!(detail(Found::Removed(vec!["VirtualBox"])).as_deref(), Some("removed from VirtualBox"));
    assert_eq!(detail(Found::NothingToProbe).as_deref(), Some("nothing to probe for this provider"));
    assert_eq!(
        error(Failure::Foreign { hypervisor: "VirtualBox" }).to_string(),
        "VirtualBox has a VM named \"CyberCTF test VM\" that isn't this test's, so it was left alone. Rename or remove it in VirtualBox, then test again."
    );
    assert_eq!(error(Failure::NoReply { address: ADDRESSES[0], last: "no reply".into() }).to_string(), "192.168.56.250 did not answer ping: no reply");
    assert_eq!(
        error(Failure::NoTestBox { hypervisor: "hyperv", arch: isothrum_runner::Arch::Arm64 }).to_string(),
        "no test VM exists yet for hyperv on arm64 computers, so the test has nothing to boot here"
    );
}
