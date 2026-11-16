//! Where a container lab running here is reachable, for the website to open after a launch: the
//! page of a web port the lab declares, else a port of a machine that is no datastore nor another
//! non-web service by its name.

use isothrum_runner::{MachineState, Status};

use super::described::Described;

/// Whole words of a machine's or port's name that mark it as no web UI: a datastore, or a
/// non-HTTP network service (a daemon's name with or without its `d`).
const NOT_WEB: &[&str] = &[
    "mysql",
    "mariadb",
    "postgres",
    "postgresql",
    "redis",
    "mongo",
    "mongodb",
    "memcached",
    "rabbitmq",
    "elasticsearch",
    "mssql",
    "oracledb",
    "ssh",
    "openssh",
    "ftp",
    "vsftp",
    "proftp",
    "pureftp",
    "rdp",
    "xrdp",
    "vnc",
    "smtp",
    "postfix",
    "telnet",
    "bind9",
    "dnsmasq",
    "samba",
    "openldap",
];

/// Machine names that are no web UI only as the whole name: as a word they also name the web
/// front ends of these services (`db-admin`, `dns-server`, `ldap-user-manager`).
const NOT_WEB_MACHINES: &[&str] = &["db", "database", "sftp", "dns", "smb", "ldap"];

/// Oracle's databases (`oracle-xe`), told from its web servers (`oracle-weblogic`) by the word
/// next to `oracle`.
const ORACLE_DATABASE: &[&str] = &["xe", "free", "database", "db"];

/// Whether machine `machine`'s port `port` (their names) is no web UI: a database, a cache, a
/// broker, or a non-HTTP network service (SSH, FTP, RDP…). Whole words only: substrings took web
/// apps for them (`wordpress` holds "rdp", `novnc` "vnc", `sftpgo` "ftp", `oracle-weblogic`
/// "oracle").
pub(super) fn not_web(machine: &str, port: &str) -> bool {
    let machine = machine.to_ascii_lowercase();
    let words: Vec<String> =
        format!("{machine} {}", port.to_ascii_lowercase()).split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_owned).collect();
    let has = |k: &str| words.iter().any(|w| w == k || w.strip_suffix('d') == Some(k));
    NOT_WEB.iter().any(|k| has(k))
        || NOT_WEB_MACHINES.contains(&machine.as_str())
        || has("database")
        || (has("oracle") && ORACLE_DATABASE.iter().any(|k| has(k)))
}

/// The loopback URL of the lowest host port a running machine of the lab publishes for one of the
/// lab's web ports (`http`, `https`); else for any of its TCP ports [`not_web`] doesn't rule out.
/// None until one is published.
pub(super) fn local_url(lab: &Described, status: &Status) -> Option<String> {
    let published = |web: bool| {
        lab.machines
            .iter()
            .filter_map(|d| Some((d, status.machines.iter().find(|m| m.name == d.name && matches!(m.state, MachineState::Running | MachineState::Unhealthy))?)))
            .flat_map(move |(d, m)| {
                d.ports
                    .iter()
                    .filter(move |p| p.tcp() && if web { p.web() } else { !not_web(&d.name, &p.name) })
                    .filter_map(move |p| super::view::published(m, p))
            })
            .min()
    };
    published(true).or_else(|| published(false)).map(|port| format!("http://127.0.0.1:{port}"))
}

#[cfg(test)]
mod tests {
    use super::super::described::{Declared, DeclaredPort, Described};
    use super::{local_url, not_web};

    /// A port as a spec declares it: its name, number and protocol.
    type Spec<'a> = (&'a str, u16, &'a str);
    /// A port as the runner reports it published: its number, host port and protocol.
    type Seen<'a> = (u16, u16, &'a str);

    /// A lab of `machines`, each with its ports.
    fn lab(machines: &[(&str, &[Spec<'_>])]) -> Described {
        let machines = machines
            .iter()
            .map(|(name, ports)| Declared {
                name: (*name).to_owned(),
                ports: ports.iter().map(|(n, p, proto)| DeclaredPort { name: (*n).to_owned(), port: *p, protocol: (*proto).to_owned() }).collect(),
                interfaces: Vec::new(),
            })
            .collect();
        Described { machines, ..Described::default() }
    }

    /// The runner's status of `machines`, each in its state with its published ports.
    fn status(machines: &[(&str, &str, &[Seen<'_>])]) -> isothrum_runner::Status {
        let machines: Vec<serde_json::Value> = machines
            .iter()
            .map(|(name, state, published)| {
                let published: Vec<serde_json::Value> =
                    published.iter().map(|(port, host, proto)| serde_json::json!({ "port": port, "host_port": host, "protocol": proto })).collect();
                serde_json::json!({ "name": name, "state": state, "published": published, "address": null, "console": null })
            })
            .collect();
        serde_json::from_value(serde_json::json!({ "state": "ready", "live": true, "provider": null, "machines": machines, "outputs": [] })).unwrap()
    }

    #[test]
    fn open_goes_to_the_lowest_published_web_port_the_lab_declares() {
        // The declared web port wins over a lower port of a machine whose name says nothing.
        let l = lab(&[("portal", &[("site", 80, "http"), ("ssh", 22, "tcp")]), ("metrics", &[("agent", 3000, "tcp")])]);
        let s = status(&[("portal", "running", &[(80, 8443, "tcp"), (22, 2222, "tcp")]), ("metrics", "running", &[(3000, 3000, "tcp")])]);
        assert_eq!(local_url(&l, &s).as_deref(), Some("http://127.0.0.1:8443"));
        // No web port: the lowest of the others, never one named for a non-web service.
        let l = lab(&[("portal", &[("site", 80, "tcp"), ("ssh", 22, "tcp")]), ("metrics", &[("agent", 3000, "tcp")])]);
        assert_eq!(local_url(&l, &s).as_deref(), Some("http://127.0.0.1:3000"));
    }

    #[test]
    fn open_skips_datastores_machines_not_running_and_ports_not_published() {
        // Only a database is up, publishing a low port: no URL.
        let l = lab(&[("database", &[("mysql", 3306, "tcp")]), ("web", &[("site", 80, "http")])]);
        assert_eq!(local_url(&l, &status(&[("database", "running", &[(3306, 3207, "tcp")]), ("web", "stopped", &[])])), None);
        // The web machine runs: its port, though the database's is lower.
        let s = status(&[("database", "running", &[(3306, 3207, "tcp")]), ("web", "running", &[(80, 3206, "tcp"), (53, 53, "udp")])]);
        assert_eq!(local_url(&l, &s).as_deref(), Some("http://127.0.0.1:3206"));
        // A UDP port, or a TCP one not published, is no page.
        let l = lab(&[("web", &[("dns", 53, "udp"), ("site", 80, "http")])]);
        assert_eq!(local_url(&l, &status(&[("web", "running", &[(53, 53, "udp")])])), None);
    }

    #[test]
    fn non_web_services_are_told_by_whole_words_of_their_names() {
        for (machine, port) in
            [("x", "mysql"), ("cache", "redis"), ("db", "app"), ("database", "app"), ("oracle", "xe"), ("shell", "openssh"), ("files", "vsftpd")]
        {
            assert!(not_web(machine, port), "{machine} {port}");
        }
        // Regression: substrings took web apps for them (`wordpress` holds "rdp", `novnc` "vnc").
        for (machine, port) in
            [("wordpress", "http"), ("web", "app"), ("viewer", "novnc"), ("files", "sftpgo"), ("wls", "oracle-weblogic"), ("db-admin", "adminer")]
        {
            assert!(!not_web(machine, port), "{machine} {port}");
        }
    }
}
