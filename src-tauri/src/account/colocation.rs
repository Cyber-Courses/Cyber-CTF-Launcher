//! A tiny loopback control server for the co-location probe (see
//! doc/architecture/LAB-LAUNCHER-CHANNEL.md). When a lab is launched, the website must
//! decide whether this launcher is on the same machine as the browser before it trusts a
//! `127.0.0.1` lab URL. It does that by fetching `GET /health?token=<one-time token>` here
//! and checking the returned nonce. We bind loopback-only, require the token, and echo an
//! allowed Origin so only the site can read the response.

use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::error::{Error, Result};

/// Site origins allowed to read the probe. Loopback origins are allowed too, in dev builds only
/// (the site's dev server): in a release any local web page could otherwise read it.
const ALLOWED_ORIGINS: [&str; 4] = ["https://www.cybercourses.com", "https://cybercourses.com", "https://www.cyberctf.org", "https://cyberctf.org"];

/// A loopback dev origin, by exact host match (not a prefix): `http(s)://localhost` or
/// `127.0.0.1`, with an optional numeric port. A prefix match would also accept
/// `http://localhost.evil.com`.
fn is_loopback_origin(origin: &str) -> bool {
    let Some(rest) = origin.strip_prefix("http://").or_else(|| origin.strip_prefix("https://")) else {
        return false;
    };
    // Reject anything with a path/extra; an Origin is scheme://host[:port] only.
    if rest.contains('/') {
        return false;
    }
    let (name, port) = rest.split_once(':').map_or((rest, None), |(n, p)| (n, Some(p)));
    let host_ok = name == "localhost" || name == "127.0.0.1";
    let port_ok = port.is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    host_ok && port_ok
}

fn origin_allowed(origin: &str) -> bool {
    origin_allowed_in(origin, cfg!(debug_assertions))
}

/// [`origin_allowed`] in a dev build (`dev`) or a release.
fn origin_allowed_in(origin: &str, dev: bool) -> bool {
    ALLOWED_ORIGINS.contains(&origin) || (dev && is_loopback_origin(origin))
}

/// How long a connection gets to send its request, and to take the answer: a client that
/// connects and stays silent must not hold a task (and a socket) for the life of the app.
const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(if cfg!(test) { 1 } else { 5 });

/// Constant-time string equality, so the token check doesn't leak via response timing.
fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn header<'a>(req: &'a str, name: &str) -> Option<&'a str> {
    req.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// Builds the HTTP response for a raw request. Pure, so it is unit-tested directly.
fn build_response(req: &str, token: &str, nonce: &str) -> String {
    let request_line = req.lines().next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let origin = header(req, "origin").unwrap_or("");

    let reply = |status: &str, body: &str| -> String {
        let cors = if origin_allowed(origin) && !origin.is_empty() { format!("Access-Control-Allow-Origin: {origin}\r\n") } else { String::new() };
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\n{cors}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    };

    if method == "OPTIONS" {
        return reply("204 No Content", "");
    }
    if method != "GET" {
        return reply("405 Method Not Allowed", "{\"error\":\"method\"}");
    }
    if path != "/health" {
        return reply("404 Not Found", "{\"error\":\"not_found\"}");
    }
    let given = query.split('&').find_map(|kv| kv.strip_prefix("token=")).unwrap_or("");
    if !ct_eq(given, token) {
        return reply("403 Forbidden", "{\"error\":\"forbidden\"}");
    }
    reply("200 OK", &format!("{{\"nonce\":\"{nonce}\"}}"))
}

/// Answers one connection's request, each way within [`IO_TIMEOUT`].
async fn answer(socket: &mut tokio::net::TcpStream, token: &str, nonce: &str) {
    let mut buf = [0u8; 2048];
    let Ok(Ok(n)) = tokio::time::timeout(IO_TIMEOUT, socket.read(&mut buf)).await else { return };
    let req = String::from_utf8_lossy(&buf[..n]);
    let resp = build_response(&req, token, nonce);
    let _ = tokio::time::timeout(IO_TIMEOUT, async {
        let _ = socket.write_all(resp.as_bytes()).await;
        let _ = socket.shutdown().await;
    })
    .await;
}

/// The co-location server of the current launch. A new launch retires the previous one, so
/// listeners (and their ports/tokens) don't accumulate for the life of the app.
static CURRENT: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>> = std::sync::Mutex::new(None);

/// Starts the control server on a free loopback port and returns the port. The previous
/// launch's server is aborted (its one-time token is spent by then), so only the latest is live.
pub async fn serve(token: String, nonce: String) -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(Error::Io)?;
    let port = listener.local_addr().map_err(Error::Io)?.port();
    let handle = tokio::spawn(async move {
        loop {
            let (mut socket, _addr): (_, SocketAddr) = match listener.accept().await {
                Ok(pair) => pair,
                // A persistent accept error (e.g. fd exhaustion) must not become a hot spin loop.
                Err(_) => {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
            };
            let (token, nonce) = (token.clone(), nonce.clone());
            tokio::spawn(async move { answer(&mut socket, &token, &nonce).await });
        }
    });
    if let Ok(mut cur) = CURRENT.lock()
        && let Some(old) = cur.replace(handle)
    {
        old.abort();
    }
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::build_response;

    fn get(path: &str, origin: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: {origin}\r\n\r\n")
    }

    #[test]
    fn returns_the_nonce_only_with_the_right_token_and_echoes_allowed_origin() {
        let ok = build_response(&get("/health?token=secret", "https://www.cybercourses.com"), "secret", "abc123");
        assert!(ok.starts_with("HTTP/1.1 200"));
        assert!(ok.contains("\"nonce\":\"abc123\""));
        assert!(ok.contains("Access-Control-Allow-Origin: https://www.cybercourses.com"));
    }

    #[test]
    fn rejects_a_wrong_token() {
        let bad = build_response(&get("/health?token=nope", "https://www.cybercourses.com"), "secret", "abc123");
        assert!(bad.starts_with("HTTP/1.1 403"));
        assert!(!bad.contains("abc123"));
    }

    #[test]
    fn does_not_echo_a_foreign_origin() {
        let r = build_response(&get("/health?token=secret", "https://evil.example.com"), "secret", "abc123");
        assert!(r.starts_with("HTTP/1.1 200"));
        assert!(!r.contains("Access-Control-Allow-Origin"));
    }

    #[test]
    fn unknown_path_is_404() {
        assert!(build_response(&get("/secrets", "https://cybercourses.com"), "secret", "n").starts_with("HTTP/1.1 404"));
    }

    #[test]
    fn loopback_origin_is_exact_not_a_prefix() {
        use super::is_loopback_origin;
        assert!(is_loopback_origin("http://localhost"));
        assert!(is_loopback_origin("http://localhost:3000"));
        assert!(is_loopback_origin("http://127.0.0.1:47290"));
        assert!(is_loopback_origin("https://localhost"));
        // Prefix-match escapes that must be rejected:
        assert!(!is_loopback_origin("http://localhost.evil.com"));
        assert!(!is_loopback_origin("http://127.0.0.1.evil.com"));
        assert!(!is_loopback_origin("http://localhostXYZ"));
        assert!(!is_loopback_origin("http://localhost/path"));
        assert!(!is_loopback_origin("http://localhost:not-a-port"));
    }

    #[test]
    fn loopback_origins_read_the_probe_in_dev_builds_only() {
        use super::origin_allowed_in;
        assert!(origin_allowed_in("http://localhost:3000", true));
        assert!(!origin_allowed_in("http://localhost:3000", false));
        assert!(!origin_allowed_in("http://127.0.0.1:47290", false));
        for site in super::ALLOWED_ORIGINS {
            assert!(origin_allowed_in(site, false) && origin_allowed_in(site, true));
        }
        assert!(!origin_allowed_in("https://evil.example.com", true));
        assert_eq!(super::origin_allowed("http://localhost:3000"), cfg!(debug_assertions));
    }

    #[tokio::test]
    async fn a_silent_connection_is_dropped_after_the_timeout() {
        use tokio::io::AsyncReadExt;
        // Its own listener: `serve` would retire the server another test is using.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut s = tokio::net::TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
        let (mut server_side, _) = listener.accept().await.unwrap();
        let served = tokio::spawn(async move { super::answer(&mut server_side, "tok", "n").await });
        let mut out = Vec::new();
        // Nothing sent: the server closes on its own, without an answer.
        let read = tokio::time::timeout(super::IO_TIMEOUT * 5, s.read_to_end(&mut out)).await;
        assert!(read.is_ok(), "still open after the timeout");
        assert!(out.is_empty());
        served.await.unwrap();
    }

    #[test]
    fn options_preflight_and_other_methods() {
        let pre = build_response("OPTIONS /health HTTP/1.1\r\nOrigin: https://cyberctf.org\r\n\r\n", "t", "n");
        assert!(pre.starts_with("HTTP/1.1 204") && pre.contains("Access-Control-Allow-Origin: https://cyberctf.org"));
        assert!(build_response("POST /health?token=t HTTP/1.1\r\n\r\n", "t", "n").starts_with("HTTP/1.1 405"));
        assert!(build_response("", "t", "n").starts_with("HTTP/1.1 405"));
        assert!(!super::ct_eq("abc", "abd") && !super::ct_eq("abc", "ab") && super::ct_eq("abc", "abc"));
        assert!(!super::is_loopback_origin("ftp://localhost") && !super::is_loopback_origin("http://localhost:"));
    }

    async fn health(port: u16, token: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(format!("GET /health?token={token} HTTP/1.1\r\n\r\n").as_bytes()).await.unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out).await;
        out
    }

    #[tokio::test]
    async fn serve_answers_on_loopback_and_retires_the_previous_server() {
        let port = super::serve("tok1".into(), "nonce1".into()).await.unwrap();
        let ok = health(port, "tok1").await;
        assert!(ok.starts_with("HTTP/1.1 200") && ok.contains("nonce1"), "{ok}");
        assert!(health(port, "bad").await.starts_with("HTTP/1.1 403"));
        let second = super::serve("tok2".into(), "nonce2".into()).await.unwrap();
        assert_ne!(port, second);
        assert!(health(second, "tok2").await.contains("nonce2"));
    }
}
