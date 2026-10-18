//! Which Docker-compatible engines this machine has, which one the CLI uses, and switching or
//! starting one the way the player would.

use std::time::Duration;

use crate::error::{Error, Result};
use crate::platform::exec::{docker_denied, docker_denied_error, run, run_read};

/// How long each engine's context may take to answer before it counts as stopped.
const ENGINE_PROBE_TIMEOUT: Duration = Duration::from_secs(4);
/// How long a started engine is waited for: 90 tries, 2 s apart.
const START_TRIES: u32 = 90;
const START_POLL: Duration = Duration::from_secs(2);

/// `docker info`'s operating system line: the daemon answers (for this account).
pub(super) async fn daemon_os() -> Result<String> {
    run_read("docker", &["info", "--format", "{{.OperatingSystem}}"], None).await
}

/// Name the running engine from the daemon's OS string and the active CLI context.
pub(super) async fn engine_in_use(os: &str) -> &'static str {
    let context = run_read("docker", &["context", "show"], None).await.unwrap_or_default();
    engine_named(os, &context)
}

/// The engine a daemon's OS string and the CLI's current context point to.
fn engine_named(os: &str, context: &str) -> &'static str {
    let (os, context) = (os.to_lowercase(), context.to_lowercase());
    if os.contains("orbstack") || context.contains("orbstack") {
        "orbstack"
    } else if context.contains("colima") {
        "colima"
    } else if os.contains("rancher") || context.contains("rancher") {
        "rancher-desktop"
    } else if os.contains("podman") || context.contains("podman") {
        "podman"
    } else if os.contains("docker desktop") || context.starts_with("desktop-") {
        "docker-desktop"
    } else {
        "docker-engine"
    }
}

/// The engine behind a Docker CLI context, by the names the engines give their contexts.
/// `default` only counts on Linux, where it is the native daemon; elsewhere its socket is a
/// link to whichever desktop app installed it, so it says nothing on its own.
fn context_engine(name: &str, os: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    if n == "orbstack" {
        Some("orbstack")
    } else if n.starts_with("desktop-") {
        Some("docker-desktop")
    } else if n == "colima" || n.starts_with("colima-") {
        Some("colima")
    } else if n == "rancher-desktop" {
        Some("rancher-desktop")
    } else if n.starts_with("podman") {
        Some("podman")
    } else if n == "default" && os == "linux" {
        Some("docker-engine")
    } else {
        None
    }
}

/// The Docker CLI contexts, each with the engine behind it (unknown contexts are left out).
async fn engine_contexts() -> Vec<(String, &'static str)> {
    let out = run_read("docker", &["context", "ls", "--format", "{{.Name}}"], None).await.unwrap_or_default();
    contexts_from(&out, std::env::consts::OS)
}

/// `docker context ls` names (the current one marked ` *`), each with its engine.
fn contexts_from(listing: &str, os: &str) -> Vec<(String, &'static str)> {
    listing.lines().map(|l| l.trim().trim_end_matches(" *").to_string()).filter_map(|name| context_engine(&name, os).map(|e| (name, e))).collect()
}

/// Engines whose context answers, probed one by one (a stopped engine fails fast; the
/// timeout covers one that hangs).
pub(super) async fn running_engines() -> Vec<&'static str> {
    let mut running = Vec::new();
    for (name, engine) in engine_contexts().await {
        let args = ["--context", name.as_str(), "version", "--format", "{{.Server.Version}}"];
        let probe = run("docker", &args, None);
        if matches!(tokio::time::timeout(ENGINE_PROBE_TIMEOUT, probe).await, Ok(Ok(_))) && !running.contains(&engine) {
            running.push(engine);
        }
    }
    running
}

/// Points the Docker CLI at another running engine (`docker context use`), as the player
/// would in a terminal. Labs then run on it.
pub async fn docker_use_engine(engine: String) -> Result<()> {
    let name = engine_contexts()
        .await
        .into_iter()
        .find(|(_, e)| *e == engine)
        .map(|(name, _)| name)
        .ok_or_else(|| Error::Invalid(format!("no Docker context for {engine}")))?;
    run("docker", &["context", "use", &name], None).await?;
    Ok(())
}

/// Starts an installed engine that isn't running, the way the player would (open the app or run
/// its CLI), points Docker at it and waits until it answers. Docker Desktop and OrbStack take a
/// while on first start, hence the generous wait.
pub async fn docker_start_engine(engine: String) -> Result<()> {
    // Running already, but not for this account: starting it again can't help.
    if let Err(e) = daemon_os().await
        && docker_denied(&e)
    {
        return Err(docker_denied_error());
    }
    launch(&engine).await?;
    for _ in 0..START_TRIES {
        // The engine's context appears once it is up; point Docker at it, then ask the daemon.
        let _ = docker_use_engine(engine.clone()).await;
        match daemon_os().await {
            Ok(_) => return Ok(()),
            // Up now, but not for this account: say so instead of waiting out the 3 minutes.
            Err(e) if docker_denied(&e) => return Err(docker_denied_error()),
            Err(_) => {}
        }
        tokio::time::sleep(START_POLL).await;
    }
    Err(Error::Invalid(format!("{engine} didn't answer within 3 minutes. Check its window, then re-check.")))
}

/// Opens the engine's app, or runs its CLI, on this OS.
async fn launch(engine: &str) -> Result<String> {
    match (std::env::consts::OS, engine) {
        ("macos", "docker-desktop") => run("open", &["-a", "Docker"], None).await,
        ("macos", "orbstack") => run("open", &["-a", "OrbStack"], None).await,
        ("windows", "docker-desktop") => run("cmd", &["/C", "start", "", r"C:\Program Files\Docker\Docker\Docker Desktop.exe"], None).await,
        ("linux", "docker-engine") => run("pkexec", &["systemctl", "start", "docker"], None).await.map_err(|e| match e {
            Error::ToolMissing { .. } => Error::Invalid(crate::machine::tools::steps::NO_PKEXEC.into()),
            e => e,
        }),
        (_, "colima") => run("colima", &["start"], None).await,
        _ => Err(Error::Invalid(format!("Cyber CTF can't start {engine} here; start it yourself, then re-check."))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_engine_from_the_daemon_and_the_context() {
        assert_eq!(engine_named("OrbStack", "default"), "orbstack");
        assert_eq!(engine_named("Ubuntu 24.04", "colima"), "colima");
        assert_eq!(engine_named("Rancher Desktop WSL Distribution", "default"), "rancher-desktop");
        assert_eq!(engine_named("Docker Desktop", "default"), "docker-desktop");
        assert_eq!(engine_named("Debian", "desktop-linux"), "docker-desktop");
        assert_eq!(engine_named("Ubuntu 24.04 LTS", "default"), "docker-engine");
    }

    #[test]
    fn contexts_map_to_engines_and_default_only_counts_on_linux() {
        let listing = "default *\ndesktop-linux\norbstack\ncolima-dev\nmy-remote\n";
        assert_eq!(
            contexts_from(listing, "macos"),
            [("desktop-linux".to_string(), "docker-desktop"), ("orbstack".into(), "orbstack"), ("colima-dev".into(), "colima")]
        );
        assert_eq!(contexts_from("default *\n", "linux"), [("default".to_string(), "docker-engine")]);
        assert_eq!(context_engine("podman-machine-default", "macos"), Some("podman"));
        assert_eq!(context_engine("rancher-desktop", "windows"), Some("rancher-desktop"));
    }

    #[tokio::test]
    async fn an_engine_the_launcher_cannot_start_is_refused_without_running_anything() {
        let err = launch("some-engine").await.unwrap_err();
        assert!(err.to_string().contains("can't start some-engine here"));
    }

    use std::cell::Cell;
    use std::rc::Rc;

    use crate::platform::exec::fake::{Reply, Tools, fail, ok};

    const DENIED: &str = "permission denied while trying to connect to the Docker daemon socket at unix:///var/run/docker.sock";
    const DOWN: &str = "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?";

    #[tokio::test]
    async fn the_cli_is_pointed_at_the_context_of_the_engine_asked_for() {
        let tools = Tools::rules(&[("context ls", ok("default *\norbstack\ncolima-dev\n"))]);
        docker_use_engine("colima".into()).await.unwrap();
        assert!(tools.called("docker context use colima-dev"), "{:?}", tools.calls());
        let err = docker_use_engine("podman".into()).await.unwrap_err();
        assert_eq!(err.to_string(), "no Docker context for podman");
        assert_eq!(tools.calls().iter().filter(|c| c.contains("context use")).count(), 1);
    }

    /// Docker with colima installed: the daemon answers `info` (as `daemon`, once colima
    /// started) and colima's context shows up once it runs.
    fn colima(daemon: impl Fn() -> Option<Reply> + 'static) -> (Tools, Rc<Cell<bool>>) {
        let up = Rc::new(Cell::new(false));
        let started = up.clone();
        let tools = Tools::new(move |program, args| match (program, args.first().copied()) {
            ("colima", Some("start")) => {
                started.set(true);
                ok("")
            }
            ("docker", Some("info")) if !started.get() => fail(DOWN),
            ("docker", Some("info")) => daemon(),
            ("docker", Some("context")) if args.get(1) == Some(&"ls") => ok(if started.get() { "default *\ncolima\n" } else { "default *\n" }),
            _ => ok(""),
        });
        (tools, up)
    }

    #[tokio::test]
    async fn an_engine_is_started_docker_pointed_at_it_and_its_daemon_waited_for() {
        let (tools, up) = colima(|| ok("Ubuntu 24.04 LTS"));
        docker_start_engine("colima".into()).await.unwrap();
        assert!(up.get());
        assert!(tools.called("docker context use colima"), "{:?}", tools.calls());
    }

    #[tokio::test]
    async fn a_daemon_this_account_cannot_use_is_reported_without_waiting() {
        // Running already, for another account: not started again.
        let tools = Tools::rules(&[("docker info", fail(DENIED))]);
        let err = docker_start_engine("colima".into()).await.unwrap_err();
        assert!(matches!(err.code(), crate::error::Code::DockerDenied { .. } | crate::error::Code::DockerDeniedWindows { .. }), "{err}");
        assert!(!tools.called("colima start"));
        drop(tools);
        // Up once started, but denied: said at once, not after 3 minutes.
        let (_tools, up) = colima(|| fail(DENIED));
        let err = docker_start_engine("colima".into()).await.unwrap_err();
        assert!(up.get());
        assert!(matches!(err.code(), crate::error::Code::DockerDenied { .. } | crate::error::Code::DockerDeniedWindows { .. }), "{err}");
    }

    #[tokio::test]
    async fn an_engine_that_cannot_be_started_here_is_refused() {
        let tools = Tools::rules(&[("docker info", fail(DOWN))]);
        let err = docker_start_engine("some-engine".into()).await.unwrap_err();
        assert!(err.to_string().contains("can't start some-engine here"), "{err}");
        assert_eq!(tools.calls(), ["docker info --format {{.OperatingSystem}}"]);
    }

    #[test]
    fn more_engines_by_name() {
        assert_eq!(engine_named("podman", "default"), "podman");
        assert_eq!(engine_named("Ubuntu", "rancher-desktop"), "rancher-desktop");
        assert_eq!(context_engine("colima", "macos"), Some("colima"));
        assert_eq!(context_engine("default", "macos"), None);
    }
}
