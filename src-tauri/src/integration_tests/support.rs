//! What the real-lab tests share: a scenario's run (its checks, then its teardown whatever happens
//! in them: [`scenario`]), a lab installed for a test and run through the runner, the test's log,
//! its checks (each failure collected, so the lab is always stopped after), what a lab leaves on
//! Docker, a probe run through the console the lab page opens, and the attack box driven as the
//! lab page drives it.

use std::path::{Path, PathBuf};

use isothrum_runner::ShellCommand;

use crate::error::Result;
use crate::platform::AppDirs;
use crate::platform::exec::run;
use crate::platform::unwind::{CatchUnwind, panic_message};
use crate::runtime::attack_machine::{self, Placed};
use crate::runtime::{LabStatus, docker, isothrum, launch, lifecycle};

/// How a test says it returned because a knob it needs isn't set: `tests/integration/run.sh`
/// records the scenario as skipped on this line, where cargo counts the early return as a pass.
pub(crate) const SKIPPED: &str = "integration test skipped, not set:";

/// Knob `name`, the test's input from the environment; None when it is unset or blank, said on
/// the test's output ([`SKIPPED`]) so the test returning on it isn't taken for a pass.
pub(crate) fn knob(name: &str) -> Option<String> {
    let value = std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    if value.is_none() {
        println!("{SKIPPED} {name}");
    }
    value
}

/// What a scenario keeps its failed checks in: a list of them, or a state of its own holding one.
pub(crate) trait Checks {
    fn failures(&mut self) -> &mut Vec<String>;
}

impl Checks for Vec<String> {
    fn failures(&mut self) -> &mut Vec<String> {
        self
    }
}

/// Runs a scenario's `body` (the lab started, its checks), then its `teardown` (the lab stopped,
/// what it left checked) whatever happens in the body: a panic there is a failed check of its own,
/// and the teardown still runs. A lab a scenario leaves behind fails the ones after it (a UTM VM
/// still registered makes every later docker-vm import fail). A panic in the teardown is a failed
/// check too. Each part's failures go to `state`.
pub(crate) async fn scenario<S: Checks>(state: &mut S, body: impl AsyncFnOnce(&mut S), teardown: impl AsyncFnOnce(&mut S)) {
    if let Err(panic) = (CatchUnwind(Box::pin(body(state)))).await {
        check(false, format!("the scenario ran to its end (it panicked: {})", panic_message(&*panic)), state.failures());
    }
    if let Err(panic) = (CatchUnwind(Box::pin(teardown(state)))).await {
        check(false, format!("the teardown ran to its end (it panicked: {})", panic_message(&*panic)), state.failures());
    }
}

/// `url` fetched on this machine (15 s at most): its status, or why it couldn't be.
pub(crate) async fn fetch(url: &str) -> std::result::Result<reqwest::StatusCode, String> {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().map_err(|e| e.to_string())?;
    client.get(url).send().await.map(|r| r.status()).map_err(|e| e.to_string())
}

/// One lab installed for a test, run through the Isothrum runner.
pub(crate) struct Run {
    pub(crate) dir: PathBuf,
    pub(crate) id: String,
    /// The runner's home (its runs under `runs/`).
    pub(crate) home: PathBuf,
}

impl Run {
    /// Starts the lab with the launch's values `env`.
    pub(crate) async fn start(&self, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
        launch::start_docker_run(&self.home, &self.dir, &self.id, launch::Inputs::values(env), log).await
    }

    pub(crate) async fn status(&self) -> Result<LabStatus> {
        isothrum::LabRun::docker(&self.home, &self.dir, &self.id)?.status().await
    }

    pub(crate) async fn check(&self) -> Result<docker::Check> {
        isothrum::LabRun::docker(&self.home, &self.dir, &self.id)?.check().await
    }

    /// Parks the lab (its containers stopped, kept), as the lab page's Shut down does.
    pub(crate) async fn park(&self, log: &mut impl FnMut(String)) -> Result<()> {
        isothrum::LabRun::docker(&self.home, &self.dir, &self.id)?.park(crate::runtime::Park::Shutdown, log).await
    }

    /// Starts the parked lab again, as it was.
    pub(crate) async fn resume(&self, log: &mut impl FnMut(String)) -> Result<()> {
        isothrum::LabRun::docker(&self.home, &self.dir, &self.id)?.resume(log).await
    }

    pub(crate) async fn stop(&self, log: impl FnMut(String)) -> Result<()> {
        lifecycle::stop_run(isothrum::LabRun::docker(&self.home, &self.dir, &self.id)?, &self.dir, log).await
    }
}

/// A fresh copy of the lab, as the launcher installs one: its own folder, nothing generated.
/// Copied in Rust, not with `cp -R`, so the test runs on Windows too (R7).
pub(crate) fn install_copy(src: &Path, id: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cyberctf-{id}"));
    let _ = std::fs::remove_dir_all(&dir);
    crate::runtime::lab::copy_tree(src, &dir).unwrap_or_else(|e| panic!("copy {} to {}: {e}", src.display(), dir.display()));
    dir
}

pub(crate) fn log(id: &str) -> impl FnMut(String) + use<> {
    let tag = id.to_string();
    move |l: String| println!("[{tag}] {l}")
}

async fn docker_lines(args: &[&str]) -> Vec<String> {
    run("docker", args, None).await.unwrap_or_default().lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// Everything of the lab Docker still holds: the project's containers (one-off `run` containers
/// and the attack box included: they carry the project label too) and networks.
pub(crate) async fn leftovers(id: &str) -> Vec<String> {
    let label = format!("label=com.docker.compose.project={}", docker::project(id));
    let mut left = Vec::new();
    for (what, args) in [
        ("container", vec!["ps", "-a", "--filter", label.as_str(), "--format", "{{.Names}}"]),
        ("network", vec!["network", "ls", "--filter", label.as_str(), "--format", "{{.Name}}"]),
    ] {
        left.extend(docker_lines(&args).await.into_iter().map(|n| format!("{what} {n}")));
    }
    left
}

/// The launch's values for a real VM lab: `CYBERCTF_TEST_VM_INPUTS` (`NAME=value,…`).
pub(crate) fn vm_inputs() -> Vec<(String, String)> {
    std::env::var("CYBERCTF_TEST_VM_INPUTS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|p| p.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned())))
        .collect()
}

/// `probe` run in the box, through the console the lab page opens, without a terminal. A console
/// of a shape this doesn't know is an error.
pub(crate) async fn in_box(console: &ShellCommand, probe: &str) -> crate::error::Result<String> {
    let args = probe_args(console, probe).ok_or_else(|| crate::error::Error::Invalid(format!("an unexpected console: {console:?}")))?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let program = match console.program.as_str() {
        "docker" => "docker",
        "ssh" => "ssh",
        _ => "vagrant",
    };
    crate::platform::exec::run_env_timed(program, &args, Some(console.cwd.as_path()), &console.env, std::time::Duration::from_secs(120)).await
}

/// `console`'s arguments running `probe` without a terminal, as the account the console opens as;
/// none for a console of another shape.
fn probe_args(console: &ShellCommand, probe: &str) -> Option<Vec<String>> {
    let mut args = console.args.clone();
    match (console.program.as_str(), args.len()) {
        // `vagrant ssh <vm> -c <a login as an account>`, `ssh -t … <a login as an account>` (`if
        // <the login is the account>; then <its shell>; else exec sudo -iu <account>; fi`): no
        // TTY, the probe in place of each shell it opens, the account's switch kept.
        ("vagrant" | "ssh", _) if args.last().is_some_and(|a| a.contains("; else exec sudo -iu ")) => {
            args.retain(|a| a != "-t");
            let login = args.last_mut()?;
            let (test, rest) = login.split_once("; then ")?;
            let account = rest.split_once("; else exec sudo -iu ")?.1.strip_suffix("; fi")?;
            *login = format!("{test}; then sh -c '{probe}'; else exec sudo -iu {account} sh -c '{probe}'; fi");
        }
        // `docker compose … exec <service> sh -c <shell>` (or `… exec <service> bash`): no TTY,
        // the probe for the shell.
        ("docker", _) => {
            let exec = args.iter().position(|a| a == "exec")?;
            args.insert(exec + 1, "-T".into());
            if args.len() >= 2 && args[args.len() - 2] == "-c" {
                *args.last_mut()? = probe.into();
            } else {
                *args.last_mut()? = "sh".into();
                args.extend(["-c".to_owned(), probe.to_owned()]);
            }
        }
        // `vagrant ssh <vm>`: the probe as its command.
        ("vagrant", 2) => args.extend(["-c".to_owned(), probe.to_owned()]),
        // `ssh -t … <login shell>` into a VM: no TTY, the probe for the shell.
        ("ssh", _) if !args.last().is_some_and(|a| a.contains("docker exec")) => {
            args.retain(|a| a != "-t");
            *args.last_mut()? = probe.into();
        }
        // `vagrant ssh <vm> -c <into the container>`, `ssh -t … <into the container>`: no TTY
        // into it, the probe for its shell.
        ("vagrant" | "ssh", _) => {
            args.retain(|a| a != "-t");
            let into = args.last_mut()?;
            let shell = into.find("sh -c '")?;
            *into = format!("{} sh -c '{probe}'", into[..shell].replace(" -it ", " -i "));
        }
        _ => return None,
    }
    Some(args)
}

pub(crate) fn check(ok: bool, what: String, failures: &mut Vec<String>) {
    if ok {
        println!("ok: {what}");
    } else {
        println!("FAILED: {what}");
        failures.push(what);
    }
}

/// Adds the attack box to the running lab `placed`, from `image` beside containers, as the lab
/// page's Start attack box does.
pub(crate) async fn add_box(placed: &Placed, dirs: &AppDirs, image: &str, l: &mut impl FnMut(String), f: &mut Vec<String>) {
    let added = placed.add(dirs, crate::runtime::attack_machine::Pick { image, desktop: false }, None, l).await;
    check(added.is_ok(), format!("the attack box is added to the lab ({:?})", added.err()), f);
}

/// The box runs, with an address, and a shell in it reaches `target`'s port 80, is called by the
/// box's own name in the lab ([`attack_machine::name`]: the seat the lab declares, else
/// [`attack_machine::NAME`]) and is the player's ([`box_is_the_players`]); `when` says at which
/// point of the test.
pub(crate) async fn box_reaches(placed: &Placed, target: &str, when: &str, f: &mut Vec<String>) {
    match placed.status().await {
        Ok(s) => check(s.running && !s.ip.is_empty(), format!("the attack box runs, with an address, {when} ({} on {})", s.ip, s.lab_network), f),
        Err(err) => check(false, format!("the attack box's status reads, {when}: {err}"), f),
    }
    let name = match attack_machine::name(&placed.dir) {
        Ok(name) => name,
        Err(err) => return check(false, format!("the attack box's name reads, {when}: {err}"), f),
    };
    // On a failure it says why (the connection's error, whether the name has an address, the
    // routes): a bare "not reached" left a libvirt run's failure undiagnosable. The connection is
    // made in a subshell, as a failed `exec` redirection ends the shell it runs in.
    let probe = format!(
        "bash -c \"(exec 3<>/dev/tcp/{target}/80) 2>&1 && echo reach-ok || {{ echo reach-failed; getent hosts {target} || echo {target} has no address; ip -4 route; }}; hostname\""
    );
    let reached = match placed.shell_command().await {
        Ok(console) => in_box(&console, &probe).await,
        Err(err) => Err(err),
    };
    check(
        reached.as_deref().is_ok_and(|o| o.lines().any(|l| l.trim() == "reach-ok") && o.contains(&name)),
        format!("a shell in the attack box reaches {target}:80 and is called {name}, {when} ({reached:?})"),
        f,
    );
    box_is_the_players(placed, when, f).await;
}

/// A shell in the box `placed` is the player's ([`attack_machine::PLAYER`]): its account, in its
/// home, with sudo and no password; `when` says at which point of the test.
pub(crate) async fn box_is_the_players(placed: &Placed, when: &str, f: &mut Vec<String>) {
    let player = attack_machine::PLAYER;
    let seen = match placed.shell_command().await {
        Ok(console) => in_box(&console, "whoami && cd && pwd && sudo -n true && echo sudo-ok").await,
        Err(err) => Err(err),
    };
    // `vagrant ssh -c` (a VM box) opens a terminal, whose output starts with an empty line.
    let lines: Vec<String> = seen.as_deref().unwrap_or_default().lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect();
    check(
        lines.first().is_some_and(|l| l == player) && lines.contains(&format!("/home/{player}")) && lines.iter().any(|l| l == "sudo-ok"),
        format!("a shell in the attack box is {player}'s (whoami), in its home, with sudo and no password, {when} ({seen:?})"),
        f,
    );
}

/// Takes the box out, the lab left up, and checks it is gone.
pub(crate) async fn remove_box(placed: &Placed, dirs: &AppDirs, l: &mut impl FnMut(String), f: &mut Vec<String>) {
    let removed = placed.remove(dirs, l).await;
    check(removed.is_ok(), format!("the attack box is taken out ({:?})", removed.err()), f);
    match placed.status().await {
        Ok(s) => check(!s.running, "the attack box is gone".into(), f),
        Err(err) => check(false, format!("the attack box's status reads: {err}"), f),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_knob_is_none_and_said_as_the_runner_reads_it() {
        assert_eq!(knob("CYBERCTF_TEST_NEVER_SET_BY_ANYONE"), None);
        // tests/integration/run.sh records a scenario as skipped on this line.
        assert!(include_str!("../../../tests/integration/run.sh").contains(&format!("KNOB_UNSET=\"{SKIPPED}\"")));
    }

    /// A probe runs as the account the box's console opens as: the console's own switch to it is
    /// kept (`exec --user`, the `sudo -i` login), only the shell it opens is the probe.
    #[test]
    fn a_probe_runs_as_the_account_the_console_opens_as() {
        let login = "if [ $(id -un) = player ]; then if command -v bash >/dev/null 2>&1; then exec bash -l; else exec sh -l; fi; else exec sudo -iu player; fi";
        let vagrant = ShellCommand::new("vagrant", ["ssh", "attackbox", "-c", login], "/run");
        assert_eq!(
            probe_args(&vagrant, "whoami").unwrap(),
            ["ssh", "attackbox", "-c", "if [ $(id -un) = player ]; then sh -c 'whoami'; else exec sudo -iu player sh -c 'whoami'; fi"]
        );
        let ssh = ShellCommand::new("ssh", ["-t", "-F", "ssh_config", "admin@198.51.100.10", login], "/run");
        assert_eq!(
            probe_args(&ssh, "whoami").unwrap().last().unwrap(),
            "if [ $(id -un) = player ]; then sh -c 'whoami'; else exec sudo -iu player sh -c 'whoami'; fi"
        );
        let docker = ShellCommand::new("docker", ["compose", "exec", "--user", "player", "attackbox", "sh", "-c", "cd 2>/dev/null; exec bash -l"], "/run");
        assert_eq!(probe_args(&docker, "whoami").unwrap(), ["compose", "exec", "-T", "--user", "player", "attackbox", "sh", "-c", "whoami"]);
    }

    #[tokio::test]
    async fn a_scenario_that_panics_is_still_torn_down_and_says_where_it_stopped() {
        // Regression: a scenario's `expect` panicked before its teardown, its UTM VM stayed
        // registered, and every docker-vm scenario after it failed.
        let mut failures = Vec::new();
        let torn_down = std::cell::Cell::new(false);
        scenario(
            &mut failures,
            async |f: &mut Vec<String>| {
                check(true, "the lab starts".into(), f);
                tokio::task::yield_now().await;
                panic!("a console into the box: no such machine");
            },
            async |f: &mut Vec<String>| {
                torn_down.set(true);
                check(false, "nothing of it is left".into(), f);
            },
        )
        .await;
        assert!(torn_down.get(), "the teardown ran");
        assert_eq!(failures, ["the scenario ran to its end (it panicked: a console into the box: no such machine)", "nothing of it is left"]);
        // A body that ends, and a teardown that panics: said too.
        let mut failures = Vec::new();
        scenario(&mut failures, async |_: &mut Vec<String>| {}, async |_: &mut Vec<String>| panic!("{}", String::from("stop failed"))).await;
        assert_eq!(failures, ["the teardown ran to its end (it panicked: stop failed)"]);
    }
}
