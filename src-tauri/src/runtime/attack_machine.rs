//! The learner's attack box as a machine of the lab, wherever the lab runs: added to the
//! launcher's layer over the lab's spec (`lab::overlay`), the lab regenerated, and the run brought
//! up to date with it (`place`); taken out the same way. A lab that declares an access machine
//! says with it where the player sits: the box takes its place (its name, its networks and
//! addresses, so the checks from that seat run from the box). Otherwise the box is `attackbox` on
//! the lab's first network. What it runs is the launcher's: the attack image picked in Settings
//! ([`default_image`] unless the learner picked another) beside containers, Kali with its toolset
//! among VMs. On every form the player works as [`PLAYER`], an account of the box with sudo (its
//! shell opens as it) and a password of the box's own for wherever the box asks for one (its
//! screen, `su`, SSH): [`mod@sign_in`]. Where the lab runs, and how the box goes in and out of its
//! run: [`locate`], [`Placed`]; the box as the lab page shows it: [`AttackStatus`].

mod place;
mod sign_in;
mod status;

pub(crate) use place::{Pick, Placed, locate};
pub use sign_in::SignIn;
pub(crate) use status::AttackStatus;

use std::path::Path;

use serde_yaml_ng::Value;

use super::lab::{self, overlay};
use crate::error::{Error, Result};

/// The box's name (and host name) in a lab that declares no access machine.
pub(crate) const NAME: &str = "attackbox";

/// The account the player works as in the box, whatever its form: its shell opens as it, in its
/// home, with sudo that asks no password, and a password of its box's own ([`mod@sign_in`]) where
/// something else asks (the screen locker, a login, `su`, SSH). The box makes it
/// ([`PLAYER_ACCOUNT`]); the attack box image has it already (`images/attack-box/Dockerfile`,
/// which a test holds to this name), without a password.
pub(crate) const PLAYER: &str = "player";

/// The script that makes [`PLAYER`] in the box, as root, where it is missing, lets it write in
/// the shared folder and gives it its password: the image's build runs it, and the launcher where
/// the box starts or is set up ([`player_account`]).
const PLAYER_ACCOUNT: &str = include_str!("../../../images/attack-box/player-account.sh");

/// [`PLAYER_ACCOUNT`] for [`PLAYER`], with the shared folder at its mount point and its password's
/// hash in the box's root-only file ([`sign_in::HASH_FILE`], which the box's spec places): one
/// `sh -c` command, the script's comments and blank lines left out. The command names the file,
/// never the password nor its hash: the script hands the hash to `chpasswd` on its standard input.
fn player_account() -> String {
    let script: Vec<&str> = PLAYER_ACCOUNT.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')).collect();
    format!(
        "account={PLAYER} shared={} password_hash={} sh -c {}",
        crate::shared_folder::MOUNT_POINT,
        sign_in::HASH_FILE,
        shell_words::quote(&script.join("\n"))
    )
}

/// What a container box runs: [`player_account`] (a box whose image predates the account gets it;
/// one whose shared folder's owner differs from the image's takes its uid), then idle. A box whose
/// account can't be made still starts: its shell then says why it can't open.
fn container_command() -> String {
    format!("{} || echo 'the {PLAYER} account could not be made' >&2; exec sleep infinity", player_account())
}

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
/// its signing keys checked against [`KALI_ARCHIVE_KEYS`] before apt trusts them, pinned over
/// what Debian installed, keeping its own kernel (Kali's doesn't fit the image's boot partition).
const SETUP_FILE: &str = "cyberctf-attackbox.sh";

/// The primary keys of Kali's archive keyring (`archive.kali.org/archive-keyring.gpg`): its 2025
/// automatic signing key, which signs the archive, and the repository key before it, which the
/// keyring still holds. A keyring with any other key, or without one of these, is refused.
const KALI_ARCHIVE_KEYS: [&str; 2] = ["827C8569F2518CC677FECA1AED65462EC8D5E4C5", "44C6513A8E4FB3D30875F758ED444FF07D8D0BF6"];

/// The set-up script: [`SETUP_START`], Kali's archive keyring downloaded into a private work
/// directory and checked there, then [`SETUP_REST`], then the player's account
/// ([`player_account`], once the toolset brought sudo), then the desktop when it has one.
fn setup_script(desktop: bool) -> String {
    use crate::platform::root_script::{NEED_GPG, PRIVATE_WORKDIR, check_keys};
    let keyring = format!(
        "  {PRIVATE_WORKDIR}\n  {NEED_GPG}\n  curl -fsSL https://archive.kali.org/archive-keyring.gpg -o \"$d/key\"\n  {}\n  install -m 0644 \"$d/key\" /usr/share/keyrings/kali-archive-keyring.gpg\n",
        check_keys(&KALI_ARCHIVE_KEYS, "Kali's archive")
    );
    [SETUP_START, &keyring, SETUP_REST, &player_account(), "\n", if desktop { DESKTOP } else { "" }].concat()
}

/// The set-up up to Kali's archive keyring, on a Debian VM.
const SETUP_START: &str = r"#!/bin/sh
# The Cyber CTF attack box: Kali's headless toolset, as the attack box image has it.
set -e
export DEBIAN_FRONTEND=noninteractive
# Configuration files kept as they are; on a Debian made Kali, Debian's libraries renamed in Kali
# (the t64 move) hand their files over to the new ones.
keep='-o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold -o Dpkg::Options::=--force-overwrite'
if ! grep -q '^ID=kali' /etc/os-release; then
  apt-get update
  apt-get install -y ca-certificates curl gnupg
";

/// The set-up once Kali's archive keyring is in place.
const SETUP_REST: &str = r"  echo 'deb [signed-by=/usr/share/keyrings/kali-archive-keyring.gpg] http://http.kali.org/kali kali-rolling main contrib non-free non-free-firmware' > /etc/apt/sources.list.d/kali.list
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

/// Its disk: room for Kali's headless toolset (about 12 GB) beside the image, and for the desktop
/// (about 3 GB more) when it has one.
const DISK_GB: u32 = 40;

/// What a box with a desktop adds to its set-up, once the player's account is in: Kali's XFCE
/// desktop (about 1 GB to download, 3 GB on its disk) and `LightDM` signing [`PLAYER`] in, so its
/// screen (the lab page's Screen) shows the player's desktop; and no screen lock: neither of
/// Kali's lockers starts for the player, and the session never blanks the screen nor locks it
/// (idle, on suspend, on a switch of user), so the player isn't locked out mid-exercise. The
/// account, its sudo rules and its password stay as [`PLAYER_ACCOUNT`] makes them: `LightDM`'s login
/// after a logout takes that password, as would a locker started by hand.
const DESKTOP: &str = r#"apt-get install -y $keep kali-desktop-xfce lightdm
# LightDM signs the player in to XFCE, on the VM's own screen; its X server never blanks it.
mkdir -p /etc/lightdm/lightdm.conf.d
printf '[Seat:*]\nautologin-user=player\nautologin-user-timeout=0\nautologin-session=xfce\nuser-session=xfce\nxserver-command=X -s 0 -dpms\n' > /etc/lightdm/lightdm.conf.d/50-cyberctf-autologin.conf
groupadd -f -r autologin
usermod -a -G autologin player
# No screen lock: Kali's lockers don't start for the player, and at each sign-in the session
# turns off blanking, the screensaver's lock, the power manager's and the session's own.
mkdir -p /home/player/.config/autostart
for locker in light-locker xfce4-screensaver; do
  printf '[Desktop Entry]\nHidden=true\n' > "/home/player/.config/autostart/$locker.desktop"
done
chown -R player: /home/player/.config
cat > /usr/local/bin/cyberctf-no-screen-lock <<'NOLOCK'
#!/bin/sh
xset s off -dpms
off() { xfconf-query -c "$1" -p "$2" -n -t bool -s false; }
off xfce4-screensaver /saver/enabled
off xfce4-screensaver /lock/enabled
off xfce4-power-manager /xfce4-power-manager/dpms-enabled
off xfce4-power-manager /xfce4-power-manager/lock-screen-suspend-hibernate
off xfce4-session /shutdown/LockScreen
exit 0
NOLOCK
chmod 0755 /usr/local/bin/cyberctf-no-screen-lock
printf '[Desktop Entry]\nType=Application\nName=No screen lock\nExec=/usr/local/bin/cyberctf-no-screen-lock\nNoDisplay=true\n' > /etc/xdg/autostart/cyberctf-no-screen-lock.desktop
systemctl set-default graphical.target
systemctl enable lightdm
systemctl restart lightdm
"#;

/// The memory (MB) and CPUs a box with a desktop gets: XFCE beside Kali's tools wants more than a
/// VM's default.
const DESKTOP_SIZE: (u32, u32) = (4096, 2);

/// The highest address the box takes on its network: the ones above are the gateway's, the
/// tools' and Isoloom's controller and router's.
const HIGHEST: u8 = 249;

/// What the box is, by the lab's form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind<'a> {
    /// A container beside the lab's, from `image`, kept up idle.
    Container {
        /// The image the box's container runs.
        image: &'a str,
    },
    /// A VM among the lab's, booted from Kali, set up with its [`setup_script`]; with Kali's
    /// desktop when `desktop` (the Settings choice), headless otherwise.
    Vm {
        /// It gets Kali's XFCE desktop, the player signed in.
        desktop: bool,
    },
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
/// pick when the lab declares none), with a new password for its player account ([`mod@sign_in`]):
/// kept by the launcher, its hash placed in the box for its set-up.
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
            format!("{{ {hardware}{shared}app: {{ image: {image:?} }}, networks: {{ {networks} }} }}")
        }
        (Kind::Vm { desktop }, cpu) => {
            std::fs::write(dir.join(SETUP_FILE), setup_script(desktop))?;
            let os = if cpu == Cpu::Arm { "debian@12" } else { "kali@rolling" };
            let size = if desktop { format!(", memory_mb: {}, cpus: {}", DESKTOP_SIZE.0, DESKTOP_SIZE.1) } else { String::new() };
            format!(
                "{{ os: {os}, {hardware}{shared}networks: {{ {networks} }}, resources: {{ disk_gb: {DISK_GB}{size} }}, app: {{ steps: [{{ run: {SETUP_FILE} }}] }} }}"
            )
        }
    };
    let mut machine = serde_yaml_ng::from_str::<Value>(&machine).map_err(|e| Error::Invalid(format!("the attack box's spec: {e}")))?;
    if let (Kind::Container { .. }, Value::Mapping(m)) = (kind, &mut machine) {
        m.insert("command".into(), container_command().into());
    }
    if let Value::Mapping(m) = &mut machine {
        m.insert("files".into(), Value::Sequence(vec![sign_in::hash_file(&sign_in::new(dir)?)]));
    }
    let added = overlay::add_machine(dir, &name, machine, overlay::Adding { replaces, attack_box: true });
    if added.is_err() {
        sign_in::forget(dir);
    }
    added
}

/// Takes the box out of the lab in `dir` (a lab's own access machine it stood in for comes back),
/// its password forgotten.
pub(crate) fn remove(dir: &Path) -> Result<()> {
    if let Some(name) = overlay::attack_box(dir)? {
        overlay::remove_machine(dir, &name)?;
        sign_in::forget(dir);
    }
    Ok(crate::platform::fs::remove_if_present(&dir.join(SETUP_FILE))?)
}

/// How the player signs in to the box of the lab in `dir` where it asks: [`PLAYER`] and the
/// password the launcher keeps for it. None when the lab has no box, when `machine` (a screen
/// window's) isn't the box, or when the launcher keeps no password for it.
pub(crate) fn sign_in(dir: &Path, machine: Option<&str>) -> Result<Option<SignIn>> {
    let Some(name) = overlay::attack_box(dir)? else { return Ok(None) };
    if machine.is_some_and(|m| m != name) {
        return Ok(None);
    }
    Ok(sign_in::kept(dir)?.map(|password| SignIn { user: PLAYER.to_owned(), password }))
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
        add_at(&dir, Kind::Vm { desktop: false }, Cpu::Lab, None).unwrap();
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
    fn kalis_archive_keys_are_checked_before_apt_trusts_them() {
        // Regression: the keyring was downloaded straight into apt's keyrings, trusted on TLS
        // alone, with no fingerprint holding it.
        let script = setup_script(false);
        let download = script.find("archive.kali.org/archive-keyring.gpg").unwrap();
        let checked = script.find(r#"[ "$k" = "44C6513A8E4FB3D30875F758ED444FF07D8D0BF6 827C8569F2518CC677FECA1AED65462EC8D5E4C5" ]"#).unwrap();
        let trusted = script.find("/usr/share/keyrings/kali-archive-keyring.gpg").unwrap();
        let used = script.find("signed-by=").unwrap();
        assert!(download < checked && checked < trusted && trusted < used, "{script}");
        assert!(script.contains(r#"-o "$d/key""#) && !script.contains("-o /usr/share/keyrings"), "downloaded into the private folder: {script}");
        assert!(crate::platform::sh_parses(&script), "not valid sh: {script}");
    }

    #[test]
    fn on_an_arm_computer_the_box_runs_natively_a_debian_vm_made_kali() {
        let dir = web_lab();
        add_at(&dir, Kind::Vm { desktop: false }, Cpu::Arm, None).unwrap();
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
            ip: String::new(),
            ports: Vec::new(),
            interfaces: Vec::new(),
            services: Vec::new(),
            infra: false,
            console: None,
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

    /// The box's spec in the launcher's layer over the lab in `dir`, as written.
    /// A lab of one Debian VM, `web`, on `lan`.
    fn web_lab() -> std::path::PathBuf {
        crate::runtime::test_support::write_lab(
            "version: 1\nname: t\nnetworks: { lan: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { os: debian@12, networks: { lan: 10 } }\n",
        )
    }

    fn layer_machine(dir: &Path) -> Value {
        let layer: Value = serde_yaml_ng::from_str(&std::fs::read_to_string(dir.join(overlay::FILE)).unwrap()).unwrap();
        layer["machines"][NAME].clone()
    }

    #[test]
    fn a_vm_box_makes_the_player_account_once_its_toolset_is_in() {
        // Regression: the box's shell opened as the VM's login (`vagrant` on Windows' Hyper-V),
        // the box had no account of the player's.
        let script = setup_script(false);
        let toolset = script.find("install -y $keep kali-linux-headless").unwrap();
        let account = script.find(&format!("account={PLAYER} shared={} password_hash=", crate::shared_folder::MOUNT_POINT)).unwrap();
        assert!(toolset < account, "sudo is in before the account is given it: {script}");
        assert!(script.contains("NOPASSWD: ALL") && script.contains("--create-home"), "{script}");
        assert!(crate::platform::sh_parses(&script), "not valid sh: {script}");
        let dir = web_lab();
        add_at(&dir, Kind::Vm { desktop: false }, Cpu::Lab, None).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(SETUP_FILE)).unwrap(), script);
        assert!(layer_machine(&dir).get("command").is_none(), "a VM box runs no command of its own");
        remove(&dir).unwrap();
    }

    #[test]
    fn a_desktop_box_gets_xfce_and_the_player_signed_in_after_its_account() {
        let headless = setup_script(false);
        assert!(!headless.contains("kali-desktop-xfce") && !headless.contains("lightdm"), "headless by default");
        let script = setup_script(true);
        let account = script.find(&format!("account={PLAYER} ")).unwrap();
        let desktop = script.find("install -y $keep kali-desktop-xfce lightdm").unwrap();
        assert!(account < desktop, "the account the display manager signs in exists first");
        assert!(script.contains("autologin-user=player\\n") && script.contains("systemctl set-default graphical.target"), "{script}");
        assert!(DESKTOP.contains(&format!("autologin-user={PLAYER}")), "the launcher's player account");
        assert!(crate::platform::sh_parses(&script), "not valid sh: {script}");
        let dir = web_lab();
        add_at(&dir, Kind::Vm { desktop: true }, Cpu::Lab, None).unwrap();
        let resources = &layer_machine(&dir)["resources"];
        assert_eq!((resources["memory_mb"].as_u64(), resources["cpus"].as_u64()), (Some(4096), Some(2)), "{resources:?}");
        assert_eq!(std::fs::read_to_string(dir.join(SETUP_FILE)).unwrap(), script);
        remove(&dir).unwrap();
    }

    #[test]
    fn a_container_box_makes_the_player_account_where_it_starts_then_idles() {
        // Its image may predate the account (a pinned older Cyber CTF image, another picked one),
        // and the shared folder's owner may differ from the image's account.
        let dir = crate::runtime::test_support::write_lab(
            "version: 1\nname: t\nnetworks: { lan: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { app: { image: nginx }, networks: { lan: 10 } }\n",
        );
        add_at(&dir, Kind::Container { image: "kalilinux/kali-rolling" }, Cpu::Lab, None).unwrap();
        let command = layer_machine(&dir)["command"].as_str().unwrap().to_owned();
        assert!(command.starts_with(&player_account()), "the account first: {command}");
        assert!(command.ends_with("; exec sleep infinity"), "then idle, whether the account was made or not: {command}");
        assert!(crate::platform::sh_parses(&command), "not valid sh: {command}");
        remove(&dir).unwrap();
    }

    #[test]
    fn the_image_and_its_smoke_test_name_the_launchers_player_account() {
        let image = Path::new(env!("CARGO_MANIFEST_DIR")).join("../images/attack-box");
        let dockerfile = std::fs::read_to_string(image.join("Dockerfile")).unwrap();
        assert!(dockerfile.contains(&format!("ARG PLAYER={PLAYER}\n")), "{dockerfile}");
        assert!(dockerfile.contains("RUN account=$PLAYER sh /tmp/player-account.sh"), "the same script as the launcher's: {dockerfile}");
        assert!(!dockerfile.contains("\nUSER "), "root stays the image's user: the box sets itself up as root");
        let smoke = std::fs::read_to_string(image.join("smoke-test.sh")).unwrap();
        assert!(smoke.contains(&format!("\nplayer={PLAYER}\n")), "{smoke}");
        assert!(smoke.contains(r#"[ "$(docker exec --user "$player" "$name" whoami)" = "$player" ]"#), "{smoke}");
    }

    /// Every file under `dir`, read as text (one that isn't text read as empty).
    fn every_file(dir: &Path) -> String {
        let mut text = String::new();
        let mut todo = vec![dir.to_path_buf()];
        while let Some(d) = todo.pop() {
            for path in std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().path()) {
                if path.is_dir() {
                    todo.push(path);
                } else {
                    text.push_str(&std::fs::read_to_string(&path).unwrap_or_default());
                }
            }
        }
        text
    }

    #[test]
    fn each_box_gets_its_own_password_its_hash_in_a_root_only_file_never_on_a_command_line() {
        // The set-up reads the hash from the file into chpasswd's standard input: the command
        // that runs it names the file only, so neither the password nor its hash shows in `ps`.
        let script = PLAYER_ACCOUNT;
        assert!(script.contains(r#"{ printf '%s:' "$account"; cat "$password_hash"; } | chpasswd -e"#), "{script}");
        assert!(player_account().contains(&format!(" password_hash={} sh -c ", sign_in::HASH_FILE)), "{}", player_account());
        let mut seen = Vec::new();
        // Generated for every target the box goes to: the file is placed on each.
        let containers = [lab::Target::Docker, lab::Target::DockerVm, lab::Target::CloudDocker];
        let vms = [lab::Target::Vagrant, lab::Target::CloudVm];
        for (kind, targets) in [(Kind::Container { image: "kalilinux/kali-rolling" }, &containers[..]), (Kind::Vm { desktop: true }, &vms[..])] {
            let dir = web_lab();
            add_at(&dir, kind, Cpu::Lab, None).unwrap();
            let SignIn { user, password } = sign_in(&dir, None).unwrap().expect("a password kept for the box");
            assert_eq!(user, PLAYER);
            let files = layer_machine(&dir)["files"].clone();
            let [file] = files.as_sequence().unwrap().as_slice() else { panic!("{files:?}") };
            assert_eq!((file["path"].as_str(), file["mode"].as_str()), (Some(sign_in::HASH_FILE), Some("0400")), "root's only: {file:?}");
            let hash = file["content"].as_str().unwrap().trim_end().to_owned();
            assert!(hash.starts_with("$6$") && !hash.contains(&password), "{kind:?}: a SHA-512 crypt hash, not the password");
            let setup = match kind {
                Kind::Container { .. } => layer_machine(&dir)["command"].as_str().unwrap().to_owned(),
                Kind::Vm { .. } => std::fs::read_to_string(dir.join(SETUP_FILE)).unwrap(),
            };
            assert!(setup.contains(&player_account()), "{kind:?}: the set-up gives the account its password");
            assert!(!setup.contains(&password) && !setup.contains(&hash), "{kind:?}: neither on its command line");
            for target in targets {
                lab::prepare(&dir, *target).unwrap_or_else(|e| panic!("{kind:?} on {target:?}: {e}"));
            }
            let everything = every_file(&dir);
            assert!(!everything.contains(&password), "{kind:?}: the password is in none of the lab's files");
            seen.push(password);
            remove(&dir).unwrap();
            assert_eq!(sign_in(&dir, None).unwrap(), None, "{kind:?}: forgotten with the box");
        }
        assert_ne!(seen[0], seen[1], "one password per box");
    }

    #[test]
    fn a_screen_window_gets_the_sign_in_of_the_box_alone() {
        let dir = web_lab();
        assert_eq!(sign_in(&dir, None).unwrap(), None, "no box, no sign-in");
        add_at(&dir, Kind::Vm { desktop: true }, Cpu::Lab, None).unwrap();
        assert!(sign_in(&dir, Some(NAME)).unwrap().is_some(), "the box's screen");
        assert_eq!(sign_in(&dir, Some("web")).unwrap(), None, "a target's screen: not the box's password");
        remove(&dir).unwrap();
    }

    #[test]
    fn a_desktop_box_never_locks_its_screen() {
        let script = setup_script(true);
        for off in [
            "xserver-command=X -s 0 -dpms",
            "for locker in light-locker xfce4-screensaver",
            "Hidden=true",
            "xset s off -dpms",
            "off xfce4-screensaver /lock/enabled",
            "off xfce4-power-manager /xfce4-power-manager/lock-screen-suspend-hibernate",
            "off xfce4-session /shutdown/LockScreen",
            "/etc/xdg/autostart/cyberctf-no-screen-lock.desktop",
        ] {
            assert!(script.contains(off), "{off}: {script}");
        }
        assert!(DESKTOP.contains(&format!("/home/{PLAYER}/.config/autostart")), "the launcher's player account");
        assert!(crate::platform::sh_parses(&script), "not valid sh: {script}");
        assert!(!setup_script(false).contains("no-screen-lock"), "a headless box has no screen to lock");
    }

    #[test]
    fn the_shared_folder_is_mounted_in_the_box_on_every_kind() {
        let dir = web_lab();
        let folder = Path::new("/Users/someone/CyberCTF/shared");
        for kind in [Kind::Container { image: "kalilinux/kali-rolling" }, Kind::Vm { desktop: false }] {
            add(&dir, kind, Cpu::Lab, None, Some(folder)).unwrap();
            let spec = lab::load(&dir).unwrap();
            let mounted: Vec<(&str, &str)> = spec.machines[NAME].host_folders.iter().map(|h| (h.from.as_str(), h.path.as_str())).collect();
            assert_eq!(mounted, [("/Users/someone/CyberCTF/shared", crate::shared_folder::MOUNT_POINT)], "{kind:?}");
            remove(&dir).unwrap();
        }
    }
}
