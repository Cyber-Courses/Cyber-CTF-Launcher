//! The learner's attack box as a machine of the lab: on a cloud lab run as one VM per machine
//! there is no host to put a box container in, so the box is one more machine of the lab, added
//! to the launcher's layer over its spec (`lab::overlay`) and brought up by updating the run.
//! Everything that makes it an attack box is the launcher's: its name (`attackbox`, its host
//! name too), Kali, the toolset its set-up installs, the network and address it takes.

use std::path::Path;

use serde_yaml_ng::Value;

use super::lab::{self, overlay};
use crate::error::{Error, Result};

/// The machine's name in the lab, and so its host name.
pub(crate) const NAME: &str = "attackbox";

/// Its set-up, a file of the lab's project (the project is what a machine gets): Kali's headless
/// toolset, what the launcher's attack box image installs, unattended.
const SETUP_FILE: &str = "cyberctf-attackbox.sh";
const SETUP: &str = "#!/bin/sh\n\
# The Cyber CTF attack box: Kali's headless toolset, as the attack box image has it.\n\
set -e\n\
export DEBIAN_FRONTEND=noninteractive\n\
apt-get update\n\
apt-get install -y -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold kali-linux-headless\n";

/// Its disk: room for Kali's headless toolset (about 12 GB) beside the image.
const DISK_GB: u32 = 40;

/// The highest address the box takes on its network: the ones above are the gateway's, the
/// tools' and Isoloom's controller and router's.
const HIGHEST: u8 = 249;

/// Adds the box to the lab in `dir`: on the network of the lab's first machine, at the highest
/// address no machine has there, booted from Kali, set up with [`SETUP`].
pub(crate) fn add(dir: &Path) -> Result<()> {
    let spec = lab::load(dir)?;
    let network = spec
        .machines
        .values()
        .find_map(|m| m.networks.keys().next().cloned())
        .ok_or_else(|| Error::Invalid("this lab has no network to put an attack box on".into()))?;
    let taken: Vec<u8> = spec.machines.values().filter_map(|m| m.networks.get(&network).copied()).collect();
    let octet =
        (2..=HIGHEST).rev().find(|o| !taken.contains(o)).ok_or_else(|| Error::Invalid(format!("no free address left on {network} for an attack box")))?;
    std::fs::write(dir.join(SETUP_FILE), SETUP)?;
    let machine = serde_yaml_ng::from_str::<Value>(&format!(
        "{{ os: kali@rolling, access: true, networks: {{ {network:?}: {octet} }}, resources: {{ disk_gb: {DISK_GB} }}, app: {{ steps: [{{ run: {SETUP_FILE} }}] }} }}"
    ))
    .map_err(|e| Error::Invalid(format!("the attack box's spec: {e}")))?;
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
        add(&dir).unwrap();
        assert!(added(&dir));
        let spec = lab::load(&dir).unwrap();
        let attackbox = &spec.machines[NAME];
        assert_eq!(attackbox.networks.get("lan"), Some(&248), "the highest address left");
        assert!(attackbox.access, "the lab's access machine");
        assert!(dir.join(SETUP_FILE).is_file());
        remove(&dir).unwrap();
        assert!(!added(&dir) && !dir.join(SETUP_FILE).exists());
        assert!(!lab::load(&dir).unwrap().machines.contains_key(NAME));
    }
}
