#[test]
fn a_wsl_check_is_logged_at_info_only_when_its_outcome_changes() {
    let a = format!("WSL check: test {} -> Ready", rand::random::<u32>());
    assert!(log_if_changed(&a));
    assert!(!log_if_changed(&a), "the same outcome again: debug only");
    assert!(log_if_changed("WSL check: test -> NeedsRestart"));
    assert!(log_if_changed(&a));
}

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32};

use super::*;
use isothrum_runner::fake::{FakeProcesses, Reply};

use crate::runtime::providers::testing::{Machine, host};
use crate::testing::test_app::TestApp;
use WslState::*;

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
    remember(&t.dirs(), Unknown);
    assert!(restarted_for_wsl(&t.dirs(), 2_000));
    // Ready: forgotten.
    remember(&t.dirs(), Ready);
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

/// isothrum-machine's tools on this test's thread, until the guard drops, each answered `reply`:
/// the guard, and the tools to read what was run.
fn shutdown(reply: Reply) -> (Machine, Arc<FakeProcesses>) {
    let tools = Arc::new(FakeProcesses::model(move |_| reply.clone()));
    (Machine::set(Arc::<FakeProcesses>::clone(&tools), host(std::path::Path::new("/nonexistent/cyberctf-test-machine"))), tools)
}

/// What `tools` ran, each as its command line.
fn lines(tools: &FakeProcesses) -> Vec<String> {
    tools.calls().iter().map(|c| std::iter::once(c.program.as_str()).chain(c.args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")).collect()
}

#[tokio::test]
async fn a_restart_is_scheduled_once_whoever_asks() {
    let t = TestApp::new();
    let (_machine, tools) = shutdown(Reply::ok(""));
    let (state, reopen) = (AtomicU8::new(IDLE), FakeReopen::default());
    schedule_restart(&t.dirs(), &state, "Cyber CTF: finishing WSL 2", &reopen, WindowsFeature::Wsl).await.unwrap();
    assert_eq!(lines(&tools), [SHUTDOWN]);
    assert!(reopen.written());
    // This boot's, as the check reads it (`same_boot`): Windows works the boot time out from
    // the uptime, a second apart from one call to the next now and then.
    let marked = marked_boot(&t.dirs(), RESTART_MARK);
    assert!(marked.is_some_and(|m| same_boot(m, sysinfo::System::boot_time())), "{marked:?}");
    // The other window, or the remounted step, asks again during the countdown: the same
    // restart, no second shutdown, RunOnce left as it is.
    schedule_restart(&t.dirs(), &state, "Cyber CTF: finishing WSL 2", &reopen, WindowsFeature::Wsl).await.unwrap();
    assert_eq!(lines(&tools).len(), 1);
    assert_eq!(reopen.writes.load(Ordering::SeqCst), 1);
    assert!(reopen.written());
}

#[tokio::test]
async fn a_restart_for_hyper_v_leaves_wsls_restart_mark_alone() {
    let t = TestApp::new();
    let (_machine, tools) = shutdown(Reply::ok(""));
    let (state, reopen) = (AtomicU8::new(IDLE), FakeReopen::default());
    schedule_restart(&t.dirs(), &state, "Cyber CTF: finishing WSL 2", &reopen, WindowsFeature::Hyperv).await.unwrap();
    assert_eq!(lines(&tools), [SHUTDOWN]);
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
    let (_machine, _tools) = shutdown(Reply::fail(1, "A system shutdown has already been scheduled.(1190)"));
    let (state, reopen) = (AtomicU8::new(IDLE), FakeReopen::default());
    schedule_restart(&t.dirs(), &state, "x", &reopen, WindowsFeature::Wsl).await.unwrap();
    assert!(reopen.written(), "the app still reopens after that restart");
    assert_eq!(state.load(Ordering::SeqCst), SCHEDULED);
    assert!(marked_boot(&t.dirs(), RESTART_MARK).is_some());
}

#[tokio::test]
async fn a_failed_restart_takes_back_what_it_wrote_and_can_be_tried_again() {
    let t = TestApp::new();
    let (_machine, tools) = shutdown(Reply::fail(1, "Access is denied.(5)"));
    let (state, reopen) = (AtomicU8::new(IDLE), FakeReopen::default());
    let err = schedule_restart(&t.dirs(), &state, "Cyber CTF: finishing WSL 2", &reopen, WindowsFeature::Wsl).await.unwrap_err();
    assert!(err.to_string().contains("Access is denied"), "{err}");
    assert!(!reopen.written(), "RunOnce removed");
    assert!(marked_boot(&t.dirs(), RESTART_MARK).is_none());
    drop(tools);
    let (_machine, tools) = shutdown(Reply::ok(""));
    schedule_restart(&t.dirs(), &state, "Cyber CTF: finishing WSL 2", &reopen, WindowsFeature::Wsl).await.unwrap();
    assert_eq!(lines(&tools), [SHUTDOWN]);
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
    let (_machine, _tools) = shutdown(Reply::fail(1, "Access is denied.(5)"));
    assert!(schedule_restart(&t.dirs(), &AtomicU8::new(IDLE), "x", &Untouchable, WindowsFeature::Wsl).await.is_err());
    let refused = FakeReopen { refuses: true, ..FakeReopen::default() };
    let (_machine, _tools) = shutdown(Reply::ok(""));
    schedule_restart(&t.dirs(), &AtomicU8::new(IDLE), "x", &refused, WindowsFeature::Wsl).await.unwrap();
    assert!(!refused.written());
}

#[cfg(not(windows))]
#[tokio::test]
async fn restarting_is_refused_off_windows() {
    let t = TestApp::new();
    let (_machine, tools) = shutdown(Reply::ok(""));
    let err = machine_restart(t.dirs(), String::new(), WindowsFeature::Wsl).await.unwrap_err();
    assert_eq!(err.to_string(), "Restarting from Cyber CTF is only available on Windows.");
    assert_eq!(lines(&tools), [] as [String; 0]);
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
    let (_machine, tools) = shutdown(Reply::ok(""));
    machine_restart(t.dirs(), "Cyber CTF: finishing WSL 2".into(), WindowsFeature::Wsl).await.unwrap();
    assert_eq!(lines(&tools), [SHUTDOWN]);
    // The process-wide state: later asks in this test process are the same restart.
    RESTART.store(IDLE, Ordering::SeqCst);
}

#[cfg(windows)]
#[tokio::test]
async fn a_test_never_restarts_for_real() {
    let t = TestApp::new();
    assert!(machine_restart(t.dirs(), String::new(), WindowsFeature::Wsl).await.is_err());
}

/// isothrum-machine's real probes on this Windows: they finish in time, read the registry, and
/// give a state.
#[cfg(windows)]
#[tokio::test]
async fn the_probes_answer_on_this_machine() {
    let started = std::time::Instant::now();
    let cx = isothrum_machine::Context::new(
        std::sync::Arc::new(isothrum_runner::SystemProcesses::default()),
        crate::runtime::providers::testing::host(std::path::Path::new("/")),
    );
    let found = isothrum_machine::wsl::check(&cx, Marks::default()).await.unwrap();
    println!("{:?} -> {:?}", found.probes, found.state);
    assert!(started.elapsed() < std::time::Duration::from_secs(45));
    let p = found.probes;
    assert!(p.reboot_pending.is_some() && p.lxss_service.is_some() && p.wsl_service.is_some(), "{p:?}");
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
