//! Talking to a Proxmox VE host's API for the Server screen: sign in (API token or
//! user + password), then check what a lab launch will need, so problems show up in
//! "Test connection" with their fix instead of halfway through a launch.
//!
//! A token is recognised by its id, `user@realm!name` (the username field holds it and
//! the keychain holds its secret). The cloud-init snippet still goes over SSH, which a
//! token can't do: token setups SSH as the token's user with the launcher's key.

use std::time::Duration;

use serde_json::Value;

use super::server::HostProfile;
use super::ssh;

const TIMEOUT: Duration = Duration::from_secs(8);

/// The storage `deploy/terraform/proxmox` puts the Debian image and the cloud-init
/// snippet on (its defaults, not configurable from the launcher).
pub const IMAGE_STORAGE: &str = "local";
pub const SNIPPET_STORAGE: &str = "local";

/// `user@realm!name`: a Proxmox API token id rather than a user.
pub fn is_token(username: &str) -> bool {
    username.split_once('!').is_some_and(|(user, name)| user.contains('@') && !name.is_empty())
}

/// The user a token belongs to (`root@pam!cyberctf` -> `root@pam`).
pub fn token_user(username: &str) -> &str {
    username.split_once('!').map(|(u, _)| u).unwrap_or(username)
}

/// The `api_token` value the Terraform provider takes.
pub fn terraform_token(username: &str, secret: &str) -> String {
    format!("{username}={secret}")
}

/// A host as it goes in a URL (IPv6 in brackets).
fn host_for_url(host: &str) -> String {
    if host.contains(':') { format!("[{host}]") } else { host.to_string() }
}

/// The API base URL, e.g. `https://pve.lan:8006/api2/json`.
pub fn api_url(h: &HostProfile) -> String {
    format!("https://{}:{}/api2/json", host_for_url(&h.host), h.port)
}

/// The endpoint the Terraform provider takes, e.g. `https://pve.lan:8006/`.
pub fn terraform_endpoint(h: &HostProfile) -> String {
    format!("https://{}:{}/", host_for_url(&h.host), h.port)
}

/// The bridge lab VMs attach to (the host's setting, else the Proxmox default).
fn lab_bridge(h: &HostProfile) -> String {
    h.network.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "vmbr0".into())
}

/// An interface of `/nodes/<node>/network` by name.
fn find_iface<'a>(ifaces: &'a Value, name: &str) -> Option<&'a Value> {
    ifaces.as_array()?.iter().find(|i| i["iface"].as_str() == Some(name))
}

/// A bridge without physical ports (`bridge_ports` empty or missing).
fn has_no_ports(iface: &Value) -> bool {
    iface["bridge_ports"].as_str().map(str::trim).unwrap_or_default().is_empty()
}

/// A signed-in API client.
pub struct Session {
    client: reqwest::Client,
    base: String,
    /// The header that authenticates each call: a ticket cookie or the token.
    auth: (&'static str, String),
}

#[derive(Debug)]
pub enum SignInError {
    /// The host answered and refused the credentials.
    Rejected,
    /// Couldn't talk to the API (TLS, network, unexpected answer).
    Failed(String),
}

pub async fn sign_in(h: &HostProfile, secret: &str) -> Result<Session, SignInError> {
    let client =
        reqwest::Client::builder().timeout(TIMEOUT).tls_danger_accept_invalid_certs(h.insecure_tls).build().map_err(|e| SignInError::Failed(e.to_string()))?;
    let base = api_url(h);
    if is_token(&h.username) {
        let session = Session { client, base, auth: ("Authorization", format!("PVEAPIToken={}={secret}", h.username)) };
        // Any authenticated call proves the token.
        return match session.call("/version").await {
            Ok(_) => Ok(session),
            Err(CallError::Status(401)) => Err(SignInError::Rejected),
            Err(e) => Err(SignInError::Failed(e.to_string())),
        };
    }
    let res = client
        .post(format!("{base}/access/ticket"))
        .form(&[("username", h.username.as_str()), ("password", secret)])
        .send()
        .await
        .map_err(|e| SignInError::Failed(describe(&e)))?;
    if res.status().as_u16() == 401 {
        return Err(SignInError::Rejected);
    }
    if !res.status().is_success() {
        return Err(SignInError::Failed(format!("the Proxmox API answered HTTP {}", res.status())));
    }
    let body: Value = res.json().await.map_err(|e| SignInError::Failed(e.to_string()))?;
    let ticket = body["data"]["ticket"].as_str().ok_or_else(|| SignInError::Failed("no ticket in the sign-in answer".into()))?;
    Ok(Session { client, base, auth: ("Cookie", format!("PVEAuthCookie={ticket}")) })
}

/// Turns a reqwest error into one readable line, calling out the usual self-signed case.
pub fn describe(e: &reqwest::Error) -> String {
    let detail = std::iter::successors(Some(e as &dyn std::error::Error), |e| e.source()).map(|e| e.to_string()).collect::<Vec<_>>().join(": ");
    if detail.to_lowercase().contains("certificate") {
        "the host's TLS certificate isn't trusted. Proxmox uses a self-signed certificate by default: turn on \"Self-signed certificate\" for this host, or install a trusted one".into()
    } else {
        detail
    }
}

#[derive(Debug)]
pub enum CallError {
    Status(u16),
    Other(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Status(code) => write!(f, "HTTP {code}"),
            CallError::Other(e) => f.write_str(e),
        }
    }
}

impl Session {
    /// GET `path` (under /api2/json) and return its `data`.
    pub async fn call(&self, path: &str) -> Result<Value, CallError> {
        let res = self.client.get(format!("{}{path}", self.base)).header(self.auth.0, &self.auth.1).send().await.map_err(|e| CallError::Other(describe(&e)))?;
        if !res.status().is_success() {
            return Err(CallError::Status(res.status().as_u16()));
        }
        let body: Value = res.json().await.map_err(|e| CallError::Other(e.to_string()))?;
        Ok(body["data"].clone())
    }

    /// The configured node, or the first one when none is set.
    pub async fn node(&self, h: &HostProfile) -> Result<String, CallError> {
        pick_node(&self.call("/nodes").await?, h.node.as_deref())
    }
}

/// The configured node among `/nodes`, or the first one when none is set.
fn pick_node(nodes: &Value, configured: Option<&str>) -> Result<String, CallError> {
    let names: Vec<&str> = nodes.as_array().into_iter().flatten().filter_map(|n| n["node"].as_str()).collect();
    match configured.filter(|n| !n.is_empty()) {
        Some(n) if names.contains(&n) => Ok(n.to_string()),
        Some(n) => Err(CallError::Other(format!("there is no node \"{n}\" on this host (found: {})", names.join(", ")))),
        None => names.first().map(|n| n.to_string()).ok_or_else(|| CallError::Other("this user can't see any node".into())),
    }
}

/// The outcome of "Test connection" for a Proxmox host.
pub struct Report {
    pub ok: bool,
    pub authenticated: Option<bool>,
    pub message: String,
}

fn fail(authenticated: Option<bool>, message: impl Into<String>) -> Report {
    Report { ok: false, authenticated, message: message.into() }
}

/// What a storage accepts, from `/nodes/<node>/storage` (`content` is comma-separated).
fn storage_content(storages: &Value, id: &str) -> Option<Vec<String>> {
    storages
        .as_array()?
        .iter()
        .find(|s| s["storage"].as_str() == Some(id))
        .map(|s| s["content"].as_str().unwrap_or_default().split(',').map(|c| c.trim().to_string()).collect())
}

const STORAGE_HINT: &str = "In the Proxmox web UI: Datacenter > Storage > select it > Edit > Content";

/// Missing storage content a lab launch needs, as fix-it sentences.
fn storage_problems(storages: &Value, vm_storage: &str) -> Vec<String> {
    let mut needs: Vec<(&str, &str, &str)> =
        vec![(IMAGE_STORAGE, "iso", "ISO image"), (SNIPPET_STORAGE, "snippets", "Snippets"), (vm_storage, "images", "Disk image")];
    needs.dedup();
    let mut problems = Vec::new();
    for (id, content, label) in needs {
        match storage_content(storages, id) {
            None => problems.push(format!("Storage \"{id}\" doesn't exist on this node, or this user can't see it.")),
            Some(c) if !c.iter().any(|x| x == content) => problems.push(format!("Storage \"{id}\" doesn't accept {label}s. {STORAGE_HINT}, add '{label}'.")),
            Some(_) => {}
        }
    }
    problems
}

/// Signs in, then checks the node, the storages and bridge a launch uses and, for token
/// setups, that the launcher's SSH key is authorized on the node.
pub async fn test(h: &HostProfile, secret: &str) -> Report {
    let session = match sign_in(h, secret).await {
        Ok(s) => s,
        Err(e) => return sign_in_report(e, is_token(&h.username)),
    };
    let node = match session.node(h).await {
        Ok(n) => n,
        Err(e) => return node_report(e),
    };

    let mut problems = Vec::new();
    let vm_storage = h.datastore.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "local-lvm".into());
    match session.call(&format!("/nodes/{node}/storage")).await {
        Ok(storages) => problems.extend(storage_problems(&storages, &vm_storage)),
        Err(e) => problems.push(format!("Couldn't list the node's storage ({e}).")),
    }
    let bridge = lab_bridge(h);
    match session.call(&format!("/nodes/{node}/network")).await {
        Ok(ifaces) => problems.extend(bridge_problem(&ifaces, &bridge, &node)),
        Err(e) => problems.push(format!("Couldn't list the node's network ({e}).")),
    }
    if is_token(&h.username)
        && let Some(problem) = ssh_key_problem(h).await
    {
        problems.push(problem);
    }
    outcome(&node, &bridge, &problems)
}

/// "Test connection" when signing in failed.
fn sign_in_report(e: SignInError, token: bool) -> Report {
    match e {
        SignInError::Rejected if token => fail(Some(false), "Proxmox rejected the API token. Check the token id (user@realm!name) and its secret."),
        SignInError::Rejected => fail(Some(false), "Proxmox rejected the credentials. Use user@realm, e.g. root@pam."),
        SignInError::Failed(e) => fail(None, format!("Reachable, but the API call failed: {e}")),
    }
}

/// "Test connection" when listing the nodes failed.
fn node_report(e: CallError) -> Report {
    match e {
        CallError::Status(403) => fail(
            Some(true),
            "Signed in, but this user or token can't list nodes. Give it the Administrator role, or create the token with privilege separation off.",
        ),
        e => fail(Some(true), format!("Signed in, but listing nodes failed: {e}")),
    }
}

/// The lab bridge missing from the node's interfaces, as a fix-it sentence.
fn bridge_problem(ifaces: &Value, bridge: &str, node: &str) -> Option<String> {
    find_iface(ifaces, bridge)
        .is_none()
        .then(|| format!("Bridge \"{bridge}\" doesn't exist on {node}. Use one of the node's bridges (System > Network), e.g. vmbr0."))
}

/// "Test connection" once signed in: ready, or every problem a launch would hit.
fn outcome(node: &str, bridge: &str, problems: &[String]) -> Report {
    if problems.is_empty() {
        Report {
            ok: true,
            authenticated: Some(true),
            message: format!("Signed in to the Proxmox API; node {node}, storage and bridge {bridge} are ready for labs."),
        }
    } else {
        fail(Some(true), format!("Signed in, but a lab launch would fail:\n• {}", problems.join("\n• ")))
    }
}

/// Whether the host's lab bridge is host-internal (no physical port, e.g. a NAT bridge for
/// lab VMs): this machine can't route to VMs on it, only the node can. Unknown counts as no.
pub async fn bridge_is_internal(h: &HostProfile, secret: &str) -> bool {
    let Ok(session) = sign_in(h, secret).await else { return false };
    let Ok(node) = session.node(h).await else { return false };
    let Ok(ifaces) = session.call(&format!("/nodes/{node}/network")).await else { return false };
    find_iface(&ifaces, &lab_bridge(h)).is_some_and(has_no_ports)
}

/// The node's SSH login (`user@host`) for going through it.
pub fn node_login(h: &HostProfile) -> String {
    format!("{}@{}", ssh_user(&h.username), h.host)
}

/// Lets the launcher's key into the node's SSH (password hosts; a token setup already needs
/// it, see `ssh_key_problem`), so labs on an internal bridge can be reached through the node.
/// Uses the stored password once, through SSH's own askpass hook; idempotent.
pub async fn authorize_launcher_key(h: &HostProfile, password: &str, identity: &std::path::Path, public: &str) -> crate::error::Result<()> {
    // Next to the launcher's key (the player's own app data), not the shared system temp dir:
    // on Linux /tmp is common to every account, and a file another user left there under this
    // name can't be rewritten (or could be swapped for something else).
    let askpass = write_askpass(identity)?;
    let env = vec![
        ("SSH_ASKPASS".to_string(), askpass.display().to_string()),
        ("SSH_ASKPASS_REQUIRE".to_string(), "force".to_string()),
        ("DISPLAY".to_string(), ":0".to_string()),
        ("CYBERCTF_SSH_PASSWORD".to_string(), password.to_string()),
    ];
    let args = authorize_args(h, identity, public);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    crate::exec::run_env("ssh", &refs, None, &env).await.map(|_| ())
}

/// The askpass helper next to the launcher's key: it answers SSH's prompt with the password
/// passed in the environment.
fn write_askpass(identity: &std::path::Path) -> crate::error::Result<std::path::PathBuf> {
    let askpass = identity.with_file_name("askpass.sh");
    std::fs::write(&askpass, "#!/bin/sh\nprintf '%s\\n' \"$CYBERCTF_SSH_PASSWORD\"\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&askpass, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(askpass)
}

/// The `ssh` arguments that add `public` to the node's authorized keys, signing in by password.
fn authorize_args(h: &HostProfile, identity: &std::path::Path, public: &str) -> Vec<String> {
    let key = ssh::sh_quote(public.trim());
    let command = format!("umask 077; mkdir -p ~/.ssh; grep -qxF {key} ~/.ssh/authorized_keys 2>/dev/null || echo {key} >> ~/.ssh/authorized_keys");
    let known_hosts = identity.with_file_name("known_hosts").display().to_string();
    let password_auth = ["PreferredAuthentications=password,keyboard-interactive".to_string(), "NumberOfPasswordPrompts=1".to_string()];
    let mut args: Vec<String> = Vec::new();
    for o in password_auth.into_iter().chain(ssh::options(false, &known_hosts)) {
        args.extend(["-o".into(), o]);
    }
    // `--`: the login comes from the host profile, so it can never be read as an ssh option
    // (a host such as `-oProxyCommand=...`).
    args.extend(["--".into(), node_login(h), command]);
    args
}

/// The SSH user for the snippet upload: the token's user without its realm.
pub fn ssh_user(username: &str) -> &str {
    token_user(username).split('@').next().unwrap_or("root")
}

/// Token setups upload the snippet over SSH with the launcher's key: checks it's authorized.
async fn ssh_key_problem(h: &HostProfile) -> Option<String> {
    let Some(identity) = ssh::launcher_key() else {
        return Some("The launcher's SSH key isn't ready yet; test again.".into());
    };
    let public = std::fs::read_to_string(identity.with_extension("pub")).unwrap_or_default().trim().to_string();
    let user = ssh_user(&h.username).to_string();
    let target = ssh::Target::direct(&h.host, &user, identity.clone());
    match target.exec_timed(&identity.with_file_name("known_hosts"), "true", std::time::Duration::from_secs(30)).await {
        Ok(_) => None,
        Err(_) => Some(format!(
            "The launcher can't SSH to {user}@{} with its key, which a token setup needs to upload the lab's cloud-init. On the node, run:\n  echo '{public}' >> ~/.ssh/authorized_keys",
            h.host
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recognises_token_ids() {
        assert!(is_token("root@pam!cyberctf"));
        assert!(!is_token("root@pam"));
        assert!(!is_token("root!x"));
        assert!(!is_token("root@pam!"));
        assert_eq!(token_user("root@pam!cyberctf"), "root@pam");
        assert_eq!(ssh_user("root@pam!cyberctf"), "root");
        assert_eq!(ssh_user("admin@pve"), "admin");
        assert_eq!(terraform_token("root@pam!cyberctf", "abc"), "root@pam!cyberctf=abc");
    }

    fn host(addr: &str, network: Option<&str>) -> HostProfile {
        HostProfile {
            gcp_project: None,
            id: "t".into(),
            name: "t".into(),
            provider: crate::runtime::providers::Provider::Proxmox,
            host: addr.into(),
            port: 8006,
            username: "root@pam".into(),
            datastore: None,
            network: network.map(str::to_string),
            node: None,
            insecure_tls: false,
            auto_stop_hours: None,
            use_cli_creds: false,
            aws_profile: None,
            monthly_limit: None,
        }
    }

    #[test]
    fn urls_bracket_ipv6_hosts() {
        assert_eq!(api_url(&host("10.0.0.5", None)), "https://10.0.0.5:8006/api2/json");
        assert_eq!(api_url(&host("fd00::5", None)), "https://[fd00::5]:8006/api2/json");
        assert_eq!(terraform_endpoint(&host("pve.lan", None)), "https://pve.lan:8006/");
        assert_eq!(terraform_endpoint(&host("fd00::5", None)), "https://[fd00::5]:8006/");
        assert_eq!(node_login(&host("pve.lan", None)), "root@pve.lan");
    }

    #[test]
    fn lab_bridge_defaults_to_vmbr0() {
        assert_eq!(lab_bridge(&host("h", None)), "vmbr0");
        assert_eq!(lab_bridge(&host("h", Some(""))), "vmbr0");
        assert_eq!(lab_bridge(&host("h", Some("vmbr1"))), "vmbr1");
    }

    #[test]
    fn internal_bridges_have_no_ports() {
        let ifaces = json!([{"iface": "vmbr0", "bridge_ports": "eno1"}, {"iface": "vmbr1", "bridge_ports": " "}, {"iface": "vmbr2"}]);
        assert!(!find_iface(&ifaces, "vmbr0").is_some_and(has_no_ports));
        assert!(find_iface(&ifaces, "vmbr1").is_some_and(has_no_ports));
        assert!(find_iface(&ifaces, "vmbr2").is_some_and(has_no_ports));
        assert!(find_iface(&ifaces, "vmbr9").is_none());
        assert!(find_iface(&json!(null), "vmbr0").is_none());
    }

    #[test]
    fn storage_problems_name_the_fix() {
        let ok = json!([{"storage": "local", "content": "iso,vztmpl,snippets,backup"}, {"storage": "local-lvm", "content": "images,rootdir"}]);
        assert!(storage_problems(&ok, "local-lvm").is_empty());
        let stock = json!([{"storage": "local", "content": "iso,vztmpl,backup"}, {"storage": "local-lvm", "content": "images,rootdir"}]);
        let p = storage_problems(&stock, "local-lvm");
        assert_eq!(p.len(), 1);
        assert!(p[0].contains("Snippets"));
        let p = storage_problems(&stock, "fast-ssd");
        assert!(p.iter().any(|m| m.contains("\"fast-ssd\" doesn't exist")));
    }

    /// "Test connection" against a real host (opt-in):
    ///   CYBERCTF_TEST_PVE_HOST=... CYBERCTF_TEST_PVE_PASSWORD=... [CYBERCTF_TEST_PVE_TOKEN_ID=user@realm!name CYBERCTF_TEST_PVE_TOKEN_SECRET=...
    ///   CYBERCTF_TEST_SSH_KEY=<key authorized on the node>] cargo test proxmox_live_test -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn proxmox_live_test() {
        let var = |k: &str| std::env::var(k).ok();
        let address = var("CYBERCTF_TEST_PVE_HOST").expect("CYBERCTF_TEST_PVE_HOST");
        let profile = |username: &str, bridge: &str| HostProfile {
            username: username.into(),
            datastore: Some(var("CYBERCTF_TEST_PVE_STORAGE").unwrap_or_else(|| "local-lvm".into())),
            node: Some("pve".into()),
            insecure_tls: true,
            ..host(&address, Some(bridge))
        };
        let bridge = var("CYBERCTF_TEST_PVE_BRIDGE").unwrap_or_else(|| "vmbr1".into());
        let password = var("CYBERCTF_TEST_PVE_PASSWORD").expect("CYBERCTF_TEST_PVE_PASSWORD");

        let r = test(&profile("root@pam", &bridge), &password).await;
        println!("password: ok={} {}", r.ok, r.message);
        assert!(r.ok);
        let r = test(&profile("root@pam", &bridge), "wrong").await;
        println!("bad password: {}", r.message);
        assert_eq!(r.authenticated, Some(false));
        let r = test(&profile("root@pam", "vmbr-nope"), &password).await;
        println!("bad bridge: {}", r.message);
        assert!(!r.ok && r.message.contains("vmbr-nope"));

        if let (Some(secret), Some(key)) = (var("CYBERCTF_TEST_PVE_TOKEN_SECRET"), var("CYBERCTF_TEST_SSH_KEY")) {
            ssh::set_launcher_key_for_test(key.into());
            let token = var("CYBERCTF_TEST_PVE_TOKEN_ID").unwrap_or_else(|| "root@pam!cyberctf-live".into());
            let r = test(&profile(&token, &bridge), &secret).await;
            println!("token: ok={} {}", r.ok, r.message);
            assert!(r.ok);
            let r = test(&profile(&token, &bridge), "00000000-0000-0000-0000-000000000000").await;
            println!("bad token: {}", r.message);
            assert_eq!(r.authenticated, Some(false));
        }
    }

    #[test]
    fn the_configured_node_or_the_first() {
        let nodes = json!([{"node": "pve"}, {"node": "pve2"}]);
        assert_eq!(pick_node(&nodes, None).unwrap(), "pve");
        assert_eq!(pick_node(&nodes, Some("")).unwrap(), "pve");
        assert_eq!(pick_node(&nodes, Some("pve2")).unwrap(), "pve2");
        let err = pick_node(&nodes, Some("nope")).unwrap_err().to_string();
        assert_eq!(err, "there is no node \"nope\" on this host (found: pve, pve2)");
        assert_eq!(pick_node(&json!([]), None).unwrap_err().to_string(), "this user can't see any node");
        assert!(pick_node(&json!(null), None).is_err());
    }

    #[test]
    fn call_errors_read_as_http_codes_or_their_message() {
        assert_eq!(CallError::Status(401).to_string(), "HTTP 401");
        assert_eq!(CallError::Other("boom".into()).to_string(), "boom");
    }

    #[test]
    fn sign_in_failures_say_what_to_fix() {
        let r = sign_in_report(SignInError::Rejected, true);
        assert!(!r.ok && r.authenticated == Some(false) && r.message.contains("API token"));
        let r = sign_in_report(SignInError::Rejected, false);
        assert!(r.authenticated == Some(false) && r.message.contains("root@pam"));
        let r = sign_in_report(SignInError::Failed("TLS".into()), false);
        assert_eq!(r.authenticated, None);
        assert_eq!(r.message, "Reachable, but the API call failed: TLS");
    }

    #[test]
    fn node_listing_failures_are_signed_in_failures() {
        let r = node_report(CallError::Status(403));
        assert!(!r.ok && r.authenticated == Some(true) && r.message.contains("Administrator role"));
        let r = node_report(CallError::Status(500));
        assert_eq!(r.message, "Signed in, but listing nodes failed: HTTP 500");
    }

    #[test]
    fn a_missing_bridge_is_a_problem() {
        let ifaces = json!([{"iface": "vmbr0"}]);
        assert_eq!(bridge_problem(&ifaces, "vmbr0", "pve"), None);
        assert!(bridge_problem(&ifaces, "vmbr1", "pve").unwrap().starts_with("Bridge \"vmbr1\" doesn't exist on pve."));
    }

    #[test]
    fn the_outcome_lists_every_problem() {
        let ok = outcome("pve", "vmbr0", &[]);
        assert!(ok.ok && ok.authenticated == Some(true));
        assert!(ok.message.contains("node pve") && ok.message.contains("vmbr0"));
        let bad = outcome("pve", "vmbr0", &["one".into(), "two".into()]);
        assert!(!bad.ok);
        assert_eq!(bad.message, "Signed in, but a lab launch would fail:\n• one\n• two");
    }

    #[test]
    fn storage_needs_each_content_once() {
        // The VM disks on `local` too: still one check per (storage, content).
        let all = json!([{"storage": "local", "content": "iso, snippets, images"}]);
        assert!(storage_problems(&all, "local").is_empty());
        assert_eq!(storage_content(&json!([{"storage": "x"}]), "x"), Some(vec![String::new()]));
        assert_eq!(storage_content(&json!({}), "x"), None);
    }

    #[test]
    fn authorizing_the_key_signs_in_by_password_once() {
        let dir = std::env::temp_dir().join(format!("cyberctf-pve-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        let identity = dir.join("id_ed25519");
        let askpass = write_askpass(&identity).unwrap();
        assert_eq!(askpass, dir.join("askpass.sh"));
        assert!(std::fs::read_to_string(&askpass).unwrap().contains("$CYBERCTF_SSH_PASSWORD"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&askpass).unwrap().permissions().mode() & 0o777, 0o700);
        }
        let args = authorize_args(&host("pve.lan", None), &identity, "ssh-ed25519 AAAA launcher\n");
        assert_eq!(args[..4], ["-o", "PreferredAuthentications=password,keyboard-interactive", "-o", "NumberOfPasswordPrompts=1"]);
        // Escaped for ssh's option parser (a Windows path's backslashes, a space).
        assert!(args.contains(&format!("UserKnownHostsFile={}", ssh::ssh_config_path(&dir.join("known_hosts").display().to_string()))));
        assert_eq!(args[args.len() - 3..args.len() - 1], ["--", "root@pve.lan"]);
        assert!(args.last().unwrap().contains("grep -qxF 'ssh-ed25519 AAAA launcher' ~/.ssh/authorized_keys"), "{args:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn an_unreachable_host_fails_to_sign_in() {
        // Port 1 on the loopback refuses at once: no network beyond this machine.
        let mut h = host("127.0.0.1", None);
        h.port = 1;
        assert!(matches!(sign_in(&h, "pw").await, Err(SignInError::Failed(_))));
        let r = test(&h, "pw").await;
        assert!(!r.ok && r.authenticated.is_none(), "{}", r.message);
        assert!(!bridge_is_internal(&h, "pw").await);
        h.username = "root@pam!t".into();
        assert!(matches!(sign_in(&h, "pw").await, Err(SignInError::Failed(_))));
    }
}
