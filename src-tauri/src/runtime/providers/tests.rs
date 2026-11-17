//! The launcher's places and its player's preference. Detection itself (what is installed, the
//! first missing layer, the plugins and versions read) is isothrum-machine's, tested there.

use std::sync::Arc;

use isothrum_runner::fake::{FakeProcesses, Reply};

use super::*;
use isothrum_machine::Arch;

#[test]
fn the_preferred_hypervisor_is_picked_when_ready_else_the_first_ready_one() {
    let ready = [Provider::Virtualbox, Provider::Qemu];
    assert_eq!(pick(Some(Provider::Qemu), &ready), Some(Provider::Qemu));
    assert_eq!(pick(Some(Provider::Parallels), &ready), Some(Provider::Virtualbox), "not ready: the first ready one");
    assert_eq!(pick(None, &ready), Some(Provider::Virtualbox));
    assert_eq!(pick(Some(Provider::Qemu), &[]), None);
}

#[test]
fn vmwares_desktop_hypervisor_is_named_for_the_os_and_nothing_else_is() {
    assert_eq!(Provider::VmwareDesktop.label_on(HostOs::Macos), "VMware Fusion");
    assert_eq!(Provider::VmwareDesktop.label_on(HostOs::Windows), "VMware Workstation");
    assert_eq!(Provider::VmwareDesktop.label_on(HostOs::Linux), "VMware Workstation");
    for p in Provider::ALL.iter().chain(&Provider::CLOUDS).filter(|p| **p != Provider::VmwareDesktop) {
        for os in isothrum_machine::host::ALL_OSES {
            assert_eq!(p.label_on(os), p.label(), "{p:?} on {os:?}");
        }
    }
}

#[test]
fn every_vagrant_provider_is_the_launchers_of_the_same_id_and_the_rest_run_through_terraform() {
    // The launcher's places and Isothrum's Vagrant providers agree: each Vagrant provider is the
    // launcher's of the same id, and back.
    for p in Provider::ALL.into_iter().chain(Provider::CLOUDS) {
        if let Some(vagrant) = p.vagrant() {
            assert_eq!(serde_json::to_value(vagrant).unwrap(), p.id(), "{p:?}");
            assert_eq!(Provider::from(vagrant), p);
            assert_eq!(p.plugin(), vagrant.plugin(), "{p:?}");
        } else {
            assert!(p == Provider::Proxmox || p.is_cloud(), "{p:?}");
            assert_eq!(p.plugin(), None, "{p:?}: not a Vagrant provider");
        }
    }
    assert_eq!(Provider::Utm.plugin(), Some("vagrant_utm"));
    assert_eq!(Provider::Virtualbox.plugin(), None, "built in");
}

#[test]
fn all_detectable_providers_exclude_clouds() {
    for p in Provider::ALL {
        assert!(!p.is_cloud(), "{p:?}");
    }
    assert_eq!(Provider::ALL.len(), 9);
}

#[test]
fn remote_and_cloud_classification() {
    // Servers are remote but not cloud; the billed accounts are both.
    for server in Provider::SERVERS {
        assert!(server.is_remote() && !server.is_cloud());
    }
    for cloud in [Provider::Aws, Provider::Azure, Provider::Gcp, Provider::DigitalOcean, Provider::Linode, Provider::Oci] {
        assert!(cloud.is_remote() && cloud.is_cloud());
    }
    // Local hypervisors are neither.
    for local in [Provider::Virtualbox, Provider::Hyperv, Provider::Parallels, Provider::Libvirt, Provider::Qemu, Provider::Utm] {
        assert!(!local.is_remote() && !local.is_cloud());
    }
}

#[test]
fn ids_are_unique_and_round_trip_through_serde() {
    let all: Vec<Provider> = Provider::ALL.into_iter().chain(Provider::CLOUDS).collect();
    assert_eq!(all.len(), 15, "every provider");
    let mut ids: Vec<&str> = all.iter().map(|p| p.id()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), all.len(), "provider ids must be unique");
    // id() is the serde wire name, and it round-trips back to the same provider.
    for p in all {
        assert_eq!(serde_json::to_value(p).unwrap(), p.id());
        assert_eq!(Provider::from_id(p.id()), Some(p));
        let round: Provider = serde_json::from_value(serde_json::Value::String(p.id().to_string())).unwrap();
        assert_eq!(round, p);
    }
}

#[test]
fn servers_and_cloud_accounts_are_offered_everywhere_and_share_no_folder() {
    let on = |os, arch| crate::runtime::providers::testing::host_on(os, arch, None);
    // The hypervisors follow isothrum-machine's table: QEMU never on Windows (#314), VirtualBox
    // and VMware not on Windows on Arm.
    assert!(!Provider::Qemu.runs_on(HostOs::Windows) && !Provider::Qemu.offered_on(&on(HostOs::Windows, Arch::Amd64), true));
    let arm_pc = on(HostOs::Windows, Arch::Arm64);
    assert!(!Provider::Virtualbox.offered_on(&arm_pc, true) && !Provider::VmwareDesktop.offered_on(&arm_pc, true));
    assert!(Provider::Hyperv.offered_on(&arm_pc, false));
    assert!(Provider::Qemu.shares_folders_on(HostOs::Linux) && !Provider::Qemu.shares_folders_on(HostOs::Macos));
    for p in Provider::SERVERS.into_iter().chain(Provider::CLOUDS) {
        for os in isothrum_machine::host::ALL_OSES {
            assert!(p.runs_on(os), "{p:?} on {os:?}");
            for arch in isothrum_machine::host::ALL_ARCHES {
                assert!(p.offered_on(&on(os, arch), false), "{p:?} on {os:?} {arch:?}");
            }
            assert!(!p.shares_folders_on(os), "{p:?}");
        }
        assert!(p.offered_on(&arm_pc, false) && p.vendor().is_none(), "{p:?}");
    }
}

#[test]
fn a_shared_folder_on_libvirt_or_qemu_without_virtiofsd_is_refused_with_its_code_before_anything_starts() {
    use crate::error::Code;
    for (provider, name) in [(Provider::Libvirt, "libvirt"), (Provider::Qemu, "QEMU")] {
        let refused = share_on(provider, HostOs::Linux, |p| {
            assert_eq!(Provider::from(p), provider, "asked of the lab's own hypervisor");
            false
        })
        .unwrap_err();
        assert_eq!(refused.code(), Code::VirtiofsdMissing { hypervisor: name.into() });
        let said = refused.to_string();
        assert!(said.starts_with(&format!("{name} can't share")) && said.contains("apt-get install virtiofsd"), "says what to install: {said}");
        assert!(share_on(provider, HostOs::Linux, |_| true).is_ok());
    }
    // Another hypervisor, QEMU on macOS, or a place that isn't a Vagrant provider, never asks.
    assert!(share_on(Provider::Virtualbox, HostOs::Linux, |_| panic!("not asked")).is_ok());
    assert!(share_on(Provider::Qemu, HostOs::Macos, |_| panic!("not asked")).is_ok());
    assert!(share_on(Provider::Proxmox, HostOs::Linux, |_| panic!("not asked")).is_ok());
}

/// Every reason isothrum-machine gives, with sample values: what the webview's catalogue
/// (`machine.unavailable.*`, in every language) must say.
fn every_reason() -> Vec<Unavailable> {
    let wire = [
        serde_json::json!({ "code": "vagrantMissing" }),
        serde_json::json!({ "code": "virtualboxStuck" }),
        serde_json::json!({ "code": "hypervOff" }),
        serde_json::json!({ "code": "hypervNeedsAdmin" }),
        serde_json::json!({ "code": "hypervisorStopped" }),
        serde_json::json!({ "code": "mkisofsMissing" }),
        serde_json::json!({ "code": "virtiofsdMissing", "args": { "hypervisor": "QEMU" } }),
        serde_json::json!({ "code": "notInstalled", "args": { "program": "virsh" } }),
        serde_json::json!({ "code": "notInstalled", "args": { "program": null } }),
        serde_json::json!({ "code": "newLogin", "args": { "group": "libvirt" } }),
        serde_json::json!({ "code": "noKvm" }),
        serde_json::json!({ "code": "kvmDenied", "args": { "group": "kvm" } }),
        serde_json::json!({ "code": "kvmUnusable", "args": { "error": "Device or resource busy" } }),
        serde_json::json!({ "code": "hypervisorTooOld", "args": { "name": "UTM", "version": "4.4.5", "min": "4.6.5" } }),
        serde_json::json!({ "code": "pluginTooOld", "args": { "plugin": "vagrant_utm", "version": "0.1.0", "min": "0.2.0" } }),
        serde_json::json!({ "code": "pluginMissing", "args": { "plugin": "vagrant-libvirt" } }),
        serde_json::json!({ "code": "pluginsUnknown", "args": { "plugin": "vagrant-libvirt", "reason": "exited with code 1" } }),
        serde_json::json!({ "code": "vmwareUtilityStopped" }),
        serde_json::json!({ "code": "vmwareUtilityMissing" }),
        serde_json::json!({ "code": "virtualboxDriver", "args": { "kernel": "6.12.86+deb13-amd64" } }),
        serde_json::json!({ "code": "kernelRestart", "args": { "kernel": "6.12.116+deb13-amd64" } }),
        serde_json::json!({ "code": "kvmHoldsVirtualization" }),
    ];
    wire.into_iter().map(|w| serde_json::from_value(w.clone()).unwrap_or_else(|e| panic!("{w}: {e}"))).collect()
}

#[test]
fn every_reason_says_its_english_catalogue_sentence() {
    for reason in every_reason() {
        let wire = serde_json::to_value(&reason).unwrap();
        let text = said(&reason);
        assert_ne!(text, wire["code"].as_str().unwrap(), "no sentence for {wire}");
        assert!(!text.contains('{'), "a placeholder left in {wire}: {text}");
    }
    let too_old: Unavailable =
        serde_json::from_value(serde_json::json!({ "code": "hypervisorTooOld", "args": { "name": "UTM", "version": "4.4.5", "min": "4.5.0" } })).unwrap();
    assert_eq!(said(&too_old), "UTM 4.4.5 is too old: 4.5.0 or later is needed. Update UTM, then re-check.");
    let not_named: Unavailable = serde_json::from_value(serde_json::json!({ "code": "notInstalled", "args": { "program": null } })).unwrap();
    assert_eq!(said(&not_named), "Not installed.");
}

#[test]
fn every_language_says_every_reason() {
    let codes: Vec<String> = every_reason().iter().map(|r| serde_json::to_value(r).unwrap()["code"].as_str().unwrap().to_owned()).collect();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/messages");
    for lang in ["en", "de", "es", "fr", "ja", "pt-BR"] {
        let text = std::fs::read_to_string(dir.join(lang).join("machine.json")).unwrap();
        let section = &serde_json::from_str::<serde_json::Value>(&text).unwrap()["unavailable"];
        for code in &codes {
            assert!(section[code].is_string(), "{lang} lacks machine.unavailable.{code}");
        }
        assert!(section["notInstalledNoProgram"].is_string(), "{lang}");
    }
}

#[tokio::test]
async fn the_hypervisors_ready_are_the_vagrant_providers_detected_available_and_none_without_vagrant() {
    // A VirtualBox that answers, with Vagrant: ready; nothing else installed.
    let root = crate::testing::temp_dir("providers-ready");
    let tools = FakeProcesses::model(|call| match (call.program.as_str(), call.args.first().map(String::as_str)) {
        ("VBoxManage", _) => Reply::ok("7.1.4r165100"),
        ("vagrant", Some("plugin")) => Reply::ok(""),
        _ => Reply::missing(),
    });
    let mut host = testing::host(&root);
    host.os = isothrum_runner::HostOs::Macos;
    host.arch = isothrum_runner::Arch::Amd64;
    let _machine = testing::Machine::set(Arc::new(tools), host);
    let statuses = detect(true).await;
    assert_eq!(ready(&statuses, true), [Provider::Virtualbox]);
    assert_eq!(ready(&statuses, false), Vec::<Provider>::new(), "none without Vagrant");
    let esxi = status(Provider::VmwareEsxi).await.expect("offered everywhere");
    assert!(esxi.remote && !esxi.available, "{esxi:?}");
    assert_eq!(status(Provider::Proxmox).await, None, "not a Vagrant provider");
}
