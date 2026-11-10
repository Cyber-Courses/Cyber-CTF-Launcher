//! Asking the player on this machine before a website launch runs: what the prompt shows (the
//! lab, where it would run, whether it has privileged machines), the wait for an answer, and the
//! answer from the UI. A launch the policy refuses is never asked about; one nobody approves in
//! time never runs.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::oneshot;

use super::Deps;
use crate::error::{Error, Result};
use crate::labs::Launch;
use crate::platform::AppDirs;
use crate::runtime::server;

/// The prompt that asks the player to confirm a website launch on this machine. The app shell
/// shows it in the webview and brings the window forward; unit tests record it.
pub type PromptLaunch = dyn Fn(&LaunchRequest) + Send + Sync;

/// A website launch waiting for the player's answer on this machine: what the prompt shows.
#[derive(Clone, Debug, PartialEq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LaunchConfirmRequest")]
pub struct LaunchRequest {
    session_id: String,
    /// The lab's own name (its id when it has none).
    lab_name: String,
    repository: String,
    commit: String,
    /// Where it would run.
    place: LaunchPlace,
    /// The server or cloud account's name; empty for this machine.
    target: String,
    /// Whether the lab has a machine with kernel-level access (`privileged: true`).
    privileged: bool,
    /// When the agent stops waiting for an answer (Unix time, in milliseconds): an answer after it
    /// starts nothing. Set when the prompt is shown.
    expires_at_ms: u64,
}

/// Where a website launch would run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LaunchConfirmPlace")]
pub enum LaunchPlace {
    /// On this machine.
    Here,
    /// On one of the player's servers (Proxmox, ESXi).
    Server,
    /// On one of the player's cloud accounts: it costs money.
    Cloud,
}

/// Launch confirmations awaiting the user's answer, keyed by session id. The agent inserts a
/// sender and awaits it; `confirm_launch` (from the UI) resolves it. A panic elsewhere while it
/// was held leaves the map as it was: it is used on.
pub(super) fn pending_confirmations() -> MutexGuard<'static, HashMap<String, oneshot::Sender<bool>>> {
    static P: OnceLock<Mutex<HashMap<String, oneshot::Sender<bool>>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap_or_else(PoisonError::into_inner)
}

/// The user's answer to a launch confirmation, from the UI. Whether it reached the waiting launch:
/// false when the launch no longer waits (its time ran out, or it was answered already), so the UI
/// says the answer started nothing.
pub fn confirm_launch(session_id: &str, approve: bool) -> bool {
    let waiting = pending_confirmations().remove(session_id);
    let answered = waiting.is_some_and(|tx| tx.send(approve).is_ok());
    if !answered {
        log::info!("an answer to website launch {session_id} came when it no longer waited for one");
    }
    answered
}

/// Asks the user to confirm `request` on this machine, by the prompt the UI shows, and waits for
/// the answer. A timeout counts as a decline, so a missed prompt doesn't hang the session, and
/// nothing a website asked for runs unattended.
pub(super) async fn ask(deps: &Deps, request: &LaunchRequest) -> bool {
    let (tx, rx) = oneshot::channel();
    pending_confirmations().insert(request.session_id.clone(), tx);
    (deps.prompt_launch)(&LaunchRequest { expires_at_ms: unix_ms_after(CONFIRM_WAIT), ..request.clone() });
    let approved = await_answer(rx, CONFIRM_WAIT).await;
    pending_confirmations().remove(&request.session_id);
    approved
}

/// The Unix time, in milliseconds, `wait` from now.
fn unix_ms_after(wait: Duration) -> u64 {
    let at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default() + wait;
    u64::try_from(at.as_millis()).unwrap_or(u64::MAX)
}

/// How long a launch prompt waits for the player.
pub(super) const CONFIRM_WAIT: Duration = Duration::from_secs(180);

/// The player's answer on `rx`: only an explicit approval within `wait` counts; a decline, a
/// dropped prompt or no answer in time is a no.
pub(super) async fn await_answer(rx: oneshot::Receiver<bool>, wait: Duration) -> bool {
    matches!(tokio::time::timeout(wait, rx).await, Ok(Ok(true)))
}

/// What the prompt for the claimed launch shows: the lab (its spec, as the launcher reads it
/// before installing: `runtime::lab::spec_at`), and where it would run (`host`: none for this
/// machine). The policy is applied first: a launch it refuses is never asked about.
pub(super) async fn launch_request(dirs: &AppDirs, session_id: &str, launch: &Launch, host: Option<&str>) -> Result<LaunchRequest> {
    let Launch { lab_id, repository, commit, runtime, .. } = launch.clone();
    let spec = crate::runtime::lab::spec_at(dirs, &lab_id, crate::labs::source(&repository, &commit)).await?;
    let privileged = crate::runtime::lab::is_privileged(&spec);
    crate::runtime::policy_allows(dirs, runtime, host, || Ok(privileged))?;
    let (place, target) = match host {
        None => (LaunchPlace::Here, String::new()),
        // Unreadable saved hosts are an error: a cloud launch must never read as a free one.
        Some(id) => {
            let cloud = server::host_provider(dirs, id)?.is_some_and(crate::runtime::providers::Provider::is_cloud);
            let name = server::host_name(dirs, id)?.unwrap_or_else(|| id.to_string());
            (if cloud { LaunchPlace::Cloud } else { LaunchPlace::Server }, name)
        }
    };
    let lab_name = if spec.name.trim().is_empty() { lab_id } else { spec.name.clone() };
    Ok(LaunchRequest { session_id: session_id.to_string(), lab_name, repository, commit, place, target, privileged, expires_at_ms: 0 })
}

/// A website launch runs only once the player approved it on this machine: whatever runs here,
/// on their servers or on their cloud accounts was started by someone at this computer.
pub(super) async fn confirm_here(deps: &Deps, request: &LaunchRequest) -> Result<()> {
    if ask(deps, request).await {
        return Ok(());
    }
    let where_ = if request.place == LaunchPlace::Here { "this machine".to_string() } else { request.target.clone() };
    Err(Error::Refused(format!("{} wasn't approved on this machine, so it didn't start on {where_}.", request.lab_name)))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use super::super::tests::{COMMIT, claim_of, test_deps};
    use super::*;
    use crate::platform::policy::{self, Key};

    /// A lab spec named "Web Shop", its one machine privileged or not.
    fn spec(privileged: bool) -> String {
        format!(
            "version: 1\nname: Web Shop\nnetworks:\n  lab: {{ cidr: 10.30.0.0/24 }}\nmachines:\n  web:\n    networks: {{ lab: 10 }}\n    privileged: {privileged}\n    app: {{ image: nginx:1.27 }}\n"
        )
    }

    /// Answers the prompt of session `sid` with `approve` as soon as it is asked.
    fn answer(sid: &'static str, approve: bool) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                if pending_confirmations().contains_key(sid) {
                    assert!(confirm_launch(sid, approve));
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
    }

    #[tokio::test]
    async fn every_website_launch_is_asked_about_here_with_the_lab_and_where_it_runs() {
        // Regression: only a launch onto a cloud account was asked about; one here or on a
        // server ran unattended, whatever the website sent.
        let t = crate::testing::test_app::TestApp::new();
        let dirs = t.dirs();
        let id = "agent-ask";
        t.lab(id, &[(".cyberctf-commit", COMMIT), ("isoloom.yml", &spec(true))]);
        let az = t.host("az1", "azure", json!({}));
        let pve = t.host("pve1", "proxmox", json!({}));
        let claim = claim_of(id);
        for (host, place, target) in
            [(None, LaunchPlace::Here, ""), (Some(pve.as_str()), LaunchPlace::Server, "Host pve1"), (Some(az.as_str()), LaunchPlace::Cloud, "Host az1")]
        {
            let request = launch_request(&dirs, "s-1", &claim.launch, host).await.unwrap();
            assert_eq!((request.place, request.target.as_str()), (place, target));
            assert_eq!((request.lab_name.as_str(), request.privileged), ("Web Shop", true), "the privileged machine is said");
            assert_eq!((request.repository.as_str(), request.commit.as_str()), ("CyberCTF/web", COMMIT));
        }
        t.lab(id, &[("isoloom.yml", &spec(false))]);
        assert!(!launch_request(&dirs, "s-1", &claim.launch, None).await.unwrap().privileged);
    }

    #[tokio::test]
    async fn a_website_launch_runs_only_on_an_approval_given_here() {
        let t = crate::testing::test_app::TestApp::new();
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let deps = test_deps(&t, prompts.clone());
        let id = "agent-approve";
        t.lab(id, &[(".cyberctf-commit", COMMIT), ("isoloom.yml", &spec(false))]);
        for (sid, approve) in [("s-here-yes", true), ("s-here-no", false)] {
            let request = launch_request(deps.dirs(), sid, &claim_of(id).launch, None).await.unwrap();
            let answered = answer(sid, approve);
            let r = confirm_here(&deps, &request).await;
            answered.await.unwrap();
            if approve {
                r.unwrap();
            } else {
                let err = r.unwrap_err();
                assert_eq!(err.to_string(), "Web Shop wasn't approved on this machine, so it didn't start on this machine.");
                assert!(err.is_refusal());
            }
        }
        let shown: Vec<String> = prompts.lock().unwrap().iter().map(|r| r.session_id.clone()).collect();
        assert_eq!(shown, ["s-here-yes", "s-here-no"], "each launch was shown here");
        // What the webview gets: with when the wait for it ends, so the prompt says when it did.
        let request = prompts.lock().unwrap()[0].clone();
        let expires = request.expires_at_ms;
        let in_full = unix_ms_after(CONFIRM_WAIT);
        assert!((in_full - 5_000..=in_full).contains(&expires), "{expires} is not about {} s from now", CONFIRM_WAIT.as_secs());
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({ "sessionId": "s-here-yes", "labName": "Web Shop", "repository": "CyberCTF/web", "commit": COMMIT, "place": "here", "target": "", "privileged": false, "expiresAtMs": expires })
        );
    }

    #[tokio::test]
    async fn the_policy_refuses_a_website_launch_before_it_is_asked_about() {
        let t = crate::testing::test_app::TestApp::new();
        let dirs = t.dirs();
        let id = "agent-policy";
        t.lab(id, &[(".cyberctf-commit", COMMIT), ("isoloom.yml", &spec(true))]);
        let az = t.host("az1", "azure", json!({}));
        {
            let _policy = policy::Override::with(&[Key::RefusePrivilegedLabs]);
            let err = launch_request(&dirs, "s-1", &claim_of(id).launch, None).await.unwrap_err();
            assert_eq!(err.code(), crate::error::Code::TurnedOffByPolicy { setting: "RefusePrivilegedLabs".into() });
        }
        let _policy = policy::Override::with(&[Key::DisableCloudLaunches]);
        let err = launch_request(&dirs, "s-1", &claim_of(id).launch, Some(&az)).await.unwrap_err();
        assert_eq!(err.code(), crate::error::Code::TurnedOffByPolicy { setting: "DisableCloudLaunches".into() });
        assert_eq!(launch_request(&dirs, "s-1", &claim_of(id).launch, None).await.unwrap().place, LaunchPlace::Here, "here is still allowed");
    }

    #[test]
    fn confirm_launch_answers_the_waiting_prompt_once_and_says_so() {
        // Regression: an answer to no waiting launch (late, or twice) vanished, and the prompt
        // closed as if it had been taken.
        let (tx, mut rx) = oneshot::channel();
        pending_confirmations().insert("s-confirm".into(), tx);
        assert!(!confirm_launch("s-other", true), "no such launch waits");
        assert!(rx.try_recv().is_err());
        assert!(confirm_launch("s-confirm", true));
        assert!(rx.try_recv().unwrap());
        assert!(!pending_confirmations().contains_key("s-confirm"));
        assert!(!confirm_launch("s-confirm", true), "answered already");
        // The launch stopped waiting (its receiver gone) just before the answer came.
        let (tx, rx) = oneshot::channel();
        pending_confirmations().insert("s-late".into(), tx);
        drop(rx);
        assert!(!confirm_launch("s-late", true));
    }

    #[test]
    fn a_panic_while_the_waiting_launches_were_held_doesnt_stop_answers() {
        let poisoned = std::thread::spawn(|| {
            let _held = pending_confirmations();
            panic!("held while panicking");
        });
        assert!(poisoned.join().is_err());
        let (tx, mut rx) = oneshot::channel();
        pending_confirmations().insert("s-poisoned".into(), tx);
        assert!(confirm_launch("s-poisoned", false));
        assert!(!rx.try_recv().unwrap());
    }

    #[tokio::test]
    async fn a_launch_runs_only_on_an_explicit_yes_in_time() {
        let (tx, rx) = oneshot::channel();
        tx.send(true).unwrap();
        assert!(await_answer(rx, Duration::from_secs(1)).await);
        let (tx, rx) = oneshot::channel();
        tx.send(false).unwrap();
        assert!(!await_answer(rx, Duration::from_secs(1)).await);
        // The prompt went away (sender dropped): a no.
        let (tx, rx) = oneshot::channel::<bool>();
        drop(tx);
        assert!(!await_answer(rx, Duration::from_secs(1)).await);
        // No answer in time: a no, so a missed prompt never spends money.
        let (_tx, rx) = oneshot::channel::<bool>();
        assert!(!await_answer(rx, Duration::from_millis(20)).await);
        assert_eq!(CONFIRM_WAIT, Duration::from_secs(180));
    }
}
