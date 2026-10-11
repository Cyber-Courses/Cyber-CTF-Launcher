//! The attack box: the learner's offensive container. It runs on its OWN network
//! (the "attack network") and is then connected into the lab's network, so the
//! attacker sits on a distinct segment yet can reach the targets. One per lab.

use serde::Serialize;

use crate::error::{Error, Result};
use crate::exec::{run, run_read, stream};

pub fn container(id: &str) -> String {
    format!("cyberctf-{id}-attacker")
}

/// Guards an image reference so it can't be read as a flag or smuggle extra args.
/// (It's passed to docker without a shell, so this only blocks a leading `-` and
/// anything outside a normal `registry/name:tag@digest`.)
pub fn valid_image(image: &str) -> bool {
    !image.is_empty()
        && image.len() <= 200
        && !image.starts_with('-')
        && image.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | ':' | '@'))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExegolStatus {
    /// The image is pulled locally (otherwise the first start downloads it, several GB).
    pub image_present: bool,
    pub running: bool,
    /// The attack box's address on the lab network (what the targets see), once running.
    pub ip: String,
    /// The lab network it is plugged into (the lab's name for it, e.g. "default"), once running.
    pub lab_network: String,
    /// The command that attaches a shell (shown so the player can also run it anywhere).
    pub shell_cmd: String,
}

/// The lab's Docker networks (Compose labels them with the project name), the main one
/// first: `<project>_default` when the lab has it, so the attack box's reported address
/// is the one most targets see.
async fn lab_networks(id: &str) -> Vec<String> {
    let filter = format!("label=com.docker.compose.project=cyberctf-{id}");
    let out = run_read("docker", &["network", "ls", "--filter", &filter, "--format", "{{.Name}}"], None).await.unwrap_or_default();
    main_first(out.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
}

fn main_first(mut networks: Vec<String>) -> Vec<String> {
    networks.sort();
    if let Some(i) = networks.iter().position(|n| n.ends_with("_default")) {
        let main = networks.remove(i);
        networks.insert(0, main);
    }
    networks
}

/// The main lab network.
async fn lab_network(id: &str) -> Option<String> {
    lab_networks(id).await.into_iter().next()
}

pub async fn status(id: &str, image: &str) -> ExegolStatus {
    let image_present = run_read("docker", &["image", "inspect", image], None).await.is_ok();
    let name = container(id);
    // Report the attacker's address on the lab network: the one the targets see it from.
    let lab_net = lab_network(id).await.unwrap_or_default();
    let tmpl = format!("{{{{.State.Running}}}}\t{{{{with index .NetworkSettings.Networks \"{lab_net}\"}}}}{{{{.IPAddress}}}}{{{{end}}}}");
    let probe = run_read("docker", &["inspect", "-f", &tmpl, &name], None).await;
    let (running, ip) = match probe {
        Ok(out) => parse_probe(&out),
        Err(_) => (false, String::new()),
    };
    let lab_network = if running && !lab_net.is_empty() { super::docker::short_network(id, &lab_net) } else { String::new() };
    ExegolStatus { image_present, running, ip, lab_network, shell_cmd: format!("docker exec -it {name} bash") }
}

/// `<running>\t<ip>` from the status probe: whether the box runs, and its lab address.
fn parse_probe(out: &str) -> (bool, String) {
    let line = out.lines().next().unwrap_or_default();
    let (r, rest) = line.split_once('\t').unwrap_or(("false", ""));
    (r.trim() == "true", rest.split_whitespace().next().unwrap_or_default().to_string())
}

pub async fn start(id: &str, image: &str, mut log: impl FnMut(String)) -> Result<()> {
    let name = container(id);
    if run("docker", &["image", "inspect", image], None).await.is_err() {
        log(format!("Pulling {image} — a large image, downloads only once…"));
        stream("docker", &["pull", image], None, &[], &mut log).await?;
    }
    let lab_nets = lab_networks(id).await;
    if lab_nets.is_empty() {
        return Err(Error::Invalid("the lab network isn't up — start the lab first".into()));
    }
    let attack_net = format!("cyberctf-{id}-attack");
    // Own network first, then join the lab network: a distinct attack segment that can
    // still reach the targets (so the attacker is not on the same subnet as the lab).
    let _ = run("docker", &["network", "create", &attack_net], None).await;
    // Clear any previous attack box so a re-launch is clean.
    let _ = run("docker", &["rm", "-f", &name], None).await;
    log(format!("Starting the attack box on {attack_net}…"));
    // --init: `sleep` as PID 1 ignores SIGTERM, so every `docker stop` waited out its timeout.
    let mut args = vec!["run", "-d", "--init", "--name", &name, "--network", &attack_net, "--hostname", "attacker", "--cap-add", "NET_ADMIN"];
    // The folder shared with the attack box (Settings), at /shared inside it.
    let volume = crate::shared_folder::current().map(|p| format!("{}:{}", p.display(), crate::shared_folder::MOUNT_POINT));
    if let Some(v) = &volume {
        log(format!("Sharing {} as {}.", v.rsplit_once(':').map(|(h, _)| h).unwrap_or(v), crate::shared_folder::MOUNT_POINT));
        args.extend(["-v", v.as_str()]);
    }
    args.extend([image, "sleep", "infinity"]);
    if let Err(e) = stream("docker", &args, None, &[], &mut log).await {
        // No box, so no use for its own network either: it would only hold a subnet.
        let _ = run("docker", &["network", "rm", &attack_net], None).await;
        return Err(e);
    }
    // Every lab network, so labs with their own segments (dmz, internal...) are reachable. If a
    // connect fails, tear the half-wired box down so a retry starts clean instead of leaving an
    // attacker that can only reach some of the lab.
    for lab_net in &lab_nets {
        log(format!("Connecting to the lab network {}…", super::docker::short_network(id, lab_net)));
        if let Err(e) = run("docker", &["network", "connect", lab_net, &name], None).await {
            let _ = run("docker", &["rm", "-f", &name], None).await;
            let _ = run("docker", &["network", "rm", &attack_net], None).await;
            return Err(e);
        }
    }
    log("✓ Attack box ready — open a shell to start.".into());
    Ok(())
}

/// Plugs a running attack box into every current lab network (a lab restarted, or updated to
/// a version with other networks, keeps the attack box but not its connections). Best effort.
pub async fn rejoin(id: &str) {
    let name = container(id);
    if !matches!(run_read("docker", &["inspect", "-f", "{{.State.Running}}", &name], None).await, Ok(o) if o.trim() == "true") {
        return;
    }
    for lab_net in lab_networks(id).await {
        // "already exists" when it's plugged in already: fine.
        let _ = run("docker", &["network", "connect", &lab_net, &name], None).await;
    }
}

pub async fn stop(id: &str, mut log: impl FnMut(String)) -> Result<()> {
    log("Removing the attack box…".into());
    let _ = stream("docker", &["rm", "-f", &container(id)], None, &[], &mut log).await;
    let _ = run("docker", &["network", "rm", &format!("cyberctf-{id}-attack")], None).await;
    Ok(())
}

/// `open_terminal` off the async runtime's threads and the UI thread: finding a terminal that
/// starts can take a few seconds (each candidate gets a moment to fail), which froze the window
/// when a command ran it on the main thread.
pub async fn open_terminal_async(command: String) -> Result<()> {
    tokio::task::spawn_blocking(move || open_terminal(&command)).await.map_err(|e| Error::Invalid(format!("couldn't open a terminal: {e}")))?
}

/// The command line attached to the attack box. bash is present on Kali/Parrot/Exegol alike.
pub fn shell_command(id: &str) -> String {
    format!("docker exec -it {} bash", container(id))
}

/// Marks the processes of one in-app shell inside the attack box (inherited by what it runs).
const SESSION_VAR: &str = "CYBERCTF_SHELL";

/// [`shell_command`] for an in-app shell, its processes marked with `tag` so
/// [`end_session`] can find them.
pub fn tagged_shell_command(id: &str, tag: &str) -> String {
    format!("docker exec -it -e {SESSION_VAR}={tag} {} bash", container(id))
}

/// The script that hangs up the processes an in-app shell started in the attack box (none for a
/// tag that isn't one of ours).
fn end_session_script(tag: &str) -> Option<String> {
    if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    Some(format!(
        "for e in /proc/[0-9]*/environ; do tr '\\0' '\\n' < \"$e\" 2>/dev/null | grep -qx '{SESSION_VAR}={tag}' || continue; p=${{e#/proc/}}; kill -HUP \"${{p%/environ}}\" 2>/dev/null; done; true"
    ))
}

/// Hangs up the processes an in-app shell started in the attack box. Docker leaves a
/// `docker exec` process running when its client goes away, so every closed shell window kept
/// an idle bash (and whatever it was running) until the box stopped.
pub async fn end_session(container: &str, tag: &str) {
    if let Some(script) = end_session_script(tag) {
        let _ = crate::exec::run_env_timed("docker", &["exec", container, "sh", "-c", &script], None, &[], std::time::Duration::from_secs(10)).await;
    }
}

/// [`end_session`], blocking: for when the app is quitting and nothing async will run again.
/// Bounded per session: a Docker that doesn't answer must not hold the quit up.
pub fn end_session_blocking(container: &str, tag: &str) {
    if let Some(script) = end_session_script(tag) {
        let mut cmd = std::process::Command::new("docker");
        cmd.args(["exec", container, "sh", "-c", &script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        crate::exec::headless_std(&mut cmd);
        let _ = crate::exec::status_within(&mut cmd, END_SESSION_TIMEOUT);
    }
}

/// How long ending a shell's session may take at quit.
const END_SESSION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Opens the player's own terminal attached to the attack box.
pub fn shell(id: &str) -> Result<()> {
    open_terminal(&shell_command(id))
}

/// Escapes text for an AppleScript double-quoted string.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn applescript_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Opens the OS terminal on `command` (a POSIX shell command line), titled
/// "Cyber CTF attack box". The command line itself never stays on screen: the window is
/// cleared first, and the command replaces the shell (exec), so leaving the box ends the
/// session. DOCKER_CLI_HINTS=false drops Docker's "What's next" ad on exit.
pub fn open_terminal(command: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        // do script types the line into a new window; escape it for the AppleScript string.
        // The leading space keeps it out of shell history where HIST_IGNORE_SPACE is on.
        let line = applescript_string(&format!(" clear; DOCKER_CLI_HINTS=false exec {command}"));
        // Title: ours alone where AppleScript allows it (no size, device or shell path). The working
        // folder and the running command are Terminal profile settings it can't turn off.
        let script = format!(
            "tell application \"Terminal\"\nactivate\nset t to do script \"{line}\"\nset custom title of t to \"Cyber CTF attack box\"\n\
             set title displays custom title of t to true\nset title displays window size of t to false\n\
             set title displays device name of t to false\nset title displays shell path of t to false\n\
             set title displays file name of t to false\nend tell"
        );
        std::process::Command::new("osascript").arg("-e").arg(script).spawn()?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // cmd.exe's quote-stripping with nested quotes and `&` is unreliable (it breaks when the
        // identity/known_hosts path contains a space, e.g. C:\Users\First Last\...), so write the
        // command to a temp .cmd and run that: the ssh line lives in the file with normal
        // double-quoted paths, and the only thing cmd parses is the balanced-quoted .cmd path.
        let command = command.replace('\'', "\"");
        let bat = std::env::temp_dir().join(format!("cyberctf-shell-{}.cmd", std::process::id()));
        std::fs::write(&bat, format!("@echo off\r\ncls\r\n{command}\r\n"))?;
        std::process::Command::new("cmd")
            .raw_arg(format!("/c start \"Cyber CTF attack box\" cmd /c \"{}\"", bat.display()))
            .env("DOCKER_CLI_HINTS", "false")
            .spawn()?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let run = format!("clear; DOCKER_CLI_HINTS=false exec {command}");
        // gnome-terminal's `-e` takes one string, so it gets `--`; the others run what follows `-e`.
        for (term, flag) in [("x-terminal-emulator", "-e"), ("gnome-terminal", "--"), ("konsole", "-e"), ("xterm", "-e")] {
            let Ok(mut child) = std::process::Command::new(term).args([flag, "sh", "-c", &run]).spawn() else { continue };
            // A terminal that can't start (e.g. zutty without its font) dies within a moment
            // without a window, so give it that moment and fall through to the next one.
            std::thread::sleep(std::time::Duration::from_millis(1500));
            match child.try_wait() {
                Ok(Some(status)) if !status.success() => continue,
                _ => {
                    // Reaped when it closes, or each terminal opened stays a zombie while the
                    // app runs (Debian's gnome-terminal wrapper waits for its window).
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                    return Ok(());
                }
            }
        }
        Err(Error::Invalid(format!("couldn't open a terminal; run this yourself: {command}")))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::{SESSION_VAR, end_session_script};
    use super::{applescript_string, main_first};

    #[test]
    fn main_lab_network_comes_first() {
        let nets = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            main_first(nets(&["cyberctf-a_internal", "cyberctf-a_default", "cyberctf-a_dmz"])),
            nets(&["cyberctf-a_default", "cyberctf-a_dmz", "cyberctf-a_internal"])
        );
        assert_eq!(main_first(nets(&["cyberctf-a_lan", "cyberctf-a_dmz"])), nets(&["cyberctf-a_dmz", "cyberctf-a_lan"]));
        assert!(main_first(Vec::new()).is_empty());
    }

    #[test]
    fn applescript_escaping_keeps_shell_quoting_intact() {
        let line = r#"ssh -i '/Users/a b/key' x@h echo "hi" 'it'\''s'"#;
        assert_eq!(applescript_string(line), r#"ssh -i '/Users/a b/key' x@h echo \"hi\" 'it'\\''s'"#);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn ending_a_session_hangs_up_only_its_marked_processes() {
        // The script as it runs in the attack box, against processes here (same /proc).
        let spawn = |tag: &str| std::process::Command::new("sleep").arg("30").env(SESSION_VAR, tag).spawn().unwrap();
        let (mut mine, mut other) = (spawn("4242-7"), spawn("4242-8"));
        let status = std::process::Command::new("sh").args(["-c", &end_session_script("4242-7").unwrap()]).status().unwrap();
        assert!(status.success());
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(mine.try_wait().unwrap().is_some(), "the marked shell was hung up");
        assert!(other.try_wait().unwrap().is_none(), "another shell was left alone");
        let _ = other.kill();
        let _ = other.wait();
        // A tag that could inject shell is refused.
        assert!(end_session_script("1'; rm -rf /; '").is_none() && end_session_script("").is_none());
    }

    #[test]
    fn names_and_shell_commands_follow_the_lab_id() {
        assert_eq!(super::container("web"), "cyberctf-web-attacker");
        assert_eq!(super::shell_command("web"), "docker exec -it cyberctf-web-attacker bash");
        assert_eq!(super::tagged_shell_command("web", "12-3"), "docker exec -it -e CYBERCTF_SHELL=12-3 cyberctf-web-attacker bash");
    }

    #[test]
    fn images_are_plain_references() {
        assert!(super::valid_image("nwodtuhs/exegol:free"));
        assert!(super::valid_image("ghcr.io/org/kali@sha256:abc"));
        assert!(!super::valid_image(""));
        assert!(!super::valid_image("-v"));
        assert!(!super::valid_image("img; rm -rf /"));
        assert!(!super::valid_image(&"a".repeat(201)));
    }

    #[test]
    fn session_scripts_only_for_our_tags() {
        let s = super::end_session_script("42-1").unwrap();
        assert!(s.contains("grep -qx 'CYBERCTF_SHELL=42-1'"), "{s}");
        assert!(super::end_session_script("").is_none());
        assert!(super::end_session_script("a b").is_none());
        assert!(super::end_session_script("x'y").is_none());
    }

    #[tokio::test]
    async fn ending_an_untagged_session_runs_nothing() {
        // An invalid tag never reaches docker, async or blocking.
        super::end_session("cyberctf-x-attacker", "bad tag").await;
        super::end_session_blocking("cyberctf-x-attacker", "");
    }

    #[test]
    fn the_status_probe_reads_running_and_the_address() {
        assert_eq!(super::parse_probe("true\t10.0.0.5\n"), (true, "10.0.0.5".to_string()));
        assert_eq!(super::parse_probe("true\t\n"), (true, String::new()));
        assert_eq!(super::parse_probe("false\t"), (false, String::new()));
        assert_eq!(super::parse_probe(""), (false, String::new()));
        assert_eq!(super::parse_probe("true"), (false, String::new()));
    }
}
