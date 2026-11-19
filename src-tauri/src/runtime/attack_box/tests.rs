use super::*;

/// `folder` shared with the box as most hypervisors and Docker share it: not squashed.
fn shared(folder: &Path) -> Shared {
    Shared { folder: folder.to_path_buf(), squashed: false }
}

/// [`add`] with no shared folder.
fn add_at(dir: &Path, kind: Kind<'_>, cpu: Cpu, network: Option<&str>) -> Result<()> {
    add(dir, &test_scripts(), kind, cpu, network, None)
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
fn web_lab() -> crate::testing::TempDir {
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
    let script = setup_script(&test_scripts(), false, false);
    let toolset = script.find("install -y $keep kali-linux-headless").unwrap();
    let account = script.find(&format!("account={PLAYER} shared={} password_hash=", crate::shared_folder::MOUNT_POINT)).unwrap();
    assert!(toolset < account, "sudo is in before the account is given it: {script}");
    assert!(crate::platform::sh_parses(&script), "not valid sh: {script}");
    let dir = web_lab();
    add_at(&dir, Kind::Vm { desktop: false }, Cpu::Lab, None).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join(SETUP_FILE)).unwrap(), script);
    assert!(layer_machine(&dir).get("command").is_none(), "a VM box runs no command of its own");
    remove(&dir).unwrap();
}

#[test]
fn a_desktop_box_gets_xfce_and_the_player_signed_in_after_its_account() {
    let headless = setup_script(&test_scripts(), false, false);
    assert!(!headless.contains("kali-desktop-xfce") && !headless.contains("lightdm"), "headless by default");
    let script = setup_script(&test_scripts(), true, false);
    let account = script.find(&format!("account={PLAYER} ")).unwrap();
    let desktop = script.find("install -y $keep kali-desktop-xfce lightdm").unwrap();
    assert!(account < desktop, "the account the display manager signs in exists first");
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
    assert!(command.starts_with(&player_account(&test_scripts(), false)), "the account first: {command}");
    assert!(command.ends_with("; exec sleep infinity"), "then idle, whether the account was made or not: {command}");
    assert!(crate::platform::sh_parses(&command), "not valid sh: {command}");
    remove(&dir).unwrap();
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
    assert!(
        player_account(&test_scripts(), false).contains(&format!(" password_hash={} sh -c ", sign_in::HASH_FILE)),
        "{}",
        player_account(&test_scripts(), false)
    );
    let mut seen = Vec::new();
    // Generated for every target the box goes to: the file is placed on each.
    let containers = [lab::Target::Docker, lab::Target::CloudDocker];
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
        assert!(setup.contains(&player_account(&test_scripts(), false)), "{kind:?}: the set-up gives the account its password");
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
fn the_shared_folder_is_mounted_in_the_box_on_every_kind() {
    let dir = web_lab();
    let folder = Path::new("/Users/someone/CyberCTF/shared");
    for kind in [Kind::Container { image: "kalilinux/kali-rolling" }, Kind::Vm { desktop: false }] {
        add(&dir, kind, Cpu::Lab, None, Some(&shared(folder))).unwrap();
        let spec = lab::load(&dir).unwrap();
        let mounted: Vec<(&str, &str)> = spec.machines[NAME].host_folders.iter().map(|h| (h.from.as_str(), h.path.as_str())).collect();
        assert_eq!(mounted, [("/Users/someone/CyberCTF/shared", crate::shared_folder::MOUNT_POINT)], "{kind:?}");
        remove(&dir).unwrap();
    }
}

/// Regression: the box couldn't write in its shared folder ("Read-only file system"): Isoloom 3.0
/// mounts a host folder read-only unless it says otherwise, and the launcher said nothing. The
/// folder is the player's, both ways: the spec says so, and the box's Compose service mounts it
/// read-write.
#[test]
fn the_shared_folder_is_mounted_read_write_both_ways() {
    let dir = web_lab();
    let folder = crate::testing::temp_dir("attack-box-shared");
    add(&dir, &test_scripts(), Kind::Container { image: "kalilinux/kali-rolling" }, Cpu::Lab, None, Some(&shared(&folder))).unwrap();
    let spec = lab::load(&dir).unwrap();
    assert!(spec.machines[NAME].host_folders.iter().all(|h| !h.read_only), "the spec asks it read-write");
    lab::prepare(&dir, lab::Target::Docker).unwrap();
    let compose: serde_json::Value = serde_yaml_ng::from_str(&std::fs::read_to_string(lab::compose_file(&dir)).unwrap()).unwrap();
    let services = compose["services"].as_object().unwrap();
    let (_, service) = services.iter().find(|(name, _)| name.as_str() == NAME || name.ends_with(&format!("-{NAME}"))).expect("the box's service");
    let shared: Vec<&serde_json::Value> = service["volumes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| {
            v["target"].as_str() == Some(crate::shared_folder::MOUNT_POINT)
                || v.as_str().is_some_and(|s| s.split(':').nth(1) == Some(crate::shared_folder::MOUNT_POINT))
        })
        .collect();
    assert_eq!(shared.len(), 1, "{service}");
    let mounted = shared[0];
    let read_only =
        mounted["read_only"].as_bool() == Some(true) || mounted.as_str().is_some_and(|s| s.split(':').skip(2).any(|o| o.split(',').any(|f| f == "ro")));
    assert!(!read_only, "mounted read-write: {mounted}");
    remove(&dir).unwrap();
}

#[test]
fn a_shared_folder_with_a_combining_mark_in_its_name_is_mounted() {
    // Regression: the folder was quoted with Rust's `{:?}`, which writes a combining mark as
    // `\u{301}`, an escape YAML refuses: a folder named "Café" in decomposed form (as macOS
    // file APIs give it) made adding the box fail every time ("unknown escape").
    let dir = web_lab();
    for name in ["Cafe\u{301}", "quote\"and\\backslash", "tab\there"] {
        let folder = Path::new("/Users/someone").join(name);
        add(&dir, &test_scripts(), Kind::Container { image: "kalilinux/kali-rolling" }, Cpu::Lab, None, Some(&shared(&folder))).unwrap();
        let spec = lab::load(&dir).unwrap();
        assert_eq!(spec.machines[NAME].host_folders[0].from, folder.display().to_string(), "{name:?}");
        remove(&dir).unwrap();
    }
}

#[test]
fn a_test_runs_bare_box_skips_kalis_toolset_and_nothing_else() {
    // The integration runner's attack-box-vm scenarios: the box without the toolset install that
    // took them past their caps; its account, desktop and Kali archive set up as ever.
    for desktop in [false, true] {
        let (full, bare) = (setup_script_with(&test_scripts(), desktop, true, false), setup_script_with(&test_scripts(), desktop, false, false));
        let scripts = test_scripts();
        assert!(full.contains(&scripts.toolset) && !bare.contains("kali-linux-headless"), "the toolset's install alone goes");
        let marker = "# Kali's toolset left out: a test run's bare box (CYBERCTF_TEST_BOX_SKIP_TOOLS).\n";
        assert_eq!(bare, full.replace(&scripts.toolset, marker), "everything else as it was");
        assert!(bare.contains(&player_account(&scripts, false)) && bare.contains(&scripts.kali));
    }
    // Only `1` turns it off, and only in a test build (`toolset_wanted` is `true` in the app's).
    assert!(!toolset_wanted_given(Some("1")));
    for other in [None, Some("0"), Some(""), Some("yes")] {
        assert!(toolset_wanted_given(other), "{other:?}");
    }
}

#[test]
fn a_vm_box_says_its_shared_folder_is_squashed_where_it_is_and_a_container_never() {
    let account = |squashed| format!("account={PLAYER} shared={}{} password_hash=", crate::shared_folder::MOUNT_POINT, squashed);
    let dir = web_lab();
    for squashed in [false, true] {
        let folder = Shared { folder: Path::new("/home/someone/CyberCTF/shared").to_path_buf(), squashed };
        add(&dir, &test_scripts(), Kind::Vm { desktop: false }, Cpu::Lab, None, Some(&folder)).unwrap();
        let script = std::fs::read_to_string(dir.join(SETUP_FILE)).unwrap();
        assert!(script.contains(&account(if squashed { " shared_squashed=1" } else { "" })), "{squashed}: {script}");
        assert!(crate::platform::sh_parses(&script), "not valid sh: {script}");
        remove(&dir).unwrap();
    }
    let folder = Shared { folder: Path::new("/home/someone/CyberCTF/shared").to_path_buf(), squashed: false };
    add(&dir, &test_scripts(), Kind::Container { image: "kalilinux/kali-rolling" }, Cpu::Lab, None, Some(&folder)).unwrap();
    let command = layer_machine(&dir)["command"].as_str().unwrap().to_owned();
    assert!(command.contains(&account("")) && !command.contains(" shared_squashed=1"), "Docker binds it as it is: {command}");
    remove(&dir).unwrap();
}
