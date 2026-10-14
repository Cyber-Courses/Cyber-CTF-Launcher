//! Property tests for the PTY's text decoding: however the shell's output is cut into reads, the
//! text shown is the text sent, and at most a character's start is held back.

use proptest::prelude::*;

use super::*;
use crate::proptest_support::config;

/// Cut points into `len` bytes, sorted (repeats make empty reads).
fn cuts(len: usize, raw: &[prop::sample::Index]) -> Vec<usize> {
    let mut c: Vec<usize> = raw.iter().map(|i| i.index(len + 1)).collect();
    c.sort_unstable();
    c
}

/// Feeds `bytes` as reads cut at `cuts`, as the PTY reader does; returns the text taken after
/// each read and the largest number of bytes held back between reads.
fn feed(bytes: &[u8], cuts: &[usize]) -> (String, Vec<u8>, usize) {
    let mut buf = Vec::new();
    let mut out = String::new();
    let mut held = 0;
    let mut at = 0;
    for &cut in cuts.iter().chain([&bytes.len()]) {
        buf.extend_from_slice(&bytes[at..cut]);
        at = cut;
        out.push_str(&take_text(&mut buf));
        held = held.max(buf.len());
    }
    (out, buf, held)
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn valid_text_comes_through_whole_however_it_is_cut(text in "\\PC{0,80}|(?s).{0,80}", raw in proptest::collection::vec(any::<prop::sample::Index>(), 0..12)) {
        let bytes = text.as_bytes();
        let (out, rest, held) = feed(bytes, &cuts(bytes.len(), &raw));
        prop_assert_eq!(out, text);
        prop_assert!(rest.is_empty());
        prop_assert!(held <= 3, "held {} bytes", held);
    }

    /// Any bytes at all (a binary dump to the terminal): no panic, never more than 3 bytes held,
    /// and the text is what decoding the whole output at once gives.
    #[test]
    fn any_bytes_decode_as_if_read_at_once(bytes in proptest::collection::vec(prop_oneof![any::<u8>(), 0x80u8..0xC0, 0xC0u8..0xF8], 0..80), raw in proptest::collection::vec(any::<prop::sample::Index>(), 0..12)) {
        let (out, rest, held) = feed(&bytes, &cuts(bytes.len(), &raw));
        prop_assert!(held <= 3, "held {} bytes", held);
        prop_assert!(rest.len() <= 3);
        let whole = String::from_utf8_lossy(&bytes);
        prop_assert_eq!(format!("{out}{}", String::from_utf8_lossy(&rest)), whole.into_owned());
    }

    #[test]
    fn incomplete_tail_counts_only_a_cut_off_character(text in "\\PC{0,20}", ch in any::<char>(), keep in 0usize..4) {
        let mut enc = [0u8; 4];
        let c = ch.encode_utf8(&mut enc).as_bytes();
        let keep = keep.min(c.len());
        let mut bytes = text.into_bytes();
        bytes.extend_from_slice(&c[..keep]);
        let want = if keep < c.len() { keep } else { 0 };
        prop_assert_eq!(incomplete_tail(&bytes), want);
    }
}
