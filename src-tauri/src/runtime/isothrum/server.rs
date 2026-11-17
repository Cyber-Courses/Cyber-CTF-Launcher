//! A VM lab on a Proxmox server through the Isothrum runner: the Terraform module Isoloom writes
//! (`.isoloom[-<n>]/proxmox/`).
//!
//! What stays the launcher's: the run's name (`cyberctf-<lab id>`), the server's profile turned
//! into the module's variables (`lab::host_vars`: its settings as values, the password or API
//! token as a secret), the node when the profile names none (the server's first, as the profile's
//! Test connection picks it), the node login SSH goes through first when the lab bridge is
//! internal to the node ([`crate::runtime::server::proxmox_jump_for_host`], with the runner's key, which is the
//! launcher's own: both live in the app data folder; its host keys the ones the player confirmed),
//! and the words the player reads. The runner
//! opens the SOCKS proxy through that node Terraform's SSH goes through while the module is
//! applied or destroyed, and ends it with the step.
//! The runner keeps the Terraform state and the run's slot on the server.

use std::collections::BTreeMap;
use std::path::Path;

use isothrum_runner::{RunOptions, SecretValue, Secrets};

use super::deployment::Deployment;
use super::view;
use crate::error::Result;
use crate::runtime::{LabStatus, Place};

/// The module's output saying where each published port is reached (`view`'s entry reads it).
const PUBLISHED: &str = "published";

/// The run's value naming the jump host its own SSH goes through first (Isothrum's).
const JUMP_VALUE: &str = "ssh_jump";

/// The run's value naming the key that signs in to the jump host (Isothrum's).
const JUMP_KEY_VALUE: &str = "ssh_jump_key_file";

/// The run's value naming the `known_hosts` file the jump host's keys are checked against
/// (Isothrum's).
const JUMP_KNOWN_HOSTS_VALUE: &str = "ssh_jump_known_hosts_file";

/// The module's variables that are the server's secret: connection slots, never params.
const SECRET_VARS: [&str; 2] = ["proxmox_password", "proxmox_api_token"];

/// Where a run goes on its server, as the launcher knows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ServerPlace {
    /// The server's id among the launcher's hosts (recorded: a later operation elsewhere is
    /// refused).
    pub(crate) label: String,
    /// The server's name, as the lab page shows it.
    pub(crate) name: String,
    /// Its profile's Terraform variables, with its secret (`lab::host_vars`).
    pub(crate) vars: Vec<(String, String)>,
    /// The node when the profile names none: the server's first.
    pub(crate) node: Option<String>,
    /// The node login SSH goes through first (`root@<node>`), when the lab bridge is internal.
    pub(crate) jump: Option<String>,
    /// The node host keys the player confirmed (`nodes_known_hosts`), which the jump host's are
    /// checked against: never trusted on first use.
    pub(crate) nodes_known_hosts: Option<std::path::PathBuf>,
}

impl ServerPlace {
    /// Server `conn`'s place for reading its run: its id, its name and its variables. A change
    /// finds its node and its SSH jump first ([`crate::runtime::launch::server_place`]).
    pub(crate) fn of(conn: &crate::runtime::server::Connection) -> ServerPlace {
        ServerPlace { label: conn.id.clone(), name: conn.name.clone(), vars: conn.tf_vars.clone(), ..ServerPlace::default() }
    }
}

/// What an operation that applies the module on `place` runs with: its [`options`], with the
/// launch's values `env`, and, when the lab bridge is internal to the node (`place.jump`), the node
/// as the run's jump host (`ssh_jump`, recorded with the run), signed in to with `key` (the
/// launcher's node key, `ssh_jump_key_file`), its keys checked against the confirmed ones
/// (`ssh_jump_known_hosts_file`). Through it the runner's own SSH goes first (a console, a
/// command, its endpoints' forwards), and Terraform's SSH to the router through the SOCKS proxy
/// the runner opens for each apply and destroy.
pub(super) fn applying(place: &ServerPlace, env: &[(String, String)], key: &Path) -> RunOptions {
    let opts = options(place, env);
    let Some(jump) = &place.jump else { return opts };
    let opts = opts.with_env(JUMP_VALUE, jump.clone()).with_env(JUMP_KEY_VALUE, key.to_string_lossy().into_owned());
    match &place.nodes_known_hosts {
        Some(file) => opts.with_env(JUMP_KNOWN_HOSTS_VALUE, file.to_string_lossy().into_owned()),
        None => opts,
    }
}

impl Deployment {
    /// The lab as its page shows it, on the server `place` names: its VMs as the server says they
    /// stand (the runner asks its API with `place`'s credentials), one removed there gone.
    pub(super) async fn server_status(&self, place: &ServerPlace) -> Result<LabStatus> {
        if !self.has_project() {
            return Ok(LabStatus { place: Some(Place::Server), ..LabStatus::default() });
        }
        // The server's API is asked with `place`'s credentials, which the module's variables
        // carry: a status reads the run's tool as an operation does.
        let mut status = self.read_with(&options(place, &[])).await?;
        self.reached(place, &mut status).await?;
        let shown = view::lab_status(&self.described()?, &status, None);
        Ok(LabStatus { place: Some(Place::Server), host: Some(place.name.clone()), provider: Some("proxmox".into()), ..shown })
    }

    /// `status` with the module's `published` entries where this computer reaches them, as the
    /// runner gives them ([`Deployment::endpoints`]: through the run's jump host, a local
    /// forward); one it can't reach now is left out (no link rather than a broken one), saying why
    /// in the log. Asked only while the lab runs.
    async fn reached(&self, place: &ServerPlace, status: &mut isothrum_runner::Status) -> Result<()> {
        if !view::runs(status) {
            return Ok(());
        }
        let Some(output) = status.outputs.iter_mut().find(|o| o.name == PUBLISHED && !o.sensitive) else { return Ok(()) };
        let mut reached = BTreeMap::new();
        for endpoint in self.endpoints(&options(place, &[])).await? {
            match (endpoint.address, endpoint.why) {
                (Some(at), _) => {
                    reached.insert(endpoint.name, at);
                }
                (None, why) => log::warn!("the lab's {} isn't reachable from this computer: {}", endpoint.name, why.unwrap_or_default()),
            }
        }
        output.value = Some(serde_json::to_string(&reached).map_err(|e| crate::error::Error::Invalid(e.to_string()))?);
        Ok(())
    }
}

/// The options of an operation on `place`: its variables as the module's values (its secret
/// among the secrets), the discovered node when the profile names none, and the launch's values
/// (`env`) for the lab's inputs. The jump host is given by an operation that applies the module
/// ([`applying`]).
pub(super) fn options(place: &ServerPlace, env: &[(String, String)]) -> RunOptions {
    let mut secrets = Secrets::new();
    let mut opts = RunOptions::new();
    for (k, v) in &place.vars {
        if SECRET_VARS.contains(&k.as_str()) {
            secrets.insert(k.clone(), SecretValue::new(v.clone()));
        } else {
            opts = opts.with_env(k.clone(), v.clone());
        }
    }
    if let Some(node) = place.node.as_ref().filter(|_| !opts.env.contains_key("node")) {
        opts = opts.with_env("node", node.clone());
    }
    // The server and the sign-in again under the names Proxmox's provider reads from its
    // environment, which the runner reads too to ask the server for the VMs' states (a VM powered
    // off or removed in Proxmox's own UI), and to reach a VM's screen.
    let var = |name: &str| place.vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    for (name, provider) in PROVIDER_ENV {
        if let Some(v) = var(name) {
            opts = opts.with_env(provider, v);
        }
    }
    for (name, provider) in PROVIDER_SECRETS {
        if let Some(v) = var(name) {
            secrets.insert(provider.to_owned(), SecretValue::new(v));
        }
    }
    for (k, v) in env {
        secrets.insert(k.clone(), SecretValue::new(v.clone()));
    }
    opts.with_secrets(secrets).with_host_label(&place.label)
}

/// The module's server variables and bpg/proxmox's environment names for them, with the pinned
/// certificate's under the name Isothrum's runner reads it.
const PROVIDER_ENV: [(&str, &str); 4] = [
    ("proxmox_endpoint", "PROXMOX_VE_ENDPOINT"),
    ("proxmox_username", "PROXMOX_VE_USERNAME"),
    ("proxmox_insecure", "PROXMOX_VE_INSECURE"),
    (crate::runtime::lab::TLS_PIN, "PROXMOX_VE_TLS_FINGERPRINT"),
];

/// The module's secret variables and bpg/proxmox's environment names for them.
const PROVIDER_SECRETS: [(&str, &str); 2] = [("proxmox_api_token", "PROXMOX_VE_API_TOKEN"), ("proxmox_password", "PROXMOX_VE_PASSWORD")];

#[cfg(test)]
mod tests {
    use super::*;

    fn place() -> ServerPlace {
        let vars = [("proxmox_endpoint", "https://10.0.0.5:8006/"), ("proxmox_password", "pw"), ("uplink_bridge", "vmbr1")];
        ServerPlace {
            label: "host-1".into(),
            name: "Running server".into(),
            vars: vars.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
            node: Some("pve-lab".into()),
            jump: Some("root@10.0.0.5".into()),
            nodes_known_hosts: Some("/data/ssh/nodes_known_hosts".into()),
        }
    }

    #[test]
    fn the_servers_variables_are_params_and_its_secret_a_slot() {
        let opts = options(&place(), &[("CTF_FLAG".into(), "flag{x}".into())]);
        assert_eq!(opts.env.get("proxmox_endpoint").map(String::as_str), Some("https://10.0.0.5:8006/"));
        assert_eq!(opts.env.get("node").map(String::as_str), Some("pve-lab"), "the discovered node");
        assert!(!opts.env.contains_key("proxmox_password"), "a secret is never a plain value");
        assert_eq!(opts.secrets.get("proxmox_password").map(SecretValue::expose), Some("pw"));
        assert_eq!(opts.secrets.get("CTF_FLAG").map(SecretValue::expose), Some("flag{x}"));
        assert_eq!(opts.host_label.as_deref(), Some("host-1"));
    }

    #[test]
    fn a_pinned_certificate_reaches_the_runner_under_the_name_it_reads() {
        let mut p = place();
        p.vars.push(("proxmox_tls_fingerprint".into(), "AB:CD".into()));
        assert_eq!(options(&p, &[]).env.get("PROXMOX_VE_TLS_FINGERPRINT").map(String::as_str), Some("AB:CD"));
        assert!(!options(&place(), &[]).env.contains_key("PROXMOX_VE_TLS_FINGERPRINT"));
    }

    #[test]
    fn the_profiles_own_node_wins_over_the_discovered_one() {
        let mut p = place();
        p.vars.push(("node".into(), "pve2".into()));
        assert_eq!(options(&p, &[]).env.get("node").map(String::as_str), Some("pve2"));
    }

    /// docs/contributing/testing/resilience.md M10 and N4: a Proxmox lab's page reads its VMs from the server's API,
    /// signed in with the server's saved token; a VM removed in Proxmox's web UI shows gone, a
    /// revoked token says to update it in Servers.
    mod on_the_server {
        use super::*;
        use crate::runtime::isothrum::Site;
        use crate::runtime::isothrum::test_lab::prelude::*;
        use crate::runtime::{Runtime, lab};
        use isothrum_runner::fake_https::{Request, Server, fingerprint};

        const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.32.0.0/24 }\nmachines:\n  web:\n    os: debian@12\n    networks: { lab: 10 }\n  db:\n    os: debian@12\n    networks: { lab: 11 }\n";

        /// What the fake Proxmox API says of VM 104.
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        enum Api {
            #[default]
            Has,
            Removed,
            Refuses,
        }

        /// The tools a run here runs: only Terraform is asked.
        fn answer(_: &Mutex<Api>, call: &Call) -> Reply {
            if call.program == "terraform" { terraform(call) } else { Reply::ok("") }
        }

        /// The fake Proxmox API's answer to `request` (the runner's own HTTPS client's), by
        /// what it says of VM 104.
        fn proxmox(api: &Mutex<Api>, request: &Request) -> (u16, String) {
            assert_eq!(request.header("authorization"), Some("PVEAPIToken=root@pam!cyberctf=sec"), "signed in with the saved token");
            assert_eq!(request.method, "GET", "only read");
            if *locked(api) == Api::Refuses {
                return (401, "{\"data\":null}".to_owned());
            }
            if request.target.contains("cluster/nextid") {
                // A VM missing from the listing whose id is still taken: one the token may not see.
                return (400, "{\"data\":null,\"errors\":{\"vmid\":\"VM 104 already exists\"}}".to_owned());
            }
            let listing = match *locked(api) {
                Api::Removed => r#"{"data":[{"vmid":105,"node":"pve","status":"running","type":"qemu"}]}"#,
                _ => r#"{"data":[{"vmid":104,"node":"pve","status":"running","type":"qemu"},{"vmid":105,"node":"pve","status":"running","type":"qemu"}]}"#,
            };
            (200, listing.to_owned())
        }

        /// The lab's fake tools answered by `answer`, the saved server, and the fake Proxmox API
        /// it names: on the loopback, its self-signed certificate pinned as a player confirmed it,
        /// serving until it drops.
        async fn served(home: &str, spec: &str, answer: fn(&Mutex<Api>, &Call) -> Reply) -> (Scripted<Api>, ServerPlace, Server) {
            let f = Scripted::answered_by(Home::new(home, spec, &[], lab::Target::Proxmox), answer);
            let model = f.shared_model();
            let server = Server::start(move |request| proxmox(&model, request)).await.unwrap();
            let place = place(&server);
            (f, place, server)
        }

        /// The fake Terraform's reads of the run's state: its two VMs (104 and 105) and their ids
        /// as outputs.
        fn terraform(call: &Call) -> Reply {
            let vm = |name: &str, id: u32| {
                serde_json::json!({
                    "address": format!("proxmox_virtual_environment_vm.{name}"), "mode": "managed",
                    "type": "proxmox_virtual_environment_vm", "values": { "id": id.to_string(), "vm_id": id, "node_name": "pve" },
                })
            };
            match call.args.first().map(String::as_str) {
                Some("show") => Reply::ok(&serde_json::json!({ "values": { "root_module": { "resources": [vm("web", 104), vm("db", 105)] } } }).to_string()),
                Some("output") => Reply::ok(r#"{"vm_id_web":{"value":104},"vm_id_db":{"value":105}}"#),
                _ => Reply::ok(""),
            }
        }

        /// A Proxmox VM lab whose run is up (its record and Terraform state as an `up` leaves
        /// them), on fake tools.
        async fn up() -> (Scripted<Api>, Deployment, Server) {
            up_answered_by(answer).await
        }

        /// [`up`], its tools answered by `answer`.
        async fn up_answered_by(answer: fn(&Mutex<Api>, &Call) -> Reply) -> (Scripted<Api>, Deployment, Server) {
            let (f, place, api) = served("-pve", SPEC, answer).await;
            let run = Deployment::on_server_with(f.home.config(&f.fake, []), &f.home.dir, "web", Runtime::Vm, place).unwrap();
            let state = f.home.runner().join("runs").join("cyberctf-web");
            std::fs::create_dir_all(&state).unwrap();
            std::fs::write(
                state.join("terraform.tfstate"),
                r#"{"outputs":{"vm_id_web":{"value":104},"vm_id_db":{"value":105}},"resources":[{"instances":[{}]}]}"#,
            )
            .unwrap();
            let record = serde_json::json!({
                "version": 0, "name": "cyberctf-web", "project": run.project(), "format": "terraform",
                "state": "ready", "started_at": "2026-11-11T10:00:00Z", "host_label": "pve-1",
            });
            crate::testing::runner::save_run(&f.home.runner(), &record);
            (f, run, api)
        }

        fn place(server: &Server) -> ServerPlace {
            let vars = [
                ("proxmox_endpoint", format!("{}/", server.endpoint)),
                ("proxmox_api_token", "root@pam!cyberctf=sec".to_owned()),
                (lab::TLS_PIN, fingerprint().to_string()),
            ];
            ServerPlace {
                label: "pve-1".into(),
                name: "Home lab".into(),
                vars: vars.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
                ..ServerPlace::default()
            }
        }

        fn web(status: &LabStatus) -> String {
            status.machines.iter().find(|m| m.name == "web").map(|m| m.state.clone()).unwrap()
        }

        /// [`answer`], with the node's SSH: its key as `ssh-keyscan` reads it, and the runner's proxy
        /// through it (`ssh -N -D`), which listens on its port as ssh does once the node let it in,
        /// until the step ends.
        fn through_the_node(api: &Mutex<Api>, call: &Call) -> Reply {
            static LISTENING: Mutex<Vec<std::net::TcpListener>> = Mutex::new(Vec::new());
            match call.program.as_str() {
                "ssh-keyscan" => Reply::ok("node.lab ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl"),
                "ssh" => {
                    let at = call.args.iter().position(|a| a == "-D").expect("the proxy's -D");
                    let listener = std::net::TcpListener::bind(call.args[at + 1].as_str()).expect("the proxy's port");
                    locked(&LISTENING).push(listener);
                    let mut runs = Reply::ok("");
                    runs.delay = std::time::Duration::from_secs(3600);
                    runs
                }
                _ => answer(api, call),
            }
        }

        #[tokio::test]
        async fn on_a_bridge_internal_to_the_node_terraform_goes_through_the_runners_proxy_to_it() {
            let (f, run, _api) = up_answered_by(through_the_node).await;
            let Site::Server(place) = run.site() else { unreachable!("a server's run") };
            // What the module asks of a change (a start gives them: the node, the runner's key).
            let mut vars = place.vars.clone();
            vars.push(("ssh_private_key_file".into(), run.key().to_string_lossy().into_owned()));
            vars.push(("ssh_public_key".into(), "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl runner".into()));
            // The node's keys, as the player confirmed them.
            let confirmed = run.key().with_file_name("nodes_known_hosts");
            std::fs::create_dir_all(confirmed.parent().unwrap()).unwrap();
            std::fs::write(&confirmed, "node.lab ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl\n").unwrap();
            let jumped =
                ServerPlace { jump: Some("root@node.lab".into()), node: Some("pve".into()), vars, nodes_known_hosts: Some(confirmed.clone()), ..place.clone() };
            let updated = run.at(Site::Server(jumped)).update(&mut |_| {}).await;
            assert!(updated.is_ok(), "the update went through the proxy: {:?}", updated.err().map(|e| e.error.to_string()));
            let calls = f.fake.calls();
            let proxies: Vec<_> = calls.iter().filter(|c| c.program == "ssh").collect();
            assert_eq!(proxies.len(), 1, "one proxy for the operation's apply");
            assert_eq!(proxies[0].args.last().map(String::as_str), Some("root@node.lab"));
            let key = run.key().to_string_lossy().into_owned();
            assert!(proxies[0].args.windows(2).any(|w| w[0] == "-i" && w[1] == key), "the launcher's node key: {:?}", proxies[0].args);
            let checked = format!("UserKnownHostsFile=\"{}\"", confirmed.display());
            assert!(proxies[0].args.contains(&checked), "the node's keys checked against the confirmed ones: {:?}", proxies[0].args);
            assert!(proxies[0].args.iter().any(|a| a == "StrictHostKeyChecking=yes"), "{:?}", proxies[0].args);
            assert!(calls.iter().all(|c| c.program != "ssh-keyscan"), "the node never trusted on first use");
            let at = proxies[0].args.iter().position(|a| a == "-D").unwrap();
            let port = proxies[0].args[at + 1].trim_start_matches("127.0.0.1:").to_owned();
            let applied = calls.iter().find(|c| c.program == "terraform" && c.args.first().map(String::as_str) == Some("apply")).expect("terraform applied");
            assert_eq!(
                (applied.env("TF_VAR_ssh_proxy_host"), applied.env("TF_VAR_ssh_proxy_port")),
                (Some("127.0.0.1"), Some(port.as_str())),
                "{}",
                applied.line()
            );
            // The run keeps how the lab is reached, for what only reads it (a shell, its checks).
            assert_eq!(jump_recorded(&run), Some("root@node.lab".to_owned()));
        }

        #[tokio::test]
        async fn a_bridge_the_launcher_reaches_needs_no_proxy() {
            let (f, run, _api) = up().await;
            let _ = run.update(&mut |_| {}).await;
            assert!(f.fake.calls().iter().all(|c| c.program != "ssh"), "no ssh");
            assert!(f.fake.calls().iter().filter(|c| c.program == "terraform").all(|c| c.env("TF_VAR_ssh_proxy_host").is_none()));
            assert_eq!(jump_recorded(&run), None);
        }

        /// A lab with a web port its module publishes at its router, `web/site` at
        /// 192.0.2.10:8080, its run up on fake tools.
        async fn published_lab(home: &str) -> (Scripted<Api>, Deployment, Server) {
            const WEB: &str = "version: 1\nname: t\nnetworks:\n  app: { cidr: 10.80.0.0/24 }\nmachines:\n  web:\n    os: debian@12\n    networks: { app: 10 }\n    ports: { site: { port: 80, protocol: http, publish: 8080 } }\n";
            let (f, place, server) = served(home, WEB, |api, call| match (call.program.as_str(), call.args.first().map(String::as_str)) {
                ("terraform", Some("output")) => Reply::ok(r#"{"vm_id_web":{"value":104},"published":{"value":{"web/site":"192.0.2.10:8080"}}}"#),
                _ => answer(api, call),
            })
            .await;
            let run = Deployment::on_server_with(f.home.config(&f.fake, []), &f.home.dir, home, Runtime::Vm, place).unwrap();
            let state = f.home.runner().join("runs").join(format!("cyberctf-{home}"));
            std::fs::create_dir_all(&state).unwrap();
            std::fs::write(state.join("terraform.tfstate"), r#"{"outputs":{"vm_id_web":{"value":104}},"resources":[{"instances":[{}]}]}"#).unwrap();
            record_through_the_node(&f, &run, &format!("cyberctf-{home}"));
            (f, run, server)
        }

        /// Records run `name` of `run` up, through the node `root@node.lab` (as its start keeps it).
        fn record_through_the_node(f: &Scripted<Api>, run: &Deployment, name: &str) {
            let record = serde_json::json!({
                "version": 0, "name": name, "project": run.project(), "format": "terraform",
                "state": "ready", "started_at": "2026-11-11T10:00:00Z", "host_label": "pve-1",
                "env": { "ssh_jump": "root@node.lab" },
            });
            crate::testing::runner::save_run(&f.home.runner(), &record);
        }

        /// The jump host run `run` recorded, if any.
        fn jump_recorded(run: &Deployment) -> Option<String> {
            let record = crate::testing::runner::run_record(&run.served.home, run.served.name.as_str());
            record["env"][JUMP_VALUE].as_str().map(str::to_owned)
        }

        #[tokio::test]
        async fn a_lab_reached_directly_is_linked_where_its_module_publishes_it() {
            let (f, run, _api) = published_lab("pve-direct").await;
            let record = serde_json::json!({
                "version": 0, "name": "cyberctf-pve-direct", "project": run.project(), "format": "terraform",
                "state": "ready", "started_at": "2026-11-11T10:00:00Z", "host_label": "pve-1",
            });
            crate::testing::runner::save_run(&f.home.runner(), &record);
            let tools = crate::testing::tools::Tools::script().build();
            assert_eq!(run.status().await.unwrap().url.as_deref(), Some("http://192.0.2.10:8080/"));
            assert!(tools.calls().is_empty(), "no forward");
        }

        /// The fake tools of a lab whose module names checks: its router at 192.0.2.10, `web` and
        /// `db` behind it, a runner on each; `web`'s passes, `db`'s fails.
        fn checked(api: &Mutex<Api>, call: &Call) -> Reply {
            const OUTPUTS: &str = r#"{"ip":{"value":"192.0.2.10"},"ssh_user":{"value":"isoloom"},"machines":{"value":{"web":"10.32.0.10","db":"10.32.0.11"}},"checks":{"value":[{"position":"web","machine":"web","host":"10.32.0.10","user":"isoloom","script":".isoloom/proxmox/checks/web.sh"},{"position":"db","machine":"db","host":"10.32.0.11","user":"isoloom","script":".isoloom/proxmox/checks/db.sh"}]}}"#;
            match (call.program.as_str(), call.args.first().map(String::as_str)) {
                ("terraform", Some("output")) => Reply::ok(OUTPUTS),
                ("ssh-keyscan", _) => Reply::ok("node.lab ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl"),
                ("ssh", _) if call.args.last().is_some_and(|c| c.contains("WEB-RUNNER")) => Reply::ok("isoloom-check: PASS web serves its page"),
                ("ssh", _) => {
                    let mut failed = Reply::ok("isoloom-check: FAIL db answers: nothing on 5432");
                    failed.code = 1;
                    failed
                }
                _ => answer(api, call),
            }
        }

        #[tokio::test]
        async fn a_server_labs_checks_run_in_its_machines_through_the_runner_through_the_node_first() {
            let (f, run, _api) = up_answered_by(checked).await;
            record_through_the_node(&f, &run, "cyberctf-web");
            let scripts = run.project().join("checks");
            std::fs::create_dir_all(&scripts).unwrap();
            std::fs::write(scripts.join("web.sh"), "echo WEB-RUNNER\n").unwrap();
            std::fs::write(scripts.join("db.sh"), "echo DB-RUNNER\n").unwrap();
            let check = run.check().await.unwrap();
            assert!(check.available && !check.ok, "{check:?}");
            let results: Vec<(&str, bool, &str)> = check.results.iter().map(|r| (r.name.as_str(), r.ok, r.from.as_str())).collect();
            assert_eq!(results, [("web serves its page", true, "from web"), ("db answers", false, "from db")]);
            let ssh: Vec<_> = f.fake.calls().into_iter().filter(|c| c.program == "ssh").collect();
            assert_eq!(ssh.len(), 2, "one exec per runner");
            let web = &ssh[0];
            assert_eq!(web.args.last().map(String::as_str), Some("sh -c 'echo WEB-RUNNER\n'"), "its script, handed over");
            assert_eq!(web.args[web.args.len() - 2], "isoloom@10.32.0.10");
            let proxy = web.args.iter().find_map(|a| a.strip_prefix("ProxyCommand=")).expect("through the router");
            assert!(proxy.ends_with("-W %h:%p isoloom@192.0.2.10") && proxy.contains("root@node.lab"), "the node first: {proxy}");
        }

        #[tokio::test]
        async fn a_server_lab_whose_module_names_no_check_has_none_available() {
            let (_f, run, _api) = up().await;
            let check = run.check().await.unwrap();
            assert!(!check.available && check.results.is_empty(), "{check:?}");
        }

        #[tokio::test]
        async fn a_vm_missing_from_the_listing_still_reads_and_a_revoked_token_says_to_update_it() {
            let (f, run, _api) = up().await;
            let shown = run.status().await.unwrap();
            assert_eq!((shown.running, web(&shown).as_str()), (true, "running"));
            // A VM missing from Proxmox's listing isn't read as removed (the runner's rule: a
            // listing can be partial); the status still reads.
            *f.model() = Api::Removed;
            assert!(run.status().await.is_ok());
            *f.model() = Api::Refuses;
            let refused = run.status().await.err().unwrap();
            assert_eq!(refused.code(), crate::error::Code::ServerAuthFailed { server: "Home lab".into() });
        }
    }
}
