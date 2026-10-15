//! Property tests for the proxy's request-head rewrite: any head is handled without a panic, a
//! rewritten head never carries proxy headers or more lines than it came with, and the target
//! is always a `host:port` to connect to.

use proptest::prelude::*;

use super::*;
use crate::testing::proptest_support::config;

fn host() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-z0-9]([a-z0-9.-]{0,20}[a-z0-9])?",
        any::<[u8; 4]>().prop_map(|o| std::net::Ipv4Addr::from(o).to_string()),
        any::<[u16; 8]>().prop_map(|s| format!("[{}]", std::net::Ipv6Addr::from(s))),
    ]
}

fn header() -> impl Strategy<Value = String> {
    prop_oneof![
        "(Host|Content-Type|Authorization|WSMANIDENTIFY|X-[A-Za-z]{1,8}): [ -~]{0,30}",
        "(Proxy-Connection|proxy-connection|PROXY-AUTHORIZATION|Proxy-Authorization): [ -~]{0,20}",
        "Content-Length: [0-9]{1,6}",
    ]
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn rewrite_never_panics(head in "(?s).{0,300}") {
        let _ = rewrite_head(&head);
        let _ = split_target(&head);
        let _ = rewrite_head(&format!("GET http://{head}"));
    }

    #[test]
    fn a_rewritten_head_is_origin_form_without_proxy_headers(
        method in "(GET|POST|PUT|OPTIONS)",
        host in host(),
        port in proptest::option::of(any::<u16>()),
        path in proptest::option::of("/[!-~]{0,30}"),
        headers in proptest::collection::vec(header(), 0..6),
    ) {
        let authority = match port { Some(p) => format!("{host}:{p}"), None => host.clone() };
        let uri = format!("http://{authority}{}", path.clone().unwrap_or_default());
        let head = std::iter::once(format!("{method} {uri} HTTP/1.1")).chain(headers.iter().cloned()).collect::<Vec<_>>().join("\r\n");
        let (target, out, len) = rewrite_head(&head).expect("a well-formed proxy request");

        prop_assert_eq!(target, format!("{host}:{}", port.unwrap_or(80)));
        prop_assert!(out.ends_with("\r\n\r\n"));
        let lines: Vec<&str> = out.trim_end_matches("\r\n").split("\r\n").collect();
        prop_assert_eq!(lines[0], format!("{method} {} HTTP/1.1", path.unwrap_or_else(|| "/".into())));
        let kept: Vec<&String> = headers.iter().filter(|h| !h.to_ascii_lowercase().starts_with("proxy-")).collect();
        prop_assert_eq!(lines.len() - 1, kept.len(), "no header added or lost");
        for (got, want) in lines[1..].iter().zip(kept) {
            prop_assert_eq!(*got, want.as_str());
        }
        let last_len = headers.iter().rev().find_map(|h| h.strip_prefix("Content-Length: ")).map_or(0, |v| v.parse::<usize>().unwrap());
        prop_assert_eq!(len, last_len);
    }

    #[test]
    fn only_absolute_http_targets_are_proxied(scheme in "(https|ftp|ws|HTTP)?", rest in "[!-~]{0,30}") {
        let uri = if scheme.is_empty() { format!("/{rest}") } else { format!("{scheme}://{rest}") };
        let head = format!("GET {uri} HTTP/1.1\r\nHost: x");
        prop_assert!(rewrite_head(&head).is_none());
    }

    #[test]
    fn a_bad_content_length_refuses_the_request(v in "[^0-9+\r\n \t][^\r\n]{0,10}") {
        let head = format!("POST http://h/x HTTP/1.1\r\nContent-Length: {v}");
        prop_assert!(rewrite_head(&head).is_none());
    }
}

proptest! {
    #![proptest_config(config())]

    /// Whatever the URI, a target is a `host:port` the connect can take, and the path is origin-form.
    #[test]
    fn a_split_target_is_host_port_and_an_origin_form_path(rest in "(?s).{0,60}") {
        if let Some((target, path)) = split_target(&format!("http://{rest}")) {
            let (host, port) = target.rsplit_once(':').expect("a port");
            prop_assert!(!host.is_empty());
            prop_assert!(port.parse::<u16>().is_ok(), "{}", target);
            prop_assert!(path.starts_with('/'), "{}", path);
            // Nothing is lost or added between the two halves (bar the `/` an empty path gets
            // and the default port).
            let authority = target.strip_suffix(":80").filter(|_| !rest.starts_with(&target)).unwrap_or(&target);
            prop_assert!(rest.starts_with(authority), "{} from {}", authority, rest);
        }
    }

    /// The origin never sees a proxy header or a Content-Length other than the body length the
    /// proxy forwards. Lines end in CRLF here: with a bare LF between them (which the origin
    /// also splits on, RFC 9112 §2.2) it fails, see `bug_a_header_after_a_bare_lf_is_hidden_from_the_rewrite`.
    #[test]
    fn the_origin_reads_the_same_headers_and_body_length_as_the_proxy(
        headers in proptest::collection::vec((header(), Just("\r\n")), 1..6),
    ) {
        let mut head = String::from("POST http://h:5985/wsman HTTP/1.1");
        for (h, sep) in &headers {
            head.push_str(sep);
            head.push_str(h);
        }
        let Some((_, out, len)) = rewrite_head(&head) else { return Ok(()) };
        let origin_lines: Vec<&str> = out.trim_end().split('\n').map(|l| l.trim_end_matches('\r')).skip(1).collect();
        for l in &origin_lines {
            prop_assert!(!l.to_ascii_lowercase().starts_with("proxy-"), "forwarded {:?}", l);
        }
        let origin_len = origin_lines.iter().rev().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
        prop_assert_eq!(origin_len, len);
    }

    /// Content-Length round-trips for any value that fits, with the optional whitespace around it.
    #[test]
    fn any_content_length_that_fits_is_read(n in any::<usize>(), name in "(Content-Length|content-length|CONTENT-LENGTH)", ows in "[ \t]{0,3}") {
        let head = format!("PUT http://h/x HTTP/1.1\r\n{name}:{ows}{n}{ows}");
        prop_assert_eq!(rewrite_head(&head).map(|(.., len)| len), Some(n));
        // One past the largest is refused, not wrapped.
        let head = format!("PUT http://h/x HTTP/1.1\r\n{name}: {}0", usize::MAX);
        prop_assert!(rewrite_head(&head).is_none());
    }
}

/// Found by `the_origin_reads_the_same_headers_and_body_length_as_the_proxy` with a bare LF
/// separator; shrunk: `POST http://h:5985/wsman HTTP/1.1\nProxy-Connection: ` (the header rides
/// along in the request line's version). `read_head` ends lines at `\n`, `rewrite_head` splits
/// only on `\r\n`.
#[test]
fn a_header_after_a_bare_lf_is_seen_by_the_rewrite() {
    let (_, out, len) = rewrite_head("POST http://h/x HTTP/1.1\r\nHost: h\nContent-Length: 5\r\nX: y\nProxy-Authorization: Basic eA==").unwrap();
    // The origin reads `Content-Length: 5` and waits for 5 bytes the proxy never forwards (it
    // read the body length as 0), and receives the proxy credentials.
    assert_eq!(len, 5, "{out:?}");
    assert!(!out.to_ascii_lowercase().contains("proxy-authorization"), "{out:?}");
}

/// Found while writing the properties: only Content-Length frames a body.
#[test]
fn a_chunked_request_body_is_refused_not_read_as_the_next_head() {
    let parsed = rewrite_head("POST http://h/wsman HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked");
    // Either refused up front (a 502) or framed: never forwarded as a request without a body.
    assert!(parsed.is_none(), "{parsed:?}");
}
