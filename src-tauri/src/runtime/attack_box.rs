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
//! run: [`locate`], [`place::Placed`]; the box as the lab page shows it: [`AttackStatus`].

mod place;
mod scripts;
mod sign_in;
mod status;

#[cfg(test)]
pub(crate) use place::Placed;
pub(crate) use place::{Pick, locate, reachable};
pub(crate) use scripts::Scripts;
#[cfg(test)]
pub(crate) use scripts::for_tests as test_scripts;
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
/// something else asks (the screen locker, a login, `su`, SSH). The box makes it with the attack
/// box repository's `player-account.sh` ([`Scripts::player_account`]); the attack box image has it
/// already, under this name (that repository's contract, which its smoke test checks), without a
/// password.
pub(crate) const PLAYER: &str = "player";

/// The learner's shared folder as the box gets it: the folder of this computer, and whether its
/// mount writes as the folder's owner whichever account of the box writes
/// ([`Provider::squashes_folder_writers_on`](crate::runtime::providers::Provider::squashes_folder_writers_on)),
/// where the folder's group is [`PLAYER`]'s own rather than one it joins (`player-account.sh`'s
/// `shared_squashed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shared {
    /// The folder of this computer, mounted at [`crate::shared_folder::MOUNT_POINT`].
    pub(crate) folder: std::path::PathBuf,
    /// Every account of the box writes there as the folder's owner.
    pub(crate) squashed: bool,
}

/// `player-account.sh` ([`Scripts::player_account`]) for [`PLAYER`], with the shared folder at its mount point (`squashed`:
/// written as its owner whoever writes, [`Shared::squashed`]) and its password's hash in the
/// box's root-only file ([`sign_in::HASH_FILE`], which the box's spec places): one `sh -c`
/// command, the script's comments and blank lines left out. The command names the file, never
/// the password nor its hash: the script hands the hash to `chpasswd` on its standard input.
fn player_account(scripts: &Scripts, squashed: bool) -> String {
    let script: Vec<&str> = scripts.player_account.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')).collect();
    format!(
        "account={PLAYER} shared={}{} password_hash={} sh -c {}",
        crate::shared_folder::MOUNT_POINT,
        if squashed { " shared_squashed=1" } else { "" },
        sign_in::HASH_FILE,
        shell_words::quote(&script.join("\n"))
    )
}

/// What a container box runs: [`player_account`] (a box whose image predates the account gets it;
/// one whose shared folder's owner differs from the image's takes its uid), then idle. A box whose
/// account can't be made still starts: its shell then says why it can't open. Docker binds the
/// folder, its ids as they are: never squashed.
fn container_command(scripts: &Scripts) -> String {
    format!("{} || echo 'the {PLAYER} account could not be made' >&2; exec sleep infinity", player_account(scripts, false))
}

/// The default attack image, shared with the frontend's Settings (`src/lib/settings.ts`): the
/// Cyber CTF image, pinned by its digest in `attack-box.json`, which the attack box repository's
/// publish updates (a pull request) once the image passed its smoke test.
const IMAGE_FILE: &str = scripts::PIN;

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

/// The defaults earlier launchers had: the repository unpinned, its floating `latest` tag and
/// each earlier pinned digest ([`IMAGE_FILE`]'s `previous`). A player's saved image equal to one
/// of them follows the current default (`crate::settings`).
pub(crate) fn earlier_default_images() -> &'static [String] {
    #[derive(serde::Deserialize)]
    struct File {
        #[serde(default)]
        previous: Vec<String>,
    }
    static EARLIER: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| {
        let repository = default_image().split('@').next().unwrap_or_default().to_owned();
        let previous = serde_json::from_str::<File>(IMAGE_FILE).map(|f| f.previous).unwrap_or_default();
        [format!("{repository}:latest"), repository].into_iter().chain(previous).collect()
    });
    &EARLIER
}

/// Its set-up, a file of the lab's project (the project is what a machine gets): Kali's headless
/// toolset, what the launcher's attack box image installs, unattended. A Debian VM (where Kali
/// has no image for the computer's CPU) is made Kali first: Kali's archive in place of Debian's,
/// its signing keys checked against Kali's archive keys ([`Scripts::kali`]) before apt trusts them, pinned over
/// what Debian installed, keeping its own kernel (Kali's doesn't fit the image's boot partition).
const SETUP_FILE: &str = "cyberctf-attackbox.sh";

/// The set-up script, from the attack box repository's pinned parts ([`Scripts`], its README says
/// the order): Kali ([`Scripts::kali`]), then Kali's toolset ([`Scripts::toolset`]) unless a test
/// run switched it off ([`toolset_wanted`]), then the player's account ([`player_account`]), then
/// the desktop ([`Scripts::desktop`]) when it has one. `squashed`: the shared folder's
/// [`Shared::squashed`].
fn setup_script(scripts: &Scripts, desktop: bool, squashed: bool) -> String {
    setup_script_with(scripts, desktop, toolset_wanted(), squashed)
}

/// The variable a test run (the integration runner's attack-box-vm scenarios) sets to `1` to make
/// a VM box without Kali's toolset: the bare machine, everything else of the box as it is (its
/// player account, shared folder, network and shell). Installing the toolset took most of those
/// scenarios' time, past their caps. Read in test builds only: the app and the command line never
/// read it, and offer no such choice.
#[cfg(test)]
pub(crate) const SKIP_TOOLSET: &str = "CYBERCTF_TEST_BOX_SKIP_TOOLS";

/// Whether the box's set-up installs Kali's toolset: always, but in a test run that set
/// [`SKIP_TOOLSET`] to `1`.
#[cfg(test)]
fn toolset_wanted() -> bool {
    toolset_wanted_given(std::env::var(SKIP_TOOLSET).ok().as_deref())
}

/// Whether the box's set-up installs Kali's toolset: always, outside a test build.
#[cfg(not(test))]
const fn toolset_wanted() -> bool {
    true
}

/// [`toolset_wanted`] with [`SKIP_TOOLSET`] read as `skip`.
#[cfg(test)]
fn toolset_wanted_given(skip: Option<&str>) -> bool {
    skip != Some("1")
}

/// [`setup_script`] with Kali's toolset installed or not (`toolset`).
fn setup_script_with(scripts: &Scripts, desktop: bool, toolset: bool, squashed: bool) -> String {
    let toolset = if toolset { scripts.toolset.as_str() } else { "# Kali's toolset left out: a test run's bare box (CYBERCTF_TEST_BOX_SKIP_TOOLS).\n" };
    [scripts.kali.as_str(), toolset, &player_account(scripts, squashed), "\n", if desktop { scripts.desktop.as_str() } else { "" }].concat()
}

/// Its disk: room for Kali's headless toolset (about 12 GB) beside the image, and for the desktop
/// (about 3 GB more) when it has one.
const DISK_GB: u32 = 40;

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

/// What goes inside the box: its set-up's scripts (the attack box repository's, [`Scripts`]) and
/// the learner's shared folder, when one is on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Inside<'a> {
    /// The set-up's scripts.
    pub(crate) scripts: &'a Scripts,
    /// The shared folder, mounted in the box.
    pub(crate) shared: Option<&'a Shared>,
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

/// Adds the box to the lab in `dir` as `kind`, with what goes inside it (`inside`: its set-up's
/// scripts and the learner's shared folder), at its seat ([`seat`]; `network` is the player's
/// pick when the lab declares none), with a new password for its player account ([`mod@sign_in`]):
/// kept by the launcher, its hash placed in the box for its set-up.
pub(crate) fn add(dir: &Path, kind: Kind<'_>, cpu: Cpu, network: Option<&str>, inside: Inside<'_>) -> Result<()> {
    let Inside { scripts, shared } = inside;
    let Seat { name, networks, replaces } = seat(&lab::load(dir)?, network)?;
    let networks = networks.iter().map(|(n, o)| format!("{}: {o}", yaml_string(n))).collect::<Vec<_>>().join(", ");
    let hardware = match cpu {
        Cpu::Lab => "",
        Cpu::Arm => "hardware: { arch: arm64 }, ",
    };
    // The learner's shared folder, mounted both ways at the same place on every form: read-write
    // said, since Isoloom mounts a host folder read-only unless told.
    let squashed = shared.is_some_and(|s| s.squashed);
    let shared = shared.map_or_else(String::new, |s| {
        format!(
            "host_folders: [{{ from: {}, path: {}, read_only: false }}], ",
            yaml_string(&s.folder.display().to_string()),
            yaml_string(crate::shared_folder::MOUNT_POINT)
        )
    });
    let machine = match (kind, cpu) {
        (Kind::Container { image }, _) => {
            format!("{{ {hardware}{shared}app: {{ image: {} }}, networks: {{ {networks} }} }}", yaml_string(image))
        }
        (Kind::Vm { desktop }, cpu) => {
            std::fs::write(dir.join(SETUP_FILE), setup_script(scripts, desktop, squashed))?;
            let os = if cpu == Cpu::Arm { "debian@12" } else { "kali@rolling" };
            let size = if desktop { format!(", memory_mb: {}, cpus: {}", DESKTOP_SIZE.0, DESKTOP_SIZE.1) } else { String::new() };
            format!(
                "{{ os: {os}, {hardware}{shared}networks: {{ {networks} }}, resources: {{ disk_gb: {DISK_GB}{size} }}, app: {{ steps: [{{ run: {SETUP_FILE} }}] }} }}"
            )
        }
    };
    let mut machine = serde_yaml_ng::from_str::<Value>(&machine).map_err(|e| Error::Invalid(format!("the attack box's spec: {e}")))?;
    if let (Kind::Container { .. }, Value::Mapping(m)) = (kind, &mut machine) {
        m.insert("command".into(), container_command(scripts).into());
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

/// `s` as a quoted YAML string: a JSON string, which YAML reads as written. Rust's `{:?}` writes
/// escapes YAML refuses (a combining mark as `\u{301}`).
fn yaml_string(s: &str) -> String {
    serde_json::Value::from(s).to_string()
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

/// Deletes the password kept for the box of the lab in `dir`, whether or not the box is in the lab
/// now: the lab itself is being removed, and its keychain entry must not outlive it. None kept, or
/// no keychain here at all, is deleted already; a store that refuses (a locked keychain) says why.
pub(crate) fn forget_password(dir: &Path) -> Result<()> {
    sign_in::delete(dir)
}

/// The name the password of the box of the lab in `dir` is kept under (`attack-box:<lab id>`).
pub(crate) fn password_entry(dir: &Path) -> String {
    sign_in::entry(dir)
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
mod tests;
