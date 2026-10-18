use super::{folder_candidates, parse_status, vm_names_from};

#[test]
fn parses_machine_readable_status() {
    let out = "1700000000,pfsense-1,metadata,provider,virtualbox\n\
               1700000000,pfsense-1,state,running\n\
               1700000000,debian-1,state,poweroff\n\
               1700000000,,ui,info,Current machine states:\n";
    let machines = parse_status(out);
    assert_eq!(machines.len(), 2);
    assert_eq!(machines[0].name, "pfsense-1");
    assert_eq!(machines[1].state, "poweroff");
}

#[test]
fn the_controller_is_infra_and_does_not_decide_whether_the_lab_runs() {
    let out = "1,dc01,state,running\n1,ws01,state,running\n1,isoloom-controller,state,poweroff\n";
    let machines = super::parse_status(out);
    assert!(machines.iter().find(|m| m.name == "isoloom-controller").unwrap().infra);
    assert!(!machines.iter().find(|m| m.name == "dc01").unwrap().infra);
    // Every target is up while the (powered-off) controller is ignored.
    let targets = machines.iter().filter(|m| !m.infra);
    assert!(targets.clone().count() > 0 && targets.clone().all(|m| m.state == "running"));
}

#[test]
fn vagrant_topology_reads_each_machines_lab_interface_and_the_subnet() {
    // The shape isoloom writes: a define block, then its private_network line.
    let vf = r#"
  config.vm.define "dc01" do |m|
m.vm.network "private_network", ip: "192.168.56.30", netmask: "255.255.255.0", virtualbox__intnet: "isoloom-minilab-lab", libvirt__network_name: "isoloom-minilab-lab", libvirt__dhcp_enabled: false
  end
  config.vm.define "isoloom-controller" do |m|
m.vm.network "private_network", ip: "192.168.56.253", netmask: "255.255.255.0", virtualbox__intnet: "isoloom-minilab-lab"
  end
"#;
    let (ifaces, nets) = super::vagrant_topology(vf, "minilab");
    assert_eq!(ifaces["dc01"].len(), 1);
    assert_eq!(ifaces["dc01"][0].network, "lab");
    assert_eq!(ifaces["dc01"][0].ip, "192.168.56.30");
    assert_eq!(ifaces["isoloom-controller"][0].ip, "192.168.56.253");
    // One segment, named by the lab's own network name (the isoloom-<lab>- prefix stripped),
    // with its subnet computed from the address and mask.
    assert_eq!(nets.len(), 1);
    assert_eq!(nets[0].name, "lab");
    assert_eq!(nets[0].subnet, "192.168.56.0/24");
}

#[test]
fn vagrant_topology_keeps_a_foreign_intnet_name_whole() {
    // An internal network not scoped to this lab is shown by its full name, never mangled.
    let vf = "config.vm.define \"a\" do |m|\n  m.vm.network \"private_network\", ip: \"10.0.0.5\", virtualbox__intnet: \"other-net\"\nend\n";
    let (ifaces, nets) = super::vagrant_topology(vf, "minilab");
    assert_eq!(ifaces["a"][0].network, "other-net");
    assert_eq!(nets[0].name, "other-net");
    assert_eq!(nets[0].subnet, "10.0.0.0/24"); // default mask when none is written
}

#[test]
fn cidr_masks_the_address() {
    assert_eq!(super::cidr("192.168.56.30", "255.255.255.0").as_deref(), Some("192.168.56.0/24"));
    assert_eq!(super::cidr("10.20.0.31", "255.255.0.0").as_deref(), Some("10.20.0.0/16"));
    assert_eq!(super::cidr("bad", "255.255.255.0"), None);
}

#[test]
fn vm_names_reads_the_value_for_every_provider_block() {
    // VirtualBox/Parallels/UTM use `v.name`, ESXi uses `v.guest_name`, VMware uses the quoted
    // key `v.vmx["displayName"]` — the parser must take the VALUE after `=`, not the key.
    let vf = r#"
  config.vm.define "dc01" do |m|
m.vm.provider "virtualbox" do |v|
  v.name = "minilab · dc01"
end
m.vm.provider "vmware_desktop" do |v|
  v.vmx["displayName"] = "minilab · dc01"
end
m.vm.provider "vmware_esxi" do |v|
  v.guest_name = "minilab-dc01"
end
  end
"#;
    let names = vm_names_from(vf);
    assert!(names.contains(&"minilab · dc01".to_string()), "got {names:?}");
    assert!(names.contains(&"minilab-dc01".to_string()), "got {names:?}");
    // The literal key must never be captured as a name.
    assert!(!names.iter().any(|n| n == "displayName"), "captured the key: {names:?}");
}

#[test]
fn folder_candidates_cover_virtualbox_sanitization_exactly() {
    // VirtualBox turns the middot into a hyphen for the on-disk folder; both are exact.
    let c = folder_candidates("minilab · dc01");
    assert!(c.contains(&"minilab · dc01".to_string()));
    assert!(c.contains(&"minilab - dc01".to_string()));
    // It must NOT produce a collapsed form that could match an unrelated "minilab-dc01".
    assert!(!c.iter().any(|f| f == "minilab-dc01"));
}

#[test]
fn keeps_every_state_label_verbatim() {
    // Vagrant reports many lifecycle states; each is surfaced as-is for the UI.
    let out = "ts,a,state,saved\n\
               ts,b,state,aborted\n\
               ts,c,state,not_created\n\
               ts,d,state,running\n";
    let m = parse_status(out);
    let states: Vec<(&str, &str)> = m.iter().map(|m| (m.name.as_str(), m.state.as_str())).collect();
    assert_eq!(states, vec![("a", "saved"), ("b", "aborted"), ("c", "not_created"), ("d", "running")]);
}

#[test]
fn running_requires_every_machine_up() {
    // The `status()` running rule: all machines must be "running".
    let up = parse_status("ts,a,state,running\nts,b,state,running\n");
    assert!(!up.is_empty() && up.iter().all(|m| m.state == "running"));
    let mixed = parse_status("ts,a,state,running\nts,b,state,poweroff\n");
    assert!(!mixed.iter().all(|m| m.state == "running"));
}

#[test]
fn ignores_non_state_lines_and_empty_or_malformed_output() {
    // Only `state` rows with a non-empty target become machines.
    let out = "ts,,ui,info,Current machine states:\n\
               ts,box,metadata,provider,virtualbox\n\
               ts,,state,running\n\
               short,line\n";
    assert!(parse_status(out).is_empty());
    assert!(parse_status("").is_empty());
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cyberctf-vm-{tag}-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn reads_virtualboxs_machine_folder() {
    let out = "API version: 7_0\nDefault machine folder:          /Users/a/VirtualBox VMs\nVRDE auth library: x\n";
    assert_eq!(super::parse_machine_folder(out), Some(std::path::PathBuf::from("/Users/a/VirtualBox VMs")));
    assert_eq!(super::parse_machine_folder("Default machine folder:   \n"), None);
    assert_eq!(super::parse_machine_folder(""), None);
}

#[test]
fn virtualboxs_machine_folder_falls_back_to_the_home_folder_given() {
    // Regression (#238): the fallback read `HOME`, usually unset on Windows, so leftover VM
    // folders were never found there. The caller now passes the platform's home folder.
    let home = std::path::PathBuf::from(r"C:\Users\ada");
    assert_eq!(super::machine_folder(None, Some(home.clone())), Some(home.join("VirtualBox VMs")));
    assert_eq!(super::machine_folder(Some("no such line"), Some(home.clone())), Some(home.join("VirtualBox VMs")));
    let listed = "Default machine folder:  D:\\VMs\n";
    assert_eq!(super::machine_folder(Some(listed), Some(home)), Some(std::path::PathBuf::from(r"D:\VMs")));
    assert_eq!(super::machine_folder(None, None), None);
}

#[test]
fn lists_virtualbox_vms_by_name_and_uuid() {
    let out = "\"minilab · dc01\" {1111-aaaa}\n\"other\" {2222-bbbb}\ngarbage\n} backwards {\n";
    assert_eq!(super::parse_vbox_vms(out), vec![("minilab · dc01".to_string(), "1111-aaaa".to_string()), ("other".to_string(), "2222-bbbb".to_string())]);
}

#[test]
fn only_our_parallels_vms_are_picked() {
    let wanted: std::collections::HashSet<String> = ["minilab · dc01".to_string()].into();
    let out = "minilab · dc01\n  \nminilab-dc01\nsomeone else\n";
    assert_eq!(super::ours_in_prlctl(out, &wanted), ["minilab · dc01"]);
}

#[test]
fn leftover_folders_go_by_their_exact_sanitized_names() {
    let base = temp_dir("vbox-folders");
    for f in ["minilab - dc01", "minilab-dc01", "unrelated"] {
        std::fs::create_dir_all(base.join(f)).unwrap();
    }
    std::fs::write(base.join("minilab · dc01"), "a file, not a folder").unwrap();
    let wanted: std::collections::HashSet<String> = ["minilab · dc01".to_string()].into();
    let mut lines = Vec::new();
    super::remove_leftover_folders(&base, &wanted, &mut |l| lines.push(l));
    assert!(!base.join("minilab - dc01").exists());
    assert!(base.join("minilab-dc01").is_dir(), "a lossy match is never deleted");
    assert!(base.join("unrelated").is_dir());
    assert!(base.join("minilab · dc01").is_file());
    assert_eq!(lines, ["Removing a leftover VM folder: minilab - dc01"]);
    // A missing base folder is nothing to do.
    super::remove_leftover_folders(&base.join("nope"), &wanted, &mut |l| lines.push(l));
    assert_eq!(lines.len(), 1);
    std::fs::remove_dir_all(base).unwrap();
}

/// The attack VM's folder has no lab spec above it: its addresses come from its Vagrantfile.
#[test]
fn status_without_a_lab_spec_reads_the_addresses_from_the_vagrantfile() {
    let dir = temp_dir("status-vf");
    std::fs::write(
        dir.join("Vagrantfile"),
        "Vagrant.configure(\"2\") do |config|\n  config.vm.define \"attacker\" do |m|\n    m.vm.provider :virtualbox do |v|\n      v.name = \"minilab · attacker\"\n    end\n    m.vm.network \"private_network\", ip: \"192.168.56.250\", netmask: \"255.255.255.0\", virtualbox__intnet: \"isoloom-minilab-lab\"\n  end\nend\n",
    )
    .unwrap();
    let s = super::status_from("1,attacker,state,running\n", &dir);
    assert!(s.running);
    assert_eq!(s.machines[0].ip, "192.168.56.250");
    assert_eq!(s.machines[0].interfaces[0].network, "lab");
    assert_eq!(s.networks.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["lab"]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn status_without_a_lab_spec_has_machines_and_the_provider() {
    let dir = temp_dir("status");
    let out = "1,dc01,provider-name,virtualbox\n1,dc01,state,running\n1,isoloom-controller,state,poweroff\n";
    let s = super::status_from(out, &dir);
    assert!(s.running, "the targets are up, the controller doesn't count");
    assert_eq!(s.provider.as_deref(), Some("virtualbox"));
    assert_eq!(s.machines.len(), 2);
    assert!(s.networks.is_empty());
    // Only the controller: nothing of the lab is running.
    let s = super::status_from("1,isoloom-controller,state,running\n", &dir);
    assert!(!s.running);
    assert_eq!(s.provider, None);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn status_reads_the_network_diagram_from_the_labs_spec() {
    let lab = temp_dir("status-lab");
    std::fs::write(
        lab.join("isoloom.yml"),
        "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    app: { image: nginx:1.27 }\n",
    )
    .unwrap();
    let vagrant = lab.join(".isoloom").join("vagrant");
    std::fs::create_dir_all(&vagrant).unwrap();
    let s = super::status_from("1,web,state,running\n1,other,state,running\n", &vagrant);
    assert!(s.running);
    let web = s.machines.iter().find(|m| m.name == "web").unwrap();
    assert_eq!(web.ip, "10.30.0.10");
    assert_eq!(web.interfaces.len(), 1);
    assert_eq!(s.machines.iter().find(|m| m.name == "other").unwrap().ip, "");
    assert_eq!(s.networks.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["lab"]);
    std::fs::remove_dir_all(lab).unwrap();
}

#[test]
fn the_labs_topology_is_read_again_only_once_its_spec_changed() {
    // Regression (#238): every status poll parsed the spec and the Isoloom defaults again.
    let lab = temp_dir("status-cache");
    let spec = lab.join("isoloom.yml");
    std::fs::write(
        &spec,
        "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.31.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    app: { image: nginx:1.27 }\n",
    )
    .unwrap();
    let written = std::fs::metadata(&spec).unwrap().modified().unwrap();
    assert_eq!(super::status::lab_topology(&lab).unwrap().1.len(), 1);
    // Unchanged on disk (same time): not read again, so an unreadable rewrite goes unseen.
    std::fs::write(&spec, "not: [a spec").unwrap();
    std::fs::File::options().write(true).open(&spec).unwrap().set_modified(written).unwrap();
    assert!(super::status::lab_topology(&lab).is_some(), "served from the cache");
    // Changed: read again.
    std::fs::File::options().write(true).open(&spec).unwrap().set_modified(written + std::time::Duration::from_secs(5)).unwrap();
    assert!(super::status::lab_topology(&lab).is_none(), "the new, broken spec was read");
    // A spec under Isoloom's other name is watched too.
    std::fs::remove_file(&spec).unwrap();
    std::fs::write(
        lab.join("isoloom.yaml"),
        "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.31.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    app: { image: nginx:1.27 }\n",
    )
    .unwrap();
    assert!(super::status::lab_topology(&lab).is_some(), "isoloom.yaml read");
    std::fs::remove_dir_all(lab).unwrap();
}

#[test]
fn vm_names_come_from_the_vagrantfile_on_disk() {
    let dir = temp_dir("names");
    assert!(super::vm_names(&dir).is_empty());
    std::fs::write(dir.join("Vagrantfile"), "v.name = \"b\"\nv.name = \"a\"\nv.name = \"a\"\nv.name = \"\"\nv.name\n").unwrap();
    assert_eq!(super::vm_names(&dir), ["a", "b"]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn quoted_and_field_read_vagrantfile_values() {
    assert_eq!(super::quoted("x \"a\" \"b\"").as_deref(), Some("a"));
    assert_eq!(super::quoted("no quotes"), None);
    assert_eq!(super::quoted("\"open"), None);
    assert_eq!(super::field("ip: \"10.0.0.1\", netmask: \"255.0.0.0\"", "netmask:").as_deref(), Some("255.0.0.0"));
    assert_eq!(super::field("ip: \"10.0.0.1\"", "netmask:"), None);
}

#[test]
fn topology_skips_lines_outside_a_machine_or_without_an_address() {
    let vf = "m.vm.network \"private_network\", ip: \"10.0.0.9\", virtualbox__intnet: \"isoloom-l-lab\"\n\
              config.vm.define \"a\" do |m|\n\
              m.vm.network \"forwarded_port\", guest: 80\n\
              m.vm.network \"private_network\", ip: \"10.0.0.5\"\n\
              m.vm.network \"private_network\", ip: \"10.0.0.5\", netmask: \"bad\", virtualbox__intnet: \"isoloom-l-lab\"\n";
    let (ifaces, nets) = super::vagrant_topology(vf, "l");
    assert_eq!(ifaces.len(), 1);
    assert_eq!(ifaces["a"].len(), 1);
    assert_eq!(nets[0].subnet, "", "an unreadable mask leaves the subnet blank");
}

#[test]
fn machine_readable_status_with_windows_line_endings_and_blank_lines() {
    let out = "1,dc01,provider-name,hyperv\r\n\r\n1,dc01,state,running\r\n1,ws01,state,running\r\n1,isoloom-controller,state,poweroff\r\n";
    let dir = temp_dir("status-crlf");
    let s = super::status_from(out, &dir);
    assert!(s.running, "{:?}", s.machines.iter().map(|m| (m.name.clone(), m.state.clone())).collect::<Vec<_>>());
    assert_eq!(s.provider.as_deref(), Some("hyperv"), "no carriage return in the provider");
    assert_eq!(s.machines.iter().map(|m| m.state.as_str()).collect::<Vec<_>>(), ["running", "running", "poweroff"]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_state_with_vagrants_comma_escape_is_kept_whole_and_is_not_running() {
    // Vagrant escapes a comma in a field as `%!(VAGRANT_COMMA)`; the state is shown as printed.
    let m = parse_status("1,a,state,running%!(VAGRANT_COMMA) with a note\n1,b,state,running\n");
    assert_eq!(m[0].state, "running%!(VAGRANT_COMMA) with a note");
    let dir = temp_dir("status-comma");
    assert!(!super::status_from("1,a,state,running%!(VAGRANT_COMMA) with a note\n", &dir).running);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_provider_comes_from_the_first_named_one_and_a_blank_one_is_skipped() {
    let dir = temp_dir("status-provider");
    let s = super::status_from("1,dc01,provider-name,\n1,ws01,provider-name,parallels\n1,dc01,state,running\n", &dir);
    assert_eq!(s.provider.as_deref(), Some("parallels"));
    // A machine-readable line with no data column is ignored, not read as a provider.
    assert_eq!(super::status_from("1,dc01,provider-name\n1,dc01,state,running\n", &dir).provider, None);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_broken_lab_spec_falls_back_to_the_vagrantfile_for_addresses() {
    // The spec two levels up can't be read (an edit in progress, a newer format): the diagram
    // still comes from the generated Vagrantfile rather than disappearing.
    let lab = temp_dir("status-broken-spec");
    std::fs::write(lab.join("isoloom.yml"), "version: 1\nname: t\nbogus: 1\n").unwrap();
    let vagrant = lab.join(".isoloom").join("vagrant");
    std::fs::create_dir_all(&vagrant).unwrap();
    std::fs::write(
        vagrant.join("Vagrantfile"),
        "config.vm.define \"web\" do |m|\n  m.vm.provider :virtualbox do |v|\n    v.name = \"t · web\"\n  end\n  m.vm.network \"private_network\", ip: \"10.30.0.10\", netmask: \"255.255.255.0\", virtualbox__intnet: \"isoloom-t-lab\"\nend\n",
    )
    .unwrap();
    let s = super::status_from("1,web,state,running\n", &vagrant);
    assert_eq!(s.machines[0].ip, "10.30.0.10");
    assert_eq!(s.networks.iter().map(|n| (n.name.as_str(), n.subnet.as_str())).collect::<Vec<_>>(), [("lab", "10.30.0.0/24")]);
    std::fs::remove_dir_all(lab).unwrap();
}

#[test]
fn vagrantfile_values_with_windows_line_endings_and_single_quotes() {
    // Isoloom writes double quotes; a hand-edited file with single quotes yields no names (never
    // a wrong one), and CRLF endings don't leak into a name.
    assert_eq!(vm_names_from("  v.name = \"lab · a\"\r\n  v.guest_name = \"lab-b\"\r\n"), ["lab · a", "lab-b"]);
    assert!(vm_names_from("  v.name = 'lab · a'\n").is_empty());
    // A commented-out name line is not a name; a name line without quotes is skipped.
    assert!(vm_names_from("  # v.name = \"old\"\n  v.name = name_var\n").is_empty());
    // The first quoted string after `=` is the name, whatever follows on the line.
    assert_eq!(vm_names_from("v.vmx[\"displayName\"] = \"lab · c\" # was \"other\"\n"), ["lab · c"]);
}

#[test]
fn vagrant_topology_reads_crlf_files_and_ipv6_addresses_leave_the_subnet_blank() {
    let vf = "config.vm.define \"a\" do |m|\r\n  m.vm.network \"private_network\", ip: \"fd00::10\", virtualbox__intnet: \"isoloom-l-lab\"\r\n  m.vm.network \"private_network\", ip: \"10.0.0.5\", netmask: \"255.255.0.0\", virtualbox__intnet: \"isoloom-l-back\"\r\nend\r\n";
    let (ifaces, nets) = super::vagrant_topology(vf, "l");
    assert_eq!(ifaces["a"].iter().map(|i| (i.network.as_str(), i.ip.as_str())).collect::<Vec<_>>(), [("lab", "fd00::10"), ("back", "10.0.0.5")]);
    assert_eq!(nets.iter().map(|n| (n.name.as_str(), n.subnet.as_str())).collect::<Vec<_>>(), [("lab", ""), ("back", "10.0.0.0/16")]);
}

#[test]
fn virtualbox_listings_keep_braces_and_unicode_inside_a_name() {
    let out = "\"lab · {odd} name\" {1111-aaaa}\r\n\"日本語 · dc01\" {2222}\n\"<inaccessible>\" {3333}\n";
    assert_eq!(
        super::parse_vbox_vms(out),
        vec![
            ("lab · {odd} name".to_string(), "1111-aaaa".to_string()),
            ("日本語 · dc01".to_string(), "2222".to_string()),
            ("<inaccessible>".to_string(), "3333".to_string())
        ]
    );
}

#[test]
fn a_machine_is_tracked_only_by_an_id_file() {
    let dir = temp_dir("has-machines");
    let vbox = dir.join(".vagrant").join("machines").join("dc01").join("virtualbox");
    std::fs::create_dir_all(&vbox).unwrap();
    assert!(!super::has_machines(&dir), "a provider folder alone (left by a destroy, or a `vagrant status`) is no machine");
    std::fs::create_dir_all(vbox.join("id")).unwrap();
    assert!(!super::has_machines(&dir), "an `id` folder is not Vagrant's id file");
    std::fs::remove_dir(vbox.join("id")).unwrap();
    std::fs::write(vbox.join("id"), "").unwrap();
    assert!(super::has_machines(&dir), "even an empty id file is Vagrant's record");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn leftover_folder_removal_never_follows_a_symlink_out_of_the_base() {
    // A folder named exactly like one of ours but that is a symlink to somewhere else: removing
    // it must take the link, never the folder it points to.
    let base = temp_dir("vbox-symlink");
    let elsewhere = temp_dir("vbox-elsewhere");
    std::fs::write(elsewhere.join("keep.txt"), "x").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&elsewhere, base.join("minilab - dc01")).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&elsewhere, base.join("minilab - dc01")).unwrap();
    let wanted: std::collections::HashSet<String> = ["minilab · dc01".to_string()].into();
    let mut lines = Vec::new();
    super::remove_leftover_folders(&base, &wanted, &mut |l| lines.push(l));
    assert!(elsewhere.join("keep.txt").is_file(), "the link's target is untouched");
    assert!(std::fs::symlink_metadata(base.join("minilab - dc01")).is_err(), "the link itself is removed: {lines:?}");
    std::fs::remove_dir_all(base).unwrap();
    std::fs::remove_dir_all(elsewhere).unwrap();
}
