//! What a Proxmox VE host's API answers, parsed once, here, into the fields the launcher reads:
//! every answer is `{"data": …}`, and each path's `data` has its own shape. Unknown fields are
//! skipped; a missing field is `None`, so a host that leaves one out reads as it did.

use serde::Deserialize;
use serde::de::DeserializeOwned;

/// The API's envelope: its `data`, none when it is null or missing.
#[derive(Debug, Deserialize)]
struct Answer<T> {
    data: Option<T>,
}

/// The `data` of `body`, an answer of the API (empty when it has none).
///
/// # Errors
///
/// `body` isn't JSON, or not the shape the path answers.
pub fn data<T: DeserializeOwned + Default>(body: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str::<Answer<T>>(body).map(|a| a.data.unwrap_or_default())
}

/// `access/ticket`'s answer to a user's password.
#[derive(Debug, Default, Deserialize)]
pub struct Ticket {
    /// The ticket that signs the next calls in (the `PVEAuthCookie`).
    pub ticket: Option<String>,
}

/// A node of `/nodes`.
#[derive(Debug, Deserialize)]
pub struct Node {
    /// Its name.
    pub node: Option<String>,
    /// `online` or `offline`.
    pub status: Option<String>,
}

/// A storage of `/nodes/<node>/storage`.
#[derive(Debug, Deserialize)]
pub struct Storage {
    /// Its id (`local`, `local-lvm`).
    #[serde(rename = "storage")]
    pub id: Option<String>,
    /// What it accepts, comma-separated (`images,iso,snippets`).
    pub content: Option<String>,
    /// 0 when it is disabled.
    pub enabled: Option<u64>,
    /// 0 when it isn't active.
    pub active: Option<u64>,
}

impl Storage {
    /// What it accepts (`images`, `iso`, `snippets`), each trimmed.
    pub fn contents(&self) -> Vec<String> {
        self.content.as_deref().unwrap_or_default().split(',').map(|c| c.trim().to_string()).collect()
    }

    /// Whether it can be used: not disabled, not inactive (when the API says).
    pub fn usable(&self) -> bool {
        self.enabled != Some(0) && self.active != Some(0)
    }
}

/// A network interface of `/nodes/<node>/network`.
#[derive(Debug, Deserialize)]
pub struct Iface {
    /// Its name (`vmbr0`).
    #[serde(rename = "iface")]
    pub name: Option<String>,
    /// Its address with its prefix (`10.0.0.50/24`).
    pub cidr: Option<String>,
    /// Its address, when the API gives it apart from its netmask.
    pub address: Option<String>,
    /// Its netmask (`255.255.255.0`).
    pub netmask: Option<String>,
    /// A bridge's physical ports, space-separated; empty or missing for a host-internal one.
    pub bridge_ports: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Answers as Proxmox VE 8 gives them (secrets shortened, fields trimmed to a few).
    const TICKET: &str =
        r#"{"data":{"username":"root@pam","ticket":"PVE:root@pam:6553F0A4::c2lnbmVk","CSRFPreventionToken":"6553F0A4:Y3NyZg","cap":{"vms":{"VM.Console":1}}}}"#;
    const NODES: &str = r#"{"data":[{"node":"pve","status":"online","type":"node","id":"node/pve","cpu":0.0123,"maxcpu":8,"mem":2147483648,"maxmem":34359738368,"uptime":86400,"level":"","ssl_fingerprint":"AA:BB"}]}"#;
    const STORAGE: &str = r#"{"data":[{"storage":"local","type":"dir","content":"iso,vztmpl,backup","enabled":1,"active":1,"shared":0,"total":100,"used":10,"avail":90,"used_fraction":0.1},{"storage":"local-lvm","type":"lvmthin","content":"rootdir,images","enabled":1,"active":1,"shared":0}]}"#;
    const NETWORK: &str = r#"{"data":[{"iface":"vmbr0","type":"bridge","method":"static","families":["inet"],"active":1,"autostart":1,"bridge_ports":"eno1","bridge_stp":"off","bridge_fd":"0","cidr":"192.168.1.5/24","address":"192.168.1.5","netmask":"24","gateway":"192.168.1.1","priority":3},{"iface":"eno1","type":"eth","method":"manual","active":1,"exists":1,"priority":2}]}"#;

    #[test]
    fn each_answer_reads_as_its_fields() {
        let t: Ticket = data(TICKET).unwrap();
        assert_eq!(t.ticket.as_deref(), Some("PVE:root@pam:6553F0A4::c2lnbmVk"));
        let nodes: Vec<Node> = data(NODES).unwrap();
        assert_eq!((nodes[0].node.as_deref(), nodes[0].status.as_deref()), (Some("pve"), Some("online")));
        let storages: Vec<Storage> = data(STORAGE).unwrap();
        assert_eq!(storages[0].contents(), ["iso", "vztmpl", "backup"]);
        assert!(storages.iter().all(Storage::usable));
        let ifaces: Vec<Iface> = data(NETWORK).unwrap();
        assert_eq!((ifaces[0].bridge_ports.as_deref(), ifaces[0].cidr.as_deref()), (Some("eno1"), Some("192.168.1.5/24")));
        assert_eq!(ifaces[1].bridge_ports, None);
    }

    #[test]
    fn no_data_is_empty_and_another_shape_an_error() {
        assert!(data::<Vec<Node>>(r#"{"data":null}"#).unwrap().is_empty());
        assert!(data::<Vec<Node>>("{}").unwrap().is_empty());
        assert!(data::<Vec<Node>>(r#"{"data":{"node":"pve"}}"#).is_err(), "not a list");
        assert!(data::<Ticket>("<html>").is_err());
        let off: Vec<Storage> = data(r#"{"data":[{"storage":"nfs","enabled":1,"active":0}]}"#).unwrap();
        assert!(!off[0].usable());
    }
}
