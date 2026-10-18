//! Whether a Terraform state a launcher deployment left before the Isothrum runner still holds
//! resources — read for the one-time handover to the runner (`isothrum::import`). The launcher no
//! longer writes or applies Terraform state; the runner does.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::runtime::lab::outputs as isoloom_outputs;

/// The Terraform state file in a deployment's folder.
const STATE_FILE: &str = "terraform.tfstate";

fn read_json(path: PathBuf) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// The local state, parsed.
fn read_state(state: &Path) -> Option<Value> {
    read_json(state.join(STATE_FILE))
}

fn has_resources(state: &Value) -> bool {
    state["resources"].as_array().is_some_and(|r| !r.is_empty())
}

/// The state holds a lab host: Isoloom modules output its `ip`.
fn has_instance(outputs: &Value) -> bool {
    outputs[isoloom_outputs::IP]["value"].as_str().is_some_and(|ip| !ip.is_empty())
}

/// Whether a leftover state still holds resources, so there is something to hand to the runner.
pub fn deployed(state: &Path) -> bool {
    read_state(state).is_some_and(|j| has_instance(&j["outputs"]) || has_resources(&j))
}
