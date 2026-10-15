//! The runner's failures as the player reads them. The runner's messages are plain and technical,
//! with a stable code; the launcher's wording (and the codes its UI knows) is decided here, from
//! that code, the same as the launcher's own Docker path says it.

use isothrum_runner::{ErrorCode, RunnerError};

use crate::error::{Code, Error};

/// The launcher's error for a runner's failure.
pub(super) fn from_runner(e: RunnerError) -> Error {
    match e {
        RunnerError::PortInUse { port, .. } => Error::coded(Code::PortInUse { port }),
        RunnerError::ToolMissing { tool, .. } => Error::coded(Code::ToolMissing { tool }),
        RunnerError::ToolFailed { step, reason } => tool_failed(&step, &reason),
        RunnerError::Timeout { step, after_s } => Error::Invalid(format!(
            "The lab's `{step}` step didn't finish within {} minutes, so it was stopped. Start the lab again; if it keeps happening, check the resources of Docker, the hypervisor or the server.",
            after_s.div_ceil(60)
        )),
        RunnerError::ToolTooOld { tool, found, required } => Error::Invalid(format!(
            "This lab needs {} {required} or later, and this computer has {found}. Update it, then start the lab again.",
            tool_name(&tool)
        )),
        RunnerError::Elevation { .. } => {
            Error::Invalid("Hyper-V needs administrator rights, and the permission prompt was declined. Start the lab again and accept the prompt.".into())
        }
        RunnerError::NotReady { detail, .. } => Error::Invalid(format!("The lab started, but didn't become ready: {detail}")),
        RunnerError::SecretMissing { name } => {
            Error::Invalid(format!("This lab needs a value for `{name}` that the launch didn't give. Start it from the catalogue again."))
        }
        other => by_code(&other),
    }
}

/// The failures whose wording needs nothing but their code (their message is kept otherwise).
fn by_code(e: &RunnerError) -> Error {
    match e.code() {
        ErrorCode::AlreadyRunning => Error::Refused("This lab is already running on this machine. Stop it before starting it again.".into()),
        ErrorCode::RunningElsewhere => Error::Refused("This lab is still running somewhere else. Stop it there before starting it here.".into()),
        ErrorCode::Locked => Error::coded(Code::AlreadyInProgress),
        ErrorCode::NothingToResume => {
            Error::Invalid("Nothing is left of this lab to resume (its stopped containers or VMs are gone). Start it again for a fresh copy.".into())
        }
        ErrorCode::NotRunning => Error::Invalid("This lab isn't running. Start it first.".into()),
        ErrorCode::NotParkable => Error::Invalid("This lab can't be paused or shut down. Stop it instead.".into()),
        ErrorCode::ManifestVersion => Error::Invalid(format!(
            "This lab was generated for another version of Cyber CTF's runner. Update Cyber CTF (Settings > About), then start the lab again. ({e})"
        )),
        ErrorCode::ManifestInvalid | ErrorCode::Unsupported => Error::Invalid(format!("Cyber CTF can't run this lab's generated files: {e}")),
        _ => Error::Invalid(e.to_string()),
    }
}

/// A tool as the player knows it (`vagrant` is Vagrant).
fn tool_name(tool: &str) -> String {
    match tool {
        "vagrant" => "Vagrant".into(),
        "docker" => "Docker".into(),
        "docker-compose" => "Docker Compose".into(),
        other => format!("`{other}`"),
    }
}

/// A step's tool that ran and failed: Docker not answering its probe reads as the engine being
/// stopped (or this account not allowed to use it); an address block another network holds,
/// as what to do about it; anything else, as the command that failed.
fn tool_failed(step: &str, reason: &str) -> Error {
    if step == "tools" && reason.starts_with("`docker` doesn't answer") {
        return if reason.to_ascii_lowercase().contains("permission denied") {
            crate::platform::exec::docker_denied_error()
        } else {
            Error::coded(Code::DockerNotRunning)
        };
    }
    super::super::docker::explain_pool_overlap(Error::CommandFailed { command: format!("lab step {step}"), stderr: reason.to_owned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(e: RunnerError) -> (Code, String) {
        let e = from_runner(e);
        (e.code(), e.to_string())
    }

    #[test]
    fn a_taken_port_and_a_missing_tool_keep_the_codes_the_ui_knows() {
        assert_eq!(said(RunnerError::PortInUse { port: 8080, protocol: "tcp" }).0, Code::PortInUse { port: 8080 });
        assert_eq!(said(RunnerError::ToolMissing { step: "tools".into(), tool: "docker".into() }).0, Code::ToolMissing { tool: "docker".into() });
        assert_eq!(said(RunnerError::Locked { run: "cyberctf-web".into() }).0, Code::AlreadyInProgress);
    }

    #[test]
    fn docker_not_answering_its_probe_reads_as_a_stopped_engine() {
        let down = RunnerError::ToolFailed { step: "tools".into(), reason: "`docker` doesn't answer: Cannot connect to the Docker daemon".into() };
        assert_eq!(said(down).0, Code::DockerNotRunning);
        let denied = RunnerError::ToolFailed { step: "tools".into(), reason: "`docker` doesn't answer: permission denied while trying to connect".into() };
        assert!(matches!(said(denied).0, Code::DockerDenied { .. } | Code::DockerDeniedWindows { .. }));
    }

    #[test]
    fn a_failed_step_says_which_and_why_and_a_pool_overlap_what_to_do() {
        let (code, text) = said(RunnerError::ToolFailed { step: "up".into(), reason: "exit status 1".into() });
        assert_eq!(code, Code::CommandFailed { command: "lab step up".into(), stderr: "exit status 1".into() });
        assert_eq!(text, "`lab step up` failed: exit status 1");
        let overlap = RunnerError::ToolFailed { step: "up".into(), reason: "invalid pool request: Pool overlaps with other one on this address space".into() };
        assert!(said(overlap).1.starts_with("Another Docker network already uses the addresses this lab needs."));
    }

    #[test]
    fn the_lifecycle_refusals_say_what_to_do() {
        let (code, text) = said(RunnerError::AlreadyRunning { run: "cyberctf-web".into() });
        assert_eq!((code, text.as_str()), (Code::Other, "This lab is already running on this machine. Stop it before starting it again."));
        assert!(matches!(from_runner(RunnerError::AlreadyRunning { run: "r".into() }), Error::Refused(_)), "a refusal leaves the last verdict as it was");
        assert!(said(RunnerError::NothingToResume { run: "r".into() }).1.starts_with("Nothing is left of this lab to resume"));
        assert!(said(RunnerError::NotRunning { run: "r".into() }).1.contains("Start it first"));
        assert!(said(RunnerError::Timeout { step: "up".into(), after_s: 1800 }).1.contains("within 30 minutes"));
        assert!(said(RunnerError::SecretMissing { name: "CTF_FLAG_MAIN".into() }).1.contains("`CTF_FLAG_MAIN`"));
    }

    #[test]
    fn a_vm_labs_own_failures_say_what_to_do() {
        let old = RunnerError::ToolTooOld { tool: "vagrant".into(), found: "2.3.8".into(), required: "2.4.0".into() };
        assert_eq!(said(old).1, "This lab needs Vagrant 2.4.0 or later, and this computer has 2.3.8. Update it, then start the lab again.");
        let declined = RunnerError::Elevation { reason: "the prompt was declined".into() };
        assert!(said(declined).1.starts_with("Hyper-V needs administrator rights"));
        assert!(said(RunnerError::Timeout { step: "up".into(), after_s: 21600 }).1.starts_with("The lab's `up` step didn't finish within 360 minutes"));
    }

    #[test]
    fn anything_else_keeps_the_runners_words() {
        let e = RunnerError::Unbound { at: "lifecycle.up.up".into(), placeholder: "params.X".into() };
        let text = e.to_string();
        assert!(said(e).1.ends_with(&text));
        let other = RunnerError::Param { name: "ISOLOOM_DERIVED".into(), reason: "`2` isn't one of 0, 1".into() };
        assert_eq!(said(other).1, "param `ISOLOOM_DERIVED`: `2` isn't one of 0, 1");
    }
}
