//! The launcher's errors: [`Error`] inside the app, [`Wire`] as the webview gets it, and the
//! [`Code`] the webview branches on and says in the player's language.

use serde::{Deserialize, Serialize};

use crate::english;

/// What went wrong, as the UI knows it: a code it can branch on and say in the player's
/// language, with the values its message needs. `Other` and the refusals are the codes the UI
/// shows as the error's own (English) message (`Code::OWN_MESSAGE`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "code", content = "args", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(rename = "ErrorCode")]
pub enum Code {
    /// Anything the UI has no code for: it shows the error's own (English) message.
    Other,
    /// An operation refused before it changed anything ([`Error::Refused`]), in its own words.
    Refused,
    /// A change of a lab refused before it acted, for something to put right first
    /// ([`Error::RefusedBeforeActing`]), in its own words.
    RefusedBeforeActing,
    /// A call that needs the account, with no session kept.
    SignedOut,
    /// The session can't be renewed any more: signed out.
    SessionExpired,
    /// The browser sign-in wasn't finished in time.
    LoginTimeout {
        /// How long the sign-in waited, in minutes.
        minutes: u64,
    },
    /// Another operation on the lab runs: this one was refused before it changed anything.
    AlreadyInProgress,
    /// A server host saved without its password.
    NoPasswordStored,
    /// A host port is taken: one a container lab publishes, or one a local VM forwards. `holder`
    /// is a short description of what holds it ("nginx (pid 4123)"), filled in on the failure
    /// path when it can be found, so the player knows what to stop.
    PortInUse {
        /// The host port that is taken.
        #[ts(type = "number")]
        port: isothrum_runner::HostPort,
        /// What holds the port, when it could be found.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        holder: Option<String>,
    },
    /// The player cancelled the sign-in (or started another one): not a failure to show.
    SignInCancelled,
    /// The player declined the sign-in at cyber-auth (its consent screen, or the code's page).
    SignInDenied,
    /// cyber-auth doesn't let the launcher sign in with a code (the device grant isn't enabled
    /// on its OAuth client): the browser sign-in still works.
    SignInCodeUnavailable,
    /// CyberBackend or the sign-in service couldn't be reached (no network, an outage, an
    /// answer that isn't the API's): trying again later may work. `detail` is the technical why.
    Unreachable {
        /// The technical why: the transport's error or what answered instead.
        detail: String,
    },
    /// Cyber CTF couldn't be reached, and the lab is installed here, so it can start offline.
    OfflineReady {
        /// The technical why, as for `Unreachable`.
        detail: String,
    },
    /// The container engine isn't running.
    DockerNotRunning,
    /// Docker runs, but this account isn't allowed to use it.
    DockerDenied {
        /// The account's user name, for the `usermod` command the message gives.
        user: String,
    },
    /// The same on Windows: the engine's pipe belongs to the `docker-users` group.
    DockerDeniedWindows {
        /// The account's user name, for the `net localgroup` command the message gives.
        user: String,
    },
    /// Docker's install stopped, changing nothing: this Linux distribution isn't Debian, Ubuntu or
    /// one built on them, so the player installs Docker Engine themselves.
    DockerOtherDistribution,
    /// Hyper-V can't be turned on: this edition of Windows (Home) has none.
    HypervHomeEdition,
    /// The auto-stop couldn't destroy a lab: no account is recorded for its run. (Its other
    /// failures are the runner's, with their own codes.)
    ReapNoHost,
    /// A server host's placement (provider, address, node…) can't change while labs are on it.
    PlacementLockedHost {
        /// How many labs are on the host.
        count: u64,
        /// The placement fields the edit would change, as the message lists them.
        fields: String,
    },
    /// The same for a cloud account (its region, subscription, profile…).
    PlacementLockedAccount {
        /// How many labs are on the account.
        count: u64,
        /// The placement fields the edit would change, as the message lists them.
        fields: String,
    },
    /// Something of the lab is still up on this machine (running, or its machines left over): it
    /// can't be replaced, nor started somewhere else, until it is stopped.
    LabRunningHere,
    /// The same on the saved server or cloud account `host` (its name).
    LabRunningOn {
        /// The saved host's name.
        host: String,
    },
    /// A host can't be removed while labs run on it.
    HostHasLabs {
        /// How many labs run on the host.
        count: u64,
    },
    /// A tool the operation runs isn't installed.
    ToolMissing {
        /// The tool's command name (`docker`, `vagrant`, …).
        tool: String,
    },
    /// A command ran and failed.
    CommandFailed {
        /// The command line that failed.
        command: String,
        /// What the command wrote to its standard error.
        stderr: String,
    },
    /// Downloading the lab's images, before its containers start, showed no progress for
    /// `minutes`, so it was stopped.
    DownloadStalled {
        /// How long it went without progress, in minutes.
        minutes: u64,
    },
    /// The lab's images couldn't be downloaded, before its containers start.
    DownloadFailed {
        /// Why, in the container engine's words.
        reason: String,
    },
    /// Downloading the lab's VM box (Vagrant's), before its VMs start, showed no progress for
    /// `minutes`, so it was stopped.
    BoxDownloadStalled {
        /// How long it went without progress, in minutes.
        minutes: u64,
    },
    /// The lab's VM box (Vagrant's) couldn't be downloaded, before its VMs start.
    BoxDownloadFailed {
        /// Why, in Vagrant's words.
        reason: String,
    },
    /// A tool the lab needs answers at a version older than the one it needs.
    ToolTooOld {
        /// The tool, as the player knows it (`Vagrant`).
        tool: String,
        /// The version this computer has.
        found: String,
        /// The oldest version the lab takes.
        required: String,
    },
    /// Hyper-V's commands need administrator rights, and they couldn't be had: the prompt
    /// declined or unanswered, the elevated session ended.
    ElevationFailed {
        /// Why, in the runner's words.
        reason: String,
    },
    /// The lab started, but a check that it is ready didn't hold.
    LabNotReady {
        /// What didn't hold, in the runner's words.
        detail: String,
    },
    /// A resume found some of the lab's machines removed outside the app.
    MachinesGone {
        /// The machines gone, comma separated.
        machines: String,
    },
    /// A resume found nothing of the lab left (its stopped containers or VMs are gone).
    NothingToResume,
    /// The lab can't be paused or shut down: it has no way to.
    NotParkable,
    /// A step of the lab printed lines but none new for `minutes`, so it was stopped.
    StepStalled {
        /// The step (`up`).
        step: String,
        /// How long it went without progress, in minutes.
        minutes: u64,
        /// The last line it printed.
        last_line: String,
    },
    /// A step of the lab printed nothing for `minutes`, so it was stopped.
    StepSilent {
        /// The step (`up`).
        step: String,
        /// How long it printed nothing, in minutes.
        minutes: u64,
    },
    /// A step of the lab still worked after the longest any step may run, so it was stopped.
    StepCeiling {
        /// The step (`up`).
        step: String,
        /// The longest a step may run, in hours.
        hours: u64,
    },
    /// A step of the lab ran into its time limit, so it was stopped.
    StepTimedOut {
        /// The step (`up`).
        step: String,
        /// Its limit, in minutes.
        minutes: u64,
    },
    /// The server the lab runs on (Proxmox, ESXi) refused its saved credentials: its API token
    /// was revoked, or its password changed.
    ServerAuthFailed {
        /// The server, by its name in Servers.
        server: String,
    },
    /// A lab that runs only in a cloud account was started with none picked.
    PickCloudAccount,
    /// A VM lab was started on this machine with no hypervisor here to run it on.
    NoHypervisor,
    /// `hypervisor` (libvirt, QEMU) can't share a folder of this computer with a VM (the attack
    /// box's shared folder): no virtiofsd it finds here (Linux), which its install brings.
    VirtiofsdMissing {
        /// The hypervisor's name ("libvirt", "QEMU").
        hypervisor: String,
    },
    /// macOS doesn't let this app drive `hypervisor` (UTM) through Apple events (`-1743`, the
    /// Automation permission). No wait fixes it: the permission in System Settings.
    HypervisorNotPermitted {
        /// The hypervisor's name ("UTM").
        hypervisor: String,
    },
    /// macOS never approved `hypervisor`'s app (UTM): downloaded, never opened from Finder, it
    /// answered the VM's start with the system's "Operation not permitted", refused its own VM
    /// launcher. No wait fixes it: the app opened once from Finder, or reinstalled.
    HypervisorNotApproved {
        /// The hypervisor's name ("UTM").
        hypervisor: String,
    },
    /// This computer's administrator turned the feature off: `setting` is the policy switch
    /// (`platform::policy::Key`) that did.
    TurnedOffByPolicy {
        /// The policy setting's name.
        setting: String,
    },
    /// A server with a self-signed certificate whose fingerprint the learner hasn't confirmed
    /// yet: nothing was sent to it.
    TlsUnconfirmed {
        /// The server, by its name in Servers.
        server: String,
        /// The SHA-256 fingerprint of the certificate it offered.
        presented: String,
    },
    /// A server offered another certificate than the one the learner confirmed: nothing was sent
    /// to it.
    TlsChanged {
        /// The server, by its name in Servers.
        server: String,
        /// The fingerprint confirmed for it.
        pinned: String,
        /// The fingerprint of the certificate it offered.
        presented: String,
    },
    /// A server node's SSH host key isn't one the learner confirmed (none yet, or another one):
    /// the connection was refused.
    SshHostKeyRefused {
        /// The node's address.
        host: String,
    },
    /// The runner service, which the app hosts and every lab operation goes through, can't be
    /// reached: restarting the app starts it again. `detail` is the technical why.
    RunnerUnreachable {
        /// Why it can't be reached.
        detail: String,
    },
}

impl Code {
    /// The codes whose message is the error's own English text rather than a sentence of the
    /// webview's catalogue: anything without a code, and the refusals, which say in their own
    /// words what to put right. The webview reads this list from the generated bindings, which the
    /// tests write (`testing::ts_bindings`).
    #[cfg(test)]
    pub const OWN_MESSAGE: [Code; 3] = [Code::Other, Code::Refused, Code::RefusedBeforeActing];

    /// The English message, for the log and for a UI without this code's translation: the English
    /// catalogue's sentence (`errors.codes.*`, what the English UI shows), then a port clash's
    /// holder as the lab page says it under the error (`labs.failure.portHeldBy`). For a code of
    /// `Code::OWN_MESSAGE`, which the catalogue doesn't have, what is said when an error has none
    /// of its own.
    fn message(&self) -> String {
        match self {
            Code::Other => "Something went wrong.".into(),
            Code::Refused => "The operation was refused before it changed anything.".into(),
            Code::RefusedBeforeActing => "The change was refused before it acted: nothing changed.".into(),
            Code::PortInUse { holder: Some(holder), .. } => {
                let held = english::sentence(&english::LABS["failure"], "portHeldBy", &serde_json::json!({ "holder": holder }));
                format!("{} {held}", english::coded(&english::ERRORS["codes"], self))
            }
            _ => english::coded(&english::ERRORS["codes"], self),
        }
    }
}

/// A failure of the launcher's own code, as its functions return it; the webview gets it as a
/// [`Wire`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A request or a value the operation can't use (a bad argument, a file that isn't what it
    /// should be): its message says what.
    #[error("{0}")]
    Invalid(String),
    /// An operation refused before it changed anything (a lab still up elsewhere, an update of a
    /// running lab): not a failed deploy, so the lab's last verdict stays what it was.
    #[error("{0}")]
    Refused(String),
    /// A change of a lab refused before it acted, for something to put right first (the run isn't
    /// up, a value it needs is missing or unusable, a cloud sign-in ran out): nothing changed, so
    /// what was written for it can be put back. Unlike [`Error::Refused`], a launch refused so
    /// failed: its verdict says so.
    #[error("{0}")]
    RefusedBeforeActing(String),
    /// The player cancelled sign-in (or started another one): not a failure to show.
    #[error("{}", Code::SignInCancelled.message())]
    Cancelled,
    /// CyberBackend or the sign-in service couldn't be reached (no network, an outage, an answer
    /// that isn't the API's): unlike a refusal, trying again later may work.
    #[error("{0}")]
    Unreachable(String),
    /// A failure the UI knows by its code (see [`Code`]).
    #[error("{message}")]
    Coded {
        /// The code the UI branches on.
        code: Code,
        /// The code's English message.
        message: String,
    },
    /// A tool the operation runs isn't installed, or isn't on `PATH`.
    #[error("{}", Code::ToolMissing { tool: (*tool).into() }.message())]
    ToolMissing {
        /// The tool's command name.
        tool: &'static str,
    },
    /// A command ran and failed.
    #[error("{}", Code::CommandFailed { command: command.clone(), stderr: stderr.clone() }.message())]
    CommandFailed {
        /// The command line that failed.
        command: String,
        /// What the command wrote to its standard error.
        stderr: String,
    },
    /// A lock across processes couldn't be taken (its file can't be opened or locked): what it
    /// guards stops there rather than run unlocked.
    #[error("Couldn't take a lock: {0}")]
    LockUnavailable(#[source] std::io::Error),
    /// An I/O failure, with the system's message.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    /// The error with this code, and the code's English message.
    pub fn coded(code: Code) -> Self {
        Error::Coded { message: code.message(), code }
    }

    /// Whether the operation was refused before it changed anything (the lab still up where it
    /// would be replaced or started again, a policy that turns it off, a start with nowhere to
    /// run): the lab's last verdict stays what it was, and a failed start has nothing of its own
    /// to undo.
    pub fn is_refusal(&self) -> bool {
        matches!(
            self,
            Error::Refused(_)
                | Error::Coded {
                    code: Code::LabRunningHere | Code::LabRunningOn { .. } | Code::TurnedOffByPolicy { .. } | Code::PickCloudAccount | Code::NoHypervisor,
                    ..
                }
        )
    }

    /// The code the UI knows this error by.
    pub fn code(&self) -> Code {
        match self {
            Error::Coded { code, .. } => code.clone(),
            Error::ToolMissing { tool } => Code::ToolMissing { tool: (*tool).into() },
            Error::CommandFailed { command, stderr } => Code::CommandFailed { command: command.clone(), stderr: stderr.clone() },
            Error::Cancelled => Code::SignInCancelled,
            Error::Unreachable(detail) => Code::Unreachable { detail: detail.clone() },
            Error::Refused(_) => Code::Refused,
            Error::RefusedBeforeActing(_) => Code::RefusedBeforeActing,
            Error::Invalid(_) | Error::LockUnavailable(_) | Error::Io(_) => Code::Other,
        }
    }

    /// The error as the webview gets it (and as a deploy worker hands it back to the app).
    pub fn to_wire(&self) -> Wire {
        Wire { code: self.code(), message: self.to_string() }
    }
}

/// An error as the webview receives it: `{ code, args?, message }`. `message` is English and
/// always there, for the log and for the codes the UI says as they come (`Code::OWN_MESSAGE`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(rename = "CommandError")]
pub struct Wire {
    /// The code the UI branches on, with its values (`code` and `args` on the wire).
    #[serde(flatten)]
    pub code: Code,
    /// The English message.
    pub message: String,
}

impl From<Wire> for Error {
    fn from(w: Wire) -> Self {
        match w.code {
            Code::Other => Error::Invalid(w.message),
            Code::Refused => Error::Refused(w.message),
            Code::RefusedBeforeActing => Error::RefusedBeforeActing(w.message),
            Code::SignInCancelled => Error::Cancelled,
            Code::Unreachable { .. } => Error::Unreachable(w.message),
            code => Error::Coded { code, message: w.message },
        }
    }
}

impl Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        self.to_wire().serialize(s)
    }
}

/// The launcher's result: an [`Error`] on failure.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests;
