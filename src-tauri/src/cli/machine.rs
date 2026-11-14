//! The machine commands: the machine check, the guided setup (its plan, then each step run by the
//! functions the setup window's commands call, `machine::setup::run`), the tools Cyber CTF
//! installs, the storage, and the doctor's report of the app's health screens.

use std::collections::VecDeque;

use serde_json::{Value, json};

use super::args::{Machine, SetupFor, Storage, Tools};
use super::{Answer, Context, Exit, Failure, Output, by_id, to_json};
use crate::error::Error;
use crate::machine::setup::run::{Event, OnFailure, Outcome, left_alone, walk, would};
use crate::machine::setup::{Plan, SetupPath, Wanted, plan};
use crate::machine::system::system_check;
use crate::machine::tools::install::{self, Dependency};
use crate::machine::tools::uninstall;
use crate::runtime::providers::Provider;

/// How many of a failed step's last lines its error repeats.
const TAIL: usize = 20;

/// Runs a machine command.
pub(super) async fn machine(command: Machine, ctx: &Context, out: &mut Output<'_>) -> Exit {
    match command {
        Machine::Check => {
            let report = system_check(ctx.dirs.clone()).await;
            out.finish(to_json(&report), check_text)
        }
        Machine::Setup { paths, providers, engine, dry_run } => match wanted(&paths, &providers, engine.as_deref()) {
            Ok(wanted) => setup(ctx, out, wanted, dry_run).await,
            Err(e) => out.fail(&e),
        },
        Machine::Tools(Tools::List) => {
            let tools = uninstall::installed_tools(ctx.dirs.clone()).await;
            out.finish(to_json(&tools), |t| {
                lines_of(
                    t,
                    |t| format!("{} ({})", t["name"].as_str().unwrap_or_default(), t["dependency"].as_str().unwrap_or_default()),
                    "Cyber CTF installed no tool here.",
                )
            })
        }
        Machine::Tools(Tools::Install { tool }) => tool_change(ctx, out, &tool, Change::Install).await,
        Machine::Tools(Tools::Remove { tool }) => tool_change(ctx, out, &tool, Change::Remove).await,
    }
}

/// What the setup is for, from the command line: the lab types (default both), the hypervisors in
/// order, the engine.
fn wanted(paths: &[SetupFor], providers: &[String], engine: Option<&str>) -> Result<Wanted, Failure> {
    let paths = if paths.is_empty() {
        vec![SetupPath::Docker, SetupPath::Vm]
    } else {
        paths.iter().map(|p| if *p == SetupFor::Docker { SetupPath::Docker } else { SetupPath::Vm }).collect()
    };
    let hypervisors =
        providers.iter().map(|p| Provider::from_id(p).ok_or_else(|| Failure::usage(format!("unknown provider `{p}`")))).collect::<Result<Vec<_>, _>>()?;
    let engine = engine.map(|e| by_id("Docker engine", e)).transpose()?;
    Ok(Wanted { paths, engine, hypervisors })
}

/// The plan `p` as JSON: each step with its item, kind, action and what it does.
fn plan_json(p: &Plan) -> Value {
    let steps: Vec<Value> = p
        .steps
        .iter()
        .map(|s| {
            let does = would(&s.action).or_else(|| {
                left_alone(&s.action).map(|o| serde_json::to_value(o).map(|v| v["note"].as_str().unwrap_or_default().to_owned()).unwrap_or_default())
            });
            json!({ "item": s.item, "kind": crate::machine::setup::run::kind(&s.action), "action": s.action, "does": does, "endsPass": s.action.ends_pass() })
        })
        .collect();
    let passed_over: Vec<Value> = p.passed_over.iter().map(|(provider, why)| json!({ "provider": provider, "why": why })).collect();
    json!({ "steps": steps, "hypervisor": p.hypervisor, "passedOver": passed_over, "items": p.items })
}

/// A plan as text: its steps in order, the hypervisors passed over.
fn plan_text(p: &Value) -> String {
    let mut lines: Vec<String> = p["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| format!("  {}: {}", s["item"].as_str().unwrap_or_default(), s["does"].as_str().unwrap_or_default()))
        .collect();
    if lines.is_empty() {
        lines.push("Nothing to do: this machine is set up.".into());
    }
    for o in p["passedOver"].as_array().into_iter().flatten() {
        lines.push(format!("  {} passed over: {}", o["provider"].as_str().unwrap_or_default(), o["why"].as_str().unwrap_or_default()));
    }
    lines.join("\n")
}

/// The guided setup `wanted` on this machine: the plan (all of it, with `dry_run`), then, once
/// confirmed, each step run and told as it goes; stops at the first that fails, with its name, its
/// output's last lines and the setup's id. A step that ends a pass (a restart, a new login) is
/// never run: the setup stops before it ([`Exit::NotReady`]), and runs again after it.
async fn setup(ctx: &Context, out: &mut Output<'_>, wanted: Wanted, dry_run: bool) -> Exit {
    let report = system_check(ctx.dirs.clone()).await;
    let first = plan(&report, &report.setup, &wanted);
    let shown = plan_json(&first);
    if dry_run || first.steps.is_empty() {
        return out.finish(Ok(json!({ "plan": shown, "ran": false })), |v| plan_text(&v["plan"]));
    }
    out.say(&plan_text(&shown));
    if let Err(e) = ctx.confirm(out, &format!("Set this machine up ({} steps)", first.steps.len())) {
        return out.fail(&e);
    }
    if ctx.unattended()
        && let Err(e) = crate::platform::elevation::check_unattended().await
    {
        return out.fail(&e.into());
    }
    let operation = format!("setup-{}", crate::platform::random_hex(8));
    let wanted = Wanted { hypervisors: first.hypervisor.into_iter().collect(), ..wanted };
    let mut said = Said::default();
    let walked = walk(&ctx.dirs, &wanted, report, OnFailure::Stop, |event| said.tell(out, &operation, &event)).await;
    if let Some(step) = walked.failed {
        let tail: Vec<String> = said.tail.into_iter().collect();
        let message = format!("step {} failed: {}\n{}", step.item, said.last_note, tail.join("\n"));
        return out.fail(&Failure::of_operation(Error::Invalid(message.trim_end().to_owned()).to_wire(), Some(operation)));
    }
    if let Some(end) = walked.ends {
        let what = would(&end.action).unwrap_or_default().replacen("would ", "", 1);
        let failure = Failure { operation: Some(operation), ..Failure::not_ready(format!("{}: {what}, then run the setup again to finish it", end.item)) };
        return out.fail(&failure);
    }
    out.finish(Ok(json!({ "operation": operation, "ran": true, "acted": walked.acted, "hypervisor": first.hypervisor })), |_| {
        "This machine is set up.".to_owned()
    })
}

/// What a setup told so far: the current step's last lines, and how the last step ended.
#[derive(Default)]
struct Said {
    /// The current step's last lines ([`TAIL`]).
    tail: VecDeque<String>,
    /// The last step's note.
    last_note: String,
}

impl Said {
    /// Tells `event` of setup `operation` on `out`, and keeps what a failure repeats.
    fn tell(&mut self, out: &mut Output<'_>, operation: &str, event: &Event) {
        let text = match event {
            Event::Started { item, action, kind } => {
                self.tail.clear();
                format!("==> {item}: {}", would(action).map_or_else(|| (*kind).to_owned(), |w| w.replacen("would ", "", 1)))
            }
            Event::Line { line, .. } => {
                if self.tail.len() == TAIL {
                    self.tail.pop_front();
                }
                self.tail.push_back(line.clone());
                format!("    {line}")
            }
            Event::Ended { item, outcome } => {
                let (word, note) = match outcome {
                    Outcome::Done(n) => ("done", n),
                    Outcome::Skipped(n) => ("skipped", n),
                    Outcome::Failed(n) => ("FAILED", n),
                };
                self.last_note.clone_from(note);
                format!("<== {item}: {word}: {note}")
            }
        };
        let mut shown = serde_json::to_value(event).unwrap_or(Value::Null);
        if let Some(fields) = shown.as_object_mut() {
            fields.insert("operation".into(), json!(operation));
        }
        out.event(&shown, Some(&text));
    }
}

/// A tool installed or removed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Change {
    /// Installed.
    Install,
    /// Removed.
    Remove,
}

/// Installs or removes `tool` (a Vagrant plugin as `plugin:<name>`, installed only), confirmed,
/// its output streamed.
async fn tool_change(ctx: &Context, out: &mut Output<'_>, tool: &str, change: Change) -> Exit {
    let verb = if change == Change::Install { "Install" } else { "Remove" };
    if let Err(e) = ctx.confirm(out, &format!("{verb} {tool}")) {
        return out.fail(&e);
    }
    let plugin = tool.strip_prefix("plugin:");
    let dependency = match plugin {
        Some(_) if change == Change::Remove => return out.fail(&Failure::usage("a Vagrant plugin is removed with Vagrant, not on its own")),
        Some(_) => None,
        None => match by_id::<Dependency>("tool", tool) {
            Ok(d) => Some(d),
            Err(e) => return out.fail(&e),
        },
    };
    if ctx.unattended()
        && plugin.is_none()
        && let Err(e) = crate::platform::elevation::check_unattended().await
    {
        return out.fail(&e.into());
    }
    let mut on_line = |line: String| out.event(&json!({ "event": "line", "item": tool, "line": line }), Some(&format!("    {line}")));
    let done = match (plugin, dependency) {
        (Some(name), _) => install::install_vagrant_plugin(name.to_owned(), &mut on_line).await,
        (None, Some(d)) if change == Change::Install => install::install_dependency(ctx.dirs.clone(), d, &mut on_line).await,
        (None, Some(d)) => uninstall::uninstall_dependency(ctx.dirs.clone(), d, &mut on_line).await,
        (None, None) => Err(Error::Invalid(format!("unknown tool `{tool}`"))),
    };
    out.finish(done.map(|()| json!({ "tool": tool, "change": verb.to_ascii_lowercase() })).map_err(Failure::from), |_| format!("{tool}: done."))
}

/// Runs a storage command.
pub(super) async fn storage(command: Storage, ctx: &Context, out: &mut Output<'_>) -> Exit {
    match command {
        Storage::Show => {
            let answer = crate::machine::storage::machine_storage(ctx.dirs.clone(), Vec::new()).await.map_err(Failure::from).and_then(|s| to_json(&s));
            out.finish(answer, storage_text)
        }
        Storage::Clean => {
            if let Err(e) = ctx.confirm(out, "Remove what Cyber CTF stored that no lab needs now") {
                return out.fail(&e);
            }
            let answer =
                crate::commands::machine::storage_clean_in(ctx.dirs.clone(), Vec::new()).await.map(|freed| json!({ "freed": freed })).map_err(Failure::from);
            out.finish(answer, |v| format!("Freed {} bytes.", v["freed"]))
        }
    }
}

/// The storage as text: each kind's items, with their sizes.
fn storage_text(s: &Value) -> String {
    let mut lines = Vec::new();
    for kind in ["images", "boxes", "plugins"] {
        for item in s[kind].as_array().into_iter().flatten() {
            lines.push(format!("{kind:<8} {:>14} {}", item["bytes"], item["name"].as_str().unwrap_or_default()));
        }
    }
    if lines.is_empty() { "Cyber CTF stores nothing here.".into() } else { lines.join("\n") }
}

/// The doctor: the app's health screens in one report (the machine check, the account, the
/// servers, the labs).
pub(super) async fn doctor(ctx: &Context, out: &mut Output<'_>) -> Exit {
    let answer = report(ctx).await;
    out.finish(answer, doctor_text)
}

/// [`doctor`]'s report.
async fn report(ctx: &Context) -> Answer {
    let machine = system_check(ctx.dirs.clone()).await;
    let account = crate::account::auth::auth_status().await;
    let servers = crate::commands::server::host_list(&ctx.dirs).map_err(Failure::from).and_then(|l| to_json(&l));
    let labs = super::labs::list(ctx).await;
    let data = ctx.dirs.data().ok().map(|d| d.display().to_string());
    let problem = |a: &Answer| a.as_ref().err().map(|f| f.error.message.clone());
    Ok(json!({
        "version": super::version(),
        "dataDir": data,
        "machine": to_json(&machine)?,
        "account": to_json(&account)?,
        "servers": servers.as_ref().ok(),
        "serversError": problem(&servers),
        "labs": labs.as_ref().ok(),
        "labsError": problem(&labs),
    }))
}

/// The doctor's report as text: one line a screen.
fn doctor_text(r: &Value) -> String {
    let account = &r["account"];
    let who = if account["loggedIn"] == true { format!("signed in as {}", account["email"].as_str().unwrap_or_default()) } else { "signed out".to_owned() };
    let servers = r["servers"]["hosts"].as_array().map_or(0, Vec::len);
    let labs = r["labs"].as_array().map_or(0, Vec::len);
    [
        format!("cyberctf {}", r["version"]["version"].as_str().unwrap_or_default()),
        format!("data: {}", r["dataDir"].as_str().unwrap_or("unknown")),
        check_text(&r["machine"]),
        format!("account: {who}"),
        format!("servers: {servers} saved"),
        format!("labs: {labs} installed"),
    ]
    .join("\n")
}

/// The machine check as text: the OS, Docker, the tools, the hypervisors ready.
fn check_text(r: &Value) -> String {
    let tool = |name: &str, t: &Value| format!("{name}: {}", if t["installed"] == true { t["version"].as_str().unwrap_or("installed") } else { "missing" });
    let ready: Vec<&str> = r["readyHypervisors"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    [
        format!(
            "machine: {} {} ({})",
            r["os"].as_str().unwrap_or_default(),
            r["arch"].as_str().unwrap_or_default(),
            r["pkgManager"]["name"].as_str().unwrap_or_default()
        ),
        format!("{} ({})", tool("docker", &r["docker"]), if r["dockerRunning"] == true { "running" } else { "not running" }),
        tool("compose", &r["dockerCompose"]),
        tool("vagrant", &r["vagrant"]),
        tool("terraform", &r["terraform"]),
        format!("hypervisors ready: {}", if ready.is_empty() { "none".to_owned() } else { ready.join(", ") }),
    ]
    .join("\n")
}

/// The items of the array `v`, one a line by `line`, or `empty` when there are none.
fn lines_of(v: &Value, line: impl Fn(&Value) -> String, empty: &str) -> String {
    let lines: Vec<String> = v.as_array().into_iter().flatten().map(line).collect();
    if lines.is_empty() { empty.to_owned() } else { lines.join("\n") }
}
