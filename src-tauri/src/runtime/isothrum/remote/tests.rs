use isothrum_client::v1::{OperationId, phase_event};
use isothrum_runner::{Op, OperationPhase, RunName};

use super::{Others, OthersTold};

/// What `others` tells of `events` (each with whether its operation is this process's), in words.
fn told(others: &mut OthersTold, events: &[(isothrum_runner::Event, bool)]) -> Vec<String> {
    let mut said = Vec::new();
    for (event, mine) in events {
        others.take(event, *mine, &mut |o| {
            said.push(match o {
                Others::Began { operation, run, op } => format!("began {operation} {op:?} of {run}"),
                Others::Line { operation, .. } => format!("line {operation}"),
                Others::Ended { operation, error } => format!("ended {operation}{}", if error.is_some() { " failed" } else { "" }),
                Others::Left { run } => format!("left {run}"),
            });
        });
    }
    said
}

/// The `operation` event of operation `id`, an `up` of run `web`, saying `phase`.
fn phase(id: i64, phase: OperationPhase) -> isothrum_runner::Event {
    phase_event(&RunName::new("cyberctf-web").unwrap(), (Op::Up, OperationId(id)), phase, None, None)
}

#[test]
fn another_process_s_operation_is_told_begun_then_ended() {
    let mut others = OthersTold::since_now(Default::default());
    let said = told(&mut others, &[(phase(7, OperationPhase::Started), false), (phase(7, OperationPhase::Ended), false)]);
    assert_eq!(said, ["began 7 Up of cyberctf-web", "ended 7"]);
}

#[test]
fn an_operation_of_ours_is_never_told() {
    let mut others = OthersTold::since_now(Default::default());
    let said = told(&mut others, &[(phase(8, OperationPhase::Started), true), (phase(8, OperationPhase::Ended), true)]);
    assert!(said.is_empty(), "{said:?}");
}

#[test]
fn an_operation_of_ours_told_before_its_id_was_ours_is_told_ended_once_it_shows_to_be_ours() {
    // The stream carried its start before `operate` had its id: told as another's. Its end is
    // ours to tell, and without this one it stayed in flight, its lab busy and starting for good.
    let mut others = OthersTold::since_now(Default::default());
    let said =
        told(&mut others, &[(phase(9, OperationPhase::Queued), false), (phase(9, OperationPhase::Started), true), (phase(9, OperationPhase::Ended), true)]);
    assert_eq!(said, ["began 9 Up of cyberctf-web", "ended 9"]);
}
