//! The Docker-compatible engines, as isothrum-machine finds them ([`isothrum_machine::docker`]),
//! in the launcher's errors; and switching or starting one the way the player would.

use isothrum_machine::docker::StartFailed;
pub use isothrum_machine::docker::{Engine, EngineStatus};

use crate::error::{Error, Result};
use crate::platform::exec::docker_denied_error;
use crate::runtime::docker::{context_error, engine_of};
// A start pins a lab to an engine's context and says the daemon's answers from the runtime,
// below this layer; the machine page says them the same way.
pub(crate) use crate::runtime::docker::{engine_context, no_answer};

/// What isothrum-machine runs Docker with ([`crate::runtime::providers::machine_check`]).
fn machine() -> isothrum_machine::Context {
    crate::runtime::providers::machine_check()
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
