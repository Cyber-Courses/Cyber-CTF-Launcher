//! The two pieces of HTTP the app's loopback servers (the co-location answer and the screen's
//! `WebSocket` bridge) read of a raw request, in one place: a header's value, and the token check
//! that leaks nothing through its timing.

/// The value of request `req`'s header `name` (case-insensitive), trimmed. Only header lines are
/// read: the request line's `:` (a port in an absolute URL, a path) never reads as one.
pub fn header<'a>(req: &'a str, name: &str) -> Option<&'a str> {
    req.lines().skip(1).take_while(|l| !l.trim().is_empty()).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// Constant-time string equality, so a token check doesn't leak through response timing.
pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_is_read_from_the_header_lines_only() {
        // As a browser sends a WebSocket upgrade.
        let req = "GET http://host:1/x HTTP/1.1\r\nHost: 127.0.0.1:4100\r\nOrigin: tauri://localhost\r\nUpgrade: websocket\r\n\r\nbody: not a header";
        assert_eq!(header(req, "host"), Some("127.0.0.1:4100"));
        assert_eq!(header(req, "ORIGIN"), Some("tauri://localhost"));
        assert_eq!(header(req, "get http"), None, "the request line is no header");
        assert_eq!(header(req, "body"), None, "the body is no header");
        assert_eq!(header("", "host"), None);
    }

    #[test]
    fn tokens_compare_whole() {
        assert!(ct_eq("abc", "abc") && !ct_eq("abc", "abd") && !ct_eq("abc", "ab") && ct_eq("", ""));
    }
}
