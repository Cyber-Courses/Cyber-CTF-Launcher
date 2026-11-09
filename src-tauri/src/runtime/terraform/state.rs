//! Whether a Terraform state a launcher deployment left before the Isothrum runner still holds
//! resources — read for the one-time handover to the runner (`isothrum::import`). The launcher no
//! longer writes or applies Terraform state; the runner does.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::runtime::lab::outputs as isoloom_outputs;

/// The Terraform state file in a deployment's folder.
const STATE_FILE: &str = "terraform.tfstate";

/// The local state, parsed; none when there is no state file. One that can't be read or parsed
/// is an error: it may hold resources still billing.
fn read_state(state: &Path) -> std::io::Result<Option<Value>> {
    let path: PathBuf = state.join(STATE_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    serde_json::from_str(&text).map(Some).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{}: {e}", path.display())))
}

fn has_resources(state: &Value) -> bool {
    state["resources"].as_array().is_some_and(|r| !r.is_empty())
}

/// The state holds a lab host: Isoloom modules output its `ip`.
fn has_instance(outputs: &Value) -> bool {
    outputs[isoloom_outputs::IP]["value"].as_str().is_some_and(|ip| !ip.is_empty())
}

/// Whether a leftover state still holds resources, so there is something to hand to the runner.
/// No state file holds none; one that can't be read is an error, never "nothing deployed".
pub fn deployed(state: &Path) -> std::io::Result<bool> {
    Ok(read_state(state)?.is_some_and(|j| has_instance(&j["outputs"]) || has_resources(&j)))
}
