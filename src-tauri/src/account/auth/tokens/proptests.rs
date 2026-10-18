//! Property tests for what the launcher reads from tokens it did not make: a JWT's claims (any
//! string can come back from the token endpoint) and the session a token response makes.

use proptest::prelude::*;
use serde_json::{Value, json};

use super::*;
use crate::testing::proptest_support::{config, json as any_json};

fn b64(v: &impl serde::Serialize) -> String {
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(v).unwrap())
}

/// A claim's value: usually a string (the case read), sometimes anything else.
fn claim() -> impl Strategy<Value = Value> {
    prop_oneof![3 => "\\PC{0,30}".prop_map(Value::from), 1 => any_json()]
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn claim_parsing_never_panics(token in "(?s).{0,200}", name in ".{0,10}", payload in any_json(), bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        let _ = jwt_claim(&token, &name);
        let _ = id_token_profile(&token);
        // A payload that decodes, to JSON of any shape (an array, a number...) or to raw bytes.
        for p in [b64(&payload), URL_SAFE_NO_PAD.encode(&bytes)] {
            let token = format!("h.{p}.s");
            let _ = jwt_claim(&token, &name);
            let _ = id_token_profile(&token);
        }
    }

    #[test]
    fn a_string_claim_reads_back_and_anything_else_is_none(
        claims in proptest::collection::btree_map("[a-z_]{1,8}", claim(), 0..6),
        name in "[a-z_]{1,8}",
        header in "[A-Za-z0-9_-]{0,20}",
        sig in "[A-Za-z0-9_.-]{0,40}",
    ) {
        // Anything after the payload (a signature, even one holding more dots) is not read.
        let token = format!("{header}.{}.{sig}", b64(&claims));
        let want = claims.get(&name).and_then(Value::as_str).map(str::to_string);
        prop_assert_eq!(jwt_claim(&token, &name), want);
        let (n, e) = id_token_profile(&token);
        prop_assert_eq!(n, claims.get("name").and_then(Value::as_str).map(str::to_string));
        prop_assert_eq!(e, claims.get("email").and_then(Value::as_str).map(str::to_string));
        // An opaque token (no dot) has no claims.
        prop_assert_eq!(jwt_claim(&header, &name), None);
    }

    #[test]
    fn a_session_keeps_what_the_response_left_out(
        access in "\\PC{1,20}",
        refresh in proptest::option::of("\\PC{1,20}"),
        expires_in in proptest::option::of(any::<u64>()),
        id in proptest::option::of((proptest::option::of("\\PC{0,12}"), proptest::option::of("\\PC{0,12}"))),
        previous in proptest::option::of((proptest::option::of("\\PC{1,12}"), proptest::option::of("\\PC{0,12}"), proptest::option::of("\\PC{0,12}"))),
    ) {
        let id_token = id.as_ref().map(|(name, email)| {
            let mut c = json!({});
            if let Some(n) = name { c["name"] = json!(n); }
            if let Some(e) = email { c["email"] = json!(e); }
            format!("h.{}.s", b64(&c))
        });
        let prev = previous.as_ref().map(|(r, n, e)| Session { access_token: "old".into(), refresh_token: r.clone(), expires_at: 1, name: n.clone(), email: e.clone() });
        let before = now();
        let s = session_from(TokenResponse { access_token: access.clone(), refresh_token: refresh.clone(), expires_in, id_token }, prev.as_ref());
        prop_assert_eq!(&s.access_token, &access);
        prop_assert_eq!(s.refresh_token, refresh.or_else(|| prev.as_ref().and_then(|p| p.refresh_token.clone())));
        // Never in the past, never wrapped around.
        let lifetime = expires_in.unwrap_or(DEFAULT_EXPIRES_IN);
        prop_assert!(s.expires_at >= before.saturating_add(lifetime));
        prop_assert!(s.expires_at <= now().saturating_add(lifetime));
        if let Some((name, email)) = id {
            prop_assert_eq!(s.name, name);
            prop_assert_eq!(s.email, email);
        } else {
            prop_assert_eq!(s.name, prev.as_ref().and_then(|p| p.name.clone()));
            prop_assert_eq!(s.email, prev.as_ref().and_then(|p| p.email.clone()));
        }
    }

    #[test]
    fn the_code_challenge_is_a_43_char_base64url_digest(verifier in "\\PC{0,128}") {
        let c = code_challenge(&verifier);
        prop_assert_eq!(c.len(), 43);
        prop_assert!(c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'));
        prop_assert_eq!(URL_SAFE_NO_PAD.decode(&c).unwrap(), Sha256::digest(verifier.as_bytes()).to_vec());
    }
}
