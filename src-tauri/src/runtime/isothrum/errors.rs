//! The runner's failures as the player reads them. The runner's messages are plain and technical,
//! with a stable code; the launcher's wording (and the codes its UI knows) is decided here, from
//! that code, the same as the launcher's own Docker path says it.

use isothrum_runner::{Detail, Error as RunnerError, ErrorCode, ErrorDetail, TimedOut};

use super::runner_said;
use crate::error::{Code, Error};
use crate::platform::exec::DockerSaid;
use crate::runtime::port::HostPort;
use crate::runtime::providers::Provider;
use crate::runtime::server::kind::{KINDS, kind};

/// Whether `e` is a change of a run refused before acting on it: refused as such (the run isn't
/// up, a value it needs is missing or unusable — the runner checks the launch's values before it
/// runs anything —, the cloud account's sign-in ran out, asked before the change:
/// [`Error::RefusedBeforeActing`]; the run already up or recorded elsewhere: [`Error::Refused`]),
/// another operation holds it, or a tool it needs is missing. Nothing of the run changed then.
pub(crate) fn refused_before_acting(e: &Error) -> bool {
    matches!(e, Error::RefusedBeforeActing(_) | Error::Refused(_)) || matches!(e.code(), Code::AlreadyInProgress | Code::ToolMissing { .. })
}

/// What a step stopped for taking too long says: how long it went without progress (and what it
/// last printed), or that it hit the longest any step may run.
fn timed_out_step(step: &str, timed_out: &TimedOut) -> Error {
    let step = step.to_owned();
    let minutes = timed_out.after_s().div_ceil(60);
    Error::coded(match timed_out {
        TimedOut::NoProgress { last_line: Some(line), .. } => Code::StepStalled { step, minutes, last_line: line.clone() },
        TimedOut::NoProgress { last_line: None, .. } => Code::StepSilent { step, minutes },
        TimedOut::Ceiling { after_s } => Code::StepCeiling { step, hours: after_s.div_ceil(3600) },
        // A reason added later reads as the plain limit.
        _ => Code::StepTimedOut { step, minutes },
    })
}

/// The launcher's error for a runner's failure.
pub(super) fn from_runner(e: RunnerError) -> Error {
    match e {
        // The holder is filled in later, on the launch path, where an async lookup can run.
        RunnerError::PortInUse { port, .. } => Error::coded(Code::PortInUse { port: HostPort::new(port.get()), holder: None }),
        RunnerError::ToolMissing { tool, .. } => Error::coded(Code::ToolMissing { tool }),
        RunnerError::ToolFailed { step, reason } => tool_failed(&step, &reason),
        RunnerError::ToolUnanswered { tool, reason, .. } if tool == "docker" => docker_unanswered(&reason),
        RunnerError::Timeout { step, timed_out } => timed_out_step(&step, &timed_out),
        // The lab's images, pulled before its containers start: a slow connection, not the
        // resources of Docker, the hypervisor or the server.
        // A Vagrant box, downloaded before its VMs boot, in a step of its own (`boxes-<step>`).
        RunnerError::DownloadStalled { step, after_s, .. } if runner_said::is_box_download(&step) => {
            Error::coded(Code::BoxDownloadStalled { minutes: after_s.div_ceil(60) })
        }
        // Vagrant ends its sentences with a period; the message adds its own.
        RunnerError::DownloadFailed { step, reason } if runner_said::is_box_download(&step) => {
            Error::coded(Code::BoxDownloadFailed { reason: reason.trim().trim_end_matches('.').to_owned() })
        }
        RunnerError::DownloadStalled { after_s, .. } => Error::coded(Code::DownloadStalled { minutes: after_s.div_ceil(60) }),
        RunnerError::DownloadFailed { reason, .. } => Error::coded(Code::DownloadFailed { reason: reason.trim().to_owned() }),
        // Named by the address the run's settings give until [`on_server`] names it as Servers does.
        RunnerError::ServerAuthFailed { server, .. } => Error::coded(Code::ServerAuthFailed { server: server.unwrap_or_else(|| "the server".into()) }),
        RunnerError::ToolTooOld { tool, found, required } => Error::coded(Code::ToolTooOld { tool: tool_name(&tool), found, required }),
        // The prompt declined or unanswered, or the elevated session ended (the reason says which).
        RunnerError::Elevation { reason } => Error::coded(Code::ElevationFailed { reason }),
        RunnerError::NotReady { detail, .. } => Error::coded(Code::LabNotReady { detail }),
        RunnerError::PartlyGone { machines, .. } => Error::coded(Code::MachinesGone { machines: machines.join(", ") }),
        RunnerError::SecretMissing { name } => {
            Error::RefusedBeforeActing(format!("This lab needs a value for `{name}` that the launch didn't give. Start it from the catalogue again."))
        }
        RunnerError::Input { name, reason } => {
            Error::RefusedBeforeActing(format!("This lab needs a value for `{name}` it can use: the one given {reason}. Start it from the catalogue again."))
        }
        other => by_code(other.code(), &other),
    }
}

/// `e`, a failure of a run on the server named `name` in Servers: a refused sign-in names it so,
/// for the player to find it there.
pub(crate) fn on_server(e: Error, name: &str) -> Error {
    match e.code() {
        Code::ServerAuthFailed { .. } => Error::coded(Code::ServerAuthFailed { server: name.to_owned() }),
        _ => e,
    }
}

/// What is said of a container lab an earlier version runs inside one helper VM (Isoloom's former
/// `docker-vm`) when the runner no longer opens that run: nothing here reaches the VM any more.
pub(crate) const HELPER_VM_LEFT: &str = "This lab still runs Docker inside a VM, as an earlier version of Cyber CTF started it, and this version can't reach that VM any more: remove it on the Machine page or in your hypervisor's app, then start the lab again";

/// The runner's refusal to open the run of a helper VM an earlier version left, run `run`: when
/// it no longer runs that form (`unsupported`), what is left and how to remove it; any other
/// failure as [`from_runner`] says.
pub(super) fn of_helper_vm(e: RunnerError, run: &str) -> Error {
    match e.code() {
        ErrorCode::Unsupported => Error::Invalid(format!("{HELPER_VM_LEFT} (its name starts with {run}).")),
        _ => from_runner(e),
    }
}

/// The launcher's error for an operation that failed in the runner service: its code `code`, the
/// runner's `message`, the `step` that failed and the failure's typed fields (`detail`), worded as
/// [`from_runner`] words the same failure in this process; by its code alone, its message kept,
/// where the service gives no fields this build reads.
pub(crate) fn from_service(code: ErrorCode, message: &str, step: Option<&str>, detail: Option<&ErrorDetail>) -> Error {
    let step = step.unwrap_or_default();
    match detail.map(|d| &d.data) {
        Some(Detail::PortInUse { port, .. }) => Error::coded(Code::PortInUse { port: HostPort::new(*port), holder: None }),
        Some(Detail::ToolMissing { tool }) => Error::coded(Code::ToolMissing { tool: tool.clone() }),
        Some(Detail::ToolUnanswered { tool }) if tool == "docker" => docker_unanswered(message),
        Some(Detail::ToolTooOld { tool, found, required }) => {
            Error::coded(Code::ToolTooOld { tool: tool_name(tool), found: found.clone(), required: required.clone() })
        }
        Some(Detail::Timeout { timed_out }) => timed_out_step(step, timed_out),
        Some(Detail::DownloadStalled { after_s }) if runner_said::is_box_download(step) => {
            Error::coded(Code::BoxDownloadStalled { minutes: after_s.div_ceil(60) })
        }
        Some(Detail::DownloadStalled { after_s }) => Error::coded(Code::DownloadStalled { minutes: after_s.div_ceil(60) }),
        Some(Detail::ServerAuthFailed { server }) => Error::coded(Code::ServerAuthFailed { server: server.clone().unwrap_or_else(|| "the server".into()) }),
        Some(Detail::PartlyGone { machines }) => Error::coded(Code::MachinesGone { machines: machines.join(", ") }),
        Some(Detail::SecretMissing { name }) => {
            Error::RefusedBeforeActing(format!("This lab needs a value for `{name}` that the launch didn't give. Start it from the catalogue again."))
        }
        Some(Detail::InputInvalid { name }) => {
            Error::RefusedBeforeActing(format!("This lab needs a value for `{name}` it can use: {message}. Start it from the catalogue again."))
        }
        _ => match code {
            ErrorCode::ToolFailed => tool_failed(step, message),
            _ => by_code(code, message),
        },
    }
}

/// The failures whose wording needs nothing but their code `code` (the runner's words `e`, kept
/// otherwise).
fn by_code(code: ErrorCode, e: impl std::fmt::Display) -> Error {
    match code {
        ErrorCode::AlreadyRunning => Error::Refused("This lab is already running on this machine. Stop it before starting it again.".into()),
        ErrorCode::RunningElsewhere => Error::Refused("This lab is still running somewhere else. Stop it there before starting it here.".into()),
        ErrorCode::Locked => Error::coded(Code::AlreadyInProgress),
        ErrorCode::NothingToResume => Error::coded(Code::NothingToResume),
        ErrorCode::NotRunning => Error::RefusedBeforeActing("This lab isn't running. Start it first.".into()),
        ErrorCode::NotParkable => Error::coded(Code::NotParkable),
        ErrorCode::ProjectInvalid | ErrorCode::Unsupported => Error::Invalid(format!("Cyber CTF can't run this lab's generated files: {e}")),
        _ => Error::Invalid(e.to_string()),
    }
}

/// `e`, a container lab's step on this machine's Docker that failed, as the player reads it: the
/// engine stopped while the step ran (Docker Desktop quit, the service stopped) is said as a
/// stopped engine, as the start's own probe says it, not as the step's failure. Only for a lab on
/// this machine's Docker: a container lab in a VM runs its Docker inside the VM.
pub(super) fn engine_stopped(e: Error) -> Error {
    if DockerSaid::of_error(&e) == Some(DockerSaid::EngineDown) { Error::coded(Code::DockerNotRunning) } else { e }
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

/// A cloud that signs in through its CLI: its name, and how its sign-in is made again (its row's,
/// [`crate::runtime::server::kind::CliSignIn`]).
fn sign_in_of(cloud: Provider) -> Option<(&'static str, &'static str)> {
    kind(cloud)?.cli_sign_in.as_ref().map(|s| (cloud.label(), s.again))
}

/// The cloud a Terraform failure comes from, by its Terraform provider's name in the message.
fn cloud_in(reason: &str) -> Option<Provider> {
    KINDS.iter().find(|k| k.cli_sign_in.as_ref().is_some_and(|s| reason.contains(s.terraform_provider))).map(|k| k.provider)
}

/// The launcher's word for a cloud tool's `reason` when it says the account's sign-in ran out or
/// is missing (`cloud`, when known): sign in again, then try again, a refusal before acting
/// (nothing could change without the sign-in); `None` for any other failure.
pub(crate) fn expired_sign_in(reason: &str, cloud: Option<Provider>) -> Option<Error> {
    if !crate::platform::tool_output::cloud_sign_in::expired(reason) {
        return None;
    }
    let (name, again) = cloud.or_else(|| cloud_in(reason)).and_then(sign_in_of).map_or(("the cloud account", String::new()), |(n, a)| (n, format!(" ({a})")));
    Some(Error::RefusedBeforeActing(format!(
        "Your sign-in to {name} has expired or is missing, so the lab wasn't changed. Sign in again{again}, then try again. ({})",
        reason.trim()
    )))
}

/// Docker not answering the runner's probe (`tool_unanswered`), Docker's own words `said`: the
/// engine stopped, or this account not allowed to use it.
fn docker_unanswered(said: &str) -> Error {
    if DockerSaid::is_refusal(&said.to_ascii_lowercase()) { crate::platform::exec::docker_denied_error() } else { Error::coded(Code::DockerNotRunning) }
}

/// A step's tool that ran and failed: a cloud sign-in that ran out reads as signing in again; an
/// address block another network holds, as what to do about it; anything else, as the command
/// that failed.
fn tool_failed(step: &str, reason: &str) -> Error {
    if let Some(expired) = expired_sign_in(reason, None) {
        return expired;
    }
    super::super::docker::explain_pool_overlap(Error::CommandFailed { command: format!("lab step {step}"), stderr: reason.to_owned() })
}

/// UTM's passing scripting errors, by the code `osascript` reports, each with how the log says
/// it: `UTM got an error: Application isn't running. (-600)` from a VM customization right after
/// an import, or from the UTM provider reading the VM's forwarded ports while it boots (a start,
/// a reload), while UTM runs and answers every other command; and `AppleEvent timed out.
/// (-1712)` from the same reads while UTM is busy. The same operation then goes through. The
/// other -600, a UTM that really isn't reachable, the start's UTM check catches before.
///
/// How `e` is said, if it is one of them (by its Apple event error number, [`AppleEvent`]).
fn utm_transient(e: &Error) -> Option<&'static str> {
    use crate::platform::tool_output::apple_events::AppleEvent;
    match AppleEvent::of(&e.to_string())? {
        AppleEvent::AppNotRunning => Some("\"Application isn't running\" while it runs"),
        AppleEvent::TimedOut => Some("\"AppleEvent timed out\" while busy"),
        AppleEvent::NotAuthorized | AppleEvent::Other(_) => None,
    }
}

/// What the log says before an operation UTM refused in passing (`said`) is tried once more.
fn utm_retry_line(said: &str) -> String {
    format!("UTM answered {said} (a passing UTM scripting error, seen while a VM boots): trying again.")
}

/// Whether `e` is UTM's passing scripting error ([`utm_transient`]), saying in `log` that it is
/// tried once more when it is (which try, `LabRun::after_utm_refusal` decides from what the
/// failed attempt left).
pub(super) fn try_again_after_utm(e: &Error, log: &mut impl FnMut(String)) -> bool {
    let said = utm_transient(e);
    if let Some(said) = said {
        log(utm_retry_line(said));
    }
    said.is_some()
}

#[cfg(test)]
mod tests;
