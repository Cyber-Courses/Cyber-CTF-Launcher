//! Property tests for reading a tool's output line by line: any bytes, arriving in any pieces,
//! come out as the lines they hold, invalid UTF-8 replaced, nothing lost or merged.

use std::pin::Pin;
use std::task::{Context, Poll};

use proptest::prelude::*;
use tokio::io::{AsyncRead, BufReader, ReadBuf};

// Only the Unix test below uses `forward_lines` (it runs a real `sh`).
#[cfg(unix)]
use super::*;
use crate::testing::proptest_support::config;

/// A pipe that hands out its bytes in the given pieces, one per read, then ends.
struct Pieces(std::collections::VecDeque<Vec<u8>>);

impl AsyncRead for Pieces {
    fn poll_read(mut self: Pin<&mut Self>, _: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        if let Some(mut piece) = self.0.pop_front() {
            let n = piece.len().min(buf.remaining());
            buf.put_slice(&piece[..n]);
            if n < piece.len() {
                self.0.push_front(piece.split_off(n));
            }
        }
        Poll::Ready(Ok(()))
    }
}

fn pieces(bytes: &[u8], raw: &[prop::sample::Index]) -> Pieces {
    let mut cuts: Vec<usize> = raw.iter().map(|i| i.index(bytes.len() + 1)).collect();
    cuts.sort_unstable();
    let mut out = std::collections::VecDeque::new();
    let mut at = 0;
    for cut in cuts.into_iter().chain([bytes.len()]) {
        // An empty read means end of input: skip empty pieces.
        if cut > at {
            out.push_back(bytes[at..cut].to_vec());
        }
        at = cut;
    }
    Pieces(out)
}

/// The lines `bytes` holds: split at `\n`, one `\r` at a line's end dropped (a last line without
/// `\n` too: a progress line ending in `\r`), a last line without `\n` kept.
fn expected(bytes: &[u8]) -> Vec<String> {
    let mut lines: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.into_iter().map(|l| String::from_utf8_lossy(l.strip_suffix(b"\r").unwrap_or(l)).into_owned()).collect()
}

/// One line as `forward_lines` reads it: `read_until` into a buffer kept across calls (so a
/// cancelled read loses nothing), then `take_line`. None at the end.
async fn next_line_lossy<R: tokio::io::AsyncBufRead + Unpin>(reader: &mut R, buf: &mut Vec<u8>) -> std::io::Result<Option<String>> {
    use tokio::io::AsyncBufReadExt;
    if reader.read_until(b'\n', buf).await? == 0 && buf.is_empty() {
        return Ok(None);
    }
    Ok(Some(super::take_line(buf)))
}

fn read_all(reader: Pieces, capacity: usize) -> Vec<String> {
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    rt.block_on(async {
        let mut reader = BufReader::with_capacity(capacity, reader);
        let mut buf = Vec::new();
        let mut lines = Vec::new();
        while let Some(line) = next_line_lossy(&mut reader, &mut buf).await.unwrap() {
            lines.push(line);
        }
        lines
    })
}

/// Output bytes biased to line ends, carriage returns and broken UTF-8.
fn output() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(prop_oneof![4 => any::<u8>(), 2 => Just(b'\n'), 1 => Just(b'\r'), 1 => 0x80u8..0xC0, 1 => 0xC0u8..0xF8], 0..200)
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn any_output_reads_back_as_its_lines(bytes in output(), raw in proptest::collection::vec(any::<prop::sample::Index>(), 0..10), capacity in 1usize..64) {
        prop_assert_eq!(read_all(pieces(&bytes, &raw), capacity), expected(&bytes));
    }

    #[test]
    fn text_lines_come_back_exactly(lines in proptest::collection::vec("[^\r\n]{0,30}", 0..8), crlf in any::<bool>(), raw in proptest::collection::vec(any::<prop::sample::Index>(), 0..10)) {
        let end = if crlf { "\r\n" } else { "\n" };
        let text: String = lines.iter().map(|l| format!("{l}{end}")).collect();
        prop_assert_eq!(read_all(pieces(text.as_bytes(), &raw), 8), lines);
    }
}

/// `forward_lines` races stdout against stderr with `select!`, which drops the losing read.
/// `read_until` has by then moved the partial line's bytes into `buf` (and consumed them from the
/// reader): the buffer is kept across calls, so the start of a line that arrived in pieces stays.
#[tokio::test]
async fn a_cancelled_read_keeps_the_start_of_its_line() {
    use tokio::io::AsyncWriteExt;
    let (mut tool, pipe) = tokio::io::duplex(64);
    let mut reader = BufReader::new(pipe);
    let mut buf = Vec::new();
    tool.write_all(b"Downloading box ").await.unwrap();
    // The other stream's line wins the race: this read is dropped mid-line.
    let raced = tokio::time::timeout(std::time::Duration::from_millis(20), next_line_lossy(&mut reader, &mut buf)).await;
    assert!(raced.is_err());
    tool.write_all(b"50%\n").await.unwrap();
    assert_eq!(next_line_lossy(&mut reader, &mut buf).await.unwrap().as_deref(), Some("Downloading box 50%"));
}

/// The same through `forward_lines` and a real tool: a progress line printed in two writes, with
/// a stderr line between them.
#[cfg(unix)]
#[tokio::test]
async fn forward_lines_keeps_a_line_split_by_the_other_stream_whole() {
    let mut child = tokio::process::Command::new("sh")
        .args(["-c", "printf 'Downloading box '; sleep 0.3; echo warning >&2; sleep 0.3; echo '50%'"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = Vec::new();
    let (status, _) = forward_lines(&mut child, 5, |l| lines.push(l), |_| {}).await.unwrap();
    assert!(status.success());
    assert_eq!(lines, ["warning", "Downloading box 50%"]);
}
