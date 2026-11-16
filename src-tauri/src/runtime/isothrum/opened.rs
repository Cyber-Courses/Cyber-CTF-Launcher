//! A lab run the launcher opens from its generated Terraform module under its own name: a
//! server's (Proxmox) or a cloud account's. Both are changed (brought up to date, a machine
//! retired), read and consoled into the same way.

use std::path::{Path, PathBuf};

use isothrum_runner::{Format, RunName, RunOptions, Runner, RunnerConfig, Running};

use super::reach::Reach;
use super::{errors, remote};
use crate::error::{Error, Result};
use crate::runtime::{lab, ssh};

/// What a run's Terraform module is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Module {
    /// The one Isoloom generates for a target.
    Generated(lab::Target),
    /// The lab's own, hand-written (a cloud-services lab's `cloud/`).
    Own,
    /// A helper VM's an earlier version left on a Proxmox server, whose files Isoloom no longer
    /// generates: only read, stopped and removed.
    HelperVm,
}

/// A lab's run on the runner, opened by its name from its Terraform module.
pub(crate) struct OpenedRun {
    pub(super) runner: Runner,
    /// The runner's home: where its SSH key is (`ssh/id_ed25519`), the launcher's own.
    home: PathBuf,
    /// The lab's folder.
    pub(super) dir: PathBuf,
    /// What its module is.
    module: Module,
    /// The run's Terraform module.
    pub(super) project: PathBuf,
    pub(super) name: RunName,
    /// Whether its operations go to the home's runner service (a test's scripted runner runs them
    /// in this process).
    service: bool,
}

impl OpenedRun {
    /// The run of lab `id` in `dir`, whose Terraform module `project` is `module`, on a runner
    /// set up with `config`, its operations through the home's runner service when `service` (in
    /// this process for a test's).
    pub(super) fn new(config: RunnerConfig, dir: &Path, id: &str, (project, module): (PathBuf, Module), service: bool) -> Result<Self> {
        let name = RunName::new(&crate::runtime::docker::project(id)).map_err(errors::from_runner)?;
        let home = config.home.clone();
        Ok(OpenedRun { runner: Runner::new(config).map_err(errors::from_runner)?, home, dir: dir.to_path_buf(), module, project, name, service })
    }

    /// `act` on the run with `opts`, its events as `log`'s lines: through the home's runner
    /// service, else in this process (none reachable, or a test's runner).
    pub(super) async fn act(&self, act: remote::Act<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
        self.run(act, opts, log).await.map(drop).map_err(remote::Failure::into_error)
    }

    /// [`Self::act`], saying whether it reached its goal (false: a command it ran failed).
    pub(super) async fn run(&self, act: remote::Act<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> std::result::Result<bool, remote::Failure> {
        if self.service
            && let Some(client) = remote::client(&self.home).await
        {
            let on = remote::Served { home: &self.home, client: &client, target: remote::target(&self.project, Format::Terraform, &self.name) };
            return Box::pin(remote::operate(on, act, opts, log)).await;
        }
        let lab = self.running().map_err(remote::Failure::Launcher)?;
        Box::pin(remote::in_process(&lab, act, opts, log)).await
    }

    /// The run's status, read by the home's runner service (else by the runner in this process).
    pub(super) async fn read(&self) -> Result<isothrum_runner::Status> {
        self.read_with(&RunOptions::new()).await
    }

    /// [`Self::read`], its tools given `opts`' values and secrets (a Proxmox server's API token,
    /// for its VMs' states on the server).
    pub(super) async fn read_with(&self, opts: &RunOptions) -> Result<isothrum_runner::Status> {
        if self.service
            && let Some(client) = remote::client(&self.home).await
        {
            let target = remote::target(&self.project, Format::Terraform, &self.name);
            return remote::status(&self.home, &client, &self.name, target, opts).await.map_err(remote::Failure::into_error);
        }
        self.running()?.status_with(opts).await.map_err(errors::from_runner)
    }

    /// Whether the module was generated (a start that failed before generation made none).
    pub(crate) fn has_project(&self) -> bool {
        std::fs::read_dir(&self.project).is_ok_and(|mut entries| entries.any(|e| e.is_ok_and(|e| e.path().extension().is_some_and(|x| x == "tf"))))
    }

    /// The Isoloom target the run's files are generated for; none for a module of the lab's own
    /// and for a run an earlier version left in a helper VM.
    pub(crate) fn target(&self) -> Option<lab::Target> {
        match self.module {
            Module::Generated(target) => Some(target),
            Module::Own | Module::HelperVm => None,
        }
    }

    /// What the run's module is.
    pub(crate) fn module(&self) -> Module {
        self.module
    }

    /// The lab's module, opened under the run's name.
    pub(super) fn running(&self) -> Result<Running<'_>> {
        self.runner.open_as(&self.project, Format::Terraform, Some(self.name.clone())).map_err(|e| match self.module {
            Module::Generated(_) | Module::Own => errors::from_runner(e),
            // A helper VM an earlier version left on a server: said what to remove if not opened.
            Module::HelperVm => errors::of_helper_vm(e, self.name.as_str()),
        })
    }

    /// The run's status as the runner reports it; None before the lab's files exist.
    pub(crate) async fn run_status(&self) -> Result<Option<isothrum_runner::Status>> {
        if !self.has_project() {
            return Ok(None);
        }
        self.read().await.map(Some)
    }

    /// Brings the running lab up to date with its regenerated files, with `opts` (what changing
    /// the run needs: the place's connection and credentials).
    pub(crate) async fn update_with(&self, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
        self.act(remote::Act::Update, opts, log).await
    }

    /// Takes `machine` out of the running lab ahead of its removal from the lab's files, with
    /// `opts`, as [`OpenedRun::update_with`].
    pub(crate) async fn retire_with(&self, machine: &str, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
        self.act(remote::Act::Retire(machine), opts, log).await
    }

    /// The module's sign-in key for the machines it makes (Isoloom's Terraform modules ask for
    /// it, with no default): the runner's public key, made when there is none, as
    /// `ssh_public_key`, and its private key's file as `ssh_private_key_file`.
    pub(crate) async fn key_values(&self) -> Result<Vec<(String, String)>> {
        let public = self.runner.ssh_public_key().await.map_err(errors::from_runner)?;
        Ok(vec![("ssh_public_key".to_owned(), public.trim().to_owned()), ("ssh_private_key_file".to_owned(), self.key().to_string_lossy().into_owned())])
    }

    /// The runner's private key (the launcher's own): what the machines and a jump node let in.
    pub(crate) fn key(&self) -> PathBuf {
        self.home.join("ssh").join("id_ed25519")
    }

    /// The command that opens a console into `machine`, any machine of the lab, as account `user`
    /// when given: SSH with the runner's key (the launcher's own), as the module's `ssh_user`, to
    /// the machine's address through the module's router (its `ip` output) as a jump host, and
    /// through the server's node first when only it reaches the lab ([`Reach`], as the run
    /// recorded it). The runner's own console signs in as `user`, who doesn't let the runner's key
    /// in; this one signs in as the module's user, then becomes `user`.
    pub(crate) async fn console_command(&self, machine: &str, user: Option<&str>) -> Result<isothrum_runner::ShellCommand> {
        let status = self.read().await?;
        let output = |name: &str| status.outputs.iter().find(|o| o.name == name).and_then(|o| o.revealed()).map(str::to_owned);
        let (Some(router), Some(login)) = (output("ip"), output("ssh_user")) else {
            return Err(Error::Invalid("This lab isn't up: its router's address isn't known yet.".into()));
        };
        let address = status.machines.iter().find(|m| m.name == machine).and_then(|m| m.address.clone());
        let Some(address) = address else { return Err(Error::Invalid(format!("{machine} has no address yet: start the lab first."))) };
        // Made when there is none: the key the module's machines let in.
        self.runner.ssh_public_key().await.map_err(errors::from_runner)?;
        let way = Way { node: self.reach()?.node().map(str::to_owned), router, login, address };
        Ok(ssh_through(&self.home, &way, user.map(checked_user).transpose()?))
    }

    /// How the player reaches the run, as its last start or update recorded it.
    pub(crate) fn reach(&self) -> Result<Reach> {
        let record = self.runner.run_record(&self.name).map_err(errors::from_runner)?;
        Ok(Reach::of_record(record.as_ref()))
    }

    /// `machine`'s graphical console (its screen), as the runner reaches it with `opts` (the
    /// place's credentials: a Proxmox server's API, as the module's own values carry them).
    pub(crate) async fn graphical_console(&self, machine: &str, opts: &RunOptions) -> Result<isothrum_runner::Console> {
        self.running()?.graphical_console(machine, opts).await.map_err(errors::from_runner)
    }

    /// What opens running VM `machine` in its hypervisor's own app (a server's web console), as
    /// the runner says with `opts`.
    pub(crate) async fn native_console(&self, machine: &str, opts: &RunOptions) -> Result<isothrum_runner::NativeConsole> {
        self.running()?.native_console(machine, opts).await.map_err(errors::from_runner)
    }
}

/// `user`, when it can name an account: letters, digits, `_`, `-` and `.`, not starting with `-`.
fn checked_user(user: &str) -> Result<&str> {
    let fits = !user.is_empty() && !user.starts_with('-') && user.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if fits { Ok(user) } else { Err(Error::Invalid(format!("`{user}` can't name an account"))) }
}

/// Where a console goes: the machine's `address`, signed in as `login`, through the module's
/// `router` (as `login` too) when the machine is behind it, and through the server's `node` first
/// when only it reaches the lab.
struct Way {
    node: Option<String>,
    router: String,
    login: String,
    address: String,
}

/// SSH along `way` with the key of the runner whose home is `home`: each hop an `ssh -W` in the
/// `ProxyCommand` of the next; straight to the machine when it is the router's address itself (a
/// cloud's one public VM, the Docker host). As account `user` when given (`sudo -iu`). The lab's
/// host keys aren't kept across runs (its machines are made anew at each start); the node's are
/// the ones the player confirmed for it ([`ssh::node_known_hosts`]), checked strictly.
fn ssh_through(home: &Path, way: &Way, user: Option<&str>) -> isothrum_runner::ShellCommand {
    let key = home.join("ssh").join("id_ed25519");
    let lab_known = home.join("ssh").join("lab_known_hosts");
    let options = |known: &Path, strict: &str| {
        [
            "-i".to_owned(),
            key.display().to_string(),
            "-o".to_owned(),
            "IdentitiesOnly=yes".to_owned(),
            "-o".to_owned(),
            format!("StrictHostKeyChecking={strict}"),
            "-o".to_owned(),
            format!("UserKnownHostsFile={}", ssh::ssh_config_path(&known.to_string_lossy())),
        ]
    };
    // Each hop's own ssh takes the same key (`-J` alone would use the default one), reached
    // through the one before it; `ssh` expands `%` in a ProxyCommand once per level.
    let hop = |options: &[String], before: Option<String>, to: &str| {
        let mut words: Vec<String> = std::iter::once("ssh".to_owned()).chain(options.iter().cloned()).collect();
        if let Some(before) = before {
            words.extend(["-o".to_owned(), format!("ProxyCommand={before}")]);
        }
        let line = words.iter().map(|w| ssh::command_word(w)).collect::<Vec<_>>().join(" ");
        format!("{} -W %h:%p {to}", line.replace('%', "%%"))
    };
    let node = way.node.as_ref().map(|node| hop(&options(&ssh::node_known_hosts(&key), "yes"), None, node));
    let lab_options = options(&lab_known, "no");
    let proxy = if way.address == way.router { node } else { Some(hop(&lab_options, node, &format!("{}@{}", way.login, way.router))) };
    let mut args: Vec<String> = lab_options.into();
    if let Some(proxy) = proxy {
        args.extend(["-o".to_owned(), format!("ProxyCommand={proxy}")]);
    }
    if user.is_some() {
        args.push("-t".to_owned());
    }
    args.push(format!("{}@{}", way.login, way.address));
    if let Some(user) = user {
        args.extend(["sudo".to_owned(), "-iu".to_owned(), user.to_owned()]);
    }
    isothrum_runner::ShellCommand::new("ssh", args, home)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn way(node: Option<&str>, address: &str) -> Way {
        Way { node: node.map(str::to_owned), router: "192.0.2.10".into(), login: "isoloom".into(), address: address.into() }
    }

    #[test]
    fn a_modules_machine_is_reached_through_its_router_with_the_runners_key() {
        let home = Path::new("/data/isothrum");
        let cmd = ssh_through(home, &way(None, "10.40.0.11"), Some("player"));
        assert_eq!(cmd.program, "ssh");
        // The key and known hosts are the home's files, in this OS's separators.
        let key = home.join("ssh").join("id_ed25519").display().to_string();
        let known = ssh::ssh_config_path(&home.join("ssh").join("lab_known_hosts").display().to_string());
        let line = cmd.args.join(" ");
        assert!(line.contains(&format!("-i {key}")) && line.contains(&format!("UserKnownHostsFile={known}")), "{line}");
        assert!(line.contains(&format!("ProxyCommand=ssh -i {}", ssh::command_word(&key))) && line.contains("-W %h:%p isoloom@192.0.2.10"), "{line}");
        assert!(line.ends_with("-t isoloom@10.40.0.11 sudo -iu player"), "{line}");
        assert!(checked_user("-oProxyCommand=x").is_err());
        // The machine at the router's address itself: reached straight, no jump to itself.
        let direct = ssh_through(home, &way(None, "192.0.2.10"), None).args.join(" ");
        assert!(!direct.contains("ProxyCommand") && direct.ends_with("isoloom@192.0.2.10"), "{direct}");
    }

    #[test]
    fn a_lab_only_its_node_reaches_is_reached_through_the_node_first_its_host_key_the_confirmed_one() {
        let home = Path::new("/data/isothrum");
        let cmd = ssh_through(home, &way(Some("root@node.lab"), "10.40.0.11"), Some("player"));
        let proxy = cmd.args.iter().find_map(|a| a.strip_prefix("ProxyCommand=")).expect("a ProxyCommand").to_owned();
        assert!(proxy.ends_with("-W %h:%p isoloom@192.0.2.10"), "the machine through the router: {proxy}");
        assert!(proxy.contains("-W %%h:%%p root@node.lab"), "the router through the node, a level down: {proxy}");
        let nodes = ssh::ssh_config_path(&ssh::node_known_hosts(&home.join("ssh").join("id_ed25519")).display().to_string());
        assert!(proxy.contains(&format!("UserKnownHostsFile={nodes}")), "the node's confirmed keys: {proxy}");
        assert!(proxy.contains("StrictHostKeyChecking=yes"), "{proxy}");
        assert!(cmd.args.join(" ").ends_with("-t isoloom@10.40.0.11 sudo -iu player"));
        // The router itself: through the node alone.
        let router = ssh_through(home, &way(Some("root@node.lab"), "192.0.2.10"), None);
        let proxy = router.args.iter().find_map(|a| a.strip_prefix("ProxyCommand=")).expect("a ProxyCommand");
        assert!(proxy.ends_with("-W %h:%p root@node.lab") && !proxy.contains("isoloom@"), "{proxy}");
    }
}
