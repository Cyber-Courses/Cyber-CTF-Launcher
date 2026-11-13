use super::*;

#[tokio::test]
async fn the_vm_test_picks_no_hypervisor_without_vagrant() {
    // Regression: the test's own copy of "ready" never asked whether Vagrant was there, and
    // picked a hypervisor whose every Vagrant call would then fail.
    use crate::platform::exec::fake::{Tools, ok};
    let tools = Tools::new(|program, _| if program == "vagrant" { None } else { ok("") });
    assert_eq!(test_provider(Some(Provider::Virtualbox)).await, None);
    assert!(tools.called("vagrant --version"), "{:?}", tools.calls());
}

#[tokio::test]
async fn the_test_vm_takes_an_address_nothing_answers_on_yet() {
    // Regression: the test VM always took .250, so another VM there made the ping pass.
    use crate::platform::exec::fake::{Tools, fail, ok};
    let _tools = Tools::new(|program, args| match (program, args.last().copied()) {
        ("ping", Some("192.168.56.250")) => ok("64 bytes from 192.168.56.250"),
        ("ping", _) => fail("Request timeout"),
        _ => ok(""),
    });
    assert_eq!(free_ip().await, Some("192.168.56.249"));
    let v = vagrantfile(Provider::Virtualbox, "x", free_ip().await, Path::new("/t"));
    assert!(v.contains("ip: \"192.168.56.249\"") && !v.contains(".250"), "{v}");
    let _all = Tools::strict_rules(&[("ping -c 1", crate::platform::exec::fake::ok("64 bytes"))]);
    assert_eq!(free_ip().await, None, "every address answers");
    assert!(!vagrantfile(Provider::Virtualbox, "x", None, Path::new("/t")).contains("private_network"));
}

#[test]
fn the_command_output_is_told_from_vagrants_own_lines() {
    assert_eq!(uname_line("warning: something\ncyberctf-uname:Linux 6.6 x86_64\nConnection to 172.20.0.5 closed.\n"), "Linux 6.6 x86_64");
    assert_eq!(uname_line("Connection to x closed.\n"), "");
    assert_eq!(uname_command(), "echo cyberctf-uname:$(uname -srm)");
    assert!(
        vagrantfile(Provider::Hyperv, "x", Some(VM_IPS[0]), Path::new("/t"))
            .contains("config.vm.network \"public_network\", bridge: ENV[\"ISOLOOM_HYPERV_SWITCH\"]")
    );
    assert!(!vagrantfile(Provider::Virtualbox, "x", Some(VM_IPS[0]), Path::new("/t")).contains("public_network"));
}

#[test]
fn the_qemu_test_vm_boots_with_the_acceleration_the_labs_ask_for() {
    // Regression: without a usable /dev/kvm the test VM fell back to software emulation
    // (`accel=tcg`) and passed on Debian 13, where every lab's `accel=kvm` was refused.
    let v = vagrantfile(Provider::Qemu, "generic/alpine319", Some(VM_IPS[0]), Path::new("/t"));
    assert!(v.contains("h.memory = \"1G\"") && !v.contains("h.machine") && !v.contains("accel") && !v.contains("h.cpu"), "{v}");
}

#[test]
fn no_hypervisor_ready_says_what_keeps_the_preferred_one_from_running_vms() {
    use crate::runtime::providers::{ProviderStatus, Unavailable};
    let status = |provider, reason: Option<Unavailable>| ProviderStatus {
        provider,
        remote: false,
        available: reason.is_none(),
        hypervisor: Some(true),
        plugin: None,
        plugin_installed: true,
        utility: None,
        needs: Vec::new(),
        reason,
        account_lacks: None,
    };
    let kvm = Unavailable::KvmDenied { group: "kvm".into() };
    let statuses = [status(Provider::Virtualbox, Some(Unavailable::VirtualboxStuck)), status(Provider::Qemu, Some(kvm.clone()))];
    assert_eq!(none_ready(Some(Provider::Qemu), &statuses), format!("QEMU can't run VMs here: {kvm}"));
    assert!(none_ready(None, &statuses).starts_with("VirtualBox can't run VMs here: "), "the first installed one");
    assert_eq!(none_ready(Some(Provider::Qemu), &[]), "no local hypervisor with its Vagrant plugin is installed");
}

#[test]
fn a_box_counts_only_for_its_provider_and_cpu() {
    let list = "bento/debian-12   (virtualbox, 202510.26.0, (amd64))\ngeneric/alpine319 (libvirt, 4.3.12)\n";
    assert!(box_listed(list, "bento/debian-12", "virtualbox", Arch::X86_64));
    assert!(!box_listed(list, "bento/debian-12", "virtualbox", Arch::Aarch64));
    assert!(!box_listed(list, "bento/debian-12", "parallels", Arch::X86_64));
    // No architecture listed: any CPU.
    assert!(box_listed(list, "generic/alpine319", "libvirt", Arch::Aarch64));
    assert!(!box_listed(list, "generic/alpine", "libvirt", Arch::Aarch64));
}

#[test]
fn box_listings_with_windows_endings_wide_spacing_or_look_alike_names_match_exactly() {
    // CRLF from Vagrant on Windows, tabs or runs of spaces before the parenthesis, a name
    // that merely starts with the wanted one, and the empty listing.
    let list = "bento/debian-12\t(virtualbox, 1)\r\nbento/debian-120    (virtualbox, 1)\r\ngeneric/alpine319 (vmware_desktop, 4.3.12, (arm64))\r\n";
    assert!(box_listed(list, "bento/debian-12", "virtualbox", Arch::X86_64));
    assert!(box_listed(list, "bento/debian-120", "virtualbox", Arch::Aarch64));
    assert!(!box_listed(list, "bento/debian-1", "virtualbox", Arch::X86_64));
    assert!(box_listed(list, "generic/alpine319", "vmware_desktop", Arch::Aarch64));
    assert!(!box_listed(list, "generic/alpine319", "vmware_desktop", Arch::X86_64), "an ARM box is not an x86 one");
    assert!(!box_listed(list, "generic/alpine319", "vmware", Arch::Aarch64), "the provider id is whole");
    assert!(!box_listed("There are no installed boxes! Use `vagrant box add` to add some.\n", "generic/alpine319", "qemu", Arch::X86_64));
    assert!(!box_listed("", "generic/alpine319", "qemu", Arch::X86_64));
}

#[test]
fn the_test_vm_is_on_a_private_network_where_the_provider_has_one() {
    assert!(vagrantfile(Provider::Virtualbox, "x", Some(VM_IPS[0]), Path::new("/t")).contains(VM_IPS[0]));
    assert!(!vagrantfile(Provider::Utm, "x", Some(VM_IPS[0]), Path::new("/t")).contains(VM_IPS[0]));
    assert_eq!(ping_args("10.0.0.1")[4], "10.0.0.1");
}

/// The Windows uninstaller's removal of the test VM (packaging/windows): the app's own, for the
/// test's folder in the app's cache (`%LOCALAPPDATA%\<identifier>\selftest\vm`). Written by
/// this test; a change shows in the diff, and CI fails on one left uncommitted.
#[test]
fn the_uninstaller_removes_the_test_vm_as_the_app_does() {
    let under = format!("\"$env:LOCALAPPDATA\\{}\\selftest\\vm\\\"", crate::platform::config::KEYCHAIN_SERVICE);
    let script = format!(
        "# Written by `cargo test` (machine/selftest/vm.rs) from the app's own removal: don't edit.\n# Run by installer-hooks.nsh before the uninstaller removes the app's folders.\n{}",
        crate::platform::exec::leftover_script(VM_NAME, &under)
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../packaging/windows/remove-test-vm.ps1");
    let crlf = script.replace('\n', "\r\n");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(crlf.as_str()) {
        std::fs::write(&path, &crlf).unwrap();
    }
    assert!(script.contains(r#"StartsWith("$env:LOCALAPPDATA\org.cyberctf.desktop\selftest\vm\", "#), "{script}");
}

#[test]
fn a_virtualbox_test_vm_carries_its_folder_so_a_leftover_is_known_as_the_tests() {
    let v = vagrantfile(Provider::Virtualbox, "x", None, Path::new(r"C:\Users\O'Brien\selftest\vm"));
    assert!(v.contains(r#"h.customize ["modifyvm", :id, "--description", 'cyberctf-selftest:C:\\Users\\O\'Brien\\selftest\\vm']"#), "{v}");
}

#[tokio::test]
async fn an_earlier_tests_vm_is_removed_before_the_test_and_a_foreign_one_stops_it() {
    // Regression: a VM left running that Vagrant no longer knew made every later test fail at
    // `vagrant up`; the destroy's failure was dropped and nothing removed the VM.
    use crate::platform::exec::fake::{Tools, fail, ok};
    let dir = std::env::temp_dir().join(format!("cyberctf-selftest-vm-{}", rand::random::<u32>()));
    std::fs::create_dir_all(dir.join(".vagrant").join("machines").join("default").join("virtualbox")).unwrap();
    let ours = format!("description=\"{}\"\n", leftover::vbox_description(&dir));
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = events.clone();
    let r = Reporter::new(move |e: crate::runtime::selftest_report::Event| seen.lock().unwrap().push(format!("{}:{}", e.step, e.state)));
    let tools = Tools::strict_rules(&[
        ("VBoxManage controlvm", crate::platform::exec::fake::ok("")),
        ("VBoxManage unregistervm", crate::platform::exec::fake::ok("")),
        ("vagrant destroy", fail("The machine isn't created")),
        ("VBoxManage list vms", ok("\"CyberCTF test VM\" {abc}\n")),
        ("VBoxManage showvminfo abc", ok(&ours)),
        ("vagrant --version", fail("stop here")),
    ]);
    crate::testing::logs::take_warnings();
    let _ = run_test(&dir, None, &r).await;
    assert!(tools.called("VBoxManage unregistervm abc --delete"), "{:?}", tools.calls());
    assert!(!dir.join(".vagrant").exists(), "Vagrant's stale state is gone");
    assert!(tools.called("vagrant --version"), "the test went on: {:?}", tools.calls());
    assert_eq!(events.lock().unwrap()[0], "leftover:ok");
    assert!(crate::testing::logs::take_warnings().iter().any(|w| w.contains("The machine isn't created")), "the destroy's failure is logged");

    let tools = Tools::strict_rules(&[("VBoxManage list vms", ok("\"CyberCTF test VM\" {abc}\n")), ("VBoxManage showvminfo abc", ok("description=\"\"\n"))]);
    events.lock().unwrap().clear();
    let err = run_test(&dir, None, &r).await.unwrap_err();
    assert!(err.to_string().contains("isn't this test's"), "{err}");
    assert!(!tools.called("unregistervm") && !tools.called("vagrant --version"), "{:?}", tools.calls());
    assert_eq!(*events.lock().unwrap(), ["leftover:fail"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_provider_gets_its_own_vm_settings() {
    for (p, needle) in [
        (Provider::Virtualbox, "h.name = \"CyberCTF test VM\""),
        (Provider::VmwareDesktop, "h.vmx[\"displayName\"]"),
        (Provider::Parallels, "h.name = "),
        (Provider::Libvirt, "h.default_prefix = \"cyberctf-\""),
        (Provider::Hyperv, "h.vmname = "),
        (Provider::Utm, "h.name = "),
    ] {
        let v = vagrantfile(p, "box/x", Some(VM_IPS[0]), Path::new("/t"));
        assert!(v.contains(needle) && v.contains(&format!("config.vm.provider \"{}\"", p.id())), "{v}");
        assert!(v.starts_with("Vagrant.configure") && v.ends_with("end\n"));
    }
}

#[test]
fn probes_and_boxes_per_provider() {
    assert_eq!(responds(Provider::Virtualbox).map(|r| r.0), Some("VBoxManage"));
    assert_eq!(responds(Provider::VmwareDesktop).map(|r| r.0), Some("vmrun"));
    assert_eq!(responds(Provider::Parallels).map(|r| r.0), Some("prlctl"));
    assert_eq!(responds(Provider::Libvirt).map(|r| r.0), Some("virsh"));
    assert!(responds(Provider::Utm).is_some_and(|r| r.0.ends_with("utmctl")));
    assert!(responds(Provider::Qemu).is_none());
    assert_eq!(candidate_boxes(Provider::Virtualbox, Arch::X86_64), ["generic/alpine319", "bento/debian-12"]);
    assert_eq!(candidate_boxes(Provider::Parallels, Arch::Aarch64), ["bento/debian-12"]);
    assert_eq!(candidate_boxes(Provider::Utm, Arch::Aarch64), ["utm/bookworm"]);
    assert_eq!(candidate_boxes(Provider::Qemu, Arch::Aarch64), ["generic/alpine319"]);
    assert!(has_private_network(Provider::Libvirt) && !has_private_network(Provider::Qemu) && !has_private_network(Provider::Hyperv));
    // QEMU stores and lists its box as libvirt (vagrant-qemu boots a libvirt box); others use their own id.
    assert_eq!(box_provider(Provider::Qemu), "libvirt");
    assert_eq!(box_provider(Provider::Virtualbox), "virtualbox");
    assert_eq!(box_provider(Provider::Libvirt), "libvirt");
}
