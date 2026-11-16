//! The settings: their defaults, each change, what can't be saved, and the webview's import from
//! the storage shapes earlier versions wrote.

use std::collections::BTreeMap;

use super::{Change, PortMode, Settings, TestRecord, TestResult, VmForm, settings_change, settings_get, settings_import_webview};
use crate::machine::system::docker::Engine;
use crate::runtime::providers::Provider;
use crate::testing::test_app::TestApp;

fn record(result: TestResult, at: u64, why: Option<&str>) -> TestRecord {
    TestRecord { result, at, why: why.map(str::to_owned) }
}

fn items(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
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
fn the_tests_and_last_runs_are_kept_by_key_and_a_last_run_is_forgotten() {
    let t = TestApp::new();
    let dirs = t.dirs();
    let failed = record(TestResult::Fail, 2, Some("no KVM"));
    settings_change(
        &dirs,
        &[
            Change::SelfTest { key: "vm.utm".into(), record: record(TestResult::Ok, 1, None) },
            Change::SelfTest { key: "vm".into(), record: failed.clone() },
            Change::ServerTest { host: "pve1".into(), record: record(TestResult::Ok, 3, None) },
            Change::LastRun { lab: "web".into(), at: Some(10) },
            Change::LastRun { lab: "kube".into(), at: Some(20) },
        ],
    )
    .unwrap();
    let s = settings_change(&dirs, &[Change::LastRun { lab: "web".into(), at: None }, Change::LastRun { lab: "kube".into(), at: Some(30) }]).unwrap();
    assert_eq!(s.self_tests.get("vm"), Some(&failed));
    assert_eq!(s.self_tests.get("vm.utm").map(|r| r.result), Some(TestResult::Ok));
    assert_eq!(s.server_tests.get("pve1").map(|r| r.at), Some(3));
    assert_eq!(s.last_runs, BTreeMap::from([("kube".to_owned(), 30)]), "web forgotten, kube's later start kept");
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

/// The webview's storage as the last version before the store left it: every setting, in the
/// shapes `src/lib/settings.ts`, `last-run.ts` and `vm-tests.ts` wrote, beside the preferences the
/// webview keeps and keys of no setting.
const WEBVIEW_1_9: &[(&str, &str)] = &[
    ("cyberctf.appearance", "light"),
    ("cyberctf.locale", "fr"),
    ("cyberctf.onboarded", "1"),
    ("cyberctf.diagram.fitWidth", "true"),
    ("cyberctf.attackbox.image", "parrotsec/security"),
    ("cyberctf.attackbox.auto", "false"),
    ("cyberctf.attackbox.desktop", "true"),
    ("cyberctf.vm.provider", "vmware_desktop"),
    ("cyberctf.docker.engine", "colima"),
    ("cyberctf.labs.ports", "default"),
    ("cyberctf.labs.vmForm", "vms"),
    ("cyberctf.selftest.docker", r#"{"result":"ok","at":1763300000000,"why":null}"#),
    ("cyberctf.selftest.vm", r#"{"result":"fail","at":1763300001000,"why":"VT-x is off"}"#),
    ("cyberctf.selftest.vm.vmware_desktop", r#"{"result":"fail","at":1763300001000,"why":"VT-x is off"}"#),
    ("cyberctf.server.vmtest", r#"{"pve1":{"result":"ok","at":1763300002000},"esx":{"result":"maybe"}}"#),
    ("cyberctf.lastrun.kube-dashboard-exposed", "1763300003000"),
    ("cyberctf.lastrun.local-goad", "1763300004000"),
    ("__wdio_attack__", "web"),
];

#[test]
fn the_webviews_settings_from_1_9_are_taken_whole_and_its_own_preferences_left() {
    let t = TestApp::new();
    let dirs = t.dirs();
    let imported = settings_import_webview(&dirs, &items(WEBVIEW_1_9)).unwrap();
    let left: Vec<&str> = WEBVIEW_1_9.iter().map(|(k, _)| *k).filter(|k| !imported.taken.iter().any(|t| t == k)).collect();
    assert_eq!(left, ["cyberctf.appearance", "cyberctf.locale", "cyberctf.onboarded", "cyberctf.diagram.fitWidth", "__wdio_attack__"]);
    let s = settings_get(&dirs).unwrap();
    assert_eq!(imported.settings, s);
    assert_eq!(s.attack_image.as_deref(), Some("parrotsec/security"));
    assert!(!s.attack_auto && s.attack_desktop);
    assert_eq!(
        (s.vm_provider, s.docker_engine, s.port_mode, s.vm_form),
        (Some(Provider::VmwareDesktop), Some(Engine::Colima), Some(PortMode::Default), Some(VmForm::Vms))
    );
    assert_eq!(s.self_tests.get("docker"), Some(&record(TestResult::Ok, 1_763_300_000_000, None)));
    assert_eq!(s.self_tests.get("vm.vmware_desktop"), Some(&record(TestResult::Fail, 1_763_300_001_000, Some("VT-x is off"))));
    assert_eq!(s.server_tests.keys().collect::<Vec<_>>(), ["pve1"], "the entry that wasn't a test is left out, as the webview left it out");
    assert_eq!(s.last_runs.get("kube-dashboard-exposed"), Some(&1_763_300_003_000));
    assert_eq!(s.last_runs.get("local-goad"), Some(&1_763_300_004_000));
}

#[test]
fn values_the_webview_read_as_unset_are_taken_and_leave_the_defaults() {
    // Each read as "none" in the webview (`readStoredOneOf`, a record of another shape): taken so
    // the webview drops them, the setting at its default, as the player saw it.
    let t = TestApp::new();
    let dirs = t.dirs();
    let odd = items(&[
        ("cyberctf.attackbox.image", "cyberctf/attack-box:latest"),
        ("cyberctf.attackbox.auto", "true"),
        ("cyberctf.vm.provider", "proxmox"),
        ("cyberctf.docker.engine", "lima"),
        ("cyberctf.labs.ports", "ask"),
        ("cyberctf.labs.vmForm", "vm"),
        ("cyberctf.selftest.docker", "not json"),
        ("cyberctf.server.vmtest", "[]"),
        ("cyberctf.lastrun.a", ""),
        ("cyberctf.lastrun.x", "garbage"),
    ]);
    let imported = settings_import_webview(&dirs, &odd).unwrap();
    assert_eq!(imported.taken.len(), odd.len(), "{:?}", imported.taken);
    assert_eq!(imported.settings, Settings::default());
}

#[test]
fn importing_again_gives_the_same_settings_and_the_webviews_values_win() {
    // The webview takes its keys out only once the import answered: a window that closed between
    // the two hands the same items over at its next start.
    let t = TestApp::new();
    let dirs = t.dirs();
    settings_change(&dirs, &[Change::PortMode(Some(PortMode::Random)), Change::LastRun { lab: "web".into(), at: Some(5) }]).unwrap();
    let first = settings_import_webview(&dirs, &items(WEBVIEW_1_9)).unwrap();
    let again = settings_import_webview(&dirs, &items(WEBVIEW_1_9)).unwrap();
    assert_eq!(first, again);
    assert_eq!(again.settings.port_mode, Some(PortMode::Default), "the webview's value, which the player saw last");
    assert_eq!(again.settings.last_runs.get("web"), Some(&5), "a lab the webview didn't name keeps its time");
}

#[test]
fn a_webview_with_nothing_to_hand_over_changes_nothing() {
    let t = TestApp::new();
    let dirs = t.dirs();
    let imported = settings_import_webview(&dirs, &items(&[("cyberctf.appearance", "dark"), ("cyberctf.locale", "en")])).unwrap();
    assert_eq!(imported.taken, Vec::<String>::new());
    assert_eq!(imported.settings, Settings::default());
}

#[test]
fn a_change_reaches_the_webview_in_the_shape_it_reads() {
    // The webview's `Change` is `{ setting, value }`, its settings camelCase (the generated types).
    let change: Change = serde_json::from_str(r#"{ "setting": "vmProvider", "value": "utm" }"#).unwrap();
    assert_eq!(change, Change::VmProvider(Some(Provider::Utm)));
    let change: Change = serde_json::from_str(r#"{ "setting": "lastRun", "value": { "lab": "web", "at": null } }"#).unwrap();
    assert_eq!(change, Change::LastRun { lab: "web".into(), at: None });
    let json = serde_json::to_value(Settings::default()).unwrap();
    assert_eq!(json["attackAuto"], true);
    assert!(json["lastRuns"].as_object().is_some_and(serde_json::Map::is_empty));
}
