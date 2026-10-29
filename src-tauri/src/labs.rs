//! Launching a lab: startLab (CyberBackend) -> download the lab at its pinned
//! commit from GitHub -> run it with `CTF_API_URL` / `CTF_LAUNCH_TOKEN`, which the
//! lab's `evidence` service exchanges for this player's evidence.

use std::path::{Path, PathBuf};

use crate::AppHandle;
use serde::Deserialize;
use serde_json::json;
use tauri::Manager;
use tauri::ipc::Channel;

use crate::account::api;
use crate::error::{Error, Result};
use crate::runtime::{self, Runtime, providers::Provider};

mod launch_env;
mod offline;
pub mod operations;

use launch_env::{EnvVar, launch_env};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Launch {
    lab_id: String,
    runtime: Runtime,
    repository: String,
    commit: String,
    env: Vec<EnvVar>,
}

/// A lab release to install: the lab, and the CyberCTF repository and commit it comes from.
#[derive(Clone, Copy)]
struct Release<'a> {
    lab_id: &'a str,
    repository: &'a str,
    commit: &'a str,
}

/// Where and how a launched lab runs: VM labs locally with `provider`, or on the server `host`
/// when one is given; the attack box image; and whether a container lab keeps its own ports.
#[derive(Default)]
pub(crate) struct Placement<'a> {
    pub provider: Option<Provider>,
    pub host: Option<&'a str>,
    pub attackbox_image: Option<&'a str>,
    pub default_ports: bool,
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

/// Extracts a GitHub tarball (single top-level `<repo>-<sha>/` folder) into
/// `dest`, rejecting absolute paths, `..` and links.
fn extract(tarball: &[u8], dest: &Path) -> Result<()> {
    extract_within(tarball, dest, crate::platform::http::MAX_LAB_UNPACKED)
}

/// [`extract`], refused before the files would add up to more than `max` bytes.
fn extract_within(tarball: &[u8], dest: &Path, max: u64) -> Result<()> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tarball));
    let mut unpacked: u64 = 0;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            continue;
        }
        unpacked = unpacked.saturating_add(entry.size());
        if unpacked > max {
            return Err(Error::Invalid(format!("the lab archive unpacks to more than {} MB; refused", max / (1024 * 1024))));
        }
        let path = entry.path()?.into_owned();
        // Past a leading `./` (GNU tar's `-C dir .` style), then the top folder GitHub names
        // `<repo>-<sha>`.
        let relative: PathBuf = path.components().skip_while(|c| matches!(c, std::path::Component::CurDir)).skip(1).collect();
        if relative.as_os_str().is_empty() {
            continue;
        }
        if relative.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return Err(Error::Invalid(format!("unsafe path in lab archive: {}", path.display())));
        }
        // Windows can't hold every name git can (a trailing space, `aux.c`): said plainly, not
        // as the file system's "not found" halfway through the lab.
        if cfg!(windows)
            && let Some(why) = relative.components().find_map(|c| windows_name_problem(&c.as_os_str().to_string_lossy()))
        {
            return Err(Error::Invalid(format!("this lab can't be installed on Windows: its archive holds {}, {why}", path.display())));
        }
        let target = dest.join(&relative);
        if kind.is_dir() {
            std::fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            entry.unpack(&target)?;
        }
    }
    Ok(())
}

/// Why Windows can't create a file or folder named `name` (None: it can): a name ending in a
/// space or a dot, a character Windows reserves, or a device name (`CON`, `aux.c`, `LPT1.txt`).
fn windows_name_problem(name: &str) -> Option<&'static str> {
    const DEVICES: &[&str] = &["CON", "PRN", "AUX", "NUL"];
    if name.ends_with([' ', '.']) {
        return Some("a name ending in a space or a dot");
    }
    if name.chars().any(|c| c < ' ' || "<>:\"|?*\\".contains(c)) {
        return Some("a name with a character Windows reserves");
    }
    let stem = name.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    let numbered = |prefix: &str| stem.strip_prefix(prefix).is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit() && n != "0");
    if DEVICES.contains(&stem.as_str()) || numbered("COM") || numbered("LPT") {
        return Some("a device name Windows reserves");
    }
    None
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
async fn install(app: &AppHandle, release: Release<'_>, log: &impl Fn(String)) -> Result<PathBuf> {
    install_with(app, release, log, || download(release.repository, release.commit)).await
}

/// [`install`], the archive fetched by `fetch` (when the commit isn't installed yet).
async fn install_with(app: &AppHandle, release: Release<'_>, log: &impl Fn(String), fetch: impl AsyncFnOnce() -> Result<Vec<u8>>) -> Result<PathBuf> {
    let Release { lab_id, repository, commit } = release;
    // The lab id comes from the backend and is used to build paths (join, remove_dir_all, rename).
    // Validate it like every other id so a hostile/buggy response can't escape the labs dir with
    // `..` or an absolute path. Reachable unattended through the agent, so this is the guard.
    runtime::validate_id(lab_id)?;
    validate_source(repository, commit)?;
    let labs = app.path().app_data_dir().map_err(|e| Error::Invalid(e.to_string()))?.join("labs");
    let dir = labs.join(lab_id);
    let marker = dir.join(".cyberctf-commit");
    // Docker's networks now, so a new lab's instance lands on blocks nothing else holds.
    let in_use = runtime::subnets_in_use(lab_id).await;
    if installed(&marker, commit) {
        runtime::lab::ensure_instance(&labs, &dir, &in_use)?;
        return Ok(dir);
    }

    // A new version of a lab that is up here: swapping the folder would pull its files out from
    // under the running containers (they bind-mount them) and lose track of them. Stop it first.
    // Same on a server host: the folder holds Vagrant's record of its ESXi machines (and the
    // module its Terraform state was applied from), which the swap would delete.
    if dir.exists() {
        let on_server = runtime::deployed_on_server(app, &dir, lab_id);
        // Running here, or VMs Vagrant still tracks here (a launch that failed on one machine, a
        // VM powered off by hand): the swap would delete its record and orphan them.
        let here = on_server.is_none() && (runtime::running_here(&dir, lab_id).await || runtime::vms_recorded_here(&dir));
        refuse_update(here, on_server.as_deref())?;
    }

    log(format!("Downloading {repository}@{}", &commit[..12]));
    let bytes = fetch().await?;

    let staging = stage(&labs, &dir, &bytes, release)?;
    // A paused copy of the previous version would be orphaned at the hypervisor once its
    // folder (and Vagrant's record of it) is gone: take it down first.
    runtime::clear_parked(&dir, lab_id, log).await;
    swap_in(&staging, &dir)?;
    // Its own room on this machine (networks, names), so it runs beside the other labs.
    runtime::lab::ensure_instance(&labs, &dir, &in_use)?;
    log("Lab installed".into());
    Ok(dir)
}

/// Refuses to replace an installed lab with a new version while it runs `here`, or on the
/// server host named `on_server`.
fn refuse_update(here: bool, on_server: Option<&str>) -> Result<()> {
    if let Some(host) = on_server {
        return Err(Error::Refused(format!(
            "A new version of this lab is out, but it is still running on {host}. Stop it, then start it again to get the new version."
        )));
    }
    if here {
        return Err(Error::Refused(
            "A new version of this lab is out, but it is still running here (or its machines still exist here). Stop it, then start it again to get the new version.".into(),
        ));
    }
    Ok(())
}

/// Whether the lab at `commit` is the one installed (its commit marker says so).
fn installed(marker: &Path, commit: &str) -> bool {
    std::fs::read_to_string(marker).is_ok_and(|c| c.trim() == commit)
}

/// Extracts the downloaded lab into a staging folder next to `dir`, with its source markers and
/// the markers of where the installed copy runs. Returns the staging folder.
fn stage(labs: &Path, dir: &Path, tarball: &[u8], release: Release<'_>) -> Result<PathBuf> {
    let Release { lab_id, repository, commit } = release;
    let staging = labs.join(format!(".{lab_id}.staging"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    extract(tarball, &staging)?;
    std::fs::write(staging.join(".cyberctf-commit"), commit)?;
    // Terraform targets fetch the lab themselves, from this repository at that commit.
    std::fs::write(staging.join(".cyberctf-repository"), repository)?;
    // Keep where the lab runs, so a lab still up on a server host or in a local VM can be
    // stopped there.
    for marker in [".cyberctf-host", runtime::LOCAL_VM_MARKER, runtime::lab::INSTANCE_MARKER] {
        if let Ok(value) = std::fs::read(dir.join(marker)) {
            std::fs::write(staging.join(marker), value)?;
        }
    }
    Ok(staging)
}

/// Replaces the installed lab with the staged one, all or nothing: the old one is moved aside
/// (a rename, which Windows refuses whole while a file inside is open, where deleting it would
/// stop halfway and leave a broken lab), the new one moved in, and the old one put back if
/// that fails. The old copy is deleted last, best effort; a leftover goes on the next update.
fn swap_in(staging: &Path, dir: &Path) -> Result<()> {
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let old = dir.with_file_name(format!(".{name}.old"));
    let _ = std::fs::remove_dir_all(&old);
    let had_old = dir.exists();
    if had_old {
        rename_retrying(dir, &old).map_err(|e| in_use(e, dir))?;
    }
    if let Err(e) = rename_retrying(staging, dir) {
        if had_old {
            let _ = rename_retrying(&old, dir);
        }
        return Err(in_use(e, dir));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
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

/// A `startLab` / `claimLaunch` payload.
fn parse_launch(launch_json: serde_json::Value) -> Result<Launch> {
    serde_json::from_value(launch_json).map_err(|e| Error::Invalid(format!("invalid launch spec: {e}")))
}

/// Installs and starts a lab from a launch spec (the `startLab`/`claimLaunch` shape:
/// `{ labId, runtime, repository, commit, env }`). Shared by the manual launch command
/// and the agent's claim loop (bring your own compute).
/// It runs where `placement` says.
pub(crate) async fn run(app: &AppHandle, launch_json: serde_json::Value, placement: &Placement<'_>, log: impl Fn(String)) -> Result<Option<String>> {
    let launch = parse_launch(launch_json)?;
    runtime::validate_id(&launch.lab_id)?;
    // The lab is this launch's from the download on: two installs of one lab share its staging
    // folder, and a stop must not run while the new version is swapped in. Shared with the
    // deploy worker's own hold on it (same process), so a worker runs on.
    let _lock = runtime::lock_lab(app, &launch.lab_id).await?;
    let release = Release { lab_id: &launch.lab_id, repository: &launch.repository, commit: &launch.commit };
    let dir = install(app, release, &log).await?;
    // Fixed names for every lab; the evidence itself is never in the environment.
    let env = launch_env(launch.env, placement.attackbox_image, placement.default_ports);
    runtime::start_locked(app, &dir, &launch.lab_id, launch.runtime, placement.provider, placement.host, &env, log).await?;
    // Where the lab's target is reachable on this machine, for the website to open.
    Ok(runtime::primary_url(&dir, &launch.lab_id, launch.runtime).await)
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
/// `token`'s `startLab` (no extras): for the real-lab tests, against a local test backend.
#[cfg(test)]
pub(crate) async fn launch_values_at(base: &str, token: &str, lab_id: &str) -> Result<Vec<(String, String)>> {
    let data = api::send(base, Some(token.to_owned()), START_LAB, json!({ "id": lab_id })).await?;
    Ok(launch_env(parse_launch(data["startLab"].clone())?.env, None, false))
}

/// The launch spec for `lab_id`: the backend's answer (whose source is kept for a later offline
/// start), or, when the backend can't be reached and the player asked for it (`offline`), the
/// installed lab's (see `offline`).
async fn launch_spec(app: &AppHandle, lab_id: &str, offline: bool, logs: &Channel<String>) -> Result<serde_json::Value> {
    let base = app.path().app_data_dir().map_err(|e| Error::Invalid(e.to_string()))?;
    let user = crate::account::auth::signed_in_user().await;
    match start_lab(lab_id).await {
        Ok(answer) => {
            if let Some(user) = &user
                && let Err(e) = offline::keep(&base, &answer, user)
            {
                log::warn!("couldn't keep {lab_id}'s launch for an offline start: {e}");
            }
            Ok(answer)
        }
        Err(failed) => {
            let spec = offline::instead_of(failed, &base, lab_id, user.as_deref(), offline)?;
            let _ = logs.send(offline::STARTING_OFFLINE.into());
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
    /// An attack box started next to the lab on a server host (where the lab network isn't
    /// reachable from this machine).
    pub attackbox_image: Option<String>,
    /// A container lab's services published on their own ports instead of random free ones.
    pub default_ports: bool,
    /// The installed lab started without the backend when it can't be reached (no progress
    /// saved): the player's answer to a launch that failed with `Code::OfflineReady`.
    pub offline: bool,
}

/// Starts `request`'s lab: the backend's launch, then the deploy in a detached worker whose log
/// this follows on `logs`.
pub async fn lab_launch(app: AppHandle, request: LaunchRequest, logs: Channel<String>) -> Result<()> {
    let LaunchRequest { lab_id, provider, host, attackbox_image, default_ports, offline } = request;
    if let Some(image) = &attackbox_image
        && !crate::platform::exec::valid_image(image)
    {
        return Err(Error::Invalid(format!("invalid attack-box image `{image}`")));
    }
    let startlab = launch_spec(&app, &lab_id, offline, &logs).await?;
    // The deploy runs in a detached worker process, so quitting (or crashing) this app never
    // cuts a vagrant/docker/terraform run short; this command only follows the worker's log.
    let job = crate::deploy_worker::Job { lab_id, op: crate::deploy_worker::Op::Launch, launch: startlab, provider, host, attackbox_image, default_ports };
    crate::deploy_worker::run_job(&app, job, logs).await
}

/// A stable lab id for a lab imported from the local directory `src`: `local-<slug>` of the lab's
/// own name (its folder name when the spec has none), so the ordinary lifecycle commands act on it
/// by id. Errors when `src` is not a lab folder.
fn local_id(src: &Path) -> Result<String> {
    let spec = runtime::lab::load(src).map_err(|e| Error::Invalid(format!("not a lab folder ({}): {e}", src.display())))?;
    let base = if spec.name.is_empty() { src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() } else { spec.name };
    let slug: String = base.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c.to_ascii_lowercase() } else { '-' }).collect();
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "lab" } else { slug };
    let id = format!("local-{}", &slug[..slug.len().min(56)]);
    runtime::validate_id(&id)?;
    Ok(id)
}

/// Installs the lab from the local directory `src` into `<app data>/labs/<lab id>/` by copying it
/// (no download): an imported lab. The rest is the ordinary install — its own instance room, any
/// parked copy cleared first — so the lifecycle commands then drive it like any installed lab.
async fn install_local(app: &AppHandle, lab_id: &str, src: &Path, log: &impl Fn(String)) -> Result<PathBuf> {
    runtime::validate_id(lab_id)?;
    runtime::lab::load(src).map_err(|e| Error::Invalid(format!("not a lab folder ({}): {e}", src.display())))?;
    let labs = app.path().app_data_dir().map_err(|e| Error::Invalid(e.to_string()))?.join("labs");
    let dir = labs.join(lab_id);
    let in_use = runtime::subnets_in_use(lab_id).await;
    // Re-importing while the previous copy runs here would pull its files out from under the
    // containers; stop it first.
    if dir.exists() && runtime::running_here(&dir, lab_id).await {
        return Err(Error::Refused(format!("{lab_id} is running; stop it before importing it again")));
    }
    log(format!("Importing {lab_id} from {}", src.display()));
    let staging = dir.with_file_name(format!(".{lab_id}.staging"));
    let _ = std::fs::remove_dir_all(&staging);
    runtime::lab::copy_tree(src, &staging)?;
    runtime::clear_parked(&dir, lab_id, log).await;
    swap_in(&staging, &dir)?;
    runtime::lab::ensure_instance(&labs, &dir, &in_use)?;
    log("Lab imported".into());
    Ok(dir)
}

/// Installs and starts the lab in the local directory `src` as a Docker lab with development
/// inputs (no backend, no flags): an imported custom lab, for running one not published yet.
/// Returns its local URL.
pub(crate) async fn run_local(app: &AppHandle, lab_id: &str, src: &Path, log: impl Fn(String)) -> Result<Option<String>> {
    runtime::validate_id(lab_id)?;
    let _lock = runtime::lock_lab(app, lab_id).await?;
    let dir = install_local(app, lab_id, src, &log).await?;
    runtime::start_locked(app, &dir, lab_id, Runtime::Docker, None, None, &[], log).await?;
    Ok(runtime::primary_url(&dir, lab_id, Runtime::Docker).await)
}

/// Imports a lab from the local directory `src` and starts it, bypassing the catalogue and the
/// backend: for running a lab that is not published (a lab author's own, a lab under test). The
/// deploy runs in a detached worker, its log on `logs`.
pub async fn lab_launch_local(app: AppHandle, src: String, logs: Channel<String>) -> Result<()> {
    let src = PathBuf::from(src);
    let lab_id = local_id(&src)?;
    let job = crate::deploy_worker::Job::on_lab(&lab_id, crate::deploy_worker::Op::LaunchLocal { src: src.to_string_lossy().into_owned() });
    crate::deploy_worker::run_job(&app, job, logs).await
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn tarball(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for (path, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            // Write the raw name so tests can include `..` (append_data refuses it).
            let name = header.as_old_mut().name.as_mut();
            name[..path.len()].copy_from_slice(path.as_bytes());
            header.set_cksum();
            builder.append(&header, *data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn extracts_without_the_top_level_folder() {
        let dir = std::env::temp_dir().join(format!("cyberctf-extract-{}", rand::random::<u32>()));
        extract(&tarball(&[("lab-abc/docker-compose.yml", b"services: {}"), ("lab-abc/evidence/claim.sh", b"#!/bin/sh")]), &dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("docker-compose.yml")).unwrap(), "services: {}");
        assert!(dir.join("evidence/claim.sh").is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn names_windows_cant_hold_are_told_apart() {
        for bad in ["0 ", "notes.", "aux", "AUX.c", "con.txt", "Lpt1", "com9.log", "a:b", "what?", "tab\there", "x\"y"] {
            assert!(windows_name_problem(bad).is_some(), "{bad:?}");
        }
        for good in ["0", "a b", ".env", "auxiliary.c", "com10", "lpt0", "console", "README.md", "caf\u{e9}"] {
            assert_eq!(windows_name_problem(good), None, "{good:?}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_lab_archive_with_a_name_windows_cant_hold_is_refused_plainly() {
        let dir = std::env::temp_dir().join(format!("cyberctf-extract-{}", rand::random::<u32>()));
        let err = extract(&tarball(&[("lab-abc/ok.txt", b"x"), ("lab-abc/notes ", b"y")]), &dir).unwrap_err().to_string();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(err.contains("can't be installed on Windows") && err.contains("ending in a space"), "{err}");
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
    fn staging_carries_the_source_and_where_the_old_copy_runs() {
        let labs = std::env::temp_dir().join(format!("cyberctf-stage-{}", rand::random::<u32>()));
        let dir = labs.join("invoice");
        let sha = "0123456789abcdef0123456789abcdef01234567";
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".cyberctf-host"), "pve1").unwrap();
        std::fs::write(dir.join(runtime::LOCAL_VM_MARKER), "virtualbox").unwrap();
        std::fs::write(dir.join("old.txt"), "old").unwrap();
        // Nothing is installed at that commit yet.
        assert!(!installed(&dir.join(".cyberctf-commit"), sha));

        let staging = stage(
            &labs,
            &dir,
            &tarball(&[("lab-abc/isoloom.yml", b"name: invoice")]),
            Release { lab_id: "invoice", repository: "CyberCTF/invoice", commit: sha },
        )
        .unwrap();
        assert_eq!(staging, labs.join(".invoice.staging"));
        assert_eq!(std::fs::read_to_string(staging.join("isoloom.yml")).unwrap(), "name: invoice");
        assert_eq!(std::fs::read_to_string(staging.join(".cyberctf-repository")).unwrap(), "CyberCTF/invoice");
        assert_eq!(std::fs::read_to_string(staging.join(".cyberctf-host")).unwrap(), "pve1");
        assert_eq!(std::fs::read_to_string(staging.join(runtime::LOCAL_VM_MARKER)).unwrap(), "virtualbox");
        assert!(!staging.join(runtime::lab::INSTANCE_MARKER).exists());

        swap_in(&staging, &dir).unwrap();
        assert!(!staging.exists());
        assert!(!dir.join("old.txt").exists());
        assert!(installed(&dir.join(".cyberctf-commit"), sha));
        assert!(!installed(&dir.join(".cyberctf-commit"), "ffffffffffffffffffffffffffffffffffffffff"));
        assert!(!labs.join(".invoice.old").exists(), "the old copy is cleared");
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

    #[test]
    fn a_lab_up_here_or_on_a_server_is_not_updated_under_it() {
        // Regression: the update only asked this machine's Docker and hypervisors, so a lab
        // running on ESXi was swapped out and its `.vagrant` (Vagrant's record of the machines)
        // deleted.
        assert!(refuse_update(false, None).is_ok());
        assert!(refuse_update(true, None).unwrap_err().to_string().contains("still running here"));
        let err = refuse_update(false, Some("ESXi lab")).unwrap_err().to_string();
        assert!(err.contains("still running on ESXi lab"), "{err}");
    }

    #[test]
    fn links_and_the_top_level_folder_entry_are_skipped() {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        let mut dir = tar::Header::new_gnu();
        dir.set_entry_type(tar::EntryType::Directory);
        dir.set_size(0);
        dir.set_mode(0o755);
        builder.append_data(&mut dir.clone(), "lab-abc/", &[][..]).unwrap();
        builder.append_data(&mut dir, "lab-abc/docs/", &[][..]).unwrap();
        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_size(0);
        builder.append_link(&mut link, "lab-abc/passwd", "/etc/passwd").unwrap();
        let gz = builder.into_inner().unwrap().finish().unwrap();
        let dest = std::env::temp_dir().join(format!("cyberctf-extract-{}", rand::random::<u32>()));
        extract(&gz, &dest).unwrap();
        assert!(dest.join("docs").is_dir());
        assert!(std::fs::symlink_metadata(dest.join("passwd")).is_err());
        std::fs::remove_dir_all(dest).unwrap();
    }

    const SPEC: &str = "version: 1\nname: invoice\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    ports: { web: { port: 80, protocol: http } }\n    app: { image: nginx:1.27 }\n";
    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    #[tokio::test]
    async fn a_new_lab_version_is_installed_unless_the_old_one_still_runs() {
        use crate::platform::exec::fake::{Tools, ok};
        let t = crate::testing::test_app::TestApp::new();
        let gz = tarball(&[("lab-abc/isoloom.yml", SPEC.as_bytes())]);
        let tools = Tools::rules(&[]);
        let lines = std::sync::Mutex::new(Vec::<String>::new());
        let log = |l: String| lines.lock().unwrap().push(l);
        let dir =
            install_with(t.handle(), Release { lab_id: "invoice", repository: "CyberCTF/invoice", commit: SHA }, &log, async || Ok(gz.clone())).await.unwrap();
        assert_eq!(dir, t.data().join("labs").join("invoice"));
        assert!(installed(&dir.join(".cyberctf-commit"), SHA) && dir.join("isoloom.yml").is_file());
        assert_eq!(lines.lock().unwrap().as_slice(), ["Downloading CyberCTF/invoice@0123456789ab", "Lab installed"]);
        // The same commit again: nothing downloaded.
        install_with(t.handle(), Release { lab_id: "invoice", repository: "CyberCTF/invoice", commit: SHA }, &log, async || panic!("no download"))
            .await
            .unwrap();
        drop(tools);
        // A newer commit while the old one runs here: refused, the running copy left alone.
        let _tools = Tools::rules(&[("ps --all", ok(r#"{"Service":"web","State":"running"}"#))]);
        let newer = "1123456789abcdef0123456789abcdef01234567";
        let err = install_with(t.handle(), Release { lab_id: "invoice", repository: "CyberCTF/invoice", commit: newer }, &log, async || panic!("no download"))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.starts_with("A new version of this lab is out, but it is still running here"), "{err}");
        // Ids and sources are checked before anything else.
        assert!(install_with(t.handle(), Release { lab_id: "../x", repository: "CyberCTF/invoice", commit: SHA }, &log, async || panic!()).await.is_err());
        assert!(install_with(t.handle(), Release { lab_id: "invoice", repository: "evil/lab", commit: SHA }, &log, async || panic!()).await.is_err());
    }

    #[tokio::test]
    async fn a_launch_waits_for_the_lab_lock_before_installing_anything() {
        // Regression: the install (download, staging, swap) ran before the lock was taken, so
        // two installs of one lab shared its staging folder and a stop could run mid-swap.
        use crate::platform::exec::fake::Tools;
        let t = crate::testing::test_app::TestApp::new();
        let dir = t.lab("invoice", &[("isoloom.yml", SPEC), (".cyberctf-commit", SHA)]);
        let tools = Tools::rules(&[]);
        let held = runtime::lock_lab(t.handle(), "invoice").await.unwrap();
        let launch = serde_json::json!({ "labId": "invoice", "runtime": "DOCKER", "repository": "CyberCTF/invoice", "commit": SHA, "env": [] });
        let waiting = tokio::time::timeout(std::time::Duration::from_millis(300), run(t.handle(), launch.clone(), &Placement::default(), |_| {})).await;
        assert!(waiting.is_err(), "the launch waits while another operation holds the lab");
        assert!(tools.calls().is_empty(), "nothing of the install ran meanwhile: {:?}", tools.calls());
        assert!(!dir.join(runtime::lab::INSTANCE_MARKER).exists());
        // Once the lock is free the launch may proceed (its install and the runner start are
        // covered by the runner's own tests and the real-lab integration tests).
        drop(held);
        let _ = launch;
    }

    #[tokio::test]
    async fn a_launch_refuses_a_bad_attack_box_image_before_asking_the_backend() {
        let t = crate::testing::test_app::TestApp::new();
        let request =
            LaunchRequest { lab_id: "invoice".into(), provider: None, host: None, attackbox_image: Some("-x".into()), default_ports: false, offline: false };
        let err = lab_launch(t.handle().clone(), request, tauri::ipc::Channel::new(|_| Ok(()))).await;
        assert_eq!(err.unwrap_err().to_string(), "invalid attack-box image `-x`");
    }

    #[tokio::test]
    async fn importing_a_folder_without_a_lab_is_refused() {
        let t = crate::testing::test_app::TestApp::new();
        let dir = std::env::temp_dir().join(format!("cyberctf-notalab-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // No isoloom.yml: lab_launch_local refuses before it ever reaches the runner.
        let err = lab_launch_local(t.handle().clone(), dir.to_string_lossy().into_owned(), tauri::ipc::Channel::new(|_| Ok(()))).await;
        let _ = std::fs::remove_dir_all(&dir);
        assert!(err.is_err(), "a folder with no isoloom.yml is not a lab");
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
    fn rejects_path_traversal_in_archives() {
        let dir = std::env::temp_dir().join(format!("cyberctf-extract-{}", rand::random::<u32>()));
        let err = extract(&tarball(&[("lab-abc/../../evil", b"x")]), &dir);
        assert!(err.is_err());
        let _ = std::fs::remove_dir_all(dir);
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

    fn extract_to(entries: &[(&str, &[u8])]) -> Result<PathBuf> {
        let dir = std::env::temp_dir().join(format!("cyberctf-extract-{}", rand::random::<u32>()));
        extract(&tarball(entries), &dir).map(|()| dir)
    }

    #[test]
    fn an_archive_unpacking_past_the_limit_is_refused() {
        let dir = std::env::temp_dir().join(format!("cyberctf-extract-{}", rand::random::<u32>()));
        // Highly compressible, like a gzip bomb: 3 MiB of zeros in a few KiB.
        let zeros = vec![0u8; 3 * 1024 * 1024];
        let archive = tarball(&[("lab-abc/a.bin", &zeros), ("lab-abc/b.bin", &zeros)]);
        assert!(archive.len() < 64 * 1024);
        let err = extract_within(&archive, &dir, 4 * 1024 * 1024).unwrap_err().to_string();
        assert_eq!(err, "the lab archive unpacks to more than 4 MB; refused");
        // The entry past the limit was never written.
        assert!(!dir.join("b.bin").exists());
        let _ = std::fs::remove_dir_all(&dir);
        extract_within(&archive, &dir, 6 * 1024 * 1024).unwrap();
        assert_eq!(std::fs::metadata(dir.join("b.bin")).unwrap().len(), zeros.len() as u64);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_empty_archive_installs_nothing_and_fails_nothing() {
        let dir = extract_to(&[]).unwrap();
        // The destination is created by the caller (`stage`); nothing appears on its own.
        assert!(!dir.exists() || std::fs::read_dir(&dir).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(dir);
        // An entry that is only the top-level folder name (no slash) is the folder itself: skipped.
        let dir = extract_to(&[("lab-abc", b"not a file we want")]).unwrap();
        assert!(!dir.exists() || std::fs::read_dir(&dir).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn paths_outside_the_top_folder_land_inside_the_destination_or_are_refused() {
        // An absolute path and a leading `..` lose their first component like the top folder
        // does, so they can only land inside the destination.
        let dir = extract_to(&[("/etc/passwd", b"x"), ("../evil", b"y")]).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("etc/passwd")).unwrap(), "x");
        assert_eq!(std::fs::read_to_string(dir.join("evil")).unwrap(), "y");
        std::fs::remove_dir_all(dir).unwrap();
        // A `..` anywhere after the top folder is refused, wherever it sits.
        for path in ["lab-abc/a/../b", "lab-abc/..", "lab-abc/a/b/../../../c"] {
            let err = extract_to(&[(path, b"x")]).unwrap_err().to_string();
            assert!(err.starts_with("unsafe path in lab archive"), "{path}: {err}");
        }
        // A `.` in the middle is nothing; a backslash is a plain character on Unix, not a separator.
        let dir = extract_to(&[("lab-abc/./a", b"x")]).unwrap();
        assert!(dir.join("a").is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_top_folder_with_a_leading_dot_is_stripped_too() {
        // GNU tar (`tar czf x.tgz .`, or `-C dir .`) writes `./` before every entry; GitHub's
        // tarballs don't, but a lab archive built any other way would install under
        // `<dest>/<repo>-<sha>/`, where nothing looks.
        // Fails with: assertion failed: dir.join("isoloom.yml").is_file().
        // Suggested fix: skip a leading `CurDir` component before dropping the top folder
        // (`path.components().skip_while(|c| matches!(c, Component::CurDir)).skip(1)`).
        let dir = extract_to(&[("./lab-abc/isoloom.yml", b"name: x")]).unwrap();
        assert!(dir.join("isoloom.yml").is_file(), "{:?}", std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect::<Vec<_>>());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_file_under_a_path_that_is_a_file_fails_cleanly_and_a_truncated_archive_fails() {
        let err = extract_to(&[("lab-abc/sub", b"a file"), ("lab-abc/sub/x", b"under a file")]).unwrap_err();
        assert!(matches!(err, Error::Io(_)), "{err}");
        // Cut mid-stream: an error, never a lab installed from half an archive.
        let big = vec![7u8; 20_000];
        let gz = tarball(&[("lab-abc/isoloom.yml", b"name: x"), ("lab-abc/big", &big)]);
        let dir = std::env::temp_dir().join(format!("cyberctf-extract-{}", rand::random::<u32>()));
        assert!(extract(&gz[..gz.len() / 2], &dir).is_err());
        assert!(extract(b"not a gzip at all", &dir).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn long_unicode_paths_and_hard_links_in_an_archive() {
        // A path past tar's 100-byte name field (GNU long-name extension) with non-ASCII names
        // extracts whole; a hard link (not a file or folder) is skipped like a symlink.
        let deep = format!("lab-abc/{}/{}/fichier é.txt", "dossier-très-long-".repeat(4), "ü".repeat(40));
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        let mut file = tar::Header::new_gnu();
        file.set_size(2);
        file.set_mode(0o644);
        file.set_entry_type(tar::EntryType::Regular);
        builder.append_data(&mut file, &deep, &b"ok"[..]).unwrap();
        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Link);
        link.set_size(0);
        builder.append_link(&mut link, "lab-abc/alias", &deep).unwrap();
        let gz = builder.into_inner().unwrap().finish().unwrap();
        let dir = std::env::temp_dir().join(format!("cyberctf-extract-{}", rand::random::<u32>()));
        extract(&gz, &dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(&deep["lab-abc/".len()..])).unwrap(), "ok");
        assert!(std::fs::symlink_metadata(dir.join("alias")).is_err(), "a hard link is skipped");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_leftover_staging_folder_is_replaced_whole_and_an_installed_marker_is_trimmed() {
        let labs = std::env::temp_dir().join(format!("cyberctf-stage-{}", rand::random::<u32>()));
        let dir = labs.join("lab");
        let staging = labs.join(".lab.staging");
        std::fs::create_dir_all(staging.join("old")).unwrap();
        std::fs::write(staging.join("old/file"), "from a crashed install").unwrap();
        let staged =
            stage(&labs, &dir, &tarball(&[("lab-abc/isoloom.yml", b"name: lab")]), Release { lab_id: "lab", repository: "CyberCTF/lab", commit: SHA }).unwrap();
        assert_eq!(staged, staging);
        assert!(!staging.join("old").exists(), "the crashed install's files are gone");
        // The commit marker reads back with surrounding whitespace or CRLF (a hand edit).
        std::fs::write(staging.join(".cyberctf-commit"), format!("  {SHA}\r\n")).unwrap();
        assert!(installed(&staging.join(".cyberctf-commit"), SHA));
        assert!(!installed(&staging.join(".cyberctf-commit"), &SHA.to_uppercase()));
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
        let dir = install_with(t.handle(), Release { lab_id: &max, repository: "CyberCTF/invoice", commit: SHA }, &log, async || Ok(gz.clone())).await.unwrap();
        assert_eq!(dir.file_name().unwrap().to_str().unwrap(), max);
        for bad in ["", &"a".repeat(65), "é", "a b", "a/b", ".", "a.b", "..", "a\u{0}b"] {
            let bad = bad.to_string();
            let err = install_with(t.handle(), Release { lab_id: &bad, repository: "CyberCTF/invoice", commit: SHA }, &log, async || {
                panic!("no download for {bad:?}")
            })
            .await
            .unwrap_err()
            .to_string();
            assert!(err.starts_with("invalid lab id"), "{bad:?}: {err}");
        }
        // "-" and "_" alone pass the rule (a folder of that name is harmless).
        let dir = install_with(t.handle(), Release { lab_id: "-", repository: "CyberCTF/invoice", commit: SHA }, &log, async || Ok(gz.clone())).await.unwrap();
        assert_eq!(dir, t.data().join("labs").join("-"));
    }
}

#[cfg(test)]
mod proptests;
