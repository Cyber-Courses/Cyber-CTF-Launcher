//! Login with cyber-auth: OAuth authorization code + PKCE (RFC 7636) through the
//! system browser, back to the app by a `cyberctf://` link or a loopback redirect (RFC 8252,
//! see `config::scheme_redirect`). Tokens live in the OS keychain and never reach the webview;
//! API calls go through `api.rs`.

mod callback;
mod scheme;
mod store;
mod tokens;

pub use scheme::deliver as deliver_link;

use std::time::Duration;

use crate::AppHandle;
use serde::Serialize;
use tauri_plugin_opener::OpenerExt;

use crate::config;
use crate::error::{Error, Result};
use store::Session;
use tokens::{code_challenge, now, random_b64, session_from, token_request};

const SCOPES: &str = "openid profile email offline_access";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
/// An access token this close to expiry is refreshed before use.
const REFRESH_MARGIN_SECS: u64 = 60;

/// What a call needing the account says when there is no session (never signed in, signed out,
/// or the keychain lost it). The app matches on "signed out" to flip to its signed-out state.
pub const SIGNED_OUT: &str = "You're signed out. Sign in again to continue.";

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
        Ok(None) => Err(Error::Invalid(SIGNED_OUT.into())),
        Err(why) => Err(Error::Invalid(why)),
    }
}

/// Whether a session's access token is still good for a call now.
fn fresh(session: &Session) -> bool {
    session.expires_at > now() + REFRESH_MARGIN_SECS
}

/// A valid access token for CyberBackend, refreshed if it expires within a minute.
///
/// One refresh at a time: at start-up the agent and the first API calls all find the same
/// expired token and would each send the same refresh token. With refresh-token rotation the
/// second one is rejected (`invalid_grant`), and that used to wipe the session: the app came
/// back signed out after every restart past the token's lifetime. Callers queue here and re-read
/// the session once they hold the lock, so the first refresh serves them all.
pub async fn access_token() -> Result<String> {
    static REFRESH: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    let session = session_or_signed_out().await?;
    if fresh(&session) {
        return Ok(session.access_token);
    }
    let _one_at_a_time = REFRESH.get_or_init(|| tokio::sync::Mutex::new(())).lock().await;
    let session = session_or_signed_out().await?;
    if fresh(&session) {
        return Ok(session.access_token);
    }
    let refresh = session.refresh_token.clone().ok_or_else(|| Error::Invalid("Your session expired: you're signed out. Sign in again to continue.".into()))?;
    let (client_id, api) = (config::client_id(), config::api_url());
    let tokens = match token_request(&[("grant_type", "refresh_token"), ("refresh_token", &refresh), ("client_id", &client_id), ("resource", &api)]).await {
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
    store::store(renewed).await?;
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

const LOGIN_TIMED_OUT: &str = "Sign-in wasn't finished within 5 minutes, so it was cancelled. Try again.";

/// Opens the sign-in page in the system browser.
fn open_browser(app: &AppHandle, authorize: &url::Url) -> Result<()> {
    app.opener()
        .open_url(authorize.as_str(), None::<&str>)
        .map_err(|_| Error::Invalid("No browser opened for sign-in. Set a default web browser for this account and try again.".into()))
}

#[tauri::command]
pub async fn auth_login(app: AppHandle) -> Result<AuthStatus> {
    if config::scheme_redirect() {
        return login_by_link(&app).await;
    }
    let (listener, port) = callback::bind_loopback().await?;
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let verifier = random_b64(32);
    let state = random_b64(16);
    let (client_id, api) = (config::client_id(), config::api_url());

    open_browser(&app, &authorize_url(&client_id, &redirect_uri, &state, &verifier, &api)?)?;

    // Only the redirect carrying this login's `state` is taken, and the browser hears
    // "signed in" only once the code was exchanged and the session kept.
    let finish = |code: String| finish_login(code, &redirect_uri, &client_id, &verifier, &api);
    tokio::time::timeout(LOGIN_TIMEOUT, callback::receive(listener, &state, finish)).await.map_err(|_| Error::Invalid(LOGIN_TIMED_OUT.into()))?
}

/// Sign-in that comes back as a `cyberctf://auth/callback` link (see `scheme`): the browser asks
/// to open Cyber CTF, which takes the code and exchanges it.
async fn login_by_link(app: &AppHandle) -> Result<AuthStatus> {
    let verifier = random_b64(32);
    let state = random_b64(16);
    let (client_id, api) = (config::client_id(), config::api_url());
    let link = scheme::expect(&state);
    open_browser(app, &authorize_url(&client_id, scheme::REDIRECT_URI, &state, &verifier, &api)?)?;
    let url = tokio::time::timeout(LOGIN_TIMEOUT, link)
        .await
        .map_err(|_| Error::Invalid(LOGIN_TIMED_OUT.into()))?
        .map_err(|_| Error::Invalid("Another sign-in was started; this one was cancelled.".into()))?;
    let (code, _) = callback::outcome(&url)?;
    finish_login(code, scheme::REDIRECT_URI, &client_id, &verifier, &api).await
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
    store::store(session).await.map_err(|e| Error::Invalid(format!("You signed in, but the session wasn't saved. {e}")))?;
    Ok(AuthStatus { logged_in: true, name, email, keychain_error: None })
}

/// Async so the keychain is read off the UI thread: a locked GNOME keyring shows its unlock
/// prompt and waits, which froze the window while this ran on the main thread.
#[tauri::command]
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

#[tauri::command]
pub async fn auth_logout() {
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
        assert_eq!(session_from_read(Ok(None)).err().unwrap().to_string(), SIGNED_OUT);
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

    /// The commands over the dev session store (a file of this test run's own).
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn the_status_follows_the_kept_session_and_logout_forgets_it() {
        let kept = Session { expires_at: now() + 3600, ..session() };
        store::store(kept).await.unwrap();
        let status = serde_json::to_value(auth_status().await).unwrap();
        assert_eq!(status, serde_json::json!({ "loggedIn": true, "name": "Ada", "email": "ada@example.com" }));
        // A fresh token is used as is: no refresh, no network.
        assert_eq!(access_token().await.unwrap(), "tok");
        assert_eq!(signed_in_user().await.as_deref(), Some("ada@example.com"));

        auth_logout().await;
        assert_eq!(serde_json::to_value(auth_status().await).unwrap()["loggedIn"], false);
        assert_eq!(access_token().await.unwrap_err().to_string(), SIGNED_OUT);
        assert_eq!(signed_in_user().await, None);
        // Signing out twice is fine.
        auth_logout().await;
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
