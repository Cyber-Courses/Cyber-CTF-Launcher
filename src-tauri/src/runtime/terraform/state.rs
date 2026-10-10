//! What a lab's Terraform state says, read from the files only (no Terraform run, so it's cheap
//! to poll): the lab host's address and SSH login, whether it runs, and when it auto-stops. Also
//! the non-secret run record kept beside the state so a destroy can be replayed.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::runtime::{LabStatus, Machine, ssh};

/// Non-secret run parameters, kept next to the state so `destroy` can be replayed.
pub(super) const RUN_FILE: &str = "run.json";
pub(super) const EXPIRES_AT: &str = "expires_at";
pub(super) const STATE_FILE: &str = "terraform.tfstate";
/// Where the node login for a lab reached through its Proxmox node is kept (`user@host`).
pub const JUMP_FILE: &str = "ssh-jump";

pub(super) fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The non-secret run record written beside the state at apply time, so a later destroy can be
/// replayed without the launch spec: the connection/key variables plus, for auto-stopping cloud
/// hosts, the computed `expires_at` (seconds since epoch; `now` is the current time).
pub(super) fn run_record(vars: &[(String, String)], now: u64) -> serde_json::Map<String, Value> {
    let mut run: serde_json::Map<String, Value> = vars
        .iter()
        .filter(|(k, _)| matches!(k.as_str(), "ssh_public_key" | "ssh_private_key_file" | "allowed_cidr"))
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();
    // When the lab host stops itself (cloud auto-stop), so status can tell.
    if let Some(hours) = vars.iter().find(|(k, _)| k == "auto_stop_hours").and_then(|(_, v)| v.parse::<u64>().ok()).filter(|h| *h > 0) {
        run.insert(EXPIRES_AT.into(), Value::from(now.saturating_add(hours.saturating_mul(3600))));
    }
    run
}

/// Lab variables restored from the run record for a destroy. Strings only: the lab variables
/// the provider needs again (`expires_at` is a number and is skipped). A missing or malformed
/// record yields nothing, never an error.
pub(super) fn saved_vars(run_json: &str) -> Vec<(String, String)> {
    match serde_json::from_str::<Value>(run_json) {
        Ok(Value::Object(run)) => run.into_iter().filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string()))).collect(),
        _ => Vec::new(),
    }
}

fn read_json(path: PathBuf) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// The local state, parsed.
fn read_state(state: &Path) -> Option<Value> {
    read_json(state.join(STATE_FILE))
}

/// When the lab host stops itself, from the run record (seconds since epoch).
fn expires_at(state: &Path) -> Option<u64> {
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
fn has_tainted(state: &Value) -> bool {
    state["resources"]
        .as_array()
        .is_some_and(|rs| rs.iter().any(|r| r["instances"].as_array().is_some_and(|is| is.iter().any(|i| i["status"].as_str() == Some("tainted")))))
}

/// The state holds a lab host: Isoloom modules output its `ip`.
fn has_instance(outputs: &Value) -> bool {
    outputs["ip"]["value"].as_str().is_some_and(|ip| !ip.is_empty())
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
    let ip = output_of(&v, "ip")?.to_string();
    let user = v["outputs"]["ssh_user"]["value"].as_str().unwrap_or("isoloom").to_string();
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
    let ip = outputs["ip"]["value"].as_str().unwrap_or_default().to_string();
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
    }
}

/// True when the state still holds a cloud instance whose auto-stop time has passed, so it
/// needs a `destroy` to free the resources and end billing (an OS poweroff does not deallocate
/// on Azure). Unlike `status`, this stays true after expiry; it's the signal the reaper uses.
pub fn expired(state: &Path) -> bool {
    // A completed apply exposes the lab host's `ip`; an interrupted or crashed apply may have
    // already created billable resources without ever writing that output. Reap on either, so a
    // half-done cloud deploy can't keep billing silently.
    let has_billable = read_state(state).is_some_and(|j| has_instance(&j["outputs"]) || j["resources"].as_array().is_some_and(|r| !r.is_empty()));
    has_billable && expires_at(state).is_some_and(|t| now() >= t)
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
    fn run_record_keeps_only_connection_vars_and_expiry() {
        let vars = vec![
            ("ssh_public_key".to_string(), "ssh-ed25519 AAAA".to_string()),
            ("ssh_private_key_file".to_string(), "/k/id".to_string()),
            ("allowed_cidr".to_string(), "1.2.3.0/24".to_string()),
            ("proxmox_password".to_string(), "secret".to_string()),
            ("region".to_string(), "eu-west-3".to_string()),
            ("auto_stop_hours".to_string(), "4".to_string()),
        ];
        let run = run_record(&vars, 1_000);
        // Secrets and lab-specific vars are not persisted; the connection keys are.
        assert_eq!(run.get("ssh_public_key").and_then(|v| v.as_str()), Some("ssh-ed25519 AAAA"));
        assert_eq!(run.get("ssh_private_key_file").and_then(|v| v.as_str()), Some("/k/id"));
        assert_eq!(run.get("allowed_cidr").and_then(|v| v.as_str()), Some("1.2.3.0/24"));
        assert!(run.get("proxmox_password").is_none());
        assert!(run.get("region").is_none());
        // 4 hours past the given `now`.
        assert_eq!(run.get(EXPIRES_AT).and_then(|v| v.as_u64()), Some(1_000 + 4 * 3600));
    }

    #[test]
    fn run_record_omits_expiry_when_auto_stop_absent_or_zero() {
        assert!(run_record(&[], 1_000).get(EXPIRES_AT).is_none());
        let zero = vec![("auto_stop_hours".to_string(), "0".to_string())];
        assert!(run_record(&zero, 1_000).get(EXPIRES_AT).is_none());
    }

    #[test]
    fn saved_vars_round_trips_connection_vars_and_skips_numbers() {
        let vars = vec![
            ("ssh_public_key".to_string(), "ssh-ed25519 AAAA".to_string()),
            ("allowed_cidr".to_string(), "1.2.3.0/24".to_string()),
            ("auto_stop_hours".to_string(), "4".to_string()),
        ];
        let json = serde_json::to_string(&run_record(&vars, 1_000)).unwrap();
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
