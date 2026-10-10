//! Property tests for the proxy's request-head rewrite: any head is handled without a panic, a
//! rewritten head never carries proxy headers or more lines than it came with, and the target
//! is always a `host:port` to connect to.

use proptest::prelude::*;

use super::*;
use crate::proptest_support::config;

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
        let last_len = headers.iter().rev().find_map(|h| h.strip_prefix("Content-Length: ")).map(|v| v.parse::<usize>().unwrap()).unwrap_or(0);
        prop_assert_eq!(len, last_len);
    }

    #[test]
    fn only_absolute_http_targets_are_proxied(scheme in "(https|ftp|ws|HTTP)?", rest in "[!-~]{0,30}") {
        let uri = if scheme.is_empty() { format!("/{rest}") } else { format!("{scheme}://{rest}") };
        let head = format!("GET {uri} HTTP/1.1\r\nHost: x");
        prop_assert!(rewrite_head(&head).is_none());
    }

    #[test]
    fn a_bad_content_length_refuses_the_request(v in "[^0-9+\r\n ][^\r\n]{0,10}") {
        let head = format!("POST http://h/x HTTP/1.1\r\nContent-Length: {v}");
        prop_assert!(rewrite_head(&head).is_none());
    }
}
