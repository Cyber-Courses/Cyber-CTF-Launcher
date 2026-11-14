use super::*;
use tokens::token_request_at;

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
    let session = |expires_at| Session { access_token: "a".into(), refresh_token: None, expires_at, name: None, email: None, picture: None };
    assert!(fresh(&session(unix_secs() + 3600)));
    assert!(!fresh(&session(unix_secs() + 30)));
    assert!(!fresh(&session(0)));
}

fn session() -> Session {
    Session { access_token: "tok".into(), refresh_token: None, expires_at: 1, name: Some("Ada".into()), email: Some("ada@example.com".into()), picture: None }
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
    let signed_in = json(status_from_read(Ok(Some(session())), false));
    assert_eq!(signed_in, serde_json::json!({ "loggedIn": true, "name": "Ada", "email": "ada@example.com" }));
    let with_picture = json(status_from_read(Ok(Some(Session { picture: Some("https://cdn.example/a.png".into()), ..session() })), true));
    assert_eq!(with_picture["picture"], "https://cdn.example/a.png");
    // Signed in: whatever ended an earlier session is over.
    assert!(with_picture.get("sessionExpired").is_none());
    let out = json(status_from_read(Ok(None), false));
    assert_eq!(out, serde_json::json!({ "loggedIn": false, "name": null, "email": null }));
    let expired = json(status_from_read(Ok(None), true));
    assert_eq!(expired["loggedIn"], false);
    assert_eq!(expired["sessionExpired"], true);
    let failed = json(status_from_read(Err("no Secret Service".into()), true));
    assert_eq!(failed["keychainError"], "no Secret Service");
    assert_eq!(failed["loggedIn"], false);
    // Unreadable, not expired: the keychain is the reason given.
    assert!(failed.get("sessionExpired").is_none());
}

/// Each test thread has its own session file, `UNSAVED` and `EXPIRED`, but `SESSION_WRITE` is
/// the process's: the tests that wait on it run one at a time. Those tests run over the dev
/// session store, a debug build's alone: so does what they share, or a release build's test
/// target (the rig's `rig` profile) finds it unused.
#[cfg(debug_assertions)]
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The commands over the dev session store (a file of this test run's own).
#[cfg(debug_assertions)]
#[tokio::test]
async fn the_status_follows_the_kept_session_and_logout_forgets_it() {
    let _serial = ONE_AT_A_TIME.lock().await;
    let kept = Session { expires_at: unix_secs() + 3600, ..session() };
    store::store(kept).await.unwrap();
    let status = serde_json::to_value(auth_status().await).unwrap();
    assert_eq!(status, serde_json::json!({ "loggedIn": true, "name": "Ada", "email": "ada@example.com" }));
    // A fresh token is used as is: no refresh, no network.
    assert_eq!(access_token().await.unwrap(), "tok");
    assert_eq!(signed_in_user().await.unwrap().as_deref(), Some("ada@example.com"));

    auth_logout().await.unwrap();
    assert_eq!(serde_json::to_value(auth_status().await).unwrap()["loggedIn"], false);
    assert_eq!(access_token().await.unwrap_err().code(), Code::SignedOut);
    assert_eq!(signed_in_user().await.unwrap(), None);
    // Signing out twice is fine.
    auth_logout().await.unwrap();
}

/// A renewed session the keychain couldn't take is served (its refresh token is the only
/// valid one now), saved once that works, and dropped by a sign-out.
#[cfg(debug_assertions)]
#[tokio::test]
async fn a_renewed_session_that_could_not_be_saved_is_served_then_saved_and_forgotten_by_logout() {
    let _serial = ONE_AT_A_TIME.lock().await;
    let stale = Session { expires_at: unix_secs() + 10, ..session() };
    store::store(stale).await.unwrap();
    let renewed = Session { access_token: "tok2".into(), refresh_token: Some("ref2".into()), expires_at: unix_secs() + 3600, ..session() };
    set_unsaved(&SessionWrite::lock().await, Some(renewed));
    // Served over the stored one, and saved now that the store takes it.
    assert_eq!(access_token().await.unwrap(), "tok2");
    assert!(unsaved().is_none());
    assert_eq!(store::read().await.unwrap().unwrap().refresh_token.as_deref(), Some("ref2"));
    // An older leftover never replaces a newer stored session.
    set_unsaved(&SessionWrite::lock().await, Some(Session { access_token: "old".into(), expires_at: unix_secs() + 5, ..session() }));
    assert_eq!(access_token().await.unwrap(), "tok2");
    assert!(unsaved().is_none());
    // Sign-out forgets both.
    set_unsaved(&SessionWrite::lock().await, Some(Session { access_token: "tok3".into(), expires_at: unix_secs() + 3600, ..session() }));
    auth_logout().await.unwrap();
    assert!(unsaved().is_none());
    assert_eq!(access_token().await.unwrap_err().code(), Code::SignedOut);
}

/// A session whose access token needs a refresh, and the tokens a refresh answers with.
#[cfg(debug_assertions)]
fn expiring() -> Session {
    Session { access_token: "old".into(), refresh_token: Some("r1".into()), expires_at: unix_secs() + 10, ..session() }
}

#[cfg(debug_assertions)]
fn renewed(access: &str) -> TokenResponse {
    TokenResponse { access_token: access.into(), refresh_token: Some("r2".into()), expires_in: Some(3600), id_token: None }
}

/// A refresh started in the background that is in flight when this returns, and answers once
/// the returned sender says so.
#[cfg(debug_assertions)]
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
    logout.await.unwrap().unwrap();
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
    let other = Session { access_token: "other-account".into(), expires_at: unix_secs() + 3600, ..session() };
    let mut sign_in = tokio::spawn(keep_signed_in(other));
    assert!(still_waiting(&mut sign_in).await, "the sign-in was saved while the refresh was in flight");
    release.send(()).unwrap();
    refresh.await.unwrap().unwrap();
    sign_in.await.unwrap().unwrap();
    assert_eq!(access_token().await.unwrap(), "other-account");
    auth_logout().await.unwrap();
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

/// A token response with an ID token naming Ada.
fn ada_tokens() -> &'static str {
    // {"sub":"u1","name":"Ada","email":"ada@example.com"}
    r#"{"access_token":"at1","refresh_token":"rt1","expires_in":3600,"id_token":"h.eyJzdWIiOiJ1MSIsIm5hbWUiOiJBZGEiLCJlbWFpbCI6ImFkYUBleGFtcGxlLmNvbSJ9.s"}"#
}

#[tokio::test]
async fn the_code_exchange_sends_the_pkce_verifier_and_reads_who_signed_in() {
    let (base, requests) = testing::serve(vec![(200, ada_tokens())]).await;
    let form = code_exchange_form("code-1", "http://127.0.0.1:47290/callback", "client", "verifier", "https://api.example");
    let session = session_from(token_request_at(&base, &form).await.unwrap(), None);
    assert_eq!((session.access_token.as_str(), session.refresh_token.as_deref()), ("at1", Some("rt1")));
    assert_eq!((session.name.as_deref(), session.email.as_deref()), (Some("Ada"), Some("ada@example.com")));
    assert!(session.expires_at > unix_secs() + 3500);
    let sent = &requests.lock().unwrap()[0];
    assert_eq!(sent.path, "/oauth2/token");
    assert_eq!(sent.form["grant_type"], "authorization_code");
    assert_eq!(sent.form["code"], "code-1");
    assert_eq!(sent.form["code_verifier"], "verifier");
    assert_eq!(sent.form["redirect_uri"], "http://127.0.0.1:47290/callback");
    assert_eq!(sent.form["client_id"], "client");
    assert_eq!(sent.form["resource"], "https://api.example");
    assert!(!sent.form.contains_key("client_secret"), "the launcher is a public client");
}

/// An expired access token is renewed with the refresh token, and the rotated refresh token
/// is the one kept; a refused refresh token signs the player out.
#[cfg(debug_assertions)]
#[tokio::test]
async fn an_expired_token_is_refreshed_silently_and_a_refused_refresh_signs_out() {
    let _serial = ONE_AT_A_TIME.lock().await;
    store::store(expiring()).await.unwrap();
    let (base, requests) = testing::serve(vec![(200, r#"{"access_token":"at2","refresh_token":"r2","expires_in":3600}"#)]).await;
    let renew = |refresh: String| async move { refresh_tokens_at(&base, "client", "https://api.example", &refresh).await };
    assert_eq!(access_token_renewed_by(renew).await.unwrap(), "at2");
    let sent = requests.lock().unwrap()[0].clone();
    assert_eq!(sent.form["grant_type"], "refresh_token");
    assert_eq!(sent.form["refresh_token"], "r1");
    assert_eq!(sent.form["resource"], "https://api.example");
    let kept = store::read().await.unwrap().unwrap();
    assert_eq!(kept.refresh_token.as_deref(), Some("r2"));
    // The profile carries over a refresh without an ID token.
    assert_eq!(kept.email.as_deref(), Some("ada@example.com"));

    store::store(expiring()).await.unwrap();
    let (base, _) = testing::serve(vec![(400, r#"{"error":"invalid_grant"}"#)]).await;
    let renew = |refresh: String| async move { refresh_tokens_at(&base, "client", "api", &refresh).await };
    assert_eq!(access_token_renewed_by(renew).await.unwrap_err().code(), Code::SessionExpired);
    assert!(store::read().await.unwrap().is_none(), "a refused refresh token was kept");
    // Signed out under the app: the status says the session expired, until a sign-out on purpose.
    let status = serde_json::to_value(auth_status().await).unwrap();
    assert_eq!(status["loggedIn"], false);
    assert_eq!(status["sessionExpired"], true);
    auth_logout().await.unwrap();
    assert!(serde_json::to_value(auth_status().await).unwrap().get("sessionExpired").is_none());

    // Another refusal keeps the session, even one whose description names `invalid_grant`:
    // only the OAuth error code says the refresh token itself was refused.
    store::store(expiring()).await.unwrap();
    let (base, _) = testing::serve(vec![(400, r#"{"error":"invalid_request","error_description":"not an invalid_grant: resource missing"}"#)]).await;
    let renew = |refresh: String| async move { refresh_tokens_at(&base, "client", "api", &refresh).await };
    let err = access_token_renewed_by(renew).await.unwrap_err();
    assert_ne!(err.code(), Code::SessionExpired, "{err}");
    assert!(err.to_string().starts_with("token request rejected"), "{err}");
    assert!(store::read().await.unwrap().is_some(), "another refusal signed the player out");
    assert!(serde_json::to_value(auth_status().await).unwrap().get("sessionExpired").is_none());

    // An outage keeps the session: a blip near expiry doesn't sign the player out.
    store::store(expiring()).await.unwrap();
    let (base, _) = testing::serve(vec![(503, "{}")]).await;
    let renew = |refresh: String| async move { refresh_tokens_at(&base, "client", "api", &refresh).await };
    assert!(matches!(access_token_renewed_by(renew).await.unwrap_err(), Error::Unreachable(_)));
    assert!(store::read().await.unwrap().is_some());
    auth_logout().await.unwrap();
}

const DEVICE_GRANT: &str = r#"{"device_code":"dev-1","user_code":"WDJB-MJHT","verification_uri":"https://auth.example/device","expires_in":600,"interval":0}"#;

/// Signing in with a code: the code is shown, then the approved sign-in is kept like a browser one.
#[cfg(debug_assertions)]
#[tokio::test]
async fn a_sign_in_with_a_code_shows_it_then_keeps_the_session() {
    let _serial = ONE_AT_A_TIME.lock().await;
    let _one_sign_in = waiting::tests::ONE_AT_A_TIME.lock().await;
    let (base, _) = testing::serve(vec![(200, DEVICE_GRANT), (400, r#"{"error":"authorization_pending"}"#), (200, ada_tokens())]).await;
    let shown = std::sync::Mutex::new(None);
    let status = login_by_device_at(&base, |code| *shown.lock().unwrap() = Some(code.clone()), |_| std::future::ready(())).await.unwrap();
    let status = serde_json::to_value(status).unwrap();
    assert_eq!(status, serde_json::json!({ "loggedIn": true, "name": "Ada", "email": "ada@example.com" }));
    let shown = serde_json::to_value(shown.into_inner().unwrap().unwrap()).unwrap();
    assert_eq!(shown["userCode"], "WDJB-MJHT");
    assert_eq!(shown["verificationUri"], "https://auth.example/device");
    assert_eq!(access_token().await.unwrap(), "at1");
    auth_logout().await.unwrap();
}

/// Cancel ends a sign-in with a code waiting for its approval, as cancelled.
#[tokio::test]
async fn a_sign_in_with_a_code_can_be_cancelled() {
    let _one_sign_in = waiting::tests::ONE_AT_A_TIME.lock().await;
    let (base, _) = testing::serve(vec![(200, DEVICE_GRANT)]).await;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut login = tokio::spawn(async move { login_by_device_at(&base, move |code| tx.send(code).unwrap(), |_| std::future::pending::<()>()).await });
    rx.await.unwrap();
    assert!(still_waiting(&mut login).await);
    auth_login_cancel();
    assert!(matches!(login.await.unwrap(), Err(Error::Cancelled)));
}

/// The stored session is corrupt (the file or keychain entry holds something that isn't a
/// session): the app shows signed out with no keychain error, and a call needing the account
/// says signed out rather than failing with a parse error.
#[cfg(debug_assertions)]
#[tokio::test]
async fn a_corrupt_stored_session_reads_as_signed_out_without_an_error() {
    let _serial = ONE_AT_A_TIME.lock().await;
    let path = store::dev_session_path().unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{\"access_token\":").unwrap();
    let status = serde_json::to_value(auth_status().await).unwrap();
    assert_eq!(status["loggedIn"], false);
    assert!(status.get("keychainError").is_none(), "{status}");
    assert_eq!(access_token().await.unwrap_err().code(), Code::SignedOut);
    assert_eq!(signed_in_user().await.unwrap(), None);
    auth_logout().await.unwrap();
}

/// Regression: a session that is there but can't be read (the dev build's file, as a locked
/// keychain in a release) read as signed out: the agent forgot its registration, and calls
/// went out as anonymous. It is an error now.
#[cfg(debug_assertions)]
#[tokio::test]
async fn an_unreadable_stored_session_is_an_error_not_signed_out() {
    let _serial = ONE_AT_A_TIME.lock().await;
    let path = store::dev_session_path().unwrap();
    let _ = std::fs::remove_file(&path);
    std::fs::create_dir_all(path.join("held")).unwrap();
    assert!(signed_in_user().await.unwrap_err().to_string().starts_with("the session file "));
    assert_eq!(access_token().await.unwrap_err().code(), Code::Other);
    std::fs::remove_dir_all(&path).unwrap();
    assert_eq!(signed_in_user().await.unwrap(), None);
}

/// An access token past its margin with no refresh token kept (a sign-in that was given
/// none): the call says the session expired, asks cyber-auth for nothing, and leaves the
/// session as it is for the next sign-in to replace.
#[cfg(debug_assertions)]
#[tokio::test]
async fn an_expired_session_without_a_refresh_token_says_session_expired() {
    let _serial = ONE_AT_A_TIME.lock().await;
    store::store(Session { refresh_token: None, expires_at: unix_secs() + 10, ..session() }).await.unwrap();
    let renew = |_: String| -> std::future::Ready<Result<TokenResponse>> { panic!("there is no refresh token to send") };
    let err = access_token_renewed_by(renew).await.unwrap_err();
    assert_eq!(err.code(), Code::SessionExpired);
    assert!(store::read().await.unwrap().is_some(), "the session was dropped");
    auth_logout().await.unwrap();
}

/// CyberBackend refusing a call (401: the token isn't accepted, 403: not allowed) is that
/// call's failure, in GraphQL's words: it never signs the player out. Only cyber-auth refusing
/// the refresh token does that, so the session is still there for the next call.
#[cfg(debug_assertions)]
#[tokio::test]
async fn a_backend_refusal_keeps_the_session() {
    let _serial = ONE_AT_A_TIME.lock().await;
    store::store(Session { expires_at: unix_secs() + 3600, ..session() }).await.unwrap();
    for status in ["401 Unauthorized", "403 Forbidden"] {
        let body = r#"{"errors":[{"message":"not allowed"}]}"#;
        let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        let base = crate::platform::http::tests::serve_once(response.into_bytes()).await;
        let token = access_token().await.unwrap();
        let err = crate::account::api::send(base.trim_end_matches('/'), Some(token), "{ me { id } }", serde_json::Value::Null).await.unwrap_err();
        assert_eq!((err.code(), err.to_string()), (Code::Other, "not allowed".to_string()), "{status}");
        assert_eq!(access_token().await.unwrap(), "tok", "{status}: the session was dropped");
    }
    auth_logout().await.unwrap();
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
        picture: None,
    };
    assert_eq!(user_of(&session(jwt(r#"{"sub":"u1"}"#), Some("a@x"))), "u1");
    assert_eq!(user_of(&session(jwt(r#"{"sub":"u2"}"#), Some("a@x"))), "u2");
    assert_eq!(user_of(&session("opaque".into(), Some("a@x"))), "a@x");
    assert_eq!(user_of(&session("opaque".into(), None)), "");
}
