//! The attack box: the learner's offensive container. It runs on its OWN network
//! (the "attack network") and is then connected into the lab's network, so the
//! attacker sits on a distinct segment yet can reach the targets. One per lab.

use serde::Serialize;

use crate::error::{Error, Result};
use crate::exec::{run, run_read, stream};

/// The file naming the default attack box, read by the webview (`src/lib/settings.ts`) too. The
/// attack-box workflow's bump PR rewrites it after each smoke-tested publish.
const DEFAULT_IMAGE_FILE: &str = include_str!("../../attack-box-image.json");

/// The default attack box: the Cyber CTF image pinned by digest (`cyberctf/attack-box@sha256:…`),
/// so every player on one launcher version gets the same, tested box.
pub fn default_image() -> &'static str {
    static IMAGE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    IMAGE.get_or_init(|| {
        let v: serde_json::Value = serde_json::from_str(DEFAULT_IMAGE_FILE).expect("attack-box-image.json is JSON");
        v["image"].as_str().expect("attack-box-image.json names an image").to_string()
    })
}

pub fn container(id: &str) -> String {
    format!("{}-attacker", super::docker::project(id))
}

#[derive(Serialize, ts_rs::TS)]
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

/// The lab's Docker networks, the main one first: `<project>_default` when the lab has it, so
/// the attack box's reported address is the one most targets see.
async fn lab_networks(id: &str) -> Vec<String> {
    main_first(super::docker::lab_network_names(id).await)
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
    let attack_net = super::docker::attack_network(id);
    // Own network first, then join the lab network: a distinct attack segment that can
    // still reach the targets (so the attacker is not on the same subnet as the lab). Inside the
    // lab's own address room, not a block from Docker's default pool, which a later lab's
    // instance could need.
    let mut create = vec!["network".to_string(), "create".to_string()];
    if let Some(subnet) = attack_subnet(id, &lab_nets).await {
        create.extend(["--subnet".to_string(), subnet]);
    }
    create.push(attack_net.clone());
    let _ = run("docker", &create.iter().map(String::as_str).collect::<Vec<_>>(), None).await;
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

/// A free /24 for the attack network in the /16 of the lab's first network (see
/// [`pick_attack_subnet`]); `None` when Docker can't say, and Docker then picks one.
async fn attack_subnet(id: &str, lab_nets: &[String]) -> Option<String> {
    let mut args = vec!["network", "inspect", "-f", "{{range .IPAM.Config}}{{.Subnet}} {{end}}"];
    args.extend(lab_nets.iter().map(String::as_str));
    let lab: Vec<String> = run_read("docker", &args, None).await.ok()?.split_whitespace().filter(|s| !s.contains(':')).map(str::to_string).collect();
    pick_attack_subnet(&lab, &super::docker::subnets_in_use(id).await)
}

/// The highest /24 of the /16 the lab's first network (`lab`, its subnets) is in that overlaps
/// none of the lab's networks nor `in_use` (every other Docker network): next to the lab, out of
/// the default pool, and in use from then on, so a later lab's instance is picked around it.
fn pick_attack_subnet(lab: &[String], in_use: &[String]) -> Option<String> {
    let (first, _) = super::lab::ipv4_range(lab.first()?)?;
    let room = first & 0xFFFF_0000;
    let taken: Vec<(u32, u32)> = lab.iter().chain(in_use).filter_map(|c| super::lab::ipv4_range(c)).collect();
    (0..=255u32)
        .rev()
        .map(|c| room | (c << 8))
        .find(|&lo| taken.iter().all(|&(a, b)| lo + 255 < a || b < lo))
        .map(|lo| format!("{}/24", std::net::Ipv4Addr::from(lo)))
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
    let _ = run("docker", &["network", "rm", &super::docker::attack_network(id)], None).await;
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
        open_terminal_cmd(&command.replace('\'', "\""))
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

/// Opens a console window titled "Cyber CTF attack box" on `line`, a cmd.exe command line.
/// cmd.exe's quote-stripping with nested quotes and `&` is unreliable (it breaks when the
/// identity/known_hosts path contains a space, e.g. C:\Users\First Last\...), so the line goes
/// into a temp .cmd that is run: the line lives in the file with normal double-quoted paths, and
/// the only thing cmd parses is the balanced-quoted .cmd path.
#[cfg(windows)]
pub fn open_terminal_cmd(line: &str) -> Result<()> {
    use std::os::windows::process::CommandExt;
    let bat = write_shell_script(&std::env::temp_dir(), line)?;
    std::process::Command::new("cmd")
        .raw_arg(format!("/c start \"Cyber CTF attack box\" cmd /c \"{}\"", bat.display()))
        .env("DOCKER_CLI_HINTS", "false")
        .spawn()?;
    Ok(())
}

/// The temp scripts the Windows terminals run: `cyberctf-shell-<unique>.cmd`.
#[cfg_attr(not(windows), allow(dead_code))]
const SCRIPT_PREFIX: &str = "cyberctf-shell-";

/// How long a shell's script is kept: cmd has read the line it runs long before.
#[cfg_attr(not(windows), allow(dead_code))]
const SCRIPT_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Writes the script a Windows terminal runs for `line` into `dir`, under a name no other shell
/// uses, and clears the scripts earlier shells left. The name was the app's pid alone, so a
/// second shell opened before the first started overwrote its script (both opened the same box),
/// and none was ever deleted. Now the script deletes itself when its shell ends; one whose window
/// was closed mid-session is cleared by a later call.
#[cfg_attr(not(windows), allow(dead_code))]
fn write_shell_script(dir: &std::path::Path, line: &str) -> Result<std::path::PathBuf> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    clear_old_scripts(dir, SCRIPT_TTL);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("{SCRIPT_PREFIX}{}-{n}-{nanos}.cmd", std::process::id()));
    std::fs::write(&path, shell_script(line))?;
    Ok(path)
}

/// A script running `line`: the screen cleared first, the script removed once it ends. A `%` in
/// a batch file starts a variable, so the line's are doubled to stay literal. The file is UTF-8
/// and cmd.exe reads each line in the console's code page as it reaches it: `chcp 65001` first,
/// or a path with "é" (C:\Users\José) arrives garbled.
#[cfg_attr(not(windows), allow(dead_code))]
fn shell_script(line: &str) -> String {
    format!("@echo off\r\nchcp 65001 >nul\r\ncls\r\n{}\r\n(goto) 2>nul & del \"%~f0\"\r\n", line.replace('%', "%%"))
}

/// Removes the shell scripts in `dir` older than `ttl`.
#[cfg_attr(not(windows), allow(dead_code))]
fn clear_old_scripts(dir: &std::path::Path, ttl: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.starts_with(SCRIPT_PREFIX) && name.ends_with(".cmd")) {
            continue;
        }
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age >= ttl);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::{SESSION_VAR, end_session_script};
    use super::{applescript_string, main_first, pick_attack_subnet};

    #[test]
    fn the_attack_network_takes_a_free_block_beside_the_lab() {
        // Regression: it was created with no subnet, taking a block from Docker's default pool.
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(pick_attack_subnet(&s(&["10.31.0.0/24", "10.31.1.0/24"]), &[]).as_deref(), Some("10.31.255.0/24"));
        // Its top block is taken (by another network): the next one down.
        assert_eq!(pick_attack_subnet(&s(&["10.31.0.0/24"]), &s(&["10.31.255.0/24", "172.17.0.0/16"])).as_deref(), Some("10.31.254.0/24"));
        // A lab filling its whole /16, or no lab subnet known: Docker picks.
        assert_eq!(pick_attack_subnet(&s(&["10.31.0.0/16"]), &[]), None);
        assert_eq!(pick_attack_subnet(&[], &[]), None);
    }

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

    const NETS: &str = "cyberctf-web_internal\ncyberctf-web_default\n";

    #[tokio::test]
    async fn the_attack_box_is_pulled_started_and_wired_to_every_lab_network() {
        use crate::exec::fake::{Tools, fail, ok};
        let tools = Tools::rules(&[("image inspect", fail("No such image")), ("network ls", ok(NETS))]);
        let mut log = Vec::new();
        super::start("web", "nwodtuhs/exegol:free", |l| log.push(l)).await.unwrap();
        assert!(tools.called("docker pull nwodtuhs/exegol:free") && tools.called("docker network create cyberctf-web-attack"));
        assert!(tools.called("--name cyberctf-web-attacker --network cyberctf-web-attack"));
        assert!(tools.called("network connect cyberctf-web_default cyberctf-web-attacker") && tools.called("network connect cyberctf-web_internal"));
        assert_eq!(log.last().map(String::as_str), Some("✓ Attack box ready — open a shell to start."));
        drop(tools);
        // No lab network: refused.
        let _tools = Tools::rules(&[]);
        let err = super::start("web", "nwodtuhs/exegol:free", |_| {}).await.unwrap_err().to_string();
        assert_eq!(err, "the lab network isn't up — start the lab first");
    }

    #[tokio::test]
    async fn a_half_wired_attack_box_is_torn_down() {
        use crate::exec::fake::{Tools, fail, ok};
        let tools = Tools::rules(&[("network ls", ok(NETS)), ("network connect", fail("no such network"))]);
        assert!(super::start("web", "img:1", |_| {}).await.is_err());
        assert!(tools.calls().iter().filter(|c| c.as_str() == "docker rm -f cyberctf-web-attacker").count() == 2, "{:?}", tools.calls());
        assert!(tools.called("docker network rm cyberctf-web-attack"));
        drop(tools);
        let tools = Tools::rules(&[("network ls", ok(NETS)), ("run -d", fail("port is already allocated"))]);
        assert!(super::start("web", "img:1", |_| {}).await.is_err());
        assert!(tools.called("docker network rm cyberctf-web-attack") && !tools.called("network connect"));
    }

    #[tokio::test]
    async fn the_status_reports_the_address_on_the_main_lab_network() {
        use crate::exec::fake::{Tools, ok};
        let _tools = Tools::rules(&[("network ls", ok(NETS)), ("docker inspect -f", ok("true\t10.0.0.9\n"))]);
        let s = super::status("web", "img:1").await;
        assert!(s.image_present && s.running);
        assert_eq!((s.ip.as_str(), s.lab_network.as_str()), ("10.0.0.9", "default"));
        assert_eq!(s.shell_cmd, "docker exec -it cyberctf-web-attacker bash");
    }

    #[tokio::test]
    async fn a_running_box_rejoins_the_lab_networks() {
        use crate::exec::fake::{Tools, ok};
        let tools = Tools::rules(&[("{{.State.Running}}", ok("true\n")), ("network ls", ok(NETS))]);
        super::rejoin("web").await;
        assert!(tools.called("network connect cyberctf-web_default cyberctf-web-attacker"));
        drop(tools);
        let tools = Tools::rules(&[("{{.State.Running}}", ok("false\n"))]);
        super::rejoin("web").await;
        assert!(!tools.called("network connect"));
    }

    #[test]
    fn each_windows_shell_gets_its_own_script_and_old_ones_are_cleared() {
        let dir = std::env::temp_dir().join(format!("cyberctf-scripts-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("other.cmd"), "keep").unwrap();
        // Two shells opened at once (regression): two scripts, each with its own line.
        let a = super::write_shell_script(&dir, "docker exec -it cyberctf-a-attacker bash").unwrap();
        let b = super::write_shell_script(&dir, "docker exec -it cyberctf-b-attacker bash").unwrap();
        assert_ne!(a, b);
        assert!(std::fs::read_to_string(&a).unwrap().contains("cyberctf-a-attacker"));
        assert!(std::fs::read_to_string(&b).unwrap().contains("cyberctf-b-attacker"));
        // Fresh scripts survive a later call; old ones go, and nothing else in the folder does.
        super::clear_old_scripts(&dir, super::SCRIPT_TTL);
        assert!(a.exists() && b.exists());
        super::clear_old_scripts(&dir, std::time::Duration::ZERO);
        assert!(!a.exists() && !b.exists());
        assert!(dir.join("other.cmd").exists());
        super::clear_old_scripts(&dir.join("missing"), std::time::Duration::ZERO);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_shell_script_runs_its_line_literally_then_deletes_itself() {
        let s = super::shell_script(r#"ssh -o "UserKnownHostsFile=C:\a%%b" x"#);
        assert_eq!(s, "@echo off\r\nchcp 65001 >nul\r\ncls\r\nssh -o \"UserKnownHostsFile=C:\\a%%%%b\" x\r\n(goto) 2>nul & del \"%~f0\"\r\n");
    }

    #[test]
    fn names_and_shell_commands_follow_the_lab_id() {
        assert_eq!(super::container("web"), "cyberctf-web-attacker");
        assert_eq!(super::shell_command("web"), "docker exec -it cyberctf-web-attacker bash");
        assert_eq!(super::tagged_shell_command("web", "12-3"), "docker exec -it -e CYBERCTF_SHELL=12-3 cyberctf-web-attacker bash");
    }

    /// `cyberctf/attack-box@sha256:<64 lowercase hex digits>`: a tag (`:latest`, a date) can be
    /// pushed over, a digest can't.
    fn pinned(image: &str) -> bool {
        image.strip_prefix("cyberctf/attack-box@sha256:").is_some_and(|d| d.len() == 64 && d.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()))
    }

    #[test]
    fn the_default_image_is_the_cyber_ctf_box_pinned_by_digest() {
        let image = super::default_image();
        assert!(pinned(image), "the default attack box must be cyberctf/attack-box@sha256:<digest>, not {image}");
        assert!(crate::exec::valid_image(image));
        let file: serde_json::Value = serde_json::from_str(super::DEFAULT_IMAGE_FILE).unwrap();
        let previous = file["previous"].as_array().unwrap();
        assert!(previous.iter().all(|p| p.as_str() != Some(image)), "the default is not also a previous one");
        // Earlier defaults are the digests the bump PRs replaced (the bare tag and `:latest` are
        // known to the webview's migration on their own).
        assert!(previous.iter().all(|p| p.as_str().is_some_and(pinned)), "{previous:?}");
    }

    #[test]
    fn only_a_digest_counts_as_pinned() {
        assert!(pinned(&format!("cyberctf/attack-box@sha256:{}", "0123456789abcdef".repeat(4))));
        for tag in ["cyberctf/attack-box", "cyberctf/attack-box:latest", "cyberctf/attack-box:2026-10-05", "cyberctf/attack-box@sha256:abc"] {
            assert!(!pinned(tag), "{tag}");
        }
        assert!(!pinned(&format!("cyberctf/attack-box@sha256:{}", "0123456789ABCDEF".repeat(4))));
        assert!(!pinned(&format!("other/box@sha256:{}", "0".repeat(64))));
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

    #[tokio::test]
    async fn the_status_reads_odd_inspect_output_without_misreading_it() {
        use crate::exec::fake::{Tools, fail, ok};
        // Two addresses on the line, an extra tab, a second line: the first address of the
        // first line. Docker's "true" is exact: anything else is not running.
        assert_eq!(super::parse_probe("true\t10.0.0.5 10.0.0.6\n"), (true, "10.0.0.5".to_string()));
        assert_eq!(super::parse_probe("true\t\t10.0.0.5"), (true, "10.0.0.5".to_string()));
        assert_eq!(super::parse_probe("true\t10.0.0.5\nfalse\t\n"), (true, "10.0.0.5".to_string()));
        assert!(!super::parse_probe("TRUE\t10.0.0.5").0);
        assert!(!super::parse_probe("<no value>\t<no value>").0);
        // Running but not on the lab network yet (plugged in by hand, or mid-start): no address,
        // the lab network still named.
        let tools = Tools::rules(&[("network ls", ok(NETS)), ("docker inspect -f", ok("true\t\n"))]);
        let s = super::status("web", "img:1").await;
        assert!(s.running && s.ip.is_empty());
        assert_eq!(s.lab_network, "default");
        assert!(tools.called("index .NetworkSettings.Networks \"cyberctf-web_default\""), "{:?}", tools.calls());
        drop(tools);
        // No such container, and the image not pulled: nothing at all.
        let _tools = Tools::rules(&[("network ls", ok(NETS)), ("image inspect", fail("No such image")), ("docker inspect -f", fail("No such object"))]);
        let s = super::status("web", "img:1").await;
        assert!(!s.image_present && !s.running && s.ip.is_empty() && s.lab_network.is_empty());
    }

    #[tokio::test]
    async fn a_pull_that_fails_starts_nothing() {
        use crate::exec::fake::{Tools, fail, ok};
        let tools = Tools::rules(&[("image inspect", fail("No such image")), ("docker pull", fail("manifest unknown")), ("network ls", ok(NETS))]);
        let err = super::start("web", "nwodtuhs/exegol:free", |_| {}).await.unwrap_err().to_string();
        assert!(err.contains("manifest unknown"), "{err}");
        assert!(!tools.called("network create") && !tools.called("run -d") && !tools.called("rm -f"), "{:?}", tools.calls());
    }

    #[tokio::test]
    async fn the_main_lab_network_is_joined_first_however_docker_lists_them() {
        use crate::exec::fake::{Tools, ok};
        // Windows line ends, blank lines, the main network last.
        let listed = "\r\ncyberctf-web_internal\r\n\r\ncyberctf-web_dmz\r\ncyberctf-web_default\r\n";
        let tools = Tools::rules(&[("network ls", ok(listed))]);
        super::start("web", "img:1", |_| {}).await.unwrap();
        let connects: Vec<String> = tools.calls().into_iter().filter(|c| c.starts_with("docker network connect")).collect();
        assert_eq!(
            connects,
            [
                "docker network connect cyberctf-web_default cyberctf-web-attacker",
                "docker network connect cyberctf-web_dmz cyberctf-web-attacker",
                "docker network connect cyberctf-web_internal cyberctf-web-attacker"
            ]
        );
        assert!(!tools.calls().iter().any(|c| c.contains('\r')), "{:?}", tools.calls());
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
