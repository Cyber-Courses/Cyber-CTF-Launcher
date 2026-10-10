//! Property tests for what the launcher reads from `docker compose`: `ps` in either of its JSON
//! formats and `config`, whatever Compose (or something else) prints.

use proptest::prelude::*;
use serde_json::{Value, json};

use super::*;
use crate::proptest_support::{config, json as any_json, json_ish_text};

/// One `docker compose ps` entry as Compose prints it (`v`), with the fields we expect back.
#[derive(Debug, Clone)]
struct Ps {
    v: Value,
    service: String,
    state: String,
    name: String,
    pubs: Vec<(u16, u16, String)>,
    one_off: bool,
}

fn ps_entry() -> impl Strategy<Value = Ps> {
    let publisher = (any::<u16>(), any::<u16>(), prop_oneof![Just("tcp".to_string()), Just("udp".to_string()), Just(String::new())]);
    ("\\PC{0,16}", "\\PC{0,10}", "\\PC{0,20}", proptest::collection::vec(publisher, 0..4), any::<bool>()).prop_map(|(service, state, name, pubs, one_off)| {
        let labels = if one_off { "com.docker.compose.project=x,com.docker.compose.oneoff=True" } else { "com.docker.compose.project=x" };
        let v = json!({
            "Service": service, "State": state, "Name": name, "Labels": labels, "Health": "",
            "Publishers": pubs.iter().map(|(p, t, proto)| json!({"PublishedPort": p, "TargetPort": t, "Protocol": proto, "URL": "0.0.0.0"})).collect::<Vec<_>>(),
            // Fields we don't read are ignored.
            "ExitCode": 0, "Command": "\"sleep\"",
        });
        Ps { v, service, state, name, pubs, one_off }
    })
}

/// Compose's `config --format json` ports, in the shapes it prints (numbers or strings).
fn port() -> impl Strategy<Value = Value> {
    let published = prop_oneof![
        Just(None),
        Just(Some(json!(""))),
        Just(Some(json!("0"))),
        any::<u16>().prop_map(|p| Some(json!(p.to_string()))),
        any::<u16>().prop_map(|p| Some(json!(p))),
    ];
    let host_ip = prop_oneof![Just(None), Just(Some("127.0.0.1")), Just(Some("0.0.0.0")), Just(Some("::")), Just(Some("::1")), Just(Some(""))];
    let protocol = prop_oneof![Just(None), Just(Some("tcp".to_string())), Just(Some("udp".to_string())), "\\PC{1,8}".prop_map(Some)];
    (any::<u16>(), published, host_ip, protocol).prop_map(|(target, published, host_ip, protocol)| {
        let mut p = json!({ "target": target, "mode": "ingress" });
        if let Some(v) = published {
            p["published"] = v;
        }
        if let Some(ip) = host_ip {
            p["host_ip"] = json!(ip);
        }
        if let Some(proto) = protocol {
            p["protocol"] = json!(proto);
        }
        p
    })
}

fn compose_config() -> impl Strategy<Value = Value> {
    proptest::collection::btree_map("\\PC{1,12}", proptest::collection::vec(port(), 0..4), 0..4).prop_map(|services| {
        let services: serde_json::Map<String, Value> =
            services.into_iter().map(|(name, ports)| (name, if ports.is_empty() { json!({ "image": "x" }) } else { json!({ "ports": ports }) })).collect();
        json!({ "name": "cyberctf-lab", "services": services })
    })
}

fn is_ephemeral(p: &Value) -> bool {
    match &p["published"] {
        Value::Null => true,
        Value::String(s) => s.is_empty() || s == "0",
        Value::Number(n) => n.as_u64() == Some(0),
        _ => false,
    }
}

/// The override's YAML, with `!override` tags taken off.
fn untag(v: serde_yaml_ng::Value) -> serde_yaml_ng::Value {
    match v {
        serde_yaml_ng::Value::Tagged(t) => untag(t.value),
        serde_yaml_ng::Value::Mapping(m) => serde_yaml_ng::Value::Mapping(m.into_iter().map(|(k, v)| (k, untag(v))).collect()),
        other => other,
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn parsers_never_panic_on_any_output(out in json_ish_text()) {
        let _ = parse_ps(&out);
        let _ = host_ports_from_config(&out);
        let _ = published_from_config(&out);
        let _ = serving_services_from_config(&out);
        let _ = pinned_ports(&out, |_| Some(40000));
        let _ = pinned_ports(&out, |_| None);
    }

    #[test]
    fn config_parsers_never_panic_on_any_json(v in any_json()) {
        let s = v.to_string();
        let _ = host_ports_from_config(&s);
        let _ = published_from_config(&s);
        let _ = serving_services_from_config(&s);
        let _ = pinned_ports(&s, Some);
    }

    #[test]
    fn ps_reads_the_same_entries_from_both_formats(entries in proptest::collection::vec(ps_entry(), 0..6), pad in "[ \n]{0,3}") {
        let values: Vec<Value> = entries.iter().map(|e| e.v.clone()).collect();
        let array = format!("{pad}{}{pad}", Value::Array(values.clone()));
        let ndjson = format!("{pad}{}\n", values.iter().map(Value::to_string).collect::<Vec<_>>().join("\n\n"));
        for out in [array, ndjson] {
            let parsed = parse_ps(&out);
            prop_assert_eq!(parsed.len(), entries.len());
            for (p, Ps { service, state, name, pubs, one_off, .. }) in parsed.iter().zip(&entries) {
                prop_assert_eq!(&p.service, service);
                prop_assert_eq!(&p.state, state);
                prop_assert_eq!(&p.name, name);
                prop_assert_eq!(p.one_off(), *one_off);
                let got: Vec<(u16, u16, String)> = p.publishers.iter().map(|x| (x.published_port, x.target_port, x.protocol.clone())).collect();
                prop_assert_eq!(&got, pubs);
            }
        }
    }

    #[test]
    fn a_bad_ndjson_line_costs_only_itself(entries in proptest::collection::vec(ps_entry(), 1..5), junk in "[^\\[\\s][^\n]{0,30}") {
        let mut lines: Vec<String> = entries.iter().map(|e| e.v.to_string()).collect();
        lines.insert(lines.len() / 2, junk.clone());
        let parsed = parse_ps(&lines.join("\n"));
        // The junk line is dropped (unless it happens to be an entry itself).
        let junk_is_entry = serde_json::from_str::<PsEntry>(&junk).is_ok();
        prop_assert_eq!(parsed.len(), entries.len() + usize::from(junk_is_entry));
    }

    #[test]
    fn config_ports_are_read_as_compose_prints_them(cfg in compose_config()) {
        let s = cfg.to_string();
        let services = cfg["services"].as_object().unwrap();
        let num = |v: &Value| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()));
        let mut want_host = Vec::new();
        let mut want_published = Vec::new();
        let mut want_serving = Vec::new();
        for (name, svc) in services {
            let ports = svc["ports"].as_array().cloned().unwrap_or_default();
            if !ports.is_empty() {
                want_serving.push(name.clone());
            }
            for p in &ports {
                if let Some(h) = num(&p["published"]).filter(|h| *h > 0) {
                    want_host.push(h as u16);
                    want_published.push((name.clone(), p["target"].as_u64().unwrap() as u16, h as u16));
                }
            }
        }
        prop_assert_eq!(host_ports_from_config(&s), want_host);
        prop_assert_eq!(published_from_config(&s), want_published);
        prop_assert_eq!(serving_services_from_config(&s), want_serving);
    }

    /// The pinned-ports override is valid YAML whatever the names and values, and pins exactly
    /// the ephemeral ports, keeping the rest.
    #[test]
    fn pinned_ports_writes_valid_yaml_for_any_config(cfg in compose_config(), base in 1024u16..60000) {
        let mut next = base;
        let yaml = pinned_ports(&cfg.to_string(), |_| {
            next = next.wrapping_add(1);
            Some(next)
        });
        let services = cfg["services"].as_object().unwrap();
        let with_ephemeral: Vec<&String> =
            services.iter().filter(|(_, s)| s["ports"].as_array().is_some_and(|ps| ps.iter().any(is_ephemeral))).map(|(n, _)| n).collect();
        let Some(yaml) = yaml else {
            prop_assert!(with_ephemeral.is_empty());
            return Ok(());
        };
        let doc = untag(serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&yaml).map_err(|e| TestCaseError::fail(format!("{e}\n{yaml}")))?);
        let pinned = doc["services"].as_mapping().expect("a services mapping");
        prop_assert_eq!(pinned.len(), with_ephemeral.len());
        for name in with_ephemeral {
            let want = services[name]["ports"].as_array().unwrap();
            let got = pinned.get(name.as_str()).and_then(|s| s["ports"].as_sequence()).expect("the service's ports");
            prop_assert_eq!(got.len(), want.len());
            for (g, w) in got.iter().zip(want) {
                let g = g.as_str().expect("a short-syntax port string");
                let proto = w["protocol"].as_str().unwrap_or("tcp");
                prop_assert!(g.ends_with(&format!(":{}/{proto}", w["target"])), "{} for {}", g, w);
                match w["host_ip"].as_str().filter(|ip| !ip.is_empty()) {
                    Some(ip) if ip.contains(':') => prop_assert!(g.starts_with(&format!("[{ip}]:")), "{}", g),
                    Some(ip) => prop_assert!(g.starts_with(&format!("{ip}:")), "{}", g),
                    None => prop_assert!(g.starts_with(|c: char| c.is_ascii_digit()), "{}", g),
                }
                if !is_ephemeral(w) {
                    let published = w["published"].as_str().map(str::to_string).unwrap_or_else(|| w["published"].to_string());
                    prop_assert!(g.contains(&format!("{published}:{}", w["target"])), "{}", g);
                }
            }
        }
    }
}
