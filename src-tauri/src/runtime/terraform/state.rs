//! What a lab's Terraform state says, read from the files only (no Terraform run, so it's cheap
//! to poll): the lab host's address and SSH login, whether it runs, and when it auto-stops. Also
//! the non-secret run record kept beside the state so a destroy can be replayed.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::runtime::lab::outputs as isoloom_outputs;
use crate::runtime::{LabStatus, Machine, ssh};

/// Non-secret run parameters, kept next to the state so `destroy` can be replayed.
pub(super) const RUN_FILE: &str = "run.json";
pub(super) const EXPIRES_AT: &str = "expires_at";
pub(super) const STATE_FILE: &str = "terraform.tfstate";
/// Where the node login for a lab reached through its Proxmox node is kept (`user@host`).
pub const JUMP_FILE: &str = "ssh-jump";

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The variables a run record keeps (none of them secret): the launcher's key and firewall, and
/// where the resources are, so a destroy goes where the apply went even after the account's
/// settings changed (another region, project or Proxmox node) and never "succeeds" against an
/// empty place while the lab keeps billing where it was created.
pub(super) const KEPT_VARS: &[&str] = &[
    // How the launcher reaches the lab host.
    "ssh_public_key",
    "ssh_private_key_file",
    "allowed_cidr",
    // Where a cloud deployment is.
    "region",
    "project",
    "billing_account",
    "org_id",
    "compartment_id",
    // Which Proxmox server, node and storage. Not how to sign in (the login, its TLS setting):
    // those may be renewed since, and the current ones are the ones that work.
    "proxmox_endpoint",
    "proxmox_ssh_address",
    "node",
    "datastore",
    "uplink_bridge",
];

/// The provider environment a run record keeps, for the same reason: the region and project the
/// providers read from it (never a credential).
pub(super) const KEPT_ENV: &[&str] = &["AWS_REGION", "AWS_DEFAULT_REGION", "GOOGLE_PROJECT", "GOOGLE_REGION", "ARM_SUBSCRIPTION_ID"];

/// The run record's object holding the kept environment.
const ENV_KEY: &str = "env";

/// The non-secret run record written beside the state at apply time, so a later destroy can be
/// replayed without the launch spec and against the same place: the kept variables and provider
/// environment (see [`KEPT_VARS`], [`KEPT_ENV`]) plus, for auto-stopping cloud hosts, the
/// computed `expires_at` (seconds since epoch; `now` is the current time).
pub(super) fn run_record(vars: &[(String, String)], env: &[(String, String)], now: u64) -> serde_json::Map<String, Value> {
    let mut run = kept(vars, KEPT_VARS);
    // When the lab host stops itself (cloud auto-stop), so status can tell.
    if let Some(hours) = vars.iter().find(|(k, _)| k == "auto_stop_hours").and_then(|(_, v)| v.parse::<u64>().ok()).filter(|h| *h > 0) {
        run.insert(EXPIRES_AT.into(), Value::from(now.saturating_add(hours.saturating_mul(3600))));
    }
    record_env(&mut run, env);
    run
}

/// The pairs of `pairs` named in `names`, the last value of each, as a JSON object.
fn kept(pairs: &[(String, String)], names: &[&str]) -> serde_json::Map<String, Value> {
    pairs.iter().filter(|(k, _)| names.contains(&k.as_str())).map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect()
}

/// Adds the kept provider environment of `env` to a run record.
pub(super) fn record_env(run: &mut serde_json::Map<String, Value>, env: &[(String, String)]) {
    let env = kept(env, KEPT_ENV);
    if !env.is_empty() {
        run.insert(ENV_KEY.into(), Value::Object(env));
    }
}

/// The provider environment a run record kept (see [`record_env`]); nothing from a missing or
/// malformed record, or from an older one that kept none.
pub(super) fn saved_env(run_json: &str) -> Vec<(String, String)> {
    let Ok(Value::Object(mut run)) = serde_json::from_str::<Value>(run_json) else { return Vec::new() };
    match run.remove(ENV_KEY) {
        Some(Value::Object(env)) => strings(env).filter(|(k, _)| KEPT_ENV.contains(&k.as_str())).collect(),
        _ => Vec::new(),
    }
}

/// `current` with every name `saved` has taken from `saved`: what the apply ran with wins over
/// the account's settings now.
pub(super) fn with_saved(current: &[(String, String)], saved: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = current.iter().filter(|(k, _)| !saved.iter().any(|(s, _)| s == k)).cloned().collect();
    out.extend(saved);
    out
}

/// Lab variables restored from the run record for a destroy. Strings only: the lab variables
/// the provider needs again (`expires_at` is a number and is skipped, the kept environment an
/// object). A missing or malformed record yields nothing, never an error.
pub(super) fn saved_vars(run_json: &str) -> Vec<(String, String)> {
    match serde_json::from_str::<Value>(run_json) {
        Ok(Value::Object(run)) => strings(run).collect(),
        _ => Vec::new(),
    }
}

/// The string members of a JSON object.
fn strings(object: serde_json::Map<String, Value>) -> impl Iterator<Item = (String, String)> {
    object.into_iter().filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
}

fn read_json(path: PathBuf) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// The local state, parsed.
pub fn read_state(state: &Path) -> Option<Value> {
    read_json(state.join(STATE_FILE))
}

/// When the lab host stops itself, from the run record (seconds since epoch).
pub fn expires_at(state: &Path) -> Option<u64> {
    read_json(state.join(RUN_FILE))?[EXPIRES_AT].as_u64()
}

/// A string output of a state, when set and non-empty.
fn output_of<'a>(tfstate: &'a Value, name: &str) -> Option<&'a str> {
    tfstate["outputs"][name]["value"].as_str().filter(|s| !s.is_empty())
}

/// A string output from the local state.
pub(super) fn output(state: &Path, name: &str) -> Option<String> {
    output_of(&read_state(state)?, name).map(str::to_string)
}

/// Whether any resource instance in a Terraform state is tainted (a create or provisioner failed).
pub fn has_tainted(state: &Value) -> bool {
    state["resources"]
        .as_array()
        .is_some_and(|rs| rs.iter().any(|r| r["instances"].as_array().is_some_and(|is| is.iter().any(|i| i["status"].as_str() == Some("tainted")))))
}

/// Whether a state holds resources (anything that may bill).
pub fn has_resources(state: &Value) -> bool {
    state["resources"].as_array().is_some_and(|r| !r.is_empty())
}

/// The state holds a lab host: Isoloom modules output its `ip`.
fn has_instance(outputs: &Value) -> bool {
    outputs[isoloom_outputs::IP]["value"].as_str().is_some_and(|ip| !ip.is_empty())
}

/// The host keys of a deployment's lab host, kept with its state and removed with it.
pub fn known_hosts(state: &Path) -> PathBuf {
    state.join("known_hosts")
}

/// The lab host as an SSH target with the launcher's key, through the node when the launch
/// recorded one (see JUMP_FILE).
pub fn ssh_target(state: &Path, identity: PathBuf) -> Option<ssh::Target> {
    let (host, user) = ssh_endpoint(state)?;
    let jump = std::fs::read_to_string(state.join(JUMP_FILE)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    Some(ssh::Target { jump, ..ssh::Target::direct(host, user, identity) })
}

/// The lab host's address and SSH user, from the local state's outputs.
pub fn ssh_endpoint(state: &Path) -> Option<(String, String)> {
    let v = read_state(state)?;
    let ip = output_of(&v, isoloom_outputs::IP)?.to_string();
    let user = v["outputs"][isoloom_outputs::SSH_USER]["value"].as_str().unwrap_or(isoloom_outputs::DEFAULT_SSH_USER).to_string();
    Some((ip, user))
}

/// Status from the local state's outputs (no container run, so it's cheap to poll).
pub fn status(state: &Path) -> LabStatus {
    let tfstate = read_state(state);
    let outputs = tfstate.as_ref().map(|v| v["outputs"].clone()).unwrap_or(Value::Null);
    // A step that failed (the lab's setup on the VM) leaves its resource tainted: the VM exists
    // but the lab never came up, so it is not running; its machine stays listed as left behind.
    let failed = tfstate.as_ref().is_some_and(has_tainted);
    let expires_at = expires_at(state);
    // Past its auto-stop, the cloud instance has terminated itself.
    let expired = expires_at.is_some_and(|t| now() >= t);
    let created = has_instance(&outputs) && !expired;
    let ip = outputs[isoloom_outputs::IP]["value"].as_str().unwrap_or_default().to_string();
    let machines = if created {
        vec![Machine {
            name: "labhost".into(),
            state: "running".into(),
            image: String::new(),
            ip,
            ports: Vec::new(),
            interfaces: Vec::new(),
            services: Vec::new(),
            infra: false,
        }]
    } else {
        Vec::new()
    };
    LabStatus {
        running: created && !failed,
        parked: None,
        machines,
        networks: Vec::new(),
        url: None,
        host: None,
        expires_at: expires_at.filter(|_| created),
        place: None,
        provider: None,
        attacker: None,
        outputs: Vec::new(),
        message: None,
        reap_error: None,
    }
}

/// True when the state still holds a cloud instance whose auto-stop time has passed, so it
/// needs a `destroy` to free the resources and end billing (an OS poweroff does not deallocate
/// on Azure). Unlike `status`, this stays true after expiry; it's the signal the reaper uses.
pub fn expired(state: &Path) -> bool {
    deployed(state) && expires_at(state).is_some_and(|t| now() >= t)
}

/// True when the state still holds resources a `destroy` has to remove. A completed apply
/// exposes the lab host's `ip`; an interrupted or crashed apply may have already created
/// billable resources without ever writing that output. Either counts, so a half-done cloud
/// deploy can't keep billing silently.
pub fn deployed(state: &Path) -> bool {
    read_state(state).is_some_and(|j| has_instance(&j["outputs"]) || has_resources(&j))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::terraform::tests::temp_state;

    #[test]
    fn a_tainted_setup_is_not_running() {
        let ok = serde_json::json!({"resources": [{"instances": [{"status": null}]}]});
        let failed = serde_json::json!({"resources": [{"instances": [{}]}, {"instances": [{"status": "tainted"}]}]});
        assert!(!has_tainted(&ok));
        assert!(has_tainted(&failed));
        assert!(!has_tainted(&serde_json::json!({})));
    }

    #[test]
    fn run_record_keeps_connection_and_placement_vars_and_expiry() {
        let vars = vec![
            ("ssh_public_key".to_string(), "ssh-ed25519 AAAA".to_string()),
            ("ssh_private_key_file".to_string(), "/k/id".to_string()),
            ("allowed_cidr".to_string(), "1.2.3.0/24".to_string()),
            ("proxmox_password".to_string(), "secret".to_string()),
            ("proxmox_api_token".to_string(), "root@pam!x=secret".to_string()),
            ("inputs".to_string(), r#"{"CTF_LAUNCH_TOKEN":"secret"}"#.to_string()),
            ("region".to_string(), "eu-west-3".to_string()),
            ("node".to_string(), "pve2".to_string()),
            ("auto_stop_hours".to_string(), "4".to_string()),
        ];
        let run = run_record(&vars, &[], 1_000);
        // Secrets and the lab's inputs are not persisted; the connection keys and placement are.
        assert_eq!(run.get("ssh_public_key").and_then(|v| v.as_str()), Some("ssh-ed25519 AAAA"));
        assert_eq!(run.get("ssh_private_key_file").and_then(|v| v.as_str()), Some("/k/id"));
        assert_eq!(run.get("allowed_cidr").and_then(|v| v.as_str()), Some("1.2.3.0/24"));
        assert_eq!(run.get("region").and_then(|v| v.as_str()), Some("eu-west-3"));
        assert_eq!(run.get("node").and_then(|v| v.as_str()), Some("pve2"));
        let text = serde_json::to_string(&run).unwrap();
        assert!(!text.contains("secret"), "{text}");
        // 4 hours past the given `now`.
        assert_eq!(run.get(EXPIRES_AT).and_then(|v| v.as_u64()), Some(1_000 + 4 * 3600));
    }

    #[test]
    fn run_record_keeps_the_providers_region_never_its_credentials() {
        let env = vec![
            ("AWS_ACCESS_KEY_ID".to_string(), "AKIA".to_string()),
            ("AWS_SECRET_ACCESS_KEY".to_string(), "secret".to_string()),
            ("AWS_REGION".to_string(), "eu-west-3".to_string()),
        ];
        let json = serde_json::to_string(&run_record(&[], &env, 1)).unwrap();
        assert!(!json.contains("AKIA") && !json.contains("secret"), "{json}");
        assert_eq!(saved_env(&json), [("AWS_REGION".to_string(), "eu-west-3".to_string())]);
        // The kept environment is not a variable.
        assert!(saved_vars(&json).is_empty());
        // Nothing kept: no `env` at all; and odd records read as none.
        assert!(!serde_json::to_string(&run_record(&[], &[], 1)).unwrap().contains("env"));
        for raw in ["", "garbage", "[]", r#"{"env":"x"}"#, r#"{"env":{"AWS_SECRET_ACCESS_KEY":"s","AWS_REGION":1}}"#] {
            assert!(saved_env(raw).is_empty(), "{raw}");
        }
    }

    #[test]
    fn what_the_apply_ran_with_wins_over_the_settings_now() {
        let pair = |k: &str, v: &str| (k.to_string(), v.to_string());
        let current = [pair("region", "us-east-1"), pair("proxmox_password", "pw"), pair("node", "pve1")];
        let saved = vec![pair("region", "eu-west-3"), pair("node", "pve2")];
        assert_eq!(with_saved(&current, saved), [pair("proxmox_password", "pw"), pair("region", "eu-west-3"), pair("node", "pve2")]);
        assert_eq!(with_saved(&current, Vec::new()), current);
    }

    #[test]
    fn run_record_omits_expiry_when_auto_stop_absent_or_zero() {
        assert!(run_record(&[], &[], 1_000).get(EXPIRES_AT).is_none());
        let zero = vec![("auto_stop_hours".to_string(), "0".to_string())];
        assert!(run_record(&zero, &[], 1_000).get(EXPIRES_AT).is_none());
    }

    #[test]
    fn saved_vars_round_trips_connection_vars_and_skips_numbers() {
        let vars = vec![
            ("ssh_public_key".to_string(), "ssh-ed25519 AAAA".to_string()),
            ("allowed_cidr".to_string(), "1.2.3.0/24".to_string()),
            ("auto_stop_hours".to_string(), "4".to_string()),
        ];
        let json = serde_json::to_string(&run_record(&vars, &[], 1_000)).unwrap();
        let mut restored = saved_vars(&json);
        restored.sort();
        // expires_at is a number and is not restored as a variable.
        assert_eq!(restored, vec![("allowed_cidr".to_string(), "1.2.3.0/24".to_string()), ("ssh_public_key".to_string(), "ssh-ed25519 AAAA".to_string())]);
    }

    #[test]
    fn saved_vars_tolerates_garbage_and_non_objects() {
        assert!(saved_vars("").is_empty());
        assert!(saved_vars("not json").is_empty());
        assert!(saved_vars("[1,2,3]").is_empty());
        assert!(saved_vars(r#"{"expires_at":123}"#).is_empty());
    }

    #[test]
    fn has_instance_needs_a_nonempty_ip_output() {
        assert!(has_instance(&serde_json::json!({"ip": {"value": "10.0.0.9"}})));
        // State with resources but no ip output (or an empty one) is not a live instance.
        assert!(!has_instance(&serde_json::json!({"ip": {"value": ""}})));
        assert!(!has_instance(&serde_json::json!({"other": {"value": "x"}})));
        assert!(!has_instance(&Value::Null));
    }

    #[test]
    fn output_reads_nonempty_string_values_only() {
        let dir = temp_state();
        // No state file yet.
        assert_eq!(output(&dir, "ip"), None);
        std::fs::write(dir.join(STATE_FILE), r#"{"outputs":{"ip":{"value":"10.0.0.9"},"empty":{"value":""},"ready_file":{"value":"/run/ready"}}}"#).unwrap();
        assert_eq!(output(&dir, "ip").as_deref(), Some("10.0.0.9"));
        assert_eq!(output(&dir, "ready_file").as_deref(), Some("/run/ready"));
        assert_eq!(output(&dir, "empty"), None);
        assert_eq!(output(&dir, "missing"), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ssh_endpoint_defaults_the_user_and_needs_an_ip() {
        let dir = temp_state();
        assert_eq!(ssh_endpoint(&dir), None);
        std::fs::write(dir.join(STATE_FILE), r#"{"outputs":{"ip":{"value":"10.0.0.9"}}}"#).unwrap();
        assert_eq!(ssh_endpoint(&dir), Some(("10.0.0.9".to_string(), "isoloom".to_string())));
        std::fs::write(dir.join(STATE_FILE), r#"{"outputs":{"ip":{"value":"10.0.0.9"},"ssh_user":{"value":"ubuntu"}}}"#).unwrap();
        assert_eq!(ssh_endpoint(&dir), Some(("10.0.0.9".to_string(), "ubuntu".to_string())));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ssh_target_goes_through_the_recorded_jump() {
        let dir = temp_state();
        std::fs::write(dir.join(STATE_FILE), r#"{"outputs":{"ip":{"value":"10.10.0.9"}}}"#).unwrap();
        let t = ssh_target(&dir, "/k/id".into()).unwrap();
        assert_eq!((t.host.as_str(), t.port, t.user.as_str(), t.jump.as_deref()), ("10.10.0.9", 22, "isoloom", None));
        std::fs::write(dir.join(JUMP_FILE), "root@pve.lan\n").unwrap();
        assert_eq!(ssh_target(&dir, "/k/id".into()).unwrap().jump.as_deref(), Some("root@pve.lan"));
        std::fs::write(dir.join(JUMP_FILE), "  ").unwrap();
        assert_eq!(ssh_target(&dir, "/k/id".into()).unwrap().jump, None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn expired_is_true_only_with_an_instance_past_its_stop_time() {
        let dir = temp_state();
        // No state: not expired.
        assert!(!expired(&dir));
        std::fs::write(dir.join(STATE_FILE), r#"{"outputs":{"ip":{"value":"10.0.0.9"}}}"#).unwrap();
        // Instance but no run.json (no auto-stop): never expires.
        assert!(!expired(&dir));
        // Future stop time: not expired yet (unlike status, which also shows it running).
        std::fs::write(dir.join(RUN_FILE), format!(r#"{{"expires_at":{}}}"#, now() + 3600)).unwrap();
        assert!(!expired(&dir));
        // Past stop time: expired, so the reaper destroys it.
        std::fs::write(dir.join(RUN_FILE), r#"{"expires_at":1}"#).unwrap();
        assert!(expired(&dir));
        // Expiry past but the instance is already gone (no ip): nothing to reap.
        std::fs::write(dir.join(STATE_FILE), r#"{"outputs":{}}"#).unwrap();
        assert!(!expired(&dir));
        // A half-done apply (resources, no ip output yet) is still reaped.
        std::fs::write(dir.join(STATE_FILE), r#"{"outputs":{},"resources":[{"type":"aws_instance"}]}"#).unwrap();
        assert!(expired(&dir));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_state_whose_stop_time_is_this_very_second_is_expired_and_not_running() {
        // Expiry is inclusive, and status and the reaper agree at the boundary: the instance
        // has terminated itself, so it isn't shown, and the reaper destroys what is left.
        let dir = temp_state();
        std::fs::write(dir.join(STATE_FILE), r#"{"outputs":{"ip":{"value":"10.0.0.9"}},"resources":[{}]}"#).unwrap();
        std::fs::write(dir.join(RUN_FILE), format!(r#"{{"expires_at":{}}}"#, now())).unwrap();
        assert!(expired(&dir));
        let s = status(&dir);
        assert!(!s.running && s.machines.is_empty() && s.expires_at.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_run_record_with_an_unreadable_stop_time_never_expires() {
        // The launcher writes `expires_at` as a whole number; anything else (a hand edit, an
        // older or newer format) is no auto-stop at all rather than an instant expiry that
        // would destroy a lab the player is using.
        let dir = temp_state();
        std::fs::write(dir.join(STATE_FILE), r#"{"outputs":{"ip":{"value":"10.0.0.9"}}}"#).unwrap();
        for raw in [r#"{"expires_at":"1"}"#, r#"{"expires_at":1.5}"#, r#"{"expires_at":-1}"#, r#"{"expires_at":null}"#, "[]", "garbage", ""] {
            std::fs::write(dir.join(RUN_FILE), raw).unwrap();
            assert_eq!(expires_at(&dir), None, "{raw}");
            assert!(!expired(&dir), "{raw}");
            let s = status(&dir);
            assert!(s.running && s.expires_at.is_none(), "{raw}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn status_reads_outputs_from_state() {
        let dir = temp_state();
        assert!(!status(&dir).running);
        std::fs::write(dir.join(STATE_FILE), r#"{"outputs":{"ip":{"value":"10.10.10.150"}}}"#).unwrap();
        let s = status(&dir);
        assert!(s.running);
        assert_eq!(s.machines[0].ip, "10.10.10.150");
        // Auto-stop: shown while pending, stopped once past.
        std::fs::write(dir.join(RUN_FILE), format!(r#"{{"expires_at":{}}}"#, now() + 3600)).unwrap();
        assert!(status(&dir).running && status(&dir).expires_at.is_some());
        std::fs::write(dir.join(RUN_FILE), r#"{"expires_at":1}"#).unwrap();
        assert!(!status(&dir).running);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_tainted_state_keeps_its_machine_but_is_not_running() {
        let dir = temp_state();
        std::fs::write(dir.join(STATE_FILE), r#"{"outputs":{"ip":{"value":"10.0.0.9"}},"resources":[{"instances":[{"status":"tainted"}]}]}"#).unwrap();
        let s = status(&dir);
        assert!(!s.running);
        assert_eq!(s.machines.len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod proptests;
