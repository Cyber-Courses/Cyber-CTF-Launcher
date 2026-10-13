//! Property tests for the loopback redirect: whatever a browser (or anything else on the machine)
//! sends to the callback port is read without a panic, bounded, and a well-formed redirect gives
//! back exactly the code and state it carried.

use proptest::prelude::*;

use super::*;
use crate::proptest_support::config;

/// A query value percent-encoded as a browser sends it.
fn encoded(v: &str) -> String {
    url::form_urlencoded::byte_serialize(v.as_bytes()).collect()
}

/// The longest a request head can be read as: the read stops at the first full chunk past 8 KiB.
const MAX_READ: usize = 8192 + 1024;

/// Where `read_request` must have read up to, at least: the end of the first blank line, or 8 KiB.
fn must_read(bytes: &[u8]) -> usize {
    let crlf = bytes.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4);
    let lf = bytes.windows(2).position(|w| w == b"\n\n").map(|i| i + 2);
    [crlf, lf, Some(8192), Some(bytes.len())].into_iter().flatten().min().unwrap_or(0)
}

/// Sends `bytes` in `cuts` pieces to a loopback connection, closes it, and returns what
/// `read_request` read on the other side.
fn read_over_loopback(bytes: Vec<u8>, cuts: Vec<usize>) -> Option<String> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async move {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let writer = tokio::spawn(async move {
            let mut client = TcpStream::connect(addr).await.unwrap();
            let mut at = 0;
            for cut in cuts.into_iter().chain([bytes.len()]) {
                let end = cut.clamp(at, bytes.len());
                // The reader may stop early and close: a failed write is fine.
                if client.write_all(&bytes[at..end]).await.is_err() {
                    return;
                }
                let _ = client.flush().await;
                at = end;
            }
            let _ = client.shutdown().await;
        });
        let (mut server, _) = listener.accept().await.unwrap();
        let got = read_request(&mut server).await;
        drop(server);
        let _ = writer.await;
        got
    })
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn request_parsing_never_panics(request in "(?s).{0,300}", state in ".{0,20}") {
        if let Some(url) = request_url(&request) {
            let _ = outcome(&url);
            let _ = state_matches(&url, &state);
        }
        // As the browser sends it, with whatever target.
        let _ = request_url(&format!("GET {request} HTTP/1.1"));
    }

    #[test]
    fn a_redirect_gives_back_its_code_and_state(
        code in "\\PC{0,40}",
        state in "\\PC{1,40}",
        extra in proptest::collection::vec(("[a-z_]{1,8}", "\\PC{0,12}"), 0..3),
        headers in "(Host: 127\\.0\\.0\\.1:47290\r\n)?(User-Agent: [ -~]{0,30}\r\n)?",
    ) {
        let mut pairs: Vec<(String, String)> = extra.into_iter().filter(|(k, _)| !["code", "state", "error"].contains(&k.as_str())).collect();
        pairs.insert(0, ("code".into(), code.clone()));
        pairs.push(("state".into(), state.clone()));
        let query = pairs.iter().map(|(k, v)| format!("{k}={}", encoded(v))).collect::<Vec<_>>().join("&");
        let request = format!("GET /callback?{query} HTTP/1.1\r\n{headers}\r\n");
        let url = request_url(&request).expect("a redirect parses");
        prop_assert_eq!(url.path(), "/callback");
        prop_assert!(state_matches(&url, &state));
        let other = format!("{state}x");
        prop_assert!(!state_matches(&url, &other));
        let (c, s) = outcome(&url).expect("code and state");
        prop_assert_eq!(c, code);
        prop_assert_eq!(s, state);
    }

    #[test]
    fn an_error_in_the_redirect_always_fails_the_sign_in(error in "\\PC{0,30}", code in "[a-zA-Z0-9]{0,10}", error_first in any::<bool>()) {
        let query = if error_first { format!("error={}&code={code}&state=s", encoded(&error)) } else { format!("code={code}&state=s&error={}", encoded(&error)) };
        let url = request_url(&format!("GET /callback?{query} HTTP/1.1")).unwrap();
        let err = outcome(&url).unwrap_err().to_string();
        prop_assert!(err.contains(&error), "{}", err);
    }
}

proptest! {
    // Each case opens a loopback connection: fewer of them.
    #![proptest_config(ProptestConfig { cases: 48, ..config() })]

    #[test]
    fn read_request_reads_a_bounded_prefix_of_any_bytes(
        bytes in prop_oneof![
            proptest::collection::vec(any::<u8>(), 0..600),
            // Long enough to reach the 8 KiB cap, sometimes with a blank line inside.
            (proptest::collection::vec(any::<u8>(), 0..12000), any::<prop::sample::Index>(), any::<bool>()).prop_map(|(mut b, at, lf)| {
                if !b.is_empty() {
                    let i = at.index(b.len());
                    b.splice(i..i, if lf { b"\n\n".to_vec() } else { b"\r\n\r\n".to_vec() });
                }
                b
            }),
        ],
        cuts in proptest::collection::vec(0usize..12000, 0..6),
    ) {
        let mut cuts = cuts;
        cuts.sort_unstable();
        let got = read_over_loopback(bytes.clone(), cuts);
        if bytes.is_empty() {
            prop_assert_eq!(got, None);
            return Ok(());
        }
        let got = got.expect("bytes were sent before the close");
        // What was read is the decoded text of a prefix of what was sent, past its blank line
        // (or 8 KiB), and never more than a chunk past that.
        let low = must_read(&bytes);
        let high = (low + 1023).min(bytes.len()).min(MAX_READ);
        let matches = (low..=high).any(|k| String::from_utf8_lossy(&bytes[..k]) == got.as_str());
        prop_assert!(matches, "read {} chars, expected a prefix of {}..={} bytes", got.chars().count(), low, high);
    }
}
