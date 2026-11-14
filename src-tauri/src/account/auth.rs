//! Login with cyber-auth: OAuth authorization code + PKCE (RFC 7636) through the
//! system browser, back to the app by a `cyberctf://` link or a loopback redirect (RFC 8252,
//! see `config::scheme_redirect`); or, when that browser can't be used, with a code entered on
//! any device (RFC 8628, see `device`). Tokens live in the OS keychain (a debug build keeps
//! them in `~/.cyberctf/dev-session.json`, see `store`) and never reach the webview; API calls
//! go through `api.rs`.

mod callback;
mod device;
mod scheme;
mod store;
#[cfg(test)]
mod testing;
mod tokens;
mod waiting;

pub use device::DeviceCode;
pub use scheme::deliver as deliver_link;

use std::future::Future;

use serde::Serialize;

use crate::error::{Code, Error, Result};
use crate::platform::clock::unix_secs;
use crate::platform::config;
use store::Session;
use tokens::{TokenResponse, code_challenge, random_b64, refresh_request_at, session_from, token_request};

const SCOPES: &str = "openid profile email offline_access";
/// An access token this close to expiry is refreshed before use.
const REFRESH_MARGIN_SECS: u64 = 60;

/// Whether the player is signed in, and who as, for the webview's account menu and its sign-in
/// gates.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    logged_in: bool,
    name: Option<String>,
    email: Option<String>,
    /// The account's picture, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    picture: Option<String>,
    /// Why the session couldn't be read (the keychain is locked, its prompt was dismissed, or
    /// there is no Secret Service), when that is why the player shows as signed out.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    keychain_error: Option<String>,
    /// Signed out because the session expired (cyber-auth refused its refresh token), not by
    /// the player: said until the next sign-in, so the app can tell them why.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    session_expired: Option<bool>,
}

impl AuthStatus {
    fn signed_out() -> Self {
        AuthStatus { logged_in: false, name: None, email: None, picture: None, keychain_error: None, session_expired: None }
    }
}

/// Whether the last session ended because cyber-auth refused its refresh token (see
/// `access_token_renewed_by`): the player was signed out under the app, and is told so until
/// they sign in or out themselves.
#[cfg(not(test))]
static EXPIRED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
thread_local! {
    /// Per test thread, like the session file (`store::dev_session_path`).
    static EXPIRED: std::sync::atomic::AtomicBool = const { std::sync::atomic::AtomicBool::new(false) };
}

fn expired() -> bool {
    let read = |e: &std::sync::atomic::AtomicBool| e.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(test)]
    return EXPIRED.with(read);
    #[cfg(not(test))]
    read(&EXPIRED)
}

fn set_expired(expired: bool) {
    let write = |e: &std::sync::atomic::AtomicBool| e.store(expired, std::sync::atomic::Ordering::Relaxed);
    #[cfg(test)]
    EXPIRED.with(write);
    #[cfg(not(test))]
    write(&EXPIRED);
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
    session.expires_at > unix_secs() + REFRESH_MARGIN_SECS
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

/// `SESSION_WRITE`, held: what writes the session (the renewed one waiting to be saved
/// included) takes it, so a write outside the lock doesn't compile.
struct SessionWrite {
    /// Held for its lock, never read.
    _held: tokio::sync::MutexGuard<'static, ()>,
}

impl SessionWrite {
    /// Waits for the other writes of the session to finish, then holds the lock.
    async fn lock() -> SessionWrite {
        SessionWrite { _held: SESSION_WRITE.lock().await }
    }

    /// The session calls use: the stored one, unless a newer renewed one is still waiting to be
    /// saved (then that one, and saving is tried again).
    async fn current_session(&self) -> Result<Session> {
        let stored = session_or_signed_out().await;
        let Some(renewed) = unsaved() else { return stored };
        match stored {
            // Signed out (or unreadable) since: the renewed session is not resurrected.
            Err(e) => {
                set_unsaved(self, None);
                Err(e)
            }
            Ok(s) if renewed.expires_at <= s.expires_at => {
                set_unsaved(self, None);
                Ok(s)
            }
            Ok(_) => {
                if store::store(renewed.clone()).await.is_ok() {
                    set_unsaved(self, None);
                }
                Ok(renewed)
            }
        }
    }
}

/// A renewed session the keychain refused to save. Refresh tokens rotate: the old one in the
/// keychain is spent, so dropping the new one would sign the player out at the next call (an
/// `invalid_grant`). It is served from here, and saved again at each read until that works.
#[cfg(not(test))]
static UNSAVED: std::sync::Mutex<Option<Session>> = std::sync::Mutex::new(None);
#[cfg(test)]
thread_local! {
    /// Per test thread, like the session file (`store::dev_session_path`).
    static UNSAVED: std::sync::Mutex<Option<Session>> = const { std::sync::Mutex::new(None) };
}

/// `f` on the renewed session waiting to be saved.
fn with_unsaved<T>(f: impl FnOnce(&mut Option<Session>) -> T) -> T {
    let run = |m: &std::sync::Mutex<Option<Session>>| f(&mut m.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    #[cfg(test)]
    return UNSAVED.with(run);
    #[cfg(not(test))]
    run(&UNSAVED)
}

fn unsaved() -> Option<Session> {
    with_unsaved(|u| u.clone())
}

/// Sets the renewed session waiting to be saved (`UNSAVED`): only under the write lock, which
/// `_write` proves.
fn set_unsaved(_write: &SessionWrite, session: Option<Session>) {
    with_unsaved(|u| *u = session);
}

/// A valid access token for CyberBackend, refreshed if it expires within a minute.
pub async fn access_token() -> Result<String> {
    access_token_renewed_by(refresh_tokens).await
}

/// Exchanges a refresh token at cyber-auth.
async fn refresh_tokens(refresh: String) -> Result<TokenResponse> {
    refresh_tokens_at(&config::auth_base(), &config::client_id(), &config::api_url(), &refresh).await
}

/// [`refresh_tokens`] at the cyber-auth at `base`.
async fn refresh_tokens_at(base: &str, client_id: &str, api: &str, refresh: &str) -> Result<TokenResponse> {
    refresh_request_at(base, &[("grant_type", "refresh_token"), ("refresh_token", refresh), ("client_id", client_id), ("resource", api)]).await
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
    let write = SessionWrite::lock().await;
    let session = write.current_session().await?;
    if fresh(&session) {
        return Ok(session.access_token);
    }
    let refresh = session.refresh_token.clone().ok_or_else(|| Error::coded(Code::SessionExpired))?;
    let tokens = match renew(refresh).await {
        Ok(tokens) => tokens,
        Err(e) => {
            // Only drop the session when the server explicitly rejected the refresh token
            // (OAuth `invalid_grant`, see `refresh_request_at`). On any other failure (network,
            // 5xx, another refusal) keep it, so a blip near expiry doesn't log the user out and
            // stop the background agent.
            if e.code() == Code::SessionExpired {
                if let Err(kept) = store::clear().await {
                    log::warn!("the session the server refused stays stored: {kept}");
                }
                set_expired(true);
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
        set_unsaved(&write, Some(renewed));
    }
    Ok(token)
}

/// Who is signed in, to tell accounts apart: the access token's `sub`, else the email. None
/// when signed out; an error when the session is there but can't be read (the keychain locked),
/// which must not read as signed out.
pub async fn signed_in_user() -> Result<Option<String>> {
    store::read().await.map(|s| s.map(|s| user_of(&s))).map_err(Error::Invalid)
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

/// Signs in through the system browser: `show` gets the sign-in link before the wait, to tell
/// the page and open the browser. The page hears it even when no browser could be opened: a
/// browser that takes the link and shows nothing can't be detected, so the app offers the link
/// to copy into one. `auth_login_cancel` (or another sign-in) ends the wait with
/// [`Error::Cancelled`].
pub async fn auth_login(show: impl FnOnce(&url::Url)) -> Result<AuthStatus> {
    let open = |url: &url::Url| {
        show(url);
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
    keep_tokens(token_request(&code_exchange_form(&code, redirect_uri, client_id, verifier, api)).await?).await
}

/// The token request that exchanges an authorization code (RFC 6749 §4.1.3, with the PKCE
/// verifier of RFC 7636 §4.5).
fn code_exchange_form<'a>(code: &'a str, redirect_uri: &'a str, client_id: &'a str, verifier: &'a str, api: &'a str) -> [(&'static str, &'a str); 6] {
    [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("code_verifier", verifier),
        ("resource", api),
    ]
}

/// Signs in with a code: `show` gets the code and where to enter it, then the player approves
/// it on cyberauth.co from any device. `auth_login_cancel` (or another sign-in) ends the wait
/// with [`Error::Cancelled`].
pub async fn auth_login_device(show: impl FnOnce(DeviceCode)) -> Result<AuthStatus> {
    login_by_device_at(&config::auth_base(), show, tokio::time::sleep).await
}

/// [`auth_login_device`] at the cyber-auth at `base`, waiting between polls with `sleep`.
async fn login_by_device_at<S, F>(base: &str, show: impl FnOnce(DeviceCode), sleep: S) -> Result<AuthStatus>
where
    S: Fn(std::time::Duration) -> F,
    F: Future<Output = ()>,
{
    // Begun before the request, so a browser sign-in still waiting ends now: one sign-in at a time.
    let wait = waiting::begin();
    let disarm = wait.disarm();
    let (client_id, api) = (config::client_id(), config::api_url());
    let grant = device::request(base, &client_id, &api).await?;
    show(grant.shown());
    wait.run_within(grant.lifetime(), async {
        let tokens = device::poll(base, &client_id, &api, &grant, sleep).await?;
        disarm.now();
        keep_tokens(tokens).await
    })
    .await
}

/// Keeps a new sign-in's tokens as the session.
async fn keep_tokens(tokens: TokenResponse) -> Result<AuthStatus> {
    let session = session_from(tokens, None);
    let (name, email, picture) = (session.name.clone(), session.email.clone(), session.picture.clone());
    store::ask_keychain_again();
    // Signed in at cyber-auth, but the session can't be kept: say so, so the app doesn't look
    // like the login was never accepted.
    keep_signed_in(session).await.map_err(|e| Error::Invalid(format!("You signed in, but the session wasn't saved. {e}")))?;
    set_expired(false);
    Ok(AuthStatus { logged_in: true, name, email, picture, keychain_error: None, session_expired: None })
}

/// Keeps a new sign-in's session, after any refresh in flight (whose renewed session, maybe
/// another account's, would otherwise land over it). A renewed session still waiting to be saved
/// belongs to the session this one replaces: it is dropped.
async fn keep_signed_in(session: Session) -> Result<()> {
    let no_refresh = SessionWrite::lock().await;
    set_unsaved(&no_refresh, None);
    store::store(session).await
}

/// Async so the keychain is read off the UI thread: a locked GNOME keyring shows its unlock
/// prompt and waits, which froze the window while this ran on the main thread.
pub async fn auth_status() -> AuthStatus {
    status_from_read(store::read().await, expired())
}

/// What the app shows for a session read: signed in as someone, signed out (and whether the
/// session expired under the app, see `EXPIRED`), or why the keychain failed.
fn status_from_read(read: std::result::Result<Option<Session>, String>, expired: bool) -> AuthStatus {
    match read {
        Ok(Some(s)) => AuthStatus { logged_in: true, name: s.name, email: s.email, picture: s.picture, keychain_error: None, session_expired: None },
        Ok(None) => AuthStatus { session_expired: expired.then_some(true), ..AuthStatus::signed_out() },
        Err(why) => AuthStatus { keychain_error: Some(why), ..AuthStatus::signed_out() },
    }
}

/// Signs out: forgets the session, here and in the keychain.
///
/// # Errors
///
/// When the keychain (the session file, in a dev build) still holds the session: the player
/// isn't signed out, and is told so rather than signed in again at the next start.
pub async fn auth_logout() -> Result<()> {
    // After a refresh in flight: it would otherwise store its renewed session right after this
    // clear, and the player would be back in at the next call.
    let no_refresh = SessionWrite::lock().await;
    set_unsaved(&no_refresh, None);
    // Signed out on purpose: not an expiry to explain.
    set_expired(false);
    store::clear().await
}

#[cfg(test)]
mod tests;
