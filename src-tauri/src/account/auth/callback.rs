//! The loopback redirect (RFC 8252): a one-shot local server the browser comes back to after
//! sign-in, and the page it shows there.

use std::future::Future;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::error::{Code, Error, Result};
use crate::platform::config;

/// The first free loopback port registered on the OAuth client.
pub(super) async fn bind_loopback() -> Result<(TcpListener, u16)> {
    for port in config::REDIRECT_PORTS {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)).await {
            return Ok((listener, port));
        }
    }
    Err(Error::Invalid("no free login callback port (47290-47292)".into()))
}

/// The page the browser shows once Cyber Auth sends the player back: the family look (the launcher's
/// dark tokens, light when the browser is), the app mark, and a button back to the app.
fn login_page(ok: bool) -> String {
    const TEMPLATE: &str = include_str!("login-page.html");
    const LOGO: &str = include_str!("../../../../public/logo-mark.svg");
    const CHECK: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>"#;
    const CROSS: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18M6 6l12 12"/></svg>"#;
    // The SVG file starts with an XML prolog, which can't sit inside HTML.
    let logo = LOGO.find("<svg").map_or("", |i| &LOGO[i..]);
    let (state, icon, title, body, button) = if ok {
        (
            "ok",
            CHECK,
            "You're signed in",
            "Cyber CTF is connected to your account. Labs you launch from the website now run on this machine.",
            "Return to Cyber CTF",
        )
    } else {
        ("error", CROSS, "Sign-in didn't finish", "It was cancelled or the link expired. Go back to Cyber CTF and press Sign in again.", "Back to Cyber CTF")
    };
    TEMPLATE
        .replace("__STATE__", state)
        .replace("__LOGO__", logo)
        .replace("__ICON__", icon)
        .replace("__TITLE__", title)
        .replace("__BODY__", body)
        .replace("__BUTTON__", button)
}

/// The redirect's URL, from the raw request's target (`GET /callback?code=… HTTP/1.1`).
fn request_url(request: &str) -> Option<url::Url> {
    let target = request.split_whitespace().nth(1).unwrap_or("");
    url::Url::parse(&format!("http://127.0.0.1{target}")).ok()
}

/// What a callback URL says: (code, state), or why sign-in failed.
pub(super) fn outcome(url: &url::Url) -> Result<(String, String)> {
    let param = |k: &str| url.query_pairs().find(|(key, _)| key == k).map(|(_, v)| v.into_owned());
    if let Some(error) = param("error") {
        // The player said no on cyber-auth's consent screen: not a failure of the app.
        if error == "access_denied" {
            return Err(Error::coded(Code::SignInDenied));
        }
        return Err(Error::Invalid(format!("login failed: {error}")));
    }
    match (param("code"), param("state")) {
        (Some(code), Some(state)) => Ok((code, state)),
        _ => Err(Error::Invalid("login callback without code".into())),
    }
}

fn html_response(page: &str) -> String {
    format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len())
}

/// How long a connection may take to send its request line and headers. A browser opens
/// speculative connections that send nothing; each is read on its own and dropped after this.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// A connection's request head (up to the blank line, at most 8 KiB), or None when it sends
/// nothing in time.
async fn read_request(stream: &mut TcpStream) -> Option<String> {
    let read = async {
        let mut buf = Vec::with_capacity(1024);
        let mut chunk = [0u8; 1024];
        loop {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.windows(2).any(|w| w == b"\n\n") || buf.len() >= 8192 {
                break;
            }
        }
        Some(String::from_utf8_lossy(&buf).into_owned())
    };
    tokio::time::timeout(READ_TIMEOUT, read).await.ok().flatten().filter(|r| !r.is_empty())
}

/// Whether a callback URL belongs to this sign-in: it carries the `state` this login sent.
pub(super) fn state_matches(url: &url::Url, state: &str) -> bool {
    url.query_pairs().any(|(k, v)| k == "state" && v == state)
}

/// Waits for the browser's redirect for this sign-in (`state`), then finishes it with
/// `finish(code)` (the token exchange) before answering the browser: the page says "signed in"
/// only once that succeeded. Each connection is read in its own task, so an idle one (a
/// browser's preconnect) can't hold up the real redirect, and a callback with a wrong or missing
/// state is answered and ignored.
pub(super) async fn receive<T, F, Fut>(listener: TcpListener, state: &str, mut finish: F) -> Result<T>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(TcpStream, url::Url)>(8);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (mut stream, _) = accepted?;
                let tx = tx.clone();
                tokio::spawn(async move {
                    let Some(url) = read_request(&mut stream).await.as_deref().and_then(request_url) else { return };
                    if url.path() != "/callback" {
                        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                        return;
                    }
                    let _ = tx.send((stream, url)).await;
                });
            }
            Some((mut stream, url)) = rx.recv() => {
                if !state_matches(&url, state) {
                    // Not this sign-in's redirect (an old tab, another page): keep waiting.
                    let _ = stream.write_all(html_response(&login_page(false)).as_bytes()).await;
                    continue;
                }
                let result = match outcome(&url) {
                    Ok((code, _)) => finish(code).await,
                    Err(e) => Err(e),
                };
                let _ = stream.write_all(html_response(&login_page(result.is_ok())).as_bytes()).await;
                let _ = stream.shutdown().await;
                return result;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_callback_gives_the_code_and_state_or_the_error() {
        let url = request_url("GET /callback?code=abc&state=xyz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
        assert_eq!(url.path(), "/callback");
        assert_eq!(outcome(&url).unwrap(), ("abc".to_string(), "xyz".to_string()));
        let denied = request_url("GET /callback?error=access_denied&state=xyz HTTP/1.1").unwrap();
        assert_eq!(outcome(&denied).unwrap_err().code(), Code::SignInDenied);
        let failed = request_url("GET /callback?error=server_error&state=xyz HTTP/1.1").unwrap();
        assert_eq!(outcome(&failed).unwrap_err().to_string(), "login failed: server_error");
        let bare = request_url("GET /callback HTTP/1.1").unwrap();
        assert_eq!(outcome(&bare).unwrap_err().to_string(), "login callback without code");
        assert_eq!(request_url("GET /favicon.ico HTTP/1.1").unwrap().path(), "/favicon.ico");
    }

    #[test]
    fn the_login_page_says_how_it_went() {
        let ok = login_page(true);
        assert!(ok.contains("You're signed in") && !ok.contains("__"), "placeholders left in the page");
        assert!(login_page(false).contains("Sign-in didn't finish"));
        assert!(html_response("hi").ends_with("Content-Length: 2\r\nConnection: close\r\n\r\nhi"));
    }

    async fn send(port: u16, request: &str) -> String {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut out = String::new();
        let _ = stream.read_to_string(&mut out).await;
        out
    }

    /// A listener on a free port, waiting for sign-in `s1` whose exchange answers `exchange`.
    async fn waiting(exchange: Result<&'static str>) -> (u16, tokio::task::JoinHandle<Result<String>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut exchange = Some(exchange);
        let task = tokio::spawn(async move {
            receive(listener, "s1", move |code| {
                let answer = exchange.take().unwrap().map(|t| format!("{t}:{code}"));
                async move { answer }
            })
            .await
        });
        (port, task)
    }

    #[tokio::test]
    async fn receive_skips_other_paths_and_finishes_the_callback() {
        let (port, task) = waiting(Ok("token")).await;
        assert!(send(port, "GET /favicon.ico HTTP/1.1\r\n\r\n").await.starts_with("HTTP/1.1 404"));
        let page = send(port, "GET /callback?code=c1&state=s1 HTTP/1.1\r\n\r\n").await;
        assert!(page.starts_with("HTTP/1.1 200") && page.contains("You're signed in"));
        assert_eq!(task.await.unwrap().unwrap(), "token:c1");
    }

    #[tokio::test]
    async fn an_idle_connection_does_not_block_the_redirect() {
        // A browser preconnect that never sends anything held the one-at-a-time listener.
        let (port, task) = waiting(Ok("token")).await;
        let _idle = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let page = tokio::time::timeout(Duration::from_secs(3), send(port, "GET /callback?code=c1&state=s1 HTTP/1.1\r\n\r\n")).await.unwrap();
        assert!(page.contains("You're signed in"));
        assert!(task.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn a_callback_with_a_wrong_or_missing_state_is_ignored() {
        // It ended the login before the state was checked.
        let (port, task) = waiting(Ok("token")).await;
        assert!(send(port, "GET /callback?code=evil&state=other HTTP/1.1\r\n\r\n").await.contains("Sign-in didn't finish"));
        assert!(send(port, "GET /callback?code=evil HTTP/1.1\r\n\r\n").await.contains("Sign-in didn't finish"));
        assert!(!task.is_finished());
        assert!(send(port, "GET /callback?code=c2&state=s1 HTTP/1.1\r\n\r\n").await.contains("You're signed in"));
        assert_eq!(task.await.unwrap().unwrap(), "token:c2");
    }

    #[tokio::test]
    async fn a_failed_exchange_shows_the_failure_page() {
        // "Signed in" was shown before the token exchange ran.
        let (port, task) = waiting(Err(Error::Invalid("invalid_grant".into()))).await;
        let page = send(port, "GET /callback?code=c1&state=s1 HTTP/1.1\r\n\r\n").await;
        assert!(page.contains("Sign-in didn't finish") && !page.contains("You're signed in"));
        assert_eq!(task.await.unwrap().unwrap_err().to_string(), "invalid_grant");
    }

    #[test]
    fn repeated_and_encoded_parameters_take_the_first_decoded_value() {
        // A query is decoded before it is read, and a repeated key keeps its first value (a
        // second `code` appended to the URL never replaces the one Cyber Auth sent).
        let url = request_url("GET /callback?code=a%2Bb&code=second&state=s%31 HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(outcome(&url).unwrap(), ("a+b".to_string(), "s1".to_string()));
        assert!(state_matches(&url, "s1"));
        assert!(!state_matches(&url, "s%31"), "compared decoded, not as typed");
        // An error wins over a code that comes with it.
        let both = request_url("GET /callback?code=c&error=access_denied&state=s1 HTTP/1.1").unwrap();
        assert_eq!(outcome(&both).unwrap_err().code(), Code::SignInDenied);
    }

    #[tokio::test]
    async fn a_request_without_an_http_version_or_with_bare_line_feeds_is_still_the_redirect() {
        // HTTP/0.9 style: no version, LF-only line endings. Both are read and finish the login.
        assert_eq!(request_url("GET /callback?code=c&state=s1\n").unwrap().path(), "/callback");
        let (port, task) = waiting(Ok("token")).await;
        let page = send(port, "GET /callback?code=c1&state=s1\n\n").await;
        assert!(page.contains("You're signed in"), "{page}");
        assert_eq!(task.await.unwrap().unwrap(), "token:c1");
    }

    #[tokio::test]
    async fn an_oversized_garbled_or_closed_request_is_read_as_not_the_callback() {
        // Read directly: the request head is capped at 8 KiB and whatever came is parsed, so a
        // flood with no request line is a 404, not a hang; binary junk is decoded lossily and has
        // no target; a connection closed without a byte is nothing at all.
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (mut server, _) = listener.accept().await.unwrap();
        client.write_all(&vec![b'A'; 12_000]).await.unwrap();
        let head = read_request(&mut server).await.unwrap();
        assert!((8192..12_000).contains(&head.len()), "{}", head.len());
        assert_eq!(request_url(&head).unwrap().path(), "/");
        let mut client = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (mut server, _) = listener.accept().await.unwrap();
        client.write_all(b"\xff\xfe\n\n").await.unwrap();
        let head = read_request(&mut server).await.unwrap();
        assert_eq!(request_url(&head).unwrap().path(), "/");
        let client = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (mut server, _) = listener.accept().await.unwrap();
        drop(client);
        assert_eq!(read_request(&mut server).await, None);
    }

    #[tokio::test]
    async fn receive_shows_the_failure_page_on_an_error() {
        let (port, task) = waiting(Ok("token")).await;
        let page = send(port, "GET /callback?error=access_denied&state=s1 HTTP/1.1\r\n\r\n").await;
        assert!(page.contains("Sign-in didn't finish"));
        assert!(task.await.unwrap().is_err());
    }
}

#[cfg(test)]
mod proptests;
