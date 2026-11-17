//! The run service: the commands ask it for operations and the webview gets each one's start, log
//! and end as events, from the launcher's own service and from any other behind the trait.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::Manager;

use super::{EventLog, OperationId, Request, RunEvent, RunService};
use crate::error::{Result, Wire};
use crate::runtime::{Operation, Runtime};
use crate::testing::test_app::TestApp;

/// Waits for operation `id`'s end among `app`'s run events: `Ok` when it went through, its error
/// when it failed. Every log line it sent, in order, beside.
pub(crate) async fn finished(app: &crate::AppHandle, id: &OperationId) -> (std::result::Result<(), Wire>, Vec<String>) {
    let events = crate::app::runs::of(app);
    for _ in 0..3000 {
        let all = events.events().since(None);
        let lines = all
            .iter()
            .filter_map(|e| match &e.event {
                RunEvent::Log { operation, line, .. } if operation == id => Some(line.clone()),
                _ => None,
            })
            .collect();
        if let Some(error) = all.iter().find_map(|e| match &e.event {
            RunEvent::Ended { operation, error, .. } if operation == id => Some(error.clone()),
            _ => None,
        }) {
            return (error.map_or(Ok(()), Err), lines);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("operation {id:?} never ended");
}

/// A run service that runs nothing: it takes each request, and the test tells its events (the
/// runner's service, as the launcher sees it through the trait).
/// One `watch` the webview asked: the labs shown (each with its runtime) and the pages open.
type Watched = (Vec<(String, Runtime)>, Vec<String>);

#[derive(Default)]
struct Fake {
    events: EventLog,
    taken: Mutex<Vec<(OperationId, String, Operation)>>,
    watched: Mutex<Vec<Watched>>,
}

impl RunService for Fake {
    fn start(&self, request: Request) -> Result<OperationId> {
        let id = OperationId(format!("fake-{}", self.taken.lock().unwrap().len() + 1));
        self.taken.lock().unwrap().push((id.clone(), request.lab_id().to_owned(), request.operation()));
        Ok(id)
    }

    fn events(&self) -> &EventLog {
        &self.events
    }

    fn follow_runner(&self) -> super::BoxFuture<'static, ()> {
        Box::pin(std::future::pending())
    }

    fn watch(&self, labs: Vec<(String, Runtime)>, open: Vec<String>) {
        self.watched.lock().unwrap().push((labs, open));
    }

    fn reconcile_once(&self) -> super::BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

/// `t`'s app with `fake` as its run service.
fn serving(t: &TestApp, fake: Arc<Fake>) {
    let service: Arc<dyn RunService> = fake;
    assert!(t.handle().manage(service), "no service made before");
}

#[tokio::test]
async fn an_operation_runs_end_to_end_through_a_fake_service_and_a_reloaded_window_replays_it() {
    let t = TestApp::new();
    t.lab("web", &[]);
    let fake = Arc::new(Fake::default());
    serving(&t, Arc::clone(&fake));
    let id = crate::commands::labs::lab_resume(t.handle().clone(), "web".into(), Runtime::Docker).await.unwrap();
    assert_eq!(*fake.taken.lock().unwrap(), [(id.clone(), "web".to_owned(), Operation::Resume)]);
    // The service tells its progress; the webview hears it as run events.
    let started = RunEvent::Started {
        operation: id.clone(),
        lab_id: "web".into(),
        op: Operation::Resume,
        machine: None,
        runtime: Some(Runtime::Docker),
        deploying: true,
        at: 1,
    };
    fake.events.emit(started);
    fake.events.emit(RunEvent::Log { operation: id.clone(), lab_id: "web".into(), line: "Get images".into() });
    let seen = crate::commands::labs::run_events(t.handle().clone(), None).await;
    assert_eq!(seen.len(), 2);
    fake.events.emit(RunEvent::Ended { operation: id.clone(), lab_id: "web".into(), op: Operation::Resume, error: None });
    let (outcome, lines) = finished(t.handle(), &id).await;
    assert_eq!((outcome, lines), (Ok(()), vec!["Get images".to_owned()]));
    // A reloaded window asks for what came after the last event it saw.
    let missed = crate::commands::labs::run_events(t.handle().clone(), Some(seen[1].cursor)).await;
    assert_eq!(missed.len(), 1);
    assert!(matches!(missed[0].event, RunEvent::Ended { .. }));
}

#[tokio::test]
async fn the_commands_refuse_what_they_can_check_before_asking_the_service() {
    let t = TestApp::new();
    let fake = Arc::new(Fake::default());
    serving(&t, Arc::clone(&fake));
    let app = || t.handle().clone();
    assert!(crate::commands::labs::lab_park(app(), "absent".into(), Runtime::Vm, crate::runtime::Park::Pause).await.is_err(), "not installed");
    t.lab("ad", &[]);
    let err = crate::commands::labs::lab_provision(app(), "ad".into(), Runtime::Vm, Some("a b".into())).await.unwrap_err();
    assert_eq!(err.to_string(), "invalid machine name `a b`");
    let pick = super::AttachPick { image: Some("bad image!".into()), ..super::AttachPick::default() };
    assert!(crate::commands::labs::attack_box_attach(app(), "ad".into(), Runtime::Docker, pick).await.is_err());
    assert!(fake.taken.lock().unwrap().is_empty(), "nothing asked of the service");
}

#[tokio::test]
async fn the_launchers_own_service_tells_an_operation_and_its_failure_by_its_id() {
    let t = TestApp::new();
    let _tools = crate::runtime::providers::testing::no_tools();
    // A resume of a lab that isn't installed fails in the app, before any worker.
    let id = crate::app::runs::of(t.handle()).start(Request::Resume { lab_id: "local-x".into(), runtime: Runtime::Docker }).unwrap();
    let (outcome, _lines) = finished(t.handle(), &id).await;
    let error = outcome.unwrap_err();
    assert_eq!(error.message, "lab `local-x` is not installed");
    let events = crate::app::runs::of(t.handle()).events().since(None);
    let RunEvent::Started { op, lab_id, deploying, runtime, .. } = &events[0].event else { panic!("{events:?}") };
    assert_eq!((*op, lab_id.as_str(), *deploying, *runtime), (Operation::Resume, "local-x", true, Some(Runtime::Docker)));
    assert!(events.iter().any(|e| matches!(&e.event, RunEvent::Ended { operation, .. } if *operation == id)), "ended");
}

#[tokio::test]
async fn a_second_operation_on_a_busy_lab_is_refused_and_a_panicking_one_still_ends() {
    let t = TestApp::new();
    let service = super::LocalRunService::new(t.dirs());
    let started = |id: &OperationId| RunEvent::Started {
        operation: id.clone(),
        lab_id: "web".into(),
        op: Operation::Launch,
        machine: None,
        runtime: None,
        deploying: true,
        at: 0,
    };
    // An operation under way that never ends by itself.
    let first = OperationId::new();
    service.begin(started(&first), true, |_| Box::pin(std::future::pending())).unwrap();
    let again = service.start(Request::Resume { lab_id: "web".into(), runtime: Runtime::Docker }).unwrap_err();
    assert_eq!(again.code(), crate::error::Code::AlreadyInProgress);
    let refused = OperationId::new();
    assert!(service.begin(started(&refused), true, |_| Box::pin(async { Ok(()) })).is_err(), "checked and admitted as one step");
    assert!(!service.events().since(None).iter().any(|e| matches!(&e.event, RunEvent::Started { operation, .. } if *operation == refused)), "nothing told");
    // One that panics ends all the same, as failed, its lab no longer busy.
    let t2 = TestApp::new();
    let service = super::LocalRunService::new(t2.dirs());
    let panics = OperationId::new();
    service.begin(started(&panics), true, |_| Box::pin(async { buggy_step() })).unwrap();
    let outcome = service.events().ended(&panics).await;
    assert!(outcome.unwrap_err().message.contains("internal error: a bug in a step"));
    assert!(!service.is_busy("web"));
}

#[tokio::test]
async fn two_starts_of_a_lab_one_is_refused_and_a_start_during_a_stop_is_refused_too() {
    let t = TestApp::new();
    let _tools = crate::runtime::providers::testing::no_tools();
    t.lab("web", &[]);
    let service = std::sync::Arc::new(super::LocalRunService::new(t.dirs()));
    let resume = || Request::Resume { lab_id: "web".into(), runtime: Runtime::Docker };
    // A start under way (held here), and a second start of the lab: refused, never begun.
    let start = service.hold("web", Operation::Launch);
    assert_eq!(service.start(resume()).unwrap_err().code(), crate::error::Code::AlreadyInProgress);
    drop(start);
    // A stop under way: a start meanwhile is refused as well.
    let stop = service.hold("web", Operation::Stop);
    assert_eq!(service.start(resume()).unwrap_err().code(), crate::error::Code::AlreadyInProgress);
    drop(stop);
    // Free again: the next one is taken.
    let id = service.start(resume()).unwrap();
    let _ = service.events().ended(&id).await;
}

#[tokio::test]
async fn a_stop_waits_for_the_operation_asked_before_it_then_runs() {
    let t = TestApp::new();
    let _tools = crate::runtime::providers::testing::no_tools();
    t.lab("web", &[]);
    let service = super::LocalRunService::new(t.dirs());
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let launch = OperationId::new();
    let started =
        RunEvent::Started { operation: launch.clone(), lab_id: "web".into(), op: Operation::Launch, machine: None, runtime: None, deploying: true, at: 0 };
    service
        .begin(started, true, |_| {
            Box::pin(async move {
                let _ = released.await;
                Ok(())
            })
        })
        .unwrap();
    // Asked while the launch runs (still downloading, say): taken, never refused, but it waits.
    // Its end is waited on, to a deadline: one that ran beside the launch ends well within it.
    let stop = service.start(Request::Stop { lab_id: "web".into(), runtime: Runtime::Docker }).unwrap();
    let beside = tokio::time::timeout(Duration::from_millis(300), service.events().ended(&stop)).await;
    assert!(beside.is_err(), "the stop runs after the launch, never beside it: {beside:?}");
    release.send(()).unwrap();
    assert_eq!(service.events().ended(&launch).await, Ok(()));
    service.events().ended(&stop).await.unwrap();
    let ends: Vec<OperationId> = service
        .events()
        .since(None)
        .into_iter()
        .filter_map(|e| match e.event {
            RunEvent::Ended { operation, .. } => Some(operation),
            _ => None,
        })
        .collect();
    assert_eq!(ends, [launch, stop], "the launch ends first, then the stop");
    assert!(!service.is_busy("web"));
}

#[tokio::test]
async fn an_operation_cut_off_with_its_process_leaves_nothing_holding_the_lab() {
    let t = TestApp::new();
    let _tools = crate::runtime::providers::testing::no_tools();
    // A process that died mid-operation: its account went with it, and no lock file stayed.
    let dead = std::sync::Arc::new(super::LocalRunService::new(t.dirs()));
    std::mem::forget(dead.hold("web", Operation::Launch));
    let next = super::LocalRunService::new(t.dirs());
    let id = next.start(Request::Resume { lab_id: "web".into(), runtime: Runtime::Docker }).unwrap();
    let outcome = next.events().ended(&id).await;
    assert_ne!(outcome.unwrap_err().code, crate::error::Code::AlreadyInProgress, "not held by the dead one");
    let locks: Vec<_> = walk(t.data()).into_iter().filter(|p| p.extension().is_some_and(|e| e == "lock")).collect();
    assert!(locks.is_empty(), "no lab lock file: {locks:?}");
}

/// Every file under `dir`.
fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries.flatten().flat_map(|e| if e.path().is_dir() { walk(&e.path()) } else { vec![e.path()] }).collect()
}

#[tokio::test]
async fn an_operation_run_outside_a_request_is_told_like_the_others() {
    let t = TestApp::new();
    let service = super::LocalRunService::of(&t.dirs()).unwrap();
    assert!(std::sync::Arc::ptr_eq(&service, &super::LocalRunService::of(&t.dirs()).unwrap()), "one per data folder");
    let url = service
        .observe("web", Operation::Launch, Some(Runtime::Docker), |log| async move {
            log("==> web: Starting".into());
            // A step the runner said within the operation reaches its events.
            let step = crate::runtime::deploy_log::DeployStep {
                step: "up".into(),
                state: crate::runtime::deploy_log::DeployStepState::Started,
                label: Some("web".into()),
                count: None,
            };
            crate::runtime::deploy_log::tell(step);
            Ok(Some("http://127.0.0.1:8080".to_owned()))
        })
        .await
        .unwrap();
    assert_eq!(url.as_deref(), Some("http://127.0.0.1:8080"));
    let kinds: Vec<&str> = service
        .events()
        .since(None)
        .iter()
        .map(|e| match e.event {
            RunEvent::Started { .. } => "started",
            RunEvent::Log { .. } => "log",
            RunEvent::Step { .. } => "step",
            RunEvent::Ended { .. } => "ended",
            RunEvent::Status { .. } | RunEvent::Running { .. } => "lab",
        })
        .collect();
    assert_eq!(kinds, ["started", "log", "step", "ended"]);
}

/// A step with a bug: it panics.
fn buggy_step() -> crate::error::Result<()> {
    panic!("a bug in a step")
}

#[tokio::test]
async fn a_watched_lab_has_its_status_told_at_once_then_only_when_it_changes() {
    let t = TestApp::new();
    let _tools = crate::runtime::providers::testing::no_tools();
    let service = super::LocalRunService::new(t.dirs());
    // Not installed: not running, read without touching any engine.
    service.watch(vec![("web".into(), Runtime::Docker)], Vec::new());
    let statuses = || -> Vec<(String, bool)> {
        service
            .events()
            .since(None)
            .into_iter()
            .filter_map(|e| match e.event {
                RunEvent::Status { lab_id, status: Some(s), .. } => Some((lab_id, s.running)),
                _ => None,
            })
            .collect()
    };
    for _ in 0..500 {
        if !statuses().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(statuses(), [("web".to_owned(), false)]);
    // A pass that reads it again finds the same: nothing more is told.
    service.reconcile_once().await;
    assert_eq!(statuses(), [("web".to_owned(), false)]);
}

#[tokio::test]
async fn the_webview_says_what_it_watches_through_its_command() {
    let t = TestApp::new();
    let fake = Arc::new(Fake::default());
    serving(&t, Arc::clone(&fake));
    let lab = |id: &str| serde_json::from_value(serde_json::json!({ "id": id, "runtime": "VM" })).unwrap();
    crate::commands::labs::labs_watch(t.handle().clone(), vec![lab("ad")], vec!["ad".into()]).await.unwrap();
    assert_eq!(*fake.watched.lock().unwrap(), [(vec![("ad".to_owned(), Runtime::Vm)], vec!["ad".to_owned()])]);
    assert!(crate::commands::labs::labs_watch(t.handle().clone(), vec![lab("../x")], Vec::new()).await.is_err(), "a bad id");
}
