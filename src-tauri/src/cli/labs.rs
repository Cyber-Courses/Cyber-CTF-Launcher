//! The lab and attack box commands: reads through the functions the lab page's commands call
//! (`commands::labs`'s `*_in`), operations through the run service, checked as the app checks them
//! (`commands::labs::start_checked`), their events streamed until the operation ends.

use std::path::Path;

use serde_json::{Value, json};

use super::args::{AttackBox, LabArg, Labs, RuntimeArg, StartArgs};
use super::output::masked;
use super::{Answer, Context, Exit, Failure, Output, to_json};
use crate::commands::labs::{box_status_in, check_in, local_request, remove_in, sign_in_in, start_checked, status_in, tools_in};
use crate::labs::LaunchRequest;
use crate::machine::system::docker::Engine;
use crate::run_service::{AttachPick, LocalRunService, Request, RunEvent, RunService};
use crate::runtime::paths::{installed_lab_dir, lab_dir, labs_root};
use crate::runtime::placement;
use crate::runtime::providers::Provider;
use crate::runtime::{Park, Runtime};

/// Runs a lab command.
pub(super) async fn labs(command: Labs, ctx: &Context, out: &mut Output<'_>) -> Exit {
    match command {
        Labs::List => {
            let answer = list(ctx).await;
            out.finish(answer, list_text)
        }
        Labs::Show { lab, show_secrets } => {
            let answer = show(ctx, &lab, show_secrets).await;
            out.finish(answer, show_text)
        }
        Labs::Status { lab } => {
            let answer = status(ctx, &lab).await;
            out.finish(answer, status_text)
        }
        Labs::Install { folder } => match local_request(folder) {
            Ok(request) => operate(ctx, out, request).await,
            Err(e) => out.fail(&e.into()),
        },
        Labs::Start(start) => start_lab(ctx, out, start).await,
        Labs::Stop { lab } => stop(ctx, out, lab).await,
        Labs::Park { lab, shutdown } => {
            let mode = if shutdown { Park::Shutdown } else { Park::Pause };
            on_lab(ctx, out, &lab, |lab_id, runtime| Request::Park { lab_id, runtime, mode }).await
        }
        Labs::Resume { lab } => on_lab(ctx, out, &lab, |lab_id, runtime| Request::Resume { lab_id, runtime }).await,
        Labs::Remove { id } => {
            let answer = remove(ctx, out, &id).await;
            out.finish(answer, |_| format!("Lab {id} removed."))
        }
        Labs::Logs { id, follow } => logs(ctx, out, &id, follow).await,
    }
}

/// Stops lab `lab`, confirmed.
async fn stop(ctx: &Context, out: &mut Output<'_>, lab: LabArg) -> Exit {
    let checked = with_runtime(ctx, &lab).and_then(|runtime| ctx.confirm(out, &format!("Stop lab {} and remove what it ran", lab.id)).map(|()| runtime));
    match checked {
        Ok(runtime) => operate(ctx, out, Request::Stop { lab_id: lab.id, runtime }).await,
        Err(e) => out.fail(&e),
    }
}

/// Runs an attack box command.
pub(super) async fn attack_box(command: AttackBox, ctx: &Context, out: &mut Output<'_>) -> Exit {
    match command {
        AttackBox::Start { lab, network, image, desktop } => {
            let pick = AttachPick { network, image, desktop };
            on_lab(ctx, out, &lab, |lab_id, runtime| Request::AttachBox { lab_id, runtime, pick }).await
        }
        AttackBox::Stop { lab } => on_lab(ctx, out, &lab, |lab_id, runtime| Request::DetachBox { lab_id, runtime }).await,
        AttackBox::Status { lab, show_secrets } => {
            let answer = box_status(ctx, &lab, show_secrets).await;
            out.finish(answer, box_text)
        }
    }
}

/// The runtime a lab command acts in: `--runtime`, else what the installed lab declares.
fn with_runtime(ctx: &Context, lab: &LabArg) -> Result<Runtime, Failure> {
    if let Some(r) = lab.runtime {
        return Ok(match r {
            RuntimeArg::Docker => Runtime::Docker,
            RuntimeArg::Vm => Runtime::Vm,
            RuntimeArg::Cloud => Runtime::Cloud,
        });
    }
    Ok(declared_runtime(&lab_dir(&ctx.dirs, &lab.id)?))
}

/// How the lab installed in `dir` runs, by its spec: cloud services when it declares a cloud, VMs
/// when its machines are operating systems with no container among them, else containers. What
/// the catalogue says of it; the app has it from there, a command on an installed lab from here.
pub(crate) fn declared_runtime(dir: &Path) -> Runtime {
    let spec: Value = std::fs::read_to_string(dir.join("isoloom.yml")).ok().and_then(|text| serde_yaml_ng::from_str(&text).ok()).unwrap_or(Value::Null);
    if !spec["cloud"].is_null() {
        return Runtime::Cloud;
    }
    let machines: Vec<&Value> = spec["machines"].as_object().map(|m| m.values().collect()).unwrap_or_default();
    let vm = |m: &&Value| !m["os"].is_null() && m["app"].is_null();
    if !machines.is_empty() && machines.iter().all(vm) { Runtime::Vm } else { Runtime::Docker }
}

/// Runs `make(lab id, runtime)` on lab `lab`, as an operation.
async fn on_lab(ctx: &Context, out: &mut Output<'_>, lab: &LabArg, make: impl FnOnce(String, Runtime) -> Request) -> Exit {
    match with_runtime(ctx, lab) {
        Ok(runtime) => operate(ctx, out, make(lab.id.clone(), runtime)).await,
        Err(e) => out.fail(&e),
    }
}

/// Starts a catalogue lab as the command line's options say ([`launch_request`]).
async fn start_lab(ctx: &Context, out: &mut Output<'_>, start: StartArgs) -> Exit {
    match launch_request(ctx, start).await {
        Ok(request) => operate(ctx, out, request).await,
        Err(e) => out.fail(&e),
    }
}

/// A catalogue launch's request, from the command line's options: its placement the app's one
/// rule ([`placement::choose`]) gives for the flags, else the saved preference, among what the
/// lab's spec at the catalogue's commit allows and what is ready here. Sent explicit, as the
/// window sends it.
async fn launch_request(ctx: &Context, s: StartArgs) -> Result<Request, Failure> {
    let choice = choice_of(&s)?;
    let listed = crate::labs::catalogue_source(&ctx.dirs, &s.id, s.offline).await?;
    let placement = placement::for_lab(&ctx.dirs, &s.id, listed.runtime, listed.source(), &choice).await?;
    Ok(Request::Launch(LaunchRequest { lab_id: s.id, placement, offline: s.offline }))
}

/// What the command line's flags pick; what they leave unset, the saved preference decides.
fn choice_of(s: &StartArgs) -> Result<placement::Choice, Failure> {
    let provider = match s.provider.as_deref() {
        Some(id) => Some(Provider::from_id(id).ok_or_else(|| Failure::usage(format!("unknown provider `{id}`")))?),
        None => None,
    };
    let engine = match s.docker_engine.as_deref() {
        Some(id) => Some(Engine::from_id(id).ok_or_else(|| Failure::usage(format!("unknown container engine `{id}`")))?),
        None => None,
    };
    Ok(placement::Choice {
        here: false,
        host: s.host.clone(),
        provider,
        per_machine_vm: s.per_machine_vm,
        engine,
        // Unset, the player's saved port mode decides, as in the window.
        default_ports: s.default_ports.then_some(true),
    })
}

/// Runs `request` on a run service of the app's folders, as the app's commands start it, its
/// events streamed until it ends. Ctrl+C stops following it: an operation in a deploy worker (a
/// launch, a resume, a park) goes on there, one in this process (a stop, the attack box) ends
/// with it.
async fn operate(ctx: &Context, out: &mut Output<'_>, request: Request) -> Exit {
    let service = LocalRunService::new(ctx.dirs.clone());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    service.events().listen(Box::new(move |e| {
        let _ = tx.send(e.event.clone());
    }));
    let operation = match start_checked(&ctx.dirs, &service, request) {
        Ok(id) => id,
        Err(e) => return out.fail(&e.into()),
    };
    loop {
        let event = tokio::select! {
            event = rx.recv() => event,
            _ = tokio::signal::ctrl_c() => {
                let failure = Failure::of_operation(crate::error::Error::Invalid("interrupted: what the lab's run was asked goes on in the runner service (`cyberctf labs logs <lab> --follow`)".into()).to_wire(), Some(operation.to_string()));
                return out.fail(&failure);
            }
        };
        let Some(event) = event else { break };
        let text = match &event {
            RunEvent::Started { op, lab_id, .. } => Some(format!("{op:?} of {lab_id} (operation {operation})")),
            RunEvent::Log { line, .. } => Some(line.clone()),
            RunEvent::Ended { .. } | RunEvent::Status { .. } | RunEvent::Running { .. } => None,
        };
        out.event(&event, text.as_deref());
        if let RunEvent::Ended { operation: ended, lab_id, op, error } = event
            && ended == operation
        {
            let answer = match error {
                None => Ok(json!({ "operation": operation, "lab": lab_id, "op": op })),
                Some(error) => Err(Failure::of_operation(error, Some(operation.to_string()))),
            };
            return out.finish(answer, |_| format!("{op:?} of {lab_id}: done."));
        }
    }
    out.fail(&Failure::of_operation(crate::error::Error::Invalid("the operation ended without a verdict".into()).to_wire(), Some(operation.to_string())))
}

/// The labs installed here, each with its name, runtime, the host it runs on, whether it runs and
/// whether an operation acts on it.
pub(super) async fn list(ctx: &Context) -> Answer {
    let root = labs_root(&ctx.dirs)?;
    let mut ids: Vec<String> = std::fs::read_dir(&root)
        .map(|entries| entries.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|id| crate::runtime::validate_id(id).is_ok()).collect())
        .unwrap_or_default();
    ids.sort();
    let imported: Vec<String> = crate::labs::imported::list(&ctx.dirs).map(|l| l.into_iter().map(|l| l.id).collect()).unwrap_or_default();
    let running = crate::runtime::recorded::running_here(&ctx.dirs).await;
    let busy = crate::app::operations::busy_labs(ctx.dirs.clone()).await.unwrap_or_default();
    let mut labs = Vec::new();
    for id in ids {
        let Some(dir) = installed_lab_dir(&ctx.dirs, &id)? else { continue };
        labs.push(json!({
            "id": id,
            "name": spec_name(&dir),
            "runtime": declared_runtime(&dir),
            "imported": imported.contains(&id),
            "host": crate::runtime::recorded::Recorded::of(&ctx.dirs, &id).await.ok().and_then(|r| r.host().map(str::to_owned)),
            "running": running.contains(&id),
            "busy": busy.contains(&id),
        }));
    }
    Ok(Value::Array(labs))
}

/// The `name` the lab's spec gives it, if any.
fn spec_name(dir: &Path) -> Option<String> {
    let spec: Value = serde_yaml_ng::from_str(&std::fs::read_to_string(dir.join("isoloom.yml")).ok()?).ok()?;
    spec["name"].as_str().map(str::to_owned)
}

/// [`list`] as text: one lab a line.
fn list_text(labs: &Value) -> String {
    let rows: Vec<String> = labs
        .as_array()
        .into_iter()
        .flatten()
        .map(|l| {
            let state = if l["busy"] == true {
                "busy"
            } else if l["running"] == true {
                "running"
            } else {
                "stopped"
            };
            format!(
                "{:<32} {:<8} {:<8} {}",
                l["id"].as_str().unwrap_or_default(),
                l["runtime"].as_str().unwrap_or_default(),
                state,
                l["name"].as_str().unwrap_or_default()
            )
        })
        .collect();
    if rows.is_empty() { "No lab is installed.".into() } else { rows.join("\n") }
}

/// What the lab page's details card shows of lab `lab`: its status (machines, ports, network,
/// where it runs, what it exposes), its observers, the attack box's sign-in; secrets masked
/// unless `show_secrets`.
async fn show(ctx: &Context, lab: &LabArg, show_secrets: bool) -> Answer {
    let runtime = with_runtime(ctx, lab)?;
    let mut status = status_in(&ctx.dirs, &lab.id, runtime).await?;
    for output in status.outputs.iter_mut().filter(|o| o.sensitive) {
        output.value = masked(&output.value, show_secrets);
    }
    let tools = tools_in(&ctx.dirs, &lab.id, runtime).await?;
    let sign_in = sign_in_in(&ctx.dirs, &lab.id)?.map(|s| json!({ "user": s.user, "password": masked(&s.password, show_secrets) }));
    let failed = crate::runtime::isothrum::last_run_failed(&ctx.dirs, &lab.id).await;
    Ok(json!({ "id": lab.id, "runtime": runtime, "status": to_json(&status)?, "tools": to_json(&tools)?, "attackBox": sign_in, "lastLaunchFailed": failed }))
}

/// [`show`] as the details card reads.
fn show_text(lab: &Value) -> String {
    let s = &lab["status"];
    let mut lines = vec![format!("{} ({})", lab["id"].as_str().unwrap_or_default(), lab["runtime"].as_str().unwrap_or_default()), state_line(s)];
    if let Some(url) = s["url"].as_str() {
        lines.push(format!("url: {url}"));
    }
    for m in s["machines"].as_array().into_iter().flatten().filter(|m| m["infra"] != true) {
        let ports: Vec<String> = m["ports"].as_array().into_iter().flatten().map(|p| format!("{}->{}", p["published"], p["target"])).collect();
        lines.push(format!(
            "  {} {} {} {}",
            m["name"].as_str().unwrap_or_default(),
            m["state"].as_str().unwrap_or_default(),
            m["ip"].as_str().unwrap_or_default(),
            ports.join(" ")
        ));
    }
    for o in s["outputs"].as_array().into_iter().flatten() {
        lines.push(format!("{}: {}", o["name"].as_str().unwrap_or_default(), o["value"].as_str().unwrap_or_default()));
    }
    if let Some(b) = lab["attackBox"].as_object() {
        lines.push(format!("attack box: {} / {}", b["user"].as_str().unwrap_or_default(), b["password"].as_str().unwrap_or_default()));
    }
    if lab["lastLaunchFailed"] == true {
        lines.push("its last launch failed: `cyberctf labs logs` shows why".into());
    }
    lines.join("\n")
}

/// Whether a status `s` runs, is parked, and where.
fn state_line(s: &Value) -> String {
    let state = if s["running"] == true {
        "running".to_owned()
    } else if let Some(p) = s["parked"].as_str() {
        format!("parked ({p})")
    } else {
        "not running".to_owned()
    };
    let place = [s["place"].as_str(), s["provider"].as_str(), s["host"].as_str()].into_iter().flatten().collect::<Vec<_>>().join(", ");
    if place.is_empty() { state } else { format!("{state} ({place})") }
}

/// Lab `lab`'s status, and its exploitability check where it runs.
async fn status(ctx: &Context, lab: &LabArg) -> Answer {
    let runtime = with_runtime(ctx, lab)?;
    let status = status_in(&ctx.dirs, &lab.id, runtime).await?;
    let check = if status.running { Some(to_json(&check_in(&ctx.dirs, &lab.id, runtime).await?)?) } else { None };
    Ok(json!({ "id": lab.id, "runtime": runtime, "status": to_json(&status)?, "check": check }))
}

/// [`status`] as text.
fn status_text(lab: &Value) -> String {
    let s = &lab["status"];
    let machines: Vec<String> = s["machines"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| format!("  {} {}", m["name"].as_str().unwrap_or_default(), m["state"].as_str().unwrap_or_default()))
        .collect();
    std::iter::once(format!("{}: {}", lab["id"].as_str().unwrap_or_default(), state_line(s))).chain(machines).collect::<Vec<_>>().join("\n")
}

/// Removes lab `id`, confirmed.
async fn remove(ctx: &Context, out: &mut Output<'_>, id: &str) -> Answer {
    crate::runtime::validate_id(id)?;
    ctx.confirm(out, &format!("Remove lab {id} and everything the app keeps for it"))?;
    to_json(&remove_in(&ctx.dirs, id).await?)
}

/// Lab `id`'s run's operations, as the runner service journals them, and with `follow` those
/// under way followed to their end, their lines as they come.
async fn logs(ctx: &Context, out: &mut Output<'_>, id: &str, follow: bool) -> Exit {
    if let Err(e) = crate::runtime::validate_id(id) {
        return out.fail(&e.into());
    }
    let journal = match crate::runtime::isothrum::journal_of(&ctx.dirs, id).await {
        Ok(journal) => journal,
        Err(e) => return out.fail(&e.into()),
    };
    let running = journal.as_array().into_iter().flatten().any(|o| o["ended"].is_null());
    if !(follow && running) {
        return out.finish(Ok(json!({ "lab": id, "running": running, "operations": journal })), |l| journal_text(&l["operations"]));
    }
    let mut lines = Vec::new();
    let followed = crate::runtime::isothrum::follow_under_way(&ctx.dirs, id, &mut |line| lines.push(line)).await;
    for line in &lines {
        out.event(&json!({ "kind": "log", "labId": id, "line": line }), Some(line));
    }
    match followed {
        Ok(()) => out.finish(Ok(json!({ "lab": id, "running": false })), |_| String::new()),
        Err(e) => out.fail(&e.into()),
    }
}

/// A run's journal as text: one line per operation, newest first.
fn journal_text(operations: &Value) -> String {
    let lines: Vec<String> = operations
        .as_array()
        .into_iter()
        .flatten()
        .map(|o| {
            let ended = if o["ended"].is_null() { "under way".to_owned() } else { o["ended"].to_string() };
            format!("{} {} {}", o["queued_at"].as_str().unwrap_or_default(), o["op"].as_str().unwrap_or_default(), ended)
        })
        .collect();
    if lines.is_empty() { "no operation recorded".to_owned() } else { lines.join("\n") }
}

/// The attack box of lab `lab`: its state, address, networks and sign-in.
async fn box_status(ctx: &Context, lab: &LabArg, show_secrets: bool) -> Answer {
    let runtime = with_runtime(ctx, lab)?;
    let status = box_status_in(&ctx.dirs, &lab.id, runtime).await?;
    let sign_in = sign_in_in(&ctx.dirs, &lab.id)?.map(|s| json!({ "user": s.user, "password": masked(&s.password, show_secrets) }));
    Ok(json!({ "lab": lab.id, "status": to_json(&status)?, "signIn": sign_in }))
}

/// [`box_status`] as text.
fn box_text(b: &Value) -> String {
    let s = &b["status"];
    let state = if s["running"] == true {
        "running"
    } else if s["reachable"] == true {
        "not running"
    } else {
        "unreachable"
    };
    let mut lines = vec![format!("attack box of {}: {state}", b["lab"].as_str().unwrap_or_default())];
    if let Some(ip) = s["ip"].as_str().filter(|ip| !ip.is_empty()) {
        lines.push(format!("ip: {ip} ({})", s["labNetwork"].as_str().unwrap_or_default()));
    }
    if let Some(si) = b["signIn"].as_object() {
        lines.push(format!("sign in: {} / {}", si["user"].as_str().unwrap_or_default(), si["password"].as_str().unwrap_or_default()));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(flags: &[&str]) -> StartArgs {
        use clap::Parser;
        let mut args = vec!["cyberctf", "labs", "start", "web"];
        args.extend_from_slice(flags);
        match super::super::args::Cli::try_parse_from(args).unwrap().command {
            super::super::args::Command::Labs(Labs::Start(s)) => s,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_flags_pick_what_they_say_and_leave_the_rest_to_the_saved_preference() {
        assert_eq!(choice_of(&start(&[])).unwrap(), placement::Choice::default());
        let all = start(&["--provider", "utm", "--host", "aws1", "--per-machine-vm", "--docker-engine", "colima", "--default-ports"]);
        let expected = placement::Choice {
            here: false,
            host: Some("aws1".into()),
            provider: Some(Provider::Utm),
            per_machine_vm: true,
            engine: Some(Engine::Colima),
            default_ports: Some(true),
        };
        assert_eq!(choice_of(&all).unwrap(), expected);
        assert!(choice_of(&start(&["--provider", "nope"])).is_err());
        assert!(choice_of(&start(&["--docker-engine", "nope"])).is_err());
    }
}
