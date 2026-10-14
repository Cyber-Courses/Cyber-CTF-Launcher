//! Running Terraform on a module: the checks and clean-up every run starts with (`prepare`),
//! `terraform init` + a command for Isoloom's modules (the host binary, or on macOS for Proxmox
//! the official container, see `in_container`), and what every run shares (its environment, the
//! provider plugin cache, a failed `init` said plainly, a destroy's time limit). Variables reach
//! it through the environment.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::state::STATE_FILE;
use crate::error::{Error, Result};
use crate::exec::{run_read, stream};

/// The Terraform image for runs that can't use the host binary, pinned by the digest of its
/// multi-arch index: a tag can be moved to another image, a digest can't. The tag stays for
/// people reading `docker ps`; Docker pulls by the digest and ignores it.
///
/// To bump it, take the new tag's index digest and replace both:
/// `docker buildx imagetools inspect hashicorp/terraform:<version>` (the `Digest:` line).
const TERRAFORM_IMAGE: &str = "hashicorp/terraform:1.9.8@sha256:18f9986038bbaf02cf49db9c09261c778161c51dcc7fb7e355ae8938459428cd";

/// What a run says when Terraform isn't on this machine, whichever kind of lab asked.
pub(super) const NOT_INSTALLED: &str = "Terraform isn't installed. Install it from the setup of your server or cloud account (its tools section).";

/// The local backend's lock file, left behind by a crashed or killed run.
const LOCK_FILE: &str = ".terraform.tfstate.lock.info";

/// The folder, beside `deployments` in the app data folder, where every lab's runs share their
/// provider plugins: one copy of each provider version instead of one per lab and target (the
/// AWS provider alone is several hundred MB).
pub const PLUGIN_CACHE: &str = "terraform-plugins";

/// How long a destroy (its `init` included) may take before it is given up: a hung provider or
/// tool must not hold the lab, and the auto-stop sweep behind it, forever. Generous: deleting a
/// cloud lab's network and disks can take several minutes on a slow region.
pub(in crate::runtime) const DESTROY_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// The checks and clean-up every run on `module` (state in `state`) starts with: the module is
/// there, Terraform is installed, the state folder exists, and no stale lock is left. Changes
/// nothing when it refuses.
pub(super) async fn prepare(module: &Path, state: &Path) -> Result<()> {
    if !is_module(module) {
        return Err(Error::Invalid(format!("this lab has no Terraform module at {}", module.display())));
    }
    if run_read("terraform", &["version"], None).await.is_err() {
        return Err(Error::Invalid(NOT_INSTALLED.into()));
    }
    std::fs::create_dir_all(state)?;
    // A crashed or killed terraform can leave the local backend's lock file behind. The launcher
    // serializes operations on a lab (lab_lock) and is the only writer of this state, so any lock
    // present now is stale and would otherwise block this op with "Error acquiring the state
    // lock". Clear it before init.
    let _ = std::fs::remove_file(state.join(LOCK_FILE));
    Ok(())
}

/// Whether a folder is a Terraform module: it holds at least one `.tf` file.
pub fn is_module(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|d| d.flatten().any(|e| e.path().extension().is_some_and(|x| x == "tf")))
}

/// `run` (a Terraform `command` and the `init` before it), given up after [`DESTROY_TIMEOUT`]
/// when it is a destroy: the run is then killed (dropped) and the error says what to do.
pub(super) async fn bounded(command: &str, run: impl Future<Output = Result<()>>) -> Result<()> {
    if command != "destroy" {
        return run.await;
    }
    within(DESTROY_TIMEOUT, run).await
}

/// `run`, given up as a destroy that didn't finish after `limit`.
async fn within(limit: Duration, run: impl Future<Output = Result<()>>) -> Result<()> {
    tokio::time::timeout(limit, run).await.unwrap_or_else(|_| {
        Err(Error::Invalid(format!(
            "Terraform's destroy didn't finish within {} minutes and was stopped. Nothing was removed from the launcher's records: stop the lab again to retry, and check your account if it keeps happening.",
            limit.as_secs() / 60
        )))
    })
}

/// Runs `terraform init` then `command` (apply / destroy, auto-approved) on `module`, with its
/// state in `state` and `env` as the full environment (variables as `TF_VAR_*`).
pub(super) async fn terraform(module: &Path, state: &Path, env: &[(String, String)], command: &str, log: impl FnMut(String)) -> Result<()> {
    prepare(module, state).await?;
    bounded(command, async {
        if in_container(module).await {
            return terraform_container(module, state, env, command, log).await;
        }
        terraform_host(module, state, env, command, log).await
    })
    .await
}

/// macOS lets an app reach the local network but not the third-party tools it starts: Terraform's
/// Proxmox provider got "no route to host" to a LAN server the app itself could reach (even with
/// Cyber CTF allowed under Local Network), while Docker Desktop, an allowed app, carries container
/// traffic fine. So on macOS a Proxmox module runs in the Terraform container when Docker answers.
/// Clouds keep the host binary (they need the host's CLI logins, and the internet isn't affected).
async fn in_container(module: &Path) -> bool {
    cfg!(target_os = "macos")
        && module.components().any(|c| c.as_os_str() == "proxmox")
        && run_read("docker", &["info", "--format", "{{.ServerVersion}}"], None).await.is_ok()
}

/// The lab folder a module belongs to: the parent of its `.isoloom…` folder (the module itself
/// when it isn't under one).
fn lab_root(module: &Path) -> &Path {
    module.ancestors().find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(crate::runtime::lab::is_output_dir)).and_then(Path::parent).unwrap_or(module)
}

/// `env` plus what every run sets: its data folder beside the state, the shared plugin cache
/// when the state is a lab's (see [`plugin_cache`]), and automation mode.
pub(super) fn run_env(env: &[(String, String)], state: &Path) -> Vec<(String, String)> {
    let mut full = env.to_vec();
    full.push(("TF_DATA_DIR".to_string(), state.join(DATA_DIR).display().to_string()));
    if let Some(cache) = plugin_cache(state) {
        full.push(("TF_PLUGIN_CACHE_DIR".to_string(), cache.display().to_string()));
    }
    full.push(("TF_IN_AUTOMATION".to_string(), "1".to_string()));
    full
}

/// A run's own data folder (the modules and the plugins it links), under its state.
pub(super) const DATA_DIR: &str = ".terraform";

/// The shared plugin cache (`<app data>/terraform-plugins`) for a lab's state folder
/// (`<app data>/deployments/<lab>/<target>`); None for a state anywhere else (a self-test's).
pub(super) fn plugin_cache(state: &Path) -> Option<PathBuf> {
    let deployments = state.parent()?.parent()?;
    (deployments.file_name()? == "deployments").then(|| deployments.with_file_name(PLUGIN_CACHE))
}

/// `terraform init` on `module` with `args` after its own flags, holding the plugin cache while
/// it writes there: Terraform doesn't lock the cache, and two labs starting at once (each in its
/// own deploy worker) could otherwise corrupt a plugin they both download. A failed init is said
/// plainly when it couldn't download the plugins.
pub(super) async fn init(program: &'static str, args: &[&str], module: &Path, full: &[(String, String)], log: impl FnMut(String)) -> Result<()> {
    let _cache = lock_cache(full).await;
    stream(program, args, Some(module), full, log).await.map_err(init_failure)
}

/// An exclusive lock on the plugin cache named in a run's environment, held until dropped. None
/// when the run has no cache, or when another run kept it past [`CACHE_LOCK_WAIT`] (a stuck
/// init must not hold every other lab's runs, a destroy's included: this one then goes on
/// unlocked, as runs did before the cache was shared).
async fn lock_cache(full: &[(String, String)]) -> Option<std::fs::File> {
    let dir = PathBuf::from(&full.iter().find(|(k, _)| k == "TF_PLUGIN_CACHE_DIR")?.1);
    std::fs::create_dir_all(&dir).ok()?;
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(".lock")).ok()?;
    let deadline = tokio::time::Instant::now() + CACHE_LOCK_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => return Some(file),
            Err(std::fs::TryLockError::WouldBlock) if tokio::time::Instant::now() < deadline => tokio::time::sleep(Duration::from_millis(250)).await,
            Err(_) => return None,
        }
    }
}

/// Empties the plugin cache `dir`, holding its lock: never while an `init` writes or links from
/// it. Whether it was emptied (false while a run holds it: left for a later clean-up).
pub fn clear_plugin_cache(dir: &Path) -> bool {
    let Ok(lock) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(".lock")) else { return false };
    if lock.try_lock().is_err() {
        return false;
    }
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.file_name() != ".lock") {
        let _ = if entry.path().is_dir() { std::fs::remove_dir_all(entry.path()) } else { std::fs::remove_file(entry.path()) };
    }
    true
}

/// How long a run waits for another one's `init` to be done with the plugin cache.
const CACHE_LOCK_WAIT: Duration = Duration::from_secs(5 * 60);

/// `init`'s flag pointing the local backend at the state folder.
fn backend_arg(state: &Path) -> String {
    format!("-backend-config=path={}", state.join(STATE_FILE).display())
}

/// Folders to mount into the container: the whole lab, the state, the shared plugin cache, and
/// the folders of files a variable names (`*_file`, e.g. the SSH key), sorted and deduplicated.
fn container_mounts(module: &Path, state: &Path, env: &[(String, String)]) -> Vec<String> {
    // The whole lab, not just the module: Isoloom modules reach the rest of the lab through
    // `path.module/..` (the Proxmox module tars the lab folder to upload it; mounting only the
    // module left the archive without the lab's compose file).
    let mut mounts: Vec<String> = vec![lab_root(module).display().to_string(), state.display().to_string()];
    mounts.extend(plugin_cache(state).map(|c| c.display().to_string()));
    for (_, v) in env.iter().filter(|(k, _)| k.ends_with("_file")) {
        if let Some(parent) = Path::new(v).parent().filter(|p| p.is_absolute()) {
            mounts.push(parent.display().to_string());
        }
    }
    mounts.sort();
    mounts.dedup();
    mounts
}

/// Runs terraform in the official image, with the module, its state and the files the variables
/// name mounted at their own paths (so every path in the module and the state stays valid), and
/// the variables passed through the environment (`-e NAME`, never their values on a command line).
async fn terraform_container(dir: &Path, state: &Path, env: &[(String, String)], command: &str, mut log: impl FnMut(String)) -> Result<()> {
    let full = run_env(env, state);
    let mounts = container_mounts(dir, state, env);
    let workdir = dir.display().to_string();
    let docker_args = |tf_args: &[&str]| container_args(&workdir, &mounts, &full, tf_args);
    log("Running Terraform in its container (macOS keeps third-party tools off the local network).".into());
    let backend = backend_arg(state);
    let init_args = docker_args(&["init", "-input=false", "-no-color", &backend]);
    init("docker", &as_strs(&init_args), dir, &full, &mut log).await?;
    let cmd = docker_args(&[command, "-auto-approve", "-input=false", "-no-color"]);
    stream("docker", &as_strs(&cmd), Some(dir), &full, log).await
}

/// `docker run` of the Terraform image in `workdir`, with `mounts` at their own paths and the
/// names (never the values) of `env` passed through.
fn container_args(workdir: &str, mounts: &[String], env: &[(String, String)], tf_args: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = vec!["run".into(), "--rm".into(), "-i".into(), "-w".into(), workdir.to_string()];
    for m in mounts {
        args.extend(["-v".into(), format!("{m}:{m}")]);
    }
    for (k, _) in env {
        args.extend(["-e".into(), k.clone()]);
    }
    args.push(TERRAFORM_IMAGE.into());
    args.extend(tf_args.iter().map(|a| a.to_string()));
    args
}

fn as_strs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

/// Whether a failed `init` failed on downloading the provider plugins (offline, DNS, TLS).
fn is_download_failure(message: &str) -> bool {
    let s = message.to_lowercase();
    ["registry", "no such host", "timeout", "tls", "connection", "network is unreachable", "could not download"].iter().any(|m| s.contains(m))
}

/// A failed `init`, said plainly when it couldn't download the provider plugins.
fn init_failure(e: Error) -> Error {
    if is_download_failure(&e.to_string()) {
        Error::Invalid("Couldn't download the Terraform provider plugins (the first run needs internet). Check your connection and try again.".into())
    } else {
        e
    }
}

/// Runs terraform from the host PATH (init, then the command). State lives in `state` as
/// plain files on the host.
async fn terraform_host(dir: &Path, state: &Path, env: &[(String, String)], command: &str, mut log: impl FnMut(String)) -> Result<()> {
    let backend = backend_arg(state);
    let full = run_env(env, state);
    // Isoloom pins provider versions in the module; the lock file is written next to it. init
    // downloads the provider plugins the first time, which needs the network; say so plainly if
    // that's what failed (otherwise a destroy of an existing lab can look impossible when it's
    // just offline).
    init("terraform", &["init", "-input=false", "-no-color", backend.as_str()], dir, &full, &mut log).await?;
    stream("terraform", &[command, "-auto-approve", "-input=false", "-no-color"], Some(dir), &full, log).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terraform_image_is_pinned_by_digest() {
        let (name, digest) = TERRAFORM_IMAGE.split_once('@').expect("pinned by digest");
        assert!(name.starts_with("hashicorp/terraform:"), "{name}");
        let hex = digest.strip_prefix("sha256:").expect("a sha256 digest");
        assert!(hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{digest}");
    }

    #[test]
    fn container_mounts_the_whole_lab() {
        assert_eq!(lab_root(Path::new("/d/labs/x/.isoloom-1/docker-vm/proxmox")), Path::new("/d/labs/x"));
        assert_eq!(lab_root(Path::new("/d/labs/x/.isoloom/proxmox")), Path::new("/d/labs/x"));
        assert_eq!(lab_root(Path::new("/tmp/selftest/terraform/proxmox")), Path::new("/tmp/selftest/terraform/proxmox"));
    }

    // The Docker Terraform runner is macOS-only; Unix paths.
    #[cfg(unix)]
    #[test]
    fn container_mounts_add_key_folders_once() {
        let env = vec![
            ("TF_VAR_ssh_private_key_file".to_string(), "/k/id_ed25519".to_string()),
            ("TF_VAR_proxmox_ssh_private_key_file".to_string(), "/k/id_ed25519".to_string()),
            ("TF_VAR_relative_file".to_string(), "rel/key".to_string()),
            ("TF_VAR_region".to_string(), "/not/a/file".to_string()),
        ];
        let mounts = container_mounts(Path::new("/d/labs/x/.isoloom/proxmox"), Path::new("/d/state/x"), &env);
        assert_eq!(mounts, ["/d/labs/x", "/d/state/x", "/k"]);
    }

    #[test]
    fn a_labs_runs_share_the_plugin_cache_beside_the_deployments() {
        let state = Path::new("/d/data/deployments/web/aws");
        assert_eq!(plugin_cache(state), Some(PathBuf::from("/d/data/terraform-plugins")));
        let env = run_env(&[], state);
        assert!(env.contains(&("TF_PLUGIN_CACHE_DIR".to_string(), PathBuf::from("/d/data/terraform-plugins").display().to_string())));
        // A state elsewhere (a self-test's) keeps its plugins to itself.
        assert_eq!(plugin_cache(Path::new("/tmp/selftest/state")), None);
        assert_eq!(plugin_cache(Path::new("/")), None);
        assert!(!run_env(&[], Path::new("/tmp/selftest/state")).iter().any(|(k, _)| k == "TF_PLUGIN_CACHE_DIR"));
    }

    #[cfg(unix)]
    #[test]
    fn the_container_mounts_the_plugin_cache() {
        let mounts = container_mounts(Path::new("/d/labs/x/.isoloom/proxmox"), Path::new("/d/deployments/x/proxmox"), &[]);
        assert_eq!(mounts, ["/d/deployments/x/proxmox", "/d/labs/x", "/d/terraform-plugins"]);
    }

    #[tokio::test]
    async fn one_init_at_a_time_writes_the_plugin_cache() {
        let dir = std::env::temp_dir().join(format!("cyberctf-tf-cache-{}", rand::random::<u32>()));
        let env = vec![("TF_PLUGIN_CACHE_DIR".to_string(), dir.display().to_string())];
        let held = lock_cache(&env).await.expect("the cache is free");
        // A second init waits for the first one's lock.
        assert!(tokio::time::timeout(Duration::from_millis(600), lock_cache(&env)).await.is_err());
        // Nor is it emptied meanwhile.
        std::fs::write(dir.join("plugin"), "x").unwrap();
        assert!(!clear_plugin_cache(&dir) && dir.join("plugin").exists());
        drop(held);
        assert!(clear_plugin_cache(&dir) && !dir.join("plugin").exists());
        assert!(lock_cache(&env).await.is_some());
        assert!(lock_cache(&[]).await.is_none(), "no cache, no lock");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_hung_destroy_is_given_up_and_says_how_to_retry() {
        let err = within(Duration::from_millis(50), std::future::pending()).await.unwrap_err().to_string();
        assert!(err.starts_with("Terraform's destroy didn't finish within 0 minutes"), "{err}");
        assert!(within(Duration::from_secs(5), async { Ok(()) }).await.is_ok());
        // Only a destroy is bounded: an apply may legitimately take long.
        assert!(bounded("apply", async { Ok(()) }).await.is_ok());
        assert_eq!(DESTROY_TIMEOUT, Duration::from_secs(1800));
    }

    #[tokio::test]
    async fn every_run_needs_terraform_and_says_where_to_get_it() {
        let dir = std::env::temp_dir().join(format!("cyberctf-tf-prep-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("main.tf"), "").unwrap();
        let state = dir.join("state");
        let _tools = crate::exec::fake::Tools::rules(&[("terraform version", None)]);
        let err = prepare(&dir, &state).await.unwrap_err().to_string();
        assert_eq!(err, NOT_INSTALLED);
        assert!(!state.exists(), "refused before anything changed");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn every_run_sets_the_data_dir_and_automation() {
        let env = run_env(&[("TF_VAR_x".into(), "1".into())], Path::new("/s"));
        assert_eq!(env[0], ("TF_VAR_x".to_string(), "1".to_string()));
        assert!(env.contains(&("TF_DATA_DIR".to_string(), Path::new("/s").join(".terraform").display().to_string())));
        assert!(env.contains(&("TF_IN_AUTOMATION".to_string(), "1".to_string())));
        assert_eq!(backend_arg(Path::new("/s")), format!("-backend-config=path={}", Path::new("/s").join("terraform.tfstate").display()));
    }

    #[test]
    fn the_container_gets_mounts_and_variable_names_only() {
        let env = vec![("TF_VAR_password".to_string(), "s3cret".to_string())];
        let args = container_args("/lab/m", &["/lab".into(), "/state".into()], &env, &["apply", "-auto-approve"]);
        assert_eq!(
            args,
            ["run", "--rm", "-i", "-w", "/lab/m", "-v", "/lab:/lab", "-v", "/state:/state", "-e", "TF_VAR_password", TERRAFORM_IMAGE, "apply", "-auto-approve"]
        );
        assert!(!args.iter().any(|a| a.contains("s3cret")));
        assert_eq!(as_strs(&["a".to_string()]), ["a"]);
    }

    #[tokio::test]
    async fn an_init_failure_reads_the_same_in_the_container_and_on_the_host() {
        // Regression: the container's `init` skipped `init_failure`, so an offline destroy read
        // differently depending on where Terraform ran.
        let _tools = crate::exec::fake::Tools::rules(&[("init", crate::exec::fake::fail("dial tcp: lookup registry.terraform.io: no such host"))]);
        for program in ["terraform", "docker"] {
            let err = init(program, &["init"], Path::new("."), &[], |_| {}).await.unwrap_err().to_string();
            assert!(err.starts_with("Couldn't download the Terraform provider plugins"), "{program}: {err}");
        }
    }

    #[test]
    fn init_failures_name_the_download() {
        let offline = init_failure(Error::CommandFailed { command: "terraform init".into(), stderr: "no such host".into() });
        assert!(offline.to_string().contains("provider plugins"), "{offline}");
        let other = init_failure(Error::Invalid("Unsupported argument".into()));
        assert_eq!(other.to_string(), Error::Invalid("Unsupported argument".into()).to_string());
    }

    #[tokio::test]
    async fn a_lab_without_a_module_is_refused_before_terraform_runs() {
        let dir = std::env::temp_dir().join(format!("cyberctf-tf-nomodule-{}", rand::random::<u32>()));
        let err = terraform(&dir, &dir.join("state"), &[], "apply", |_| {}).await.unwrap_err();
        assert!(err.to_string().contains("no Terraform module"), "{err}");
        assert!(!dir.exists(), "nothing is created");
    }

    #[test]
    fn a_module_is_a_folder_with_tf_files() {
        let dir = std::env::temp_dir().join(format!("cyberctf-tf-mod-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_module(&dir));
        assert!(!is_module(&dir.join("missing")));
        // AWSGoat's root module has no main.tf, only other .tf files.
        std::fs::write(dir.join("providers.tf"), "").unwrap();
        assert!(is_module(&dir));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn download_failures_are_told_apart() {
        assert!(is_download_failure("Failed to query available provider packages: could not connect to registry.terraform.io"));
        assert!(is_download_failure("dial tcp: lookup registry: no such host"));
        assert!(!is_download_failure("Error: Unsupported argument"));
    }
}
