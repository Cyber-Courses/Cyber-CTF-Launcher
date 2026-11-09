//! The learner's attack box as a machine of the lab, wherever the lab runs: added to the
//! launcher's layer over the lab's spec (`lab::overlay`), the lab regenerated, and the run brought
//! up to date with it (`place`); taken out the same way. A lab that declares an access machine
//! says with it where the player sits: the box takes its place (its name, its networks and
//! addresses, so the checks from that seat run from the box). Otherwise the box is `attackbox` on
//! the lab's first network. What it runs is the launcher's: the attack image picked in Settings
//! ([`default_image`] unless the learner picked another) beside containers, Kali with its toolset
//! among VMs. Where the lab runs, and how the box goes in and out of its run: [`locate`],
//! [`Placed`]; the box as the lab page shows it: [`AttackStatus`].

mod place;
mod status;

pub(crate) use place::{Placed, locate};
pub(crate) use status::AttackStatus;

use std::path::Path;

use serde_yaml_ng::Value;

use super::lab::{self, overlay};
use crate::error::{Error, Result};

/// The box's name (and host name) in a lab that declares no access machine.
pub(crate) const NAME: &str = "attackbox";

/// The default attack image, shared with the frontend's Settings (`src/lib/settings.ts`): the
/// Cyber CTF image, pinned by its digest in `attack-box-image.json`, which the attack-box workflow
/// updates once a published image passed its smoke test.
const IMAGE_FILE: &str = include_str!("../../attack-box-image.json");

/// The attack image a container box runs when the learner picked none: [`IMAGE_FILE`]'s `image`
/// (its repository unpinned, should the file not read: a test keeps it readable).
pub(crate) fn default_image() -> &'static str {
    #[derive(serde::Deserialize)]
    struct File {
        image: String,
    }
    static IMAGE: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| serde_json::from_str::<File>(IMAGE_FILE).map_or_else(|_| "cyberctf/attack-box".to_owned(), |f| f.image));
    &IMAGE
}

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

/// Where the box sits in a lab: its name, and its address on each network.
struct Seat {
    name: String,
    networks: Vec<(String, u8)>,
    /// It takes the place of the lab's own access machine of that name.
    replaces: bool,
}

/// The lab's own access machine, the seat it declares for the player, if it declares one.
fn declared(spec: &lab::Spec) -> Option<(&String, &lab::Machine)> {
    spec.machines.iter().find(|(_, m)| m.access)
}

/// The seat in `spec`: the declared access machine's, else [`NAME`] on `network` (else the
/// network of the lab's first machine) at the highest address no machine has there.
fn seat(spec: &lab::Spec, network: Option<&str>) -> Result<Seat> {
    if let Some((name, m)) = declared(spec) {
        let networks = m.networks.iter().map(|(n, o)| (n.clone(), *o)).collect();
        return Ok(Seat { name: name.clone(), networks, replaces: true });
    }
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
    Ok(Seat { name: NAME.to_owned(), networks: vec![(network, octet)], replaces: false })
}

/// The box's name in the lab in `dir`: the one added (the layer's attack box), else the lab's own
/// access machine's (the seat it would take), else [`NAME`].
pub(crate) fn name(dir: &Path) -> Result<String> {
    if let Some(name) = overlay::attack_box(dir)? {
        return Ok(name);
    }
    Ok(declared(&lab::load(dir)?).map_or_else(|| NAME.to_owned(), |(n, _)| n.clone()))
}

/// The lab networks the box can join in a lab, for the learner to pick from, and the one it
/// joins unpicked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Networks {
    /// The lab's networks; none when there is nothing to pick: the lab declares where the player
    /// sits (the box takes that seat, whatever is picked), or the box is in already.
    pub(crate) pickable: Vec<String>,
    /// The box's network unpicked: the declared seat's (its first), else the network of the lab's
    /// first machine; once the box is in, the one it joined (its first).
    pub(crate) default: String,
}

/// [`Networks`] for the box in the lab in `dir`, as [`add`] places it.
pub(crate) fn networks(dir: &Path) -> Result<Networks> {
    let spec = lab::load(dir)?;
    if let Some(name) = overlay::attack_box(dir)? {
        let joined = spec.machines.get(&name).and_then(|m| m.networks.keys().next().cloned()).unwrap_or_default();
        return Ok(Networks { pickable: Vec::new(), default: joined });
    }
    let Seat { networks, replaces, .. } = seat(&spec, None)?;
    let default = networks.into_iter().next().map(|(n, _)| n).unwrap_or_default();
    let pickable = if replaces { Vec::new() } else { spec.networks.keys().cloned().collect() };
    Ok(Networks { pickable, default })
}

/// Adds the box to the lab in `dir` as `kind`, at its seat ([`seat`]; `network` is the player's
/// pick when the lab declares none).
pub(crate) fn add(dir: &Path, kind: Kind<'_>, cpu: Cpu, network: Option<&str>, shared: Option<&Path>) -> Result<()> {
    let Seat { name, networks, replaces } = seat(&lab::load(dir)?, network)?;
    let networks = networks.iter().map(|(n, o)| format!("{n:?}: {o}")).collect::<Vec<_>>().join(", ");
    let hardware = match cpu {
        Cpu::Lab => "",
        Cpu::Arm => "hardware: { arch: arm64 }, ",
    };
    // The learner's shared folder, mounted both ways at the same place on every form.
    let shared = shared.map_or_else(String::new, |folder| {
        format!("host_folders: [{{ from: {:?}, path: {:?} }}], ", folder.display().to_string(), crate::shared_folder::MOUNT_POINT)
    });
    let machine = match (kind, cpu) {
        (Kind::Container { image }, _) => {
            format!("{{ {hardware}{shared}app: {{ image: {image:?} }}, command: sleep infinity, networks: {{ {networks} }} }}")
        }
        (Kind::Vm, cpu) => {
            std::fs::write(dir.join(SETUP_FILE), SETUP)?;
            let os = if cpu == Cpu::Arm { "debian@12" } else { "kali@rolling" };
            format!(
                "{{ os: {os}, {hardware}{shared}networks: {{ {networks} }}, resources: {{ disk_gb: {DISK_GB} }}, app: {{ steps: [{{ run: {SETUP_FILE} }}] }} }}"
            )
        }
    };
    let machine = serde_yaml_ng::from_str::<Value>(&machine).map_err(|e| Error::Invalid(format!("the attack box's spec: {e}")))?;
    overlay::add_machine(dir, &name, machine, overlay::Adding { replaces, attack_box: true })
}

/// Takes the box out of the lab in `dir` (a lab's own access machine it stood in for comes back).
pub(crate) fn remove(dir: &Path) -> Result<()> {
    if let Some(name) = overlay::attack_box(dir)? {
        overlay::remove_machine(dir, &name)?;
    }
    match std::fs::remove_file(dir.join(SETUP_FILE)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// The lab's status with its attack box set apart: the box has its own status and panel, and
/// the diagram draws it as the player, so it is neither one of the lab's machines (the diagram
/// would draw it twice) nor what makes the lab running (a lab whose targets are all down is down,
/// its box up or not).
pub(crate) fn set_apart(status: crate::runtime::LabStatus, dir: &Path) -> Result<crate::runtime::LabStatus> {
    let Some(name) = overlay::attack_box(dir)? else { return Ok(status) };
    if !status.machines.iter().any(|m| m.name == name) {
        return Ok(status);
    }
    let machines: Vec<_> = status.machines.into_iter().filter(|m| m.name != name).collect();
    let running = status.running && machines.iter().any(|m| !m.infra && matches!(m.state.as_str(), "running" | "unhealthy"));
    Ok(crate::runtime::LabStatus { running, machines, ..status })
}

/// Whether the lab in `dir` has the box.
pub(crate) fn added(dir: &Path) -> Result<bool> {
    Ok(overlay::attack_box(dir)?.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`add`] with no shared folder.
    fn add_at(dir: &Path, kind: Kind<'_>, cpu: Cpu, network: Option<&str>) -> Result<()> {
        add(dir, kind, cpu, network, None)
    }

    #[test]
    fn the_default_image_is_the_cyber_ctf_image_pinned_by_its_digest() {
        let image = default_image();
        assert!(image.starts_with("cyberctf/attack-box@sha256:"), "{image}");
        assert!(crate::platform::exec::valid_image(image), "{image}");
    }

    #[test]
    fn the_box_is_a_kali_machine_on_the_first_machines_network_at_a_free_address() {
        let dir = crate::runtime::test_support::write_lab(
            "version: 1\nname: t\nnetworks: { lan: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { os: debian@12, networks: { lan: 249 } }\n",
        );
        add_at(&dir, Kind::Vm, Cpu::Lab, None).unwrap();
        assert!(added(&dir).unwrap());
        let spec = lab::load(&dir).unwrap();
        let attackbox = &spec.machines[NAME];
        assert_eq!(attackbox.networks.get("lan"), Some(&248), "the highest address left");
        assert!(!attackbox.access, "an ordinary machine: up with the lab, not on demand");
        assert!(dir.join(SETUP_FILE).is_file());
        remove(&dir).unwrap();
        assert!(!added(&dir).unwrap() && !dir.join(SETUP_FILE).exists());
        assert!(!lab::load(&dir).unwrap().machines.contains_key(NAME));
    }

    #[test]
    fn a_lab_that_declares_where_the_player_sits_gets_the_box_there_in_place_of_its_own() {
        let dir = crate::runtime::test_support::write_lab(
            "version: 1\nname: t\nnetworks: { front: { cidr: 10.70.0.0/24 }, access: { cidr: 10.71.0.0/24 } }\nmachines:\n  web: { os: debian@12, networks: { front: 10 } }\n  user: { access: true, networks: { access: 10 } }\n",
        );
        assert_eq!(name(&dir).unwrap(), "user", "the seat the box would take, before it is added");
        assert_eq!(networks(&dir).unwrap(), Networks { pickable: Vec::new(), default: "access".into() }, "nothing to pick: the declared seat's");
        add_at(&dir, Kind::Container { image: "kalilinux/kali-rolling" }, Cpu::Lab, Some("front")).unwrap();
        let spec = lab::load(&dir).unwrap();
        let user = &spec.machines["user"];
        assert!(!user.access, "an ordinary machine of the lab now, up with it");
        assert_eq!(user.networks.iter().collect::<Vec<_>>(), [(&"access".to_owned(), &10)], "the declared seat, not the pick");
        assert!(!spec.machines.contains_key(NAME));
        assert_eq!(name(&dir).unwrap(), "user");
        remove(&dir).unwrap();
        assert!(lab::load(&dir).unwrap().machines["user"].access, "the lab's own access machine is back");
    }

    #[test]
    fn beside_containers_the_box_runs_the_picked_image_on_the_picked_network() {
        let dir = crate::runtime::test_support::write_lab(
            "version: 1\nname: t\nnetworks: { lan: { cidr: 10.70.0.0/24 }, dmz: { cidr: 10.71.0.0/24 } }\nmachines:\n  web: { app: { image: nginx }, networks: { lan: 10, dmz: 10 } }\n",
        );
        let unpicked = Networks { pickable: vec!["lan".into(), "dmz".into()], default: "lan".into() };
        assert_eq!(networks(&dir).unwrap(), unpicked, "the lab's networks, the first machine's first unpicked");
        add_at(&dir, Kind::Container { image: "kalilinux/kali-rolling" }, Cpu::Lab, Some("dmz")).unwrap();
        let spec = lab::load(&dir).unwrap();
        let attackbox = &spec.machines[NAME];
        assert_eq!(attackbox.networks.keys().collect::<Vec<_>>(), ["dmz"]);
        assert_eq!(networks(&dir).unwrap(), Networks { pickable: Vec::new(), default: "dmz".into() }, "in: the one it joined");
        assert!(!dir.join(SETUP_FILE).exists(), "no set-up: the image is the box");
        assert!(add_at(&dir, Kind::Container { image: "x" }, Cpu::Lab, Some("nope")).is_err(), "no such network");
        remove(&dir).unwrap();
    }

    #[test]
    fn another_machine_of_the_layer_is_never_taken_for_the_box() {
        let dir = crate::runtime::test_support::write_lab(
            "version: 1\nname: t\nnetworks: { lan: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { app: { image: nginx }, networks: { lan: 10 } }\n",
        );
        let other = serde_yaml_ng::from_str::<Value>("{ app: { image: nginx }, networks: { lan: 20 } }").unwrap();
        overlay::add_machine(&dir, "aaa-extra", other, overlay::Adding::default()).unwrap();
        assert!(!added(&dir).unwrap());
        assert_eq!(name(&dir).unwrap(), NAME);
        add_at(&dir, Kind::Container { image: "kalilinux/kali-rolling" }, Cpu::Lab, None).unwrap();
        assert_eq!(name(&dir).unwrap(), NAME, "the box, not the layer's first machine");
        remove(&dir).unwrap();
        let spec = lab::load(&dir).unwrap();
        assert!(spec.machines.contains_key("aaa-extra") && !spec.machines.contains_key(NAME), "only the box is taken out");
    }

    #[test]
    fn on_an_arm_computer_the_box_runs_natively_a_debian_vm_made_kali() {
        let dir = crate::runtime::test_support::write_lab(
            "version: 1\nname: t\nnetworks: { lan: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { os: debian@12, networks: { lan: 10 } }\n",
        );
        add_at(&dir, Kind::Vm, Cpu::Arm, None).unwrap();
        let spec = lab::load(&dir).unwrap();
        let attackbox = &spec.machines[NAME];
        assert_eq!(attackbox.os.as_deref(), Some("debian@12"));
        assert_eq!(attackbox.arch, crate::runtime::lab::Arch::Arm64);
        assert!(std::fs::read_to_string(dir.join(SETUP_FILE)).unwrap().contains("kali-archive-keyring"));
        remove(&dir).unwrap();
    }

    #[test]
    fn the_box_is_set_apart_from_the_labs_machines_and_never_makes_it_running() {
        use crate::runtime::{LabStatus, Machine};
        let dir = crate::runtime::test_support::write_lab(
            "version: 1\nname: t\nnetworks: { lan: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { app: { image: nginx }, networks: { lan: 10 } }\n",
        );
        let machine = |name: &str, state: &str| Machine {
            name: name.into(),
            state: state.into(),
            image: String::new(),
            ip: String::new(),
            ports: Vec::new(),
            interfaces: Vec::new(),
            services: Vec::new(),
            infra: false,
        };
        let status = |web: &str| LabStatus { running: true, machines: vec![machine("web", web), machine(NAME, "running")], ..LabStatus::default() };
        // Not added: a machine of that name is the lab's own.
        assert_eq!(set_apart(status("running"), &dir).unwrap().machines.len(), 2);
        add_at(&dir, Kind::Container { image: "kalilinux/kali-rolling" }, Cpu::Lab, None).unwrap();
        let up = set_apart(status("running"), &dir).unwrap();
        assert_eq!(up.machines.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["web"]);
        assert!(up.running);
        assert!(!set_apart(status("exited"), &dir).unwrap().running, "the box alone up: the lab is down");
        remove(&dir).unwrap();
    }

    #[test]
    fn the_shared_folder_is_mounted_in_the_box_on_every_kind() {
        let dir = crate::runtime::test_support::write_lab(
            "version: 1\nname: t\nnetworks: { lan: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { os: debian@12, networks: { lan: 10 } }\n",
        );
        let folder = Path::new("/Users/someone/CyberCTF/shared");
        for kind in [Kind::Container { image: "kalilinux/kali-rolling" }, Kind::Vm] {
            add(&dir, kind, Cpu::Lab, None, Some(folder)).unwrap();
            let spec = lab::load(&dir).unwrap();
            let mounted: Vec<(&str, &str)> = spec.machines[NAME].host_folders.iter().map(|h| (h.from.as_str(), h.path.as_str())).collect();
            assert_eq!(mounted, [("/Users/someone/CyberCTF/shared", crate::shared_folder::MOUNT_POINT)], "{kind:?}");
            remove(&dir).unwrap();
        }
    }
}
