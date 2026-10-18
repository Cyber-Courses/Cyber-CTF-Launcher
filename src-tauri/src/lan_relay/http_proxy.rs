//! A loopback HTTP forward proxy (plain HTTP only), for Vagrant's WinRM to Windows guests: the
//! WinRM client (`HTTPClient`) follows `http_proxy` and sends absolute-URI requests. Each client
//! connection gets one connection to its target, which WinRM's Negotiate auth needs.
//!
//! Any local process can reach a loopback port, and through it the LAN that macOS keeps third
//! party tools off. So the proxy only forwards what it is for: WinRM's plain-HTTP port on the
//! ESXi host or on an address of the local network, where the host's guests are (`Targets`).
//! Anything else (the internet, this machine's own services, another port) gets a 502.

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use super::Relay;

const BAD_GATEWAY: &[u8] = b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
/// A request head longer than this is refused.
const MAX_HEAD: usize = 64 * 1024;
/// A request or header line longer than this is refused (WinRM's are well under 1 KB).
const MAX_LINE: usize = 8 * 1024;
/// WinRM over plain HTTP, the only traffic the proxy carries.
const WINRM_PORT: u16 = 5985;

/// Where the proxy may forward: WinRM on the ESXi host it was started for, or on an address of
/// the local network (the host's guests get theirs from it, so they aren't known in advance).
#[derive(Clone, Debug)]
pub struct Targets {
    esxi_host: String,
    port: u16,
}

impl Targets {
    pub fn new(esxi_host: &str) -> Targets {
        Targets { esxi_host: esxi_host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase(), port: WINRM_PORT }
    }

    /// Only `host:port`, for a test's own origin server on loopback.
    #[cfg(test)]
    fn only(host: &str, port: u16) -> Targets {
        Targets { port, ..Targets::new(host) }
    }

    /// Whether `target` (`host:port`, as [`split_target`] gives it) may be reached.
    pub fn allows(&self, target: &str) -> bool {
        let Some((host, port)) = target.rsplit_once(':') else { return false };
        if port.parse::<u16>() != Ok(self.port) {
            return false;
        }
        let host = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
        host == self.esxi_host || host.parse::<std::net::IpAddr>().is_ok_and(on_local_network)
    }
}

/// A private-network address (RFC 1918, IPv4 link-local, IPv6 unique-local or link-local): not
/// loopback, not the internet.
fn on_local_network(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
        std::net::IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

pub async fn start_http_proxy(targets: Targets) -> std::io::Result<Relay> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let local = listener.local_addr()?.port();
    let task = super::serve(listener, move |inbound| {
        let targets = targets.clone();
        async move {
            let _ = proxy_connection(inbound, &targets).await;
        }
    });
    Ok(Relay { port: local, task })
}

/// The request head's target as (host:port, origin-form path), from an absolute `http://` URI.
pub(super) fn split_target(uri: &str) -> Option<(String, String)> {
    let rest = uri.strip_prefix("http://")?;
    // The authority ends at the path, or at a query or fragment following it directly
    // (`http://host:5985?x=1` is a valid absolute-URI): the query stays on the request.
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(i) if rest.as_bytes()[i] == b'/' => (&rest[..i], rest[i..].to_string()),
        Some(i) => (&rest[..i], format!("/{}", &rest[i..])),
        None => (rest, "/".to_string()),
    };
    if authority.is_empty() {
        return None;
    }
    let authority = if authority.rsplit_once(':').is_some_and(|(_, p)| p.parse::<u16>().is_ok()) && !authority.ends_with(']') {
        authority.to_string()
    } else {
        format!("{authority}:80")
    };
    Some((authority, path))
}

/// Rewrites one request head for the origin server: origin-form target, no proxy headers.
/// Returns the target and the body length.
pub(super) fn rewrite_head(head: &str) -> Option<(String, String, usize)> {
    // Lines end at LF, a CR before it optional: as `read_head` reads them. Split on CRLF only, a
    // header after a bare LF rode along hidden inside the line before it.
    let mut lines = head.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l));
    let mut first = lines.next()?.splitn(3, ' ');
    let (method, uri, version) = (first.next()?, first.next()?, first.next()?);
    let (target, path) = split_target(uri)?;
    let mut out = format!("{method} {path} {version}\r\n");
    let mut len = 0;
    for line in lines.filter(|l| !l.is_empty()) {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("proxy-connection:") || lower.starts_with("proxy-authorization:") {
            continue;
        }
        // A chunked body has no length to relay by: refused (a 502), not read as the next head.
        if lower.starts_with("transfer-encoding:") {
            return None;
        }
        if let Some(v) = lower.strip_prefix("content-length:") {
            // Only spaces and tabs are optional whitespace in HTTP (`str::trim` also takes \v, \f...).
            len = v.trim_matches([' ', '\t']).parse().ok()?;
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    Some((target, out, len))
}

/// What the client sent next.
#[derive(Debug, PartialEq)]
enum Head {
    /// A request head, up to its blank line.
    Request(String),
    /// A line or a head over its limit: refused, not read on without end.
    TooLong,
    /// The client closed the connection.
    Closed,
}

/// Reads one request head (up to its blank line), skipping stray CRLFs between requests. No
/// line is read past [`MAX_LINE`], no head past [`MAX_HEAD`].
async fn read_head<R: tokio::io::AsyncBufRead + Unpin>(reader: &mut R) -> std::io::Result<Head> {
    let mut head = String::new();
    loop {
        let mut line = String::new();
        let n = (&mut *reader).take(MAX_LINE as u64 + 1).read_line(&mut line).await?;
        if n == 0 {
            return Ok(if head.is_empty() { Head::Closed } else { Head::TooLong });
        }
        if line.len() > MAX_LINE || !line.ends_with('\n') {
            return Ok(Head::TooLong);
        }
        let blank = line == "\r\n" || line == "\n";
        if blank && head.is_empty() {
            continue; // stray CRLF between requests
        }
        head.push_str(&line);
        if head.len() > MAX_HEAD {
            return Ok(Head::TooLong);
        }
        // A head ends at a blank line, a CR before its LF optional (as `rewrite_head` splits it).
        if blank {
            return Ok(Head::Request(head));
        }
    }
}

/// The client's side of a proxied connection: its write half until its first request reaches
/// an origin, then that origin's connection, whose pump writes the responses back to it.
enum Peer {
    Waiting(OwnedWriteHalf),
    Connected(Upstream),
}

/// The one origin a client connection is proxied to.
struct Upstream {
    /// `host:port`.
    target: String,
    to_origin: OwnedWriteHalf,
    /// Streams the origin's responses back to the client, in order (it pairs them with its
    /// requests).
    pump: JoinHandle<()>,
}

impl Upstream {
    fn pump(target: String, origin: TcpStream, mut client: OwnedWriteHalf) -> Upstream {
        let (mut from_origin, to_origin) = origin.into_split();
        let pump = tokio::spawn(async move {
            let _ = tokio::io::copy(&mut from_origin, &mut client).await;
            let _ = client.shutdown().await;
        });
        Upstream { target, to_origin, pump }
    }
}

impl Peer {
    /// Whether a request to `target` may go on this connection: one origin per connection.
    fn may_send_to(&self, target: &str) -> bool {
        match self {
            Peer::Waiting(_) => true,
            Peer::Connected(upstream) => upstream.target == target,
        }
    }

    /// Answers 502 and closes, while the client's write half is still ours.
    async fn refuse(self) -> std::io::Result<()> {
        if let Peer::Waiting(mut client) = self {
            client.write_all(BAD_GATEWAY).await?;
        }
        Ok(())
    }

    /// The client is done: lets the last response finish.
    async fn finish(self) {
        if let Peer::Connected(mut upstream) = self {
            let _ = upstream.to_origin.shutdown().await;
            let _ = upstream.pump.await;
        }
    }

    /// The connection to `target`, connecting on the first request; the client is refused
    /// when the origin can't be reached.
    async fn connected(self, target: String) -> std::result::Result<Upstream, std::io::Result<()>> {
        match self {
            Peer::Connected(upstream) => Ok(upstream),
            Peer::Waiting(client) => match TcpStream::connect(target.as_str()).await {
                Ok(origin) => Ok(Upstream::pump(target, origin, client)),
                Err(_) => Err(Peer::Waiting(client).refuse().await),
            },
        }
    }
}

async fn proxy_connection(inbound: TcpStream, targets: &Targets) -> std::io::Result<()> {
    let (read_half, write_half) = inbound.into_split();
    let mut reader = BufReader::new(read_half);
    let mut peer = Peer::Waiting(write_half);
    loop {
        let head = match read_head(&mut reader).await? {
            Head::Request(head) => head,
            Head::TooLong => return peer.refuse().await,
            Head::Closed => {
                peer.finish().await;
                return Ok(());
            }
        };
        let parsed = rewrite_head(head.trim_end());
        let Some((target, rewritten, body_len)) = parsed.filter(|(t, ..)| targets.allows(t) && peer.may_send_to(t)) else {
            // Malformed, a target it isn't for, or a second origin on one connection: refuse
            // and close.
            return peer.refuse().await;
        };
        let mut upstream = match peer.connected(target).await {
            Ok(upstream) => upstream,
            Err(refused) => return refused,
        };
        upstream.to_origin.write_all(rewritten.as_bytes()).await?;
        let mut body = (&mut reader).take(body_len as u64);
        tokio::io::copy(&mut body, &mut upstream.to_origin).await?;
        peer = Peer::Connected(upstream);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_length_takes_only_spaces_and_tabs_around_it() {
        assert_eq!(rewrite_head("POST http://h/x HTTP/1.1\r\nContent-Length: \t12 ").unwrap().2, 12);
        for v in ["\u{b}0", "\u{c}5", "1 2"] {
            assert!(rewrite_head(&format!("POST http://h/x HTTP/1.1\r\nContent-Length: {v}")).is_none(), "{v:?}");
        }
    }

    #[test]
    fn proxy_heads_become_origin_form() {
        let (t, head, len) =
            rewrite_head("POST http://10.0.0.7:5985/wsman HTTP/1.1\r\nHost: 10.0.0.7:5985\r\nProxy-Connection: Keep-Alive\r\nContent-Length: 12").unwrap();
        assert_eq!(t, "10.0.0.7:5985");
        assert_eq!(head, "POST /wsman HTTP/1.1\r\nHost: 10.0.0.7:5985\r\nContent-Length: 12\r\n\r\n");
        assert_eq!(len, 12);
        assert_eq!(split_target("http://host").unwrap(), ("host:80".into(), "/".into()));
        assert!(rewrite_head("GET /relative HTTP/1.1").is_none());
    }

    #[test]
    fn content_length_takes_the_last_header_and_refuses_signs_or_blanks_and_proxy_headers_go_whatever_their_case() {
        let head = |extra: &str| format!("POST http://h/x HTTP/1.1\r\nHost: h\r\n{extra}");
        assert_eq!(rewrite_head(&head("Content-Length: 5\r\nContent-Length: 7")).unwrap().2, 7);
        for bad in ["Content-Length: -1", "Content-Length:", "Content-Length: 1e3", "Content-Length: 0x10"] {
            assert!(rewrite_head(&head(bad)).is_none(), "{bad:?}");
        }
        let (_, out, len) = rewrite_head(&head("PROXY-AUTHORIZATION: Basic x\r\nproxy-connection: close\r\nX-Odd no colon")).unwrap();
        assert_eq!(len, 0);
        assert_eq!(out, "POST /x HTTP/1.1\r\nHost: h\r\nX-Odd no colon\r\n\r\n");
        // A request line with a missing part is malformed.
        assert!(rewrite_head("GET http://h/x").is_none());
        assert!(rewrite_head("").is_none());
    }

    #[test]
    fn a_target_without_a_path_keeps_its_query_on_the_request() {
        // `http://host:5985?x=1` is a valid absolute-URI (RFC 3986 lets the query follow the
        // authority directly). It connects to `host:5985?x=1:80` (a DNS failure, so a 502) and
        // would send `/`. Fix: end the authority at the first of `/`, `?` or `#`, and keep the
        // query on the origin-form path (`/?x=1`).
        assert_eq!(split_target("http://10.0.0.7:5985?x=1").unwrap(), ("10.0.0.7:5985".to_string(), "/?x=1".to_string()));
    }

    #[test]
    fn split_target_defaults_the_port_and_keeps_ipv6() {
        assert_eq!(split_target("http://[fd00::7]/wsman").unwrap(), ("[fd00::7]:80".into(), "/wsman".into()));
        assert_eq!(split_target("http://[fd00::7]:5985/x").unwrap(), ("[fd00::7]:5985".into(), "/x".into()));
        assert!(split_target("https://host/").is_none());
        assert!(split_target("http:///path").is_none());
    }

    /// The proxy of an ESXi host at 10.0.0.5.
    fn esxi() -> Targets {
        Targets::new("10.0.0.5")
    }

    /// What the proxy answers `request` with, until it closes.
    async fn answer(targets: Targets, request: &[u8]) -> Vec<u8> {
        let proxy = start_http_proxy(targets).await.unwrap();
        let mut c = TcpStream::connect(("127.0.0.1", proxy.port)).await.unwrap();
        // The proxy may refuse and close before it has read all of it.
        let _ = c.write_all(request).await;
        let mut got = Vec::new();
        let _ = c.read_to_end(&mut got).await;
        got
    }

    #[tokio::test]
    async fn malformed_requests_get_a_502() {
        assert_eq!(answer(esxi(), b"GET /relative HTTP/1.1\r\nHost: x\r\n\r\n").await, BAD_GATEWAY);
    }

    #[test]
    fn only_winrm_on_the_esxi_host_or_the_local_network_is_a_target() {
        let t = esxi();
        for ok in ["10.0.0.5:5985", "10.0.0.77:5985", "192.168.1.40:5985", "172.20.3.4:5985", "169.254.10.2:5985", "[fd00::7]:5985", "[fe80::1]:5985"] {
            assert!(t.allows(ok), "{ok}");
        }
        for refused in [
            "10.0.0.5:22",
            "10.0.0.77:80",
            "127.0.0.1:5985",
            "localhost:5985",
            "[::1]:5985",
            "93.184.216.34:5985",
            "[2001:4860::8888]:5985",
            "evil.example.com:5985",
            "0.0.0.0:5985",
            "10.0.0.77",
            "10.0.0.77:x",
        ] {
            assert!(!t.allows(refused), "{refused}");
        }
        // A host by name: that name only, any case.
        let named = Targets::new("ESXi.lan");
        assert!(named.allows("esxi.lan:5985") && !named.allows("other.lan:5985") && !named.allows("esxi.lan:443"));
        assert!(Targets::new("[fd00::5]").allows("[fd00::5]:5985"));
    }

    #[tokio::test]
    async fn a_target_it_is_not_for_gets_a_502_without_a_connection() {
        // A listener of this machine's: the proxy must not reach it for another process.
        let local = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = local.local_addr().unwrap().port();
        let req = format!("GET http://127.0.0.1:{port}/ HTTP/1.1\r\nHost: x\r\n\r\n");
        assert_eq!(answer(esxi(), req.as_bytes()).await, BAD_GATEWAY);
        let probe = tokio::time::timeout(std::time::Duration::from_millis(100), local.accept()).await;
        assert!(probe.is_err(), "the proxy connected to a target it isn't for");
        assert_eq!(answer(esxi(), b"GET http://93.184.216.34:5985/wsman HTTP/1.1\r\n\r\n").await, BAD_GATEWAY);
    }

    #[tokio::test]
    async fn an_endless_line_or_head_gets_a_502() {
        let long_line = format!("GET http://10.0.0.5:5985/{} HTTP/1.1\r\n\r\n", "a".repeat(MAX_LINE));
        assert_eq!(answer(esxi(), long_line.as_bytes()).await, BAD_GATEWAY);
        let many =
            format!("GET http://10.0.0.5:5985/ HTTP/1.1\r\n{}\r\n", "X-Pad: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n".repeat(MAX_HEAD / 50 + 1));
        assert_eq!(answer(esxi(), many.as_bytes()).await, BAD_GATEWAY);
    }

    #[tokio::test]
    async fn heads_are_read_line_by_line_within_their_limits() {
        let mut r = BufReader::new(&b"\r\nGET http://h/ HTTP/1.1\r\nHost: h\r\n\r\n"[..]);
        assert_eq!(read_head(&mut r).await.unwrap(), Head::Request("GET http://h/ HTTP/1.1\r\nHost: h\r\n\r\n".into()));
        assert_eq!(read_head(&mut r).await.unwrap(), Head::Closed);
        // Cut off mid-head, or a line with no end within the limit.
        assert_eq!(read_head(&mut BufReader::new(&b"GET http://h/ HTTP/1.1\r\nHo"[..])).await.unwrap(), Head::TooLong);
        let endless = vec![b'a'; MAX_LINE + 10];
        assert_eq!(read_head(&mut BufReader::new(&endless[..])).await.unwrap(), Head::TooLong);
    }

    #[tokio::test]
    async fn proxy_keeps_one_origin_connection_for_several_requests() {
        let origin = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = origin.local_addr().unwrap().port();
        // Answers each request on the same connection with the body it got (echo).
        tokio::spawn(async move {
            let (s, _) = origin.accept().await.unwrap();
            let mut r = BufReader::new(s);
            for _ in 0..2 {
                let mut len = 0;
                loop {
                    let mut l = String::new();
                    r.read_line(&mut l).await.unwrap();
                    if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if l == "\r\n" {
                        break;
                    }
                }
                let mut body = vec![0; len];
                r.read_exact(&mut body).await.unwrap();
                let reply = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
                r.get_mut().write_all(reply.as_bytes()).await.unwrap();
                r.get_mut().write_all(&body).await.unwrap();
            }
        });
        let proxy = start_http_proxy(Targets::only("127.0.0.1", port)).await.unwrap();
        let mut c = TcpStream::connect(("127.0.0.1", proxy.port)).await.unwrap();
        for msg in ["first", "second!"] {
            let req = format!("POST http://127.0.0.1:{port}/wsman HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{msg}", msg.len());
            c.write_all(req.as_bytes()).await.unwrap();
            let expect = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{msg}", msg.len());
            let mut got = vec![0; expect.len()];
            c.read_exact(&mut got).await.unwrap();
            assert_eq!(String::from_utf8(got).unwrap(), expect);
        }
    }
}

#[cfg(test)]
mod proptests;
