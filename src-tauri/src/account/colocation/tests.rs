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
    let r = build_response(&get("/health?token=secret", "https://cyberctf.com"), None, "abc123", PORT);
    assert!(r.text.starts_with("HTTP/1.1 403") && !r.text.contains("abc123") && !r.gave_nonce);
}

#[test]
fn the_nonce_is_given_once() {
    let probe = Probe::new("secret".into(), "abc123".into(), PORT);
    let req = get("/health?token=secret", "https://cyberctf.com");
    assert!(probe.reply_to(&req).contains("\"nonce\":\"abc123\""));
    let again = probe.reply_to(&req);
    assert!(again.starts_with("HTTP/1.1 403") && !again.contains("abc123"), "{again}");
}

#[test]
fn only_a_loopback_host_on_this_port_reads_the_nonce() {
    // A DNS-rebinding page sends its own host name; the token alone isn't enough.
    let with_host = |host: &str| format!("GET /health?token=t HTTP/1.1\r\nHost: {host}\r\nOrigin: https://cyberctf.com\r\n\r\n");
    for host in [format!("127.0.0.1:{PORT}"), format!("localhost:{PORT}"), format!("LOCALHOST:{PORT}")] {
        assert!(respond(&with_host(&host), "t", "n").starts_with("HTTP/1.1 200"), "{host}");
    }
    for host in ["evil.example.com".to_string(), format!("evil.example.com:{PORT}"), "127.0.0.1".into(), "127.0.0.1:1".into(), format!("127.0.0.2:{PORT}")] {
        let r = respond(&with_host(&host), "t", "n");
        assert!(r.starts_with("HTTP/1.1 403") && !r.contains("\"nonce\""), "{host}: {r}");
    }
    // No Host header at all: refused.
    assert!(respond("GET /health?token=t HTTP/1.1\r\nOrigin: https://cyberctf.com\r\n\r\n", "t", "n").starts_with("HTTP/1.1 403"));
}

#[test]
fn a_foreign_origin_gets_no_nonce_and_spends_nothing() {
    let probe = Probe::new("secret".into(), "abc123".into(), PORT);
    for origin in ["https://evil.example.com", ""] {
        let r = probe.reply_to(&get("/health?token=secret", origin));
        assert!(r.starts_with("HTTP/1.1 403") && !r.contains("abc123") && !r.contains("Access-Control-Allow-Origin"), "{origin}: {r}");
    }
    // The site's own check still gets it.
    assert!(probe.reply_to(&get("/health?token=secret", "https://cyberctf.com")).contains("abc123"));
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
    let io = std::time::Duration::from_millis(200);
    let served = tokio::spawn(async move { super::answer(&mut server_side, &probe, io).await });
    let mut out = Vec::new();
    // Nothing sent: the server closes on its own, without an answer.
    let read = tokio::time::timeout(io * 5, s.read_to_end(&mut out)).await;
    assert!(read.is_ok(), "still open after the timeout");
    assert_eq!(out, b"");
    served.await.unwrap();
}

#[test]
fn the_token_is_the_first_token_parameter_read_as_typed() {
    // Among other parameters, and first of two: the one checked. A percent-encoded, a
    // prefixed (`tokens=`) or a bare `token` never matches (tokens are plain hex).
    let origin = "https://cyberctf.com";
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
    ok(&format!("GET /health?token=t HTTP/1.1\nHost: 127.0.0.1:{PORT}\nOrigin: https://cyberctf.com\n\n"));
    ok(&format!("GET /health?token=t\r\nhost: localhost:{PORT}\r\norigin: https://cyberctf.com"));
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
    let pre = respond("OPTIONS /health HTTP/1.1\r\nOrigin: https://cyberctf.com\r\n\r\n", "t", "n");
    assert!(pre.starts_with("HTTP/1.1 204") && pre.contains("Access-Control-Allow-Origin: https://cyberctf.com"));
    assert!(respond("POST /health?token=t HTTP/1.1\r\n\r\n", "t", "n").starts_with("HTTP/1.1 405"));
    assert!(respond("", "t", "n").starts_with("HTTP/1.1 405"));
    assert!(!super::ct_eq("abc", "abd") && !super::ct_eq("abc", "ab") && super::ct_eq("abc", "abc"));
    assert!(!super::is_loopback_origin("ftp://localhost") && !super::is_loopback_origin("http://localhost:"));
}

async fn health(port: u16, token: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let Ok(mut s) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await else { return String::new() };
    let _ = s.write_all(format!("GET /health?token={token} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: https://cyberctf.com\r\n\r\n").as_bytes()).await;
    let mut out = String::new();
    let _ = s.read_to_string(&mut out).await;
    out
}

/// The tests over `serve` (the servers of every lab) run one at a time.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn serve_answers_once_on_loopback_then_closes() {
    let _serial = ONE_AT_A_TIME.lock().await;
    let port = super::serve("lab-1", "tok1".into(), "nonce1".into()).await.unwrap();
    assert!(health(port, "bad").await.starts_with("HTTP/1.1 403"));
    let ok = health(port, "tok1").await;
    assert!(ok.starts_with("HTTP/1.1 200") && ok.contains("nonce1"), "{ok}");
    // Spent: the server is gone (or, while it closes, refuses).
    let after = health(port, "tok1").await;
    assert!(!after.contains("nonce1"), "{after}");
}

/// Regression: one server for the whole app, so a lab launched while another lab's launch
/// waited for its check ended that check: the website couldn't confirm the first lab ran here.
#[tokio::test]
async fn labs_launched_at_once_each_keep_their_check() {
    let _serial = ONE_AT_A_TIME.lock().await;
    let first = super::serve("lab-1", "tok1".into(), "nonce1".into()).await.unwrap();
    let second = super::serve("lab-2", "tok2".into(), "nonce2".into()).await.unwrap();
    assert_ne!(first, second);
    let (one, two) = tokio::join!(health(first, "tok1"), health(second, "tok2"));
    assert!(one.starts_with("HTTP/1.1 200") && one.contains("nonce1"), "{one}");
    assert!(two.starts_with("HTTP/1.1 200") && two.contains("nonce2"), "{two}");
}

#[tokio::test]
async fn a_labs_new_launch_or_the_lab_stopping_retires_its_server_only() {
    let _serial = ONE_AT_A_TIME.lock().await;
    let first = super::serve("lab-1", "tok1".into(), "nonce1".into()).await.unwrap();
    let other = super::serve("lab-2", "tok2".into(), "nonce2".into()).await.unwrap();
    let again = super::serve("lab-1", "tok3".into(), "nonce3".into()).await.unwrap();
    tokio::task::yield_now().await;
    assert!(!health(first, "tok1").await.contains("nonce1"), "the lab's earlier launch's server is retired");
    // Another lab stopping leaves it alone; its own lab stopping ends it.
    super::retire("lab-2");
    assert!(health(again, "bad").await.starts_with("HTTP/1.1 403"));
    super::retire("lab-1");
    tokio::task::yield_now().await;
    assert!(!health(again, "tok3").await.contains("nonce3"));
    assert!(!health(other, "tok2").await.contains("nonce2"));
}

#[tokio::test]
async fn an_unused_server_ends_after_its_time() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let probe = std::sync::Arc::new(Probe::new("tok".into(), "n".into(), port));
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        super::accept_until_spent(listener, probe, super::Limits { ttl: std::time::Duration::from_millis(50), ..super::SERVED }),
    )
    .await
    .expect("the server outlived its time");
    assert_eq!(health(port, "tok").await, "");
}
