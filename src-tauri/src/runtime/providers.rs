//! Vagrant providers the launcher knows how to drive. Vagrant abstracts the
//! hypervisor; we only detect what is usable on this machine and pass
//! `--provider` through. The cloud accounts share the enum (labs run there through Terraform)
//! but are never detected here.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::exec::run_read;

/// Vagrant provider ids (the value given to `vagrant up --provider`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Virtualbox,
    VmwareDesktop,
    Hyperv,
    Parallels,
    Libvirt,
    Qemu,
    /// UTM (QEMU front end for macOS). Runs ARM guests natively on Apple
    /// Silicon and x86 guests under emulation (slow). Plugin `vagrant_utm`.
    Utm,
    /// Remote VMware ESXi host (plugin `vagrant-vmware-esxi`).
    VmwareEsxi,
    /// Remote Proxmox VE host (Terraform bpg/proxmox; plugin `vagrant-proxmox` is dead).
    Proxmox,
    /// The player's AWS account (Terraform, a lab's Isoloom cloud-docker module). Not a Vagrant
    /// provider and never detected locally, so it is not in `ALL`.
    Aws,
    /// The player's Azure subscription (Terraform, a lab's Isoloom cloud-docker module). Like
    /// AWS: not a Vagrant provider, never detected locally, not in `ALL`.
    Azure,
    /// The player's Google Cloud project (Terraform, a lab's Isoloom cloud-docker module). Like
    /// AWS and Azure: not a Vagrant provider, never detected locally, not in `ALL`.
    Gcp,
    /// The player's DigitalOcean account (Terraform, a lab's Isoloom cloud-docker module).
    /// Authenticated with an API token; not a Vagrant provider, never detected, not in `ALL`.
    #[serde(rename = "digitalocean")]
    DigitalOcean,
    /// The player's Linode account (Terraform, a lab's Isoloom cloud-docker module). Like
    /// DigitalOcean: API-token auth, not a Vagrant provider, never detected, not in `ALL`.
    Linode,
    /// The player's Oracle Cloud tenancy (Terraform, a lab's Isoloom cloud-docker module). Authenticated
    /// with an API signing key in ~/.oci/config; not a Vagrant provider, never detected, not in `ALL`.
    Oci,
}

impl Provider {
    pub const ALL: [Provider; 9] = [
        Provider::Virtualbox,
        Provider::VmwareDesktop,
        Provider::Hyperv,
        Provider::Parallels,
        Provider::Libvirt,
        Provider::Qemu,
        Provider::Utm,
        Provider::VmwareEsxi,
        Provider::Proxmox,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Provider::Virtualbox => "virtualbox",
            Provider::VmwareDesktop => "vmware_desktop",
            Provider::Hyperv => "hyperv",
            Provider::Parallels => "parallels",
            Provider::Libvirt => "libvirt",
            Provider::Qemu => "qemu",
            Provider::Utm => "utm",
            Provider::VmwareEsxi => "vmware_esxi",
            Provider::Proxmox => "proxmox",
            Provider::Aws => "aws",
            Provider::Azure => "azure",
            Provider::Gcp => "gcp",
            Provider::DigitalOcean => "digitalocean",
            Provider::Linode => "linode",
            Provider::Oci => "oci",
        }
    }

    /// The provider with this id (as `id()` prints it), e.g. from Vagrant's `provider-name`.
    pub fn from_id(id: &str) -> Option<Provider> {
        Provider::ALL.iter().copied().find(|p| p.id() == id)
    }

    /// Remote providers run VMs on another host and need connection settings.
    pub fn is_remote(self) -> bool {
        matches!(
            self,
            Provider::VmwareEsxi
                | Provider::Proxmox
                | Provider::Aws
                | Provider::Azure
                | Provider::Gcp
                | Provider::DigitalOcean
                | Provider::Linode
                | Provider::Oci
        )
    }

    /// Cloud accounts (billed to the player), as opposed to the player's own servers.
    pub fn is_cloud(self) -> bool {
        self.is_remote() && !matches!(self, Provider::VmwareEsxi | Provider::Proxmox)
    }

    /// Whether this provider can run on the current OS at all. Local hypervisors are
    /// platform-specific; remote ones (ESXi, Proxmox) apply anywhere. We only surface
    /// providers that are possible on this machine, not the whole catalogue.
    fn applicable(self) -> bool {
        use std::env::consts::{ARCH, OS};
        match self {
            // Neither ships for Windows on Arm (no installer, no Arm VMs there): VM labs on those
            // PCs go to Hyper-V, a server or the cloud.
            Provider::Virtualbox | Provider::VmwareDesktop if OS == "windows" && ARCH == "aarch64" => false,
            Provider::Hyperv => OS == "windows",
            Provider::Parallels | Provider::Utm => OS == "macos",
            Provider::Libvirt => OS == "linux",
            _ => true,
        }
    }

    /// Vagrant plugin required, if the provider is not built in.
    fn plugin(self) -> Option<&'static str> {
        match self {
            Provider::Virtualbox | Provider::Hyperv => None,
            Provider::VmwareDesktop => Some("vagrant-vmware-desktop"),
            Provider::Parallels => Some("vagrant-parallels"),
            Provider::Libvirt => Some("vagrant-libvirt"),
            Provider::Qemu => Some("vagrant-qemu"),
            Provider::Utm => Some("vagrant_utm"),
            Provider::VmwareEsxi => Some("vagrant-vmware-esxi"),
            Provider::Proxmox => Some("vagrant-proxmox"),
            Provider::Aws | Provider::Azure | Provider::Gcp | Provider::DigitalOcean | Provider::Linode | Provider::Oci => None,
        }
    }

    /// Local tool that proves the hypervisor itself is installed.
    fn probe(self) -> Option<(&'static str, &'static [&'static str])> {
        match self {
            Provider::Virtualbox => Some(("VBoxManage", &["--version"])),
            Provider::VmwareDesktop => Some(("vmrun", &[])),
            Provider::Parallels => Some(("prlctl", &["--version"])),
            Provider::Libvirt => Some(("virsh", &["--version"])),
            Provider::Qemu => Some((qemu_binary(), &["--version"])),
            // Hyper-V is a Windows feature, UTM is checked by its app bundle, and
            // remote providers have nothing local to probe.
            Provider::Hyperv
            | Provider::Utm
            | Provider::VmwareEsxi
            | Provider::Proxmox
            | Provider::Aws
            | Provider::Azure
            | Provider::Gcp
            | Provider::DigitalOcean
            | Provider::Linode
            | Provider::Oci => None,
        }
    }

    /// Whether the hypervisor itself is installed here. None when there's nothing local to check:
    /// remote providers, and Hyper-V (a Windows feature, not probed).
    async fn hypervisor_installed(self) -> Option<bool> {
        if self == Provider::Utm {
            return Some(std::path::Path::new(UTM_APP).exists());
        }
        let (program, args) = self.probe()?;
        Some(tool_present(program, args).await)
    }
}

const UTM_APP: &str = "/Applications/UTM.app";

/// Why libvirt can't run VMs here although `virsh` is installed: vagrant-libvirt boots KVM
/// guests, so it needs `/dev/kvm`, and the player must be allowed to open it.
pub(crate) fn kvm_problem() -> Option<String> {
    let dev = std::path::Path::new("/dev/kvm");
    if !dev.exists() {
        return Some("KVM isn't available on this machine (no /dev/kvm): turn on virtualization in the firmware, or use another hypervisor".into());
    }
    if std::fs::OpenOptions::new().read(true).write(true).open(dev).is_err() {
        return Some("you can't use /dev/kvm yet: add yourself to the kvm and libvirt groups, then sign out and back in".into());
    }
    None
}

fn qemu_binary() -> &'static str {
    if std::env::consts::ARCH == "aarch64" { "qemu-system-aarch64" } else { "qemu-system-x86_64" }
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub provider: Provider,
    pub remote: bool,
    pub available: bool,
    /// Whether the hypervisor itself is installed. None for remote providers (nothing
    /// local to run) and Hyper-V (a Windows feature we do not probe).
    pub hypervisor: Option<bool>,
    /// The Vagrant plugin this provider needs, if it is not built in.
    pub plugin: Option<String>,
    /// Whether that plugin is present (true when the provider is built in).
    pub plugin_installed: bool,
    /// Why it is not available (missing plugin, missing hypervisor, wrong OS).
    pub reason: Option<String>,
}

/// Plugin names from `vagrant plugin list` lines like `vagrant-libvirt (0.12.2, global)`.
pub fn parse_plugins(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|name| name.starts_with("vagrant-") || name.starts_with("vagrant_"))
        .map(str::to_string)
        .collect()
}

async fn tool_present(program: &'static str, args: &[&str]) -> bool {
    match run_read(program, args, None).await {
        Ok(_) => true,
        // vmrun without arguments prints usage and exits non-zero, but it exists.
        Err(crate::error::Error::CommandFailed { .. }) => true,
        Err(_) => false,
    }
}

/// The Vagrant plugins installed (none when `vagrant plugin list` fails).
async fn installed_plugins() -> Vec<String> {
    run_read("vagrant", &["plugin", "list"], None).await.map(|o| parse_plugins(&o)).unwrap_or_default()
}

pub async fn detect(vagrant_installed: bool) -> Vec<ProviderStatus> {
    let plugins = if vagrant_installed { installed_plugins().await } else { Vec::new() };

    let mut statuses = Vec::new();
    for provider in Provider::ALL {
        // Only surface providers that can run on this machine, not the whole catalogue.
        if !provider.applicable() {
            continue;
        }

        // The hypervisor layer (the VM software itself).
        let hypervisor = provider.hypervisor_installed().await;

        // The Vagrant layer (the plugin that drives this provider).
        let plugin = provider.plugin().map(str::to_string);
        let plugin_installed = match &plugin {
            None => true, // built in to Vagrant (virtualbox, hyperv)
            Some(p) => vagrant_installed && plugins.iter().any(|i| i == p),
        };

        // Installed is not the same as working: `VBoxManage --version` never talks to VirtualBox's
        // service (VBoxSVC), which can wedge so that every real call hangs (a deploy then sits on
        // "Bringing machine up" forever). Ask it something that needs the service, briefly.
        let unresponsive = provider == Provider::Virtualbox
            && hypervisor == Some(true)
            && matches!(
                crate::exec::run_env_timed("VBoxManage", &["list", "runningvms"], None, &[], std::time::Duration::from_secs(8)).await,
                Err(crate::error::Error::CommandFailed { ref stderr, .. }) if stderr.contains("timed out")
            );

        let reason = unavailable_reason(provider, vagrant_installed, unresponsive, hypervisor, plugin.as_deref(), plugin_installed);

        statuses.push(ProviderStatus { provider, remote: provider.is_remote(), available: reason.is_none(), hypervisor, plugin, plugin_installed, reason });
    }
    statuses
}

/// Why a provider can't be used here (None: it can), from what `detect` found out about it.
fn unavailable_reason(
    provider: Provider,
    vagrant_installed: bool,
    unresponsive: bool,
    hypervisor: Option<bool>,
    plugin: Option<&str>,
    plugin_installed: bool,
) -> Option<String> {
    if !vagrant_installed {
        Some("Vagrant is not installed".to_string())
    } else if unresponsive {
        Some(
            "VirtualBox isn't responding (its background service is stuck). Quit VirtualBox and any VM windows, or restart the computer, then re-check."
                .to_string(),
        )
    } else if hypervisor == Some(false) {
        provider.probe().map(|(program, _)| format!("`{program}` was not found")).or_else(|| Some("not installed".to_string()))
    } else if let Some(why) = (provider == Provider::Libvirt).then(kvm_problem).flatten() {
        Some(why)
    } else if !plugin_installed {
        plugin.map(|p| format!("Vagrant plugin `{p}` is not installed"))
    } else {
        None
    }
}

/// Verifies a local provider is actually usable before a start, so a missing hypervisor or
/// Vagrant plugin fails with an actionable message instead of a raw Vagrant error mid-boot.
/// Remote and cloud providers are validated by their own host/connection checks, so they pass.
pub async fn ensure_usable(provider: Provider) -> std::result::Result<(), String> {
    if provider.is_remote() {
        return Ok(());
    }
    if !tool_present("vagrant", &["--version"]).await {
        return Err("Vagrant isn't installed. Install it from the Machine page, then start the lab again.".into());
    }
    if provider.hypervisor_installed().await == Some(false) {
        let what = provider.probe().map(|(p, _)| format!("`{p}` was not found")).unwrap_or_else(|| "its hypervisor isn't installed".into());
        return Err(format!("Can't run on {} here: {what}. Install it from the Machine page, then start the lab again.", provider.id()));
    }
    if provider == Provider::Libvirt
        && let Some(why) = kvm_problem()
    {
        return Err(format!("Can't run on libvirt here: {why}."));
    }
    if let Some(needed) = provider.plugin()
        && !installed_plugins().await.iter().any(|i| i == needed)
    {
        return Err(format!(
            "The Vagrant plugin `{needed}` for {} isn't installed. Install it from the Machine page, then start the lab again.",
            provider.id()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vagrant_plugin_list() {
        let out = "vagrant-libvirt (0.12.2, global)\nvagrant_utm (0.1.3, global)\nvagrant-vmware-esxi (2.5.5, global)\n  - Version Constraint: > 0\n";
        assert_eq!(parse_plugins(out), vec!["vagrant-libvirt", "vagrant_utm", "vagrant-vmware-esxi"]);
    }

    #[test]
    fn provider_ids_match_serde_names() {
        for p in Provider::ALL {
            assert_eq!(serde_json::to_value(p).unwrap(), p.id());
        }
    }

    #[test]
    fn parse_plugins_handles_blank_versions_and_indentation() {
        // Built-in-only output, trailing detail lines, blank lines, and non-plugin noise.
        let out = "\nvagrant-vmware-desktop (3.0.4, global)\n  - Version Constraint: > 0\nvagrant-qemu (0.3.3)\nsomething-else (1.0.0)\n\n";
        assert_eq!(parse_plugins(out), vec!["vagrant-vmware-desktop", "vagrant-qemu"]);
        assert!(parse_plugins("").is_empty());
        assert!(parse_plugins("No plugins installed.\n").is_empty());
    }

    #[test]
    fn parse_plugins_accepts_both_hyphen_and_underscore_prefixes() {
        let out = "vagrant-libvirt (0.12.2)\nvagrant_utm (0.1.3)\n";
        assert_eq!(parse_plugins(out), vec!["vagrant-libvirt", "vagrant_utm"]);
    }

    #[test]
    fn all_detectable_providers_exclude_clouds() {
        // ALL is the locally detectable catalogue; cloud and AWS-family are never detected.
        for p in Provider::ALL {
            assert!(!matches!(p, Provider::Aws | Provider::Azure | Provider::Gcp | Provider::DigitalOcean | Provider::Linode | Provider::Oci));
        }
        assert_eq!(Provider::ALL.len(), 9);
    }

    #[test]
    fn remote_and_cloud_classification() {
        // Servers are remote but not cloud; the billed accounts are both.
        for server in [Provider::VmwareEsxi, Provider::Proxmox] {
            assert!(server.is_remote() && !server.is_cloud());
        }
        for cloud in [Provider::Aws, Provider::Azure, Provider::Gcp, Provider::DigitalOcean, Provider::Linode, Provider::Oci] {
            assert!(cloud.is_remote() && cloud.is_cloud());
        }
        // Local hypervisors are neither.
        for local in [Provider::Virtualbox, Provider::Hyperv, Provider::Parallels, Provider::Libvirt, Provider::Qemu, Provider::Utm] {
            assert!(!local.is_remote() && !local.is_cloud());
        }
    }

    #[test]
    fn built_in_providers_need_no_plugin_and_clouds_have_none() {
        assert_eq!(Provider::Virtualbox.plugin(), None);
        assert_eq!(Provider::Hyperv.plugin(), None);
        // Each non-builtin local/remote-hypervisor provider names a plugin.
        for p in [Provider::VmwareDesktop, Provider::Parallels, Provider::Libvirt, Provider::Qemu, Provider::Utm, Provider::VmwareEsxi, Provider::Proxmox] {
            assert!(p.plugin().is_some(), "{:?}", p);
        }
        // Cloud providers are not Vagrant providers, so no plugin.
        for p in [Provider::Aws, Provider::Azure, Provider::Gcp, Provider::DigitalOcean, Provider::Linode, Provider::Oci] {
            assert_eq!(p.plugin(), None, "{:?}", p);
        }
    }

    #[test]
    fn ids_are_unique_and_round_trip_through_serde() {
        let all = [
            Provider::Virtualbox,
            Provider::VmwareDesktop,
            Provider::Hyperv,
            Provider::Parallels,
            Provider::Libvirt,
            Provider::Qemu,
            Provider::Utm,
            Provider::VmwareEsxi,
            Provider::Proxmox,
            Provider::Aws,
            Provider::Azure,
            Provider::Gcp,
            Provider::DigitalOcean,
            Provider::Linode,
            Provider::Oci,
        ];
        let mut ids: Vec<&str> = all.iter().map(|p| p.id()).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count, "provider ids must be unique");
        // id() is the serde wire name, and it round-trips back to the same provider.
        for p in all {
            let round: Provider = serde_json::from_value(serde_json::Value::String(p.id().to_string())).unwrap();
            assert_eq!(round, p);
        }
    }

    #[test]
    fn from_id_knows_only_the_detectable_catalogue() {
        for p in Provider::ALL {
            assert_eq!(Provider::from_id(p.id()), Some(p));
        }
        assert_eq!(Provider::from_id("aws"), None);
        assert_eq!(Provider::from_id("nope"), None);
    }

    #[test]
    fn applicability_follows_the_os() {
        use std::env::consts::OS;
        assert_eq!(Provider::Hyperv.applicable(), OS == "windows");
        assert_eq!(Provider::Parallels.applicable(), OS == "macos");
        assert_eq!(Provider::Utm.applicable(), OS == "macos");
        assert_eq!(Provider::Libvirt.applicable(), OS == "linux");
        for remote in [Provider::VmwareEsxi, Provider::Proxmox, Provider::Qemu, Provider::Aws] {
            assert!(remote.applicable(), "{remote:?}");
        }
    }

    #[test]
    fn probes_name_the_hypervisors_tool() {
        assert_eq!(Provider::Virtualbox.probe().unwrap().0, "VBoxManage");
        assert_eq!(Provider::VmwareDesktop.probe().unwrap().0, "vmrun");
        assert_eq!(Provider::Parallels.probe().unwrap().0, "prlctl");
        assert_eq!(Provider::Libvirt.probe().unwrap().0, "virsh");
        assert!(Provider::Qemu.probe().unwrap().0.starts_with("qemu-system-"));
        assert_eq!(qemu_binary(), if std::env::consts::ARCH == "aarch64" { "qemu-system-aarch64" } else { "qemu-system-x86_64" });
        for none in [Provider::Hyperv, Provider::Utm, Provider::VmwareEsxi, Provider::Proxmox, Provider::Gcp, Provider::Oci] {
            assert!(none.probe().is_none(), "{none:?}");
        }
    }

    #[tokio::test]
    async fn remote_providers_have_nothing_local_to_check() {
        assert_eq!(Provider::Proxmox.hypervisor_installed().await, None);
        assert_eq!(Provider::Hyperv.hypervisor_installed().await, None);
        assert_eq!(Provider::Utm.hypervisor_installed().await, Some(std::path::Path::new(UTM_APP).exists()));
        for p in [Provider::VmwareEsxi, Provider::Proxmox, Provider::Aws, Provider::Linode] {
            assert_eq!(ensure_usable(p).await, Ok(()));
        }
    }

    #[test]
    fn kvm_problems_are_explained() {
        // Whatever this machine has, an answer is either nothing or a sentence about /dev/kvm.
        if let Some(why) = kvm_problem() {
            assert!(why.contains("/dev/kvm"), "{why}");
        }
    }

    #[test]
    fn the_first_missing_layer_is_the_reason() {
        let r = |p, vagrant, stuck, hv, plugin, installed| unavailable_reason(p, vagrant, stuck, hv, plugin, installed);
        assert_eq!(r(Provider::Virtualbox, false, true, Some(false), None, false).as_deref(), Some("Vagrant is not installed"));
        assert!(r(Provider::Virtualbox, true, true, Some(true), None, true).unwrap().contains("isn't responding"));
        assert_eq!(r(Provider::Virtualbox, true, false, Some(false), None, true).as_deref(), Some("`VBoxManage` was not found"));
        assert_eq!(r(Provider::Utm, true, false, Some(false), Some("vagrant_utm"), true).as_deref(), Some("not installed"));
        assert_eq!(
            r(Provider::Parallels, true, false, Some(true), Some("vagrant-parallels"), false).as_deref(),
            Some("Vagrant plugin `vagrant-parallels` is not installed")
        );
        assert_eq!(r(Provider::Virtualbox, true, false, Some(true), None, true), None);
        assert_eq!(r(Provider::VmwareEsxi, true, false, None, Some("vagrant-vmware-esxi"), true), None);
        // libvirt also needs a usable /dev/kvm.
        assert_eq!(r(Provider::Libvirt, true, false, Some(true), Some("vagrant-libvirt"), true), kvm_problem());
    }

    #[test]
    fn statuses_serialize_in_camel_case() {
        let s = ProviderStatus {
            provider: Provider::Virtualbox,
            remote: false,
            available: true,
            hypervisor: Some(true),
            plugin: None,
            plugin_installed: true,
            reason: None,
        };
        let v = serde_json::to_value(s).unwrap();
        assert_eq!(v["provider"], "virtualbox");
        assert_eq!(v["pluginInstalled"], true);
    }
}
