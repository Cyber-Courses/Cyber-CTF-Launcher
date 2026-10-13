//! Running `terraform init` + a command on a module: the host binary, or on macOS for Proxmox
//! the official container (see `in_container`). Variables reach it through the environment.

use std::path::Path;

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

/// Runs `terraform init` then `command` (apply / destroy, auto-approved) on `module`, with its
/// state in `state` and `env` as the full environment (variables as `TF_VAR_*`).
pub(super) async fn terraform(module: &Path, state: &Path, env: &[(String, String)], command: &str, log: impl FnMut(String)) -> Result<()> {
    if !module.join("main.tf").is_file() {
        return Err(Error::Invalid(format!("this lab has no Terraform module at {}", module.display())));
    }
    if run_read("terraform", &["version"], None).await.is_err() {
        return Err(Error::Invalid("Terraform isn't installed. Install it from the server setup (Tools on this machine).".into()));
    }
    std::fs::create_dir_all(state)?;
    // A crashed or killed terraform can leave the local backend's lock file behind. The launcher
    // serializes operations on a lab (lab_lock) and is the only writer of this state, so any lock
    // present now is stale and would otherwise block this op with "Error acquiring the state
    // lock". Clear it before init.
    let _ = std::fs::remove_file(state.join(".terraform.tfstate.lock.info"));
    if in_container(module).await {
        return terraform_container(module, state, env, command, log).await;
    }
    terraform_host(module, state, env, command, log).await
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

/// `env` plus what every run sets: the plugin cache beside the state, and automation mode.
pub(super) fn run_env(env: &[(String, String)], state: &Path) -> Vec<(String, String)> {
    let mut full = env.to_vec();
    full.push(("TF_DATA_DIR".to_string(), state.join(".terraform").display().to_string()));
    full.push(("TF_IN_AUTOMATION".to_string(), "1".to_string()));
    full
}

/// `init`'s flag pointing the local backend at the state folder.
fn backend_arg(state: &Path) -> String {
    format!("-backend-config=path={}", state.join(STATE_FILE).display())
}

/// Folders to mount into the container: the whole lab, the state, and the folders of files a
/// variable names (`*_file`, e.g. the SSH key), sorted and deduplicated.
fn container_mounts(module: &Path, state: &Path, env: &[(String, String)]) -> Vec<String> {
    // The whole lab, not just the module: Isoloom modules reach the rest of the lab through
    // `path.module/..` (the Proxmox module tars the lab folder to upload it; mounting only the
    // module left the archive without the lab's compose file).
    let mut mounts: Vec<String> = vec![lab_root(module).display().to_string(), state.display().to_string()];
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
    let init = docker_args(&["init", "-input=false", "-no-color", &backend]);
    stream("docker", &as_strs(&init), Some(dir), &full, &mut log).await?;
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
pub(super) fn init_failure(e: Error) -> Error {
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
    stream("terraform", &["init", "-input=false", "-no-color", backend.as_str()], Some(dir), &full, &mut log).await.map_err(init_failure)?;
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
    fn download_failures_are_told_apart() {
        assert!(is_download_failure("Failed to query available provider packages: could not connect to registry.terraform.io"));
        assert!(is_download_failure("dial tcp: lookup registry: no such host"));
        assert!(!is_download_failure("Error: Unsupported argument"));
    }
}
