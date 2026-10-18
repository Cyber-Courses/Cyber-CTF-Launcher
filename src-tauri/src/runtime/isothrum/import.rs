//! Labs the launcher started on its own Terraform path, handed to the Isothrum runner once it is
//! the lab engine (risk R17): a player's running Proxmox or cloud lab stays destroyable by the
//! runner's `down` and its reaper, one code path, after the switch.
//!
//! Each deployment (`<app data>/deployments/<lab id>/<target>/`, a state that still holds
//! resources) becomes run `cyberctf-<lab id>` of the manifest its lab runs from now (generated
//! first when the lab's files predate it), through [`Runner::adopt`]: the state moves into the
//! run's folder, and the record keeps the params it was applied with ([`record`]), the account or
//! server it is on and when it stops itself. An `imported` marker beside the old folder names the
//! run; with no state left there, nothing is imported twice.
//!
//! Only what the runner runs is handed over (the caller says which). A deployment it can't take
//! (its lab gone, the runner refusing) stays where it was, said once per new reason
//! (`import-error`), and the launcher's own path keeps stopping and reaping it
//! ([`left_on_launcher`]); another deployment of a lab the runner already runs stays there too.

mod record;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use isothrum_runner::isoloom_manifest::SetBy;
use isothrum_runner::{Adoption, RunName, RunState, Runner, load_manifest, rfc3339};

use crate::error::Error;
use crate::runtime::paths::{deployed_ids, deployment_states, lab_dir_in, recorded_host, recorded_runtime};
use crate::runtime::providers::Provider;
use crate::runtime::{Runtime, docker, lab, server, terraform};

use record::OldRecord;

/// Beside a state the runner took over: the run it became.
const IMPORTED: &str = "imported";

/// Beside a state the runner couldn't take over: why, the last time (`{kind, message}`).
const IMPORT_ERROR: &str = "import-error";

/// The old run record beside a state.
const RUN_FILE: &str = "run.json";

/// Beside a state, what the run's own folder takes with it: a cloud-services lab's launch values
/// (`inputs.tfvars.json`), which its destroy reads again.
const CARRIED: [&str; 1] = ["inputs.tfvars.json"];

/// The slot the launcher's own Proxmox labs were applied in (the module's default).
const PROXMOX_SLOT: u8 = 1;

/// One Terraform deployment of the launcher's own path: lab `id`'s state folder on a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Deployment {
    /// The lab.
    pub(crate) id: String,
    /// The server or cloud it is on.
    pub(crate) provider: Provider,
    /// What it runs (its `runtime` marker).
    pub(crate) runtime: Runtime,
    /// Its state folder.
    pub(crate) state: PathBuf,
}

/// Why a deployment is left as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Left {
    /// Its state holds nothing to destroy (none, empty or unreadable).
    Empty,
    /// The runner took it over already.
    Imported,
    /// The runner doesn't run labs of its kind.
    NotRunners,
    /// Another operation holds its lab: the next pass tries again.
    Busy,
    /// The runner runs another deployment of its lab.
    RunExists,
}

/// Why a deployment couldn't be handed over.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ImportError {
    /// Its lab isn't installed any more: no manifest can be had for it.
    #[error("its lab isn't installed any more ({0})")]
    LabGone(#[source] Error),
    /// Its lab's files couldn't be generated.
    #[error("its lab's files couldn't be generated ({0})")]
    Generate(#[source] Error),
    /// Its lab generates no run manifest for where it runs.
    #[error("its lab has no run manifest for {0}")]
    NoManifest(String),
    /// The runner refused it, or couldn't record it.
    #[error("the runner refused it ({0})")]
    Runner(#[source] isothrum_runner::Error),
    /// A file beside it couldn't be moved or written.
    #[error("a file beside it couldn't be moved ({0})")]
    Io(#[from] std::io::Error),
}

impl ImportError {
    /// What kind of failure it is: the same kind again is not news to the player.
    fn kind(&self) -> &'static str {
        match self {
            ImportError::LabGone(_) => "lab-gone",
            ImportError::Generate(_) => "generate",
            ImportError::NoManifest(_) => "no-manifest",
            ImportError::Runner(_) => "runner",
            ImportError::Io(_) => "io",
        }
    }
}

/// What became of one deployment.
#[derive(Debug)]
pub(crate) enum Outcome {
    /// The runner took it over.
    Imported,
    /// Left as it is.
    Left(Left),
    /// Couldn't be handed over: it stays on the launcher's path. `new` when the reason isn't the
    /// one recorded the last time (the player is told once).
    Failed { error: ImportError, new: bool },
}

/// Every deployment under the app data folder `base`, each lab's current one (on the host its
/// lab is marked for) first, so another of the same lab never takes its run.
pub(crate) fn deployments(base: &Path) -> Vec<Deployment> {
    let mut out = Vec::new();
    for id in deployed_ids(base) {
        let marked = lab_dir_in(base, &id).ok().and_then(|dir| server::marked_host(&dir));
        let mut found: Vec<Deployment> = deployment_states(base, &id)
            .into_iter()
            .filter_map(|(target, state)| {
                let provider = server::terraform_provider(&target)?;
                Some(Deployment { id: id.clone(), provider, runtime: recorded_runtime(&state), state })
            })
            .collect();
        found.sort_by_key(|d| recorded_host(&d.state).is_none_or(|h| marked.as_deref() != Some(h.as_str())));
        out.extend(found);
    }
    out
}

/// The runner of this host whose home is `home` (the app data folder), to hand deployments to.
pub(crate) fn runner(home: &Path) -> crate::error::Result<Runner> {
    Runner::new(super::host_config(home)).map_err(super::errors::from_runner)
}

/// Hands every deployment under the app data folder `base` (also the runner's home) that the
/// runner runs (`runs`) over to `runner`, each under its lab's lock (`lock`, `None` while
/// another operation holds the lab).
pub(crate) async fn import_all<G>(
    runner: &Runner,
    base: &Path,
    runs: impl Fn(Provider, Runtime) -> bool,
    lock: impl Fn(&str) -> Option<G>,
) -> Vec<(Deployment, Outcome)> {
    let mut out = Vec::new();
    for d in deployments(base) {
        let outcome = if runs(d.provider, d.runtime) {
            match lock(&d.id) {
                Some(_held) => settle(&d.state, import_one(runner, base, &d).await),
                None => Outcome::Left(Left::Busy),
            }
        } else {
            Outcome::Left(Left::NotRunners)
        };
        out.push((d, outcome));
    }
    out
}

/// Takes deployment `d` over, if there is anything to take.
async fn import_one(runner: &Runner, base: &Path, d: &Deployment) -> Result<Outcome, ImportError> {
    if !terraform::deployed(&d.state) {
        let imported = d.state.join(IMPORTED).is_file();
        return Ok(Outcome::Left(if imported { Left::Imported } else { Left::Empty }));
    }
    let name = RunName::new(&docker::project(&d.id)).map_err(ImportError::Runner)?;
    if runner.runs().map_err(ImportError::Runner)?.iter().any(|r| r.name == name && r.state != RunState::Absent) {
        return Ok(Outcome::Left(Left::RunExists));
    }
    let manifest = manifest_of(base, d)?;
    let takes = embedder_params(&manifest)?;
    runner.adopt(&manifest, adoption(d, name.clone(), &takes)).await.map_err(ImportError::Runner)?;
    // Both folders are in the app data folder: a rename moves the file.
    let run_dir = base.join("runs").join(name.as_str());
    for carried in CARRIED.iter().filter(|f| d.state.join(f).is_file()) {
        std::fs::rename(d.state.join(carried), run_dir.join(carried))?;
    }
    std::fs::write(d.state.join(IMPORTED), name.as_str())?;
    Ok(Outcome::Imported)
}

/// The run manifest `d`'s lab runs from where `d` is, its files generated first when they
/// predate it.
fn manifest_of(base: &Path, d: &Deployment) -> Result<PathBuf, ImportError> {
    let dir = lab_dir_in(base, &d.id).map_err(ImportError::LabGone)?;
    let tf = server::terraform_target(d.provider).unwrap_or_default();
    let (module, target) = lab::terraform(&dir, d.runtime, tf).map_err(ImportError::Generate)?;
    let manifest = module.join("run.json");
    if !manifest.is_file() {
        lab::prepare(&dir, target).map_err(ImportError::Generate)?;
    }
    if manifest.is_file() { Ok(manifest) } else { Err(ImportError::NoManifest(format!("{} on {}", target.id(), d.provider.id()))) }
}

/// The names of the params of the manifest at `path` that the embedder sets.
fn embedder_params(path: &Path) -> Result<BTreeSet<String>, ImportError> {
    let loaded = load_manifest(path).map_err(ImportError::Runner)?;
    Ok(loaded.manifest.params.iter().filter(|p| p.set_by == SetBy::Embedder).map(|p| p.name.as_str().to_owned()).collect())
}

/// What the runner is told of deployment `d` (run `name`, its manifest taking the params
/// `takes`): its params, its host, when it stops itself and when it was applied.
fn adoption(d: &Deployment, name: RunName, takes: &BTreeSet<String>) -> Adoption {
    let old = OldRecord::parse(&std::fs::read_to_string(d.state.join(RUN_FILE)).unwrap_or_default());
    let mut a = Adoption::new(name, &d.state).with_params(old.params(takes));
    if let Some(host) = recorded_host(&d.state) {
        a = a.with_host_label(host);
    }
    if let Some(at) = old.expires_at() {
        a = a.with_expires_at(at);
    }
    if let Ok(applied) = std::fs::metadata(d.state.join(RUN_FILE)).and_then(|m| m.modified()) {
        a = a.with_started_at(rfc3339(applied));
    }
    if d.provider == Provider::Proxmox { a.with_slot(PROXMOX_SLOT) } else { a }
}

/// The outcome of an import of the deployment in `state`: a failure is recorded beside it (and
/// is new when its kind isn't the one recorded the last time); a success forgets the last one.
fn settle(state: &Path, imported: Result<Outcome, ImportError>) -> Outcome {
    let file = state.join(IMPORT_ERROR);
    match imported {
        Ok(outcome) => {
            if matches!(outcome, Outcome::Imported) {
                let _ = std::fs::remove_file(&file);
            }
            outcome
        }
        Err(error) => {
            let last: Option<serde_json::Value> = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok());
            let new = last.is_none_or(|l| l["kind"].as_str() != Some(error.kind()));
            let record = serde_json::json!({ "kind": error.kind(), "message": error.to_string() });
            if let Err(e) = std::fs::write(&file, record.to_string()) {
                log::warn!("couldn't record why {} wasn't handed to the runner: {e}", state.display());
            }
            Outcome::Failed { error, new }
        }
    }
}

#[cfg(test)]
mod tests;
