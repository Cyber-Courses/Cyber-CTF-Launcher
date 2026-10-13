//! Launching a lab: startLab (CyberBackend) -> download the lab at its pinned
//! commit from GitHub -> run it with CTF_API_URL / CTF_LAUNCH_TOKEN, which the
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Launch {
    lab_id: String,
    runtime: Runtime,
    repository: String,
    commit: String,
    env: Vec<EnvVar>,
}

#[derive(Deserialize)]
struct EnvVar {
    name: String,
    value: String,
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
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tarball));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            continue;
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

/// The lab's GitHub tarball at `commit`.
async fn download(repository: &str, commit: &str) -> Result<Vec<u8>> {
    fetch_archive(&format!("https://codeload.github.com/{repository}/tar.gz/{commit}"), crate::http::MAX_LAB_ARCHIVE).await
}

/// A lab archive at `url`, streamed with timeouts (a stalled transfer is given up) and refused
/// past `max` bytes. It was read whole into memory with no limit and no timeout: a stalled
/// GitHub transfer hung the launch forever.
async fn fetch_archive(url: &str, max: u64) -> Result<Vec<u8>> {
    let res = crate::http::download_client()?.get(url).send().await.map_err(|e| Error::Invalid(format!("download failed: {e}")))?;
    if !res.status().is_success() {
        return Err(Error::Invalid(format!("download failed: HTTP {}", res.status())));
    }
    crate::http::body_limited(res, max).await
}

/// Downloads the lab into `<app data>/labs/<lab id>/` unless that exact commit is
/// already installed. Extracts next to it, then swaps, so a failed download
/// never leaves a half-installed lab.
async fn install(app: &AppHandle, lab_id: &str, repository: &str, commit: &str, log: &impl Fn(String)) -> Result<PathBuf> {
    install_with(app, lab_id, repository, commit, log, || download(repository, commit)).await
}

/// [`install`], the archive fetched by `fetch` (when the commit isn't installed yet).
async fn install_with(
    app: &AppHandle,
    lab_id: &str,
    repository: &str,
    commit: &str,
    log: &impl Fn(String),
    fetch: impl AsyncFnOnce() -> Result<Vec<u8>>,
) -> Result<PathBuf> {
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
        runtime::lab::ensure_instance(&labs, &dir, &in_use, &runtime::lab::registry_instances(&dir))?;
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

    let staging = stage(&labs, lab_id, &dir, &bytes, repository, commit)?;
    // A paused copy of the previous version would be orphaned at the hypervisor once its
    // folder (and Vagrant's record of it) is gone: take it down first.
    runtime::clear_parked(&dir, lab_id, log).await;
    swap_in(&staging, &dir)?;
    // Its own room on this machine (networks, names), so it runs beside the other labs.
    runtime::lab::ensure_instance(&labs, &dir, &in_use, &runtime::lab::registry_instances(&dir))?;
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
fn stage(labs: &Path, lab_id: &str, dir: &Path, tarball: &[u8], repository: &str, commit: &str) -> Result<PathBuf> {
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

/// The environment a launch runs with: only the evidence claim's two variables from the
/// backend, then the attack-box image and the default-ports switch when asked for.
fn launch_env(env: Vec<EnvVar>, attackbox_image: Option<&str>, default_ports: bool) -> Vec<(String, String)> {
    env.into_iter()
        .filter(|v| v.name == "CTF_API_URL" || v.name == "CTF_LAUNCH_TOKEN")
        .map(|v| (v.name, v.value))
        .chain(attackbox_image.map(|i| ("CYBERCTF_ATTACKBOX_IMAGE".to_string(), i.to_string())))
        .chain(default_ports.then(|| (runtime::PORTS_ENV.to_string(), "default".to_string())))
        .collect()
}

/// A `startLab` / `claimLaunch` payload.
fn parse_launch(launch_json: serde_json::Value) -> Result<Launch> {
    serde_json::from_value(launch_json).map_err(|e| Error::Invalid(format!("invalid launch spec: {e}")))
}

/// Installs and starts a lab from a launch spec (the `startLab`/`claimLaunch` shape:
/// `{ labId, runtime, repository, commit, env }`). Shared by the manual launch command
/// and the agent's claim loop (bring your own compute).
/// VM labs run locally with `provider`, or on the server `host` when one is given.
pub(crate) async fn run(
    app: &AppHandle,
    launch_json: serde_json::Value,
    provider: Option<Provider>,
    host: Option<&str>,
    attackbox_image: Option<&str>,
    default_ports: bool,
    log: impl Fn(String),
) -> Result<Option<String>> {
    let launch = parse_launch(launch_json)?;
    let dir = install(app, &launch.lab_id, &launch.repository, &launch.commit, &log).await?;
    // Fixed names for every lab; the evidence itself is never in the environment.
    let env = launch_env(launch.env, attackbox_image, default_ports);
    runtime::start(app, &dir, &launch.lab_id, launch.runtime, provider, host, &env, log).await?;
    // Where the lab's target is reachable on this machine, for the website to open.
    Ok(runtime::primary_url(&dir, &launch.lab_id, launch.runtime).await)
}

/// `attackbox_image` starts an attack box next to the lab on a server host (where the
/// lab network isn't reachable from this machine). `default_ports` publishes a container lab's
/// services on their own ports instead of random free ones.
#[tauri::command]
pub async fn lab_launch(
    app: AppHandle,
    lab_id: String,
    provider: Option<Provider>,
    host: Option<String>,
    attackbox_image: Option<String>,
    default_ports: Option<bool>,
    logs: Channel<String>,
) -> Result<()> {
    if let Some(image) = &attackbox_image
        && !runtime::valid_image(image)
    {
        return Err(Error::Invalid(format!("invalid attack-box image `{image}`")));
    }
    let data =
        api::graphql("mutation ($id: ID!) { startLab(labId: $id) { labId runtime repository commit env { name value } } }", json!({ "id": lab_id }), true)
            .await?;
    let startlab = data["startLab"].clone();
    // The deploy runs in a detached worker process, so quitting (or crashing) this app never
    // cuts a vagrant/docker/terraform run short; this command only follows the worker's log.
    let job = crate::deploy_worker::Job {
        lab_id,
        op: crate::deploy_worker::Op::Launch,
        launch: startlab,
        provider,
        host,
        attackbox_image,
        default_ports: default_ports.unwrap_or(false),
    };
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
    fn a_launch_passes_only_the_evidence_variables_and_the_asked_extras() {
        let env = |pairs: &[(&str, &str)]| pairs.iter().map(|(n, v)| EnvVar { name: n.to_string(), value: v.to_string() }).collect::<Vec<_>>();
        let backend = env(&[("CTF_API_URL", "https://api"), ("PATH", "/evil"), ("CTF_LAUNCH_TOKEN", "t0k")]);
        assert_eq!(launch_env(backend, None, false), [("CTF_API_URL".to_string(), "https://api".to_string()), ("CTF_LAUNCH_TOKEN".into(), "t0k".into())]);
        let extras = launch_env(env(&[]), Some("ghcr.io/x/box:1"), true);
        assert_eq!(extras, [("CYBERCTF_ATTACKBOX_IMAGE".to_string(), "ghcr.io/x/box:1".to_string()), (runtime::PORTS_ENV.to_string(), "default".to_string())]);
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

        let staging = stage(&labs, "invoice", &dir, &tarball(&[("lab-abc/isoloom.yml", b"name: invoice")]), "CyberCTF/invoice", sha).unwrap();
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
        let labs = std::env::temp_dir().join(format!("cyberctf-swap-{}", rand::random::<u32>()));
        let (dir, staging) = (labs.join("invoice"), labs.join(".invoice.staging"));
        for (d, v) in [(&dir, "old"), (&staging, "new")] {
            std::fs::create_dir_all(d.join("sub")).unwrap();
            std::fs::write(d.join("a.txt"), v).unwrap();
            std::fs::write(d.join("sub").join("disk.vmdk"), v).unwrap();
        }
        const FILE_SHARE_READ: u32 = 1;
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

    const SPEC: &str = "version: 1\nname: invoice\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    services: [{ port: 80, http: true }]\n    docker: { image: nginx:1.27 }\n";
    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    #[tokio::test]
    async fn a_new_lab_version_is_installed_unless_the_old_one_still_runs() {
        use crate::exec::fake::{Tools, ok};
        let t = crate::test_app::TestApp::new();
        let gz = tarball(&[("lab-abc/isoloom.yml", SPEC.as_bytes())]);
        let tools = Tools::rules(&[]);
        let lines = std::sync::Mutex::new(Vec::<String>::new());
        let log = |l: String| lines.lock().unwrap().push(l);
        let dir = install_with(t.handle(), "invoice", "CyberCTF/invoice", SHA, &log, async || Ok(gz.clone())).await.unwrap();
        assert_eq!(dir, t.data().join("labs").join("invoice"));
        assert!(installed(&dir.join(".cyberctf-commit"), SHA) && dir.join("isoloom.yml").is_file());
        assert_eq!(lines.lock().unwrap().as_slice(), ["Downloading CyberCTF/invoice@0123456789ab", "Lab installed"]);
        // The same commit again: nothing downloaded.
        install_with(t.handle(), "invoice", "CyberCTF/invoice", SHA, &log, async || panic!("no download")).await.unwrap();
        drop(tools);
        // A newer commit while the old one runs here: refused, the running copy left alone.
        let _tools = Tools::rules(&[("ps --all", ok(r#"{"Service":"web","State":"running"}"#))]);
        let newer = "1123456789abcdef0123456789abcdef01234567";
        let err = install_with(t.handle(), "invoice", "CyberCTF/invoice", newer, &log, async || panic!("no download")).await.unwrap_err().to_string();
        assert!(err.starts_with("A new version of this lab is out, but it is still running here"), "{err}");
        // Ids and sources are checked before anything else.
        assert!(install_with(t.handle(), "../x", "CyberCTF/invoice", SHA, &log, async || panic!()).await.is_err());
        assert!(install_with(t.handle(), "invoice", "evil/lab", SHA, &log, async || panic!()).await.is_err());
    }

    #[tokio::test]
    async fn an_installed_lab_launches_on_this_machines_docker() {
        use crate::exec::fake::Tools;
        let t = crate::test_app::TestApp::new();
        let dir = t.lab("invoice", &[("isoloom.yml", SPEC), (".cyberctf-commit", SHA)]);
        let tools = Tools::rules(&[]);
        let launch = serde_json::json!({
            "labId": "invoice", "runtime": "DOCKER", "repository": "CyberCTF/invoice", "commit": SHA,
            "env": [{ "name": "CTF_API_URL", "value": "https://api" }, { "name": "PATH", "value": "/evil" }]
        });
        run(t.handle(), launch, None, None, None, true, |_| {}).await.unwrap();
        assert!(tools.called("up -d") && dir.join(runtime::lab::INSTANCE_MARKER).exists());
        assert!(run(t.handle(), serde_json::json!({ "labId": "x" }), None, None, None, false, |_| {}).await.is_err());
    }

    #[tokio::test]
    async fn a_launch_refuses_a_bad_attack_box_image_before_asking_the_backend() {
        let t = crate::test_app::TestApp::new();
        let err = lab_launch(t.handle().clone(), "invoice".into(), None, None, Some("-x".into()), None, tauri::ipc::Channel::new(|_| Ok(()))).await;
        assert_eq!(err.unwrap_err().to_string(), "invalid attack-box image `-x`");
    }

    #[tokio::test]
    async fn an_archive_download_is_limited_and_says_why_it_failed() {
        use crate::http::tests::serve_once;
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
        let gz = tarball(&[("lab-abc/isoloom.yml", b"name: x"), ("lab-abc/big", &[7u8; 20_000])]);
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
        let staged = stage(&labs, "lab", &dir, &tarball(&[("lab-abc/isoloom.yml", b"name: lab")]), "CyberCTF/lab", SHA).unwrap();
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
        use crate::exec::fake::Tools;
        let t = crate::test_app::TestApp::new();
        let _tools = Tools::rules(&[]);
        let log = |_: String| {};
        let max = "A".repeat(63) + "_";
        let gz = tarball(&[("lab-abc/isoloom.yml", SPEC.as_bytes())]);
        let dir = install_with(t.handle(), &max, "CyberCTF/invoice", SHA, &log, async || Ok(gz.clone())).await.unwrap();
        assert_eq!(dir.file_name().unwrap().to_str().unwrap(), max);
        for bad in ["", &"a".repeat(65), "é", "a b", "a/b", ".", "a.b", "..", "a\u{0}b"] {
            let bad = bad.to_string();
            let err = install_with(t.handle(), &bad, "CyberCTF/invoice", SHA, &log, async || panic!("no download for {bad:?}")).await.unwrap_err().to_string();
            assert!(err.starts_with("invalid lab id"), "{bad:?}: {err}");
        }
        // "-" and "_" alone pass the rule (a folder of that name is harmless).
        let dir = install_with(t.handle(), "-", "CyberCTF/invoice", SHA, &log, async || Ok(gz.clone())).await.unwrap();
        assert_eq!(dir, t.data().join("labs").join("-"));
    }
}

#[cfg(test)]
mod proptests;
