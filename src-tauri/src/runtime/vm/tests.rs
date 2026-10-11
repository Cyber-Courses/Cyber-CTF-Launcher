use super::{folder_candidates, is_stale_state_error, on_qemu, parse_status, vm_names_from};

#[test]
fn tells_a_lab_on_qemu_from_vagrant_s_machine_folders() {
    let dir = std::env::temp_dir().join(format!("cyberctf-qemu-{}", std::process::id()));
    let machines = dir.join(".vagrant").join("machines");
    std::fs::create_dir_all(machines.join("dc01").join("virtualbox")).unwrap();
    assert!(!on_qemu(&dir));
    std::fs::create_dir_all(machines.join("isoloom-controller").join("qemu")).unwrap();
    assert!(on_qemu(&dir), "the controller stays on as the attacker there");
    std::fs::remove_dir_all(&dir).ok();
    assert!(!on_qemu(&dir), "never started");
}

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
fn ansible_verdict_keeps_the_fatal_line_without_vagrants_prefix() {
    // Regression: a MINILAB deploy failed with ws01 unreachable over WinRM, and the UI only got
    // Vagrant's "The SSH command responded with a non-zero exit status". The reason is this
    // line, which Vagrant prefixes with the machine that printed it.
    let line = "    isoloom-controller: fatal: [ws01]: UNREACHABLE! => {\"changed\": false, \"msg\": \"winrm connection error: Read timed out.\", \"unreachable\": true}";
    let v = super::ansible_verdict(line).expect("a fatal line");
    assert!(v.starts_with("fatal: [ws01]: UNREACHABLE!"), "{v}");
    assert!(!v.contains("isoloom-controller: "), "{v}");
    // Ordinary output is not a verdict; a huge line is cut to stay readable.
    assert_eq!(super::ansible_verdict("    dc01: ok: [dc01]"), None);
    assert_eq!(super::ansible_verdict("==> ws01: Running provisioner: ansible (shell)..."), None);
    let long = format!("fatal: [a]: FAILED! => {}", "x".repeat(400));
    assert_eq!(super::ansible_verdict(&long).unwrap().len(), 300);
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
fn stale_state_matches_only_real_startup_errors() {
    assert!(is_stale_state_error("VBoxManage: error: Details: code VERR_ALREADY_EXISTS"));
    assert!(is_stale_state_error("Could not rename the directory"));
    assert!(is_stale_state_error("the machine is locked! This means"));
    assert!(is_stale_state_error("An active machine was found with a different provider"));
    // Benign provisioner output must NOT trigger a destroy+retry of a healthy VM.
    assert!(!is_stale_state_error("group 'docker' already exists"));
    assert!(!is_stale_state_error("user 'vagrant' already exists, skipping"));
    assert!(!is_stale_state_error("network lab already registered"));
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

const CONTROLLED: &str = "Vagrant.configure(\"2\") do |config|\n  config.vm.define \"isoloom-controller\" do |m|\n  end\nend\n";

#[test]
fn a_controller_is_read_from_its_define_line() {
    let dir = temp_dir("ctl");
    assert!(!super::has_controller(&dir), "no Vagrantfile");
    std::fs::write(dir.join("Vagrantfile"), "config.vm.define \"dc01\" do |m|\nend\n").unwrap();
    assert!(!super::has_controller(&dir));
    std::fs::write(dir.join("Vagrantfile"), CONTROLLED).unwrap();
    assert!(super::has_controller(&dir));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn no_controller_or_one_on_qemu_is_never_powered_off() {
    let dir = temp_dir("park-ctl");
    let mut lines = Vec::new();
    super::park_controller(&dir, &[], &mut |l| lines.push(l)).await;
    // On QEMU the controller is the attacker: it stays on (no Vagrant run at all).
    std::fs::write(dir.join("Vagrantfile"), CONTROLLED).unwrap();
    std::fs::create_dir_all(dir.join(".vagrant").join("machines").join(super::CONTROLLER).join("qemu")).unwrap();
    super::park_controller(&dir, &[], &mut |l| lines.push(l)).await;
    assert!(lines.is_empty(), "{lines:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn discarding_saved_state_needs_named_vms() {
    // No Vagrantfile, so no VM names: nothing is asked of VirtualBox.
    let dir = temp_dir("discard");
    let mut lines = Vec::new();
    super::discard_aborted_saved(&dir, &mut |l| lines.push(l)).await;
    assert!(lines.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_failure_leads_with_ansibles_verdict() {
    use crate::error::Error;
    let failed = || Err(Error::CommandFailed { command: "vagrant up".into(), stderr: "non-zero exit status".into() });
    match super::lead_with_verdict(failed(), Some("fatal: [ws01]: UNREACHABLE!".into())) {
        Err(Error::CommandFailed { command, stderr }) => {
            assert_eq!(command, "vagrant up");
            assert_eq!(stderr, "fatal: [ws01]: UNREACHABLE!\nnon-zero exit status");
        }
        other => panic!("{other:?}"),
    }
    // No verdict: the error as it was; success stays success; other errors are untouched.
    match super::lead_with_verdict(failed(), None) {
        Err(Error::CommandFailed { stderr, .. }) => assert_eq!(stderr, "non-zero exit status"),
        other => panic!("{other:?}"),
    }
    assert!(super::lead_with_verdict(Ok(()), Some("fatal: [x]".into())).is_ok());
    assert!(matches!(super::lead_with_verdict(Err(Error::Invalid("x".into())), Some("v".into())), Err(Error::Invalid(m)) if m == "x"));
}

#[test]
fn parking_suspends_or_halts_and_resuming_keeps_the_provider() {
    use super::super::model::Park;
    assert_eq!(super::park_verb(Park::Pause), "suspend");
    assert_eq!(super::park_verb(Park::Shutdown), "halt");
    assert_eq!(super::resume_args(None), ["up", "--no-provision"]);
    assert_eq!(super::resume_args(Some("virtualbox")), ["up", "--no-provision", "--provider", "virtualbox"]);
}

#[test]
fn ansible_verdict_cuts_long_non_ascii_lines_on_a_char_boundary() {
    let long = format!("fatal: [a]: FAILED! => {}", "é".repeat(300));
    let v = super::ansible_verdict(&long).unwrap();
    assert!(v.ends_with("..."));
    assert!(v.len() <= 300);
    // A machine prefix with a space is not Vagrant's, so not a verdict.
    assert_eq!(super::ansible_verdict("two words: fatal: [a]: FAILED!"), None);
    assert_eq!(super::ansible_verdict("fatal: [a]: FAILED!").as_deref(), Some("fatal: [a]: FAILED!"));
}

#[test]
fn another_recorded_provider_is_a_mismatch() {
    use super::super::providers::Provider;
    let dir = temp_dir("reconcile");
    assert!(!super::recorded_on_other_provider(&dir, Provider::Virtualbox), "never started");
    let machines = dir.join(".vagrant").join("machines");
    std::fs::create_dir_all(machines.join("dc01").join("virtualbox")).unwrap();
    // A stray file is not a provider folder.
    std::fs::write(machines.join("dc01").join("index_uuid"), "x").unwrap();
    assert!(!super::recorded_on_other_provider(&dir, Provider::Virtualbox));
    assert!(super::recorded_on_other_provider(&dir, Provider::VmwareEsxi));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn reconciling_on_the_same_provider_keeps_the_state() {
    use super::super::providers::Provider;
    let dir = temp_dir("reconcile-same");
    let mut lines = Vec::new();
    super::reconcile_provider(&dir, Provider::Virtualbox, &[], &mut |l| lines.push(l)).await;
    std::fs::create_dir_all(dir.join(".vagrant").join("machines").join("dc01").join("virtualbox")).unwrap();
    super::reconcile_provider(&dir, Provider::Virtualbox, &[], &mut |l| lines.push(l)).await;
    assert!(lines.is_empty());
    assert!(dir.join(".vagrant").is_dir());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn reads_virtualboxs_machine_folder() {
    let out = "API version: 7_0\nDefault machine folder:          /Users/a/VirtualBox VMs\nVRDE auth library: x\n";
    assert_eq!(super::parse_machine_folder(out), Some(std::path::PathBuf::from("/Users/a/VirtualBox VMs")));
    assert_eq!(super::parse_machine_folder("Default machine folder:   \n"), None);
    assert_eq!(super::parse_machine_folder(""), None);
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
        "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    docker: { image: nginx:1.27 }\n",
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
