#[test]
fn a_wsl_check_is_logged_at_info_only_when_its_outcome_changes() {
    let a = format!("WSL check: test {} -> Ready", rand::random::<u32>());
    assert!(log_if_changed(&a));
    assert!(!log_if_changed(&a), "the same outcome again: debug only");
    assert!(log_if_changed("WSL check: test -> NeedsRestart"));
    assert!(log_if_changed(&a));
}

use std::sync::atomic::{AtomicBool, AtomicU32};

use super::*;
use crate::platform::exec::fake::{Tools, fail, ok};
use crate::testing::test_app::TestApp;
use WslState::*;

/// A machine where nothing could be read.
fn blank() -> Probes {
    Probes {
        status: Run::Failed,
        version: Run::Failed,
        lxss_service: None,
        wsl_service: None,
        vm_platform: None,
        wsl_feature: None,
        hypervisor: None,
        reboot_pending: None,
        renames_pending: None,
        installed_this_boot: false,
        restarted_for_wsl: false,
    }
}

/// Windows 11 without WSL: wsl.exe is there, and only offers to install.
fn win11_without_wsl() -> Probes {
    Probes {
        lxss_service: Some(false),
        wsl_service: Some(false),
        vm_platform: Some(Feature::Disabled),
        wsl_feature: Some(Feature::Disabled),
        hypervisor: Some(false),
        reboot_pending: Some(false),
        renames_pending: Some(false),
        ..blank()
    }
}

/// Windows 11 with the Store WSL and no distribution, after the restart.
fn win11_store_ready() -> Probes {
    Probes { status: Run::Ok, version: Run::Ok, wsl_service: Some(true), vm_platform: Some(Feature::Enabled), hypervisor: Some(true), ..win11_without_wsl() }
}

/// Windows 10 21H2 with the WSL feature (no Store WSL: `--version` is unknown to it).
fn win10_inbox_ready() -> Probes {
    Probes {
        status: Run::Ok,
        version: Run::Failed,
        lxss_service: Some(true),
        wsl_service: Some(false),
        vm_platform: Some(Feature::Enabled),
        wsl_feature: Some(Feature::Enabled),
        hypervisor: Some(true),
        reboot_pending: Some(false),
        renames_pending: Some(false),
        installed_this_boot: false,
        restarted_for_wsl: false,
    }
}

/// The Store WSL turned on, virtualization off in the firmware: the platform reads as on,
/// no hypervisor ever starts, and something re-adds file renames at every boot.
fn firmware_off() -> Probes {
    Probes { status: Run::Failed, hypervisor: Some(false), renames_pending: Some(true), ..win11_store_ready() }
}

#[test]
fn a_machine_without_wsl_is_missing() {
    assert_eq!(classify(&win11_without_wsl()), Missing);
    // Windows 10, whose wsl.exe may not exist at all.
    assert_eq!(classify(&Probes { status: Run::Missing, version: Run::Missing, ..win11_without_wsl() }), Missing);
    // The features couldn't be read: the services and wsl.exe still agree.
    assert_eq!(classify(&Probes { vm_platform: None, wsl_feature: None, hypervisor: None, ..win11_without_wsl() }), Missing);
    // A restart for an update, or renames, are no sign of WSL.
    assert_eq!(classify(&Probes { reboot_pending: Some(true), renames_pending: Some(true), ..win11_without_wsl() }), Missing);
}

#[test]
fn wsl_on_and_running_is_ready() {
    assert_eq!(classify(&win11_store_ready()), Ready);
    assert_eq!(classify(&win10_inbox_ready()), Ready);
    // Without a distribution `wsl --status` may fail: the platform running is enough.
    assert_eq!(classify(&Probes { status: Run::Failed, ..win11_store_ready() }), Ready);
    // A hung wsl.exe doesn't hide the rest.
    assert_eq!(classify(&Probes { status: Run::TimedOut, version: Run::TimedOut, ..win11_store_ready() }), Ready);
    // WMI unavailable: WSL answering, with nothing waiting for a restart, is enough.
    assert_eq!(classify(&Probes { vm_platform: None, wsl_feature: None, hypervisor: None, ..win11_store_ready() }), Ready);
    // An update waiting for a restart, while WSL answers.
    assert_eq!(classify(&Probes { reboot_pending: Some(true), renames_pending: Some(true), ..win11_store_ready() }), Ready);
    // Renames left by others never hold a working WSL back.
    assert_eq!(classify(&Probes { renames_pending: Some(true), ..win10_inbox_ready() }), Ready);
}

#[test]
fn a_pending_update_doesnt_hide_a_store_wsl_without_a_distribution() {
    // The usual state after `wsl --install --no-distribution` and its restart: `--status`
    // fails (no distribution), and Windows Update waits for a restart of its own.
    let store = Probes { status: Run::Failed, reboot_pending: Some(true), ..win11_store_ready() };
    assert_eq!(classify(&store), Ready);
    // Known by its service alone (`--version` timed out).
    assert_eq!(classify(&Probes { version: Run::TimedOut, ..store }), Ready);
    // The WSL feature (no Store WSL) gives no such proof: the restart may be its own.
    let inbox = Probes { status: Run::Failed, reboot_pending: Some(true), ..win10_inbox_ready() };
    assert_eq!(classify(&inbox), NeedsReboot);
}

#[test]
fn unreadable_servicing_is_no_proof_nothing_is_pending() {
    let unread = Probes { vm_platform: None, wsl_feature: None, hypervisor: None, reboot_pending: None, ..win11_store_ready() };
    assert_eq!(classify(&unread), Unknown);
    assert_eq!(classify(&Probes { reboot_pending: Some(false), ..unread }), Ready);
}

#[test]
fn turned_on_but_not_restarted_needs_a_reboot() {
    // Right after Cyber CTF's install: the Store WSL is in, the platform reads as on, no
    // hypervisor yet.
    let after_install = Probes {
        status: Run::Failed,
        version: Run::Ok,
        wsl_service: Some(true),
        vm_platform: Some(Feature::Enabled),
        hypervisor: Some(false),
        reboot_pending: Some(true),
        installed_this_boot: true,
        ..win11_without_wsl()
    };
    assert_eq!(classify(&after_install), NeedsReboot);
    // Hyper-V already ran a hypervisor, and WSL answers: still the restart first.
    assert_eq!(classify(&Probes { hypervisor: Some(true), status: Run::Ok, ..after_install }), NeedsReboot);
    // Servicing says nothing, but our install left renames.
    assert_eq!(classify(&Probes { hypervisor: Some(true), reboot_pending: Some(false), renames_pending: Some(true), ..after_install }), NeedsReboot);
    // The app was reopened before the restart, and WMI still reads the features as off.
    let reopened =
        Probes { vm_platform: Some(Feature::Disabled), wsl_feature: Some(Feature::Disabled), wsl_service: Some(false), version: Run::Failed, ..after_install };
    assert_eq!(classify(&reopened), NeedsReboot);
    // Our install ran this boot and the servicing state can't be read.
    assert_eq!(classify(&Probes { reboot_pending: None, ..reopened }), NeedsReboot);
    // Turned on by hand (`wsl --install` in PowerShell) on Windows 10: no mark of ours.
    let by_hand = Probes {
        status: Run::Failed,
        vm_platform: Some(Feature::Enabled),
        wsl_feature: Some(Feature::Enabled),
        lxss_service: Some(false),
        hypervisor: Some(false),
        reboot_pending: Some(true),
        ..win11_without_wsl()
    };
    assert_eq!(classify(&by_hand), NeedsReboot);
}

#[test]
fn our_install_that_changed_nothing_is_judged_on_the_rest() {
    // It ran this boot, yet no restart or rename is pending: WSL was ready already.
    assert_eq!(classify(&Probes { installed_this_boot: true, ..win11_store_ready() }), Ready);
}

#[test]
fn virtualization_off_in_the_firmware_never_loops_on_restarts() {
    // Renames re-added at every boot are no reason to restart (again).
    let off = firmware_off();
    assert_eq!(classify(&off), Unknown);
    assert!(virtualization_off(&off, classify(&off)));
    // After Cyber CTF's restart, even with servicing waiting for another one: no second ask.
    let after_ours = Probes { restarted_for_wsl: true, reboot_pending: Some(true), ..off };
    assert_eq!(classify(&after_ours), Unknown);
    assert!(virtualization_off(&after_ours, classify(&after_ours)));
    // The same pending restart without one of ours yet: that one is worth trying, once.
    let before_ours = Probes { reboot_pending: Some(true), ..off };
    assert_eq!(classify(&before_ours), NeedsReboot);
    assert!(!virtualization_off(&before_ours, NeedsReboot));
    // A new install of ours starts over.
    assert_eq!(classify(&Probes { installed_this_boot: true, ..after_ours }), NeedsReboot);
    // A working machine is never told virtualization is off.
    assert!(!virtualization_off(&win11_store_ready(), Ready));
}

#[test]
fn what_the_signals_cant_settle_is_unknown() {
    assert_eq!(classify(&blank()), Unknown);
    // Everything timed out.
    assert_eq!(classify(&Probes { status: Run::TimedOut, version: Run::TimedOut, ..blank() }), Unknown);
    // A hung wsl.exe with no other sign of WSL: maybe it's there and stuck.
    assert_eq!(classify(&Probes { status: Run::TimedOut, ..win11_without_wsl() }), Unknown);
    // On but no hypervisor and nothing pending: perhaps virtualization is off in the firmware.
    let no_hypervisor = Probes { hypervisor: Some(false), ..win10_inbox_ready() };
    assert_eq!(classify(&no_hypervisor), Unknown);
    assert!(virtualization_off(&no_hypervisor, Unknown));
    // Only the platform on, nothing of WSL: neither ready nor plainly missing.
    assert_eq!(classify(&Probes { vm_platform: Some(Feature::Enabled), ..win11_without_wsl() }), Unknown);
}

#[test]
fn wsl_1_without_the_platform_is_missing_wsl_2() {
    let wsl1 = Probes { vm_platform: Some(Feature::Disabled), hypervisor: Some(false), ..win10_inbox_ready() };
    assert_eq!(classify(&wsl1), Missing);
    // Unless the platform is being turned on.
    assert_eq!(classify(&Probes { reboot_pending: Some(true), ..wsl1 }), NeedsReboot);
}

#[test]
fn the_wmi_output_is_read_by_key() {
    let out = "features=ok\r\nfeature:VirtualMachinePlatform=1\r\nfeature:Microsoft-Windows-Subsystem-Linux=2\r\nhypervisor=True\r\n";
    assert_eq!(parse_cim(out), (Some(Feature::Enabled), Some(Feature::Disabled), Some(true)));
    // An edition without the feature doesn't list it.
    assert_eq!(parse_cim("features=ok\nfeature:VirtualMachinePlatform=3\nhypervisor=False\n"), (Some(Feature::Absent), Some(Feature::Absent), Some(false)));
    // The feature query failed; the hypervisor one didn't.
    assert_eq!(parse_cim("hypervisor=False\n"), (None, None, Some(false)));
    // InstallState 4 (unknown), and nothing at all.
    assert_eq!(parse_cim("features=ok\nfeature:VirtualMachinePlatform=4\nfeature:Microsoft-Windows-Subsystem-Linux=1"), (None, Some(Feature::Enabled), None));
    assert_eq!(parse_cim(""), (None, None, None));
}

#[test]
fn a_probe_reads_as_ok_failed_timed_out_or_missing() {
    let failed = |stderr: &str| Err(Error::CommandFailed { command: "wsl --status".into(), stderr: stderr.into() });
    assert_eq!(outcome(&Ok("features=ok".into())), (Run::Ok, "features=ok"));
    assert_eq!(outcome(&failed("")).0, Run::Failed);
    assert_eq!(outcome(&failed("timed out after 10s (the tool or its backend may be stuck)")).0, Run::TimedOut);
    assert_eq!(outcome(&Err(Error::ToolMissing { tool: "wsl" })).0, Run::Missing);
}

#[test]
fn the_state_reaches_the_ui_in_camel_case() {
    let report = WslReport { state: NeedsReboot, reopens_after_restart: true, virtualization_off: false };
    assert_eq!(serde_json::to_value(report).unwrap(), serde_json::json!({ "state": "needsReboot", "reopensAfterRestart": true, "virtualizationOff": false }));
}

#[test]
fn an_install_mark_counts_for_the_boot_it_was_made_in_only() {
    let t = TestApp::new();
    assert!(!installed_this_boot(&t.dirs(), 1_000));
    mark_installed(&t.dirs());
    let boot = sysinfo::System::boot_time();
    assert!(installed_this_boot(&t.dirs(), boot));
    assert!(installed_this_boot(&t.dirs(), boot + 2), "the boot time jitters");
    // After a restart the mark is gone for good.
    assert!(!installed_this_boot(&t.dirs(), boot + 600));
    assert!(!t.data().join(INSTALL_MARK).exists());
    assert!(!installed_this_boot(&t.dirs(), boot));
    std::fs::write(t.data().join(INSTALL_MARK), "garbage").unwrap();
    assert!(!installed_this_boot(&t.dirs(), boot));
    assert!(same_boot(10, 40) && !same_boot(10, 41));
}

#[test]
fn hyper_v_turned_on_asks_for_a_restart_in_that_boot_only() {
    let t = TestApp::new();
    assert!(!hyperv_on_this_boot(&t.dirs()));
    mark_hyperv_on(&t.dirs());
    assert!(hyperv_on_this_boot(&t.dirs()));
    assert!(!installed_this_boot(&t.dirs(), sysinfo::System::boot_time()), "WSL's mark is its own");
    // After a restart it is gone for good.
    mark(&t.dirs(), HYPERV_MARK, sysinfo::System::boot_time() - 600);
    assert!(!hyperv_on_this_boot(&t.dirs()));
    assert!(!t.data().join(HYPERV_MARK).exists());
}

#[test]
fn a_restart_mark_counts_from_the_next_boot_until_wsl_is_ready_or_installed_again() {
    let t = TestApp::new();
    mark(&t.dirs(), RESTART_MARK, 1_000);
    // Still counting down in the same boot: not restarted yet.
    assert!(!restarted_for_wsl(&t.dirs(), 1_000));
    assert!(restarted_for_wsl(&t.dirs(), 2_000));
    // Not ready: remembered.
    assert_eq!(assess(&t.dirs(), &Probes { restarted_for_wsl: true, ..firmware_off() }), (Unknown, true));
    assert!(restarted_for_wsl(&t.dirs(), 2_000));
    // Ready: forgotten.
    assert_eq!(assess(&t.dirs(), &win11_store_ready()), (Ready, false));
    assert!(!restarted_for_wsl(&t.dirs(), 2_000));
    // A new install forgets it too.
    mark(&t.dirs(), RESTART_MARK, 1_000);
    mark_installed(&t.dirs());
    assert!(!restarted_for_wsl(&t.dirs(), 2_000));
}

#[test]
fn a_mark_that_cant_be_written_or_removed_is_logged() {
    // Regression: both failed silently, and the restart WSL needed was asked for again, or
    // never.
    let t = TestApp::new();
    crate::testing::logs::take_warnings();
    std::fs::create_dir_all(t.data().join(RESTART_MARK).join("x")).unwrap();
    mark(&t.dirs(), RESTART_MARK, 1_000);
    unmark(&t.dirs(), RESTART_MARK);
    let logged = crate::testing::logs::take_warnings();
    assert_eq!(logged.len(), 2, "{logged:?}");
    assert!(logged[0].starts_with("couldn't keep the WSL mark wsl-restart-boot: "), "{logged:?}");
    assert!(logged[1].starts_with("couldn't remove the WSL mark "), "{logged:?}");
    // Nothing there: nothing to say.
    unmark(&t.dirs(), INSTALL_MARK);
    assert_eq!(crate::testing::logs::take_warnings(), [] as [String; 0]);
}

#[test]
fn the_restart_note_is_one_short_line() {
    assert_eq!(reason("").unwrap(), DEFAULT_REASON);
    assert_eq!(reason("  Cyber CTF: finishing WSL 2  ").unwrap(), "Cyber CTF: finishing WSL 2");
    assert!(reason("two\nlines").is_err());
    assert!(reason(&"a".repeat(201)).is_err());
    assert_eq!(reason(&"é".repeat(200)).unwrap().chars().count(), 200);
}

#[test]
fn the_app_reopens_by_its_path_or_its_store_package() {
    assert_eq!(reopen_line(r"C:\Program Files\Cyber CTF\cyberctf-desktop.exe").as_deref(), Some(r#""C:\Program Files\Cyber CTF\cyberctf-desktop.exe""#));
    assert_eq!(
        reopen_line(r"C:\Program Files\WindowsApps\F4K.CyberCTF_1.4.1.0_x64__8wekyb3d8bbwe\CyberCTF.exe").as_deref(),
        Some(r"explorer.exe shell:AppsFolder\F4K.CyberCTF_8wekyb3d8bbwe!CyberCTF")
    );
    // Anything it can't read as a package, or put in one command line, isn't registered.
    assert_eq!(reopen_line(r"C:\Program Files\WindowsApps\Odd\CyberCTF.exe"), None);
    assert_eq!(reopen_line(r"C:\Program Files\WindowsApps\A&B_1_x64__p\CyberCTF.exe"), None);
    assert_eq!(reopen_line(r#"C:\a"b\app.exe"#), None);
    assert_eq!(reopen_line(""), None);
}

/// A `RunOnce` stand-in: whether it was written, and whether writing works.
#[derive(Default)]
struct FakeReopen {
    refuses: bool,
    written: AtomicBool,
    writes: AtomicU32,
}

impl FakeReopen {
    fn written(&self) -> bool {
        self.written.load(Ordering::SeqCst)
    }
}

impl Reopen for FakeReopen {
    fn register(&self) -> bool {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.written.store(!self.refuses, Ordering::SeqCst);
        !self.refuses
    }
    fn unregister(&self) {
        self.written.store(false, Ordering::SeqCst);
    }
}

const SHUTDOWN: &str = "shutdown /r /t 5 /c Cyber CTF: finishing WSL 2";

#[tokio::test]
async fn a_restart_is_scheduled_once_whoever_asks() {
    let t = TestApp::new();
    let tools = Tools::strict_rules(&[("shutdown", ok(""))]);
    let (state, reopen) = (AtomicU8::new(IDLE), FakeReopen::default());
    schedule_restart(&t.dirs(), &state, "Cyber CTF: finishing WSL 2", &reopen, WindowsFeature::Wsl).await.unwrap();
    assert_eq!(tools.calls(), [SHUTDOWN]);
    assert!(reopen.written());
    // This boot's, as the check reads it (`same_boot`): Windows works the boot time out from
    // the uptime, a second apart from one call to the next now and then.
    let marked = marked_boot(&t.dirs(), RESTART_MARK);
    assert!(marked.is_some_and(|m| same_boot(m, sysinfo::System::boot_time())), "{marked:?}");
    // The other window, or the remounted step, asks again during the countdown: the same
    // restart, no second shutdown, RunOnce left as it is.
    schedule_restart(&t.dirs(), &state, "Cyber CTF: finishing WSL 2", &reopen, WindowsFeature::Wsl).await.unwrap();
    assert_eq!(tools.calls().len(), 1);
    assert_eq!(reopen.writes.load(Ordering::SeqCst), 1);
    assert!(reopen.written());
}

#[tokio::test]
async fn a_restart_for_hyper_v_leaves_wsls_restart_mark_alone() {
    let t = TestApp::new();
    let tools = Tools::strict_rules(&[("shutdown", ok(""))]);
    let (state, reopen) = (AtomicU8::new(IDLE), FakeReopen::default());
    schedule_restart(&t.dirs(), &state, "Cyber CTF: finishing WSL 2", &reopen, WindowsFeature::Hyperv).await.unwrap();
    assert_eq!(tools.calls(), [SHUTDOWN]);
    assert!(reopen.written());
    // WSL never reads it as a restart made for it.
    assert_eq!(marked_boot(&t.dirs(), RESTART_MARK), None);
}

#[test]
fn the_restart_can_run_as_a_tauri_command() {
    // Tauri runs a command's future on its thread pool: it must be `Send` on every OS, though
    // only Windows awaits it from the command.
    fn send<T: Send>(_: &T) {}
    let t = TestApp::new();
    let (state, reopen) = (AtomicU8::new(IDLE), FakeReopen::default());
    send(&schedule_restart(&t.dirs(), &state, "x", &reopen, WindowsFeature::Wsl));
    send(&machine_restart(t.dirs(), String::new(), WindowsFeature::Wsl));
}

#[tokio::test]
async fn a_restart_already_scheduled_by_windows_counts_as_this_one() {
    let t = TestApp::new();
    let _tools = Tools::strict_rules(&[("shutdown", fail("A system shutdown has already been scheduled.(1190)"))]);
    let (state, reopen) = (AtomicU8::new(IDLE), FakeReopen::default());
    schedule_restart(&t.dirs(), &state, "x", &reopen, WindowsFeature::Wsl).await.unwrap();
    assert!(reopen.written(), "the app still reopens after that restart");
    assert_eq!(state.load(Ordering::SeqCst), SCHEDULED);
    assert!(marked_boot(&t.dirs(), RESTART_MARK).is_some());
}

#[tokio::test]
async fn a_failed_restart_takes_back_what_it_wrote_and_can_be_tried_again() {
    let t = TestApp::new();
    let tools = Tools::strict_rules(&[("shutdown", fail("Access is denied.(5)"))]);
    let (state, reopen) = (AtomicU8::new(IDLE), FakeReopen::default());
    let err = schedule_restart(&t.dirs(), &state, "Cyber CTF: finishing WSL 2", &reopen, WindowsFeature::Wsl).await.unwrap_err();
    assert!(err.to_string().contains("Access is denied"), "{err}");
    assert!(!reopen.written(), "RunOnce removed");
    assert!(marked_boot(&t.dirs(), RESTART_MARK).is_none());
    drop(tools);
    let tools = Tools::strict_rules(&[("shutdown", ok(""))]);
    schedule_restart(&t.dirs(), &state, "Cyber CTF: finishing WSL 2", &reopen, WindowsFeature::Wsl).await.unwrap();
    assert_eq!(tools.calls(), [SHUTDOWN]);
    assert!(reopen.written());
}

#[tokio::test]
async fn a_failed_restart_leaves_a_runonce_it_didnt_write() {
    // Writing RunOnce failed: nothing of ours to remove, and the unregister isn't called.
    struct Untouchable;
    impl Reopen for Untouchable {
        fn register(&self) -> bool {
            false
        }
        fn unregister(&self) {
            panic!("removed a RunOnce entry this call didn't write");
        }
    }
    let t = TestApp::new();
    let _tools = Tools::strict_rules(&[("shutdown", fail("Access is denied.(5)"))]);
    assert!(schedule_restart(&t.dirs(), &AtomicU8::new(IDLE), "x", &Untouchable, WindowsFeature::Wsl).await.is_err());
    let refused = FakeReopen { refuses: true, ..FakeReopen::default() };
    let _tools = Tools::strict_rules(&[("shutdown", ok(""))]);
    schedule_restart(&t.dirs(), &AtomicU8::new(IDLE), "x", &refused, WindowsFeature::Wsl).await.unwrap();
    assert!(!refused.written());
}

#[cfg(not(windows))]
#[tokio::test]
async fn restarting_is_refused_off_windows_and_the_check_has_no_wsl() {
    let t = TestApp::new();
    let tools = Tools::strict_rules(&[]);
    let err = machine_restart(t.dirs(), String::new(), WindowsFeature::Wsl).await.unwrap_err();
    assert_eq!(err.to_string(), "Restarting from Cyber CTF is only available on Windows.");
    assert_eq!(tools.calls(), [] as [&str; 0]);
    assert!(report(&t.dirs()).await.is_none());
}

#[tokio::test]
async fn a_bad_restart_note_is_refused_whatever_the_os() {
    let t = TestApp::new();
    let err = machine_restart(t.dirs(), "a\nb".into(), WindowsFeature::Wsl).await.unwrap_err();
    assert_eq!(err.to_string(), "The restart note must be one line of at most 200 characters.");
}

#[cfg(windows)]
#[tokio::test]
async fn the_restart_command_runs_shutdown_with_the_note() {
    let t = TestApp::new();
    let tools = Tools::strict_rules(&[("shutdown", ok(""))]);
    machine_restart(t.dirs(), "Cyber CTF: finishing WSL 2".into(), WindowsFeature::Wsl).await.unwrap();
    assert_eq!(tools.calls(), [SHUTDOWN]);
    // The process-wide state: later asks in this test process are the same restart.
    RESTART.store(IDLE, Ordering::SeqCst);
}

#[cfg(windows)]
#[tokio::test]
async fn a_test_never_restarts_for_real() {
    let t = TestApp::new();
    assert!(machine_restart(t.dirs(), String::new(), WindowsFeature::Wsl).await.is_err());
}

/// The real probes on this Windows: they finish in time, read the registry, and give a state.
#[cfg(windows)]
#[tokio::test]
async fn the_probes_answer_on_this_machine() {
    let started = std::time::Instant::now();
    let probes = windows::probe(false, false).await;
    println!("{probes:?} -> {:?}", classify(&probes));
    assert!(started.elapsed() < std::time::Duration::from_secs(45));
    assert!(probes.reboot_pending.is_some() && probes.lxss_service.is_some() && probes.wsl_service.is_some(), "{probes:?}");
    let t = TestApp::new();
    assert!(report(&t.dirs()).await.is_some());
}

/// Removes the test's `RunOnce` value however the test ends.
#[cfg(windows)]
struct RunOnceValue(String);

#[cfg(windows)]
impl Drop for RunOnceValue {
    fn drop(&mut self) {
        windows::delete_run_once(&self.0);
    }
}

#[cfg(windows)]
#[test]
fn run_once_is_written_and_removed() {
    // A name of its own: the app's `CyberCTF` value is never touched by a test.
    let value = RunOnceValue(format!("CyberCTF-unit-test-{}", rand::random::<u32>()));
    assert!(windows::set_run_once(&value.0, r#""C:\nowhere\app.exe""#));
    assert_eq!(windows::run_once_exists(&value.0), Some(true));
    windows::delete_run_once(&value.0);
    assert_eq!(windows::run_once_exists(&value.0), Some(false));
    assert!(windows::reopen_command().is_some());
    // Nothing to write: nothing written.
    assert!(!Reopen::register(&windows::UserRunOnce(None)));
}
