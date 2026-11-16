//! The runner's failures as the player reads them, in one place: [`coded`], from the runner's
//! report (its stable code, its typed fields and its plain, technical message), whichever way the
//! failure came. The launcher's wording (and the codes its UI knows) is decided here; the tools'
//! words are read by the runner, never here.

use isothrum_runner::{Cloud, Detail, DownloadKind, ErrorCode, ErrorReport, ProviderNeed, TimedOut};

use crate::error::{Code, Error};
use crate::runtime::providers::Provider;
use crate::runtime::server::kind::kind;
use isothrum_runner::HostPort;

/// The launcher's error for a runner's failure, read from its report: the one place a runner's
/// failure becomes the launcher's, whichever way it came (the runner here, an `error` event, a
/// refusal of the service). Each code the runner has is mapped here by name ([`known`]); one a
/// later runner adds reads as its own message.
pub(crate) fn coded(report: &ErrorReport) -> Error {
    known(report).unwrap_or_else(|| Error::Invalid(report.message.clone()))
}

/// [`coded`] for a code this build knows: every code the runner has, by name (the test
/// `every_code_the_runner_has_is_mapped_by_name` holds it to that); none for a code a later
/// runner added.
fn known(report: &ErrorReport) -> Option<Error> {
    Some(match report.code {
        ErrorCode::PortInUse
        | ErrorCode::ToolMissing
        | ErrorCode::ToolTooOld
        | ErrorCode::ToolFailed
        | ErrorCode::ToolUnanswered
        | ErrorCode::ToolDenied
        | ErrorCode::CredentialsExpired
        | ErrorCode::AddressPoolOverlap
        | ErrorCode::ProviderRefused
        | ErrorCode::ElevationFailed => of_tool(report),
        ErrorCode::Timeout | ErrorCode::DownloadStalled | ErrorCode::DownloadFailed | ErrorCode::NotReady | ErrorCode::ServerAuthFailed => of_step(report),
        ErrorCode::NothingToResume
        | ErrorCode::SecretMissing
        | ErrorCode::InputInvalid
        | ErrorCode::AlreadyRunning
        | ErrorCode::RunningElsewhere
        | ErrorCode::Locked
        | ErrorCode::NotRunning
        | ErrorCode::NotParkable
        | ErrorCode::ProjectInvalid
        | ErrorCode::Unsupported => of_run(report),
        // The runner's own words: nothing the player does differently for these.
        ErrorCode::Io
        | ErrorCode::Corrupt
        | ErrorCode::RunName
        | ErrorCode::MachineUnknown
        | ErrorCode::MachineAmbiguous
        | ErrorCode::NetworkUnknown
        | ErrorCode::AdoptRefused
        | ErrorCode::Cancelled
        | ErrorCode::Usage
        | ErrorCode::NoConsole
        | ErrorCode::IllegalTransition
        | ErrorCode::StateConflict
        | ErrorCode::StoreVersion
        | ErrorCode::StoreCorrupt
        | ErrorCode::LeaseHeld => Error::Invalid(report.message.clone()),
        _ => return None,
    })
}

/// A tool's failure, from its code and fields: one it can't be read from keeps the runner's
/// words.
fn of_tool(report: &ErrorReport) -> Error {
    let message = report.message.as_str();
    let step = report.step.as_deref().unwrap_or_default();
    match (report.code, report.detail.as_ref().map(|d| &d.data)) {
        (ErrorCode::PortInUse, Some(Detail::PortInUse { port, holder, .. })) => {
            Error::coded(Code::PortInUse { port: HostPort::new(*port), holder: holder.as_ref().map(|h| format!("{} (pid {})", known_as(&h.program), h.pid)) })
        }
        (ErrorCode::ToolMissing, Some(Detail::ToolMissing { tool })) => Error::coded(Code::ToolMissing { tool: tool.clone() }),
        (ErrorCode::ToolTooOld, Some(Detail::ToolTooOld { tool, found, required })) => {
            Error::coded(Code::ToolTooOld { tool: tool_name(tool), found: found.clone(), required: required.clone() })
        }
        (ErrorCode::ToolFailed, Some(Detail::ToolFailed { reason })) => tool_failed(step, reason),
        (ErrorCode::ToolFailed, _) => tool_failed(step, message),
        // Docker's engine stopped, or quit while a step ran.
        (ErrorCode::ToolUnanswered, Some(Detail::ToolUnanswered { tool })) if tool == "docker" => Error::coded(Code::DockerNotRunning),
        // Docker's engine refused this account its socket or pipe.
        (ErrorCode::ToolDenied, Some(Detail::ToolDenied { tool })) if tool == "docker" => crate::platform::exec::docker_denied_error(),
        (ErrorCode::CredentialsExpired, Some(Detail::CredentialsExpired { cloud, reason })) => expired_sign_in(cloud.and_then(cloud_provider), reason),
        (ErrorCode::CredentialsExpired, _) => expired_sign_in(None, message),
        (ErrorCode::AddressPoolOverlap, Some(Detail::AddressPoolOverlap { reason })) => pool_overlap(&tool_failed(step, reason)),
        (ErrorCode::AddressPoolOverlap, _) => pool_overlap(&tool_failed(step, message)),
        // The machine changed after its check (virtiofsd removed): the hypervisor's refusal as
        // the check says it, not vagrant-libvirt's stack trace or vagrant-qemu's words.
        (ErrorCode::ProviderRefused, Some(Detail::ProviderRefused { provider, need: Some(ProviderNeed::Virtiofsd) })) => {
            Error::coded(Code::VirtiofsdMissing { hypervisor: hypervisor_name(provider) })
        }
        // The system refused the hypervisor's app (UTM's start answered "Operation not permitted",
        // or the Apple events refused): the way out is a permission, never a wait or a retry.
        (ErrorCode::ProviderRefused, Some(Detail::ProviderRefused { provider, need: Some(ProviderNeed::Permission) })) => {
            Error::coded(Code::HypervisorNotPermitted { hypervisor: hypervisor_name(provider) })
        }
        // The prompt declined or unanswered, or the elevated session ended (the reason says which).
        (ErrorCode::ElevationFailed, Some(Detail::Elevation { reason })) => Error::coded(Code::ElevationFailed { reason: reason.clone() }),
        _ => Error::Invalid(message.to_owned()),
    }
}

/// A step's failure (its time, its downloads, its ready condition, the server's sign-in), from
/// its code and fields.
fn of_step(report: &ErrorReport) -> Error {
    let step = report.step.as_deref().unwrap_or_default();
    match (report.code, report.detail.as_ref().map(|d| &d.data)) {
        (ErrorCode::Timeout, Some(Detail::Timeout { timed_out })) => timed_out_step(step, timed_out),
        // A Vagrant box, downloaded before its VMs boot.
        (ErrorCode::DownloadStalled, Some(Detail::DownloadStalled { what: DownloadKind::Boxes, after_s })) => {
            Error::coded(Code::BoxDownloadStalled { minutes: after_s.div_ceil(60) })
        }
        // The lab's images, pulled before its containers start: a slow connection, not the
        // resources of Docker, the hypervisor or the server.
        (ErrorCode::DownloadStalled, Some(Detail::DownloadStalled { after_s, .. })) => Error::coded(Code::DownloadStalled { minutes: after_s.div_ceil(60) }),
        // Vagrant ends its sentences with a period; the message adds its own.
        (ErrorCode::DownloadFailed, Some(Detail::DownloadFailed { what: DownloadKind::Boxes, reason })) => {
            Error::coded(Code::BoxDownloadFailed { reason: reason.trim().trim_end_matches('.').to_owned() })
        }
        (ErrorCode::DownloadFailed, Some(Detail::DownloadFailed { reason, .. })) => Error::coded(Code::DownloadFailed { reason: reason.trim().to_owned() }),
        (ErrorCode::NotReady, Some(Detail::NotReady { detail, .. })) => Error::coded(Code::LabNotReady { detail: detail.clone() }),
        // Named by the address the run's settings give until [`on_server`] names it as Servers does.
        (ErrorCode::ServerAuthFailed, Some(Detail::ServerAuthFailed { server: Some(server) })) => {
            Error::coded(Code::ServerAuthFailed { server: server.clone() })
        }
        (ErrorCode::ServerAuthFailed, _) => Error::coded(Code::ServerAuthFailed { server: "the server".into() }),
        _ => Error::Invalid(report.message.clone()),
    }
}

/// A refusal by the run's state or its values, said as what to do.
fn of_run(report: &ErrorReport) -> Error {
    let message = report.message.as_str();
    match (report.code, report.detail.as_ref().map(|d| &d.data)) {
        (ErrorCode::NothingToResume, Some(Detail::PartlyGone { machines })) => Error::coded(Code::MachinesGone { machines: machines.join(", ") }),
        (ErrorCode::NothingToResume, _) => Error::coded(Code::NothingToResume),
        (ErrorCode::SecretMissing, Some(Detail::SecretMissing { name })) => {
            Error::RefusedBeforeActing(format!("This lab needs a value for `{name}` that the launch didn't give. Start it from the catalogue again."))
        }
        (ErrorCode::InputInvalid, Some(Detail::InputInvalid { name, reason })) => {
            Error::RefusedBeforeActing(format!("This lab needs a value for `{name}` it can use: the one given {reason}. Start it from the catalogue again."))
        }
        (ErrorCode::SecretMissing | ErrorCode::InputInvalid, _) => Error::RefusedBeforeActing(message.to_owned()),
        (ErrorCode::AlreadyRunning, _) => Error::Refused("This lab is already running on this machine. Stop it before starting it again.".into()),
        (ErrorCode::RunningElsewhere, _) => Error::Refused("This lab is still running somewhere else. Stop it there before starting it here.".into()),
        (ErrorCode::Locked, _) => Error::coded(Code::AlreadyInProgress),
        (ErrorCode::NotRunning, _) => Error::RefusedBeforeActing("This lab isn't running. Start it first.".into()),
        (ErrorCode::NotParkable, _) => Error::coded(Code::NotParkable),
        (ErrorCode::ProjectInvalid | ErrorCode::Unsupported, _) => Error::Invalid(format!("Cyber CTF can't run this lab's generated files: {message}")),
        _ => Error::Invalid(message.to_owned()),
    }
}

/// What a step stopped for taking too long says: how long it went without progress (and what it
/// last printed), or that it hit the longest any step may run.
fn timed_out_step(step: &str, timed_out: &TimedOut) -> Error {
    let step = step.to_owned();
    let minutes = timed_out.after_s().div_ceil(60);
    Error::coded(match timed_out {
        TimedOut::NoProgress { last_line: Some(line), .. } => Code::StepStalled { step, minutes, last_line: line.clone() },
        TimedOut::NoProgress { last_line: None, .. } => Code::StepSilent { step, minutes },
        TimedOut::Ceiling { after_s, .. } => Code::StepCeiling { step, hours: after_s.div_ceil(3600) },
        // A reason added later reads as the plain limit.
        _ => Code::StepTimedOut { step, minutes },
    })
}

/// What the player knows a process listening on a port by: Docker's several process names (and
/// `lsof`'s 9-character cut of them) as Docker.
fn known_as(program: &str) -> &str {
    match program.trim() {
        "com.docke" | "Docker" | "docker" | "dockerd" | "docker-pr" | "docker-proxy" | "vpnkit" | "vpnkit-brid" => "Docker",
        other => other,
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

/// Whether the runner refused `report`'s run because it no longer runs that form of project at
/// all (`unsupported`): a helper VM an earlier version left.
pub(super) fn form_unsupported(report: &ErrorReport) -> bool {
    report.code == ErrorCode::Unsupported
}

/// Whether `report` says nothing of the run is left to resume.
pub(super) fn nothing_to_resume(report: &ErrorReport) -> bool {
    report.code == ErrorCode::NothingToResume
}

/// The report of an operation the service cancelled before it began: its stream ended with no
/// `result` nor `error`.
pub(super) fn cancelled_before_it_began() -> ErrorReport {
    ErrorReport::new(ErrorCode::Cancelled, "the operation was cancelled before it began")
}

/// What is said of a container lab an earlier version runs inside one helper VM (Isoloom's former
/// `docker-vm`) when the runner no longer opens that run: nothing here reaches the VM any more.
pub(crate) const HELPER_VM_LEFT: &str = "This lab still runs Docker inside a VM, as an earlier version of Cyber CTF started it, and this version can't reach that VM any more: remove it on the Machine page or in your hypervisor's app, then start the lab again";

/// The runner's refusal to open the run of a helper VM an earlier version left, run `run`: when
/// it no longer runs that form (`unsupported`), what is left and how to remove it; any other
/// failure as [`coded`] says it.
pub(super) fn of_helper_vm(report: &ErrorReport, run: &str) -> Error {
    match report.code {
        ErrorCode::Unsupported => Error::Invalid(format!("{HELPER_VM_LEFT} (its name starts with {run}).")),
        _ => coded(report),
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

/// A cloud that signs in through its CLI: its name, and how its sign-in is made again (its row's,
/// [`crate::runtime::server::kind::CliSignIn`]).
fn sign_in_of(cloud: Provider) -> Option<(&'static str, &'static str)> {
    kind(cloud)?.cli_sign_in.as_ref().map(|s| (cloud.label(), s.again))
}

/// The launcher's provider for `cloud`, the runner's name of a cloud.
fn cloud_provider(cloud: Cloud) -> Option<Provider> {
    Provider::from_id(&isothrum_runner::wire_name(&cloud))
}

/// A cloud's refusal of the sign-in its tools use (`reason`, what they said), on `cloud` when the
/// runner named it: sign in again, then try again. A refusal before acting: the cloud refused
/// before anything was applied.
fn expired_sign_in(cloud: Option<Provider>, reason: &str) -> Error {
    let (name, again) = cloud.and_then(sign_in_of).map_or(("the cloud account", String::new()), |(n, a)| (n, format!(" ({a})")));
    Error::RefusedBeforeActing(format!(
        "Your sign-in to {name} has expired or is missing, so the lab wasn't changed. Sign in again{again}, then try again. ({})",
        reason.trim()
    ))
}

/// A step's tool that ran and failed (`reason`, what it said), as the command that failed.
fn tool_failed(step: &str, reason: &str) -> Error {
    Error::CommandFailed { command: format!("lab step {step}"), stderr: reason.to_owned() }
}

/// Another network's address block in the way (`failed`, the step that failed for it), said as
/// what to do.
fn pool_overlap(failed: &Error) -> Error {
    Error::Invalid(format!(
        "Another Docker network already uses the addresses this lab needs. Stop the other lab, or remove the network left behind (`docker network prune` removes the unused ones), then start this lab again. ({failed})"
    ))
}

/// A hypervisor as the machine check names it, by the runner's provider id (`qemu` is QEMU).
fn hypervisor_name(provider: &str) -> String {
    match provider {
        "qemu" => "QEMU".into(),
        "utm" => "UTM".into(),
        other => other.into(),
    }
}

#[cfg(test)]
mod tests;
