//! The sign-in redirect as a `cyberctf://` link (RFC 8252's private-use URI scheme): the browser
//! hands cyber-auth's answer straight to the app, with no local server, no localhost page and no
//! port that Windows may have reserved. PKCE keeps it safe: another program that took the link
//! would hold a code it can't exchange without this sign-in's verifier.

use std::sync::Mutex;

use tokio::sync::oneshot;

/// The redirect URI, registered as is on the OAuth client in cyber-auth.
pub const REDIRECT_URI: &str = "cyberctf://auth/callback";

/// The sign-in waiting for its link: its `state`, and where to hand the link.
static PENDING: Mutex<Option<(String, oneshot::Sender<url::Url>)>> = Mutex::new(None);

fn pending() -> std::sync::MutexGuard<'static, Option<(String, oneshot::Sender<url::Url>)>> {
    PENDING.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Waits for the link of the sign-in that sent `state`. A newer sign-in replaces it (the older
/// one's receiver then ends with an error).
pub(super) fn expect(state: &str) -> oneshot::Receiver<url::Url> {
    let (tx, rx) = oneshot::channel();
    *pending() = Some((state.to_owned(), tx));
    rx
}

/// Whether `url` is a sign-in link (`cyberctf://auth/callback?…`). One carrying the waiting
/// sign-in's `state` goes to it; any other (an old browser tab, a forged link) is dropped.
pub fn deliver(url: &url::Url) -> bool {
    if !(url.scheme() == "cyberctf" && url.host_str() == Some("auth") && url.path() == "/callback") {
        return false;
    }
    let mut waiting = pending();
    match waiting.take() {
        Some((state, tx)) if super::callback::state_matches(url, &state) => {
            let _ = tx.send(url.clone());
        }
        other => {
            *waiting = other;
            log::warn!("ignored a sign-in link that isn't the current sign-in's");
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_sign_ins_link_finishes_it() {
        let link = |q: &str| url::Url::parse(&format!("{REDIRECT_URI}?{q}")).unwrap();
        // Not sign-in links: left to the app (a lab link) or ignored.
        assert!(!deliver(&url::Url::parse("cyberctf://labs/web-sqli").unwrap()));
        assert!(!deliver(&url::Url::parse("https://auth/callback?code=x&state=s1").unwrap()));

        let mut rx = expect("s1");
        // Another sign-in's link, or one without a state: taken, dropped, still waiting.
        assert!(deliver(&link("code=evil&state=other")));
        assert!(deliver(&link("code=evil")));
        assert!(rx.try_recv().is_err());
        assert!(deliver(&link("code=c1&state=s1")));
        let got = rx.try_recv().unwrap();
        assert_eq!(super::super::callback::outcome(&got).unwrap(), ("c1".to_string(), "s1".to_string()));
        // Used once: the same link again finds nobody waiting.
        assert!(deliver(&link("code=c1&state=s1")));

        // A newer sign-in replaces an older one, whose wait ends.
        let older = expect("a");
        let mut newer = expect("b");
        assert!(older.blocking_recv().is_err());
        assert!(deliver(&link("error=access_denied&state=b")));
        assert!(super::super::callback::outcome(&newer.try_recv().unwrap()).is_err());
    }
}
