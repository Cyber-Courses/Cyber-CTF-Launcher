//! Isoloom's path and naming conventions, in one place: where it writes a target's files
//! (`.isoloom[-<n>]/docker|vagrant|proxmox|cloud-docker/<cloud>|cloud-vm/<cloud>|cloud-services`,
//! the Compose file among them, and the `docker-vm` folder an earlier version
//! left), the environment its
//! Vagrantfiles read to reach an ESXi host (`ESXI_*`), and the output of its cloud modules the
//! launcher reads (`ip`). The rest of the launcher asks here instead of spelling them out.

use std::path::{Path, PathBuf};

use isoloom_spec::Target;

use super::instance;
use crate::runtime::Runtime;

/// Where the lab's generated files are: `.isoloom-<n>/` for an instance, else `.isoloom/`.
pub fn out(dir: &Path) -> PathBuf {
    dir.join(isoloom_spec::instance::output_dir(instance(dir)))
}

/// The lab's Compose file (local Docker).
pub fn compose_file(dir: &Path) -> PathBuf {
    out(dir).join("docker/compose.yml")
}

/// The lab's Compose project on local Docker (the folder of its Compose file): what the Isothrum
/// runner opens.
pub fn docker_dir(dir: &Path) -> PathBuf {
    out(dir).join("docker")
}

/// Where the Vagrantfile of a local or ESXi run is: one VM per machine (`vagrant`); for a
/// container lab, the folder of the one helper VM an earlier version ran it in
/// ([`docker_vm_dir`]).
pub fn vagrant_dir(dir: &Path, runtime: Runtime) -> PathBuf {
    match runtime {
        Runtime::Docker => docker_vm_dir(dir),
        Runtime::Vm => out(dir).join("vagrant"),
        // No Vagrant: its generated files (Terraform variables and scripts).
        Runtime::Cloud => out(dir).join("cloud-services"),
    }
}

/// Where an earlier version ran a container lab as Docker Compose inside one helper VM (Isoloom's
/// former `docker-vm` target), here or on ESXi, and (`proxmox/` under it) on a Proxmox server.
/// Isoloom no longer generates that form: the folder is only read, to stop and remove a run left
/// in it.
pub fn docker_vm_dir(dir: &Path) -> PathBuf {
    out(dir).join(DOCKER_VM_FOLDER)
}

/// The folder name of [`docker_vm_dir`], by which a run's project is known as one in a helper VM.
pub const DOCKER_VM_FOLDER: &str = "docker-vm";

/// The folder of Isoloom's target `project` is generated in (`docker`, `vagrant`, `docker-vm`,
/// `proxmox`, `cloud-vm`, …): the one right under the lab's `.isoloom[-<n>]` folder. None for a
/// folder outside one.
pub fn target_folder(project: &Path) -> Option<&str> {
    let mut parts = project.iter().filter_map(|c| c.to_str());
    let out = crate::runtime::isothrum::OUTPUT_DIR;
    parts.by_ref().find(|c| c.strip_prefix(out).is_some_and(|n| n.is_empty() || n.starts_with('-')))?;
    parts.next()
}

/// The Terraform module and its Isoloom target for a server or cloud (`tf` is the launcher's
/// Terraform target: `proxmox`, `aws`, `azure`, `gcp`, `digitalocean`, `linode`, `oci`). On
/// Proxmox, a lab runs as one VM per machine whatever its catalogue form: a container lab left in
/// a helper VM there by an earlier version is read from [`docker_vm_dir`] instead.
pub fn terraform(dir: &Path, runtime: Runtime, tf: &str) -> (PathBuf, Target) {
    let out = out(dir);
    match (runtime, tf) {
        (Runtime::Docker | Runtime::Vm, "proxmox") => (out.join("proxmox"), Target::Proxmox),
        (Runtime::Docker, cloud) => (out.join("cloud-docker").join(cloud), Target::CloudDocker),
        // Isoloom generates a cloud-vm module per cloud it can model the lab on; a cloud a lab
        // doesn't support has no module, and the run surfaces that when the directory is missing.
        // A cloud-services lab has no Isoloom target: it runs its own module (`cloud_services`), and
        // is never asked for here; it reads as one VM per machine.
        (Runtime::Vm | Runtime::Cloud, cloud) => (out.join("cloud-vm").join(cloud), Target::CloudVm),
    }
}

/// The variable Isoloom's Vagrantfiles read their copy's key from (unset, they make one from their
/// folder's path): the launcher hands the runner's key for the run on under it, so two copies of
/// one lab (the app's, a test's) never share a VM, network or libvirt domain name, and the
/// integration tests find this copy's VMs by it.
pub(crate) const ISOLOOM_KEY: &str = "ISOLOOM_KEY";

/// The environment Isoloom's Vagrantfiles read to reach an ESXi host (the `vagrant-vmware-esxi`
/// plugin's settings), also the run's values by the same names.
pub mod esxi {
    /// The ESXi host's address.
    pub const HOSTNAME: &str = crate::runtime::isothrum::ESXI_HOSTNAME;
    /// The host's SSH port.
    pub const HOSTPORT: &str = crate::runtime::isothrum::ESXI_HOSTPORT;
    /// The ESXi user Vagrant signs in as.
    pub const USERNAME: &str = "ESXI_USERNAME";
    /// That user's password.
    pub const PASSWORD: &str = "ESXI_PASSWORD";
    /// The datastore the lab's VM disks go on.
    pub const DATASTORE: &str = "ESXI_DATASTORE";
    /// The port group the lab's VMs are plugged into.
    pub const VIRTUAL_NETWORK: &str = "ESXI_VIRTUAL_NETWORK";
    /// The host's settings the run takes as values (its password is a connection secret).
    pub const PARAMS: [&str; 5] = [HOSTNAME, HOSTPORT, USERNAME, DATASTORE, VIRTUAL_NETWORK];
}
