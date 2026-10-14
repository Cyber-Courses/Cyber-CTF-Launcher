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
use crate::error::Result;
use crate::runtime::Runtime;

/// Where the lab's generated files are: `.isoloom-<n>/` for an instance, else `.isoloom/`.
pub fn out(dir: &Path) -> PathBuf {
    dir.join(isoloom_spec::instance::output_dir(instance(dir)))
}

/// The lab's Compose file (local Docker, and inside "Docker on one VM").
pub fn compose_file(dir: &Path) -> PathBuf {
    out(dir).join("docker/compose.yml")
}

/// The Compose file's service running the lab's checks (profile `check`).
pub const CHECK_SERVICE: &str = "isoloom-check";

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

/// The environment variable Isoloom's Vagrantfiles read on Hyper-V: the virtual switch each VM's
/// first adapter (Vagrant's connection, and the internet) goes on. Isoloom takes the Default
/// Switch without it, and stops `vagrant up` when no switch has the name.
pub const HYPERV_SWITCH_ENV: &str = "ISOLOOM_HYPERV_SWITCH";

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
pub fn terraform(dir: &Path, runtime: Runtime, tf: &str) -> Result<(PathBuf, Target)> {
    let out = out(dir);
    Ok(match (runtime, tf) {
        (Runtime::Docker, "proxmox") => (out.join("docker-vm/proxmox"), Target::DockerVm),
        (Runtime::Vm, "proxmox") => (out.join("proxmox"), Target::Proxmox),
        (Runtime::Docker, cloud) => (out.join("cloud-docker").join(cloud), Target::CloudDocker),
        // Isoloom generates a cloud-vm module per cloud it can model the lab on; a cloud a lab
        // doesn't support has no module, and the run surfaces that when the directory is missing.
        (Runtime::Vm, cloud) => (out.join("cloud-vm").join(cloud), Target::CloudVm),
        // Isoloom's variables for the lab's own module (see `cloud_services`).
        (Runtime::Cloud, _) => (out.join("cloud-services"), Target::CloudServices),
    })
}

/// The module to destroy a lab's Terraform resources with: its Isoloom module, or, for a lab
/// started before labs moved to Isoloom (still installed at that commit), its old
/// `deploy/terraform/<target>` module, so those resources can still be removed.
pub fn terraform_to_destroy(dir: &Path, runtime: Runtime, tf: &str) -> Result<PathBuf> {
    let legacy = dir.join("deploy/terraform").join(tf);
    if !out(dir).is_dir() && legacy.is_dir() {
        return Ok(legacy);
    }
    Ok(terraform(dir, runtime, tf)?.0)
}

/// Whether a folder name is one of Isoloom's output folders (`.isoloom`, `.isoloom-<n>`).
pub fn is_output_dir(name: &str) -> bool {
    name.starts_with(".isoloom")
}

/// The files Isoloom generates for a cloud-services lab: `terraform.tfvars.json` (the spec's
/// fixed `cloud.vars`), next to its own scripts.
pub fn cloud_services_vars_file(dir: &Path) -> PathBuf {
    out(dir).join("cloud-services/terraform.tfvars.json")
}

/// The machine Isoloom adds to run a lab's `provision:` playbooks (and its checks on VMs).
pub const CONTROLLER: &str = "isoloom-controller";

/// Isoloom's stand-ins for the lab's access machine (the player's seat) and its routes, as named
/// in its Compose file: the services a lab with an access machine defines (profile `check`).
pub const ACCESS_STAND_INS: [&str; 2] = ["isoloom-access", "isoloom-access-routes"];

/// The name of a lab network on the local hypervisor: the internal network (VirtualBox,
/// libvirt) or private switch (Hyper-V) `isoloom-<lab>-<network>`, `lab` being the lab's name
/// (its VM-name prefix), `net` the network's name in its spec.
pub fn vm_network(lab: &str, net: &str) -> String {
    format!("{}{net}", vm_network_prefix(lab))
}

/// What every [`vm_network`] of `lab` starts with.
pub fn vm_network_prefix(lab: &str) -> String {
    format!("isoloom-{lab}-")
}

/// The environment variable Isoloom's Vagrantfiles read for the command Vagrant's SSH to a VM
/// goes through (`config.ssh.proxy_command`), when the VMs aren't on a network this machine
/// reaches directly (an ESXi host through the launcher's relay).
pub const SSH_PROXY_COMMAND_ENV: &str = "ISOLOOM_SSH_PROXY_COMMAND";

/// The outputs of Isoloom's Terraform modules the launcher reads (docs/cloud-modules.md).
pub mod outputs {
    /// The address to start from: the lab's VM.
    pub const IP: &str = "ip";
    /// The SSH login on that VM.
    pub const SSH_USER: &str = "ssh_user";
    /// A file on the VM that holds `ready` once its set-up has finished.
    pub const READY_FILE: &str = "ready_file";
    /// The login when a module names none.
    pub const DEFAULT_SSH_USER: &str = "isoloom";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_of_what_isoloom_creates() {
        assert_eq!(vm_network("minilab", "lab"), "isoloom-minilab-lab");
        assert!(vm_network("t-2", "dmz").starts_with(&vm_network_prefix("t-2")));
        assert!(is_output_dir(".isoloom") && is_output_dir(".isoloom-3") && !is_output_dir("deploy"));
        assert_eq!(cloud_services_vars_file(Path::new("/l")), PathBuf::from("/l/.isoloom/cloud-services/terraform.tfvars.json"));
    }
}
