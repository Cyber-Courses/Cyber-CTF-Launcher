//! A VM lab's status from `vagrant status`, and its network topology from the Vagrantfile.

use super::*;

// `vagrant status --machine-readable` lines: timestamp,target,type,data...
pub(super) fn parse_status(out: &str) -> Vec<Machine> {
    out.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, ',');
            let (_ts, target, kind, data) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
            (kind == "state" && !target.is_empty()).then(|| Machine {
                infra: target == CONTROLLER,
                name: target.to_string(),
                state: data.to_string(),
                image: String::new(),
                ip: String::new(),
                ports: Vec::new(),
                interfaces: Vec::new(),
                services: Vec::new(),
            })
        })
        .collect()
}

pub async fn status(dir: &Path, env: &[(String, String)]) -> Result<LabStatus> {
    let out = run_env_timed("vagrant", &["status", "--machine-readable"], Some(dir), env, STATUS_TIMEOUT).await?;
    Ok(status_from(&out, dir))
}

/// A lab's machines' interfaces and its networks, from its spec.
type Topology = (std::collections::HashMap<String, Vec<crate::runtime::Interface>>, Vec<crate::runtime::Network>);

/// A lab folder, when its topology files last changed, and its topology then.
type Cached = (PathBuf, Vec<Option<std::time::SystemTime>>, Option<Topology>);

/// The topology of the lab in `lab` (None without a valid spec), kept while the files it comes
/// from are unchanged: each status poll read and checked the spec, and this machine's Isoloom
/// defaults, from disk again (#238).
pub(super) fn lab_topology(lab: &Path) -> Option<Topology> {
    static CACHE: std::sync::Mutex<Vec<Cached>> = std::sync::Mutex::new(Vec::new());
    let stamps = topology_stamps(lab);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, _, topology)) = cache.iter().find(|(dir, at, _)| dir == lab && *at == stamps) {
        return topology.clone();
    }
    let topology = crate::runtime::lab::spec(lab).ok().map(|spec| crate::runtime::lab::topology(&spec));
    cache.retain(|(dir, _, _)| dir != lab);
    cache.push((lab.to_path_buf(), stamps, topology.clone()));
    topology
}

/// When each file a lab's topology is read from last changed (None: absent).
fn topology_stamps(lab: &Path) -> Vec<Option<std::time::SystemTime>> {
    crate::runtime::lab::spec_files(lab).into_iter().map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok()).collect()
}

/// The status of the VMs in `dir` from `vagrant status --machine-readable` output.
pub(super) fn status_from(out: &str, dir: &Path) -> LabStatus {
    let mut machines = parse_status(out);
    // The lab is up when its targets are; the controller is powered off once they are built.
    let targets = machines.iter().filter(|m| !m.infra);
    let running = targets.clone().count() > 0 && targets.clone().all(|m| m.state == "running");
    // The network diagram, as a Docker lab gets it: addresses are static (declared in the lab),
    // so it comes from the lab's spec (`.isoloom/vagrant` sits two levels under the lab).
    let (mut ifaces, networks) = match dir.parent().and_then(Path::parent).and_then(lab_topology) {
        Some(topology) => topology,
        // No lab spec two levels up (the attack VM's own folder, `<lab>/.cyberctf-attack-vm`):
        // the generated Vagrantfile says where its machines sit.
        None => match std::fs::read_to_string(dir.join("Vagrantfile")) {
            Ok(text) => {
                let lab = super::vm_names_from(&text).first().and_then(|n| n.split(" · ").next()).unwrap_or_default().to_string();
                vagrant_topology(&text, &lab)
            }
            Err(_) => Default::default(),
        },
    };
    for m in &mut machines {
        if let Some(list) = ifaces.remove(&m.name) {
            m.ip = list.first().map(|i| i.ip.clone()).unwrap_or_default();
            m.interfaces = list;
        }
    }
    // The hypervisor Vagrant runs them on (`provider-name` lines), e.g. "virtualbox".
    let provider = out.lines().find_map(|line| {
        let mut parts = line.splitn(4, ',');
        let (_ts, _target, kind, data) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
        (kind == "provider-name" && !data.is_empty()).then(|| data.to_string())
    });
    LabStatus {
        running,
        parked: None,
        machines,
        networks,
        url: None,
        host: None,
        expires_at: None,
        place: None,
        provider,
        attacker: None,
        outputs: Vec::new(),
        message: None,
        reap_error: None,
    }
}

/// Each machine's lab interfaces and the lab's network segments, read from the generated
/// Vagrantfile: a `config.vm.define "<machine>"` block, then its `private_network` lines with
/// `ip:`, `netmask:` and the VirtualBox internal network `isoloom-<lab>-<net>` (`<net>` being the
/// lab's own network name). The subnet is the address masked.
pub(in crate::runtime) fn vagrant_topology(
    text: &str,
    lab: &str,
) -> (std::collections::HashMap<String, Vec<crate::runtime::Interface>>, Vec<crate::runtime::Network>) {
    let prefix = crate::runtime::lab::vm_network_prefix(lab);
    let mut ifaces: std::collections::HashMap<String, Vec<crate::runtime::Interface>> = std::collections::HashMap::new();
    let mut networks: Vec<crate::runtime::Network> = Vec::new();
    let mut machine: Option<String> = None;
    for line in text.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("config.vm.define ") {
            machine = quoted(rest);
            continue;
        }
        let Some(name) = machine.as_deref() else { continue };
        if !l.contains("private_network") {
            continue;
        }
        let (Some(ip), Some(intnet)) = (field(l, "ip:"), field(l, "virtualbox__intnet:")) else { continue };
        let net = intnet.strip_prefix(&prefix).unwrap_or(&intnet).to_string();
        let mask = field(l, "netmask:").unwrap_or_else(|| "255.255.255.0".into());
        if !networks.iter().any(|n| n.name == net) {
            // A VM's private network is a switch between the lab's machines; internet, when the
            // lab allows it, goes out through each VM's own NAT adapter, not through this segment.
            networks.push(crate::runtime::Network { name: net.clone(), subnet: cidr(&ip, &mask).unwrap_or_default(), internal: false });
        }
        ifaces.entry(name.to_string()).or_default().push(crate::runtime::Interface { network: net, ip });
    }
    (ifaces, networks)
}

/// The first double-quoted string in `s`.
pub(super) fn quoted(s: &str) -> Option<String> {
    let open = s.find('"')?;
    let len = s[open + 1..].find('"')?;
    Some(s[open + 1..open + 1 + len].to_string())
}

/// The quoted value of `key: "..."` in a Vagrantfile line.
pub(super) fn field(line: &str, key: &str) -> Option<String> {
    let at = line.find(key)?;
    quoted(&line[at + key.len()..])
}

/// `192.168.56.30` + `255.255.255.0` -> `192.168.56.0/24`.
pub(super) fn cidr(ip: &str, mask: &str) -> Option<String> {
    let ip: std::net::Ipv4Addr = ip.parse().ok()?;
    let mask: std::net::Ipv4Addr = mask.parse().ok()?;
    let (i, m) = (u32::from(ip), u32::from(mask));
    Some(format!("{}/{}", std::net::Ipv4Addr::from(i & m), m.count_ones()))
}
