//! Property tests for what the launcher reads from `docker inspect` and `docker network
//! inspect`, locally or from a lab host over SSH (`HostProbe`).

use proptest::prelude::*;

use super::*;
use crate::proptest_support::{config, json_ish_text};

const PROJECT: &str = "cyberctf-lab1";

/// A Docker network name (Docker allows `[a-zA-Z0-9][a-zA-Z0-9_.-]*`), sometimes the lab's own.
fn net_name() -> impl Strategy<Value = (String, String)> {
    ("[a-zA-Z0-9][a-zA-Z0-9_.-]{0,12}", any::<bool>()).prop_map(|(n, own)| if own { (format!("{PROJECT}_{n}"), n) } else { (n.clone(), n) })
}

fn ip() -> impl Strategy<Value = String> {
    prop_oneof![any::<[u8; 4]>().prop_map(|o| std::net::Ipv4Addr::from(o).to_string()), any::<[u16; 8]>().prop_map(|s| std::net::Ipv6Addr::from(s).to_string())]
}

/// One `docker inspect -f INSPECT` line and what it should read as.
fn inspect_line() -> impl Strategy<Value = (String, String, Vec<(String, String)>)> {
    ("[a-zA-Z0-9][a-zA-Z0-9_.-]{0,20}", proptest::collection::vec((net_name(), ip()), 0..4), proptest::collection::btree_map("\\PC{0,20}", "\\PC{0,20}", 0..4))
        .prop_map(|(name, nets, labels)| {
            let ifaces: String = nets.iter().map(|((full, _), ip)| format!("{full}={ip} ")).collect();
            let line = format!("/{name}\t{ifaces}\t{}", serde_json::to_string(&labels).unwrap());
            let want = nets.into_iter().map(|((_, short), ip)| (short, ip)).collect();
            (line, name, want)
        })
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn parsers_never_panic_on_any_output(out in json_ish_text(), tabs in "[\t a-z=/{}\"@]{0,60}") {
        for text in [&out, &tabs] {
            let _ = parse_inspect(PROJECT, text);
            let _ = parse_networks(PROJECT, text);
            if let Some(probe) = HostProbe::parse(text) {
                let _ = status_from_host(PROJECT, &probe);
            }
        }
        // As a lab host would answer: three sections, each anything.
        let probe = HostProbe::parse(&format!("{out}@@@{tabs}@@@{out}")).unwrap();
        let (status, _) = status_from_host(PROJECT, &probe);
        prop_assert!(status.url.is_none(), "a remote lab has no local URL");
    }

    #[test]
    fn inspect_lines_read_back_each_container_and_interface(lines in proptest::collection::vec(inspect_line(), 0..5)) {
        let out: String = lines.iter().map(|(l, ..)| format!("{l}\n")).collect();
        let got = parse_inspect(PROJECT, &out);
        for (_, name, _) in &lines {
            // A later line for the same name wins, as the map keeps one entry per container.
            let last = &lines.iter().rev().find(|(_, n, _)| n == name).expect("this line").2;
            let inspected = got.get(name).expect("every container is read");
            let ifaces: Vec<(String, String)> = inspected.interfaces.iter().map(|i| (i.network.clone(), i.ip.clone())).collect();
            prop_assert_eq!(&ifaces, last);
        }
    }

    #[test]
    fn declared_services_are_named_sorted_and_have_real_ports(labels in proptest::collection::hash_map(
        prop_oneof!["isoloom\\.service\\.\\PC{0,8}", "cyberctf\\.service\\.\\PC{0,8}", "\\PC{0,12}"],
        prop_oneof!["(http|tcp|web|db)?:?[0-9, ]{0,12}", "\\PC{0,16}"],
        0..6,
    )) {
        let services = declared_services(&labels);
        prop_assert!(services.windows(2).all(|w| w[0].name <= w[1].name));
        for s in &services {
            prop_assert!(!s.name.is_empty() && s.name == s.name.trim());
            prop_assert!(s.ports.iter().all(|p| *p > 0));
        }
        let declared = labels.keys().filter(|k| {
            k.strip_prefix("isoloom.service.").or_else(|| k.strip_prefix("cyberctf.service.")).is_some_and(|n| !n.trim().is_empty())
        });
        prop_assert_eq!(services.len(), declared.count());
    }

    #[test]
    fn networks_read_back_sorted_with_an_ipv4_subnet_first(nets in proptest::collection::vec((net_name(), proptest::collection::vec(ip(), 0..3), any::<bool>()), 0..5)) {
        let out: String = nets
            .iter()
            .map(|((full, _), subnets, internal)| format!("{full}\t{}\t{internal}\n", subnets.iter().map(|s| format!("{s}/24 ")).collect::<String>()))
            .collect();
        let got = parse_networks(PROJECT, &out);
        prop_assert_eq!(got.len(), nets.len());
        prop_assert!(got.windows(2).all(|w| w[0].name <= w[1].name));
        for ((_, short), subnets, internal) in &nets {
            let n = got.iter().find(|n| &n.name == short && n.internal == *internal).expect("each network is read");
            let want = subnets.iter().find(|s| !s.contains(':')).or(subnets.first()).map(|s| format!("{s}/24")).unwrap_or_default();
            // Two networks can share a short name; compare when this one is unique.
            if nets.iter().filter(|((_, s), ..)| s == short).count() == 1 {
                prop_assert_eq!(&n.subnet, &want);
            }
        }
    }

    #[test]
    fn tcp_ports_are_unique_and_tcp_only(pubs in proptest::collection::vec((0u16..4, 0u16..4, prop_oneof![Just(""), Just("tcp"), Just("udp"), Just("sctp")]), 0..10)) {
        let publishers: Vec<Publisher> =
            pubs.iter().map(|(p, t, proto)| Publisher { published_port: *p, target_port: *t, protocol: proto.to_string() }).collect();
        let ports = tcp_ports(&publishers);
        let mut seen = std::collections::HashSet::new();
        for p in &ports {
            prop_assert!(seen.insert((p.published, p.target)), "listed once");
            prop_assert!(p.published > 0 || p.target > 0);
            prop_assert!(pubs.iter().any(|(pp, tt, proto)| (*pp, *tt) == (p.published, p.target) && (proto.is_empty() || *proto == "tcp")));
        }
    }

    #[test]
    fn short_network_strips_only_this_labs_prefix(id in "[a-z0-9-]{1,12}", name in "[a-zA-Z0-9_.-]{0,16}") {
        let project = compose::project(&id);
        prop_assert_eq!(short_network(&id, &format!("{project}_{name}")), name.clone());
        if !name.starts_with(&format!("{project}_")) {
            prop_assert_eq!(short_network(&id, &name), name);
        }
    }
}
