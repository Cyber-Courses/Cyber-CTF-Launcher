//! The little of `WebSocket` (RFC 6455) the screen bridge speaks with its one viewer: the
//! handshake's accept key, and frames read from the viewer (masked, as a browser sends them) and
//! written to it (unmasked). No extension is ever negotiated, so a frame's payload is the
//! application's bytes as they are.

use base64::Engine;

/// The GUID RFC 6455 appends to a client's key.
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// The largest frame the bridge takes from the viewer: a VNC client's messages are small.
pub(super) const MAX_PAYLOAD: u64 = 1 << 20;

/// `Sec-`WebSocket`-Accept` for a client's `Sec-`WebSocket`-Key`.
pub(super) fn accept_key(key: &str) -> String {
    let digest = sha1(format!("{key}{GUID}").as_bytes());
    base64::engine::general_purpose::STANDARD.encode(digest)
}

/// SHA-1 of `data` (FIPS 180-4): only the handshake's accept key uses it, as RFC 6455 says.
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut state: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let bits = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    let (blocks, _) = msg.as_chunks::<64>();
    for block in blocks {
        let mut schedule = [0u32; 80];
        let (words, _) = block.as_chunks::<4>();
        for (slot, word) in schedule.iter_mut().zip(words) {
            *slot = u32::from_be_bytes(*word);
        }
        for round in 16..80 {
            schedule[round] = (schedule[round - 3] ^ schedule[round - 8] ^ schedule[round - 14] ^ schedule[round - 16]).rotate_left(1);
        }
        let mut work = state;
        for (round, word) in schedule.iter().enumerate() {
            let [s0, s1, s2, s3, s4] = work;
            let (mix, constant) = match round {
                0..=19 => ((s1 & s2) | (!s1 & s3), 0x5A82_7999),
                20..=39 => (s1 ^ s2 ^ s3, 0x6ED9_EBA1),
                40..=59 => ((s1 & s2) | (s1 & s3) | (s2 & s3), 0x8F1B_BCDC),
                _ => (s1 ^ s2 ^ s3, 0xCA62_C1D6),
            };
            let next = s0.rotate_left(5).wrapping_add(mix).wrapping_add(s4).wrapping_add(constant).wrapping_add(*word);
            work = [next, s0, s1.rotate_left(30), s2, s3];
        }
        for (kept, added) in state.iter_mut().zip(work) {
            *kept = kept.wrapping_add(added);
        }
    }
    let mut out = [0u8; 20];
    let (digest, _) = out.as_chunks_mut::<4>();
    for (chunk, word) in digest.iter_mut().zip(state) {
        *chunk = word.to_be_bytes();
    }
    out
}

/// What a frame from the viewer carries.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Frame {
    /// Application bytes (a binary, text or continuation frame's).
    Data(Vec<u8>),
    /// A ping, answered with its payload.
    Ping(Vec<u8>),
    /// A pong: nothing to do.
    Pong,
    /// The viewer closes.
    Close,
}

/// Why the viewer's bytes aren't a frame the bridge takes.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Refused {
    /// An unmasked frame: a browser masks every frame it sends.
    Unmasked,
    /// A payload over [`MAX_PAYLOAD`].
    TooLarge,
    /// An opcode RFC 6455 doesn't define, or reserved bits set (no extension was agreed).
    Malformed,
}

/// The first frame in `buf`, and how many bytes it took; `None` while it is incomplete.
pub(super) fn decode(buf: &[u8]) -> Result<Option<(Frame, usize)>, Refused> {
    let (Some(&b0), Some(&b1)) = (buf.first(), buf.get(1)) else { return Ok(None) };
    if b0 & 0x70 != 0 {
        return Err(Refused::Malformed);
    }
    if b1 & 0x80 == 0 {
        return Err(Refused::Unmasked);
    }
    let (len, mut at) = match b1 & 0x7F {
        126 => match buf.get(2..4) {
            Some(b) => (u64::from(u16::from_be_bytes([b[0], b[1]])), 4),
            None => return Ok(None),
        },
        127 => match buf.get(2..10) {
            Some(b) => (u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]), 10),
            None => return Ok(None),
        },
        n => (u64::from(n), 2),
    };
    if len > MAX_PAYLOAD {
        return Err(Refused::TooLarge);
    }
    let Some(mask) = buf.get(at..at + 4) else { return Ok(None) };
    let mask = [mask[0], mask[1], mask[2], mask[3]];
    at += 4;
    let len = usize::try_from(len).map_err(|_| Refused::TooLarge)?;
    let Some(payload) = buf.get(at..at + len) else { return Ok(None) };
    let data: Vec<u8> = payload.iter().zip(mask.iter().cycle()).map(|(b, m)| b ^ m).collect();
    let frame = match b0 & 0x0F {
        0x0..=0x2 => Frame::Data(data),
        0x8 => Frame::Close,
        0x9 => Frame::Ping(data),
        0xA => Frame::Pong,
        _ => return Err(Refused::Malformed),
    };
    Ok(Some((frame, at + len)))
}

/// A binary frame to the viewer holding `payload`.
pub(super) fn binary(payload: &[u8]) -> Vec<u8> {
    frame(0x2, payload)
}

/// A pong to the viewer, echoing `payload`.
pub(super) fn pong(payload: &[u8]) -> Vec<u8> {
    frame(0xA, payload)
}

/// A close frame to the viewer.
pub(super) fn close() -> Vec<u8> {
    frame(0x8, &[])
}

/// An unmasked, final frame of `opcode` holding `payload`.
fn frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0x80 | opcode];
    match payload.len() {
        n @ 0..=125 => out.push(u8::try_from(n).unwrap_or(125)),
        n @ 126..=0xFFFF => {
            out.push(126);
            out.extend_from_slice(&u16::try_from(n).unwrap_or(u16::MAX).to_be_bytes());
        }
        n => {
            out.push(127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    out.extend_from_slice(payload);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame as a browser sends it: masked with `mask`.
    fn masked(opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
        let mut out = vec![0x80 | opcode];
        let len = payload.len();
        if len < 126 {
            out.push(0x80 | u8::try_from(len).unwrap());
        } else {
            out.push(0x80 | 0x7E);
            out.extend_from_slice(&u16::try_from(len).unwrap().to_be_bytes());
        }
        out.extend_from_slice(&mask);
        out.extend(payload.iter().zip(mask.iter().cycle()).map(|(b, m)| b ^ m));
        out
    }

    #[test]
    fn the_accept_key_is_rfc_6455s() {
        // RFC 6455, section 1.3.
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
        // FIPS 180-2's "abc".
        let hex = sha1(b"abc").iter().fold(String::new(), |mut out, b| {
            use std::fmt::Write as _;
            let _ = write!(out, "{b:02x}");
            out
        });
        assert_eq!(hex, "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn a_masked_frame_reads_back_and_a_partial_one_waits() {
        let f = masked(0x2, b"RFB 003.008\n", [1, 2, 3, 4]);
        assert_eq!(decode(&f), Ok(Some((Frame::Data(b"RFB 003.008\n".to_vec()), f.len()))));
        assert_eq!(decode(&f[..f.len() - 1]), Ok(None));
        assert_eq!(decode(&f[..1]), Ok(None));
        let long = vec![7u8; 300];
        let l = masked(0x2, &long, [9, 8, 7, 6]);
        assert_eq!(decode(&l), Ok(Some((Frame::Data(long), l.len()))));
        assert_eq!(decode(&masked(0x9, b"hi", [0; 4])).unwrap().unwrap().0, Frame::Ping(b"hi".to_vec()));
        assert_eq!(decode(&masked(0x8, b"", [0; 4])).unwrap().unwrap().0, Frame::Close);
    }

    #[test]
    fn an_unmasked_reserved_or_huge_frame_is_refused() {
        assert_eq!(decode(&[0x82, 0x01, 0x00]), Err(Refused::Unmasked));
        assert_eq!(decode(&[0xC2, 0x81, 0, 0, 0, 0, 0]), Err(Refused::Malformed), "a compression bit no one agreed");
        assert_eq!(decode(&[0x83, 0x80, 0, 0, 0, 0]), Err(Refused::Malformed), "a reserved opcode");
        let mut huge = vec![0x82, 0x80 | 127];
        huge.extend_from_slice(&(MAX_PAYLOAD + 1).to_be_bytes());
        assert_eq!(decode(&huge), Err(Refused::TooLarge));
    }

    #[test]
    fn frames_to_the_viewer_are_unmasked_and_sized() {
        assert_eq!(binary(b"ab"), [0x82, 2, b'a', b'b']);
        let mid = binary(&[0; 200]);
        assert_eq!(&mid[..4], &[0x82, 126, 0, 200]);
        let big = binary(&vec![0; 70_000]);
        assert_eq!(&big[..2], &[0x82, 127]);
        assert_eq!(pong(b"x"), [0x8A, 1, b'x']);
        assert_eq!(close(), [0x88, 0]);
    }
}
