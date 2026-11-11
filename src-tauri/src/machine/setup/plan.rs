//! A whole setup on one machine, in order: the options the machine check worked out for it
//! ([`super::Options`], the ones the window renders), for the lab types, engine and hypervisors
//! wanted. The headless entry the test rig runs (`integration_tests::setup`) walks it.

use serde::Deserialize;

use super::{Action, HypervisorOption, ManualReason, Options, engines_on, local, vagrant_steps};
use crate::machine::system::SystemReport;
use crate::machine::system::docker::Engine;
use crate::runtime::providers::{Provider, Unavailable};

impl Action {
    /// Whether it ends a setup pass: what follows waits for the restart or the new login. A
    /// hypervisor blocked until the computer restarts (VirtualBox's driver, built for the newer
    /// kernel its install brought) ends it too, as a restart.
    pub fn ends_pass(&self) -> bool {
        matches!(self, Action::Restart { .. } | Action::Relogin | Action::Blocked { reason: Unavailable::KernelRestart { .. } })
    }
}

/// A lab type the setup readies this machine for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SetupPath {
    /// Container labs: WSL 2 on Windows, a Docker engine.
    Docker,
    /// VM labs: a hypervisor, Vagrant and what it needs to drive it.
    Vm,
}

/// What a whole setup is for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wanted {
    pub paths: Vec<SetupPath>,
    /// The engine to set up; none: the setup's own ([`Options::engine`]).
    pub engine: Option<Engine>,
    /// The hypervisors to set up, in order: the first the setup can set up is the one. None: the
    /// setup's own ([`Options::hypervisor`]).
    pub hypervisors: Vec<Provider>,
}

/// One action of a plan, with the item it is about (`wsl`, `docker`, `engine:<id>`, `vagrant`,
/// `<hypervisor>`, `plugin:<name>`, `vmware-utility`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub item: String,
    pub action: Action,
}

/// A whole setup on one machine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// What to do, in order: what ends a pass (a restart, a new login) last.
    pub steps: Vec<Step>,
    /// The hypervisor set up, when VM labs are wanted and one can be.
    pub hypervisor: Option<Provider>,
    /// The wanted hypervisors not set up, each with why.
    pub passed_over: Vec<(Provider, String)>,
    /// Every item the setup covers, whether or not it has anything to do for it.
    pub items: Vec<String>,
}

/// Why `option` can't be the hypervisor a setup sets up, if it can't: something the player gets,
/// or why it can't run here.
fn not_settable(option: &HypervisorOption) -> Option<String> {
    option.get.iter().find_map(|action| match action {
        Action::Manual { reason: ManualReason::Vendor, url, .. } => Some(format!("links out: {}", url.as_deref().unwrap_or("its vendor's site"))),
        Action::Manual { reason: ManualReason::Distribution, .. } => {
            Some(format!("this distribution isn't supported by the guided setup yet: install {} yourself", option.provider.label()))
        }
        // A restart sets up one blocked until it (VirtualBox's driver, built for a newer kernel).
        Action::Blocked { reason } if !action.ends_pass() => Some(format!("can't run here: {reason}")),
        _ => None,
    })
}

/// The hypervisor a setup sets up of `wanted` (none given: the setup's own), and why each other
/// one wanted isn't.
fn pick_hypervisor(o: &Options, wanted: &[Provider]) -> (Option<Provider>, Vec<(Provider, String)>) {
    let order: Vec<Provider> = if wanted.is_empty() { o.hypervisor.into_iter().collect() } else { wanted.to_vec() };
    let mut chosen: Option<Provider> = None;
    let mut passed_over = Vec::new();
    for provider in order {
        let Some(option) = o.hypervisors.iter().find(|h| h.provider == provider) else {
            passed_over.push((provider, "not offered on this machine".to_owned()));
            continue;
        };
        match (chosen, not_settable(option)) {
            (_, Some(why)) => passed_over.push((provider, why)),
            (Some(first), None) => passed_over.push((provider, format!("not needed: {} is set up", first.label()))),
            (None, None) => chosen = Some(provider),
        }
    }
    (chosen, passed_over)
}

/// The whole setup `wanted` on the machine `r` reports (its `setup` options, worked out by the
/// machine check), in the window's order: WSL 2 and the engine, then the hypervisor and what
/// Vagrant needs for it; a restart or a new login last, so everything else is done first.
pub fn plan(r: &SystemReport, options: &Options, wanted: &Wanted) -> Plan {
    let mut plan = Plan::default();
    if wanted.paths.contains(&SetupPath::Docker) {
        let wsl = &options.wsl;
        plan.steps.extend(wsl.iter().map(|a| Step { item: "wsl".into(), action: a.clone() }));
        let engine = wanted.engine.filter(|e| engines_on(r.os).contains(e)).unwrap_or(options.engine);
        let engine_item = format!("engine:{}", engine.id());
        let actions = options.engines.iter().find(|e| e.engine == engine).map(|e| e.actions.clone()).unwrap_or_default();
        for action in actions {
            // Docker Desktop runs on WSL 2: it starts once WSL is ready, after the restart.
            if !wsl.is_empty() && matches!(action, Action::StartEngine { .. }) {
                continue;
            }
            let item =
                if matches!(action, Action::Install { .. } | Action::Relogin) || docker_install(&action) { "docker".into() } else { engine_item.clone() };
            plan.steps.push(Step { item, action });
        }
        if r.os == "windows" {
            plan.items.push("wsl".into());
        }
        plan.items.extend(["docker".to_owned(), engine_item]);
    }
    if wanted.paths.contains(&SetupPath::Vm) {
        let (chosen, passed_over) = pick_hypervisor(options, &wanted.hypervisors);
        plan.hypervisor = chosen;
        plan.passed_over = passed_over;
        if let Some(p) = chosen.and_then(|c| local(r).find(|p| p.provider == c)) {
            let option = options.hypervisors.iter().find(|h| h.provider == p.provider);
            plan.steps.extend(option.into_iter().flat_map(|o| o.get.iter().map(|a| Step { item: p.provider.id().into(), action: a.clone() })));
            plan.steps.extend(vagrant_steps(r, p).into_iter().map(|(item, action)| Step { item, action }));
            plan.items.extend(["vagrant".to_owned(), p.provider.id().to_owned()]);
            plan.items.extend(p.plugin.iter().map(|name| format!("plugin:{name}")));
            if p.utility.is_some() {
                plan.items.push("vmware-utility".into());
            }
        }
    }
    // What ends a pass goes last, in its order.
    let (ends, rest): (Vec<Step>, Vec<Step>) = plan.steps.into_iter().partition(|s| s.action.ends_pass());
    plan.steps = rest.into_iter().chain(ends).collect();
    plan
}

/// Whether `action` is Docker's own install left to the player (a distribution without apt).
fn docker_install(action: &Action) -> bool {
    matches!(action, Action::Manual { reason: ManualReason::Distribution, .. })
}
