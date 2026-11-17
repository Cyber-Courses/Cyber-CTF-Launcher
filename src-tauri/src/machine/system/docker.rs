//! The Docker-compatible engines, as isothrum-machine finds them ([`isothrum_machine::docker`]),
//! in the launcher's errors; and switching or starting one the way the player would.

use isothrum_machine::docker::{ContextError, NoAnswer, StartFailed};
pub use isothrum_machine::docker::{Engine, EngineStatus};

use crate::error::{Error, Result};
use crate::platform::exec::docker_denied_error;

/// What isothrum-machine runs Docker with ([`crate::runtime::providers::machine_check`]).
fn machine() -> isothrum_machine::Context {
    crate::runtime::providers::machine_check()
}

/// Why the daemon gave no answer, as the launcher's error: a refusal or a stopped daemon as the
/// Docker call that failed, its words as the CLI said them ([`isothrum_machine::docker::says_denied`]).
pub(crate) fn no_answer(why: NoAnswer) -> Error {
    match why {
        NoAnswer::NoCli => Error::ToolMissing { tool: "docker" },
        NoAnswer::Denied(said) | NoAnswer::Down(said) => Error::CommandFailed { command: "docker info".into(), stderr: said },
        empty => Error::Invalid(empty.to_string()),
    }
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
/// its CLI), points Docker at it and waits until it answers ([`isothrum_machine::docker::start_engine`]).
/// Docker Desktop and OrbStack take a while on first start, hence the generous wait.
pub async fn docker_start_engine(engine: String) -> Result<()> {
    let Some(id) = Engine::from_id(&engine) else { return Err(cant_start(&engine)) };
    let elevation = crate::machine::tools::install::elevation();
    isothrum_machine::docker::start_engine(&machine(), id, elevation, &mut |_| {}).await.map_err(|failed| match failed {
        StartFailed::NotHere(_) => cant_start(&engine),
        // Running, but not for this account: starting it again can't help.
        StartFailed::Denied(_) => docker_denied_error(),
        StartFailed::Step(failed) => crate::machine::tools::steps::step_failed(failed, false),
        StartFailed::NoAnswer { waited, .. } => {
            Error::Invalid(format!("{engine} didn't answer within {} minutes. Check its window, then re-check.", waited.as_secs() / 60))
        }
        other => Error::Invalid(other.to_string()),
    })
}

/// An engine Cyber CTF doesn't start on this OS.
fn cant_start(engine: &str) -> Error {
    Error::Invalid(format!("Cyber CTF can't start {engine} here; start it yourself, then re-check."))
}

#[cfg(test)]
mod tests;
