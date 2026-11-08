//! The learner's attack box as a machine of the lab, wherever the lab runs: one more machine,
//! added to the launcher's layer over the lab's spec (`lab::overlay`), the lab regenerated, and
//! the run brought up to date with it (`place`); taken out the same way. Everything that makes it
//! an attack box is the launcher's: its name (`attackbox`, its host name too), what it runs (the
//! attack image picked in Settings beside containers, Kali with its toolset among VMs), the
//! network and address it takes.

mod place;

pub(crate) use place::{Host, Placed};

use std::path::Path;

use serde_yaml_ng::Value;

use super::lab::{self, overlay};
use crate::error::{Error, Result};

/// The machine's name in the lab, and so its host name.
pub(crate) const NAME: &str = "attackbox";

/// Its set-up, a file of the lab's project (the project is what a machine gets): Kali's headless
/// toolset, what the launcher's attack box image installs, unattended. A Debian VM (where Kali
/// has no image for the computer's CPU) is made Kali first: Kali's archive in place of Debian's,
/// pinned over what Debian installed, keeping its own kernel (Kali's doesn't fit the image's boot
/// partition).
const SETUP_FILE: &str = "cyberctf-attackbox.sh";
const SETUP: &str = r"#!/bin/sh
# The Cyber CTF attack box: Kali's headless toolset, as the attack box image has it.
set -e
export DEBIAN_FRONTEND=noninteractive
# Configuration files kept as they are; on a Debian made Kali, Debian's libraries renamed in Kali
# (the t64 move) hand their files over to the new ones.
keep='-o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold -o Dpkg::Options::=--force-overwrite'
if ! grep -q '^ID=kali' /etc/os-release; then
  apt-get update
  apt-get install -y ca-certificates curl
  curl -fsSL https://archive.kali.org/archive-keyring.gpg -o /usr/share/keyrings/kali-archive-keyring.gpg
  echo 'deb [signed-by=/usr/share/keyrings/kali-archive-keyring.gpg] http://http.kali.org/kali kali-rolling main contrib non-free non-free-firmware' > /etc/apt/sources.list.d/kali.list
  # Kali's archive only, over what Debian installed (its versions win, older ones included).
  rm -f /etc/apt/sources.list.d/debian.sources; : > /etc/apt/sources.list
  printf 'Package: *\nPin: release o=Kali\nPin-Priority: 1001\n' > /etc/apt/preferences.d/kali
  apt-get update
  # Its own kernel stays: Kali's needs more room than the image's boot partition has.
  apt-mark hold $(dpkg -l 'linux-image-*' | awk '/^ii/ {print $2}')
  apt-get full-upgrade -y --allow-downgrades $keep
  apt-get install -f -y $keep
fi
apt-get update
apt-get install -y $keep kali-linux-headless
";

/// Its disk: room for Kali's headless toolset (about 12 GB) beside the image.
const DISK_GB: u32 = 40;

/// The highest address the box takes on its network: the ones above are the gateway's, the
/// tools' and Isoloom's controller and router's.
const HIGHEST: u8 = 249;

/// What the box is, by the lab's form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind<'a> {
    /// A container beside the lab's, from `image`, kept up idle.
    Container { image: &'a str },
    /// A VM among the lab's, booted from Kali, set up with [`SETUP`].
    Vm,
}

/// The box's own CPU, when it isn't the lab's: `arm64` on an Arm computer running the lab
/// itself (the box runs natively there, where Kali's Vagrant box is for amd64 only: a Debian
/// VM made Kali by its set-up).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cpu {
    /// The lab's.
    Lab,
    /// Arm (`arm64`).
    Arm,
}

/// Adds the box to the lab in `dir` as `kind`: on `network` (else the network of the lab's first
/// machine), at the highest address no machine has there.
pub(crate) fn add(dir: &Path, kind: Kind<'_>, cpu: Cpu, network: Option<&str>) -> Result<()> {
    let spec = lab::load(dir)?;
    let network = match network {
        Some(n) if spec.networks.contains_key(n) => n.to_owned(),
        Some(n) => return Err(Error::Invalid(format!("this lab has no network {n:?}"))),
        None => spec
            .machines
            .values()
            .find_map(|m| m.networks.keys().next().cloned())
            .ok_or_else(|| Error::Invalid("this lab has no network to put an attack box on".into()))?,
    };
    let taken: Vec<u8> = spec.machines.values().filter_map(|m| m.networks.get(&network).copied()).collect();
    let octet =
        (2..=HIGHEST).rev().find(|o| !taken.contains(o)).ok_or_else(|| Error::Invalid(format!("no free address left on {network} for an attack box")))?;
    let hardware = match cpu {
        Cpu::Lab => "",
        Cpu::Arm => "hardware: { arch: arm64 }, ",
    };
    let machine = match (kind, cpu) {
        (Kind::Container { image }, _) => {
            format!("{{ {hardware}app: {{ image: {image:?} }}, command: sleep infinity, networks: {{ {network:?}: {octet} }} }}")
        }
        (Kind::Vm, cpu) => {
            std::fs::write(dir.join(SETUP_FILE), SETUP)?;
            let os = if cpu == Cpu::Arm { "debian@12" } else { "kali@rolling" };
            format!(
                "{{ os: {os}, {hardware}networks: {{ {network:?}: {octet} }}, resources: {{ disk_gb: {DISK_GB} }}, app: {{ steps: [{{ run: {SETUP_FILE} }}] }} }}"
            )
        }
    };
    let machine = serde_yaml_ng::from_str::<Value>(&machine).map_err(|e| Error::Invalid(format!("the attack box's spec: {e}")))?;
    overlay::add_machine(dir, NAME, machine)
}

/// Takes the box out of the lab in `dir`.
pub(crate) fn remove(dir: &Path) -> Result<()> {
    overlay::remove_machine(dir, NAME)?;
    match std::fs::remove_file(dir.join(SETUP_FILE)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Whether the lab in `dir` has the box.
pub(crate) fn added(dir: &Path) -> bool {
    overlay::has_machine(dir, NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_box_is_a_kali_machine_on_the_first_machines_network_at_a_free_address() {
        let dir = crate::runtime::test_support::write_lab(
            "version: 1\nname: t\nnetworks: { lan: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { os: debian@12, networks: { lan: 249 } }\n",
        );
        add(&dir, Kind::Vm, Cpu::Lab, None).unwrap();
        assert!(added(&dir));
        let spec = lab::load(&dir).unwrap();
        let attackbox = &spec.machines[NAME];
        assert_eq!(attackbox.networks.get("lan"), Some(&248), "the highest address left");
        assert!(!attackbox.access, "an ordinary machine: up with the lab, not on demand");
        assert!(dir.join(SETUP_FILE).is_file());
        remove(&dir).unwrap();
        assert!(!added(&dir) && !dir.join(SETUP_FILE).exists());
        assert!(!lab::load(&dir).unwrap().machines.contains_key(NAME));
    }

    #[test]
    fn beside_containers_the_box_runs_the_picked_image_on_the_picked_network() {
        let dir = crate::runtime::test_support::write_lab(
            "version: 1\nname: t\nnetworks: { lan: { cidr: 10.70.0.0/24 }, dmz: { cidr: 10.71.0.0/24 } }\nmachines:\n  web: { app: { image: nginx }, networks: { lan: 10, dmz: 10 } }\n",
        );
        add(&dir, Kind::Container { image: "kalilinux/kali-rolling" }, Cpu::Lab, Some("dmz")).unwrap();
        let spec = lab::load(&dir).unwrap();
        let attackbox = &spec.machines[NAME];
        assert_eq!(attackbox.networks.keys().collect::<Vec<_>>(), ["dmz"]);
        assert!(!dir.join(SETUP_FILE).exists(), "no set-up: the image is the box");
        assert!(add(&dir, Kind::Container { image: "x" }, Cpu::Lab, Some("nope")).is_err(), "no such network");
        remove(&dir).unwrap();
    }

    #[test]
    fn on_an_arm_computer_the_box_runs_natively_a_debian_vm_made_kali() {
        let dir = crate::runtime::test_support::write_lab(
            "version: 1\nname: t\nnetworks: { lan: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { os: debian@12, networks: { lan: 10 } }\n",
        );
        add(&dir, Kind::Vm, Cpu::Arm, None).unwrap();
        let spec = lab::load(&dir).unwrap();
        let attackbox = &spec.machines[NAME];
        assert_eq!(attackbox.os.as_deref(), Some("debian@12"));
        assert_eq!(attackbox.arch, isoloom_spec::Arch::Arm64);
        assert!(std::fs::read_to_string(dir.join(SETUP_FILE)).unwrap().contains("kali-archive-keyring"));
        remove(&dir).unwrap();
    }
}
