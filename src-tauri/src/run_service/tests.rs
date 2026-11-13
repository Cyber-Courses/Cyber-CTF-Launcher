//! The run service: the commands ask it for operations and the webview gets each one's start, log
//! and end as events, from the launcher's own service and from any other behind the trait.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::Manager;

use super::{BoxFuture, EventLog, OperationId, Request, RunEvent, RunService};
use crate::error::{Result, Wire};
use crate::runtime::{Operation, Runtime};
use crate::testing::test_app::TestApp;

/// Waits for operation `id`'s end among `app`'s run events: `Ok` when it went through, its error
/// when it failed. Every log line it sent, in order, beside.
pub(crate) async fn finished(app: &crate::AppHandle, id: &OperationId) -> (std::result::Result<(), Wire>, Vec<String>) {
    let events = super::of(app);
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
#[derive(Default)]
struct Fake {
    events: EventLog,
    taken: Mutex<Vec<(OperationId, String, Operation)>>,
    cancelled: Mutex<Vec<OperationId>>,
}

impl RunService for Fake {
    fn start(&self, request: Request) -> Result<OperationId> {
        let id = OperationId(format!("fake-{}", self.taken.lock().unwrap().len() + 1));
        self.taken.lock().unwrap().push((id.clone(), request.lab_id().to_owned(), request.operation()));
        Ok(id)
    }

    fn cancel(&self, operation: &OperationId) -> BoxFuture<'_, ()> {
        self.cancelled.lock().unwrap().push(operation.clone());
        Box::pin(async {})
    }

    fn events(&self) -> &EventLog {
        &self.events
    }

    fn pick_up_running(&self) {}
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
    crate::commands::labs::operation_cancel(t.handle().clone(), id.clone()).await;
    assert_eq!(*fake.cancelled.lock().unwrap(), [id]);
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
    let _tools = crate::platform::exec::fake::Tools::rules(&[]);
    // A resume of a lab that isn't installed fails in the app, before any worker.
    let id = super::of(t.handle()).start(Request::Resume { lab_id: "local-x".into(), runtime: Runtime::Docker }).unwrap();
    let (outcome, _lines) = finished(t.handle(), &id).await;
    let error = outcome.unwrap_err();
    assert_eq!(error.message, "lab `local-x` is not installed");
    let events = super::of(t.handle()).events().since(None);
    let RunEvent::Started { op, lab_id, deploying, runtime, .. } = &events[0].event else { panic!("{events:?}") };
    assert_eq!((*op, lab_id.as_str(), *deploying, *runtime), (Operation::Resume, "local-x", true, Some(Runtime::Docker)));
    assert!(events.iter().any(|e| matches!(&e.event, RunEvent::Ended { operation, .. } if *operation == id)), "ended");
}

#[tokio::test]
async fn a_second_operation_on_a_busy_lab_is_refused_but_a_stop_is_taken() {
    let t = TestApp::new();
    let service = super::LocalRunService::new(t.dirs());
    // An operation under way that never ends by itself.
    let id = OperationId::new();
    let started =
        RunEvent::Started { operation: id.clone(), lab_id: "web".into(), op: Operation::Launch, machine: None, runtime: None, deploying: true, at: 0 };
    service.begin(started, false, |_| Box::pin(std::future::pending()));
    let again = service.start(Request::Resume { lab_id: "web".into(), runtime: Runtime::Docker }).unwrap_err();
    assert_eq!(again.code(), crate::error::Code::AlreadyInProgress);
    // Cancelled: it ends, once, as cancelled, and the app's log names it by the id the page shows,
    // for the player to quote.
    crate::testing::logs::take_warnings();
    service.cancel(&id).await;
    service.cancel(&id).await;
    let warned = crate::testing::logs::take_warnings();
    assert_eq!(warned, [format!("operation {id}: Launch of lab web failed: the operation was cancelled before it ended")]);
    let ends: Vec<_> = service.events().since(None).into_iter().filter(|e| matches!(e.event, RunEvent::Ended { .. })).collect();
    let [end] = ends.as_slice() else { panic!("{ends:?}") };
    let RunEvent::Ended { error: Some(error), .. } = &end.event else { panic!("{end:?}") };
    assert!(error.message.contains("cancelled"), "{error:?}");
    assert!(!service.busy("web"));
}
