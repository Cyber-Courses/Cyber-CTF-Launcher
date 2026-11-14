//! A lab's docker calls pinned to the engine its launch chose, wherever the run executes.

use isothrum_runner::{BoxFuture, Exit, OnLine, ProcessError, ProcessRunner, ProcessSpec, SystemProcesses};

/// The variable a run's tools get the Docker context chosen at the lab's launch in (the engine
/// marker): [`EngineProcesses`] turns it into `--context` on the run's docker calls, wherever the
/// run executes (this process, or the runner service this app hosts).
pub(crate) const ENGINE_VAR: &str = "CYBERCTF_DOCKER_CONTEXT";

/// Pins a lab's docker/compose calls to one engine: when the run's tools get a Docker context
/// ([`ENGINE_VAR`]), `docker …` becomes `docker --context <ctx> …`. The runner's `SystemProcesses`
/// strips a `DOCKER_CONTEXT` env (to follow the current context), so the flag is used instead; it
/// takes precedence and is not stripped. Non-docker tools, and docker calls that already carry
/// `--context`, pass through untouched (the variable left out of every tool's environment).
pub(crate) struct EngineProcesses {
    inner: SystemProcesses,
}

impl EngineProcesses {
    /// `inner`'s tools, their docker calls pinned as their run says.
    pub(crate) fn new(inner: SystemProcesses) -> EngineProcesses {
        EngineProcesses { inner }
    }
}

impl ProcessRunner for EngineProcesses {
    fn run<'a>(&'a self, spec: &'a ProcessSpec, on_line: OnLine<'a>) -> BoxFuture<'a, std::result::Result<Exit, ProcessError>> {
        match pin_docker(spec) {
            None => self.inner.run(spec, on_line),
            Some(pinned) => Box::pin(async move { self.inner.run(&pinned, on_line).await }),
        }
    }
}

/// `spec` without [`ENGINE_VAR`] in its environment, and with `--context <context>` put before its
/// arguments when it is a plain `docker` call of a run pinned to `context`; None when it needs no
/// change (no context given).
fn pin_docker(spec: &ProcessSpec) -> Option<ProcessSpec> {
    let context = spec.env.iter().find(|(k, _)| k == ENGINE_VAR).map(|(_, v)| v.clone())?;
    let mut pinned = spec.clone();
    pinned.env.retain(|(k, _)| k != ENGINE_VAR);
    if pinned.program == "docker" && pinned.args.first().is_none_or(|a| a != "--context") {
        pinned.args.splice(0..0, ["--context".to_string(), context]);
    }
    Some(pinned)
}

/// The tools every run of this process starts, shared: a tool cut short (its operation dropped,
/// a cancel) is asked to stop on Unix and given a grace before it is ended, and this process waits
/// for those still stopping before it exits (`end_cut_short_tools`).
pub(super) static PROCESSES: std::sync::LazyLock<SystemProcesses> = std::sync::LazyLock::new(SystemProcesses::default);

#[cfg(test)]
mod engine_tests {
    use super::{ENGINE_VAR, ProcessSpec, pin_docker};

    /// `program args`, run by a run pinned to `context` when given.
    fn spec(program: &str, args: &[&str], context: Option<&str>) -> ProcessSpec {
        let mut spec = ProcessSpec::new(program, args.iter().copied());
        spec.env.extend(context.map(|c| (ENGINE_VAR.to_owned(), c.to_owned())));
        spec
    }

    #[test]
    fn a_chosen_engine_pins_the_labs_docker_calls_through_its_context() {
        // Every docker/compose call the runner makes for the lab gets `--context <chosen>` first,
        // so it reaches the chosen engine however the current context changes under it.
        let pinned = pin_docker(&spec("docker", &["compose", "-p", "cyberctf-x", "up"], Some("orbstack"))).unwrap();
        assert_eq!(pinned.program, "docker");
        assert_eq!(pinned.args, ["--context", "orbstack", "compose", "-p", "cyberctf-x", "up"]);
        assert!(pinned.env.iter().all(|(k, _)| k != ENGINE_VAR), "the variable never reaches the tool");
        // A run with no engine chosen is left exactly as it was.
        assert!(pin_docker(&spec("docker", &["ps"], None)).is_none());
        // A non-docker tool (vagrant for a VM form) keeps its arguments.
        assert_eq!(pin_docker(&spec("vagrant", &["up"], Some("orbstack"))).unwrap().args, ["up"]);
        // A docker call that already names a context is left alone (never two `--context`).
        assert_eq!(pin_docker(&spec("docker", &["--context", "colima", "ps"], Some("orbstack"))).unwrap().args, ["--context", "colima", "ps"]);
    }
}
