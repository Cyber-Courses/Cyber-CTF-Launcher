use super::compose::parse_ps;
use super::{declared_services, first_published_url, ipv4_subnets, is_datastore, networks_of_others, open_url, parse_inspect};

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
fn network_subnets_skip_ipv6() {
    assert_eq!(ipv4_subnets("172.17.0.0/16 fd00::/64\n10.30.0.0/24 \n"), ["172.17.0.0/16", "10.30.0.0/24"]);
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
fn declared_services_maps_isoloom_http_to_web_and_keeps_tcp_name() {
    let labels = [
        ("isoloom.port.portal".to_string(), "http:80,443".to_string()),
        ("isoloom.port.shell".to_string(), "tcp:22".to_string()),
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
    let labels = [("isoloom.port.db".to_string(), "database:abc,0,5432".to_string())].into();
    let s = declared_services(&labels);
    // Only the valid, non-zero port survives; bad ones are dropped, never guessed.
    assert_eq!(s[0].ports, vec![5432]);
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
fn reads_the_services_a_lab_declares_and_nothing_else() {
    let out = "/cyberctf-x-app-1\t{\"cyberctf.service.web\":\"web:80,443\",\"cyberctf.service.ssh\":\"ssh:22\",\"cyberctf.service.jobs\":\"worker\",\"com.docker.compose.service\":\"app\"}\n";
    let m = parse_inspect(out);
    let s = &m["cyberctf-x-app-1"].services;
    let got: Vec<(&str, &str, Vec<u16>)> = s.iter().map(|s| (s.name.as_str(), s.kind.as_str(), s.ports.clone())).collect();
    assert_eq!(got, vec![("jobs", "worker", vec![]), ("ssh", "ssh", vec![22]), ("web", "web", vec![80, 443])]);
    // No labels, null labels: no services (never inferred).
    assert_eq!(parse_inspect("/a\tnull\n")["a"].services, Vec::<crate::runtime::model::Service>::new());
    assert_eq!(declared_services(&[("cyberctf.service.db".to_string(), "database:abc".to_string())].into()).first().unwrap().ports, Vec::<u16>::new());
}

#[test]
fn a_web_app_whose_name_contains_a_datastore_token_keeps_its_open_url() {
    // Regression: `is_datastore` matched substrings ("wordpress" contains "rdp"), so a WordPress lab had
    // no Open button.
    let ps = |svc: &str, image: &str| {
        parse_ps(&format!(r#"[{{"Service":"{svc}","Image":"{image}","State":"running","Publishers":[{{"PublishedPort":8080,"Protocol":"tcp"}}]}}]"#))
            .pop()
            .unwrap()
    };
    assert!(!is_datastore(&ps("wordpress", "wordpress:6.4")));
    assert!(!is_datastore(&ps("web", "giftportal/app:1")));
    assert_eq!(first_published_url(&[ps("wordpress", "wordpress:6.4")]).as_deref(), Some("http://127.0.0.1:8080"));
}

#[test]
fn web_uis_named_like_a_service_keep_their_open_url_and_databases_dont() {
    // Regression: `novnc` (a web UI) matched "vnc", `sftpgo` "ftp", `oracle-weblogic` "oracle":
    // those labs got no Open URL, or the database's.
    let ps = |svc: &str, image: &str| parse_ps(&format!(r#"[{{"Service":"{svc}","Image":"{image}","State":"running"}}]"#)).pop().unwrap();
    for (svc, image) in [
        ("viewer", "theasp/novnc:latest"),
        ("files", "drakkan/sftpgo:v2"),
        ("app", "container-registry.oracle.com/middleware/weblogic:14"),
        ("wls", "vulhub/oracle-weblogic:12"),
        // Web front ends named after the service they manage.
        ("db-admin", "adminer:4"),
        ("resolver", "technitium/dns-server"),
        ("users", "wheelybird/ldap-user-manager"),
    ] {
        assert!(!is_datastore(&ps(svc, image)), "{svc} {image}");
    }
    for (svc, image) in [
        ("oracle", "gvenzl/oracle-xe:21"),
        ("x", "container-registry.oracle.com/database/free:23"),
        ("shell", "linuxserver/openssh-server"),
        ("files", "fauria/vsftpd"),
    ] {
        assert!(is_datastore(&ps(svc, image)), "{svc} {image}");
    }
}

#[test]
fn open_goes_to_the_service_the_lab_declares_as_http() {
    // The declared web service wins over a lower port of a service whose name says nothing.
    let entries = parse_ps(
        r#"[{"Service":"portal","Name":"p-portal-1","Image":"acme/portal","State":"running","Publishers":[{"PublishedPort":8443,"TargetPort":80,"Protocol":"tcp"},{"PublishedPort":2222,"TargetPort":22,"Protocol":"tcp"}]},
            {"Service":"metrics","Name":"p-metrics-1","Image":"acme/agent","State":"running","Publishers":[{"PublishedPort":3000,"TargetPort":3000,"Protocol":"tcp"}]}]"#,
    );
    assert_eq!(first_published_url(&entries).as_deref(), Some("http://127.0.0.1:2222"), "by name alone, the lowest port");
    let declared = parse_inspect("/p-portal-1\t{\"isoloom.port.site\":\"http:80\",\"isoloom.port.ssh\":\"tcp:22\"}\n");
    assert_eq!(open_url(&entries, &declared).as_deref(), Some("http://127.0.0.1:8443"));
    // Nothing declared: the name-based pick.
    assert_eq!(open_url(&entries, &std::collections::HashMap::default()).as_deref(), Some("http://127.0.0.1:2222"));
}

#[test]
fn a_null_publishers_list_is_no_ports() {
    // Regression: an explicit null (`"Publishers":null`) dropped the NDJSON line, and failed the whole
    // array format: a running lab read as not running.
    let line = r#"{"Service":"web","State":"running","Name":"p-web-1","Publishers":null}"#;
    assert_eq!(parse_ps(line).len(), 1);
    assert_eq!(parse_ps(&format!("[{line}]")).len(), 1);
}

#[test]
fn ps_entries_with_odd_but_legal_values_are_read() {
    // Unicode in names and images, a tab inside a label value, a very long name, CRLF endings.
    let long = "n".repeat(10_000);
    let out = format!(
        "{{\"Service\":\"wéb\",\"State\":\"running\",\"Name\":\"{long}\",\"Image\":\"ghcr.io/x/y:1\",\"Labels\":\"a=b\\tc,com.docker.compose.oneoff=True\"}}\r\n{{\"Service\":\"db\",\"State\":\"running\",\"Health\":\"starting\",\"Publishers\":[]}}\r\n"
    );
    let e = parse_ps(&out);
    assert_eq!(e.len(), 2);
    assert_eq!(e[0].service, "wéb");
    assert_eq!(e[0].name.len(), 10_000);
    assert!(e[1].publishers.is_empty());
}

#[test]
fn inspect_lines_with_missing_columns_or_odd_names_are_read_or_skipped() {
    // A name with no labels column is no container to read; an unreadable labels column is no
    // services.
    let m = parse_inspect("/cyberctf-x-a-1\n\n/b\tnot json\n");
    assert_eq!(m.keys().collect::<Vec<_>>(), ["b"]);
    assert_eq!(m["b"].services, [] as [crate::runtime::Service; 0]);
    // A label value holding a tab or newline comes JSON-escaped, so the columns hold.
    let m = parse_inspect("/c\t{\"isoloom.port.app\":\"http:80\",\"note\":\"a\\tb\\nc\"}\n");
    assert_eq!(m["c"].services[0].kind, "web");
    // Windows line endings: no carriage return in the labels JSON.
    let m = parse_inspect("/e\t{\"isoloom.port.s\":\"tcp:22\"}\r\n");
    assert_eq!(m["e"].services[0].ports, [22]);
}

#[test]
fn declared_service_ports_at_the_boundaries() {
    let labels = [
        ("isoloom.port.a".to_string(), "tcp:65535,65536,-1,+22, 80 ,".to_string()),
        ("isoloom.port.".to_string(), "http:80".to_string()),
        ("isoloom.port.b".to_string(), String::new()),
        ("isoloom.port.c".to_string(), ":80".to_string()),
    ]
    .into();
    let s = declared_services(&labels);
    let got: Vec<(&str, &str, Vec<u16>)> = s.iter().map(|s| (s.name.as_str(), s.kind.as_str(), s.ports.clone())).collect();
    // 65536 and -1 are dropped; "+22" parses as 22 (a quirk of integer parsing, harmless); an
    // empty name is no service; an empty value is a service with no kind (its own name) and no
    // ports; an empty kind before the colon becomes the service's own name.
    assert_eq!(got, vec![("a", "a", vec![65535, 22, 80]), ("b", "b", vec![]), ("c", "c", vec![80])]);
}
