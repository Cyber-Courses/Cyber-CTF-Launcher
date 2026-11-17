//! The Docker-compatible engines, as isothrum-machine finds them ([`isothrum_machine::docker`]),
//! in the launcher's errors; and switching or starting one the way the player would.

use std::time::Duration;

use isothrum_machine::docker::{ContextError, NoAnswer};
pub use isothrum_machine::docker::{Engine, EngineStatus};

use crate::error::{Error, Result};
use crate::platform::exec::{docker_denied, docker_denied_error, run};
use crate::platform::host::Os;

/// How many times a started engine is asked whether it answers: 90, [`START_POLL`] apart.
const START_TRIES: u32 = 90;
/// How long between two of those tries.
const START_POLL: Duration = Duration::from_secs(2);

/// What isothrum-machine runs Docker with ([`crate::runtime::providers::machine_check`]).
fn machine() -> isothrum_machine::Context {
    crate::runtime::providers::machine_check()
}

/// Why the daemon gave no answer, as the launcher's error: a refusal or a stopped daemon as the
/// Docker call that failed, which [`docker_denied`] reads.
pub(crate) fn no_answer(why: NoAnswer) -> Error {
    match why {
        NoAnswer::NoCli => Error::ToolMissing { tool: "docker" },
        NoAnswer::Denied(said) | NoAnswer::Down(said) => Error::CommandFailed { command: "docker info".into(), stderr: said },
        empty => Error::Invalid(empty.to_string()),
    }
}

/// `docker info`'s operating system line: the daemon answers (for this account).
async fn daemon_os() -> Result<String> {
    isothrum_machine::docker::daemon_os(&machine()).await.map_err(no_answer)
}

/// The engine with the webview's id `engine`.
fn engine_of(engine: &str) -> Result<Engine> {
    Engine::from_id(engine).ok_or_else(|| Error::Invalid(format!("no Docker context for {engine}")))
}

/// A context that couldn't be found or used, as the launcher's error.
fn context_error(engine: &str, e: ContextError) -> Error {
    match e {
        ContextError::Unlisted(why) => Error::Invalid(format!("couldn't list the Docker contexts to find {engine}'s: {}", no_answer(why))),
        ContextError::Unused(why) => no_answer(why),
        none => Error::Invalid(none.to_string()),
    }
}

/// The Docker context behind `engine` on this machine, for pinning a lab to it (`DOCKER_CONTEXT` /
/// `docker --context <name>`); an error when no context on this machine names that engine.
pub async fn engine_context(engine: &str) -> Result<String> {
    isothrum_machine::docker::engine_context(&machine(), engine_of(engine)?).await.map_err(|e| context_error(engine, e))
}

/// Points the Docker CLI at another running engine (`docker context use`), as the player
/// would in a terminal. Labs then run on it.
pub async fn docker_use_engine(engine: String) -> Result<()> {
    isothrum_machine::docker::use_engine(&machine(), engine_of(&engine)?).await.map_err(|e| context_error(&engine, e))
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
