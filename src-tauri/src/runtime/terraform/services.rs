//! A lab's own Terraform module (cloud-services labs). It is used as is: it has no backend block
//! pointing at the launcher's state, so the state is named with `-state` (the local backend's
//! own option, as Isoloom's generated `up.sh` does), its data goes under the state folder
//! (TF_DATA_DIR, the plugins linked from the shared cache), and the variables come from var files (the lab's fixed values, then the
//! launch-time inputs) plus `TF_VAR_*` for the launcher's own (`expires_at`, `region`), which
//! Terraform ignores when the module doesn't declare them.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::runner::{bounded, init, prepare, run_env};
use super::state::{EXPIRES_AT, RUN_FILE, STATE_FILE, record_env, saved_env, saved_vars, with_saved};
use super::{forget_state, recorded_deploy, with_env};
use crate::error::Result;
use crate::exec::stream;

/// Applies a lab's own module. `vars` are the launcher's variables (`TF_VAR_*`), `env` the
/// cloud's credentials, `expires_at` when the lab auto-stops, `retries` how many more applies
/// after a failed one (`cloud.apply_retries`: a module that converges on a second apply). The
/// last failure is the run's.
#[allow(clippy::too_many_arguments)]
pub async fn apply_services(
    module: &Path,
    state: &Path,
    var_files: &[PathBuf],
    vars: &[(String, String)],
    expires_at: Option<u64>,
    retries: u8,
    env: &[(String, String)],
    mut log: impl FnMut(String),
) -> Result<()> {
    std::fs::create_dir_all(state)?;
    std::fs::write(state.join(RUN_FILE), serde_json::to_string(&run_record(expires_at, vars, env)).unwrap_or_default())?;
    let full = initialized(module, state, &with_env(vars, env), &mut log).await?;
    let mut attempt = 0;
    loop {
        match command(module, state, var_files, &full, "apply", &mut log).await {
            Err(e) if attempt < retries => {
                attempt += 1;
                log(format!("The apply failed ({e}); this lab's module may need another one: applying again ({attempt} of {retries})…"));
            }
            done => return done,
        }
    }
}

/// Destroys what [`apply_services`] created, with the variables and the placement (region,
/// project) it recorded. Idempotent: with no state left, there is nothing to do.
pub async fn destroy_services(module: &Path, state: &Path, var_files: &[PathBuf], env: &[(String, String)], mut log: impl FnMut(String)) -> Result<()> {
    let Some(raw) = recorded_deploy(state, &mut log) else { return Ok(()) };
    let env = with_saved(env, saved_env(&raw));
    let full = with_env(&replayed_vars(&raw), &env);
    bounded("destroy", async {
        let full = initialized(module, state, &full, &mut log).await?;
        command(module, state, var_files, &full, "destroy", log).await
    })
    .await?;
    forget_state(state);
    Ok(())
}

/// The checks every run starts with, then `terraform init` on the lab's module, its state in
/// `state`; the full environment of the run.
async fn initialized(module: &Path, state: &Path, env: &[(String, String)], log: impl FnMut(String)) -> Result<Vec<(String, String)>> {
    prepare(module, state).await?;
    let full = run_env(env, state);
    init("terraform", &["init", "-input=false", "-no-color"], module, &full, log).await?;
    Ok(full)
}

/// `terraform <command>` (apply, destroy) on the initialized module, with the full environment.
async fn command(module: &Path, state: &Path, var_files: &[PathBuf], full: &[(String, String)], command: &str, log: impl FnMut(String)) -> Result<()> {
    let args = command_args(command, state, var_files);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    stream("terraform", &refs, Some(module), full, log).await
}

/// The arguments of an `apply` or `destroy`: the state file, then the var files.
fn command_args(command: &str, state: &Path, var_files: &[PathBuf]) -> Vec<String> {
    let mut args: Vec<String> = vec![command.into(), "-auto-approve".into(), "-input=false".into(), "-no-color".into()];
    args.push(format!("-state={}", state.join(STATE_FILE).display()));
    args.extend(var_files.iter().map(|f| format!("-var-file={}", f.display())));
    args
}

/// The run record of a deploy: when it ends (`expires_at`, for status and the reaper), the
/// launcher variables a destroy passes again, and the provider environment that says where its
/// resources are (the region, the GCP project).
fn run_record(expires_at: Option<u64>, vars: &[(String, String)], env: &[(String, String)]) -> serde_json::Map<String, Value> {
    let mut run: serde_json::Map<String, Value> = vars.iter().filter(|(k, _)| k == "region").map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect();
    if let Some(t) = expires_at {
        run.insert(EXPIRES_AT.into(), Value::from(t));
    }
    run.insert("cloud_services".into(), Value::Bool(true));
    record_env(&mut run, env);
    run
}

/// The launcher variables of a destroy, from the run record: the same `region` and
/// `expires_at` the apply had (a module may require them even to destroy); no auto-stop is the
/// empty `expires_at` the apply passed.
fn replayed_vars(run_json: &str) -> Vec<(String, String)> {
    let mut vars = saved_vars(run_json);
    vars.retain(|(k, _)| k == "region");
    let expires_at = serde_json::from_str::<Value>(run_json).ok().and_then(|v| v[EXPIRES_AT].as_u64());
    vars.push(("expires_at".into(), expires_at.map(|t| t.to_string()).unwrap_or_default()));
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
        // Paths as the platform joins them (backslashes on Windows).
        let path = |p: &Path| p.display().to_string();
        assert_eq!(
            command_args("apply", state, &files),
            [
                "apply".to_string(),
                "-auto-approve".into(),
                "-input=false".into(),
                "-no-color".into(),
                format!("-state={}", path(&state.join("terraform.tfstate"))),
                format!("-var-file={}", path(&files[0])),
                format!("-var-file={}", path(&files[1])),
            ]
        );
        assert_eq!(command_args("destroy", state, &[])[0], "destroy");
    }

    #[test]
    fn a_cloud_services_record_replays_region_and_expiry_on_destroy() {
        let vars = vec![("region".to_string(), "eu-west-3".to_string()), ("expires_at".to_string(), "5000".to_string())];
        let run = run_record(Some(5000), &vars, &[]);
        assert_eq!(run.get(EXPIRES_AT).and_then(Value::as_u64), Some(5000));
        assert_eq!(run.get("region").and_then(Value::as_str), Some("eu-west-3"));
        let json = serde_json::to_string(&run).unwrap();
        let mut back = replayed_vars(&json);
        back.sort();
        assert_eq!(back, vec![("expires_at".to_string(), "5000".to_string()), ("region".to_string(), "eu-west-3".to_string())]);
        // No auto-stop: no expiry recorded, and the apply's empty one replayed (a module that
        // declares it requires a value even to destroy).
        let json = serde_json::to_string(&run_record(None, &[], &[])).unwrap();
        assert!(!json.contains(EXPIRES_AT));
        assert_eq!(replayed_vars(&json), [("expires_at".to_string(), String::new())]);
        assert_eq!(replayed_vars("garbage"), [("expires_at".to_string(), String::new())]);
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
        let err = apply_services(&state.join("module"), &state, &[], &vars, Some(42), 2, &[], |_| {}).await.unwrap_err();
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
        apply_services(&module, &state, &files, &vars, Some(4_102_444_800), 0, &[], print).await.expect("apply");
        let s = read_state(&state).expect("state beside the launcher's other deployments");
        assert_eq!(s["outputs"]["svc"]["value"], "203.0.113.7/32:2:4102444800");
        assert_eq!(s["outputs"]["key"]["sensitive"], true);
        assert!(!module.join("terraform.tfstate").exists());
        destroy_services(&module, &state, &files, &[], print).await.expect("destroy");
        assert!(read_state(&state).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
