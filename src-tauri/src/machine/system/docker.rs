//! Which Docker-compatible engines this machine has, which one the CLI uses, and switching or
//! starting one the way the player would.

use std::time::Duration;

use crate::error::{Error, Result};
use crate::platform::exec::{docker_denied, docker_denied_error, run, run_read, run_read_env};
use crate::platform::host::Os;

/// How long each engine's context may take to answer before it counts as stopped.
const ENGINE_PROBE_TIMEOUT: Duration = Duration::from_secs(4);
/// How many times a started engine is asked whether it answers: 90, [`START_POLL`] apart.
const START_TRIES: u32 = 90;
/// How long between two of those tries.
const START_POLL: Duration = Duration::from_secs(2);

/// A Docker-compatible engine, as the webview names it (its id) and as the player reads it (its
/// `label`). The one list of engines: the webview's is generated from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "kebab-case")]
#[ts(rename = "DockerEngine")]
pub enum Engine {
    /// Docker Desktop.
    DockerDesktop,
    /// OrbStack.
    Orbstack,
    /// Colima.
    Colima,
    /// Rancher Desktop.
    RancherDesktop,
    /// Podman.
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

    /// Where the player gets it, when the setup doesn't install it.
    pub fn url(self) -> &'static str {
        match self {
            Engine::DockerDesktop => "https://www.docker.com/products/docker-desktop/",
            Engine::Orbstack => "https://orbstack.dev/",
            Engine::Colima => "https://github.com/abiosoft/colima",
            Engine::RancherDesktop => "https://rancherdesktop.io/",
            Engine::Podman => "https://podman.io/",
            Engine::Native => "https://docs.docker.com/engine/install/",
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
    daemon_info("{{.OperatingSystem}}", &[]).await
}

/// `docker info --format <field>` (`{{.ServerVersion}}`, say), with `env` (a `DOCKER_CONTEXT`):
/// the daemon answers (for this account). Docker 26 and older (Debian's `docker.io`) exit 0 from
/// `docker info` when the daemon doesn't answer, saying so on stderr alone with an empty line on
/// stdout: an empty answer is asked again of `docker version`, which fails with the reason (not
/// running, or refused to this account).
pub(crate) async fn daemon_info(field: &str, env: &[(String, String)]) -> Result<String> {
    let out = run_read_env("docker", &["info", "--format", field], None, env).await?;
    if answered(&out).is_some() {
        return Ok(out);
    }
    run_read_env("docker", &["version", "--format", "{{.Server.Os}}"], None, env).await?;
    Err(Error::Invalid(format!("docker info --format {field} answered nothing, though the Docker daemon answers")))
}

/// A daemon's `docker info` answer, when it gave one: Docker 26 and older print an empty line
/// (and exit 0) when the daemon doesn't answer.
fn answered(info: &str) -> Option<&str> {
    Some(info.trim()).filter(|out| !out.is_empty())
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
fn context_engine(name: &str, os: Os) -> Option<Engine> {
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
    } else if n == "default" && os == Os::Linux {
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
    /// Which engine it is (Docker Desktop, Colima, the Linux daemon…).
    pub engine: Engine,
    /// The CLI context that reaches it.
    pub context: String,
    /// Whether its daemon answers.
    pub running: bool,
    /// Whether its context is the CLI's current one, which labs run on.
    pub current: bool,
}

/// The Docker CLI contexts: each name, and whether it is the current one; why not when
/// `docker context ls` fails.
async fn contexts() -> Result<Vec<(String, bool)>> {
    let out = run_read("docker", &["context", "ls", "--format", "{{.Name}}\t{{.Current}}"], None).await?;
    Ok(contexts_from(&out))
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
async fn engine_contexts() -> Result<Vec<(String, Engine)>> {
    let os = Os::HERE;
    Ok(contexts().await?.into_iter().filter_map(|(name, _)| context_engine(&name, os).map(|e| (name, e))).collect())
}

/// Every engine this machine has, one per context that names one or answers: probed one by one
/// with `docker --context <c> info` (a stopped engine fails fast; the timeout covers one that
/// hangs). A context that answers names its engine from the daemon's OS too, so `default` on a
/// Mac reads as the desktop app behind it; one that doesn't is known by its name alone. Two
/// contexts to one engine keep the first (the current one first of all).
pub(super) async fn engines() -> Vec<EngineStatus> {
    let os = Os::HERE;
    let mut found: Vec<EngineStatus> = Vec::new();
    let mut listed = contexts().await.unwrap_or_else(|e| {
        // No Docker CLI: no engine to list, and the report says Docker is missing.
        if !matches!(e, Error::ToolMissing { .. }) {
            log::warn!("the Docker contexts can't be listed, so no engine is: {e}");
        }
        Vec::new()
    });
    listed.sort_by_key(|(_, current)| !current);
    for (name, current) in listed {
        let args = ["--context", name.as_str(), "info", "--format", "{{.OperatingSystem}}"];
        let answer = match tokio::time::timeout(ENGINE_PROBE_TIMEOUT, run_read("docker", &args, None)).await {
            Ok(Ok(daemon_os)) => answered(&daemon_os).map(str::to_owned),
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
    let listed = engine_contexts().await.map_err(|e| Error::Invalid(format!("couldn't list the Docker contexts to find {engine}'s: {e}")))?;
    let found = listed.into_iter().find(|(_, e)| e.id() == engine).map(|(name, _)| name);
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
            // Up now, but not for this account: say so instead of waiting out the whole wait.
            Err(e) if docker_denied(&e) => return Err(docker_denied_error()),
            Err(_) => {}
        }
        tokio::time::sleep(START_POLL).await;
    }
    let minutes = (START_POLL * START_TRIES).as_secs() / 60;
    Err(Error::Invalid(format!("{engine} didn't answer within {minutes} minutes. Check its window, then re-check.")))
}

/// Opens the engine's app, or runs its CLI, on this OS.
async fn launch(engine: &str) -> Result<String> {
    match (Os::HERE, engine) {
        (Os::Macos, "docker-desktop") => run("open", &["-a", "Docker"], None).await,
        (Os::Macos, "orbstack") => run("open", &["-a", "OrbStack"], None).await,
        // Started on its own, with none of this process's handles: through `cmd /C start` the
        // app inherited them and kept them open, and a caller waiting for this process's output
        // (the guided setup's test entry under a redirect) waited for Docker Desktop to quit.
        #[cfg(windows)]
        (Os::Windows, "docker-desktop") => crate::platform::exec::launch_detached(r"C:\Program Files\Docker\Docker\Docker Desktop.exe").map(|()| String::new()),
        // Unattended (see `platform::elevation`): as root already, or through sudo, never asking.
        (Os::Linux, "docker-engine") if crate::platform::elevation::unattended() => {
            if crate::platform::elevation::is_root() {
                run("systemctl", &["start", "docker"], None).await
            } else {
                run("sudo", &["-n", "systemctl", "start", "docker"], None).await
            }
        }
        (Os::Linux, "docker-engine") => run("pkexec", &["systemctl", "start", "docker"], None).await.map_err(|e| match e {
            Error::ToolMissing { .. } => Error::Invalid(crate::machine::tools::steps::NO_PKEXEC.into()),
            e => e,
        }),
        (_, "colima") => run("colima", &["start"], None).await,
        _ => Err(Error::Invalid(format!("Cyber CTF can't start {engine} here; start it yourself, then re-check."))),
    }
}

#[cfg(test)]
mod tests;
