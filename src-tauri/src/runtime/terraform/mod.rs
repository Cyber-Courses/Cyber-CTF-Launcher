//! Reading the Terraform state a launcher deployment left before the Isothrum runner existed, for
//! the one-time handover to the runner (`isothrum::import`), and the Terraform plugin cache the
//! Storage page cleans. The launcher no longer runs Terraform itself — the runner does.
//!
//! - `state`: whether a leftover state still holds resources (`deployed`).
//! - `runner`: the plugin cache on disk (its name, and clearing it).

mod runner;
mod state;

pub use runner::{PLUGIN_CACHE, clear_plugin_cache};
pub use state::deployed;
