//! The host networks the VM self-test makes, removed after it (#353): VirtualBox's host-only
//! adapter (`vboxnet<n>`, its DHCP server and route) for the ping check's private network, or on
//! VirtualBox 7 on a Mac its host-only network; libvirt's management network (`vagrant-libvirt`).
//! Vagrant never removes them, and a route to 192.168.56.0/24 can collide with a VPN's ranges.
//!
//! Only what the test made goes: the networks there before it ([`Networks::here`], read before
//! the VM boots) stay, and so does one a VM still uses once the test VM is deleted (the player's
//! own Vagrant projects share them). Linux and macOS only: on Windows removing a host-only adapter
//! asks for elevation, which the test never does.

use crate::platform::exec::run;
use crate::runtime::providers::Provider;

/// The host networks seen at one moment, by kind.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Networks {
    /// VirtualBox's host-only adapters (`VBoxManage list hostonlyifs`).
    pub(super) vbox_ifs: Vec<String>,
    /// VirtualBox 7's host-only networks, on a Mac (`VBoxManage list hostonlynets`).
    pub(super) vbox_nets: Vec<String>,
    /// libvirt's networks on the system instance (`virsh net-list --all --name`).
    pub(super) libvirt: Vec<String>,
}

/// The `Name:` values of a `VBoxManage list …` listing.
fn vbox_names(out: &str) -> Vec<String> {
    out.lines().filter_map(|l| l.strip_prefix("Name:")).map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()).collect()
}

/// The non-empty lines of a listing.
fn lines(out: &str) -> Vec<String> {
    out.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect()
}

impl Networks {
    /// What is here now of the networks a test on `provider` can make (VirtualBox's, libvirt's;
    /// none for another hypervisor); a tool that fails lists nothing.
    pub(super) async fn here(provider: Provider) -> Networks {
        if cfg!(windows) {
            return Networks::default();
        }
        let listed = |program: &'static str, args: &'static [&'static str], parse: fn(&str) -> Vec<String>| async move {
            run(program, args, None).await.map(|out| parse(&out)).unwrap_or_default()
        };
        match provider {
            Provider::Virtualbox => Networks {
                vbox_ifs: listed("VBoxManage", &["list", "hostonlyifs"], vbox_names).await,
                vbox_nets: listed("VBoxManage", &["list", "hostonlynets"], vbox_names).await,
                libvirt: Vec::new(),
            },
            Provider::Libvirt => {
                Networks { libvirt: listed("virsh", &["-c", "qemu:///system", "net-list", "--all", "--name"], lines).await, ..Networks::default() }
            }
            _ => Networks::default(),
        }
    }

    /// What is in `self` and wasn't in `before`.
    fn made_since(&self, before: &Networks) -> Networks {
        let new = |now: &[String], then: &[String]| now.iter().filter(|n| !then.contains(n)).cloned().collect();
        Networks {
            vbox_ifs: new(&self.vbox_ifs, &before.vbox_ifs),
            vbox_nets: new(&self.vbox_nets, &before.vbox_nets),
            libvirt: new(&self.libvirt, &before.libvirt),
        }
    }
}

/// Removes the networks made on `provider` since `before` (read before the test VM booted) that
/// no VM uses now the test VM is deleted. The names removed; an error, which ends nothing (the
/// test's result stands), when one couldn't be.
pub(super) async fn remove_made_since(provider: Provider, before: &Networks) -> Result<Vec<String>, String> {
    if cfg!(windows) {
        return Ok(Vec::new());
    }
    let made = Networks::here(provider).await.made_since(before);
    let mut done = Removal::default();
    remove_vbox(&made, &mut done).await;
    remove_libvirt(&made.libvirt, &mut done).await;
    if done.failed.is_empty() { Ok(done.removed) } else { Err(done.failed.join("; ")) }
}

/// What a removal did: the networks removed, and the ones that couldn't be, each with why.
#[derive(Default)]
struct Removal {
    /// Removed.
    removed: Vec<String>,
    /// Not removed, with the tool's words.
    failed: Vec<String>,
}

impl Removal {
    /// Notes how removing `name` went.
    fn note<T>(&mut self, name: &str, went: crate::error::Result<T>) {
        match went {
            Ok(_) => self.removed.push(name.to_owned()),
            Err(e) => self.failed.push(format!("{name}: {e}")),
        }
    }
}

/// Removes VirtualBox's host-only adapters (each with its DHCP server) and networks of `made` that
/// no registered VM is attached to.
async fn remove_vbox(made: &Networks, done: &mut Removal) {
    if made.vbox_ifs.is_empty() && made.vbox_nets.is_empty() {
        return;
    }
    let vms = run("VBoxManage", &["list", "vms", "--long"], None).await.unwrap_or_default();
    for name in made.vbox_ifs.iter().filter(|n| !vbox_uses(&vms, "Host-only Interface", n)) {
        let went = run("VBoxManage", &["hostonlyif", "remove", name], None).await;
        if went.is_ok() {
            // Its DHCP server goes with it; one never made is no error.
            let _ = run("VBoxManage", &["dhcpserver", "remove", &format!("--network=HostInterfaceNetworking-{name}")], None).await;
        }
        done.note(name, went);
    }
    for name in made.vbox_nets.iter().filter(|n| !vbox_uses(&vms, "Host-only Network", n)) {
        done.note(name, run("VBoxManage", &["hostonlynet", "remove", &format!("--name={name}")], None).await);
    }
}

/// Removes the libvirt networks `made` that no domain has an interface on.
async fn remove_libvirt(made: &[String], done: &mut Removal) {
    for name in made {
        if libvirt_in_use(name).await {
            continue;
        }
        let _ = run("virsh", &["-c", "qemu:///system", "net-destroy", name], None).await;
        done.note(name, run("virsh", &["-c", "qemu:///system", "net-undefine", name], None).await);
    }
}

/// Whether a VM of `VBoxManage list vms --long` (`vms`) is attached to `kind` `name`
/// (`Attachment: Host-only Interface 'vboxnet0'`).
fn vbox_uses(vms: &str, kind: &str, name: &str) -> bool {
    vms.lines().any(|l| l.contains(kind) && l.contains(&format!("'{name}'")))
}

/// Whether a libvirt domain of the system instance has an interface on network `name`.
async fn libvirt_in_use(name: &str) -> bool {
    let Ok(domains) = run("virsh", &["-c", "qemu:///system", "list", "--all", "--name"], None).await else { return true };
    for domain in lines(&domains) {
        // Not told: in use, so it stays.
        let Ok(ifs) = run("virsh", &["-c", "qemu:///system", "domiflist", &domain], None).await else { return true };
        if ifs.lines().any(|l| l.split_whitespace().nth(2) == Some(name)) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::exec::fake::{Tools, ok};

    #[test]
    fn the_names_listed_are_read_and_what_is_new_is_told_apart() {
        let listing = "Name:            vboxnet0\nGUID:            786f\n\nName:            vboxnet1\n";
        assert_eq!(vbox_names(listing), ["vboxnet0", "vboxnet1"]);
        let before = Networks { vbox_ifs: vec!["vboxnet0".into()], libvirt: vec!["default".into()], ..Networks::default() };
        let now =
            Networks { vbox_ifs: vec!["vboxnet0".into(), "vboxnet1".into()], libvirt: vec!["default".into(), "vagrant-libvirt".into()], ..Networks::default() };
        assert_eq!(now.made_since(&before), Networks { vbox_ifs: vec!["vboxnet1".into()], libvirt: vec!["vagrant-libvirt".into()], ..Networks::default() });
        assert!(vbox_uses("NIC 2:  MAC: 0800, Attachment: Host-only Interface 'vboxnet1', Cable connected: on", "Host-only Interface", "vboxnet1"));
        assert!(!vbox_uses("NIC 2:  MAC: 0800, Attachment: Host-only Interface 'vboxnet10'", "Host-only Interface", "vboxnet1"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn only_the_networks_the_test_made_and_nothing_uses_are_removed() {
        // VirtualBox. There before: vboxnet0. The test made vboxnet1 (no VM on it once the test VM
        // is gone) and vboxnet2 (a VM of the player's joined it meanwhile, so it stays).
        let tools = Tools::strict_rules(&[
            ("VBoxManage list hostonlyifs", ok("Name: vboxnet0\nName: vboxnet1\nName: vboxnet2\n")),
            ("VBoxManage list hostonlynets", ok("")),
            ("VBoxManage list vms --long", ok("Name: mine\nNIC 1:  MAC: 08, Attachment: Host-only Interface 'vboxnet2', Cable connected: on\n")),
            ("VBoxManage hostonlyif remove vboxnet1", ok("")),
            ("VBoxManage dhcpserver remove --network=HostInterfaceNetworking-vboxnet1", ok("")),
        ]);
        let before = Networks { vbox_ifs: vec!["vboxnet0".into()], ..Networks::default() };
        assert_eq!(remove_made_since(Provider::Virtualbox, &before).await, Ok(vec!["vboxnet1".to_owned()]));
        drop(tools);
        // libvirt. There before: default (the player's VM is on it). The test made vagrant-libvirt.
        let _tools = Tools::strict_rules(&[
            ("virsh -c qemu:///system net-list --all --name", ok("default\nvagrant-libvirt\n")),
            ("virsh -c qemu:///system list --all --name", ok("players-vm\n")),
            (
                "virsh -c qemu:///system domiflist players-vm",
                ok(" Interface   Type      Source    Model    MAC\n vnet0       network   default   virtio   52:54\n"),
            ),
            ("virsh -c qemu:///system net-destroy vagrant-libvirt", ok("")),
            ("virsh -c qemu:///system net-undefine vagrant-libvirt", ok("")),
        ]);
        let before = Networks { libvirt: vec!["default".into()], ..Networks::default() };
        assert_eq!(remove_made_since(Provider::Libvirt, &before).await, Ok(vec!["vagrant-libvirt".to_owned()]));
        // A network there before the test is never removed, whatever uses it.
        let all = Networks { libvirt: vec!["default".into(), "vagrant-libvirt".into()], ..Networks::default() };
        assert_eq!(remove_made_since(Provider::Libvirt, &all).await, Ok(Vec::new()));
    }
}
