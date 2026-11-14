//! The launcher's errors: [`Error`] inside the app, [`Wire`] as the webview gets it, and the
//! [`Code`] the webview branches on and says in the player's language.

use serde::{Deserialize, Serialize};

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
        port: crate::runtime::port::HostPort,
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
    /// This computer's administrator turned the feature off: `setting` is the policy switch
    /// (`platform::policy::Key`) that did.
    TurnedOffByPolicy {
        /// The policy setting's name.
        setting: String,
    },
}

impl Code {
    /// The codes whose message is the error's own English text rather than a sentence of the
    /// webview's catalogue: anything without a code, and the refusals, which say in their own
    /// words what to put right. The webview reads this list from the generated bindings, which the
    /// tests write (`testing::ts_bindings`).
    #[cfg(test)]
    pub const OWN_MESSAGE: [Code; 3] = [Code::Other, Code::Refused, Code::RefusedBeforeActing];

    /// The English message, for the log and for a UI without this code's translation; for a code
    /// of `Code::OWN_MESSAGE`, what is said when an error has none of its own.
    fn message(&self) -> String {
        match self {
            Code::Other => "Something went wrong.".into(),
            Code::Refused => "The operation was refused before it changed anything.".into(),
            Code::RefusedBeforeActing => "The change was refused before it acted: nothing changed.".into(),
            Code::SignedOut => "You're signed out. Sign in again to continue.".into(),
            Code::SessionExpired => "Your session expired: you're signed out. Sign in again to continue.".into(),
            Code::LoginTimeout { minutes } => format!("Sign-in wasn't finished within {minutes} minutes, so it was cancelled. Try again."),
            Code::AlreadyInProgress => "Another operation on this lab is already in progress; wait for it to finish.".into(),
            Code::NoPasswordStored => "No password is stored for this host. Edit it and enter one.".into(),
            Code::SignInCancelled => "Sign-in cancelled.".into(),
            Code::SignInDenied => "Sign-in was declined at Cyber Auth, so you're still signed out. Sign in again and allow Cyber CTF to continue.".into(),
            Code::SignInCodeUnavailable => {
                "Signing in with a code isn't available yet. Copy the sign-in link into any browser instead, on this machine or another.".into()
            }
            Code::Unreachable { detail } => format!("Cyber CTF can't be reached ({detail}). Check your connection and try again."),
            Code::OfflineReady { detail } => {
                format!("Cyber CTF can't be reached ({detail}). This lab is installed here, so it can start offline, without saving progress.")
            }
            Code::PortInUse { port, holder } => {
                let clash = format!(
                    "Host port {port} is already in use on this machine (another lab or app holds it). Stop whatever is using it, then start the lab again."
                );
                // What holds it, as the lab page says it under the error (`labs.failure.portHeldBy`):
                // the catalogue has one form, the holder its own line.
                match holder {
                    Some(holder) => format!("{clash} Held by {holder}. Stop it, then start the lab again."),
                    None => clash,
                }
            }
            Code::DockerNotRunning => {
                "Docker isn't running. Start your container engine (Docker Desktop, or the docker service on Linux), then start the lab again.".into()
            }
            Code::DockerDenied { user } => format!(
                "Docker is running, but your account can't use it yet. Add yourself to the docker group in a terminal (sudo usermod -aG docker {user}), then log out and back in."
            ),
            Code::DockerDeniedWindows { user } => format!(
                "Docker is running, but your account can't use it yet. Add yourself to the docker-users group (in a terminal run as administrator: net localgroup docker-users \"{user}\" /add), then sign out and back in."
            ),
            Code::ReapNoHost => "No account is recorded for it, so the launcher can't destroy it. Remove its resources in your cloud console.".into(),
            Code::PlacementLockedHost { count, fields } => placement_locked(*count, "host", fields),
            Code::PlacementLockedAccount { count, fields } => placement_locked(*count, "account", fields),
            Code::LabRunningHere => "This lab is still running on this machine (or its machines are still there). Stop it, then try again.".into(),
            Code::LabRunningOn { host } => format!("This lab is still running on {host}. Stop it there, then try again."),
            Code::HostHasLabs { count } => {
                let (labs, them) = if *count == 1 { ("lab", "it") } else { ("labs", "them") };
                format!("{count} {labs} still running on this host. Stop {them} before removing the host.")
            }
            Code::ToolMissing { tool } => format!("`{tool}` is not installed or not on PATH"),
            Code::CommandFailed { command, stderr } => format!("`{command}` failed: {stderr}"),
            Code::DownloadStalled { .. } | Code::DownloadFailed { .. } | Code::BoxDownloadStalled { .. } | Code::BoxDownloadFailed { .. } => {
                download_message(self)
            }
            Code::PickCloudAccount => "This lab runs in your cloud account: pick one of your cloud accounts to start it on.".into(),
            Code::NoHypervisor => {
                "No hypervisor ready on this machine runs this lab. Install one from the Machine page, or run the lab on a server or in your cloud account."
                    .into()
            }
            Code::TurnedOffByPolicy { setting } => {
                format!("Your organization turned this off on this computer (policy setting {setting}). Ask your IT team if you need it.")
            }
            _ => run_message(self),
        }
    }
}

/// The server `server` refused the lab's saved credentials.
fn server_refused(server: &str) -> String {
    format!("Cyber CTF can't sign in to {server}: its token was revoked or its password changed. Update it in Servers.")
}

/// The message of a download that stalled or failed (`code`): the lab's images, or its VM box.
fn download_message(code: &Code) -> String {
    match code {
        Code::DownloadStalled { minutes } => format!(
            "Downloading the lab's images stopped making progress (nothing new for {minutes} minutes), so it was stopped. Check your internet connection, then try again: what was already downloaded is kept."
        ),
        Code::DownloadFailed { reason } => format!("Couldn't download the lab's images: {reason}. Check your internet connection, then try again."),
        Code::BoxDownloadStalled { minutes } => format!(
            "Downloading the lab's VM box stopped making progress (nothing new for {minutes} minutes), so it was stopped. Check your internet connection, then try again."
        ),
        Code::BoxDownloadFailed { reason } => format!("Couldn't download the lab's VM box: {reason}. Check your internet connection, then try again."),
        other => other.message(),
    }
}

/// The message of a lab's run that the runner failed (`code`): the server refusing its
/// credentials, a tool too old, Hyper-V's elevation refused, a lab not ready, machines gone or
/// none left, a lab that can't be parked, a step stopped for taking too long.
fn run_message(code: &Code) -> String {
    match code {
        Code::ServerAuthFailed { server } => server_refused(server),
        Code::ToolTooOld { tool, found, required } => {
            format!("This lab needs {tool} {required} or later, and this computer has {found}. Update it, then start the lab again.")
        }
        Code::ElevationFailed { reason } => {
            format!("Hyper-V needs administrator rights, and they couldn't be had ({reason}). Start the lab again and accept the prompt.")
        }
        Code::LabNotReady { detail } => format!("The lab started, but didn't become ready: {detail}"),
        Code::MachinesGone { machines } => format!(
            "Some of this lab's machines are gone ({machines}): they were removed outside Cyber CTF, so it can't resume as it was. Start it again for a fresh copy."
        ),
        Code::NothingToResume => "Nothing is left of this lab to resume (its stopped containers or VMs are gone). Start it again for a fresh copy.".into(),
        Code::NotParkable => "This lab can't be paused or shut down. Stop it instead.".into(),
        step => step_stopped(step),
    }
}

/// The message of a lab's step stopped for taking too long (`code`), with what to do then.
fn step_stopped(code: &Code) -> String {
    const AGAIN: &str = "Start the lab again; if it keeps happening, check the network and the resources of Docker, the hypervisor or the server.";
    match code {
        Code::StepStalled { step, minutes, last_line } => {
            format!(
                "The lab's `{step}` step showed no progress for {minutes} minutes, so it was stopped. Its last output was: \u{201c}{last_line}\u{201d}. {AGAIN}"
            )
        }
        Code::StepSilent { step, minutes } => format!("The lab's `{step}` step printed nothing for {minutes} minutes, so it was stopped. {AGAIN}"),
        Code::StepCeiling { step, hours } => {
            format!("The lab's `{step}` step was still working after {hours} hours, the longest any step may run, so it was stopped. {AGAIN}")
        }
        Code::StepTimedOut { step, minutes } => format!("The lab's `{step}` step ran into its {minutes}-minute limit, so it was stopped. {AGAIN}"),
        // The codes `Code::message` words itself never reach here.
        _ => format!("The lab's step was stopped. {AGAIN}"),
    }
}

/// "2 labs are still on this account: stop them before changing its region."
fn placement_locked(count: u64, what: &str, fields: &str) -> String {
    let (labs, them) = if count == 1 { ("lab is", "it") } else { ("labs are", "them") };
    format!("{count} {labs} still on this {what}: stop {them} before changing its {fields}.")
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
    /// A lab's lock couldn't be taken (its file can't be opened or locked): the operation stops
    /// there rather than run unlocked.
    #[error("Couldn't lock the lab: {0}")]
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
