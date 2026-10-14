//! OAuth tokens: PKCE values, the token endpoint, and the session a token response makes.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::store::Session;
use crate::config;
use crate::error::{Error, Result};

/// Lifetime assumed for an access token whose response doesn't say.
const DEFAULT_EXPIRES_IN: u64 = 3600;

#[derive(Deserialize)]
pub(super) struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
    pub id_token: Option<String>,
}

pub(super) fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub(super) fn random_b64(bytes: usize) -> String {
    let buf: Vec<u8> = (0..bytes).map(|_| rand::random::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(buf)
}

/// S256 code challenge for a verifier (RFC 7636 §4.2).
pub fn code_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Display claims from the ID token. It comes straight from the token endpoint
/// over TLS, so its signature is not re-verified here; it is never used for
/// authorization (the API verifies the access token itself).
fn id_token_profile(id_token: &str) -> (Option<String>, Option<String>) {
    let claims = id_token.split('.').nth(1).and_then(|p| URL_SAFE_NO_PAD.decode(p).ok()).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    match claims {
        Some(c) => (c.get("name").and_then(|v| v.as_str()).map(str::to_string), c.get("email").and_then(|v| v.as_str()).map(str::to_string)),
        None => (None, None),
    }
}

/// A string claim of a JWT's payload (unverified: used to tell accounts apart locally, never
/// for authorization). None for an opaque token.
pub(super) fn jwt_claim(token: &str, name: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let claims: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    claims.get(name)?.as_str().map(str::to_string)
}

pub(super) async fn token_request(form: &[(&str, &str)]) -> Result<TokenResponse> {
    token_request_at(&config::auth_base(), form).await
}

/// [`token_request`] to the cyber-auth at `base`.
async fn token_request_at(base: &str, form: &[(&str, &str)]) -> Result<TokenResponse> {
    let res = crate::http::api_client()?
        .post(format!("{base}/oauth2/token"))
        .timeout(std::time::Duration::from_secs(30))
        .form(form)
        .send()
        .await
        .map_err(|e| Error::Unreachable(format!("token request failed: {e}")))?;
    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        // A server error is an outage, not a refusal of the tokens.
        return Err(if status.is_server_error() {
            Error::Unreachable(format!("token request failed: HTTP {status}"))
        } else {
            Error::Invalid(format!("token request rejected: {body}"))
        });
    }
    res.json().await.map_err(|e| Error::Invalid(format!("invalid token response: {e}")))
}

pub(super) fn session_from(tokens: TokenResponse, previous: Option<&Session>) -> Session {
    let (name, email) = match tokens.id_token.as_deref() {
        Some(t) => id_token_profile(t),
        None => (previous.and_then(|p| p.name.clone()), previous.and_then(|p| p.email.clone())),
    };
    Session {
        access_token: tokens.access_token,
        // Refresh tokens may rotate; keep the old one if none is returned.
        refresh_token: tokens.refresh_token.or_else(|| previous.and_then(|p| p.refresh_token.clone())),
        expires_at: now().saturating_add(tokens.expires_in.unwrap_or(DEFAULT_EXPIRES_IN)),
        name,
        email,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An outage of cyber-auth (no answer, a 5xx) is unreachable; a refusal (`invalid_grant`) is not.
    #[tokio::test]
    async fn an_auth_outage_is_unreachable_and_a_refusal_is_not() {
        use crate::http::tests::serve_once;
        let base = serve_once(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndown".to_vec()).await;
        let err = token_request_at(base.trim_end_matches('/'), &[]).await.err().unwrap();
        assert!(matches!(err, Error::Unreachable(_)), "{err}");
        let body = r#"{"error":"invalid_grant"}"#;
        let base = serve_once(format!("HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes()).await;
        let err = token_request_at(base.trim_end_matches('/'), &[]).await.err().unwrap();
        assert!(matches!(err, Error::Invalid(_)) && err.to_string().contains("invalid_grant"), "{err}");
        let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap();
        assert!(matches!(token_request_at(&format!("http://{closed}"), &[]).await.err().unwrap(), Error::Unreachable(_)));
    }

    #[test]
    fn pkce_challenge_is_base64url_sha256_of_the_verifier() {
        // Cross-checked with:
        // printf '%s' <verifier> | openssl dgst -sha256 -binary | base64 | tr '+/' '-_' | tr -d '='
        assert_eq!(code_challenge("dBjftJeZ4CVP-mJ0kKTiKjKNRKG8W9lLr1J-HQ8YP4U"), "ryJ-YCh3KzrVMsHxmF-ZbP5xcAvmtebmf63k9UP0L3k");
    }

    #[test]
    fn verifiers_are_long_and_unique() {
        let (a, b) = (random_b64(32), random_b64(32));
        assert_eq!(a.len(), 43);
        assert_ne!(a, b);
    }

    fn tokens(access: &str, refresh: Option<&str>, id: Option<&str>, expires_in: Option<u64>) -> TokenResponse {
        TokenResponse { access_token: access.to_string(), refresh_token: refresh.map(str::to_string), id_token: id.map(str::to_string), expires_in }
    }

    fn session(refresh: Option<&str>, name: Option<&str>, email: Option<&str>) -> Session {
        Session {
            access_token: "old-access".into(),
            refresh_token: refresh.map(str::to_string),
            expires_at: 0,
            name: name.map(str::to_string),
            email: email.map(str::to_string),
        }
    }

    #[test]
    fn session_from_carries_over_the_refresh_token_when_none_returned() {
        let prev = session(Some("keep-me"), None, None);
        // Refresh grant that returns no new refresh token: keep the previous one.
        let s = session_from(tokens("new-access", None, None, Some(3600)), Some(&prev));
        assert_eq!(s.access_token, "new-access");
        assert_eq!(s.refresh_token.as_deref(), Some("keep-me"));
        // A rotated refresh token replaces the old one.
        let s = session_from(tokens("new-access", Some("rotated"), None, Some(3600)), Some(&prev));
        assert_eq!(s.refresh_token.as_deref(), Some("rotated"));
    }

    #[test]
    fn session_from_keeps_previous_profile_without_a_new_id_token() {
        let prev = session(Some("r"), Some("Camille"), Some("c@cyberctf.fr"));
        // No id_token in a refresh response: display name/email carry over.
        let s = session_from(tokens("a", None, None, Some(60)), Some(&prev));
        assert_eq!(s.name.as_deref(), Some("Camille"));
        assert_eq!(s.email.as_deref(), Some("c@cyberctf.fr"));
        // A fresh id_token overrides the carried-over profile.
        let payload = URL_SAFE_NO_PAD.encode(br#"{"name":"Noor","email":"n@cyberctf.fr"}"#);
        let s = session_from(tokens("a", None, Some(&format!("h.{payload}.s")), Some(60)), Some(&prev));
        assert_eq!(s.name.as_deref(), Some("Noor"));
        assert_eq!(s.email.as_deref(), Some("n@cyberctf.fr"));
    }

    #[test]
    fn session_from_defaults_expiry_and_has_no_previous_on_first_login() {
        // First login (no previous session) and a missing expires_in: default 3600s lifetime.
        let before = now();
        let s = session_from(tokens("a", Some("r"), None, None), None);
        assert!(s.expires_at >= before + 3600);
        assert_eq!(s.refresh_token.as_deref(), Some("r"));
        assert_eq!(s.name, None);
        assert_eq!(s.email, None);
    }

    #[test]
    fn jwt_claims_need_unpadded_base64url_and_string_values() {
        use base64::engine::general_purpose::URL_SAFE;
        let jwt = |payload: String| format!("h.{payload}.s");
        assert_eq!(jwt_claim(&jwt(URL_SAFE_NO_PAD.encode(br#"{"sub":"u12"}"#)), "sub").as_deref(), Some("u12"));
        // RFC 7515 forbids padding; a padded payload (13 bytes, so `=` at the end) is not read.
        let padded = URL_SAFE.encode(br#"{"sub":"u12"}"#);
        assert!(padded.ends_with('='));
        assert_eq!(jwt_claim(&jwt(padded), "sub"), None);
        // A claim that isn't a string, an empty payload, two segments, and a payload that isn't an object.
        assert_eq!(jwt_claim(&jwt(URL_SAFE_NO_PAD.encode(br#"{"sub":123}"#)), "sub"), None);
        assert_eq!(jwt_claim("h..s", "sub"), None);
        assert_eq!(jwt_claim(&format!("a.{}", URL_SAFE_NO_PAD.encode(br#"{"sub":"u"}"#)), "sub").as_deref(), Some("u"));
        assert_eq!(jwt_claim(&jwt(URL_SAFE_NO_PAD.encode(b"[1,2]")), "sub"), None);
        assert_eq!(id_token_profile(&jwt(URL_SAFE_NO_PAD.encode(br#"{"name":1,"email":["x"]}"#))), (None, None));
    }

    #[test]
    fn a_huge_expiry_saturates_instead_of_wrapping() {
        let s = session_from(tokens("a", None, None, Some(u64::MAX)), None);
        assert_eq!(s.expires_at, u64::MAX);
        // And zero means "already expired", not the default lifetime.
        let s = session_from(tokens("a", None, None, Some(0)), None);
        assert!(s.expires_at <= now());
    }

    #[test]
    fn reads_display_claims_from_id_token() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"u1","name":"Camille","email":"c@cyberctf.fr"}"#);
        let (name, email) = id_token_profile(&format!("h.{payload}.s"));
        assert_eq!(name.as_deref(), Some("Camille"));
        assert_eq!(email.as_deref(), Some("c@cyberctf.fr"));
        assert_eq!(id_token_profile("garbage"), (None, None));
    }
}

#[cfg(test)]
mod proptests;
