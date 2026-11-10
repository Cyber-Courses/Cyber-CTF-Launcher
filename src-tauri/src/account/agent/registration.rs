//! This machine's registration with the website, for the signed-in account: registering (again
//! for another account), the heartbeat that keeps it online, and what the UI says of it.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde_json::json;

use super::identity::{app_version, arch, capabilities, install_id, machine_name, os_label};
use super::{Deps, poll_once};
use crate::account::{api, auth};
use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::platform::policy::{self, Key};
use crate::runtime::server;

/// Upserts this machine as the player's launcher agent and returns the server agent id.
async fn register(dirs: &AppDirs) -> Result<String> {
    let data = api::graphql(
        "mutation ($i: ID!, $n: String!, $c: [String!]!, $a: String, $t: [LaunchTargetInput!], $v: String, $o: String) { registerLauncher(installId: $i, name: $n, capabilities: $c, arch: $a, targets: $t, version: $v, os: $o) { id } }",
        json!({ "i": install_id(dirs)?, "n": machine_name(), "c": capabilities(), "a": arch(), "t": server::launch_targets(dirs)?, "v": app_version(), "o": os_label() }),
        true,
    )
    .await?;
    data["registerLauncher"]["id"].as_str().map(String::from).ok_or_else(|| Error::Invalid("registerLauncher returned no id".into()))
}

/// Keep-alive, re-reporting the targets so hosts added since registering show on the website, and
/// the app version and OS, so an update installed since registering shows too.
async fn heartbeat(dirs: &AppDirs, agent_id: &str) -> Result<()> {
    api::graphql(
        "mutation ($id: ID!, $t: [LaunchTargetInput!], $v: String, $o: String) { launcherHeartbeat(agentId: $id, targets: $t, version: $v, os: $o) { id } }",
        json!({ "id": agent_id, "t": server::launch_targets(dirs)?, "v": app_version(), "o": os_label() }),
        true,
    )
    .await
    .map(|_| ())
}

/// The agent this machine is registered as, and the account it was registered for.
#[derive(Clone, Debug, PartialEq)]
struct Registration {
    user: String,
    agent_id: String,
}

/// What the agent loop does next, from whom it is registered for and who is signed in now.
#[derive(Debug, PartialEq)]
enum Next {
    /// Signed out: no agent (its id is forgotten), wait for a sign-in.
    SignedOut,
    /// The session can't be read now (the keychain locked): the registration is kept, and the
    /// loop waits. Read as signed out, it dropped the agent until the next sign-in.
    Unreadable,
    /// Signed in as someone it isn't registered for (first sign-in, or another account).
    Register,
    /// Registered for the signed-in account: heartbeat and poll.
    Run,
}

/// `user`: who is signed in (see `auth::signed_in_user`).
fn next_step(current: Option<&Registration>, user: std::result::Result<Option<&str>, &Error>) -> Next {
    match (current, user) {
        (_, Err(_)) => Next::Unreadable,
        (_, Ok(None)) => Next::SignedOut,
        (Some(r), Ok(Some(u))) if r.user == u => Next::Run,
        _ => Next::Register,
    }
}

/// The agent this machine is registered as now, for the signed-in account: none while signed
/// out, and while it registers (again, for another account). What `agent_info` says of it.
static REGISTRATION: Mutex<Option<Registration>> = Mutex::new(None);

/// The registration in `current`, poisoned or not (it is only ever replaced whole).
fn registration(current: &Mutex<Option<Registration>>) -> MutexGuard<'_, Option<Registration>> {
    current.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The current registration's agent id, if any.
fn agent_id_of(current: &Mutex<Option<Registration>>) -> Option<String> {
    registration(current).as_ref().map(|r| r.agent_id.clone())
}

/// The agent to heartbeat for: the current registration's, while it is for the account signed in
/// (`user`). None while signed out, or signed in as another account (it registers again first):
/// the heartbeat never says the agent of one account is online with another's token.
fn heartbeat_agent(current: &Mutex<Option<Registration>>, user: Option<&str>) -> Option<String> {
    registration(current).as_ref().filter(|r| Some(r.user.as_str()) == user).map(|r| r.agent_id.clone())
}

/// Heartbeats (~30s) for the current registration, on its own task: claiming and running a lab
/// can block for a long time (a cloud launch waits on terraform apply, up to tens of minutes),
/// and the machine must keep reporting that it's online for the whole launch, not drop offline
/// while busy.
fn spawn_heartbeat(dirs: AppDirs, current: &'static Mutex<Option<Registration>>) {
    tokio::spawn(async move {
        loop {
            // A session that can't be read is logged by the agent loop: no heartbeat then.
            if let Ok(user) = auth::signed_in_user().await
                && let Some(agent_id) = heartbeat_agent(current, user.as_deref())
                && auth::access_token().await.is_ok()
                && let Err(e) = heartbeat(&dirs, &agent_id).await
            {
                log::warn!("launcherHeartbeat failed: {e}");
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

/// Runs the background agent, unless the policy turns it off: waits for login, registers, then
/// heartbeats (~30s) and polls for launches (~6s) for as long as the app runs. Signing out forgets the agent, and
/// signing in as another account registers again, so launches are never polled for (and
/// the machine never shows online for) an account that isn't signed in here. The app shell spawns
/// this on its runtime (see `crate::app`), with the effects (`Deps`) the agent reaches the player
/// through, so this loop itself is free of `tauri`.
pub async fn run(deps: Deps) {
    if policy::current().is_on(Key::DisableWebsiteAgent) {
        log::info!("the website agent is off (policy {}): this machine takes no launches from the website", Key::DisableWebsiteAgent.name());
        return;
    }
    let current = &REGISTRATION;
    spawn_heartbeat(deps.dirs().clone(), current);
    loop {
        let read = auth::signed_in_user().await;
        let registered = registration(current).clone();
        match next_step(registered.as_ref(), read.as_ref().map(Option::as_deref)) {
            Next::Unreadable => {
                if let Err(e) = &read {
                    log::warn!("the agent can't read the session, and waits with its registration kept: {e}");
                }
                tokio::time::sleep(Duration::from_secs(30)).await;
                continue;
            }
            Next::SignedOut => {
                *registration(current) = None;
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
            Next::Register => {
                // Not online for the account registered before while this one registers.
                *registration(current) = None;
                // No token yet, or registering failed: wait and start the cycle over.
                if register_step(&deps, current, read.ok().flatten()).await {
                    continue;
                }
            }
            Next::Run => {}
        }
        if let Some(agent_id) = agent_id_of(current)
            && auth::access_token().await.is_ok()
        {
            Box::pin(poll_once(&deps, &agent_id)).await;
        }
        tokio::time::sleep(Duration::from_secs(6)).await;
    }
}

/// Registers this machine for `user`, keeping its agent id in `current`. Returns whether the loop
/// should wait and start over — there was no access token yet, or `registerLauncher` failed — so a
/// poll is never made for an account this machine isn't registered as.
async fn register_step(deps: &Deps, current: &Mutex<Option<Registration>>, user: Option<String>) -> bool {
    if auth::access_token().await.is_err() {
        tokio::time::sleep(Duration::from_secs(5)).await;
        return true;
    }
    match register(deps.dirs()).await {
        Ok(agent_id) => {
            log::info!("launcher agent online: {agent_id}");
            *registration(current) = Some(Registration { user: user.unwrap_or_default(), agent_id });
            false
        }
        Err(e) => {
            log::warn!("registerLauncher failed, retrying: {e}");
            tokio::time::sleep(Duration::from_secs(15)).await;
            true
        }
    }
}

/// This machine's launcher-agent identity, for the UI ("running as ...").
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    install_id: String,
    name: String,
    arch: &'static str,
    capabilities: Vec<String>,
    /// Whether the website sees this machine, as of now.
    state: AgentState,
}

/// Whether the website sees this machine as somewhere to launch labs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentState {
    /// The administrator's policy turned the agent off (`DisableWebsiteAgent`).
    OffByPolicy,
    /// Not registered (yet) for the signed-in account: waiting for its first registration, or
    /// retrying one that failed.
    Registering,
    /// Registered for the signed-in account: it shows online and takes launches.
    Online,
}

/// The agent's state, from whether the policy turns it `off` and whether it is `registered`.
fn agent_state(off: bool, registered: bool) -> AgentState {
    match (off, registered) {
        (true, _) => AgentState::OffByPolicy,
        (false, false) => AgentState::Registering,
        (false, true) => AgentState::Online,
    }
}

/// This machine's identity, for the UI ("running as ..."), and whether the website sees it.
pub fn agent_info(dirs: &AppDirs) -> Result<AgentInfo> {
    let state = agent_state(policy::current().is_on(Key::DisableWebsiteAgent), registration(&REGISTRATION).is_some());
    Ok(AgentInfo { install_id: install_id(dirs)?, name: machine_name(), arch: arch(), capabilities: capabilities(), state })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::account::agent::tests::test_deps;

    #[test]
    fn the_agent_registers_again_for_another_account_and_forgets_it_on_sign_out() {
        // After signing out and in as someone else, the old agent id was still polled with.
        let a = Registration { user: "u1".into(), agent_id: "agent-1".into() };
        assert_eq!(next_step(None, Ok(None)), Next::SignedOut);
        assert_eq!(next_step(None, Ok(Some("u1"))), Next::Register);
        assert_eq!(next_step(Some(&a), Ok(Some("u1"))), Next::Run);
        assert_eq!(next_step(Some(&a), Ok(Some("u2"))), Next::Register);
        assert_eq!(next_step(Some(&a), Ok(None)), Next::SignedOut);
        // Regression: a session that couldn't be read (the keychain locked) read as signed out,
        // and the agent forgot its registration.
        assert_eq!(next_step(Some(&a), Err(&Error::Invalid("the keychain is locked".into()))), Next::Unreadable);
        let current = Mutex::new(Some(a));
        assert_eq!(agent_id_of(&current).as_deref(), Some("agent-1"));
        // Regression: during a long claim, a sign-in as another account sent the first account's
        // agent with the new account's token.
        assert_eq!(heartbeat_agent(&current, Some("u1")).as_deref(), Some("agent-1"));
        assert_eq!(heartbeat_agent(&current, Some("u2")), None);
        assert_eq!(heartbeat_agent(&current, None), None);
        *current.lock().unwrap() = None;
        assert_eq!(agent_id_of(&current), None);
    }

    #[test]
    fn the_agent_is_online_only_registered_and_never_against_the_policy() {
        // Regression: Settings showed every signed-in machine online, the policy's too.
        assert_eq!(agent_state(true, false), AgentState::OffByPolicy);
        assert_eq!(agent_state(true, true), AgentState::OffByPolicy);
        assert_eq!(agent_state(false, false), AgentState::Registering);
        assert_eq!(agent_state(false, true), AgentState::Online);
        let t = crate::testing::test_app::TestApp::new();
        let _policy = policy::Override::with(&[Key::DisableWebsiteAgent]);
        assert_eq!(serde_json::to_value(agent_info(&t.dirs()).unwrap()).unwrap()["state"], "offByPolicy");
    }

    #[test]
    fn this_machines_identity_is_its_install_id_name_and_arch() {
        let t = crate::testing::test_app::TestApp::new();
        let info = || serde_json::to_value(agent_info(&t.dirs()).unwrap()).unwrap();
        let first = info();
        assert_eq!(first["installId"].as_str().unwrap().len(), 32);
        assert_eq!(info()["installId"], first["installId"], "kept");
        let info = first;
        assert_eq!(info["arch"], arch());
        assert_eq!(info["capabilities"], json!(["docker"]));
    }

    #[tokio::test]
    async fn the_policy_keeps_the_agent_off() {
        // With the website agent off the loop returns at once: it never registers nor polls.
        let t = crate::testing::test_app::TestApp::new();
        let _policy = policy::Override::with(&[Key::DisableWebsiteAgent]);
        let ran = tokio::time::timeout(Duration::from_secs(5), Box::pin(run(test_deps(&t, Arc::default())))).await;
        assert!(ran.is_ok(), "the agent kept running");
    }
}
