//! Property tests for splitting a long session across keychain entries: any session up to the
//! largest kept comes back whole, and every entry fits a Windows credential.

use proptest::prelude::*;

use super::*;
use crate::testing::proptest_support::config;

/// Text of any size up to past the most that is kept, mixing one- and two-unit (UTF-16)
/// characters so piece boundaries land everywhere.
fn text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        3 => "\\PC{0,20}",
        1 => ("[a-z]", 1usize..2000).prop_map(|(s, n)| s.repeat(n)),
        1 => ("[\u{10000}-\u{10ffff}]", 1usize..800).prop_map(|(s, n)| s.repeat(n)),
        1 => ("[\u{80}-\u{ffff}]", 1usize..800).prop_map(|(s, n)| s.repeat(n)),
    ];
    proptest::collection::vec(piece, 0..40).prop_map(|p| p.concat())
}

/// A session as it is saved: its JSON, so it never starts with the pieces marker.
fn session_json() -> impl Strategy<Value = String> {
    (text(), proptest::option::of(text()), any::<u64>(), proptest::option::of("\\PC{0,40}")).prop_map(|(access, refresh, expires_at, name)| {
        serde_json::to_string(&Session { access_token: access, refresh_token: refresh, expires_at, name, email: None }).unwrap()
    })
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn a_session_round_trips_through_the_keychain_entries_or_is_refused_whole(raw in prop_oneof![session_json(), text()]) {
        prop_assume!(!raw.starts_with(CHUNKS_PREFIX));
        let units = raw.encode_utf16().count();
        match split_chunks(&raw) {
            Ok((head, parts)) => {
                prop_assert!(head.encode_utf16().count() <= CHUNK_UNITS);
                prop_assert!(parts.len() <= MAX_CHUNKS);
                prop_assert!(parts.iter().all(|p| !p.is_empty() && p.encode_utf16().count() <= CHUNK_UNITS));
                if units <= CHUNK_UNITS {
                    prop_assert!(parts.is_empty());
                } else {
                    // As few pieces as the two-unit characters allow.
                    prop_assert!(parts.len() >= units.div_ceil(CHUNK_UNITS));
                    prop_assert!(parts.len() <= units.div_ceil(CHUNK_UNITS - 1));
                }
                if !parts.is_empty() {
                    prop_assert_eq!(parts.concat(), raw.clone());
                }
                let back = join_chunks(head.clone(), |i| parts.get(i - 1).cloned());
                prop_assert_eq!(back.as_deref(), Some(raw.as_str()));
                // Any missing piece reads as no session, never a cut one.
                for missing in 1..=parts.len() {
                    prop_assert_eq!(join_chunks(head.clone(), |i| (i != missing).then(|| parts[i - 1].clone())), None);
                }
            }
            Err(e) => {
                prop_assert!(units > (CHUNK_UNITS - 1) * MAX_CHUNKS, "{} units refused", units);
                prop_assert!(e.to_string().contains("too large"));
            }
        }
    }

    #[test]
    fn any_head_reads_without_a_panic(head in prop_oneof!["\\PC{0,30}", "cyberctf-chunks:[0-9+-]{0,25}", "cyberctf-chunks:.{0,10}"], piece in "\\PC{0,10}") {
        let mut asked = Vec::new();
        let got = join_chunks(head.clone(), |i| {
            asked.push(i);
            Some(piece.clone())
        });
        if head.starts_with(CHUNKS_PREFIX) {
            // Only pieces 1..=MAX_CHUNKS are ever asked for, in order.
            prop_assert!(asked.len() <= MAX_CHUNKS);
            let n = asked.len();
            prop_assert_eq!(asked, (1..=n).collect::<Vec<_>>());
        } else {
            prop_assert_eq!(got, Some(head));
        }
    }
}
