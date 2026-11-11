//! Launching a lab: startLab (CyberBackend) -> download the lab at its pinned
//! commit from GitHub -> run it with `CTF_API_URL` / `CTF_LAUNCH_TOKEN`, which the
//! lab's `evidence` service exchanges for this player's evidence.

use std::path::{Path, PathBuf};

use crate::platform::AppDirs;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::account::api;
use crate::error::{Error, Result};
use crate::runtime::{self, Runtime, providers::Provider};

mod archive;
pub(crate) mod ctf;
pub(crate) mod imported;
mod launch_env;
mod offline;
pub(crate) mod remove;

use archive::extract;
use launch_env::{EnvVar, launch_env};

/// A lab to download and run, as the backend's `startLab` and `claimLaunch` answer it (and as a
/// launch's job file carries it to its worker). Its `Debug` leaves the values out: they carry the
/// launch token.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Launch {
    /// The lab's id.
    pub(crate) lab_id: String,
    /// The runtime the lab declares.
    pub(crate) runtime: Runtime,
    /// The CyberCTF repository the lab comes from.
    pub(crate) repository: String,
    /// The commit of `repository` to install.
    pub(crate) commit: String,
    env: Vec<EnvVar>,
}

impl std::fmt::Debug for Launch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Launch")
            .field("lab_id", &self.lab_id)
            .field("runtime", &self.runtime)
            .field("repository", &self.repository)
            .field("commit", &self.commit)
            .field("env", &format_args!("<redacted>"))
            .finish()
    }
}

/// A lab release to install: the lab, the CyberCTF repository and commit it comes from, and the
/// runtime it declares (what a parked copy of it is taken down in before it is replaced).
#[derive(Clone, Copy)]
struct Release<'a> {
    lab_id: &'a str,
    repository: &'a str,
    commit: &'a str,
    runtime: Runtime,
}

/// A catalogue lab's source: `repository` at `commit`, fetched only when [`validate_source`] allows
/// it (a CyberCTF repository at a full SHA).
pub(crate) fn source<'a>(repository: &'a str, commit: &'a str) -> crate::runtime::lab::Source<'a> {
    crate::runtime::lab::Source { repository, commit, check: validate_source }
}

/// Only CyberCTF repositories at a full commit SHA are downloaded; the API
/// enforces the same rule when labs are published.
pub(crate) fn validate_source(repository: &str, commit: &str) -> Result<()> {
    let repo_ok = repository.strip_prefix("CyberCTF/").is_some_and(|name| {
        !name.is_empty() && name.len() <= 100 && !name.contains("..") && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    });
    let commit_ok = commit.len() == 40 && commit.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
    if repo_ok && commit_ok { Ok(()) } else { Err(Error::Invalid(format!("refusing to download {repository}@{commit}"))) }
}

/// The lab's GitHub tarball at `commit`.
async fn download(repository: &str, commit: &str) -> Result<Vec<u8>> {
    fetch_archive(&format!("https://codeload.github.com/{repository}/tar.gz/{commit}"), crate::platform::http::MAX_LAB_ARCHIVE).await
}

/// A lab archive at `url`, streamed with timeouts (a stalled transfer is given up) and refused
/// past `max` bytes. It was read whole into memory with no limit and no timeout: a stalled
/// GitHub transfer hung the launch forever.
async fn fetch_archive(url: &str, max: u64) -> Result<Vec<u8>> {
    let res = crate::platform::http::download_client()?.get(url).send().await.map_err(|e| Error::Invalid(format!("download failed: {e}")))?;
    if !res.status().is_success() {
        return Err(Error::Invalid(format!("download failed: HTTP {}", res.status())));
    }
    crate::platform::http::body_limited(res, max).await
}

/// Downloads the lab into `<app data>/labs/<lab id>/` unless that exact commit is
/// already installed. Extracts next to it, then swaps, so a failed download
/// never leaves a half-installed lab.
async fn install(dirs: &AppDirs, release: Release<'_>, log: &impl Fn(String)) -> Result<PathBuf> {
    install_with(dirs, release, log, || download(release.repository, release.commit)).await
}

/// [`install`], the archive fetched by `fetch` (when the commit isn't installed yet).
async fn install_with(dirs: &AppDirs, release: Release<'_>, log: &impl Fn(String), fetch: impl AsyncFnOnce() -> Result<Vec<u8>>) -> Result<PathBuf> {
    let Release { lab_id, repository, commit, runtime: declared } = release;
    // The lab id comes from the backend and is used to build paths (join, remove_dir_all, rename).
    // Validate it like every other id so a hostile/buggy response can't escape the labs dir with
    // `..` or an absolute path. Reachable unattended through the agent, so this is the guard.
    runtime::validate_id(lab_id)?;
    validate_source(repository, commit)?;
    let slot = Slot::of(dirs, lab_id).await?;
    if runtime::lab::installed_at(&slot.dir, commit) {
        runtime::lab::ensure_instance(&slot.labs, &slot.dir, &slot.in_use)?;
        return Ok(slot.dir);
    }
    // A new version of a lab that is up, here or elsewhere: swapping the folder would pull the
    // files its run reads (its manifest, what its containers bind-mount) out from under it. Stop
    // it first.
    runtime::refuse_replace(dirs, &slot.dir, lab_id)?;
    log(runtime::deploy_log::downloading(repository, &commit[..12]));
    let bytes = fetch().await?;
    let (labs, into, id, sha) = (slot.labs.clone(), slot.dir.clone(), lab_id.to_owned(), commit.to_owned());
    let staged = move || stage(&labs, &into, &bytes, &id, &sha);
    install_into(dirs, &slot, (lab_id, declared), log, staged).await?;
    log(runtime::deploy_log::INSTALLED.into());
    Ok(slot.dir)
}

/// Where an install puts a lab: the labs folder, the lab's folder there, and the address blocks
/// Docker's networks hold now (so a new lab's instance lands on blocks nothing else holds).
struct Slot {
    labs: PathBuf,
    dir: PathBuf,
    in_use: Vec<String>,
}

impl Slot {
    /// Lab `lab_id`'s slot.
    async fn of(dirs: &AppDirs, lab_id: &str) -> Result<Slot> {
        let labs = crate::runtime::paths::labs_root(dirs)?;
        let dir = labs.join(lab_id);
        Ok(Slot { labs, dir, in_use: runtime::subnets_in_use(lab_id).await })
    }
}

/// The steps every install shares, once the caller has refused a replacement of a lab that is up:
/// a parked copy of the lab (`id`, declared `runtime`) is taken down, the folder `fill` stages
/// (seconds of file work, on a blocking thread) swapped in all or nothing, and the lab given its
/// own room on this machine (networks, names), so it runs beside the other labs.
async fn install_into(
    dirs: &AppDirs,
    slot: &Slot,
    (id, runtime): (&str, Runtime),
    log: &impl Fn(String),
    fill: impl FnOnce() -> Result<PathBuf> + Send + 'static,
) -> Result<()> {
    // A parked copy of the previous version would be orphaned once its folder (and the runner's
    // record of it) is gone: take it down first, before its markers are carried over.
    clear_parked(dirs, runtime::LabRef { dir: &slot.dir, id, runtime }, log).await?;
    let into = slot.dir.clone();
    blocking(move || swap_in(&fill()?, &into)).await?;
    runtime::lab::ensure_instance(&slot.labs, &slot.dir, &slot.in_use)?;
    Ok(())
}

/// Takes down a parked copy of `lab` before its folder is replaced ([`runtime::clear_parked`]);
/// when that fails the replacement is refused, the parked copy left as it was.
async fn clear_parked(dirs: &AppDirs, lab: runtime::LabRef<'_>, log: &impl Fn(String)) -> Result<()> {
    runtime::clear_parked(dirs, lab, &mut |l| log(l)).await.map_err(|e| {
        Error::Refused(format!("A parked copy of this lab is here and couldn't be removed ({e}). Resume it and stop it, then start the lab again."))
    })
}

/// The folder an install of lab `lab_id` fills under `labs` before it is swapped in: one per lab,
/// which two installs of it share (they hold the lab's lock).
fn staging_dir(labs: &Path, lab_id: &str) -> PathBuf {
    labs.join(format!(".{lab_id}.staging"))
}

/// Extracts the downloaded lab `lab_id` at `commit` into its staging folder, with its commit
/// marker and the installed copy (`dir`)'s instance number, which stays the installed lab's.
/// Nothing of where the old copy ran comes along: the install refused a lab still up and took a
/// parked one down first, so such a marker could only be stale. Returns the staging folder.
fn stage(labs: &Path, dir: &Path, tarball: &[u8], lab_id: &str, commit: &str) -> Result<PathBuf> {
    let staging = staging_dir(labs, lab_id);
    clear_leftover(&staging);
    std::fs::create_dir_all(&staging)?;
    extract(tarball, &staging)?;
    std::fs::write(staging.join(runtime::lab::COMMIT_MARKER), commit)?;
    if let Ok(value) = std::fs::read(dir.join(runtime::lab::INSTANCE_MARKER)) {
        std::fs::write(staging.join(runtime::lab::INSTANCE_MARKER), value)?;
    }
    Ok(staging)
}

/// Replaces the installed lab with the staged one, all or nothing: the old one is moved aside
/// (a rename, which Windows refuses whole while a file inside is open, where deleting it would
/// stop halfway and leave a broken lab), the new one moved in, and the old one put back if
/// that fails. The old copy is deleted last, best effort; a leftover goes on the next update.
fn swap_in(staging: &Path, dir: &Path) -> Result<()> {
    swap_in_with(staging, dir, rename_retrying)
}

/// [`swap_in`], moving folders with `rename`. Should the old copy not go back after the new
/// one failed to move in, the lab is in neither place the launcher looks: that is logged, and
/// the error says where the old copy is.
fn swap_in_with(staging: &Path, dir: &Path, rename: impl Fn(&Path, &Path) -> std::io::Result<()>) -> Result<()> {
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let old = dir.with_file_name(format!(".{name}.old"));
    clear_leftover(&old);
    let had_old = dir.exists();
    if had_old {
        rename(dir, &old).map_err(|e| in_use(e, dir))?;
    }
    if let Err(e) = rename(staging, dir) {
        if had_old && let Err(back) = rename(&old, dir) {
            log::error!("lab update: {} didn't move in ({e}), and the installed copy didn't go back from {} ({back})", dir.display(), old.display());
            return Err(Error::Invalid(format!(
                "Couldn't update the lab, nor put the installed copy back: it is in {}. Move that folder back to {}, or remove the lab and add it again.",
                old.display(),
                dir.display()
            )));
        }
        return Err(in_use(e, dir));
    }
    clear_leftover(&old);
    Ok(())
}

/// Removes `path`, a staging folder or an old copy an earlier install may have left. Best effort:
/// one that won't go is logged, and tried again by the next install.
fn clear_leftover(path: &Path) {
    if let Err(e) = crate::platform::fs::remove_dir_if_present(path) {
        log::warn!("couldn't remove {}: {e}", path.display());
    }
}

/// Runs `f`, file work that can take seconds (unpacking an archive, copying a folder, a rename
/// retried while a file is held), on a blocking thread, off the async workers.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| Error::Invalid(format!("installing the lab's files: {e}")))?
}

/// `rename`, retried briefly: on Windows an antivirus or the search indexer holds a just-written
/// file open for a moment, and the rename fails with "access denied" meanwhile.
fn rename_retrying(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut last = None;
    for wait_ms in [0, 100, 250, 500, 1000] {
        std::thread::sleep(std::time::Duration::from_millis(wait_ms));
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("rename failed")))
}

/// A rename refused because something holds a file in the lab folder, said as what to do.
fn in_use(e: std::io::Error, dir: &Path) -> Error {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        Error::Invalid(format!(
            "Couldn't update the lab: a file in {} is open in another program (an editor, a terminal, or a VM). Close it and try again.",
            dir.display()
        ))
    } else {
        Error::Io(e)
    }
}

/// A `startLab` or `claimLaunch` answer.
pub(crate) fn parse_launch(launch_json: serde_json::Value) -> Result<Launch> {
    serde_json::from_value(launch_json).map_err(|e| Error::Invalid(format!("invalid launch spec: {e}")))
}

/// Installs and starts a lab from a launch spec (the `startLab`/`claimLaunch` answer). Shared by
/// the manual launch command and the agent's claim loop (bring your own compute).
/// It runs where `to` says.
pub(crate) async fn run(dirs: &AppDirs, launch: Launch, to: runtime::Destination<'_>, log: impl Fn(String)) -> Result<Option<String>> {
    runtime::validate_id(&launch.lab_id)?;
    // Nowhere to start it, or a start the policy turns off: said before the download.
    runtime::check_destination(dirs, launch.runtime, &to)?;
    // The lab is this launch's from the download on: two installs of one lab share its staging
    // folder, and a stop must not run while the new version is swapped in. Shared with the
    // deploy worker's own hold on it (same process), so a worker runs on.
    let _lock = runtime::lock_lab(dirs, &launch.lab_id).await?;
    let release = Release { lab_id: &launch.lab_id, repository: &launch.repository, commit: &launch.commit, runtime: launch.runtime };
    let dir = install(dirs, release, &log).await?;
    // Fixed names for every lab; the evidence itself is never in the environment.
    let env = launch_env(launch.env);
    let lab = runtime::LabRef { dir: &dir, id: &launch.lab_id, runtime: launch.runtime };
    start_recorded(dirs, lab, to, &env, log).await?;
    // Where the lab's target is reachable on this machine, for the website to open.
    Ok(runtime::primary_url(&dir, &launch.lab_id, launch.runtime).await)
}

/// Stops lab `id` wherever it runs ([`runtime::stop_lab`]), first ending the deploy still running
/// in its worker, if any (`end_workers`: the app shell runs the workers), and the website's
/// co-location check of it.
pub(crate) async fn stop_lab(dirs: &AppDirs, id: &str, runtime: Runtime, end_workers: impl Future<Output = ()>, log: impl FnMut(String)) -> Result<()> {
    let end_first = async {
        end_workers.await;
        crate::account::colocation::retire(id);
    };
    runtime::stop_lab(dirs, id, runtime, end_first, log).await
}

/// The backend's operation starting a lab for the signed-in player: what to download and run,
/// and the launch's values (the evidence claim, the player's flags).
const START_LAB: &str = "mutation ($id: ID!) { startLab(labId: $id) { labId runtime repository commit env { name value } } }";

/// The backend's `startLab` answer for `lab_id`: what to download and run, and the launch's values.
async fn start_lab(lab_id: &str) -> Result<serde_json::Value> {
    let data = api::graphql(START_LAB, json!({ "id": lab_id }), true).await?;
    Ok(data["startLab"].clone())
}

/// The values a launch of `lab_id` gives the lab, as the backend at `base` answers player
/// `token`'s `startLab` (no extras): for the integration tests, against a local test backend.
#[cfg(test)]
pub(crate) async fn launch_values_at(base: &str, token: &str, lab_id: &str) -> Result<Vec<(String, String)>> {
    let data = api::send(base, Some(token.to_owned()), START_LAB, json!({ "id": lab_id })).await?;
    Ok(launch_env(parse_launch(data["startLab"].clone())?.env))
}

/// The launch spec for `lab_id`: the backend's answer (whose source is kept for a later offline
/// start), or, when the backend can't be reached and the player asked for it (`offline`), the
/// installed lab's (see `offline`).
async fn launch_spec(dirs: &AppDirs, lab_id: &str, offline: bool, log: &impl Fn(String)) -> Result<Launch> {
    let base = dirs.data()?;
    let user = crate::account::auth::signed_in_user().await.unwrap_or_else(|e| {
        log::warn!("the session can't be read, so {lab_id}'s launch isn't kept for an offline start: {e}");
        None
    });
    match start_lab(lab_id).await.and_then(parse_launch) {
        Ok(answer) => {
            if let Some(user) = &user
                && let Err(e) = offline::keep(base, &answer, user)
            {
                log::warn!("couldn't keep {lab_id}'s launch for an offline start: {e}");
            }
            Ok(answer)
        }
        Err(failed) => {
            let spec = offline::instead_of(failed, base, lab_id, user.as_deref(), offline)?;
            log(offline::STARTING_OFFLINE.into());
            Ok(spec)
        }
    }
}

/// What the player asked to start, and how.
pub struct LaunchRequest {
    /// The lab.
    pub lab_id: String,
    /// The Vagrant provider a VM lab runs on (this machine's default when None).
    pub provider: Option<Provider>,
    /// The saved server or cloud account it runs on (this machine when None).
    pub host: Option<String>,
    /// A container lab's services published on their own ports instead of random free ones.
    pub default_ports: bool,
    /// The installed lab started without the backend when it can't be reached (no progress
    /// saved): the player's answer to a launch that failed with `Code::OfflineReady`.
    pub offline: bool,
    /// A container lab whose spec has VM recipes too, run on this machine as one VM per machine
    /// (the form of a VM lab) on `provider`, instead of as containers.
    pub per_machine_vm: bool,
    /// The container engine a container lab here runs on, when the player chose one other than the
    /// current context (`docker-desktop`, `orbstack`, …). Resolved to its Docker context at launch.
    pub docker_engine: Option<String>,
}

/// A launch ready to run: the lab, its `startLab` spec, and where and how it runs (the Docker
/// context a container lab here is pinned to resolved from the chosen engine).
pub(crate) struct PreparedLaunch {
    /// The lab's id.
    pub(crate) lab_id: String,
    /// Its `startLab` spec.
    pub(crate) launch: Launch,
    /// Where it runs and how.
    pub(crate) place: runtime::Placement,
}

/// Prepares `request`'s launch: resolves the `startLab` spec (the backend's, kept for a later
/// offline start, or the installed lab's when the backend can't be reached and the player asked)
/// and the engine it runs on. The deploy itself runs in a detached worker, started by the command
/// layer (so quitting the app never cuts a run short); `log` carries the one "starting offline"
/// line the preparation may emit.
pub(crate) async fn prepare_launch(dirs: &AppDirs, request: LaunchRequest, log: &impl Fn(String)) -> Result<PreparedLaunch> {
    let LaunchRequest { lab_id, provider, host, default_ports, offline, per_machine_vm, docker_engine } = request;
    let docker_context = pinned_context(docker_engine.as_deref(), host.as_deref()).await?;
    let launch = launch_spec(dirs, &lab_id, offline, log).await?;
    Ok(PreparedLaunch { lab_id, launch, place: runtime::Placement { provider, host, per_machine_vm, docker_context, default_ports } })
}

/// The Docker context a container lab here is pinned to, resolved now (on the main process, where
/// Docker answers) from the engine the player chose (`engine`); none without one, or on a server
/// (`host`). An engine with no context here is an error: starting on the current context would
/// run the lab on another engine than the one picked.
async fn pinned_context(engine: Option<&str>, host: Option<&str>) -> Result<Option<String>> {
    match (engine, host) {
        (Some(engine), None) => crate::machine::system::docker::engine_context(engine).await.map(Some),
        _ => Ok(None),
    }
}

/// A stable lab id for a lab imported from the local directory `src`: `local-<slug>` of the lab's
/// own name (its folder name when the spec has none), so the ordinary lifecycle commands act on it
/// by id. Errors when `src` is not a lab folder.
pub(crate) fn local_id(src: &Path) -> Result<String> {
    let spec = runtime::lab::load(src).map_err(|e| Error::Invalid(format!("not a lab folder ({}): {e}", src.display())))?;
    let base = if spec.name.is_empty() { src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() } else { spec.name };
    let slug: String = base.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c.to_ascii_lowercase() } else { '-' }).collect();
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "lab" } else { slug };
    let id = format!("{}{}", imported::PREFIX, &slug[..slug.len().min(56)]);
    runtime::validate_id(&id)?;
    Ok(id)
}

/// Installs the lab from the local directory `src` into `<app data>/labs/<lab id>/` by copying it
/// (no download): an imported lab. The rest is the ordinary install — its own instance room, any
/// parked copy cleared first — so the lifecycle commands then drive it like any installed lab.
async fn install_local(dirs: &AppDirs, lab_id: &str, src: &Path, log: &impl Fn(String)) -> Result<PathBuf> {
    runtime::validate_id(lab_id)?;
    // The folder is read again here: the worker that runs this may start well after the import
    // was asked for (`local_id`), and the folder may have changed since.
    runtime::lab::load(src).map_err(|e| Error::Invalid(format!("not a lab folder ({}): {e}", src.display())))?;
    let slot = Slot::of(dirs, lab_id).await?;
    // Re-importing while the previous copy is up would pull its files out from under it, as an
    // update would. Stop it first.
    runtime::refuse_replace(dirs, &slot.dir, lab_id)?;
    log(runtime::deploy_log::importing(lab_id, src));
    let (from, staging) = (src.to_path_buf(), staging_dir(&slot.labs, lab_id));
    let copied = move || {
        clear_leftover(&staging);
        runtime::lab::copy_tree(&from, &staging)?;
        Ok(staging)
    };
    install_into(dirs, &slot, (lab_id, Runtime::Docker), log, copied).await?;
    log(runtime::deploy_log::IMPORTED.into());
    Ok(slot.dir)
}

/// Installs and starts the lab in the local directory `src` as a Docker lab with development
/// inputs (no backend, no flags): an imported custom lab, for running one not published yet.
/// Returns its local URL.
pub(crate) async fn run_local(dirs: &AppDirs, lab_id: &str, src: &Path, log: impl Fn(String)) -> Result<Option<String>> {
    runtime::validate_id(lab_id)?;
    let _lock = runtime::lock_lab(dirs, lab_id).await?;
    let dir = install_local(dirs, lab_id, src, &log).await?;
    let lab = runtime::LabRef { dir: &dir, id: lab_id, runtime: Runtime::Docker };
    start_recorded(dirs, lab, runtime::Destination::default(), &[], log).await?;
    Ok(runtime::primary_url(&dir, lab_id, Runtime::Docker).await)
}

/// Starts an installed lab ([`runtime::start_locked`]), recording as the app's what the start
/// downloaded to this machine (Storage cleans it later), whether or not the start went through.
async fn start_recorded(
    dirs: &AppDirs,
    lab: runtime::LabRef<'_>,
    to: runtime::Destination<'_>,
    env: &[(String, String)],
    log: impl FnMut(String),
) -> Result<()> {
    let runtime::Downloads { images, boxes } = runtime::downloads(lab.runtime, &to);
    let before = crate::machine::storage::owned::Before::take(images, boxes).await;
    let started = runtime::start_locked(dirs, lab, to, env, log).await;
    crate::machine::storage::claim_lab(dirs.clone(), lab.dir, lab.id, &before).await;
    started
}

/// An update and an import as a launch and the import run them, for the integration tests.
#[cfg(test)]
pub(crate) mod for_tests {
    use std::path::{Path, PathBuf};

    use crate::error::Result;
    use crate::platform::AppDirs;
    use crate::runtime::Runtime;

    /// Lab `lab_id`, declared `runtime`, updated to `commit`, whose GitHub tarball is `tarball`.
    pub(crate) async fn update(dirs: &AppDirs, lab_id: &str, runtime: Runtime, commit: &str, tarball: Vec<u8>) -> Result<PathBuf> {
        let release = super::Release { lab_id, repository: "CyberCTF/integration-test", commit, runtime };
        super::install_with(dirs, release, &|_| {}, async move || Ok(tarball)).await
    }

    /// Lab `lab_id` imported again from the folder `src`.
    pub(crate) async fn reimport(dirs: &AppDirs, lab_id: &str, src: &Path) -> Result<PathBuf> {
        super::install_local(dirs, lab_id, src, &|_| {}).await
    }
}

#[cfg(test)]
mod tests {
    use super::archive::tests::tarball;
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn an_installs_file_work_leaves_the_async_worker_free() {
        // Regression: unpacking an archive and the retried renames ran on the async worker,
        // stalling every task on it for seconds.
        let (release, held) = std::sync::mpsc::channel::<()>();
        let work = tokio::spawn(blocking(move || held.recv_timeout(std::time::Duration::from_secs(5)).map_err(|e| Error::Invalid(e.to_string()))));
        // The only worker runs this while the file work waits on it.
        tokio::spawn(async move { release.send(()) }).await.unwrap().unwrap();
        work.await.unwrap().unwrap();
    }
    #[test]
    fn only_cyberctf_repos_at_full_shas() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert!(validate_source("CyberCTF/invoice-portal-sqli", sha).is_ok());
        for (repo, commit) in [
            ("someone/lab", sha),
            ("CyberCTF/", sha),
            ("CyberCTF/../x", sha),
            ("CyberCTF/a/b", sha),
            ("CyberCTF/lab", "main"),
            ("CyberCTF/lab", &sha.to_uppercase()),
        ] {
            assert!(validate_source(repo, commit).is_err(), "{repo}@{commit}");
        }
    }

    /// A release of a container lab from `CyberCTF/invoice`, as `lab_id` at `commit`.
    fn invoice<'a>(lab_id: &'a str, commit: &'a str) -> Release<'a> {
        Release { lab_id, repository: "CyberCTF/invoice", commit, runtime: Runtime::Docker }
    }

    #[test]
    fn launch_specs_parse_or_say_why_not() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let l = parse_launch(serde_json::json!({
            "labId": "invoice", "runtime": "DOCKER", "repository": "CyberCTF/invoice", "commit": sha,
            "env": [{"name": "CTF_API_URL", "value": "https://api"}]
        }))
        .unwrap();
        assert_eq!(
            (l.lab_id.as_str(), l.runtime, l.repository.as_str(), l.commit.as_str(), l.env.len()),
            ("invoice", Runtime::Docker, "CyberCTF/invoice", sha, 1)
        );
        let err = parse_launch(serde_json::json!({ "labId": "x" })).err().unwrap().to_string();
        assert!(err.starts_with("invalid launch spec: "), "{err}");
    }

    #[test]
    fn staging_carries_the_lab_its_commit_and_its_instance_but_no_stale_place() {
        let labs = std::env::temp_dir().join(format!("cyberctf-stage-{}", rand::random::<u32>()));
        let dir = labs.join("invoice");
        let sha = "0123456789abcdef0123456789abcdef01234567";
        std::fs::create_dir_all(&dir).unwrap();
        // Regression: where the old copy ran was carried over, though nothing of it is up by now
        // (an install refuses a lab still up and takes a parked one down first), so a stale
        // marker sent the new copy's status and stop to a host it never ran on.
        std::fs::write(dir.join(runtime::server::HOST_MARKER), "pve1").unwrap();
        std::fs::write(dir.join(runtime::LOCAL_VM_MARKER), "virtualbox").unwrap();
        std::fs::write(dir.join(runtime::lab::INSTANCE_MARKER), "3").unwrap();
        std::fs::write(dir.join("old.txt"), "old").unwrap();
        // Nothing is installed at that commit yet.
        assert!(!runtime::lab::installed_at(&dir, sha));

        let staging = stage(&labs, &dir, &tarball(&[("lab-abc/isoloom.yml", b"name: invoice")]), "invoice", sha).unwrap();
        assert_eq!(staging, labs.join(".invoice.staging"));
        assert_eq!(std::fs::read_to_string(staging.join("isoloom.yml")).unwrap(), "name: invoice");
        assert!(!staging.join(runtime::server::HOST_MARKER).exists());
        assert!(!staging.join(runtime::LOCAL_VM_MARKER).exists());
        assert_eq!(std::fs::read_to_string(staging.join(runtime::lab::INSTANCE_MARKER)).unwrap(), "3", "the installed lab's room stays");

        swap_in(&staging, &dir).unwrap();
        assert!(!staging.exists());
        assert!(!dir.join("old.txt").exists());
        assert!(runtime::lab::installed_at(&dir, sha));
        assert!(!runtime::lab::installed_at(&dir, "ffffffffffffffffffffffffffffffffffffffff"));
        assert!(!labs.join(".invoice.old").exists(), "the old copy is cleared");
        std::fs::remove_dir_all(labs).unwrap();
    }

    #[test]
    fn a_failed_update_that_cant_put_the_old_copy_back_says_where_it_is() {
        let labs = std::env::temp_dir().join(format!("cyberctf-swap-back-{}", rand::random::<u32>()));
        let (dir, staging, old) = (labs.join("invoice"), labs.join(".invoice.staging"), labs.join(".invoice.old"));
        for (d, v) in [(&dir, "old"), (&staging, "new")] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join("a.txt"), v).unwrap();
        }
        // Moving aside works; moving the new copy in, and the old one back, don't.
        let rename = |from: &Path, to: &Path| if from == dir.as_path() { std::fs::rename(from, to) } else { Err(std::io::Error::other("held")) };
        let err = swap_in_with(&staging, &dir, rename).unwrap_err().to_string();
        assert!(err.contains("nor put the installed copy back") && err.contains(&old.display().to_string()), "{err}");
        assert_eq!(std::fs::read_to_string(old.join("a.txt")).unwrap(), "old", "the old copy kept where the error says");
        // The old copy back fine: the move-in's own error, the lab where it was.
        std::fs::rename(&old, &dir).unwrap();
        let rename = |from: &Path, to: &Path| if from == staging.as_path() { Err(std::io::Error::other("held")) } else { std::fs::rename(from, to) };
        let err = swap_in_with(&staging, &dir, rename).unwrap_err().to_string();
        assert!(!err.contains("put the installed copy back"), "{err}");
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "old");
        std::fs::remove_dir_all(labs).unwrap();
    }

    /// Windows: a file of the installed lab open in another program (no delete sharing, like an
    /// editor or a VM's disk) used to leave the lab half-deleted. Now the update is refused whole,
    /// the installed lab intact, and it goes through once the file is closed.
    #[cfg(windows)]
    #[test]
    fn an_update_with_a_file_held_open_keeps_the_installed_lab_whole() {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        let labs = std::env::temp_dir().join(format!("cyberctf-swap-{}", rand::random::<u32>()));
        let (dir, staging) = (labs.join("invoice"), labs.join(".invoice.staging"));
        for (d, v) in [(&dir, "old"), (&staging, "new")] {
            std::fs::create_dir_all(d.join("sub")).unwrap();
            std::fs::write(d.join("a.txt"), v).unwrap();
            std::fs::write(d.join("sub").join("disk.vmdk"), v).unwrap();
        }
        let held = std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(dir.join("sub").join("disk.vmdk")).unwrap();

        let err = swap_in(&staging, &dir).unwrap_err().to_string();
        assert!(err.contains("open in another program"), "{err}");
        for f in ["a.txt", "sub/disk.vmdk"] {
            assert_eq!(std::fs::read_to_string(dir.join(f)).unwrap(), "old", "{f}");
            assert_eq!(std::fs::read_to_string(staging.join(f)).unwrap(), "new", "{f}");
        }

        drop(held);
        swap_in(&staging, &dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("sub").join("disk.vmdk")).unwrap(), "new");
        assert!(!staging.exists() && !labs.join(".invoice.old").exists());
        std::fs::remove_dir_all(labs).unwrap();
    }

    const SPEC: &str = "version: 1\nname: invoice\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http } }\n    app: { image: nginx:1.27 }\n";
    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    #[tokio::test]
    async fn a_new_lab_version_is_installed_unless_the_old_one_still_runs() {
        use crate::platform::exec::fake::Tools;
        let t = crate::testing::test_app::TestApp::new();
        let gz = tarball(&[("lab-abc/isoloom.yml", SPEC.as_bytes())]);
        let tools = Tools::rules(&[]);
        let lines = std::sync::Mutex::new(Vec::<String>::new());
        let log = |l: String| lines.lock().unwrap().push(l);
        let dir = install_with(&t.dirs(), invoice("invoice", SHA), &log, async || Ok(gz.clone())).await.unwrap();
        assert_eq!(dir, t.data().join("labs").join("invoice"));
        assert!(runtime::lab::installed_at(&dir, SHA) && dir.join("isoloom.yml").is_file());
        assert_eq!(lines.lock().unwrap().as_slice(), ["Downloading CyberCTF/invoice@0123456789ab", "Lab installed"]);
        // The same commit again: nothing downloaded.
        install_with(&t.dirs(), invoice("invoice", SHA), &log, async || panic!("no download")).await.unwrap();
        drop(tools);
        // A newer commit while the old one is up, wherever the runner runs it: refused, the
        // running copy left alone. Regression: only this machine's Docker and the lab folder's
        // Vagrant record were asked, where the runner keeps nothing, so a VM, docker-in-VM,
        // ESXi, Proxmox or cloud run had its folder swapped out from under it.
        let newer = "1123456789abcdef0123456789abcdef01234567";
        for (host, target) in
            [(None, "docker"), (None, "docker-vm"), (None, "vagrant"), (Some("esx1"), "vagrant"), (Some("pve1"), "proxmox"), (Some("aws1"), "cloud-vm")]
        {
            crate::runtime::test_support::record_run(t.data(), "invoice", host, target, "ready");
            let err = install_with(&t.dirs(), invoice("invoice", newer), &log, async || panic!("no download")).await.unwrap_err();
            assert!(err.is_refusal(), "{target}: {err}");
            let expected = host.map_or_else(|| "This lab is still running on this machine".to_owned(), |h| format!("This lab is still running on {h}."));
            assert!(err.to_string().starts_with(&expected), "{target}: {err}");
            assert!(runtime::lab::installed_at(&dir, SHA), "{target}: the running copy was replaced");
        }
        // Stopped (the run taken down): the new version goes in.
        crate::runtime::test_support::record_run(t.data(), "invoice", None, "docker", "absent");
        install_with(&t.dirs(), invoice("invoice", newer), &log, async || Ok(gz.clone())).await.unwrap();
        assert!(runtime::lab::installed_at(&dir, newer));
        // Ids and sources are checked before anything else.
        assert!(install_with(&t.dirs(), invoice("../x", SHA), &log, async || panic!()).await.is_err());
        assert!(
            install_with(&t.dirs(), Release { lab_id: "invoice", repository: "evil/lab", commit: SHA, runtime: Runtime::Docker }, &log, async || panic!())
                .await
                .is_err()
        );
    }

    /// A parked copy the runner couldn't take down refuses the new version: swapping the folder
    /// would orphan its machines. The parked copy stays as it was.
    #[tokio::test]
    async fn a_new_lab_version_is_refused_while_a_parked_copy_couldnt_be_taken_down() {
        let t = crate::testing::test_app::TestApp::new();
        let gz = tarball(&[("lab-abc/isoloom.yml", SPEC.as_bytes())]);
        let _tools = crate::platform::exec::fake::Tools::rules(&[]);
        let dir = install_with(&t.dirs(), invoice("invoice", SHA), &|_| {}, async || Ok(gz.clone())).await.unwrap();
        crate::runtime::paths::mark_parked(&dir, Some(runtime::Park::Shutdown)).unwrap();
        let manifest = runtime::lab::docker_manifest(&dir);
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        std::fs::write(&manifest, "not a manifest").unwrap();
        let newer = "1123456789abcdef0123456789abcdef01234567";
        let err = install_with(&t.dirs(), invoice("invoice", newer), &|_| {}, async || Ok(gz.clone())).await.unwrap_err();
        assert!(matches!(err, Error::Refused(_)), "{err:?}");
        assert!(err.to_string().starts_with("A parked copy of this lab is here and couldn't be removed"), "{err}");
        assert!(runtime::lab::installed_at(&dir, SHA), "the parked copy's folder was replaced");
        assert!(!t.data().join("labs").join(".invoice.staging").exists());
    }

    #[tokio::test]
    async fn a_launch_waits_for_the_lab_lock_before_installing_anything() {
        // Regression: the install (download, staging, swap) ran before the lock was taken, so
        // two installs of one lab shared its staging folder and a stop could run mid-swap.
        use crate::platform::exec::fake::Tools;
        let t = crate::testing::test_app::TestApp::new();
        let dir = t.lab("invoice", &[("isoloom.yml", SPEC), (runtime::lab::COMMIT_MARKER, SHA)]);
        let tools = Tools::rules(&[]);
        let held = runtime::lock_lab(&t.dirs(), "invoice").await.unwrap();
        let launch =
            parse_launch(serde_json::json!({ "labId": "invoice", "runtime": "DOCKER", "repository": "CyberCTF/invoice", "commit": SHA, "env": [] })).unwrap();
        let waiting =
            tokio::time::timeout(std::time::Duration::from_millis(300), run(&t.dirs(), launch.clone(), runtime::Destination::default(), |_| {})).await;
        assert!(waiting.is_err(), "the launch waits while another operation holds the lab");
        assert!(tools.calls().is_empty(), "nothing of the install ran meanwhile: {:?}", tools.calls());
        assert!(!dir.join(runtime::lab::INSTANCE_MARKER).exists());
        // Once the lock is free the launch may proceed (its install and the runner start are
        // covered by the runner's own tests and the integration tests).
        drop(held);
        let _ = launch;
    }

    #[tokio::test]
    async fn a_chosen_engine_with_no_context_here_fails_the_launch_rather_than_use_another() {
        // Regression: an engine whose context couldn't be found pinned nothing, and the lab
        // started on the current context: another engine than the one the player picked.
        let _tools = crate::platform::exec::fake::Tools::rules(&[("context ls", crate::platform::exec::fake::ok("default *\ncolima-dev\n"))]);
        assert_eq!(pinned_context(Some("colima"), None).await.unwrap().as_deref(), Some("colima-dev"));
        assert_eq!(pinned_context(Some("orbstack"), None).await.unwrap_err().to_string(), "no Docker context for orbstack");
        // On a server, or with no engine chosen, nothing is pinned.
        assert_eq!(pinned_context(Some("orbstack"), Some("pve")).await.unwrap(), None);
        assert_eq!(pinned_context(None, None).await.unwrap(), None);
    }

    #[test]
    fn a_folder_without_a_lab_has_no_local_id() {
        // The lab id of a local import comes from its spec; a folder with no isoloom.yml is not a
        // lab, so `lab_launch_local` (the command) refuses before it ever reaches the runner.
        let dir = std::env::temp_dir().join(format!("cyberctf-notalab-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let err = local_id(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(err.is_err(), "a folder with no isoloom.yml is not a lab");
    }

    #[tokio::test]
    async fn re_importing_a_lab_that_is_up_here_or_elsewhere_is_refused() {
        // Regression: the import asked this machine's Docker, the lab folder's Vagrant record and
        // `deployments/`, where the runner keeps nothing: a copy the runner ran as VMs here, in a
        // local VM or on a server was swapped out from under it.
        use crate::platform::exec::fake::Tools;
        use crate::runtime::test_support::record_run;
        let t = crate::testing::test_app::TestApp::new();
        let src = std::env::temp_dir().join(format!("cyberctf-import-src-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("isoloom.yml"), SPEC).unwrap();
        let _tools = Tools::rules(&[]);
        let log = |_: String| {};
        let dir = install_local(&t.dirs(), "local-invoice", &src, &log).await.unwrap();
        std::fs::write(dir.join("mine"), "kept").unwrap();
        // Up here, as the runner records it (a failed start that left its VM counts too).
        record_run(t.data(), "local-invoice", None, "docker-vm", "failed");
        let err = install_local(&t.dirs(), "local-invoice", &src, &log).await.unwrap_err();
        assert_eq!(err.code(), crate::error::Code::LabRunningHere, "{err:?}");
        assert!(err.to_string().contains("its machines are still there"), "{err}");
        // On a server host: the host is named.
        record_run(t.data(), "local-invoice", Some("esxi1"), "vagrant", "ready");
        let err = install_local(&t.dirs(), "local-invoice", &src, &log).await.unwrap_err();
        assert!(err.is_refusal() && err.to_string().contains("still running on esxi1"), "{err}");
        // Refused before anything was staged: the installed copy is untouched.
        assert!(dir.join("mine").is_file() && !dir.with_file_name(".local-invoice.staging").exists());
        // Stopped: imported again.
        record_run(t.data(), "local-invoice", Some("esxi1"), "vagrant", "absent");
        install_local(&t.dirs(), "local-invoice", &src, &log).await.unwrap();
        assert!(!dir.join("mine").exists());
        let _ = std::fs::remove_dir_all(&src);
    }

    #[tokio::test]
    async fn an_archive_download_is_limited_and_says_why_it_failed() {
        use crate::platform::http::tests::serve_once;
        let gz = tarball(&[("lab-abc/isoloom.yml", b"name: x")]);
        let mut ok = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", gz.len()).into_bytes();
        ok.extend_from_slice(&gz);
        assert_eq!(fetch_archive(&serve_once(ok.clone()).await, 1024 * 1024).await.unwrap(), gz);
        // Past the limit (regression: read whole, unbounded): refused with a clear error.
        let err = fetch_archive(&serve_once(ok).await, 16).await.unwrap_err().to_string();
        assert!(err.starts_with("download refused: it is larger than"), "{err}");
        let missing = serve_once(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()).await;
        assert_eq!(fetch_archive(&missing, 16).await.unwrap_err().to_string(), "download failed: HTTP 404 Not Found");
        // Nothing listening: a connection error, not a hang.
        let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap();
        assert!(fetch_archive(&format!("http://{closed}/"), 16).await.unwrap_err().to_string().starts_with("download failed: "));
    }

    #[test]
    fn repository_names_at_the_length_boundary_and_commit_lengths() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert!(validate_source(&format!("CyberCTF/{}", "a".repeat(100)), sha).is_ok());
        assert!(validate_source(&format!("CyberCTF/{}", "a".repeat(101)), sha).is_err());
        assert!(validate_source("CyberCTF/lab.v2_x-y", sha).is_ok());
        for (repo, commit) in [
            ("CyberCTF/lab", &sha[..39]),
            ("CyberCTF/lab", &format!("{sha}0")),
            ("CyberCTF/lab", &format!(" {}", &sha[1..])),
            ("CyberCTF/lab", "0123456789abcdef0123456789abcdef0123456g"),
            ("CyberCTF/café", sha),
            ("cyberctf/lab", sha),
            ("CyberCTF/lab ", sha),
            ("CyberCTF//lab", sha),
            ("", sha),
            ("CyberCTF/lab", ""),
        ] {
            assert!(validate_source(repo, commit).is_err(), "{repo:?}@{commit:?}");
        }
    }

    #[test]
    fn a_leftover_staging_folder_is_replaced_whole_and_an_installed_marker_is_trimmed() {
        let labs = std::env::temp_dir().join(format!("cyberctf-stage-{}", rand::random::<u32>()));
        let dir = labs.join("lab");
        let staging = labs.join(".lab.staging");
        std::fs::create_dir_all(staging.join("old")).unwrap();
        std::fs::write(staging.join("old/file"), "from a crashed install").unwrap();
        let staged = stage(&labs, &dir, &tarball(&[("lab-abc/isoloom.yml", b"name: lab")]), "lab", SHA).unwrap();
        assert_eq!(staged, staging);
        assert!(!staging.join("old").exists(), "the crashed install's files are gone");
        // The commit marker reads back with surrounding whitespace or CRLF (a hand edit).
        std::fs::write(staging.join(runtime::lab::COMMIT_MARKER), format!("  {SHA}\r\n")).unwrap();
        assert!(runtime::lab::installed_at(&staging, SHA));
        assert!(!runtime::lab::installed_at(&staging, &SHA.to_uppercase()));
        std::fs::remove_dir_all(labs).unwrap();
    }

    #[tokio::test]
    async fn lab_ids_at_the_validation_boundaries_install_or_are_refused() {
        use crate::platform::exec::fake::Tools;
        let t = crate::testing::test_app::TestApp::new();
        let _tools = Tools::rules(&[]);
        let log = |_: String| {};
        let max = "A".repeat(63) + "_";
        let gz = tarball(&[("lab-abc/isoloom.yml", SPEC.as_bytes())]);
        let dir = install_with(&t.dirs(), invoice(&max, SHA), &log, async || Ok(gz.clone())).await.unwrap();
        assert_eq!(dir.file_name().unwrap().to_str().unwrap(), max);
        for bad in ["", &"a".repeat(65), "é", "a b", "a/b", ".", "a.b", "..", "a\u{0}b"] {
            let bad = bad.to_string();
            let err = install_with(&t.dirs(), invoice(&bad, SHA), &log, async || panic!("no download for {bad:?}")).await.unwrap_err().to_string();
            assert!(err.starts_with("invalid lab id"), "{bad:?}: {err}");
        }
        // "-" and "_" alone pass the rule (a folder of that name is harmless).
        let dir = install_with(&t.dirs(), invoice("-", SHA), &log, async || Ok(gz.clone())).await.unwrap();
        assert_eq!(dir, t.data().join("labs").join("-"));
    }
}
