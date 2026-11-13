//! Whether a Terraform state a launcher deployment left before the Isothrum runner still holds
//! resources — read for the one-time handover to the runner (`isothrum::import`). The launcher no
//! longer writes or applies Terraform state; the runner does.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::runtime::lab::outputs as isoloom_outputs;

/// The Terraform state file in a deployment's folder.
const STATE_FILE: &str = "terraform.tfstate";

/// A state file as Terraform writes it, as far as the handover reads it: its outputs and whether
/// it lists resources. Unknown fields are skipped.
#[derive(Debug, Default, Deserialize)]
struct TfState {
    #[serde(default)]
    outputs: BTreeMap<String, Output>,
    #[serde(default)]
    resources: Vec<serde::de::IgnoredAny>,
}

/// An output of the state; its value is the module's, any JSON.
#[derive(Debug, Deserialize)]
struct Output {
    #[serde(default)]
    value: serde_json::Value,
}

impl TfState {
    /// It holds a lab host: Isoloom modules output its `ip`.
    fn has_instance(&self) -> bool {
        self.outputs.get(isoloom_outputs::IP).and_then(|o| o.value.as_str()).is_some_and(|ip| !ip.is_empty())
    }
}

/// The local state, parsed; none when there is no state file. One that can't be read or parsed
/// (not JSON, or not the shape Terraform writes) is an error: it may hold resources still billing.
fn read_state(state: &Path) -> std::io::Result<Option<TfState>> {
    let path: PathBuf = state.join(STATE_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    serde_json::from_str(&text).map(Some).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{}: {e}", path.display())))
}

/// Whether a leftover state still holds resources, so there is something to hand to the runner.
/// No state file holds none; one that can't be read is an error, never "nothing deployed".
pub fn deployed(state: &Path) -> std::io::Result<bool> {
    Ok(read_state(state)?.is_some_and(|s| s.has_instance() || !s.resources.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `text` as the state file of a fresh folder, and whether it reads as deployed.
    fn deployed_with(text: &str) -> std::io::Result<bool> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(STATE_FILE), text).unwrap();
        deployed(dir.path())
    }

    #[test]
    fn a_state_with_resources_or_a_host_is_deployed() {
        // As Terraform 1.x writes it.
        let held = r#"{"version":4,"terraform_version":"1.9.8","serial":7,"lineage":"0b1c","outputs":{"ip":{"value":"10.0.0.5","type":"string"}},"resources":[{"mode":"managed","type":"aws_instance","name":"lab","provider":"provider[\"registry.terraform.io/hashicorp/aws\"]","instances":[{"schema_version":1,"attributes":{"id":"i-0abc"}}]}],"check_results":null}"#;
        assert!(deployed_with(held).unwrap());
        let destroyed = r#"{"version":4,"terraform_version":"1.9.8","serial":8,"lineage":"0b1c","outputs":{},"resources":[],"check_results":null}"#;
        assert!(!deployed_with(destroyed).unwrap());
        assert!(deployed_with(r#"{"outputs":{"ip":{"value":"10.0.0.5"}}}"#).unwrap());
        assert!(!deployed_with(r#"{"outputs":{"ip":{"value":""}}}"#).unwrap());
    }

    #[test]
    fn no_state_is_nothing_and_a_broken_one_an_error() {
        assert!(!deployed(tempfile::tempdir().unwrap().path()).unwrap());
        assert_eq!(deployed_with("{").unwrap_err().kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(deployed_with(r#"{"resources":{}}"#).unwrap_err().kind(), std::io::ErrorKind::InvalidData);
    }
}
