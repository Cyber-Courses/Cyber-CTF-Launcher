//! Login with cyber-auth: OAuth authorization code + PKCE (RFC 7636) through the
//! system browser, back to the app by a `cyberctf://` link or a loopback redirect (RFC 8252,
//! see `config::scheme_redirect`). Tokens live in the OS keychain and never reach the webview;
//! API calls go through `api.rs`.

mod callback;
mod scheme;
mod store;
mod tokens;
mod waiting;

pub use scheme::deliver as deliver_link;

use std::future::Future;

use crate::AppHandle;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri_plugin_opener::OpenerExt;

use crate::app::config;
use crate::error::{Code, Error, Result};
use store::Session;
use tokens::{TokenResponse, code_challenge, now, random_b64, session_from, token_request};

const SCOPES: &str = "openid profile email offline_access";
/// An access token this close to expiry is refreshed before use.
const REFRESH_MARGIN_SECS: u64 = 60;

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    logged_in: bool,
    name: Option<String>,
    email: Option<String>,
    /// Why the session couldn't be read (the keychain is locked, its prompt was dismissed, or
    /// there is no Secret Service), when that is why the player shows as signed out.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    keychain_error: Option<String>,
}

/// The session, or the error a call needing the account returns: signed out, or why the
/// keychain couldn't be read.
async fn session_or_signed_out() -> Result<Session> {
    session_from_read(store::read().await)
}

/// A session read as the session, or the error a call needing the account returns.
fn session_from_read(read: std::result::Result<Option<Session>, String>) -> Result<Session> {
    match read {
        Ok(Some(s)) => Ok(s),
        Ok(None) => Err(Error::coded(Code::SignedOut)),
        Err(why) => Err(Error::Invalid(why)),
    }
}

/// Whether a session's access token is still good for a call now.
fn fresh(session: &Session) -> bool {
    session.expires_at > now() + REFRESH_MARGIN_SECS
}

/// Every write of the session (a refresh, a sign-in, a sign-out) happens behind this lock, one
/// at a time:
/// - At start-up the agent and the first API calls all find the same expired token and would
///   each send the same refresh token. With refresh-token rotation the second one is rejected
///   (`invalid_grant`), and that used to wipe the session. Callers queue here and re-read the
///   session once they hold the lock, so the first refresh serves them all.
/// - A refresh in flight can't write its renewed session back over a sign-out (the player would
///   be signed in again at the next call) or over another account's sign-in.
static SESSION_WRITE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A renewed session the keychain refused to save. Refresh tokens rotate: the old one in the
/// keychain is spent, so dropping the new one would sign the player out at the next call (an
/// `invalid_grant`). It is served from here, and saved again at each read until that works.
static UNSAVED: std::sync::Mutex<Option<Session>> = std::sync::Mutex::new(None);

fn unsaved() -> Option<Session> {
    UNSAVED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

fn set_unsaved(session: Option<Session>) {
    *UNSAVED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = session;
}

/// The session calls use: the stored one, unless a newer renewed one is still waiting to be
/// saved (then that one, and saving is tried again). Saves: call it holding `SESSION_WRITE`.
async fn current_session() -> Result<Session> {
    let stored = session_or_signed_out().await;
    let Some(renewed) = unsaved() else { return stored };
    match stored {
        // Signed out (or unreadable) since: the renewed session is not resurrected.
        Err(e) => {
            set_unsaved(None);
            Err(e)
        }
        Ok(s) if renewed.expires_at <= s.expires_at => {
            set_unsaved(None);
            Ok(s)
        }
        Ok(_) => {
            if store::store(renewed.clone()).await.is_ok() {
                set_unsaved(None);
            }
            Ok(renewed)
        }
    }
}

/// A valid access token for CyberBackend, refreshed if it expires within a minute.
pub async fn access_token() -> Result<String> {
    access_token_renewed_by(refresh_tokens).await
}

/// Exchanges a refresh token at cyber-auth.
async fn refresh_tokens(refresh: String) -> Result<TokenResponse> {
    let (client_id, api) = (config::client_id(), config::api_url());
    token_request(&[("grant_type", "refresh_token"), ("refresh_token", &refresh), ("client_id", &client_id), ("resource", &api)]).await
}

/// [`access_token`], an expired session renewed by `renew` (given its refresh token).
async fn access_token_renewed_by<F, Fut>(renew: F) -> Result<String>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<TokenResponse>>,
{
    // The common case reads only: a fresh stored session and nothing waiting to be saved.
    if unsaved().is_none() {
        let session = session_or_signed_out().await?;
        if fresh(&session) {
            return Ok(session.access_token);
        }
    }
    let _one_at_a_time = SESSION_WRITE.lock().await;
    let session = current_session().await?;
    if fresh(&session) {
        return Ok(session.access_token);
    }
    let refresh = session.refresh_token.clone().ok_or_else(|| Error::coded(Code::SessionExpired))?;
    let tokens = match renew(refresh).await {
        Ok(tokens) => tokens,
        Err(e) => {
            // Only drop the session when the server explicitly rejected the refresh token
            // (OAuth `invalid_grant`). On a transient failure (network, 5xx) keep it, so a blip
            // near expiry doesn't log the user out and stop the background agent.
            if e.to_string().contains("invalid_grant") {
                store::clear().await;
            }
            return Err(e);
        }
    };
    let renewed = session_from(tokens, Some(&session));
    let token = renewed.access_token.clone();
    if let Err(e) = store::store(renewed.clone()).await {
        // The rotated refresh token must not be lost with the write: keep serving the renewed
        // session and retry saving it (see `UNSAVED`).
        log::warn!("the renewed session couldn't be saved yet ({e}); kept in memory until it can");
        set_unsaved(Some(renewed));
    }
    Ok(token)
}

/// Who is signed in, to tell accounts apart: the access token's `sub`, else the email. None
/// when signed out (or the session can't be read).
pub async fn signed_in_user() -> Option<String> {
    store::read().await.ok().flatten().map(|s| user_of(&s))
}

fn user_of(session: &Session) -> String {
    tokens::jwt_claim(&session.access_token, "sub").or_else(|| session.email.clone()).unwrap_or_default()
}

/// The authorize URL the browser opens.
fn authorize_url(client_id: &str, redirect_uri: &str, state: &str, verifier: &str, api: &str) -> Result<url::Url> {
    let mut authorize = url::Url::parse(&format!("{}/oauth2/authorize", config::auth_base())).map_err(|e| Error::Invalid(e.to_string()))?;
    authorize
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", SCOPES)
        .append_pair("state", state)
        .append_pair("code_challenge", &code_challenge(verifier))
        .append_pair("code_challenge_method", "S256")
        // The system browser may already be signed in, possibly as someone else: let the player
        // confirm the account or pick another instead of signing in silently.
        .append_pair("prompt", "select_account")
        .append_pair("resource", api);
    Ok(authorize)
}

/// Signs in through the system browser. `events` hears the sign-in link before the wait, and
/// even when no browser could be opened: a browser that takes the link and shows nothing can't
/// be detected, so the app offers the link to copy into one. `auth_login_cancel` (or another
/// sign-in) ends the wait with [`Error::Cancelled`].
pub async fn auth_login(app: AppHandle, events: Channel<String>) -> Result<AuthStatus> {
    let open = |url: &url::Url| {
        let _ = events.send(url.to_string());
        if let Err(e) = app.opener().open_url(url.as_str(), None::<&str>) {
            log::warn!("no browser opened for sign-in ({e}); the link is offered to copy");
        }
        Ok(())
    };
    if config::scheme_redirect() { login_by_link(open).await } else { login_by_loopback(open).await }
}

/// Ends the sign-in waiting for the browser, if any: it returns [`Error::Cancelled`].
pub fn auth_login_cancel() {
    waiting::cancel();
}

/// Sign-in that comes back to a loopback server on one of the registered ports.
async fn login_by_loopback(open: impl FnOnce(&url::Url) -> Result<()>) -> Result<AuthStatus> {
    let (listener, port) = callback::bind_loopback().await?;
    let wait = waiting::begin();
    let disarm = wait.disarm();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let verifier = random_b64(32);
    let state = random_b64(16);
    let (client_id, api) = (config::client_id(), config::api_url());

    open(&authorize_url(&client_id, &redirect_uri, &state, &verifier, &api)?)?;

    // Only the redirect carrying this login's `state` is taken, and the browser hears
    // "signed in" only once the code was exchanged and the session kept.
    let finish = |code: String| {
        disarm.now();
        finish_login(code, &redirect_uri, &client_id, &verifier, &api)
    };
    wait.run(callback::receive(listener, &state, finish)).await
}

/// Sign-in that comes back as a `cyberctf://auth/callback` link (see `scheme`): the browser asks
/// to open Cyber CTF, which takes the code and exchanges it.
async fn login_by_link(open: impl FnOnce(&url::Url) -> Result<()>) -> Result<AuthStatus> {
    let wait = waiting::begin();
    let disarm = wait.disarm();
    let verifier = random_b64(32);
    let state = random_b64(16);
    let (client_id, api) = (config::client_id(), config::api_url());
    let link = scheme::expect(&state);
    open(&authorize_url(&client_id, scheme::REDIRECT_URI, &state, &verifier, &api)?)?;
    wait.run(async {
        let url = link.await.map_err(|_| Error::Cancelled)?;
        let (code, _) = callback::outcome(&url)?;
        disarm.now();
        finish_login(code, scheme::REDIRECT_URI, &client_id, &verifier, &api).await
    })
    .await
}

/// Exchanges the redirect's code for tokens and keeps the session.
async fn finish_login(code: String, redirect_uri: &str, client_id: &str, verifier: &str, api: &str) -> Result<AuthStatus> {
    let tokens = token_request(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("code_verifier", verifier),
        ("resource", api),
    ])
    .await?;
    let session = session_from(tokens, None);
    let (name, email) = (session.name.clone(), session.email.clone());
    store::ask_keychain_again();
    // Signed in at cyber-auth, but the session can't be kept: say so, so the app doesn't look
    // like the login was never accepted.
    keep_signed_in(session).await.map_err(|e| Error::Invalid(format!("You signed in, but the session wasn't saved. {e}")))?;
    Ok(AuthStatus { logged_in: true, name, email, keychain_error: None })
}

/// Keeps a new sign-in's session, after any refresh in flight (whose renewed session, maybe
/// another account's, would otherwise land over it). A renewed session still waiting to be saved
/// belongs to the session this one replaces: it is dropped.
async fn keep_signed_in(session: Session) -> Result<()> {
    let _no_refresh = SESSION_WRITE.lock().await;
    set_unsaved(None);
    store::store(session).await
}

/// Async so the keychain is read off the UI thread: a locked GNOME keyring shows its unlock
/// prompt and waits, which froze the window while this ran on the main thread.
pub async fn auth_status() -> AuthStatus {
    status_from_read(store::read().await)
}

/// What the app shows for a session read: signed in as someone, signed out, or why the keychain failed.
fn status_from_read(read: std::result::Result<Option<Session>, String>) -> AuthStatus {
    match read {
        Ok(Some(s)) => AuthStatus { logged_in: true, name: s.name, email: s.email, keychain_error: None },
        Ok(None) => AuthStatus { logged_in: false, name: None, email: None, keychain_error: None },
        Err(why) => AuthStatus { logged_in: false, name: None, email: None, keychain_error: Some(why) },
    }
}

pub async fn auth_logout() {
    // After a refresh in flight: it would otherwise store its renewed session right after this
    // clear, and the player would be back in at the next call.
    let _no_refresh = SESSION_WRITE.lock().await;
    set_unsaved(None);
    store::clear().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_authorize_url_asks_for_pkce_and_an_account_choice() {
        let url = authorize_url("client", "http://127.0.0.1:47290/callback", "st", "verifier", "https://api.example").unwrap();
        let q: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(q["code_challenge"], code_challenge("verifier"));
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["prompt"], "select_account");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:47290/callback");
        assert_eq!(q["scope"], SCOPES);
        assert!(url.path().ends_with("/oauth2/authorize"));
    }

    #[test]
    fn a_token_near_expiry_is_not_fresh() {
        let session = |expires_at| Session { access_token: "a".into(), refresh_token: None, expires_at, name: None, email: None };
        assert!(fresh(&session(now() + 3600)));
        assert!(!fresh(&session(now() + 30)));
        assert!(!fresh(&session(0)));
    }

    fn session() -> Session {
        Session { access_token: "tok".into(), refresh_token: None, expires_at: 1, name: Some("Ada".into()), email: Some("ada@example.com".into()) }
    }

    #[test]
    fn a_missing_session_is_signed_out_and_a_keychain_failure_says_why() {
        assert_eq!(session_from_read(Ok(Some(session()))).unwrap().access_token, "tok");
        assert_eq!(session_from_read(Ok(None)).err().unwrap().code(), Code::SignedOut);
        assert_eq!(session_from_read(Err("locked".into())).err().unwrap().to_string(), "locked");
    }

    #[test]
    fn the_status_shows_who_is_signed_in_or_why_not() {
        let json = |s: AuthStatus| serde_json::to_value(s).unwrap();
        let signed_in = json(status_from_read(Ok(Some(session()))));
        assert_eq!(signed_in, serde_json::json!({ "loggedIn": true, "name": "Ada", "email": "ada@example.com" }));
        let out = json(status_from_read(Ok(None)));
        assert_eq!(out["loggedIn"], false);
        assert!(out.get("keychainError").is_none());
        let failed = json(status_from_read(Err("no Secret Service".into())));
        assert_eq!(failed["keychainError"], "no Secret Service");
        assert_eq!(failed["loggedIn"], false);
    }

    /// The dev session store and `UNSAVED` are shared: the tests over them run one at a time.
    static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// The commands over the dev session store (a file of this test run's own).
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn the_status_follows_the_kept_session_and_logout_forgets_it() {
        let _serial = ONE_AT_A_TIME.lock().await;
        let kept = Session { expires_at: now() + 3600, ..session() };
        store::store(kept).await.unwrap();
        let status = serde_json::to_value(auth_status().await).unwrap();
        assert_eq!(status, serde_json::json!({ "loggedIn": true, "name": "Ada", "email": "ada@example.com" }));
        // A fresh token is used as is: no refresh, no network.
        assert_eq!(access_token().await.unwrap(), "tok");
        assert_eq!(signed_in_user().await.as_deref(), Some("ada@example.com"));

        auth_logout().await;
        assert_eq!(serde_json::to_value(auth_status().await).unwrap()["loggedIn"], false);
        assert_eq!(access_token().await.unwrap_err().code(), Code::SignedOut);
        assert_eq!(signed_in_user().await, None);
        // Signing out twice is fine.
        auth_logout().await;
    }

    /// A renewed session the keychain couldn't take is served (its refresh token is the only
    /// valid one now), saved once that works, and dropped by a sign-out.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn a_renewed_session_that_could_not_be_saved_is_served_then_saved_and_forgotten_by_logout() {
        let _serial = ONE_AT_A_TIME.lock().await;
        let stale = Session { expires_at: now() + 10, ..session() };
        store::store(stale).await.unwrap();
        let renewed = Session { access_token: "tok2".into(), refresh_token: Some("ref2".into()), expires_at: now() + 3600, ..session() };
        set_unsaved(Some(renewed));
        // Served over the stored one, and saved now that the store takes it.
        assert_eq!(access_token().await.unwrap(), "tok2");
        assert!(unsaved().is_none());
        assert_eq!(store::read().await.unwrap().unwrap().refresh_token.as_deref(), Some("ref2"));
        // An older leftover never replaces a newer stored session.
        set_unsaved(Some(Session { access_token: "old".into(), expires_at: now() + 5, ..session() }));
        assert_eq!(access_token().await.unwrap(), "tok2");
        assert!(unsaved().is_none());
        // Sign-out forgets both.
        set_unsaved(Some(Session { access_token: "tok3".into(), expires_at: now() + 3600, ..session() }));
        auth_logout().await;
        assert!(unsaved().is_none());
        assert_eq!(access_token().await.unwrap_err().code(), Code::SignedOut);
    }

    /// A session whose access token needs a refresh, and the tokens a refresh answers with.
    fn expiring() -> Session {
        Session { access_token: "old".into(), refresh_token: Some("r1".into()), expires_at: now() + 10, ..session() }
    }

    fn renewed(access: &str) -> TokenResponse {
        TokenResponse { access_token: access.into(), refresh_token: Some("r2".into()), expires_in: Some(3600), id_token: None }
    }

    /// A refresh started in the background that is in flight when this returns, and answers once
    /// the returned sender says so.
    async fn refresh_in_flight() -> (tokio::task::JoinHandle<Result<String>>, tokio::sync::oneshot::Sender<()>) {
        let (started_tx, started) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel::<()>();
        let refresh = tokio::spawn(access_token_renewed_by(move |refresh| async move {
            assert_eq!(refresh, "r1");
            let _ = started_tx.send(());
            let _ = released.await;
            Ok(renewed("renewed"))
        }));
        started.await.unwrap();
        (refresh, release)
    }

    /// Whether `task` is still waiting a moment from now.
    async fn still_waiting<T>(task: &mut tokio::task::JoinHandle<T>) -> bool {
        tokio::time::timeout(std::time::Duration::from_millis(100), task).await.is_err()
    }

    /// The refresh's renewed session used to be stored right after the sign-out's clear: the
    /// player was signed in again at the next call, and the agent kept claiming launches.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn a_sign_out_during_a_refresh_in_flight_stays_signed_out() {
        let _serial = ONE_AT_A_TIME.lock().await;
        store::store(expiring()).await.unwrap();
        let (refresh, release) = refresh_in_flight().await;
        let mut logout = tokio::spawn(auth_logout());
        assert!(still_waiting(&mut logout).await, "the sign-out went through while the refresh was in flight");
        release.send(()).unwrap();
        assert_eq!(refresh.await.unwrap().unwrap(), "renewed");
        logout.await.unwrap();
        assert!(store::read().await.unwrap().is_none());
        assert_eq!(access_token().await.unwrap_err().code(), Code::SignedOut);
    }

    /// A sign-in during another session's refresh is the session kept, not the refreshed one.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn a_sign_in_during_a_refresh_in_flight_is_the_session_kept() {
        let _serial = ONE_AT_A_TIME.lock().await;
        store::store(expiring()).await.unwrap();
        let (refresh, release) = refresh_in_flight().await;
        let other = Session { access_token: "other-account".into(), expires_at: now() + 3600, ..session() };
        let mut sign_in = tokio::spawn(keep_signed_in(other));
        assert!(still_waiting(&mut sign_in).await, "the sign-in was saved while the refresh was in flight");
        release.send(()).unwrap();
        refresh.await.unwrap().unwrap();
        sign_in.await.unwrap().unwrap();
        assert_eq!(access_token().await.unwrap(), "other-account");
        auth_logout().await;
    }

    /// The sign-in link reaches the app before the wait, for a "copy the link" fallback when no
    /// browser shows it; Cancel then ends the sign-in at once, as cancelled (not a failure).
    #[tokio::test]
    async fn the_sign_in_link_is_given_before_the_wait_and_cancel_ends_it() {
        let _serial = waiting::tests::ONE_AT_A_TIME.lock().await;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let mut login = tokio::spawn(login_by_loopback(move |url| {
            tx.send(url.to_string()).unwrap();
            Ok(())
        }));
        let url = url::Url::parse(&rx.await.unwrap()).unwrap();
        assert!(url.path().ends_with("/oauth2/authorize"));
        let redirect = url.query_pairs().find(|(k, _)| k == "redirect_uri").unwrap().1.into_owned();
        assert!(redirect.starts_with("http://127.0.0.1:4729") && redirect.ends_with("/callback"), "{redirect}");
        assert!(still_waiting(&mut login).await);
        auth_login_cancel();
        let Err(err) = login.await.unwrap() else { panic!("a cancelled sign-in went through") };
        assert!(matches!(err, Error::Cancelled));
        assert_eq!(err.code(), Code::SignInCancelled);
    }

    #[test]
    fn the_signed_in_user_is_the_tokens_subject_else_the_email() {
        use base64::Engine;
        let jwt = |claims: &str| format!("h.{}.s", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims));
        let session = |access: String, email: Option<&str>| Session {
            access_token: access,
            refresh_token: None,
            expires_at: 0,
            name: None,
            email: email.map(str::to_string),
        };
        assert_eq!(user_of(&session(jwt(r#"{"sub":"u1"}"#), Some("a@x"))), "u1");
        assert_eq!(user_of(&session(jwt(r#"{"sub":"u2"}"#), Some("a@x"))), "u2");
        assert_eq!(user_of(&session("opaque".into(), Some("a@x"))), "a@x");
        assert_eq!(user_of(&session("opaque".into(), None)), "");
    }
}
