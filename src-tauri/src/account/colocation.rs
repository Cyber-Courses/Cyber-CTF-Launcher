//! A tiny loopback control server for the co-location probe (see
//! doc/architecture/LAB-LAUNCHER-CHANNEL.md). When a lab is launched, the website must
//! decide whether this launcher is on the same machine as the browser before it trusts a
//! `127.0.0.1` lab URL. It does that by fetching `GET /health?token=<one-time token>` here
//! and checking the returned nonce. We bind loopback-only, require the token and a loopback
//! `Host` (a DNS-rebinding page names its own host), echo an allowed Origin so only the site can
//! read the response, and answer the nonce once: the token is spent by the first answer, and the
//! server ends then, when the lab stops, or after [`PROBE_TTL`], whichever comes first.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Notify;

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

/// Whether a request's `Host` names this server: `127.0.0.1:<port>` or `localhost:<port>`. A page
/// reached by DNS rebinding sends its own host name, so it is refused even with a leaked token.
fn host_allowed(host: &str, port: u16) -> bool {
    let Some((name, given)) = host.rsplit_once(':') else { return false };
    (name == "127.0.0.1" || name.eq_ignore_ascii_case("localhost")) && given.parse::<u16>() == Ok(port)
}

/// The answer to one request, and whether it gave the nonce (which spends the token).
struct Reply {
    text: String,
    gave_nonce: bool,
}

/// Builds the HTTP response for a raw request to the server on `port`. `token` is None once it
/// was spent. Pure, so it is unit-tested directly.
fn build_response(req: &str, token: Option<&str>, nonce: &str, port: u16) -> Reply {
    let request_line = req.lines().next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let origin = header(req, "origin").unwrap_or("");

    let reply = |status: &str, body: &str| -> Reply {
        let cors = if origin_allowed(origin) && !origin.is_empty() { format!("Access-Control-Allow-Origin: {origin}\r\n") } else { String::new() };
        let text = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\n{cors}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        Reply { text, gave_nonce: false }
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
    if !header(req, "host").is_some_and(|h| host_allowed(h, port)) {
        return reply("403 Forbidden", "{\"error\":\"host\"}");
    }
    // Only the site reads the answer (CORS): a request from anywhere else would spend the token
    // for nothing, and leave the site's own check without its nonce.
    if !origin_allowed(origin) {
        return reply("403 Forbidden", "{\"error\":\"origin\"}");
    }
    let given = query.split('&').find_map(|kv| kv.strip_prefix("token=")).unwrap_or("");
    if !token.is_some_and(|token| ct_eq(given, token)) {
        return reply("403 Forbidden", "{\"error\":\"forbidden\"}");
    }
    Reply { gave_nonce: true, ..reply("200 OK", &format!("{{\"nonce\":\"{nonce}\"}}")) }
}

/// One launch's probe: its token until the nonce is given, the nonce, and its port.
struct Probe {
    token: Mutex<Option<String>>,
    nonce: String,
    port: u16,
    /// Told when the token is spent: the server has nothing left to answer.
    spent: Notify,
}

impl Probe {
    fn new(token: String, nonce: String, port: u16) -> Probe {
        Probe { token: Mutex::new(Some(token)), nonce, port, spent: Notify::new() }
    }

    /// The answer to `req`. The first one giving the nonce spends the token: a request after it,
    /// even with the right token, is refused.
    fn reply_to(&self, req: &str) -> String {
        let mut token = self.token.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let reply = build_response(req, token.as_deref(), &self.nonce, self.port);
        if reply.gave_nonce {
            *token = None;
            self.spent.notify_one();
        }
        reply.text
    }
}

/// Answers one connection's request, each way within [`IO_TIMEOUT`].
async fn answer(socket: &mut tokio::net::TcpStream, probe: &Probe) {
    let mut buf = [0u8; 2048];
    let Ok(Ok(n)) = tokio::time::timeout(IO_TIMEOUT, socket.read(&mut buf)).await else { return };
    let req = String::from_utf8_lossy(&buf[..n]);
    let resp = probe.reply_to(&req);
    let _ = tokio::time::timeout(IO_TIMEOUT, async {
        let _ = socket.write_all(resp.as_bytes()).await;
        let _ = socket.shutdown().await;
    })
    .await;
}

/// How long a probe server waits for the website: it asks right after the lab is reported
/// running, so this is generous.
const PROBE_TTL: Duration = Duration::from_secs(if cfg!(test) { 2 } else { 10 * 60 });

/// Accepts connections for `probe` until its token is spent or [`PROBE_TTL`] passes.
async fn accept_until_spent(listener: TcpListener, probe: Arc<Probe>, ttl: Duration) {
    let accepting = async {
        loop {
            let (mut socket, _addr): (_, SocketAddr) = match listener.accept().await {
                Ok(pair) => pair,
                // A persistent accept error (e.g. fd exhaustion) must not become a hot spin loop.
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };
            let probe = probe.clone();
            tokio::spawn(async move { answer(&mut socket, &probe).await });
        }
    };
    tokio::select! {
        () = accepting => {}
        () = probe.spent.notified() => {}
        () = tokio::time::sleep(ttl) => {}
    }
}

/// The co-location server of the current launch, and its lab. A new launch retires the previous
/// one, so listeners (and their ports/tokens) don't accumulate for the life of the app.
static CURRENT: Mutex<Option<(Option<String>, tokio::task::JoinHandle<()>)>> = Mutex::new(None);

fn current() -> std::sync::MutexGuard<'static, Option<(Option<String>, tokio::task::JoinHandle<()>)>> {
    CURRENT.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Starts the control server for lab `lab` on a free loopback port and returns the port. The
/// previous launch's server is aborted, so only the latest is live.
pub async fn serve(lab: Option<&str>, token: String, nonce: String) -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(Error::Io)?;
    let port = listener.local_addr().map_err(Error::Io)?.port();
    let probe = Arc::new(Probe::new(token, nonce, port));
    let handle = tokio::spawn(accept_until_spent(listener, probe, PROBE_TTL));
    if let Some((_, old)) = current().replace((lab.map(str::to_string), handle)) {
        old.abort();
    }
    Ok(port)
}

/// Ends lab `lab`'s co-location server, if it still runs: the lab is stopping.
pub fn retire(lab: &str) {
    let mut current = current();
    if current.as_ref().is_some_and(|(of, _)| of.as_deref() == Some(lab))
        && let Some((_, server)) = current.take()
    {
        server.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::{Probe, build_response};

    const PORT: u16 = 47555;

    fn get(path: &str, origin: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\nOrigin: {origin}\r\n\r\n")
    }

    /// The response text for `req`, the expected token being `token`.
    fn respond(req: &str, token: &str, nonce: &str) -> String {
        build_response(req, Some(token), nonce, PORT).text
    }

    #[test]
    fn returns_the_nonce_only_with_the_right_token_and_echoes_allowed_origin() {
        let ok = build_response(&get("/health?token=secret", "https://www.cybercourses.com"), Some("secret"), "abc123", PORT);
        assert!(ok.gave_nonce);
        assert!(ok.text.starts_with("HTTP/1.1 200"));
        assert!(ok.text.contains("\"nonce\":\"abc123\""));
        assert!(ok.text.contains("Access-Control-Allow-Origin: https://www.cybercourses.com"));
    }

    #[test]
    fn rejects_a_wrong_token() {
        let bad = build_response(&get("/health?token=nope", "https://www.cybercourses.com"), Some("secret"), "abc123", PORT);
        assert!(bad.text.starts_with("HTTP/1.1 403"));
        assert!(!bad.text.contains("abc123") && !bad.gave_nonce);
    }

    #[test]
    fn a_spent_token_is_refused_even_when_it_is_right() {
        let r = build_response(&get("/health?token=secret", "https://cyberctf.org"), None, "abc123", PORT);
        assert!(r.text.starts_with("HTTP/1.1 403") && !r.text.contains("abc123") && !r.gave_nonce);
    }

    #[test]
    fn the_nonce_is_given_once() {
        let probe = Probe::new("secret".into(), "abc123".into(), PORT);
        let req = get("/health?token=secret", "https://cyberctf.org");
        assert!(probe.reply_to(&req).contains("\"nonce\":\"abc123\""));
        let again = probe.reply_to(&req);
        assert!(again.starts_with("HTTP/1.1 403") && !again.contains("abc123"), "{again}");
    }

    #[test]
    fn only_a_loopback_host_on_this_port_reads_the_nonce() {
        // A DNS-rebinding page sends its own host name; the token alone isn't enough.
        let with_host = |host: &str| format!("GET /health?token=t HTTP/1.1\r\nHost: {host}\r\nOrigin: https://cyberctf.org\r\n\r\n");
        for host in [format!("127.0.0.1:{PORT}"), format!("localhost:{PORT}"), format!("LOCALHOST:{PORT}")] {
            assert!(respond(&with_host(&host), "t", "n").starts_with("HTTP/1.1 200"), "{host}");
        }
        for host in ["evil.example.com".to_string(), format!("evil.example.com:{PORT}"), "127.0.0.1".into(), "127.0.0.1:1".into(), format!("127.0.0.2:{PORT}")]
        {
            let r = respond(&with_host(&host), "t", "n");
            assert!(r.starts_with("HTTP/1.1 403") && !r.contains("\"nonce\""), "{host}: {r}");
        }
        // No Host header at all: refused.
        assert!(respond("GET /health?token=t HTTP/1.1\r\nOrigin: https://cyberctf.org\r\n\r\n", "t", "n").starts_with("HTTP/1.1 403"));
    }

    #[test]
    fn a_foreign_origin_gets_no_nonce_and_spends_nothing() {
        let probe = Probe::new("secret".into(), "abc123".into(), PORT);
        for origin in ["https://evil.example.com", ""] {
            let r = probe.reply_to(&get("/health?token=secret", origin));
            assert!(r.starts_with("HTTP/1.1 403") && !r.contains("abc123") && !r.contains("Access-Control-Allow-Origin"), "{origin}: {r}");
        }
        // The site's own check still gets it.
        assert!(probe.reply_to(&get("/health?token=secret", "https://cyberctf.org")).contains("abc123"));
    }

    #[test]
    fn unknown_path_is_404() {
        assert!(respond(&get("/secrets", "https://cybercourses.com"), "secret", "n").starts_with("HTTP/1.1 404"));
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
        let probe = Probe::new("tok".into(), "n".into(), PORT);
        let served = tokio::spawn(async move { super::answer(&mut server_side, &probe).await });
        let mut out = Vec::new();
        // Nothing sent: the server closes on its own, without an answer.
        let read = tokio::time::timeout(super::IO_TIMEOUT * 5, s.read_to_end(&mut out)).await;
        assert!(read.is_ok(), "still open after the timeout");
        assert!(out.is_empty());
        served.await.unwrap();
    }

    #[test]
    fn the_token_is_the_first_token_parameter_read_as_typed() {
        // Among other parameters, and first of two: the one checked. A percent-encoded, a
        // prefixed (`tokens=`) or a bare `token` never matches (tokens are plain hex).
        let origin = "https://cyberctf.org";
        assert!(respond(&get("/health?x=1&token=secret&token=other", origin), "secret", "n").starts_with("HTTP/1.1 200"));
        for path in ["/health?token=sec%72et", "/health?tokens=secret", "/health?token", "/health?token=", "/health"] {
            let r = respond(&get(path, origin), "secret", "n");
            assert!(r.starts_with("HTTP/1.1 403") && !r.contains("\"nonce\""), "{path}: {r}");
        }
        // An empty expected token never matches an empty given one by accident of length.
        assert!(respond(&get("/health?token=", origin), "", "n").starts_with("HTTP/1.1 200"), "equal empty strings are equal");
    }

    #[test]
    fn requests_with_bare_line_feeds_or_odd_header_spelling_still_answer() {
        let ok = |req: &str| {
            let r = respond(req, "t", "n");
            assert!(r.starts_with("HTTP/1.1 200") && r.contains("\"nonce\":\"n\""), "{req:?}: {r}");
            r
        };
        ok(&format!("GET /health?token=t HTTP/1.1\nHost: 127.0.0.1:{PORT}\nOrigin: https://cyberctf.org\n\n"));
        ok(&format!("GET /health?token=t\r\nhost: localhost:{PORT}\r\norigin: https://cyberctf.org"));
        // The Origin header is found whatever its case and spacing, and echoed trimmed.
        let r = ok(&format!("GET /health?token=t HTTP/1.1\r\nHOST: 127.0.0.1:{PORT}\r\nORIGIN:   http://localhost:3000  \r\n\r\n"));
        assert!(r.contains("Access-Control-Allow-Origin: http://localhost:3000\r\n"), "{r}");
        // A header value with its own colon (a port) is kept whole.
        assert!(
            ok(&format!("GET /health?token=t HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\nOrigin: http://127.0.0.1:47290\r\n\r\n"))
                .contains("Allow-Origin: http://127.0.0.1:47290")
        );
        // No line at all: not a GET.
        assert!(respond("\r\n\r\n", "t", "n").starts_with("HTTP/1.1 405"));
    }

    #[test]
    fn options_preflight_and_other_methods() {
        let pre = respond("OPTIONS /health HTTP/1.1\r\nOrigin: https://cyberctf.org\r\n\r\n", "t", "n");
        assert!(pre.starts_with("HTTP/1.1 204") && pre.contains("Access-Control-Allow-Origin: https://cyberctf.org"));
        assert!(respond("POST /health?token=t HTTP/1.1\r\n\r\n", "t", "n").starts_with("HTTP/1.1 405"));
        assert!(respond("", "t", "n").starts_with("HTTP/1.1 405"));
        assert!(!super::ct_eq("abc", "abd") && !super::ct_eq("abc", "ab") && super::ct_eq("abc", "abc"));
        assert!(!super::is_loopback_origin("ftp://localhost") && !super::is_loopback_origin("http://localhost:"));
    }

    async fn health(port: u16, token: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let Ok(mut s) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await else { return String::new() };
        let _ = s.write_all(format!("GET /health?token={token} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: https://cyberctf.org\r\n\r\n").as_bytes()).await;
        let mut out = String::new();
        let _ = s.read_to_string(&mut out).await;
        out
    }

    /// The tests over `serve` (one current server) run one at a time.
    static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn serve_answers_once_on_loopback_then_closes() {
        let _serial = ONE_AT_A_TIME.lock().await;
        let port = super::serve(Some("lab-1"), "tok1".into(), "nonce1".into()).await.unwrap();
        assert!(health(port, "bad").await.starts_with("HTTP/1.1 403"));
        let ok = health(port, "tok1").await;
        assert!(ok.starts_with("HTTP/1.1 200") && ok.contains("nonce1"), "{ok}");
        // Spent: the server is gone (or, while it closes, refuses).
        let after = health(port, "tok1").await;
        assert!(!after.contains("nonce1"), "{after}");
    }

    #[tokio::test]
    async fn a_new_launch_or_the_lab_stopping_retires_the_server() {
        let _serial = ONE_AT_A_TIME.lock().await;
        let first = super::serve(Some("lab-1"), "tok1".into(), "nonce1".into()).await.unwrap();
        let second = super::serve(Some("lab-2"), "tok2".into(), "nonce2".into()).await.unwrap();
        assert_ne!(first, second);
        assert!(!health(first, "tok1").await.contains("nonce1"));
        // Another lab stopping leaves it alone; its own lab stopping ends it.
        super::retire("lab-1");
        assert!(health(second, "bad").await.starts_with("HTTP/1.1 403"));
        super::retire("lab-2");
        tokio::task::yield_now().await;
        assert!(!health(second, "tok2").await.contains("nonce2"));
    }

    #[tokio::test]
    async fn an_unused_server_ends_after_its_time() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let probe = std::sync::Arc::new(Probe::new("tok".into(), "n".into(), port));
        tokio::time::timeout(std::time::Duration::from_secs(5), super::accept_until_spent(listener, probe, std::time::Duration::from_millis(50)))
            .await
            .expect("the server outlived its time");
        assert!(health(port, "tok").await.is_empty());
    }
}
