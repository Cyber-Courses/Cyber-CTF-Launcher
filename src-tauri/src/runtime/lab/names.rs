//! Isoloom's path and naming conventions, in one place: where it writes a target's files
//! (`.isoloom[-<n>]/docker|docker-vm|vagrant|proxmox|cloud-docker/<cloud>|cloud-vm/<cloud>|
//! cloud-services`), what it names the machines and networks it creates (`isoloom-controller`,
//! the `isoloom-<lab>-<network>` VM networks, the `isoloom-check` runners), the outputs of its
//! cloud modules (`ip`, `ssh_user`, `ready_file`) and the environment variables its generated
//! files read (`ISOLOOM_HYPERV_SWITCH`, `ISOLOOM_SSH_PROXY_COMMAND`). The rest of the launcher
//! asks here instead of spelling them out.

use std::path::{Path, PathBuf};

use isoloom_spec::Target;

use super::instance;
use crate::runtime::Runtime;

/// Where the lab's generated files are: `.isoloom-<n>/` for an instance, else `.isoloom/`.
pub fn out(dir: &Path) -> PathBuf {
    dir.join(isoloom_spec::instance::output_dir(instance(dir)))
}

/// The lab's Compose file (local Docker, and inside "Docker on one VM").
pub fn compose_file(dir: &Path) -> PathBuf {
    out(dir).join("docker/compose.yml")
}

/// The lab's run manifest on local Docker (Isoloom writes it beside the Compose file): what the
/// Isothrum runner executes.
pub fn docker_manifest(dir: &Path) -> PathBuf {
    out(dir).join("docker/run.json")
}

/// Where the Vagrantfile of a local or ESXi run is: Docker on one VM for container labs, one
/// VM per machine for VM labs.
pub fn vagrant_dir(dir: &Path, runtime: Runtime) -> PathBuf {
    match runtime {
        Runtime::Docker => out(dir).join("docker-vm"),
        Runtime::Vm => out(dir).join("vagrant"),
        // No Vagrant: its generated files (Terraform variables and scripts).
        Runtime::Cloud => out(dir).join("cloud-services"),
    }
}

/// The Isoloom target of a Vagrant run.
pub fn vagrant_target(runtime: Runtime) -> Target {
    match runtime {
        Runtime::Docker => Target::DockerVm,
        Runtime::Vm => Target::Vagrant,
        Runtime::Cloud => Target::CloudServices,
    }
}

/// The Terraform module and its Isoloom target for a server or cloud (`tf` is the launcher's
/// Terraform target: `proxmox`, `aws`, `azure`, `gcp`, `digitalocean`, `linode`, `oci`).
pub fn terraform(dir: &Path, runtime: Runtime, tf: &str) -> (PathBuf, Target) {
    let out = out(dir);
    match (runtime, tf) {
        (Runtime::Docker, "proxmox") => (out.join("docker-vm/proxmox"), Target::DockerVm),
        (Runtime::Vm, "proxmox") => (out.join("proxmox"), Target::Proxmox),
        (Runtime::Docker, cloud) => (out.join("cloud-docker").join(cloud), Target::CloudDocker),
        // Isoloom generates a cloud-vm module per cloud it can model the lab on; a cloud a lab
        // doesn't support has no module, and the run surfaces that when the directory is missing.
        (Runtime::Vm, cloud) => (out.join("cloud-vm").join(cloud), Target::CloudVm),
        // Isoloom's variables for the lab's own module (see `cloud_services`).
        (Runtime::Cloud, _) => (out.join("cloud-services"), Target::CloudServices),
    }
}

/// The environment Isoloom's Vagrantfiles read to reach an ESXi host (the `vagrant-vmware-esxi`
/// plugin's settings), also the vagrant target's run-manifest params by the same names.
pub mod esxi {
    pub const HOSTNAME: &str = crate::runtime::isothrum::ESXI_HOSTNAME;
    pub const HOSTPORT: &str = crate::runtime::isothrum::ESXI_HOSTPORT;
    pub const USERNAME: &str = "ESXI_USERNAME";
    pub const PASSWORD: &str = "ESXI_PASSWORD";
    pub const DATASTORE: &str = "ESXI_DATASTORE";
    pub const VIRTUAL_NETWORK: &str = "ESXI_VIRTUAL_NETWORK";
    /// The host's settings the manifest takes as params (its password is a connection secret).
    pub const PARAMS: [&str; 5] = [HOSTNAME, HOSTPORT, USERNAME, DATASTORE, VIRTUAL_NETWORK];
}

/// The outputs of Isoloom's Terraform modules the launcher reads (Isoloom's run manifest outputs, its docs/runner.md).
pub mod outputs {
    /// The address to start from: the lab's VM.
    pub const IP: &str = "ip";
}

#[cfg(test)]
mod tests {}
