//! The Docker-compatible engines as a start needs them: the Docker context behind one, and
//! isothrum-machine's answers about the daemon ([`isothrum_machine::docker`]) in the launcher's
//! errors. The machine page's switching and starting of engines (`machine::system::docker`)
//! says them the same way.

use isothrum_machine::docker::{ContextError, Engine, NoAnswer};

use crate::error::{Error, Result};

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
pub(crate) fn engine_of(engine: &str) -> Result<Engine> {
    Engine::from_id(engine).ok_or_else(|| Error::Invalid(format!("no Docker context for {engine}")))
}

/// A context that couldn't be found or used, as the launcher's error.
pub(crate) fn context_error(engine: &str, e: ContextError) -> Error {
    match e {
        ContextError::Unlisted(why) => Error::Invalid(format!("couldn't list the Docker contexts to find {engine}'s: {}", no_answer(why))),
        ContextError::Unused(why) => no_answer(why),
        none => Error::Invalid(none.to_string()),
    }
}

/// The Docker context behind `engine` on this machine, for pinning a lab to it (`DOCKER_CONTEXT` /
/// `docker --context <name>`); an error when no context on this machine names that engine.
pub async fn engine_context(engine: &str) -> Result<String> {
    let cx = crate::runtime::providers::machine_check();
    isothrum_machine::docker::engine_context(&cx, engine_of(engine)?).await.map_err(|e| context_error(engine, e))
}
