//! Property tests for what the launcher reads from Vagrant: `status --machine-readable`, the
//! generated Vagrantfile and the streamed `up` output.

use proptest::prelude::*;

use super::*;
use crate::proptest_support::config;

/// A Vagrant machine name (no comma: machine-readable output is comma-separated).
fn machine() -> impl Strategy<Value = String> {
    prop_oneof![Just(CONTROLLER.to_string()), "[a-z][a-z0-9_-]{0,12}"]
}

/// A state label as Vagrant prints it, for any provider (`running`, `poweroff`, `not_created`...).
fn state() -> impl Strategy<Value = String> {
    prop_oneof![Just("running".to_string()), Just("poweroff".to_string()), Just("not_created".to_string()), "[a-z_ ,]{1,16}"]
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn line_parsers_never_panic(text in "(?s).{0,400}") {
        let _ = parse_status(&text);
        let _ = vagrant_topology(&text, "lab");
        let _ = vm_names_from(&text);
        for line in text.lines() {
            let _ = ansible_verdict(line);
            let _ = is_stale_state_error(line);
        }
    }

    #[test]
    fn machine_readable_status_reads_back_every_state(
        machines in proptest::collection::vec((machine(), state(), "[0-9]{10}"), 0..6),
        noise in proptest::collection::vec("[0-9]{10},[a-z]{0,6},(metadata|provider-name|ui),[^\n]{0,20}", 0..4),
    ) {
        let mut lines: Vec<String> = machines.iter().map(|(name, state, ts)| format!("{ts},{name},state,{state}")).collect();
        lines.extend(noise);
        let parsed = parse_status(&lines.join("\n"));
        prop_assert_eq!(parsed.len(), machines.len());
        for (m, (name, state, _)) in parsed.iter().zip(&machines) {
            prop_assert_eq!(&m.name, name);
            // A state label is kept whole, commas and all.
            prop_assert_eq!(&m.state, state);
            prop_assert_eq!(m.infra, name == CONTROLLER);
        }
    }

    #[test]
    fn ansible_verdicts_are_short_and_only_for_fatal_lines(prefix in "[a-z0-9]{0,8}", body in "\\PC{0,500}") {
        let line = if prefix.is_empty() { format!("fatal: [{body}") } else { format!("{prefix}: fatal: [{body}") };
        let v = ansible_verdict(&line).expect("a fatal line has a verdict");
        prop_assert!(v.starts_with("fatal: ["));
        prop_assert!(v.len() <= 300, "{} bytes", v.len());
        // Anything else is not a verdict.
        prop_assert_eq!(ansible_verdict(&format!("ok: [{body}")), None);
    }

    #[test]
    fn cidr_masks_any_address(ip: [u8; 4], bits in 0u32..=32) {
        let mask = std::net::Ipv4Addr::from(u32::MAX.checked_shl(32 - bits).unwrap_or(0));
        let got = cidr(&std::net::Ipv4Addr::from(ip).to_string(), &mask.to_string()).unwrap();
        let (net, len) = got.split_once('/').unwrap();
        prop_assert_eq!(len.parse::<u32>().unwrap(), bits);
        let net: std::net::Ipv4Addr = net.parse().unwrap();
        prop_assert_eq!(u32::from(net), u32::from(std::net::Ipv4Addr::from(ip)) & u32::from(mask));
    }

    #[test]
    fn vagrantfile_topology_reads_back_each_interface(
        lab in "[a-z][a-z0-9-]{0,8}",
        vms in proptest::collection::vec(("[a-z][a-z0-9-]{0,8}", proptest::collection::vec(("[a-z]{1,6}", any::<[u8; 4]>()), 1..3)), 1..4),
    ) {
        let mut text = String::from("Vagrant.configure(\"2\") do |config|\n");
        for (name, nics) in &vms {
            text.push_str(&format!("  config.vm.define \"{name}\" do |m|\n    m.vm.box = \"x\"\n"));
            for (net, ip) in nics {
                let ip = std::net::Ipv4Addr::from(*ip);
                text.push_str(&format!(
                    "    m.vm.network \"private_network\", ip: \"{ip}\", netmask: \"255.255.255.0\", virtualbox__intnet: \"isoloom-{lab}-{net}\"\n"
                ));
            }
            text.push_str("  end\n");
        }
        text.push_str("end\n");
        let (ifaces, networks) = vagrant_topology(&text, &lab);
        for (name, nics) in &vms {
            // A later block for the same machine adds to it.
            let want: Vec<(String, String)> = vms
                .iter()
                .filter(|(n, _)| n == name)
                .flat_map(|(_, nics)| nics.iter().map(|(net, ip)| (net.clone(), std::net::Ipv4Addr::from(*ip).to_string())))
                .collect();
            let got: Vec<(String, String)> = ifaces[name].iter().map(|i| (i.network.clone(), i.ip.clone())).collect();
            prop_assert_eq!(got, want);
            for (net, _) in nics {
                prop_assert!(networks.iter().any(|n| &n.name == net && n.subnet.ends_with("/24")));
            }
        }
        // One segment per network name.
        let names: std::collections::HashSet<&String> = networks.iter().map(|n| &n.name).collect();
        prop_assert_eq!(names.len(), networks.len());
    }

    #[test]
    fn vm_names_reads_every_quoted_name(names in proptest::collection::vec("[^\"\n\r]{1,20}", 0..4)) {
        let text: String = names
            .iter()
            .enumerate()
            .map(|(i, n)| match i % 3 {
                0 => format!("    v.name = \"{n}\"\n"),
                1 => format!("    v.guest_name = \"{n}\"\n"),
                _ => format!("    v.vmx[\"displayName\"] = \"{n}\"\n"),
            })
            .collect();
        let mut want = names.clone();
        want.sort();
        want.dedup();
        prop_assert_eq!(vm_names_from(&text), want);
    }
}
