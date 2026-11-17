//! The settings: their defaults, each change, and what can't be saved.

use std::collections::BTreeMap;

use super::{Change, PortMode, Settings, TestRecord, TestResult, VmForm, settings_change, settings_get};
use crate::machine::system::docker::Engine;
use crate::runtime::placement::Placement;
use crate::runtime::providers::Provider;
use crate::testing::test_app::TestApp;

fn record(result: TestResult, at: u64, why: Option<&str>) -> TestRecord {
    TestRecord { result, at, why: why.map(str::to_owned) }
}

#[test]
fn a_player_who_chose_nothing_gets_the_defaults() {
    let t = TestApp::new();
    let s = settings_get(&t.dirs()).unwrap();
    assert_eq!(s, Settings::default());
    assert!(s.attack_auto && !s.attack_desktop, "the box starts with each lab, headless");
    assert_eq!((s.attack_image, s.vm_provider, s.docker_engine, s.port_mode, s.vm_form), (None, None, None, None, None));
}

#[test]
fn each_setting_is_kept_and_none_puts_it_back_to_its_default() {
    let t = TestApp::new();
    let dirs = t.dirs();
    let set = [
        Change::AttackImage(Some("kalilinux/kali-rolling".into())),
        Change::AttackAuto(false),
        Change::AttackDesktop(true),
        Change::VmProvider(Some(Provider::Utm)),
        Change::DockerEngine(Some(Engine::Orbstack)),
        Change::PortMode(Some(PortMode::Default)),
        Change::VmForm(Some(VmForm::Vms)),
    ];
    settings_change(&dirs, &set).unwrap();
    let s = settings_get(&dirs).unwrap();
    assert_eq!(s.attack_image.as_deref(), Some("kalilinux/kali-rolling"));
    assert!(!s.attack_auto && s.attack_desktop);
    assert_eq!(
        (s.vm_provider, s.docker_engine, s.port_mode, s.vm_form),
        (Some(Provider::Utm), Some(Engine::Orbstack), Some(PortMode::Default), Some(VmForm::Vms))
    );
    let reset = [Change::AttackImage(None), Change::VmProvider(None), Change::DockerEngine(None), Change::PortMode(None), Change::VmForm(None)];
    let s = settings_change(&dirs, &reset).unwrap();
    assert_eq!((s.attack_image, s.vm_provider, s.docker_engine, s.port_mode, s.vm_form), (None, None, None, None, None));
}

#[test]
fn the_default_attack_image_or_an_earlier_one_is_never_saved() {
    // Saved, it would hold the player on that image when a later launcher pins a newer one.
    let t = TestApp::new();
    let dirs = t.dirs();
    let current = crate::runtime::attack_box::default_image().to_owned();
    let earlier = crate::runtime::attack_box::earlier_default_images();
    assert!(earlier.iter().any(|i| i == "cyberctf/attack-box") && earlier.iter().any(|i| i == "cyberctf/attack-box:latest"), "{earlier:?}");
    for image in [current.as_str(), "cyberctf/attack-box", "cyberctf/attack-box:latest", "  ", ""].into_iter().chain(earlier.iter().map(String::as_str)) {
        settings_change(&dirs, &[Change::AttackImage(Some("parrotsec/security".into()))]).unwrap();
        let s = settings_change(&dirs, &[Change::AttackImage(Some(image.to_owned()))]).unwrap();
        assert_eq!(s.attack_image, None, "{image:?} follows the default");
    }
    let s = settings_change(&dirs, &[Change::AttackImage(Some("  nwodtuhs/exegol:free ".into()))]).unwrap();
    assert_eq!(s.attack_image.as_deref(), Some("nwodtuhs/exegol:free"), "trimmed");
}

#[test]
fn a_server_or_a_cloud_is_no_hypervisor_of_this_machine_and_a_refused_batch_changes_nothing() {
    let t = TestApp::new();
    let dirs = t.dirs();
    for remote in [Provider::Proxmox, Provider::VmwareEsxi, Provider::Aws] {
        let err = settings_change(&dirs, &[Change::AttackAuto(false), Change::VmProvider(Some(remote))]).unwrap_err().to_string();
        assert!(err.contains("isn't a hypervisor of this machine"), "{err}");
    }
    assert_eq!(settings_get(&dirs).unwrap(), Settings::default(), "all or none");
}

#[test]
fn the_tests_and_last_runs_are_kept_by_key_and_a_last_run_is_forgotten_with_its_place() {
    let t = TestApp::new();
    let dirs = t.dirs();
    let failed = record(TestResult::Fail, 2, Some("no KVM"));
    settings_change(
        &dirs,
        &[
            Change::SelfTest { key: "vm.utm".into(), record: record(TestResult::Ok, 1, None) },
            Change::SelfTest { key: "vm".into(), record: failed.clone() },
            Change::ServerTest { host: "pve1".into(), record: record(TestResult::Ok, 3, None) },
            Change::LastRun { lab: "web".into(), at: Some(10), place: Some(Placement::Server { host: "pve1".into() }) },
            Change::LastRun { lab: "kube".into(), at: Some(20), place: Some(Placement::Server { host: "pve1".into() }) },
        ],
    )
    .unwrap();
    let here = Placement::Vms { provider: Provider::Qemu };
    let s = settings_change(
        &dirs,
        &[Change::LastRun { lab: "web".into(), at: None, place: None }, Change::LastRun { lab: "kube".into(), at: Some(30), place: Some(here.clone()) }],
    )
    .unwrap();
    assert_eq!(s.last_places.get("kube"), Some(&here), "kube's later start's place");
    // A resume: a later time, the place kept.
    let s = settings_change(&dirs, &[Change::LastRun { lab: "kube".into(), at: Some(40), place: None }]).unwrap();
    assert_eq!(s.self_tests.get("vm"), Some(&failed));
    assert_eq!(s.self_tests.get("vm.utm").map(|r| r.result), Some(TestResult::Ok));
    assert_eq!(s.server_tests.get("pve1").map(|r| r.at), Some(3));
    assert_eq!(s.last_runs, BTreeMap::from([("kube".to_owned(), 40)]), "web forgotten, kube's later start kept");
    assert_eq!(s.last_places, BTreeMap::from([("kube".to_owned(), here)]), "web's place forgotten with it, kube's the later start's, kept by the resume");
}

#[test]
fn a_kept_value_this_version_cant_read_reads_as_the_default_and_is_said() {
    let t = TestApp::new();
    let dirs = t.dirs();
    crate::store::open(&dirs)
        .unwrap()
        .write(|tx| {
            crate::store::settings::set(tx, super::key::DOCKER_ENGINE, Some(&"a-newer-engine"))?;
            crate::store::settings::set(tx, super::key::ATTACK_AUTO, Some(&"yes"))?;
            crate::store::settings::set(tx, super::key::VM_PROVIDER, Some(&"proxmox"))
        })
        .unwrap();
    crate::testing::logs::take_warnings();
    let s = settings_get(&dirs).unwrap();
    assert_eq!((s.docker_engine, s.attack_auto, s.vm_provider), (None, true, None));
    let warned = crate::testing::logs::take_warnings();
    assert!(warned.iter().any(|w| w.contains("labs.docker_engine")) && warned.iter().any(|w| w.contains("attack_box.auto")), "{warned:?}");
}

#[test]
fn a_change_reaches_the_webview_in_the_shape_it_reads() {
    // The webview's `Change` is `{ setting, value }`, its settings camelCase (the generated types).
    let change: Change = serde_json::from_str(r#"{ "setting": "vmProvider", "value": "utm" }"#).unwrap();
    assert_eq!(change, Change::VmProvider(Some(Provider::Utm)));
    let change: Change = serde_json::from_str(r#"{ "setting": "lastRun", "value": { "lab": "web", "at": null } }"#).unwrap();
    assert_eq!(change, Change::LastRun { lab: "web".into(), at: None, place: None });
    let change: Change =
        serde_json::from_str(r#"{ "setting": "lastRun", "value": { "lab": "web", "at": 5, "place": { "kind": "vms", "provider": "qemu" } } }"#).unwrap();
    assert_eq!(change, Change::LastRun { lab: "web".into(), at: Some(5), place: Some(Placement::Vms { provider: Provider::Qemu }) });
    let json = serde_json::to_value(Settings::default()).unwrap();
    assert_eq!(json["attackAuto"], true);
    assert!(json["lastRuns"].as_object().is_some_and(serde_json::Map::is_empty));
}
