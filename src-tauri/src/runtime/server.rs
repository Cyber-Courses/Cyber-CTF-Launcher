//! Server hosts and cloud accounts: the player's own ESXi / Proxmox server (VM labs run there
//! through Vagrant or Terraform) and their AWS / Azure / GCP / `DigitalOcean` / Linode / OCI
//! accounts. Profiles (non-secret) live in `<app data>/server.json`; each host's secret lives
//! in the OS keychain (a file in debug builds, like the auth session).
//!
//! Neither `vagrant-vmware-esxi` nor `vagrant-proxmox` reads environment variables on its
//! own: a lab's Vagrantfile reads `ENV` and sets `esxi.*` / `proxmox.*` from it. `contract`
//! defines that contract (`connection_env`), documented in `docs/running-labs.md`.
//!
//! - `kind`: each kind of host in one table (what its fields mean, its checks, its Terraform).
//! - `profile`, `input`: a saved host and the setup form that makes one.
//! - `setup`: the setup form's actions (save, with a password or an SSH key; remove; placement).
//! - `store`, `secrets`: where profiles and their secrets are kept.
//! - `connection_test`, `checks`, `reachability`, `cloud_checks`: "Test connection".
//! - `budget`: an AWS account's spend and the limit a launch is checked against.
//! - `capacity`: a Proxmox server's cores and memory, for its row.
//! - `hosts`: the saved hosts' rules: the default, what moves a host, the labs on each.
//! - `contract`: what a launch is given to reach its host; `gcp`: the GCP labs project.

pub(crate) mod budget;
pub(crate) mod capacity;
pub(crate) mod checks;
mod cloud_checks;
pub(crate) mod connection_test;
mod contract;
pub mod gcp;
pub(crate) mod hosts;
pub(crate) mod input;
mod kind;
pub(crate) mod profile;
mod reachability;
pub(crate) mod secrets;
pub(crate) mod setup;
pub(crate) mod store;

// What the rest of the app uses, at the paths it always had. The globs carry each command's
// generated `__cmd__*` macros along with it (lib.rs registers the commands by this path).
pub use budget::*;
pub use contract::{
    Connection, connection, default_server, discovered_node, host_name, host_provider, lab_connection, launch_targets, mark_lab, marked_host,
    proxmox_jump_for_host,
};
/// The test profiles, for the tests of what is built from them elsewhere (`lab::vars`).
#[cfg(test)]
pub(crate) use profile::tests as profile_tests;
pub use profile::{DEFAULT_AUTO_STOP_HOURS, HostProfile, SshKeySignIn, terraform_provider, terraform_target};

/// The Isoloom module variable a cloud's instance size goes to.
pub fn size_var(provider: crate::runtime::providers::Provider) -> Option<&'static str> {
    kind::kind(provider).and_then(|k| k.size_var)
}
/// For the generated TypeScript types (`ts_bindings`).
#[cfg(test)]
pub(crate) use {checks::TestResult, input::HostInput};

/// Written into a VM lab's directory when it runs on a server host, so stop/status
/// (which re-evaluate the Vagrantfile) get the same connection.
pub const HOST_MARKER: &str = ".cyberctf-host";
