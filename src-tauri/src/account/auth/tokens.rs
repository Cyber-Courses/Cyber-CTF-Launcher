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
    let res = reqwest::Client::new()
        .post(format!("{}/oauth2/token", config::auth_base()))
        .timeout(std::time::Duration::from_secs(30))
        .form(form)
        .send()
        .await
        .map_err(|e| Error::Invalid(format!("token request failed: {e}")))?;
    if !res.status().is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(Error::Invalid(format!("token request rejected: {body}")));
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
    fn reads_display_claims_from_id_token() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"u1","name":"Camille","email":"c@cyberctf.fr"}"#);
        let (name, email) = id_token_profile(&format!("h.{payload}.s"));
        assert_eq!(name.as_deref(), Some("Camille"));
        assert_eq!(email.as_deref(), Some("c@cyberctf.fr"));
        assert_eq!(id_token_profile("garbage"), (None, None));
    }
}
