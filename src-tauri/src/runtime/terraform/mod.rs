//! Terraform runs of a lab's Isoloom module (`.isoloom/docker-vm/proxmox`, `.isoloom/proxmox`,
//! `.isoloom/cloud-docker/<cloud>`: Proxmox servers and the clouds). A local `terraform` binary
//! is required; state is kept as plain files on the host. A containerised terraform would be
//! simpler to ship but couldn't reach the host's cloud CLI auth (`~/.aws`, `az login` /
//! `~/.azure`, gcloud ADC), so the cloud targets need the host binary. Variables reach it as
//! `TF_VAR_*` through the environment (never on the command line).
//!
//! State lives outside the lab folder (`state_dir`), because reinstalling a lab at a new
//! commit replaces that folder and must not orphan the VM it created.
//!
//! - this module: `apply` / `destroy` and the variables they pass.
//! - `runner`: running terraform itself (host binary or container), and what every run shares.
//! - `ready`: waiting for the lab host's bootstrap after apply.
//! - `state`: what the state files say (status, outputs, SSH target, expiry).
//! - `services`: a cloud-services lab's own module, applied and destroyed as is.

mod ready;
mod runner;
mod services;
mod state;

use std::path::{Path, PathBuf};

#[cfg(test)]
pub(in crate::runtime) use runner::DESTROY_TIMEOUT;
#[cfg(test)]
pub use runner::is_module;
pub use runner::{PLUGIN_CACHE, clear_plugin_cache};
pub use services::{apply_services, destroy_services};
pub use state::{JUMP_FILE, deployed, expired, expires_at, has_resources, has_tainted, known_hosts, now, read_state, ssh_endpoint, ssh_target, status};

use self::ready::wait_ready;
use self::runner::{DATA_DIR, terraform};
use self::state::{RUN_FILE, STATE_FILE, run_record, saved_env, saved_vars, with_saved};
use super::ssh;
use crate::error::Result;

/// What a destroy says when a deploy was recorded but its state is gone.
const NO_STATE: &str = "No Terraform state for this lab (it may have been reset). If resources were created, check your account and remove them there.";

/// Creates (or updates) the target's resources. `vars` are Terraform variable names;
/// `env` is raw environment the provider reads itself (cloud credentials).
pub async fn apply(module: &Path, state: &Path, vars: &[(String, String)], env: &[(String, String)], log: impl FnMut(String)) -> Result<()> {
    std::fs::create_dir_all(state)?;
    let mut log = log;
    let vars = with_ssh_key(vars, ssh::launcher_key());
    // Remember the non-secret variables and where the resources go, so a later destroy has them
    // without the launch spec, and reaches them even once the account's settings changed.
    let run = run_record(&vars, env, now());
    std::fs::write(state.join(RUN_FILE), serde_json::to_string(&run).unwrap_or_default())?;
    terraform(module, state, &with_env(&vars, env), "apply", &mut log).await?;
    wait_ready(state, &mut log).await
}

/// Destroys everything the target created. `vars` and `env` carry the connection again (the
/// provider needs it); the lab variables and the placement (region, project, node…) are the
/// ones the last apply recorded, whatever the account says now. Idempotent: with no state left,
/// there is nothing to do.
pub async fn destroy(module: &Path, state: &Path, vars: &[(String, String)], env: &[(String, String)], log: impl FnMut(String)) -> Result<()> {
    let mut log = log;
    let Some(raw) = recorded_deploy(state, &mut log) else { return Ok(()) };
    let all = with_saved(vars, saved_vars(&raw));
    let env = with_saved(env, saved_env(&raw));
    let key = ssh::launcher_key();
    let public = key.as_ref().and_then(|k| std::fs::read_to_string(k.with_extension("pub")).ok());
    let all = with_ssh_key(&with_destroy_keys(all, key.clone(), public), key);
    terraform(module, state, &with_env(&all, &env), "destroy", log).await?;
    forget_state(state);
    Ok(())
}

/// The run record of a deployment that still has a state to destroy (`""` when it has none, from
/// an older version). None when there is no state: nothing to destroy from here, but resources
/// may still exist on the account when a deploy was recorded (the app data was wiped), and `log`
/// says so rather than reporting a clean stop.
fn recorded_deploy(state: &Path, log: &mut impl FnMut(String)) -> Option<String> {
    if !state.join(STATE_FILE).is_file() {
        if state.join(RUN_FILE).is_file() {
            log(NO_STATE.into());
        }
        return None;
    }
    Some(std::fs::read_to_string(state.join(RUN_FILE)).unwrap_or_default())
}

/// What a successful destroy leaves no use for: the state, the run's data folder (its modules
/// and the plugins it linked), and the per-deployment known_hosts (the next lab can get a
/// recycled host IP with a new host key, and a stale pinned key would make its SSH fail).
fn forget_state(state: &Path) {
    let _ = std::fs::remove_file(state.join(STATE_FILE));
    let _ = std::fs::remove_dir_all(state.join(DATA_DIR));
    let _ = std::fs::remove_file(known_hosts(state));
}

/// Modules validate the SSH key variables even to destroy, and older runs didn't save them: the
/// launcher's key (`key`, its public half `public`) fills in what's missing.
fn with_destroy_keys(mut vars: Vec<(String, String)>, key: Option<PathBuf>, public: Option<String>) -> Vec<(String, String)> {
    let has = |vars: &[(String, String)], name: &str| vars.iter().any(|(k, _)| k == name);
    if !has(&vars, "ssh_public_key")
        && let Some(public) = public
    {
        vars.push(("ssh_public_key".into(), public.trim().to_string()));
    }
    if !has(&vars, "ssh_private_key_file")
        && let Some(key) = key
    {
        vars.push(("ssh_private_key_file".into(), key.to_string_lossy().to_string()));
    }
    vars
}

/// Proxmox token setups upload the cloud-init snippet over SSH with the launcher's key
/// (a token can't SSH): point the module at it.
fn with_ssh_key(vars: &[(String, String)], key: Option<PathBuf>) -> Vec<(String, String)> {
    let mut out = vars.to_vec();
    let token = vars.iter().any(|(k, v)| k == "proxmox_api_token" && !v.is_empty());
    let set = vars.iter().any(|(k, _)| k == "proxmox_ssh_private_key_file");
    if let (true, false, Some(key)) = (token, set, key) {
        out.push(("proxmox_ssh_private_key_file".into(), key.to_string_lossy().to_string()));
    }
    out
}

/// The run's environment: variables as `TF_VAR_<name>`, then the raw provider environment. The
/// launcher's variables always win: an environment entry naming one of them is left out.
fn with_env(vars: &[(String, String)], env: &[(String, String)]) -> Vec<(String, String)> {
    let vars: Vec<(String, String)> = vars.iter().map(|(k, v)| (format!("TF_VAR_{k}"), v.clone())).collect();
    let others = env.iter().filter(|(k, _)| !vars.iter().any(|(v, _)| v.eq_ignore_ascii_case(k))).cloned();
    vars.iter().cloned().chain(others).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launchers_variables_win_over_the_environment() {
        let vars = [pair("expires_at", "9"), pair("region", "eu-west-3")];
        let env = [pair("TF_VAR_expires_at", ""), pair("tf_var_region", "us-east-1"), pair("AWS_REGION", "us-east-1")];
        assert_eq!(with_env(&vars, &env), [pair("TF_VAR_expires_at", "9"), pair("TF_VAR_region", "eu-west-3"), pair("AWS_REGION", "us-east-1")]);
    }

    /// A fresh, empty state folder in the system temp dir.
    pub fn temp_state() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cyberctf-tf-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn pair(k: &str, v: &str) -> (String, String) {
        (k.to_string(), v.to_string())
    }

    #[tokio::test]
    async fn destroying_without_state_is_a_no_op_that_warns_when_a_deploy_was_recorded() {
        let state = temp_state();
        let mut lines = Vec::new();
        destroy(&state.join("module"), &state, &[], &[], |l| lines.push(l)).await.unwrap();
        assert!(lines.is_empty(), "never deployed: nothing to say");
        std::fs::write(state.join(RUN_FILE), "{}").unwrap();
        destroy(&state.join("module"), &state, &[], &[], |l| lines.push(l)).await.unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("No Terraform state for this lab"));
        std::fs::remove_dir_all(state).unwrap();
    }

    #[tokio::test]
    async fn destroying_with_state_needs_the_labs_module_and_keeps_the_state() {
        let state = temp_state();
        std::fs::write(state.join(STATE_FILE), "{}").unwrap();
        std::fs::write(state.join(RUN_FILE), r#"{"allowed_cidr":"1.2.3.4/32"}"#).unwrap();
        let err = destroy(&state.join("module"), &state, &[pair("region", "r")], &[], |_| {}).await.unwrap_err();
        assert!(err.to_string().contains("no Terraform module"), "{err}");
        // The state is kept for a retry once the module is back.
        assert!(state.join(STATE_FILE).is_file());
        std::fs::remove_dir_all(state).unwrap();
    }

    #[tokio::test]
    async fn apply_records_the_run_before_terraform() {
        let state = temp_state().join("nested");
        let vars = vec![pair("ssh_public_key", "ssh-ed25519 AAAA"), pair("proxmox_password", "secret")];
        let err = apply(&state.join("module"), &state, &vars, &[], |_| {}).await.unwrap_err();
        assert!(err.to_string().contains("no Terraform module"), "{err}");
        let run = std::fs::read_to_string(state.join(RUN_FILE)).unwrap();
        assert!(run.contains("ssh-ed25519 AAAA") && !run.contains("secret"), "{run}");
        std::fs::remove_dir_all(state.parent().unwrap()).unwrap();
    }

    /// A module folder (one `.tf` file) and a state holding a deployment recorded at apply with
    /// `run` (its run record).
    fn deployed(run: &str) -> (PathBuf, PathBuf) {
        let root = temp_state();
        let (module, state) = (root.join("module"), root.join("deployments/web/aws"));
        std::fs::create_dir_all(&module).unwrap();
        std::fs::write(module.join("main.tf"), "").unwrap();
        std::fs::create_dir_all(state.join(DATA_DIR).join("providers")).unwrap();
        std::fs::write(state.join(STATE_FILE), r#"{"resources":[{"type":"aws_instance"}]}"#).unwrap();
        std::fs::write(state.join(RUN_FILE), run).unwrap();
        (module, state)
    }

    #[tokio::test]
    async fn a_destroy_goes_where_the_apply_went_after_the_account_moved() {
        // Regression: Stop rebuilt the place from the account as it is now. The player moved the
        // account to us-east-1 for their next lab: Terraform refreshed there, found nothing, and
        // the destroy "succeeded" while the instance kept billing in eu-west-3.
        let (module, state) = deployed(r#"{"region":"eu-west-3","env":{"AWS_REGION":"eu-west-3"}}"#);
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        let now = [pair("region", "us-east-1"), pair("auto_stop_hours", "4")];
        let env = [pair("AWS_REGION", "us-east-1"), pair("AWS_SECRET_ACCESS_KEY", "s3cret")];
        destroy(&module, &state, &now, &env, |_| {}).await.unwrap();
        assert!(tools.called("terraform destroy -auto-approve"), "{:?}", tools.calls());
        assert_eq!(tools.env_of("terraform destroy", "TF_VAR_region").as_deref(), Some("eu-west-3"));
        assert_eq!(tools.env_of("terraform destroy", "AWS_REGION").as_deref(), Some("eu-west-3"));
        // The credentials are still the account's (never kept beside the state).
        assert_eq!(tools.env_of("terraform destroy", "AWS_SECRET_ACCESS_KEY").as_deref(), Some("s3cret"));
        // Gone: the state, and the run's data folder with the plugins it linked.
        assert!(!state.join(STATE_FILE).exists() && !state.join(DATA_DIR).exists());
        std::fs::remove_dir_all(state.ancestors().nth(3).unwrap()).unwrap();
    }

    #[tokio::test]
    async fn a_failed_destroy_keeps_everything_for_a_retry_and_a_second_stop_is_a_no_op() {
        let (module, state) = deployed("{}");
        let failing = crate::platform::exec::fake::Tools::rules(&[(
            "terraform destroy",
            crate::platform::exec::fake::fail("ExpiredToken: the security token is expired"),
        )]);
        let err = destroy(&module, &state, &[], &[], |_| {}).await.unwrap_err();
        assert!(err.to_string().contains("ExpiredToken"), "{err}");
        assert!(state.join(STATE_FILE).is_file() && state.join(DATA_DIR).is_dir());
        drop(failing);
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        destroy(&module, &state, &[], &[], |_| {}).await.unwrap();
        // Idempotent: nothing left, nothing run.
        let runs = tools.calls().len();
        destroy(&module, &state, &[], &[], |_| {}).await.unwrap();
        assert_eq!(tools.calls().len(), runs);
        std::fs::remove_dir_all(state.ancestors().nth(3).unwrap()).unwrap();
    }

    #[tokio::test]
    async fn an_apply_records_where_it_creates_the_lab() {
        let root = temp_state();
        let (module, state) = (root.join("module"), root.join("state"));
        std::fs::create_dir_all(&module).unwrap();
        std::fs::write(module.join("main.tf"), "").unwrap();
        let _tools = crate::platform::exec::fake::Tools::rules(&[("terraform apply", crate::platform::exec::fake::fail("quota"))]);
        let vars = [pair("region", "eu-west-3"), pair("project", "labs-1"), pair("proxmox_password", "pw")];
        assert!(apply(&module, &state, &vars, &[pair("GOOGLE_PROJECT", "labs-1"), pair("LINODE_TOKEN", "tok")], |_| {}).await.is_err());
        let raw = std::fs::read_to_string(state.join(RUN_FILE)).unwrap();
        assert!(raw.contains("eu-west-3") && raw.contains("labs-1") && !raw.contains("pw") && !raw.contains("tok"), "{raw}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn with_env_prefixes_vars_and_appends_raw_env() {
        let vars = vec![pair("region", "eu-west-3")];
        let env = vec![pair("AWS_ACCESS_KEY_ID", "AKIA")];
        let out = with_env(&vars, &env);
        assert!(out.contains(&pair("TF_VAR_region", "eu-west-3")));
        assert!(out.contains(&pair("AWS_ACCESS_KEY_ID", "AKIA")));
    }

    #[test]
    fn token_setups_get_the_launcher_key() {
        let key = Some(PathBuf::from("/k/id_ed25519"));
        let token = vec![pair("proxmox_api_token", "root@pam!x=s")];
        assert!(with_ssh_key(&token, key.clone()).contains(&pair("proxmox_ssh_private_key_file", "/k/id_ed25519")));
        let password = vec![pair("proxmox_password", "p")];
        assert_eq!(with_ssh_key(&password, key), password);
    }

    #[test]
    fn destroy_fills_in_missing_launcher_keys_only() {
        let key = Some(PathBuf::from("/k/id"));
        let public = Some("ssh-ed25519 AAAA cyberctf-launcher\n".to_string());
        let filled = with_destroy_keys(vec![pair("region", "r")], key.clone(), public.clone());
        assert_eq!(filled, [pair("region", "r"), pair("ssh_public_key", "ssh-ed25519 AAAA cyberctf-launcher"), pair("ssh_private_key_file", "/k/id")]);
        // Saved values win.
        let saved = vec![pair("ssh_public_key", "saved"), pair("ssh_private_key_file", "/saved")];
        assert_eq!(with_destroy_keys(saved.clone(), key, public), saved);
        // No launcher key yet: nothing to add.
        assert_eq!(with_destroy_keys(vec![], None, None), vec![]);
    }

    /// The launcher's Proxmox path against a real host (opt-in, slow), with a lab's Isoloom
    /// module (Docker on one VM):
    ///   CYBERCTF_TEST_MODULE=<lab>/.isoloom/docker-vm/proxmox CYBERCTF_TEST_PVE_HOST=... \
    ///   CYBERCTF_TEST_PVE_PASSWORD=... cargo test proxmox_apply_status_destroy -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn proxmox_apply_status_destroy() {
        let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} is required"));
        let host = var("CYBERCTF_TEST_PVE_HOST");
        let state = std::env::temp_dir().join(format!("cyberctf-tf-it-{}", rand::random::<u32>()));
        let module = PathBuf::from(var("CYBERCTF_TEST_MODULE"));
        let (key, public) = (var("CYBERCTF_TEST_SSH_KEY"), std::fs::read_to_string(format!("{}.pub", var("CYBERCTF_TEST_SSH_KEY"))).unwrap());
        let connection: Vec<(String, String)> = [
            ("proxmox_endpoint", format!("https://{host}:8006/")),
            ("proxmox_username", "root@pam".into()),
            ("proxmox_password", var("CYBERCTF_TEST_PVE_PASSWORD")),
            ("proxmox_insecure", "true".into()),
            ("node", std::env::var("CYBERCTF_TEST_PVE_NODE").unwrap_or_else(|_| "pve".into())),
            ("datastore", std::env::var("CYBERCTF_TEST_PVE_STORAGE").unwrap_or_else(|_| "local".into())),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let mut vars = connection.clone();
        vars.extend([("ssh_public_key".to_string(), public.trim().to_string()), ("ssh_private_key_file".into(), key)]);
        let print = |l: String| println!("{l}");

        let applied = apply(&module, &state, &vars, &[], print).await;
        let s = status(&state);
        println!("status after apply: running={} machines={}", s.running, s.machines.len());
        let destroyed = destroy(&module, &state, &connection, &[], print).await;
        let after = status(&state);
        let _ = std::fs::remove_dir_all(&state);
        applied.expect("apply");
        assert!(s.running, "state outputs should show the VM");
        destroyed.expect("destroy");
        assert!(!after.running, "state should be gone after destroy");
    }
}
