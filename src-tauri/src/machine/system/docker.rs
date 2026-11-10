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

/// A Docker-compatible engine, as the webview names it (its id) and as the player reads it (its
/// `label`). The one list of engines: the webview's is generated from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "kebab-case")]
#[ts(rename = "DockerEngine")]
pub enum Engine {
    DockerDesktop,
    Orbstack,
    Colima,
    RancherDesktop,
    Podman,
    /// Docker Engine, the native daemon on Linux.
    #[serde(rename = "docker-engine")]
    Native,
}

impl Engine {
    /// Every engine, in the launcher's order.
    #[cfg(test)]
    pub const ALL: [Engine; 6] = [Engine::DockerDesktop, Engine::Orbstack, Engine::Colima, Engine::RancherDesktop, Engine::Podman, Engine::Native];

    /// Its id, as the webview and a launch request name it.
    pub fn id(self) -> &'static str {
        match self {
            Engine::DockerDesktop => "docker-desktop",
            Engine::Orbstack => "orbstack",
            Engine::Colima => "colima",
            Engine::RancherDesktop => "rancher-desktop",
            Engine::Podman => "podman",
            Engine::Native => "docker-engine",
        }
    }

    /// Its name, as the player reads it (only the webview names engines).
    #[cfg(test)]
    pub fn label(self) -> &'static str {
        match self {
            Engine::DockerDesktop => "Docker Desktop",
            Engine::Orbstack => "OrbStack",
            Engine::Colima => "Colima",
            Engine::RancherDesktop => "Rancher Desktop",
            Engine::Podman => "Podman",
            Engine::Native => "Docker Engine",
        }
    }
}

/// `docker info`'s operating system line: the daemon answers (for this account).
pub(super) async fn daemon_os() -> Result<String> {
    run_read("docker", &["info", "--format", "{{.OperatingSystem}}"], None).await
}

/// Name the running engine from the daemon's OS string and the active CLI context.
pub(super) async fn engine_in_use(os: &str) -> Engine {
    let context = run_read("docker", &["context", "show"], None).await.unwrap_or_default();
    engine_named(os, &context)
}

/// The engine a daemon's OS string and the CLI's current context point to.
fn engine_named(os: &str, context: &str) -> Engine {
    let (os, context) = (os.to_lowercase(), context.to_lowercase());
    if os.contains("orbstack") || context.contains("orbstack") {
        Engine::Orbstack
    } else if context.contains("colima") {
        Engine::Colima
    } else if os.contains("rancher") || context.contains("rancher") {
        Engine::RancherDesktop
    } else if os.contains("podman") || context.contains("podman") {
        Engine::Podman
    } else if os.contains("docker desktop") || context.starts_with("desktop-") {
        Engine::DockerDesktop
    } else {
        Engine::Native
    }
}

/// The engine behind a Docker CLI context, by the names the engines give their contexts.
/// `default` only counts on Linux, where it is the native daemon; elsewhere its socket is a
/// link to whichever desktop app installed it, so it says nothing on its own.
fn context_engine(name: &str, os: &str) -> Option<Engine> {
    let n = name.to_lowercase();
    if n == "orbstack" {
        Some(Engine::Orbstack)
    } else if n.starts_with("desktop-") {
        Some(Engine::DockerDesktop)
    } else if n == "colima" || n.starts_with("colima-") {
        Some(Engine::Colima)
    } else if n == "rancher-desktop" {
        Some(Engine::RancherDesktop)
    } else if n.starts_with("podman") {
        Some(Engine::Podman)
    } else if n == "default" && os == "linux" {
        Some(Engine::Native)
    } else {
        None
    }
}

/// One Docker-compatible engine found on this machine: the CLI context that reaches it, whether
/// its daemon answers, and whether it is the CLI's current context (the one labs run on).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub engine: Engine,
    pub context: String,
    pub running: bool,
    pub current: bool,
}

/// The Docker CLI contexts: each name, and whether it is the current one.
async fn contexts() -> Vec<(String, bool)> {
    let out = run_read("docker", &["context", "ls", "--format", "{{.Name}}\t{{.Current}}"], None).await.unwrap_or_default();
    contexts_from(&out)
}

/// `docker context ls` lines: the name, then whether it is current (`true`, or a trailing `*`
/// as the table view marks it).
fn contexts_from(listing: &str) -> Vec<(String, bool)> {
    listing
        .lines()
        .filter_map(|l| {
            let mut words = l.split_whitespace();
            let name = words.next()?.to_string();
            let current = words.any(|w| w == "true" || w == "*");
            Some((name, current))
        })
        .collect()
}

/// The Docker CLI contexts with an engine behind them (unknown contexts are left out).
async fn engine_contexts() -> Vec<(String, Engine)> {
    let os = std::env::consts::OS;
    contexts().await.into_iter().filter_map(|(name, _)| context_engine(&name, os).map(|e| (name, e))).collect()
}

/// Every engine this machine has, one per context that names one or answers: probed one by one
/// with `docker --context <c> info` (a stopped engine fails fast; the timeout covers one that
/// hangs). A context that answers names its engine from the daemon's OS too, so `default` on a
/// Mac reads as the desktop app behind it; one that doesn't is known by its name alone. Two
/// contexts to one engine keep the first (the current one first of all).
pub(super) async fn engines() -> Vec<EngineStatus> {
    let os = std::env::consts::OS;
    let mut found: Vec<EngineStatus> = Vec::new();
    let mut listed = contexts().await;
    listed.sort_by_key(|(_, current)| !current);
    for (name, current) in listed {
        let args = ["--context", name.as_str(), "info", "--format", "{{.OperatingSystem}}"];
        let answer = match tokio::time::timeout(ENGINE_PROBE_TIMEOUT, run_read("docker", &args, None)).await {
            Ok(Ok(daemon_os)) => Some(daemon_os),
            _ => None,
        };
        let engine = match &answer {
            Some(daemon_os) => engine_named(daemon_os.trim(), &name),
            None => match context_engine(&name, os) {
                Some(engine) => engine,
                None => continue,
            },
        };
        if !found.iter().any(|f| f.engine == engine) {
            found.push(EngineStatus { engine, context: name, running: answer.is_some(), current });
        }
    }
    found
}

/// The engines whose daemon answers right now.
pub(super) fn running_of(engines: &[EngineStatus]) -> Vec<Engine> {
    engines.iter().filter(|e| e.running).map(|e| e.engine).collect()
}

/// The Docker context behind `engine` on this machine, for pinning a lab to it (`DOCKER_CONTEXT` /
/// `docker --context <name>`); an error when no context on this machine names that engine.
pub async fn engine_context(engine: &str) -> Result<String> {
    let found = engine_contexts().await.into_iter().find(|(_, e)| e.id() == engine).map(|(name, _)| name);
    found.ok_or_else(|| Error::Invalid(format!("no Docker context for {engine}")))
}

/// Points the Docker CLI at another running engine (`docker context use`), as the player
/// would in a terminal. Labs then run on it.
pub async fn docker_use_engine(engine: String) -> Result<()> {
    let name = engine_context(&engine).await?;
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
        assert_eq!(engine_named("OrbStack", "default").id(), "orbstack");
        assert_eq!(engine_named("Ubuntu 24.04", "colima").id(), "colima");
        assert_eq!(engine_named("Rancher Desktop WSL Distribution", "default").id(), "rancher-desktop");
        assert_eq!(engine_named("Docker Desktop", "default").id(), "docker-desktop");
        assert_eq!(engine_named("Debian", "desktop-linux").id(), "docker-desktop");
        assert_eq!(engine_named("Ubuntu 24.04 LTS", "default").id(), "docker-engine");
    }

    #[test]
    fn contexts_are_read_with_the_current_one_marked_by_true_or_a_star() {
        assert_eq!(
            contexts_from("default\ttrue\ndesktop-linux\tfalse\norbstack\tfalse\n"),
            [("default".to_string(), true), ("desktop-linux".into(), false), ("orbstack".into(), false)]
        );
        assert_eq!(contexts_from("default *\ncolima-dev\n\n"), [("default".to_string(), true), ("colima-dev".into(), false)]);
        assert_eq!(contexts_from(""), [] as [(String, bool); 0]);
    }

    #[test]
    fn context_names_map_to_engines_and_default_only_counts_on_linux() {
        assert_eq!(context_engine("desktop-linux", "macos").map(Engine::id), Some("docker-desktop"));
        assert_eq!(context_engine("colima-dev", "macos").map(Engine::id), Some("colima"));
        assert_eq!(context_engine("my-remote", "macos").map(Engine::id), None);
        assert_eq!(context_engine("default", "linux").map(Engine::id), Some("docker-engine"));
        assert_eq!(context_engine("podman-machine-default", "macos").map(Engine::id), Some("podman"));
        assert_eq!(context_engine("rancher-desktop", "windows").map(Engine::id), Some("rancher-desktop"));
    }

    /// Several engines side by side: each context is asked with `docker --context <c> info`, so
    /// each engine has its own state, and the current context is marked; one that doesn't
    /// answer is still listed from its name, stopped; a context that names no engine and
    /// doesn't answer is left out.
    #[tokio::test]
    async fn every_engine_is_listed_with_its_own_state_and_the_current_one_marked() {
        let tools = Tools::new(|program, args| match (program, args) {
            ("docker", ["context", "ls", ..]) => ok("desktop-linux\tfalse\norbstack\ttrue\ncolima\tfalse\nmy-remote\tfalse\n"),
            ("docker", ["--context", "orbstack", "info", ..]) => ok("OrbStack\n"),
            ("docker", ["--context", "desktop-linux", "info", ..]) => ok("Docker Desktop\n"),
            ("docker", ["--context", ctx, "info", ..]) => fail(format!("Cannot connect to the Docker daemon for {ctx}")),
            _ => ok(""),
        });
        let found = engines().await;
        let brief: Vec<(&str, &str, bool, bool)> = found.iter().map(|e| (e.engine.id(), e.context.as_str(), e.running, e.current)).collect();
        // The current context first, every context probed once.
        assert_eq!(brief, [("orbstack", "orbstack", true, true), ("docker-desktop", "desktop-linux", true, false), ("colima", "colima", false, false)]);
        assert_eq!(running_of(&found), [Engine::Orbstack, Engine::DockerDesktop]);
        assert_eq!(tools.calls().iter().filter(|c| c.contains(" info ")).count(), 4, "{:?}", tools.calls());
    }

    /// A context that says nothing by its name (`default` on a Mac) is known by the daemon it
    /// reaches; two contexts to one engine count once.
    #[tokio::test]
    async fn a_context_without_a_telling_name_is_known_by_its_daemon() {
        let _tools = Tools::new(|program, args| match (program, args) {
            ("docker", ["context", "ls", ..]) => ok("default\ttrue\ndesktop-linux\tfalse\n"),
            ("docker", ["--context", _, "info", ..]) => ok("Docker Desktop\n"),
            _ => ok(""),
        });
        let found = engines().await;
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].engine.id(), found[0].context.as_str(), found[0].running, found[0].current), ("docker-desktop", "default", true, true));
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
    async fn an_engines_context_is_resolved_for_pinning_a_lab_to_it() {
        let _tools = Tools::rules(&[("context ls", ok("default *\norbstack\ncolima-dev\n"))]);
        assert_eq!(engine_context("colima").await.unwrap(), "colima-dev");
        assert_eq!(engine_context("orbstack").await.unwrap(), "orbstack");
        assert_eq!(engine_context("podman").await.unwrap_err().to_string(), "no Docker context for podman");
    }

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
        assert_eq!(engine_named("podman", "default").id(), "podman");
        assert_eq!(engine_named("Ubuntu", "rancher-desktop").id(), "rancher-desktop");
        assert_eq!(context_engine("colima", "macos").map(Engine::id), Some("colima"));
        assert_eq!(context_engine("default", "macos").map(Engine::id), None);
    }
}
