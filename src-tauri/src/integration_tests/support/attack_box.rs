//! The attack box driven as the lab page drives it: added to a running lab, reached from, its
//! shell checked to be the player's, taken out; and a probe run in it through the console the lab
//! page opens.

use isothrum_runner::ShellCommand;

use super::check;
use crate::platform::AppDirs;
use crate::runtime::attack_machine::{self, Placed};

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

/// `probe` run in the box as [`in_box`] runs it, `input` on its standard input (never on a command
/// line); its output, whether it ran or not.
async fn in_box_with_input(console: &ShellCommand, probe: &str, input: &str) -> crate::error::Result<String> {
    use tokio::io::AsyncWriteExt as _;
    let args = probe_args(console, probe).ok_or_else(|| crate::error::Error::Invalid(format!("an unexpected console: {console:?}")))?;
    let mut cmd = tokio::process::Command::new(&console.program);
    cmd.args(&args).current_dir(&console.cwd).envs(console.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).await?;
    }
    let out = tokio::time::timeout(std::time::Duration::from_secs(120), child.wait_with_output())
        .await
        .map_err(|_| crate::error::Error::Invalid("the probe took over 2 minutes".into()))??;
    Ok(format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

/// What checks the password in the box: `su` to the player's account, in a terminal of its own
/// (su reads a password from one), given the first line of its standard input when it asks; then
/// whether it signed in. Python's, as Kali and the attack image have it.
const SU_CHECK: &str = r#"import os, pty, signal, sys
signal.alarm(60)
password = sys.stdin.readline().strip()
pid, fd = pty.fork()
if pid == 0:
    os.execvp("su", ["su", sys.argv[1], "-c", "echo su-signed-in"])
out, asked = b"", False
while True:
    try:
        data = os.read(fd, 1024)
    except OSError:
        break
    if not data:
        break
    out += data
    if not asked and b"assword" in out:
        os.write(fd, (password + "\n").encode())
        asked = True
os.waitpid(pid, 0)
print("su-asked" if asked else "su-did-not-ask")
print("su-ok" if b"su-signed-in" in out else "su-refused")
"#;

/// The password the launcher keeps for the box `placed` ([`attack_machine::sign_in`]) is its
/// player account's: `su` to the account takes it (given on standard input, never on a command
/// line), and refuses another; `when` says at which point of the test. The probe's output is never
/// in a failure's message: it may echo the password.
pub(crate) async fn password_works(placed: &Placed, when: &str, f: &mut Vec<String>) {
    let sign_in = match attack_machine::sign_in(&placed.dir, None) {
        Ok(Some(sign_in)) => sign_in,
        found => return check(false, format!("the launcher keeps a password for the box, {when} ({:?})", found.err()), f),
    };
    check(sign_in.user == attack_machine::PLAYER, format!("the box's sign-in is {}'s, {when}", attack_machine::PLAYER), f);
    let console = match placed.shell_command().await {
        Ok(console) => console,
        Err(e) => return check(false, format!("a console into the box opens, {when}: {e}"), f),
    };
    let script = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, SU_CHECK);
    let probe = format!("printf %s {script} | base64 -d > /tmp/cyberctf-su-check.py && python3 /tmp/cyberctf-su-check.py {}", sign_in.user);
    let said = |out: &crate::error::Result<String>, line: &str| out.as_deref().is_ok_and(|o| o.lines().any(|l| l.trim() == line));
    let right = in_box_with_input(&console, &probe, &format!("{}\n", sign_in.password)).await;
    check(
        said(&right, "su-asked") && said(&right, "su-ok"),
        format!("su to {} takes the password the launcher keeps, {when} (asked: {}, ran: {})", sign_in.user, said(&right, "su-asked"), right.is_ok()),
        f,
    );
    let wrong = in_box_with_input(&console, &probe, "not-the-password\n").await;
    check(said(&wrong, "su-refused"), format!("su to {} refuses another password, {when} (ran: {})", sign_in.user, wrong.is_ok()), f);
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
}
