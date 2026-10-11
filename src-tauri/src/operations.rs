//! What the UI asks about lab operations in flight, from both places they run: detached deploy
//! workers and this process (a stop, or the fallback when no worker could start). A reloaded or
//! relaunched window rehydrates its "starting" / "stopping" state from these.

use crate::AppHandle;
use serde::Serialize;

use crate::deploy_worker::{self, Op};
use crate::runtime::{self, Action, Park};

/// The labs currently starting or stopping. The UI reads this on load to rehydrate the "this lab
/// is starting" state after a window reload: the deploy keeps running in this process even when
/// the webview reloaded and lost its own in-memory deploy state.
#[tauri::command]
pub fn deploying_labs(app: AppHandle) -> Vec<String> {
    // Worker keys with a dot are attack VM starts beside a lab, not the lab's own deploy.
    let workers = deploy_worker::running(&app).into_iter().filter(|k| !k.contains('.'));
    merge_unique(runtime::deploying_labs(), workers)
}

/// `ids` followed by each of `more` not already in it, in order.
fn merge_unique(mut ids: Vec<String>, more: impl IntoIterator<Item = String>) -> Vec<String> {
    for id in more {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// One operation in flight on a lab, for the sidebar: what it is and the step it is at.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActiveOperation {
    lab_id: String,
    /// "launch", "resume", "pause", "shutdown", "provision", "attack_vm" or "stop".
    op: &'static str,
    /// The machine a provisioning run targets, when one.
    machine: Option<String>,
    /// The last meaningful line of the operation's log (detached workers only).
    step: Option<String>,
}

/// The sidebar's name for a worker's operation, and the machine it targets when one.
fn worker_op(op: &Op) -> (&'static str, Option<String>) {
    match op {
        Op::Launch => ("launch", None),
        Op::Resume { .. } => ("resume", None),
        Op::Park { mode: Park::Pause, .. } => ("pause", None),
        Op::Park { mode: Park::Shutdown, .. } => ("shutdown", None),
        Op::Provision { machine, .. } => ("provision", machine.clone()),
        Op::AttackVm { .. } => ("attack_vm", None),
    }
}

/// The sidebar's name for an in-process operation.
fn in_app_op(action: Action) -> &'static str {
    match action {
        Action::Start => "launch",
        Action::Stop => "stop",
        Action::Park => "pause",
    }
}

/// Every lab operation in flight: the detached workers (with the step their log is at) and the
/// in-process ones. A lab with a worker is listed once, as the worker's operation.
#[tauri::command]
pub fn active_operations(app: AppHandle) -> Vec<ActiveOperation> {
    merge_operations(deploy_worker::running_jobs(&app), runtime::active_actions())
}

/// The workers' operations (with their step), then the in-process ones on other labs, by lab id.
fn merge_operations(jobs: Vec<(deploy_worker::Job, Option<String>)>, actions: Vec<(String, Action)>) -> Vec<ActiveOperation> {
    let mut out: Vec<ActiveOperation> = jobs
        .into_iter()
        .map(|(job, step)| {
            let (op, machine) = worker_op(&job.op);
            ActiveOperation { lab_id: job.lab_id, op, machine, step }
        })
        .collect();
    for (id, action) in actions {
        if !out.iter().any(|o| o.lab_id == id) {
            out.push(ActiveOperation { lab_id: id, op: in_app_op(action), machine: None, step: None });
        }
    }
    out.sort_by(|a, b| a.lab_id.cmp(&b.lab_id));
    out
}

/// The log so far of a lab's deploy in its worker process, so a reloaded or relaunched app can
/// re-attach to a deploy still in progress.
#[tauri::command]
pub fn lab_deploy_log(app: AppHandle, id: String) -> std::result::Result<String, String> {
    deploy_worker::log_so_far(&app, &id).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Runtime;

    #[test]
    fn worker_lab_ids_are_added_once_after_the_in_app_ones() {
        let ids = merge_unique(vec!["b".into(), "a".into()], ["a".to_string(), "c".to_string(), "c".to_string()]);
        assert_eq!(ids, ["b", "a", "c"]);
    }

    #[test]
    fn a_lab_with_a_worker_is_listed_once_as_the_workers_operation() {
        let jobs = vec![
            (deploy_worker::Job::on_lab("web", Op::Launch), Some("Pulling images".to_string())),
            (deploy_worker::Job::on_lab("ad", Op::Provision { runtime: Runtime::Vm, machine: Some("dc01".into()) }), None),
        ];
        let actions = vec![("web".to_string(), Action::Stop), ("ctf".to_string(), Action::Park)];
        let ops = merge_operations(jobs, actions);
        assert_eq!(
            ops,
            [
                ActiveOperation { lab_id: "ad".into(), op: "provision", machine: Some("dc01".into()), step: None },
                ActiveOperation { lab_id: "ctf".into(), op: "pause", machine: None, step: None },
                ActiveOperation { lab_id: "web".into(), op: "launch", machine: None, step: Some("Pulling images".into()) },
            ]
        );
        assert!(merge_operations(Vec::new(), Vec::new()).is_empty());
        // Serialized for the sidebar in camelCase.
        assert_eq!(serde_json::to_value(&ops[0]).unwrap()["labId"], "ad");
    }

    #[test]
    fn operations_get_their_sidebar_names() {
        assert_eq!(worker_op(&Op::Launch), ("launch", None));
        assert_eq!(worker_op(&Op::Park { runtime: Runtime::Vm, mode: Park::Pause }).0, "pause");
        assert_eq!(worker_op(&Op::Park { runtime: Runtime::Vm, mode: Park::Shutdown }).0, "shutdown");
        assert_eq!(worker_op(&Op::Provision { runtime: Runtime::Vm, machine: Some("dc01".into()) }), ("provision", Some("dc01".into())));
        assert_eq!(worker_op(&Op::AttackVm { box_name: "a/b".into() }).0, "attack_vm");
        assert_eq!(in_app_op(Action::Start), "launch");
        assert_eq!(in_app_op(Action::Stop), "stop");
        assert_eq!(in_app_op(Action::Park), "pause");
    }
}
