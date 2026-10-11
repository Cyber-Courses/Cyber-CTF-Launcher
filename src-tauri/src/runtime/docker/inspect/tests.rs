use super::compose::parse_ps;
use super::{
    HostProbe, declared_services, down_serving, first_published_url, is_datastore, is_running, parse_inspect, parse_networks, short_network, status_from_host,
    tcp_ports,
};
use super::{build_status, ipv4_subnets, listed_names, networks_of_others};

#[test]
fn a_labs_own_networks_are_told_by_their_exact_project() {
    // Regression: a lab's own networks were picked by name prefix, so lab `web` also took
    // lab `web-2`'s networks for its own, and a new instance could land on their subnets.
    let listed = "bridge\t\ncyberctf-web_lab\tcyberctf-web\ncyberctf-web-2_lab\tcyberctf-web-2\nhost\t\n\n";
    assert_eq!(networks_of_others(listed, "cyberctf-web"), ["bridge", "cyberctf-web-2_lab", "host"]);
    assert_eq!(networks_of_others(listed, "cyberctf-web-2"), ["bridge", "cyberctf-web_lab", "host"]);
    assert_eq!(networks_of_others("plain\n", "cyberctf-web"), ["plain"]);
}

#[test]
fn network_listings_skip_blanks_and_ipv6() {
    assert_eq!(listed_names(" bridge \n\ncyberctf-x_lab\n"), ["bridge", "cyberctf-x_lab"]);
    assert_eq!(ipv4_subnets("172.17.0.0/16 fd00::/64\n10.30.0.0/24 \n"), ["172.17.0.0/16", "10.30.0.0/24"]);
}

#[test]
fn a_serving_container_that_died_still_shows_on_the_diagram() {
    let entries = parse_ps(r#"[{"Service":"web","State":"exited","Name":"p-web-1"},{"Service":"db","State":"running","Name":"p-db-1","Health":"unhealthy"}]"#);
    let status = build_status(entries, Default::default(), Vec::new(), &["web".into()]);
    assert!(status.running);
    let states: Vec<(&str, &str)> = status.machines.iter().map(|m| (m.name.as_str(), m.state.as_str())).collect();
    assert_eq!(states, [("db", "unhealthy"), ("web", "exited")]);
    assert_eq!(status.provider.as_deref(), Some("docker"));
}

#[test]
fn a_remote_lab_host_reports_its_containers_and_the_attack_box() {
    // Captured from a Supplier portal API lab on a Proxmox server.
    let probe = HostProbe::parse(include_str!("../testdata/remote_probe.txt")).unwrap();
    let (status, attacker) = status_from_host("invoice-portal-api-1", &probe);
    assert!(status.running);
    let names: Vec<(&str, &str)> = status.machines.iter().map(|m| (m.name.as_str(), m.ip.as_str())).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&("web", "10.21.0.31")) && names.contains(&("database", "10.21.0.32")), "{names:?}");
    assert_eq!(status.networks.iter().map(|n| (n.name.as_str(), n.subnet.as_str())).collect::<Vec<_>>(), vec![("lab", "10.21.0.0/24")]);
    assert_eq!(attacker, Some(("10.21.0.2".to_string(), "lab".to_string())));
    assert_eq!(status.url, None, "a remote lab isn't on 127.0.0.1");
}

// A compose `ps --all` line, as Docker Compose prints it.
const INIT_EXITED: &str = r#"[
    {"Service":"web","State":"running"},
    {"Service":"database","State":"running"},
    {"Service":"database-init-1","State":"exited"}]"#;

#[test]
fn a_lab_is_running_when_its_one_shot_init_has_exited() {
    // Regression: after a start the evidence init exits 0; the lab must still read as running
    // (web + database up), not flip back to a "Start lab" page.
    let entries = parse_ps(INIT_EXITED);
    assert!(is_running(&entries));
    // The exited init publishes nothing, so it is never counted as a down serving container.
    assert_eq!(down_serving(&entries, &["web".into(), "database".into()]), vec![]);
}

#[test]
fn a_lab_with_only_a_finished_init_is_not_running() {
    let entries = parse_ps(r#"[{"Service":"database-init-1","State":"exited"}]"#);
    assert!(!is_running(&entries));
}

#[test]
fn a_serving_container_that_died_is_reported_down_while_others_run() {
    // web died but the database is up: running (something is up) yet degraded, so web is listed.
    let entries = parse_ps(
        r#"[
        {"Service":"web","State":"exited"},
        {"Service":"database","State":"running"},
        {"Service":"database-init-1","State":"exited"}]"#,
    );
    assert!(is_running(&entries));
    assert_eq!(down_serving(&entries, &["web".into(), "database".into()]), vec![("web".to_string(), "exited".to_string())]);
}

#[test]
fn datastores_are_recognised_by_service_or_image() {
    let ps = |svc: &str, image: &str| parse_ps(&format!(r#"[{{"Service":"{svc}","Image":"{image}","State":"running"}}]"#)).pop().unwrap();
    // By well-known image substrings.
    assert!(is_datastore(&ps("x", "mysql:8.0")));
    assert!(is_datastore(&ps("cache", "redis:7")));
    assert!(is_datastore(&ps("store", "mongo:6")));
    assert!(is_datastore(&ps("search", "docker.elastic.co/elasticsearch:8")));
    // By the conventional service names.
    assert!(is_datastore(&ps("db", "custom-image")));
    assert!(is_datastore(&ps("database", "custom-image")));
    // A plain web app is not a datastore.
    assert!(!is_datastore(&ps("web", "invoice_web")));
    assert!(!is_datastore(&ps("app", "nginx:1.27")));
}

#[test]
fn open_url_skips_a_lone_datastore_even_on_a_low_port() {
    // Only a database is up, publishing the low port 22-like case: Open must stay None.
    let out = r#"[{"Service":"db","Image":"postgres:16","State":"running","Publishers":[{"PublishedPort":22,"Protocol":"tcp"}]}]"#;
    assert_eq!(first_published_url(&parse_ps(out)), None);
}

#[test]
fn open_url_ignores_udp_and_unpublished_ports() {
    let out = r#"[{"Service":"web","Image":"app","State":"running","Publishers":[
        {"PublishedPort":53,"Protocol":"udp"},
        {"PublishedPort":0,"Protocol":"tcp"},
        {"PublishedPort":8080,"Protocol":"tcp"}]}]"#;
    // The udp port and the unpublished one are skipped; the tcp web port wins.
    assert_eq!(first_published_url(&parse_ps(out)).as_deref(), Some("http://127.0.0.1:8080"));
}

#[test]
fn short_network_strips_only_this_labs_project_prefix() {
    assert_eq!(short_network("sqli", "cyberctf-sqli_dmz"), "dmz");
    // A different lab's prefix (or none) is left untouched.
    assert_eq!(short_network("sqli", "cyberctf-other_dmz"), "cyberctf-other_dmz");
    assert_eq!(short_network("sqli", "bridge"), "bridge");
}

#[test]
fn declared_services_maps_isoloom_http_to_web_and_keeps_tcp_name() {
    let labels = [
        ("isoloom.service.portal".to_string(), "http:80,443".to_string()),
        ("isoloom.service.shell".to_string(), "tcp:22".to_string()),
        ("com.docker.compose.service".to_string(), "app".to_string()),
    ]
    .into();
    let s = declared_services(&labels);
    let got: Vec<(&str, &str, Vec<u16>)> = s.iter().map(|s| (s.name.as_str(), s.kind.as_str(), s.ports.clone())).collect();
    // isoloom http -> "web"; isoloom tcp -> the service's own name; sorted by name.
    assert_eq!(got, vec![("portal", "web", vec![80, 443]), ("shell", "shell", vec![22])]);
}

#[test]
fn declared_services_uses_the_declared_kind_for_cyberctf_labels() {
    let labels = [("cyberctf.service.cache".to_string(), "redis:6379".to_string())].into();
    let s = declared_services(&labels);
    assert_eq!((s[0].name.as_str(), s[0].kind.as_str(), s[0].ports.clone()), ("cache", "redis", vec![6379]));
}

#[test]
fn declared_services_skips_malformed_ports_and_zero() {
    let labels = [("isoloom.service.db".to_string(), "database:abc,0,5432".to_string())].into();
    let s = declared_services(&labels);
    // Only the valid, non-zero port survives; bad ones are dropped, never guessed.
    assert_eq!(s[0].ports, vec![5432]);
}

#[test]
fn networks_fall_back_to_the_ipv6_subnet_when_no_ipv4() {
    // A network reported with only an IPv6 subnet keeps it rather than showing nothing.
    let out = "cyberctf-x_v6only\tfd00::/64 \ttrue\n";
    let n = parse_networks(&super::compose::project("x"), out);
    assert_eq!(n[0].name, "v6only");
    assert_eq!(n[0].subnet, "fd00::/64");
    assert!(n[0].internal);
}

#[test]
fn tcp_ports_keeps_distinct_pairs_and_drops_udp() {
    let out = r#"[{"Service":"x","State":"running","Publishers":[
        {"TargetPort":80,"PublishedPort":8080,"Protocol":"tcp"},
        {"TargetPort":80,"PublishedPort":8080,"Protocol":"tcp"},
        {"TargetPort":443,"PublishedPort":8443,"Protocol":"tcp"},
        {"TargetPort":53,"PublishedPort":5353,"Protocol":"udp"},
        {"TargetPort":0,"PublishedPort":0,"Protocol":"tcp"}]}]"#;
    let ports = tcp_ports(&parse_ps(out)[0].publishers);
    // The duplicate tcp pair is listed once, udp is dropped, and the all-zero entry is dropped.
    assert_eq!(ports.iter().map(|p| (p.published, p.target)).collect::<Vec<_>>(), vec![(8080, 80), (8443, 443)]);
}

#[test]
fn finds_the_first_published_tcp_port() {
    let out = r#"[
      {"Service":"db","State":"running","Publishers":[{"PublishedPort":0,"Protocol":"tcp"}]},
      {"Service":"web","State":"running","Publishers":[{"PublishedPort":8080,"Protocol":"tcp"},{"PublishedPort":9090,"Protocol":"tcp"}]}
    ]"#;
    assert_eq!(first_published_url(&parse_ps(out)).as_deref(), Some("http://127.0.0.1:8080"));
    let none = r#"[{"Service":"web","State":"running","Publishers":[]}]"#;
    assert_eq!(first_published_url(&parse_ps(none)), None);
}

#[test]
fn open_url_prefers_web_and_skips_datastores() {
    // Web port is numerically higher than the DB's: still pick the web service.
    let out = r#"[
      {"Service":"database","Image":"mysql:8.0","State":"running","Publishers":[{"PublishedPort":3207,"Protocol":"tcp"}]},
      {"Service":"web","Image":"invoice_web","State":"running","Publishers":[{"PublishedPort":3206,"Protocol":"tcp"}]}
    ]"#;
    assert_eq!(first_published_url(&parse_ps(out)).as_deref(), Some("http://127.0.0.1:3206"));
    // Web has exited; only the database is up. Don't point Open at the DB.
    let db_only = r#"[{"Service":"database","Image":"mysql:8.0","State":"running","Publishers":[{"PublishedPort":3207,"Protocol":"tcp"}]},
      {"Service":"web","State":"exited","Publishers":[]}]"#;
    assert_eq!(first_published_url(&parse_ps(db_only)), None);
}

#[test]
fn reads_every_interface_with_the_lab_network_name() {
    let out = "/cyberctf-sqli-web-1\tcyberctf-sqli_dmz=172.21.0.3 cyberctf-sqli_internal=172.22.0.2 \n/cyberctf-sqli-db-1\tcyberctf-sqli_internal=172.22.0.3 \n/x\tcyberctf-sqli_dmz= \n";
    let m: std::collections::HashMap<_, _> = parse_inspect(&super::compose::project("sqli"), out).into_iter().map(|(k, v)| (k, v.interfaces)).collect();
    let web: Vec<(&str, &str)> = m["cyberctf-sqli-web-1"].iter().map(|i| (i.network.as_str(), i.ip.as_str())).collect();
    assert_eq!(web, vec![("dmz", "172.21.0.3"), ("internal", "172.22.0.2")]);
    assert_eq!(m["cyberctf-sqli-db-1"][0].network, "internal");
    assert!(m["x"].is_empty(), "a network with no address yet is skipped");
}

#[test]
fn reads_networks_preferring_ipv4_subnets() {
    let out = "cyberctf-sqli_internal\tfd00::/64 172.22.0.0/16 \ttrue\ncyberctf-sqli_default\t172.21.0.0/16 \tfalse\n";
    let n = parse_networks(&super::compose::project("sqli"), out);
    assert_eq!(n.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), vec!["default", "internal"]);
    assert_eq!(n[1].subnet, "172.22.0.0/16");
    assert!(n[1].internal && !n[0].internal);
}

#[test]
fn lists_a_port_published_on_ipv4_and_ipv6_once() {
    let out = r#"[{"Service":"db","State":"running","Publishers":[
      {"URL":"0.0.0.0","TargetPort":3207,"PublishedPort":3207,"Protocol":"tcp"},
      {"URL":"::","TargetPort":3207,"PublishedPort":3207,"Protocol":"tcp"},
      {"URL":"","TargetPort":33060,"PublishedPort":0,"Protocol":"tcp"}]}]"#;
    let ports = tcp_ports(&parse_ps(out)[0].publishers);
    assert_eq!(ports.iter().map(|p| (p.published, p.target)).collect::<Vec<_>>(), vec![(3207, 3207), (0, 33060)]);
}

#[test]
fn reads_the_services_a_lab_declares_and_nothing_else() {
    let out = "/cyberctf-x-app-1\tcyberctf-x_default=172.20.0.2 \t{\"cyberctf.service.web\":\"web:80,443\",\"cyberctf.service.ssh\":\"ssh:22\",\"cyberctf.service.jobs\":\"worker\",\"com.docker.compose.service\":\"app\"}\n";
    let m = parse_inspect(&super::compose::project("x"), out);
    let s = &m["cyberctf-x-app-1"].services;
    let got: Vec<(&str, &str, Vec<u16>)> = s.iter().map(|s| (s.name.as_str(), s.kind.as_str(), s.ports.clone())).collect();
    assert_eq!(got, vec![("jobs", "worker", vec![]), ("ssh", "ssh", vec![22]), ("web", "web", vec![80, 443])]);
    assert_eq!(m["cyberctf-x-app-1"].interfaces[0].ip, "172.20.0.2");
    // No labels, null labels: no services (never inferred).
    assert!(parse_inspect(&super::compose::project("x"), "/a\t\tnull\n")["a"].services.is_empty());
    assert!(declared_services(&[("cyberctf.service.db".to_string(), "database:abc".to_string())].into()).first().unwrap().ports.is_empty());
}
