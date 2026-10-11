//! Server hosts and cloud accounts: the player's own ESXi / Proxmox server (VM labs run there
//! through Vagrant or Terraform) and their AWS / Azure / GCP / DigitalOcean / Linode / OCI
//! accounts. Profiles (non-secret) live in `<app data>/server.json`; each host's secret lives
//! in the OS keychain (a file in debug builds, like the auth session).
//!
//! Neither `vagrant-vmware-esxi` nor `vagrant-proxmox` reads environment variables on its
//! own: a lab's Vagrantfile reads `ENV` and sets `esxi.*` / `proxmox.*` from it. `contract`
//! defines that contract (`connection_env`), documented in `docs/server.md`.
//!
//! - `profile`, `input`: a saved host and the setup form that makes one.
//! - `store`, `secrets`: where profiles and their secrets are kept.
//! - `commands`, `setup_window`: the Server screen's Tauri commands.
//! - `checks`, `reachability`, `cloud_checks`, `budget`: "Test connection" and the spend limit.
//! - `contract`: what a launch is given to reach its host; `gcp`: the GCP labs project.

mod budget;
mod checks;
mod cloud_checks;
mod commands;
mod contract;
pub mod gcp;
mod input;
mod profile;
mod reachability;
mod secrets;
mod setup_window;
mod store;

// What the rest of the app uses, at the paths it always had. The globs carry each command's
// generated `__cmd__*` macros along with it (lib.rs registers the commands by this path).
pub use budget::{BudgetCheck, check_budget};
pub use commands::*;
pub use contract::{
    Connection, connection, default_host, host_name, host_provider, lab_connection, launch_targets, mark_lab, marked_host, proxmox_jump, proxmox_jump_for_host,
};
pub use profile::{HostProfile, terraform_target};
pub use setup_window::*;

/// Written into a VM lab's directory when it runs on a server host, so stop/status
/// (which re-evaluate the Vagrantfile) get the same connection.
pub const HOST_MARKER: &str = ".cyberctf-host";
