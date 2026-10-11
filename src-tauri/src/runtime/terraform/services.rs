//! A lab's own Terraform module (cloud-services labs). It is used as is: it has no backend block
//! pointing at the launcher's state, so the state is named with `-state` (the local backend's
//! own option, as Isoloom's generated `up.sh` does), the plugins go under the state folder
//! (TF_DATA_DIR), and the variables come from var files (the lab's fixed values, then the
//! launch-time inputs) plus `TF_VAR_*` for the launcher's own (`expires_at`, `region`), which
//! Terraform ignores when the module doesn't declare them.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::runner::{init_failure, run_env};
use super::state::{EXPIRES_AT, RUN_FILE, STATE_FILE, saved_vars};
use super::with_env;
use crate::error::{Error, Result};
use crate::exec::{run_read, stream};

/// Whether a folder is a Terraform module: it holds at least one `.tf` file.
pub fn is_module(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|d| d.flatten().any(|e| e.path().extension().is_some_and(|x| x == "tf")))
}

/// Applies a lab's own module. `vars` are the launcher's variables (`TF_VAR_*`), `env` the
/// cloud's credentials, `expires_at` when the lab auto-stops.
pub async fn apply_services(
    module: &Path,
    state: &Path,
    var_files: &[PathBuf],
    vars: &[(String, String)],
    expires_at: Option<u64>,
    env: &[(String, String)],
    log: impl FnMut(String),
) -> Result<()> {
    std::fs::create_dir_all(state)?;
    std::fs::write(state.join(RUN_FILE), serde_json::to_string(&run_record(expires_at, vars)).unwrap_or_default())?;
    run(module, state, var_files, &with_env(vars, env), "apply", log).await
}

/// Destroys what [`apply_services`] created, with the variables it recorded.
pub async fn destroy_services(module: &Path, state: &Path, var_files: &[PathBuf], env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    if !state.join(STATE_FILE).is_file() {
        if state.join(RUN_FILE).is_file() {
            log("No Terraform state for this lab (it may have been reset). If resources were created, check your account and remove them there.".into());
        }
        return Ok(());
    }
    let vars = std::fs::read_to_string(state.join(RUN_FILE)).map(|raw| replayed_vars(&raw)).unwrap_or_default();
    run(module, state, var_files, &with_env(&vars, env), "destroy", log).await?;
    let _ = std::fs::remove_file(state.join(STATE_FILE));
    Ok(())
}

/// `terraform init` then `command` on the lab's module, its state in `state`.
async fn run(module: &Path, state: &Path, var_files: &[PathBuf], env: &[(String, String)], command: &str, mut log: impl FnMut(String)) -> Result<()> {
    check_can_run(module).await?;
    std::fs::create_dir_all(state)?;
    // The launcher is this state's only writer (see `runner::terraform`): a lock left now is stale.
    let _ = std::fs::remove_file(state.join(".terraform.tfstate.lock.info"));
    let full = run_env(env, state);
    stream("terraform", &["init", "-input=false", "-no-color"], Some(module), &full, &mut log).await.map_err(init_failure)?;
    let args = command_args(command, state, var_files);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    stream("terraform", &refs, Some(module), &full, log).await
}

/// Refuses a run without a module or without Terraform on this machine.
async fn check_can_run(module: &Path) -> Result<()> {
    if !is_module(module) {
        return Err(Error::Invalid(format!("this lab has no Terraform module at {}", module.display())));
    }
    if run_read("terraform", &["version"], None).await.is_err() {
        return Err(Error::Invalid("Terraform isn't installed. Install it from the Machine page (Tools on this machine).".into()));
    }
    Ok(())
}

/// The arguments of an `apply` or `destroy`: the state file, then the var files.
fn command_args(command: &str, state: &Path, var_files: &[PathBuf]) -> Vec<String> {
    let mut args: Vec<String> = vec![command.into(), "-auto-approve".into(), "-input=false".into(), "-no-color".into()];
    args.push(format!("-state={}", state.join(STATE_FILE).display()));
    args.extend(var_files.iter().map(|f| format!("-var-file={}", f.display())));
    args
}

/// The run record of a deploy: when it ends (`expires_at`, for status and the reaper) and the
/// launcher variables a destroy passes again.
fn run_record(expires_at: Option<u64>, vars: &[(String, String)]) -> serde_json::Map<String, Value> {
    let mut run: serde_json::Map<String, Value> = vars.iter().filter(|(k, _)| k == "region").map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect();
    if let Some(t) = expires_at {
        run.insert(EXPIRES_AT.into(), Value::from(t));
    }
    run.insert("cloud_services".into(), Value::Bool(true));
    run
}

/// The launcher variables of a destroy, from the run record: the same `region` and
/// `expires_at` the apply had (a module may require them even to destroy).
fn replayed_vars(run_json: &str) -> Vec<(String, String)> {
    let mut vars = saved_vars(run_json);
    if let Ok(v) = serde_json::from_str::<Value>(run_json)
        && let Some(t) = v[EXPIRES_AT].as_u64()
    {
        vars.push(("expires_at".into(), t.to_string()));
    }
    vars.retain(|(k, _)| k == "region" || k == "expires_at");
    vars
}

#[cfg(test)]
mod tests {
    use super::super::state::read_state;
    use super::*;

    #[test]
    fn a_labs_own_module_runs_on_the_launchers_state_and_var_files() {
        let state = Path::new("/d/deployments/goat/aws");
        let files = vec![PathBuf::from("/d/labs/goat/.isoloom/cloud-services/terraform.tfvars.json"), state.join("inputs.tfvars.json")];
        assert_eq!(
            command_args("apply", state, &files),
            [
                "apply",
                "-auto-approve",
                "-input=false",
                "-no-color",
                "-state=/d/deployments/goat/aws/terraform.tfstate",
                "-var-file=/d/labs/goat/.isoloom/cloud-services/terraform.tfvars.json",
                "-var-file=/d/deployments/goat/aws/inputs.tfvars.json",
            ]
        );
        assert_eq!(command_args("destroy", state, &[])[0], "destroy");
    }

    #[test]
    fn a_cloud_services_record_replays_region_and_expiry_on_destroy() {
        let vars = vec![("region".to_string(), "eu-west-3".to_string()), ("expires_at".to_string(), "5000".to_string())];
        let run = run_record(Some(5000), &vars);
        assert_eq!(run.get(EXPIRES_AT).and_then(Value::as_u64), Some(5000));
        assert_eq!(run.get("region").and_then(Value::as_str), Some("eu-west-3"));
        let json = serde_json::to_string(&run).unwrap();
        let mut back = replayed_vars(&json);
        back.sort();
        assert_eq!(back, vec![("expires_at".to_string(), "5000".to_string()), ("region".to_string(), "eu-west-3".to_string())]);
        // No auto-stop: no expiry recorded nor replayed.
        let json = serde_json::to_string(&run_record(None, &[])).unwrap();
        assert!(replayed_vars(&json).is_empty());
        assert!(replayed_vars("garbage").is_empty());
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

    #[tokio::test]
    async fn destroying_without_state_is_a_no_op_that_warns_when_a_deploy_was_recorded() {
        let state = std::env::temp_dir().join(format!("cyberctf-tf-svc-none-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&state).unwrap();
        let mut lines = Vec::new();
        destroy_services(&state.join("module"), &state, &[], &[], |l| lines.push(l)).await.unwrap();
        assert!(lines.is_empty());
        std::fs::write(state.join(RUN_FILE), "{}").unwrap();
        destroy_services(&state.join("module"), &state, &[], &[], |l| lines.push(l)).await.unwrap();
        assert_eq!(lines.len(), 1);
        std::fs::remove_dir_all(state).unwrap();
    }

    #[tokio::test]
    async fn applying_records_the_run_then_needs_the_module() {
        let state = std::env::temp_dir().join(format!("cyberctf-tf-svc-nomod-{}", rand::random::<u32>()));
        let vars = vec![("region".to_string(), "us-east-1".to_string())];
        let err = apply_services(&state.join("module"), &state, &[], &vars, Some(42), &[], |_| {}).await.unwrap_err();
        assert!(err.to_string().contains("no Terraform module"), "{err}");
        assert_eq!(super::super::state::expires_at(&state), Some(42));
        std::fs::remove_dir_all(state).unwrap();
    }

    /// A lab's own module through apply and destroy with the host Terraform, no cloud: a module of
    /// `terraform_data` only (built in, nothing to download). Opt-in (needs `terraform`):
    ///   cargo test services_apply_and_destroy_locally -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn services_apply_and_destroy_locally() {
        let root = std::env::temp_dir().join(format!("cyberctf-tf-svc-{}", rand::random::<u32>()));
        let (module, state) = (root.join("lab/terraform"), root.join("deployments/lab/aws"));
        std::fs::create_dir_all(&module).unwrap();
        std::fs::write(
            module.join("main.tf"),
            "variable \"expires_at\" {}\nvariable \"whitelist\" {}\nvariable \"size\" {}\nresource \"terraform_data\" \"svc\" { input = \"${var.whitelist}:${var.size}:${var.expires_at}\" }\noutput \"svc\" { value = terraform_data.svc.output }\noutput \"key\" {\n  value     = \"s3cr3t\"\n  sensitive = true\n}\n",
        )
        .unwrap();
        let fixed = root.join("lab/terraform.tfvars.json");
        std::fs::write(&fixed, r#"{"size": 2}"#).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("inputs.tfvars.json"), r#"{"whitelist": "203.0.113.7/32"}"#).unwrap();
        let files = vec![fixed, state.join("inputs.tfvars.json")];
        // `region` isn't declared by the module: as TF_VAR_ it is ignored, not an error.
        let vars = vec![("expires_at".to_string(), "4102444800".to_string()), ("region".to_string(), "eu-west-3".to_string())];
        let print = |l: String| println!("{l}");
        apply_services(&module, &state, &files, &vars, Some(4_102_444_800), &[], print).await.expect("apply");
        let s = read_state(&state).expect("state beside the launcher's other deployments");
        assert_eq!(s["outputs"]["svc"]["value"], "203.0.113.7/32:2:4102444800");
        assert_eq!(s["outputs"]["key"]["sensitive"], true);
        assert!(!module.join("terraform.tfstate").exists());
        destroy_services(&module, &state, &files, &[], print).await.expect("destroy");
        assert!(read_state(&state).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
