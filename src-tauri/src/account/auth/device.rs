//! Sign-in with a code (OAuth device authorization grant, RFC 8628), for a machine whose browser
//! can't be used for the sign-in: cyber-auth gives a short code, the player enters it on
//! cyberauth.co from any device, and the launcher polls the token endpoint until they did.

use std::future::Future;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::SCOPES;
use super::tokens::{Refusal, TokenResponse, post_form};
use crate::error::{Code, Error, Result};

/// The grant type of a device code at the token endpoint (RFC 8628 §3.4).
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// How often to poll when cyber-auth doesn't say (RFC 8628 §3.2).
const DEFAULT_INTERVAL_SECS: u64 = 5;
/// What a `slow_down` adds to the interval (RFC 8628 §3.5).
const SLOW_DOWN_SECS: u64 = 5;
/// How long a code is waited for, whatever cyber-auth says: the player sees it expire, not an
/// endless wait.
const MAX_LIFETIME_SECS: u64 = 30 * 60;
/// Unreachable polls in a row before the sign-in gives up: a blip is waited out, a lost
/// connection is said.
const UNREACHABLE_POLLS: u32 = 3;

/// cyber-auth's answer to a device authorization request (RFC 8628 §3.2).
#[derive(Deserialize)]
pub(super) struct Grant {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    expires_in: u64,
    interval: Option<u64>,
}

/// What the player needs to finish signing in elsewhere: the code, and where to enter it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCode {
    user_code: String,
    verification_uri: String,
    /// The same page with the code filled in, when cyber-auth gives one.
    verification_uri_complete: Option<String>,
    /// Seconds the code stays valid.
    expires_in: u64,
}

impl Grant {
    /// The part of the grant the player sees (never the device code, which only this app uses).
    pub(super) fn shown(&self) -> DeviceCode {
        DeviceCode {
            user_code: self.user_code.clone(),
            verification_uri: self.verification_uri.clone(),
            verification_uri_complete: self.verification_uri_complete.clone(),
            expires_in: self.lifetime().as_secs(),
        }
    }

    /// How long the code is waited for.
    pub(super) fn lifetime(&self) -> Duration {
        Duration::from_secs(self.expires_in.clamp(1, MAX_LIFETIME_SECS))
    }
}

/// A refusal of the device authorization request: cyber-auth doesn't let this app sign in with
/// a code (the grant isn't enabled on its OAuth client), or another reason.
fn request_refused(refusal: &Refusal) -> Error {
    match refusal.error.as_str() {
        "unauthorized_client" | "unsupported_grant_type" | "invalid_client" => Error::coded(Code::SignInCodeUnavailable),
        _ => Error::Invalid(format!("sign-in with a code was refused: {}", refusal.body)),
    }
}

/// Asks the cyber-auth at `base` for a code.
pub(super) async fn request(base: &str, client_id: &str, api: &str) -> Result<Grant> {
    let form = [("client_id", client_id), ("scope", SCOPES), ("resource", api)];
    post_form(&format!("{base}/device/code"), "sign-in code request", &form).await?.map_err(|r| request_refused(&r))
}

/// Polls the token endpoint at `base` until the player approved the code (its tokens), denied it,
/// or it expired. The tokens are for `api` (the token audience, as for the browser sign-in).
/// `sleep` waits between polls (the tests pass one that doesn't).
pub(super) async fn poll<S, F>(base: &str, client_id: &str, api: &str, grant: &Grant, sleep: S) -> Result<TokenResponse>
where
    S: Fn(Duration) -> F,
    F: Future<Output = ()>,
{
    let url = format!("{base}/oauth2/token");
    let form = [("grant_type", DEVICE_GRANT), ("device_code", grant.device_code.as_str()), ("client_id", client_id), ("resource", api)];
    let mut interval = grant.interval.unwrap_or(DEFAULT_INTERVAL_SECS);
    let mut unreachable = 0;
    loop {
        sleep(Duration::from_secs(interval)).await;
        let refusal = match post_form::<TokenResponse>(&url, "token request", &form).await {
            Ok(Ok(tokens)) => return Ok(tokens),
            Ok(Err(refusal)) => refusal,
            Err(Error::Unreachable(why)) => {
                unreachable += 1;
                if unreachable >= UNREACHABLE_POLLS {
                    return Err(Error::Unreachable(why));
                }
                continue;
            }
            Err(e) => return Err(e),
        };
        unreachable = 0;
        match refusal.error.as_str() {
            "authorization_pending" => {}
            "slow_down" => interval += SLOW_DOWN_SECS,
            "access_denied" => return Err(Error::coded(Code::SignInDenied)),
            "expired_token" => return Err(Error::coded(Code::LoginTimeout { minutes: grant.lifetime().as_secs().div_ceil(60) })),
            _ => return Err(Error::Invalid(format!("token request rejected: {}", refusal.body))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::super::testing::serve;
    use super::*;

    const GRANT: &str = r#"{"device_code":"dev-1","user_code":"WDJB-MJHT","verification_uri":"https://auth.example/device","verification_uri_complete":"https://auth.example/device?user_code=WDJB-MJHT","expires_in":900,"interval":2}"#;
    const PENDING: (u16, &str) = (400, r#"{"error":"authorization_pending"}"#);
    const TOKENS: (u16, &str) = (200, r#"{"access_token":"at","refresh_token":"rt","expires_in":3600}"#);

    fn grant(interval: Option<u64>) -> Grant {
        Grant {
            device_code: "dev-1".into(),
            user_code: "WDJB-MJHT".into(),
            verification_uri: "https://auth.example/device".into(),
            verification_uri_complete: None,
            expires_in: 600,
            interval,
        }
    }

    /// A `sleep` that doesn't, and the waits it was asked for (in seconds).
    fn recorded() -> (impl Fn(Duration) -> std::future::Ready<()>, Arc<Mutex<Vec<u64>>>) {
        let waits = Arc::new(Mutex::new(Vec::new()));
        let seen = waits.clone();
        (
            move |d: Duration| {
                seen.lock().unwrap().push(d.as_secs());
                std::future::ready(())
            },
            waits,
        )
    }

    #[tokio::test]
    async fn the_code_request_asks_for_the_launchers_scopes_and_shows_the_code_only() {
        let (base, requests) = serve(vec![(200, GRANT)]).await;
        let grant = request(&base, "client", "https://api.example").await.unwrap();
        let request = requests.lock().unwrap()[0].clone();
        assert_eq!(request.path, "/device/code");
        assert_eq!(request.form["client_id"], "client");
        assert_eq!(request.form["scope"], SCOPES);
        assert_eq!(request.form["resource"], "https://api.example");
        let shown = serde_json::to_value(grant.shown()).unwrap();
        assert_eq!(
            shown,
            serde_json::json!({
                "userCode": "WDJB-MJHT",
                "verificationUri": "https://auth.example/device",
                "verificationUriComplete": "https://auth.example/device?user_code=WDJB-MJHT",
                "expiresIn": 900,
            })
        );
        assert!(!shown.to_string().contains("dev-1"), "the device code reached the webview");
    }

    /// cyber-auth answers `unauthorized_client` while the launcher's OAuth client lacks the grant.
    #[tokio::test]
    async fn a_client_not_allowed_to_use_codes_says_so() {
        let body =
            r#"{"error":"unauthorized_client","error_description":"client is not authorized to use grant type urn:ietf:params:oauth:grant-type:device_code"}"#;
        let (base, _) = serve(vec![(400, body)]).await;
        let err = request(&base, "client", "api").await.err().unwrap();
        assert_eq!(err.code(), Code::SignInCodeUnavailable);
        let (base, _) = serve(vec![(400, r#"{"error":"invalid_scope"}"#)]).await;
        let err = request(&base, "client", "api").await.err().unwrap();
        assert!(err.to_string().contains("invalid_scope"), "{err}");
    }

    #[tokio::test]
    async fn polling_waits_out_pending_and_slows_down_when_told() {
        let (base, requests) = serve(vec![PENDING, (400, r#"{"error":"slow_down"}"#), PENDING, TOKENS]).await;
        let (sleep, waits) = recorded();
        let tokens = poll(&base, "client", "api", &grant(Some(2)), sleep).await.unwrap();
        assert_eq!(tokens.access_token, "at");
        assert_eq!(tokens.refresh_token.as_deref(), Some("rt"));
        assert_eq!(*waits.lock().unwrap(), [2, 2, 7, 7]);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[0].path, "/oauth2/token");
        assert_eq!(requests[0].form["grant_type"], DEVICE_GRANT);
        assert_eq!(requests[0].form["device_code"], "dev-1");
        assert_eq!(requests[0].form["client_id"], "client");
        assert_eq!(requests[0].form["resource"], "api");
    }

    #[tokio::test]
    async fn polling_defaults_to_five_seconds() {
        let (base, _) = serve(vec![TOKENS]).await;
        let (sleep, waits) = recorded();
        poll(&base, "client", "api", &grant(None), sleep).await.unwrap();
        assert_eq!(*waits.lock().unwrap(), [DEFAULT_INTERVAL_SECS]);
    }

    #[tokio::test]
    async fn a_denied_or_expired_code_ends_the_sign_in_with_why() {
        let (base, _) = serve(vec![PENDING, (400, r#"{"error":"access_denied"}"#)]).await;
        let err = poll(&base, "client", "api", &grant(Some(0)), recorded().0).await.err().unwrap();
        assert_eq!(err.code(), Code::SignInDenied);
        let (base, _) = serve(vec![(400, r#"{"error":"expired_token"}"#)]).await;
        let err = poll(&base, "client", "api", &grant(Some(0)), recorded().0).await.err().unwrap();
        assert_eq!(err.code(), Code::LoginTimeout { minutes: 10 });
        let (base, _) = serve(vec![(400, r#"{"error":"invalid_grant"}"#)]).await;
        let err = poll(&base, "client", "api", &grant(Some(0)), recorded().0).await.err().unwrap();
        assert!(err.to_string().contains("invalid_grant"), "{err}");
    }

    /// One outage is waited out; several in a row end the sign-in as unreachable.
    #[tokio::test]
    async fn polling_rides_out_a_blip_but_not_a_lost_connection() {
        let (base, _) = serve(vec![(503, "{}"), PENDING, TOKENS]).await;
        assert_eq!(poll(&base, "client", "api", &grant(Some(0)), recorded().0).await.unwrap().access_token, "at");
        let (base, _) = serve(vec![(503, "{}"), (502, "{}"), (503, "{}")]).await;
        let err = poll(&base, "client", "api", &grant(Some(0)), recorded().0).await.err().unwrap();
        assert!(matches!(err, Error::Unreachable(_)), "{err}");
    }

    #[test]
    fn a_code_is_waited_for_at_most_half_an_hour() {
        let long = Grant { expires_in: 86_400, ..grant(None) };
        assert_eq!(long.lifetime(), Duration::from_secs(MAX_LIFETIME_SECS));
        assert_eq!(long.shown().expires_in, MAX_LIFETIME_SECS);
        let zero = Grant { expires_in: 0, ..grant(None) };
        assert_eq!(zero.lifetime(), Duration::from_secs(1));
    }
}
