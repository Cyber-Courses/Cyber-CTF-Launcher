//! The contract between the launcher and Isothrum's runner service: what crosses the wire. Each
//! operation goes from the launcher's own client path (`remote`, its connection pool) to a real
//! runner service in this process (`testing::service`), whose runner's fake processes are the only
//! fake. A request or reply whose shape or meaning changes on either side fails here: the service
//! journals each operation as the launcher asked it (and nothing for a read or a refusal), the
//! values the launcher sends reach the tool, and the service's refusals and errors read as the
//! launcher's one mapper (`errors::coded`) says.
//!
//! The lab's life through the same path (its page, its checks, the attack box) is the runner
//! tests' (`runtime/isothrum/tests.rs`); this one holds the protocol itself.

use std::sync::Arc;

use isothrum_client::v1::Outcome;
use isothrum_runner::{ErrorCode, Op};

use super::super::*;
use crate::error::Code;

/// The lab's run's operations as the service journaled them, oldest first: what each did, and
/// how it ended (with its error's code).
async fn journaled(f: &Fixture) -> Vec<(Op, Option<(Outcome, Option<ErrorCode>)>)> {
    let name = RunName::new("cyberctf-web").unwrap();
    let rows = remote::journal(&f.home.runner(), &name).await.unwrap();
    rows.into_iter().rev().map(|r| (r.op, r.ended.map(|e| (e.outcome, e.code)))).collect()
}

/// How long a refused operation may take to be refused: a deadline only there to stop a hang.
const REFUSED_WITHIN: std::time::Duration = std::time::Duration::from_secs(60);

/// The error `asked` is refused with, at once: one queued behind an operation that never ends
/// would wait for good.
async fn refused(asked: impl Future<Output = crate::error::Result<()>>) -> Error {
    tokio::time::timeout(REFUSED_WITHIN, asked).await.expect("refused at once, not queued").unwrap_err()
}

/// An operation that went through.
const DONE: Option<(Outcome, Option<ErrorCode>)> = Some((Outcome::Succeeded, None));

#[tokio::test]
async fn each_operation_the_launcher_asks_is_journaled_by_the_service_as_asked() {
    let f = Fixture::new();
    let _tools = no_tools();
    let run = f.up().await;
    let up = last_compose(&f, "up", None).unwrap();
    assert_eq!(up.env("CTF_LAUNCH_TOKEN"), Some("t0k"), "the launch's value crossed as a secret and reached the tool");
    assert_eq!(up.env("ISOLOOM_PUBLISH_FIXED"), Some("1"), "and the ports choice as a value");
    assert!(run.status().await.unwrap().running, "a read, answered from the service");
    run.park(Park::Pause, &mut |_| {}).await.unwrap();
    assert!(last_compose(&f, "pause", None).is_some(), "the park's mode crossed: Pause");
    run.resume(&mut |_| {}).await.unwrap();
    run.park(Park::Shutdown, &mut |_| {}).await.unwrap();
    assert!(last_compose(&f, "stop", None).is_some(), "and Shutdown");
    run.resume(&mut |_| {}).await.unwrap();
    run.down(&mut |_| {}).await.unwrap();
    assert!(!run.status().await.unwrap().running);
    assert_eq!(
        journaled(&f).await,
        [(Op::Up, DONE), (Op::Park, DONE), (Op::Resume, DONE), (Op::Park, DONE), (Op::Resume, DONE), (Op::Down, DONE)],
        "every operation asked, once and in order; the status reads are none"
    );
}

#[tokio::test]
async fn a_second_operation_on_a_busy_run_is_refused_by_the_service_and_read_as_already_in_progress() {
    let f = Fixture::new();
    let _tools = no_tools();
    let began = Arc::new(tokio::sync::Notify::new());
    f.model().held_up = Some(Arc::clone(&began));
    let dirs = crate::platform::AppDirs::new(Some(f.home.root.clone()), None);
    let run = f.run();
    let env = launch_env();
    let mut first_log = |_| {};
    let first = run.up(on_own_ports(&env), None, &mut first_log);
    let meanwhile = async {
        began.notified().await;
        let again = refused(run.up(on_own_ports(&env), None, &mut |_| {})).await;
        let park = refused(run.park(Park::Pause, &mut |_| {})).await;
        let read = run.status().await;
        // A stop's first step: the run's operations under way, listed then cancelled.
        end_operations(&dirs, "web").await;
        (again, park, read)
    };
    let (first, (again, park, read)) = tokio::join!(first, meanwhile);
    assert_eq!(again.code(), Code::AlreadyInProgress, "{again}");
    assert_eq!(park.code(), Code::AlreadyInProgress, "{park}");
    assert!(read.is_ok(), "a read is no operation: answered while the run is busy: {read:?}");
    assert!(first.is_err(), "the held start was cut short");
    f.model().held_up = None;
    run.down(&mut |_| {}).await.unwrap();
    assert_eq!(
        journaled(&f).await,
        [(Op::Up, Some((Outcome::Cancelled, Some(ErrorCode::Cancelled)))), (Op::Down, DONE)],
        "the refused operations changed nothing and were never journaled"
    );
}

#[tokio::test]
async fn a_tools_failure_crosses_as_the_services_report_and_reads_through_the_one_mapper() {
    let f = Fixture::new();
    let _tools = no_tools();
    f.model().failing_pull = true;
    let err = f.run().up(on_own_ports(&launch_env()), None, &mut |_| {}).await.unwrap_err();
    let reason = "Error response from daemon: toomanyrequests: You have reached your pull rate limit (exited with code 18)";
    assert_eq!(err.code(), Code::DownloadFailed { reason: reason.into() }, "{err}");
    assert_eq!(journaled(&f).await, [(Op::Up, Some((Outcome::Failed, Some(ErrorCode::DownloadFailed))))]);
}
