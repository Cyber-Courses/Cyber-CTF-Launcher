//! A VM lab on a Proxmox server through the Isothrum runner: the Terraform module Isoloom writes
//! (`.isoloom[-<n>]/proxmox/`).
//!
//! What stays the launcher's: the run's name (`cyberctf-<lab id>`), the server's profile turned
//! into the module's variables (`lab::host_vars`: its settings as values, the password or API
//! token as a secret), the node when the profile names none (the server's first, as the profile's
//! Test connection picks it), the node login SSH goes through first when the lab bridge is
//! internal to the node ([`crate::runtime::server::proxmox_jump_for_host`], with the runner's key, which is the
//! launcher's own: both live in the app data folder), and the words the player reads.
//! The runner keeps the Terraform state and the run's slot on the server.

use std::path::Path;

use isothrum_runner::{RunName, RunOptions, SecretValue, Secrets};

use super::opened::OpenedRun;
use super::{errors, progress, view};
use crate::error::Result;
use crate::runtime::docker::Check;
use crate::runtime::{LabStatus, Place, Runtime, lab};

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
}

/// One VM lab on a Proxmox server, run by the runner (or a container lab's helper VM an earlier
/// version left there).
pub(crate) struct ServerRun {
    run: OpenedRun,
}

/// The run's attack box and its opening are [`OpenedRun`]'s, shared with the other remote place.
impl std::ops::Deref for ServerRun {
    type Target = OpenedRun;
    fn deref(&self) -> &OpenedRun {
        &self.run
    }
}

impl ServerRun {
    /// Running `id`, installed in `dir`, on the runner whose home is `home` (the app data folder),
    /// with this host's tools.
    pub(crate) fn new(home: &Path, dir: &Path, id: &str, runtime: Runtime) -> Result<ServerRun> {
        ServerRun::with(super::host_config(home, None), dir, id, runtime)
    }

    /// [`ServerRun::new`] on a runner set up with `config` (tests script its tools).
    fn with(config: isothrum_runner::RunnerConfig, dir: &Path, id: &str, runtime: Runtime) -> Result<ServerRun> {
        let name = RunName::new(&crate::runtime::docker::project(id)).map_err(errors::from_runner)?;
        // A VM lab runs Proxmox's own module, one VM per machine. A container lab never starts
        // there: its run is the helper VM an earlier version left, read from where it ran.
        let (project, target) = if runtime == Runtime::Docker {
            (lab::docker_vm_dir(dir).join("proxmox"), None)
        } else {
            let (out, target) = lab::terraform(dir, runtime, "proxmox");
            (out, Some(target))
        };
        Ok(ServerRun { run: OpenedRun::new(config, dir, target, project, name)? })
    }

    /// The runner's SSH public key (the launcher's own), made when there is none: what the node
    /// must let in for a jump.
    pub(crate) async fn public_key(&self) -> Result<String> {
        self.runner.ssh_public_key().await.map_err(errors::from_runner)
    }

    /// Brings the lab up on `place` from its generated files, and says where to start. `env` is
    /// the launch's: the inputs the lab's slots take.
    pub(crate) async fn up(&self, place: &ServerPlace, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
        super::up_with(&self.running().await?, &options(place, env), log).await.map_err(|e| errors::on_server(e, &place.name))?;
        let message = self.target().and_then(|t| super::described::message(&self.dir, t));
        message.iter().flat_map(|m| m.lines()).for_each(|l| log(l.to_owned()));
        Ok(())
    }

    /// Removes everything the run created on the server, with the params it was applied with
    /// (the connection is `place`'s, the one that works now).
    pub(crate) async fn down(&self, place: &ServerPlace, log: &mut impl FnMut(String)) -> Result<()> {
        let lab = self.running().await?;
        let (sink, lines) = progress::channel();
        progress::follow(log, lines, lab.down(&options(place, &[]), &sink)).await.map_err(|e| errors::on_server(errors::from_runner(e), &place.name))
    }

    /// Brings the running lab up to date with its regenerated files, on `place`.
    pub(crate) async fn update(&self, place: &ServerPlace, log: &mut impl FnMut(String)) -> Result<()> {
        self.update_with(&options(place, &[]), log).await.map_err(|e| errors::on_server(e, &place.name))
    }

    /// Takes `machine` out of the running lab on `place`, ahead of its removal from its files.
    pub(crate) async fn retire(&self, place: &ServerPlace, machine: &str, log: &mut impl FnMut(String)) -> Result<()> {
        self.retire_with(machine, &options(place, &[]), log).await.map_err(|e| errors::on_server(e, &place.name))
    }

    /// The lab as its page shows it, on the server `place` names: its VMs as the server says they
    /// stand (the runner asks its API with `place`'s credentials), one removed there gone.
    pub(crate) async fn status(&self, place: &ServerPlace) -> Result<LabStatus> {
        if !self.has_project() {
            return Ok(LabStatus { place: Some(Place::Server), ..LabStatus::default() });
        }
        // The server's API is asked with `place`'s credentials, which the module's variables
        // carry: a status reads the run's tool as an operation does.
        let lab = self.running().await?;
        let status = lab.status().await.map_err(|e| errors::on_server(errors::from_runner(e), &place.name))?;
        let described = match self.target() {
            Some(_) => super::described::Described::of(&self.dir, super::described::Form::Vms)?,
            None => super::described::Described::default(),
        };
        let shown = view::lab_status(&described, &status, None);
        Ok(LabStatus { place: Some(Place::Server), host: Some(place.name.clone()), provider: Some("proxmox".into()), ..shown })
    }

    /// `machine`'s graphical console, Proxmox's own through its API, signed in with `place`'s
    /// credentials.
    pub(crate) async fn screen(&self, place: &ServerPlace, machine: &str) -> Result<isothrum_runner::Console> {
        self.graphical_console(machine, &options(place, &[])).await.map_err(|e| errors::on_server(e, &place.name))
    }

    /// What opens `machine` in the server's own web console, on `place`.
    pub(crate) async fn native(&self, place: &ServerPlace, machine: &str) -> Result<isothrum_runner::NativeConsole> {
        self.native_console(machine, &options(place, &[])).await.map_err(|e| errors::on_server(e, &place.name))
    }

    /// The lab's checks: not available on a server yet (the runner runs no command in a
    /// Terraform module's VMs).
    pub(crate) fn check(&self) -> Check {
        Check::default()
    }
}

/// The options of an operation on `place`: its variables as the module's values (its secret
/// among the secrets), the discovered node when the profile names none, and the launch's values
/// (`env`) for the lab's inputs. The jump isn't the runner's any more: the node login is the
/// module's own to use.
fn options(place: &ServerPlace, env: &[(String, String)]) -> RunOptions {
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
    for (k, v) in env {
        secrets.insert(k.clone(), SecretValue::new(v.clone()));
    }
    opts.with_secrets(secrets).with_host_label(&place.label)
}

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
    fn the_profiles_own_node_wins_over_the_discovered_one() {
        let mut p = place();
        p.vars.push(("node".into(), "pve2".into()));
        assert_eq!(options(&p, &[]).env.get("node").map(String::as_str), Some("pve2"));
    }

    /// tests/resilience.md M10 and N4: a Proxmox lab's page reads its VMs from the server's API,
    /// signed in with the server's saved token; a VM removed in Proxmox's web UI shows gone, a
    /// revoked token says to update it in Servers.
    mod on_the_server {
        use super::*;
        use crate::runtime::isothrum::test_lab::prelude::*;
        use crate::runtime::lab;

        const SPEC: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.32.0.0/24 }\nmachines:\n  web:\n    os: debian@12\n    networks: { lab: 10 }\n  db:\n    os: debian@12\n    networks: { lab: 11 }\n";

        /// What the fake Proxmox API says of VM 104.
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        enum Api {
            #[default]
            Has,
            Removed,
            Refuses,
        }

        fn answer(api: &Mutex<Api>, call: &Call) -> Reply {
            if call.program != "curl" {
                return Reply::ok("");
            }
            let config = call.stdin.as_ref().map(|p| std::fs::read_to_string(p).unwrap()).unwrap_or_default();
            assert!(config.contains("Authorization: PVEAPIToken=root@pam!cyberctf=sec"), "signed in with the saved token: {config}");
            match *locked(api) {
                Api::Refuses => Reply::fail(22, "curl: (22) The requested URL returned error: 401"),
                Api::Has => Reply::ok(r#"{"data":[{"vmid":104,"status":"running","type":"qemu"},{"vmid":105,"status":"running","type":"qemu"}]}"#),
                // `web` removed in the web UI; `db` runs on.
                Api::Removed => Reply::ok(r#"{"data":[{"vmid":105,"status":"running","type":"qemu"}]}"#),
            }
        }

        /// A Proxmox VM lab whose run is up (its record and Terraform state as an `up` leaves
        /// them), on fake tools.
        fn up() -> (Scripted<Api>, ServerRun) {
            let f = Scripted::answered_by(Home::new("-pve", SPEC, &[], lab::Target::Proxmox), answer);
            let run = ServerRun::with(f.home.config(&f.fake, []), &f.home.dir, "web", Runtime::Vm).unwrap();
            let state = f.home.root.join("runs").join("cyberctf-web");
            std::fs::create_dir_all(&state).unwrap();
            std::fs::write(
                state.join("terraform.tfstate"),
                r#"{"outputs":{"vm_id_web":{"value":104},"vm_id_db":{"value":105}},"resources":[{"instances":[{}]}]}"#,
            )
            .unwrap();
            let record = serde_json::json!({
                "version": 0, "name": "cyberctf-web", "project": run.project, "format": "terraform",
                "state": "ready", "started_at": "2026-11-11T10:00:00Z", "host_label": "pve-1",
            });
            crate::testing::runner::save_run(&f.home.root, &record);
            (f, run)
        }

        fn place() -> ServerPlace {
            let vars = [("proxmox_endpoint", "https://10.0.0.5:8006/"), ("proxmox_api_token", "root@pam!cyberctf=sec")];
            ServerPlace {
                label: "pve-1".into(),
                name: "Home lab".into(),
                vars: vars.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
                ..ServerPlace::default()
            }
        }

        fn web(status: &LabStatus) -> String {
            status.machines.iter().find(|m| m.name == "web").map(|m| m.state.clone()).unwrap()
        }

        #[tokio::test]
        async fn a_vm_missing_from_the_listing_still_reads_and_a_revoked_token_says_to_update_it() {
            let (f, run) = up();
            let shown = run.status(&place()).await.unwrap();
            assert_eq!((shown.running, web(&shown).as_str()), (true, "running"));
            // A VM missing from Proxmox's listing isn't read as removed (the runner's rule: a
            // listing can be partial); the status still reads.
            *f.model() = Api::Removed;
            assert!(run.status(&place()).await.is_ok());
            *f.model() = Api::Refuses;
            let refused = run.status(&place()).await.err().unwrap();
            assert_eq!(refused.code(), crate::error::Code::ServerAuthFailed { server: "Home lab".into() });
        }
    }
}
