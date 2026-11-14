//! A lab run the launcher opens from its generated Terraform module under its own name: a
//! server's (Proxmox) or a cloud account's. Both are changed (brought up to date, a machine
//! retired), read and consoled into the same way.

use std::path::{Path, PathBuf};

use isothrum_runner::{Format, RunName, RunOptions, Runner, RunnerConfig, Running};

use super::{errors, remote};
use crate::error::{Error, Result};
use crate::runtime::lab;

/// A lab's run on the runner, opened by its name from the module Isoloom generated for it.
pub(crate) struct OpenedRun {
    pub(super) runner: Runner,
    /// The runner's home: where its SSH key is (`ssh/id_ed25519`), the launcher's own.
    home: PathBuf,
    /// The lab's folder.
    pub(super) dir: PathBuf,
    /// The Isoloom target the run's files are generated for; none for a helper VM an earlier
    /// version left on a Proxmox server, whose files Isoloom no longer generates.
    target: Option<lab::Target>,
    /// The run's Terraform module.
    pub(super) project: PathBuf,
    pub(super) name: RunName,
    /// Whether its operations go to the home's runner service (a test's scripted runner runs them
    /// in this process).
    service: bool,
}

impl OpenedRun {
    /// The run named `name` of the lab in `dir`, whose Terraform module `project` is generated
    /// for `target` (none: a helper VM's, see [`OpenedRun::target`]), on a runner set up with
    /// `config`, its operations through the home's runner service when `service` (in this process
    /// for a test's).
    pub(super) fn new(config: RunnerConfig, dir: &Path, target: Option<lab::Target>, project: PathBuf, name: RunName, service: bool) -> Result<Self> {
        let home = config.home.clone();
        Ok(OpenedRun { runner: Runner::new(config).map_err(errors::from_runner)?, home, dir: dir.to_path_buf(), target, project, name, service })
    }

    /// `act` on the run with `opts`, its events as `log`'s lines: through the home's runner
    /// service, else in this process (none reachable, or a test's runner).
    pub(super) async fn act(&self, act: remote::Act<'_>, opts: &RunOptions, log: &mut impl FnMut(String)) -> Result<()> {
        if self.service
            && let Some(client) = remote::client(&self.home).await
        {
            let target = remote::target(&self.project, Format::Terraform, &self.name);
            return Box::pin(remote::operate(&self.home, &client, act, target, opts, log)).await.map(drop).map_err(remote::Failure::into_error);
        }
        let lab = self.running().await?;
        Box::pin(remote::in_process(&lab, act, opts, log)).await.map(drop).map_err(remote::Failure::into_error)
    }

    /// The run's status, read by the home's runner service (else by the runner in this process).
    pub(super) async fn read(&self) -> Result<isothrum_runner::Status> {
        if self.service
            && let Some(client) = remote::client(&self.home).await
        {
            let target = remote::target(&self.project, Format::Terraform, &self.name);
            return remote::status(&self.home, &client, &self.name, target).await.map_err(remote::Failure::into_error);
        }
        self.running().await?.status().await.map_err(errors::from_runner)
    }

    /// Whether the module was generated (a start that failed before generation made none).
    pub(crate) fn has_project(&self) -> bool {
        std::fs::read_dir(&self.project).is_ok_and(|mut entries| entries.any(|e| e.is_ok_and(|e| e.path().extension().is_some_and(|x| x == "tf"))))
    }

    /// The Isoloom target the run's files are generated for; none for a run an earlier version
    /// left in a helper VM, which is only read, stopped and removed.
    pub(crate) fn target(&self) -> Option<lab::Target> {
        self.target
    }

    /// The lab's module, opened under the run's name.
    pub(super) async fn running(&self) -> Result<Running<'_>> {
        self.runner.open_as(&self.project, Format::Terraform, Some(self.name.clone())).map_err(|e| match self.target {
            Some(_) => errors::from_runner(e),
            // A helper VM an earlier version left on a server: said what to remove if not opened.
            None => errors::of_helper_vm(e, self.name.as_str()),
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

    /// The command that opens a console into `machine`, any machine of the lab, as account `user`
    /// when given: SSH with the runner's key (the launcher's own), as the module's `ssh_user`, to
    /// the machine's address through the module's router (its `ip` output) as a jump host. The
    /// runner gives a Terraform module's machines no console command of its own.
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
        Ok(ssh_through(&self.home, &router, &login, &address, user.map(checked_user).transpose()?))
    }

    /// `machine`'s graphical console (its screen), as the runner reaches it with `opts` (the
    /// place's credentials: a Proxmox server's API, as the module's own values carry them).
    pub(crate) async fn graphical_console(&self, machine: &str, opts: &RunOptions) -> Result<isothrum_runner::Console> {
        self.running().await?.graphical_console(machine, opts).await.map_err(errors::from_runner)
    }

    /// What opens running VM `machine` in its hypervisor's own app (a server's web console), as
    /// the runner says with `opts`.
    pub(crate) async fn native_console(&self, machine: &str, opts: &RunOptions) -> Result<isothrum_runner::NativeConsole> {
        self.running().await?.native_console(machine, opts).await.map_err(errors::from_runner)
    }
}

/// `user`, when it can name an account: letters, digits, `_`, `-` and `.`, not starting with `-`.
fn checked_user(user: &str) -> Result<&str> {
    let fits = !user.is_empty() && !user.starts_with('-') && user.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if fits { Ok(user) } else { Err(Error::Invalid(format!("`{user}` can't name an account"))) }
}

/// SSH into `address` as `login` with the key of the runner whose home is `home`: through the jump
/// host `router` (as `login` too) when the machine is behind it, straight to it when it is the
/// router's address itself (a cloud's one public VM, the Docker host); as account `user` when
/// given (`sudo -iu`). Host keys aren't kept across runs: a lab's machines are made anew at each
/// start.
fn ssh_through(home: &Path, router: &str, login: &str, address: &str, user: Option<&str>) -> isothrum_runner::ShellCommand {
    let key = home.join("ssh").join("id_ed25519");
    let known = home.join("ssh").join("lab_known_hosts");
    let options = |key: &Path, known: &Path| {
        [
            "-i".to_owned(),
            key.display().to_string(),
            "-o".to_owned(),
            "IdentitiesOnly=yes".to_owned(),
            "-o".to_owned(),
            "StrictHostKeyChecking=no".to_owned(),
            "-o".to_owned(),
            format!("UserKnownHostsFile={}", known.display()),
        ]
    };
    // The jump's own ssh takes the same key (`-J` alone would use the default one).
    let jump = format!("ssh {} -W %h:%p {login}@{router}", options(&key, &known).map(|o| quoted(&o)).join(" "));
    let mut args: Vec<String> = options(&key, &known).into();
    if address != router {
        args.extend(["-o".to_owned(), format!("ProxyCommand={jump}")]);
    }
    if user.is_some() {
        args.push("-t".to_owned());
    }
    args.push(format!("{login}@{address}"));
    if let Some(user) = user {
        args.extend(["sudo".to_owned(), "-iu".to_owned(), user.to_owned()]);
    }
    isothrum_runner::ShellCommand::new("ssh", args, home)
}

/// `word` as one word of the jump's command line (double-quoted when it holds a space).
fn quoted(word: &str) -> String {
    if word.contains([' ', '"']) { format!("\"{}\"", word.replace('"', "\\\"")) } else { word.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_modules_machine_is_reached_through_its_router_with_the_runners_key() {
        let cmd = ssh_through(Path::new("/data/isothrum"), "192.0.2.10", "isoloom", "10.40.0.11", Some("player"));
        assert_eq!(cmd.program, "ssh");
        let line = cmd.args.join(" ");
        assert!(line.contains("-i /data/isothrum/ssh/id_ed25519"), "{line}");
        assert!(line.contains("ProxyCommand=ssh -i /data/isothrum/ssh/id_ed25519") && line.contains("-W %h:%p isoloom@192.0.2.10"), "{line}");
        assert!(line.ends_with("-t isoloom@10.40.0.11 sudo -iu player"), "{line}");
        assert!(checked_user("-oProxyCommand=x").is_err());
        // The machine at the router's address itself: reached straight, no jump to itself.
        let direct = ssh_through(Path::new("/data/isothrum"), "192.0.2.10", "isoloom", "192.0.2.10", None).args.join(" ");
        assert!(!direct.contains("ProxyCommand") && direct.ends_with("isoloom@192.0.2.10"), "{direct}");
    }
}
