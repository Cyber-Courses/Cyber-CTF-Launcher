//! The screen window's bridge: a `WebSocket` server on this computer's loopback that the window's
//! VNC viewer (`noVNC`) connects to, relaying to the machine's console, so the webview itself never
//! reaches the network. As the co-location probe does (`account::colocation`), it binds the
//! loopback only, takes one connection with a one-time token, and checks the request's `Host`
//! (a DNS-rebinding page names its own) and `Origin` (the app's own page, exactly) before it
//! answers: a page of a website, or another program of this computer without the token, gets
//! nothing.
//!
//! Its listener is bound when the window opens (its port goes into the window's content security
//! policy), and armed when the window asks for its console ([`Bridge::arm`]): a target and a
//! fresh token, spent by the first viewer that connects with it.
//!
//! - **A VNC port** of the loopback ([`Target::Tcp`]): the viewer's frames are unwrapped onto
//!   the TCP connection, its bytes wrapped into binary frames back.
//! - **Proxmox VE's VNC websocket** ([`Target::WebSocket`]): the bridge opens it with the header
//!   that signs it in (TLS unverified only when the server's connection says so), then relays the
//!   frames both ways as they are (the viewer's are masked, as Proxmox wants a client's).

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::ws;
use crate::error::{Error, Result};

/// Where the bridge relays to.
#[derive(Clone)]
pub(crate) enum Target {
    /// A TCP port speaking VNC.
    Tcp(SocketAddr),
    /// A websocket speaking VNC (Proxmox's).
    WebSocket {
        /// Its `wss://` URL (it holds the session's ticket).
        url: String,
        /// The header that signs it in: its name and value.
        header: (String, String),
        /// Its TLS certificate isn't verified (the server's connection says so).
        insecure: bool,
    },
}

impl std::fmt::Debug for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Target::Tcp(at) => write!(f, "Tcp({at})"),
            // The URL holds the console's ticket, the header its credential.
            Target::WebSocket { insecure, .. } => write!(f, "WebSocket {{ insecure: {insecure} }}"),
        }
    }
}

/// The origins the app's own pages have: macOS's and Linux's `WebKit` (`tauri://localhost`),
/// Windows' `WebView2` (`http`, or `https` with Tauri's HTTPS scheme), and in a dev build (`dev`)
/// the Next dev server.
fn origin_allowed_in(origin: &str, dev: bool) -> bool {
    matches!(origin, "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost") || (dev && origin == "http://localhost:3206")
}

/// Constant-time string equality, so the token check doesn't leak through timing.
fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A request header's value.
fn header<'a>(req: &'a str, name: &str) -> Option<&'a str> {
    req.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// Why the bridge refuses a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Not a `WebSocket` upgrade (`GET`, `Upgrade: websocket`, version 13, a key).
    NotWebSocket,
    /// Its `Host` isn't this listener on the loopback.
    Host,
    /// Its `Origin` isn't the app's.
    Origin,
    /// No token, a wrong one, or one already spent.
    Token,
}

impl Refusal {
    /// The HTTP answer to a refused request.
    fn answer(self) -> &'static str {
        match self {
            Refusal::NotWebSocket => "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            Refusal::Host | Refusal::Origin | Refusal::Token => "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        }
    }
}

/// An accepted upgrade: the answer to send, and the subprotocol the viewer asked for (`binary`).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Accepted {
    /// The `101 Switching Protocols` answer.
    pub(crate) answer: String,
    /// The subprotocol agreed on (`binary`), when the viewer asked for it.
    pub(crate) protocol: Option<String>,
}

/// Judges `req`, a request to the bridge listening on `port`, whose token is `token` (None once
/// spent). Pure, so it is unit-tested directly.
pub(crate) fn judge(req: &str, token: Option<&str>, port: u16, dev: bool) -> std::result::Result<Accepted, Refusal> {
    let mut first = req.lines().next().unwrap_or_default().split_whitespace();
    let (method, target) = (first.next().unwrap_or_default(), first.next().unwrap_or_default());
    let upgrade = header(req, "upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket"));
    let key = header(req, "sec-websocket-key").filter(|k| !k.is_empty());
    if method != "GET" || !upgrade || header(req, "sec-websocket-version") != Some("13") || key.is_none() {
        return Err(Refusal::NotWebSocket);
    }
    let host_ok = header(req, "host").is_some_and(|h| h == format!("127.0.0.1:{port}") || h.eq_ignore_ascii_case(&format!("localhost:{port}")));
    if !host_ok {
        return Err(Refusal::Host);
    }
    if !header(req, "origin").is_some_and(|o| origin_allowed_in(o, dev)) {
        return Err(Refusal::Origin);
    }
    let given = target.strip_prefix('/').unwrap_or_default();
    if !token.is_some_and(|t| ct_eq(given, t)) {
        return Err(Refusal::Token);
    }
    // noVNC asks for `binary`; the bridge speaks it and nothing else.
    let protocol = header(req, "sec-websocket-protocol").and_then(|p| p.split(',').map(str::trim).find(|p| *p == "binary")).map(str::to_owned);
    let mut answer = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n",
        ws::accept_key(key.unwrap_or_default())
    );
    if let Some(p) = &protocol {
        answer = format!("{answer}Sec-WebSocket-Protocol: {p}\r\n");
    }
    answer.push_str("\r\n");
    Ok(Accepted { answer, protocol })
}

/// How long a connection gets to send its request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// The largest request the bridge reads.
const MAX_REQUEST: usize = 8 * 1024;

/// What an armed bridge holds: its target, and its token until a viewer spends it.
#[derive(Debug, Default)]
struct Armed {
    target: Option<Target>,
    token: Option<String>,
}

/// One screen window's bridge: its loopback listener, accepting while the window lives.
pub(crate) struct Bridge {
    local: SocketAddr,
    armed: Arc<Mutex<Armed>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Bridge {
    /// Binds a free port of the loopback and starts accepting; nothing is relayed until armed.
    pub(crate) async fn bind() -> Result<Bridge> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await.map_err(Error::Io)?;
        let local = listener.local_addr().map_err(Error::Io)?;
        let port = local.port();
        let armed = Arc::new(Mutex::new(Armed::default()));
        let task = tokio::spawn(accept(listener, port, Arc::clone(&armed)));
        Ok(Bridge { local, armed, task })
    }

    /// Its port on the loopback.
    pub(crate) fn port(&self) -> u16 {
        self.local.port()
    }

    /// Relays the next viewer to `target`, which must connect with the token this returns.
    pub(crate) fn arm(&self, target: Target) -> String {
        let token = crate::platform::random_hex(16);
        let mut armed = self.armed.lock().unwrap_or_else(PoisonError::into_inner);
        *armed = Armed { target: Some(target), token: Some(token.clone()) };
        token
    }
}

/// Accepts connections on `listener` (`port`), each answered on its own.
async fn accept(listener: TcpListener, port: u16, armed: Arc<Mutex<Armed>>) {
    loop {
        let Ok((socket, _)) = listener.accept().await else {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };
        tokio::spawn(serve(socket, port, Arc::clone(&armed)));
    }
}

/// Reads one request; an accepted one spends the token and is relayed to the target.
async fn serve(mut socket: TcpStream, port: u16, armed: Arc<Mutex<Armed>>) {
    let Ok(Some((req, rest))) = tokio::time::timeout(REQUEST_TIMEOUT, read_request(&mut socket)).await.unwrap_or(Ok(None)) else { return };
    let judged = {
        let mut a = armed.lock().unwrap_or_else(PoisonError::into_inner);
        let judged = judge(&req, a.token.as_deref(), port, cfg!(debug_assertions));
        if judged.is_ok() {
            // Spent: a second connection, even with the same token, is refused.
            a.token = None;
        }
        judged.map(|acc| (acc, a.target.clone()))
    };
    match judged {
        Err(refusal) => {
            log::warn!("the screen bridge refused a connection: {refusal:?}");
            let _ = socket.write_all(refusal.answer().as_bytes()).await;
        }
        Ok((_, None)) => {}
        Ok((accepted, Some(target))) => {
            if socket.write_all(accepted.answer.as_bytes()).await.is_err() {
                return;
            }
            let ended = match target {
                Target::Tcp(at) => relay_tcp(socket, rest, at).await,
                Target::WebSocket { url, header, insecure } => {
                    let upstream = Upstream { url: &url, header: &header, insecure, protocol: accepted.protocol.as_deref() };
                    relay_websocket(socket, rest, &upstream).await
                }
            };
            if let Err(e) = ended {
                log::warn!("the screen's connection ended: {e}");
            }
        }
    }
}

/// The request's head (up to its blank line) and the bytes after it; `None` when the connection
/// closes first or the head is over [`MAX_REQUEST`].
async fn read_request(socket: &mut TcpStream) -> std::io::Result<Option<(String, Vec<u8>)>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(end + 4);
            return Ok(Some((String::from_utf8_lossy(&buf).into_owned(), rest)));
        }
        if buf.len() > MAX_REQUEST {
            return Ok(None);
        }
        let n = socket.read(&mut chunk).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend(chunk.iter().take(n));
    }
}

/// Relays the viewer on `socket` (with `pending` bytes already read) to the VNC port `at`.
async fn relay_tcp(socket: TcpStream, mut pending: Vec<u8>, at: SocketAddr) -> std::io::Result<()> {
    let vnc = TcpStream::connect(at).await?;
    let (mut vnc_in, mut vnc_out) = vnc.into_split();
    let (mut viewer_in, viewer_out) = socket.into_split();
    let viewer_out = Arc::new(tokio::sync::Mutex::new(viewer_out));
    let to_viewer = {
        let viewer_out = Arc::clone(&viewer_out);
        async move {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = vnc_in.read(&mut buf).await?;
                if n == 0 {
                    let _ = viewer_out.lock().await.write_all(&ws::close()).await;
                    return Ok::<(), std::io::Error>(());
                }
                viewer_out.lock().await.write_all(&ws::binary(buf.get(..n).unwrap_or_default())).await?;
            }
        }
    };
    let from_viewer = async move {
        let mut chunk = vec![0u8; 16 * 1024];
        loop {
            loop {
                match ws::decode(&pending) {
                    Ok(Some((frame, used))) => {
                        pending.drain(..used);
                        match frame {
                            ws::Frame::Data(d) => vnc_out.write_all(&d).await?,
                            ws::Frame::Ping(p) => viewer_out.lock().await.write_all(&ws::pong(&p)).await?,
                            ws::Frame::Pong => {}
                            ws::Frame::Close => return Ok(()),
                        }
                    }
                    Ok(None) => break,
                    Err(refused) => return Err(std::io::Error::other(format!("the viewer sent a frame the bridge refuses: {refused:?}"))),
                }
            }
            let n = viewer_in.read(&mut chunk).await?;
            if n == 0 {
                return Ok(());
            }
            pending.extend(chunk.iter().take(n));
        }
    };
    tokio::select! {
        r = to_viewer => r,
        r = from_viewer => r,
    }
}

/// The websocket a viewer is relayed to.
struct Upstream<'a> {
    /// Its `wss://` URL.
    url: &'a str,
    /// The header that signs it in.
    header: &'a (String, String),
    /// Its certificate isn't verified.
    insecure: bool,
    /// The subprotocol to ask it for (the viewer's).
    protocol: Option<&'a str>,
}

/// Relays the viewer on `socket` (with `pending` bytes already read) to the websocket `to`,
/// frames as they are.
async fn relay_websocket(socket: TcpStream, pending: Vec<u8>, to: &Upstream<'_>) -> std::io::Result<()> {
    let upstream = open_websocket(to.url, to.header, to.insecure, to.protocol).await.map_err(std::io::Error::other)?;
    let (mut up_in, mut up_out) = tokio::io::split(upstream);
    let (mut viewer_in, mut viewer_out) = socket.into_split();
    up_out.write_all(&pending).await?;
    tokio::select! {
        r = tokio::io::copy(&mut viewer_in, &mut up_out) => r.map(drop),
        r = tokio::io::copy(&mut up_in, &mut viewer_out) => r.map(drop),
    }
}

/// Opens the websocket `url` (`wss://`) with `header`, asking for `protocol`: an HTTP/1.1
/// upgrade, its stream once the server switched protocols.
async fn open_websocket(url: &str, header: &(String, String), insecure: bool, protocol: Option<&str>) -> std::result::Result<reqwest::Upgraded, String> {
    let https = url.strip_prefix("wss://").map(|rest| format!("https://{rest}")).ok_or("not a wss:// URL")?;
    let client = reqwest::Client::builder()
        .http1_only()
        .tls_danger_accept_invalid_certs(insecure)
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let key = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, rand::random::<[u8; 16]>());
    let mut req = client
        .get(https)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", &key)
        .header(header.0.as_str(), header.1.as_str());
    if let Some(p) = protocol {
        req = req.header("Sec-WebSocket-Protocol", p);
    }
    let answer = req.send().await.map_err(|e| e.to_string())?;
    if answer.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS {
        return Err(format!("the server answered {}", answer.status()));
    }
    let accepted = answer.headers().get("sec-websocket-accept").and_then(|v| v.to_str().ok()).map(str::to_owned);
    if accepted.as_deref() != Some(ws::accept_key(&key).as_str()) {
        return Err("the server's accept key is wrong".to_owned());
    }
    answer.upgrade().await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A viewer's upgrade request to the bridge on `port`, with `token`, from `origin`.
    fn request(port: u16, token: &str, origin: &str) -> String {
        format!(
            "GET /{token} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Protocol: binary\r\n\r\n"
        )
    }

    #[test]
    fn the_apps_page_with_the_token_is_accepted_and_answered_as_rfc_6455_says() {
        let a = judge(&request(4100, "t0k", "tauri://localhost"), Some("t0k"), 4100, false).unwrap();
        assert!(a.answer.starts_with("HTTP/1.1 101 Switching Protocols\r\n"), "{}", a.answer);
        assert!(a.answer.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"));
        assert_eq!(a.protocol.as_deref(), Some("binary"));
        for origin in ["http://tauri.localhost", "https://tauri.localhost"] {
            assert!(judge(&request(4100, "t0k", origin), Some("t0k"), 4100, false).is_ok(), "{origin}");
        }
    }

    #[test]
    fn a_wrong_or_spent_token_another_origin_or_host_is_refused() {
        let req = request(4100, "t0k", "tauri://localhost");
        assert_eq!(judge(&req, Some("other"), 4100, false), Err(Refusal::Token));
        assert_eq!(judge(&req, None, 4100, false), Err(Refusal::Token), "spent");
        assert_eq!(judge(&request(4100, "", "tauri://localhost"), Some("t0k"), 4100, false), Err(Refusal::Token));
        for origin in ["https://evil.example", "null", "tauri://localhost.evil", "http://localhost:3206"] {
            assert_eq!(judge(&request(4100, "t0k", origin), Some("t0k"), 4100, false), Err(Refusal::Origin), "{origin}");
        }
        assert!(judge(&request(4100, "t0k", "http://localhost:3206"), Some("t0k"), 4100, true).is_ok(), "the dev server, in a dev build");
        let rebound = req.replace("Host: 127.0.0.1:4100", "Host: attacker.example:4100");
        assert_eq!(judge(&rebound, Some("t0k"), 4100, false), Err(Refusal::Host));
        assert_eq!(judge(&req, Some("t0k"), 4101, false), Err(Refusal::Host), "another port");
        let no_origin = req.replace("Origin: tauri://localhost\r\n", "");
        assert_eq!(judge(&no_origin, Some("t0k"), 4100, false), Err(Refusal::Origin));
        let plain = "GET /t0k HTTP/1.1\r\nHost: 127.0.0.1:4100\r\n\r\n";
        assert_eq!(judge(plain, Some("t0k"), 4100, false), Err(Refusal::NotWebSocket));
    }

    /// A viewer's masked binary frame.
    fn masked(payload: &[u8]) -> Vec<u8> {
        let mask = [3u8, 1, 4, 1];
        let mut out = vec![0x82, 0x80 | u8::try_from(payload.len()).unwrap()];
        out.extend_from_slice(&mask);
        out.extend(payload.iter().zip(mask.iter().cycle()).map(|(b, m)| b ^ m));
        out
    }

    /// Connects to the bridge as the screen window's viewer would, with `token` from `origin`.
    async fn connect(port: u16, token: &str, origin: &str) -> (TcpStream, String) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(request(port, token, origin).as_bytes()).await.unwrap();
        let mut head = Vec::new();
        let mut b = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if s.read(&mut b).await.unwrap() == 0 {
                break;
            }
            head.push(b[0]);
        }
        (s, String::from_utf8_lossy(&head).into_owned())
    }

    #[tokio::test]
    async fn the_bridge_listens_on_the_loopback_relays_one_viewer_and_spends_its_token() {
        // A fake VNC server: says its version, then echoes what it reads.
        let vnc = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let vnc_at = vnc.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = vnc.accept().await.unwrap();
            s.write_all(b"RFB 003.008\n").await.unwrap();
            let mut buf = [0u8; 64];
            let n = s.read(&mut buf).await.unwrap();
            s.write_all(&buf[..n]).await.unwrap();
        });
        let bridge = Bridge::bind().await.unwrap();
        let port = bridge.port();
        assert!(bridge.local.ip().is_loopback(), "bound to the loopback only: {}", bridge.local);
        let token = bridge.arm(Target::Tcp(vnc_at));
        let (_, refused) = connect(port, &token, "https://evil.example").await;
        assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
        let (mut viewer, head) = connect(port, &token, "tauri://localhost").await;
        assert!(head.starts_with("HTTP/1.1 101"), "{head}");
        let mut frame = [0u8; 14];
        viewer.read_exact(&mut frame).await.unwrap();
        assert_eq!(&frame, b"\x82\x0cRFB 003.008\n", "the server's bytes in a binary frame");
        viewer.write_all(&masked(b"RFB 003.008\n")).await.unwrap();
        viewer.read_exact(&mut frame).await.unwrap();
        assert_eq!(&frame, b"\x82\x0cRFB 003.008\n", "the viewer's bytes reached the server unmasked");
        let (_, again) = connect(port, &token, "tauri://localhost").await;
        assert!(again.starts_with("HTTP/1.1 403"), "the token was spent: {again}");
        drop(bridge);
    }

    /// One frame from the bridge (unmasked): its payload.
    async fn frame(s: &mut TcpStream) -> Vec<u8> {
        let mut head = [0u8; 2];
        s.read_exact(&mut head).await.unwrap();
        let len = match head[1] & 0x7F {
            126 => {
                let mut l = [0u8; 2];
                s.read_exact(&mut l).await.unwrap();
                usize::from(u16::from_be_bytes(l))
            }
            127 => {
                let mut l = [0u8; 8];
                s.read_exact(&mut l).await.unwrap();
                usize::try_from(u64::from_be_bytes(l)).unwrap()
            }
            n => usize::from(n),
        };
        let mut payload = vec![0u8; len];
        s.read_exact(&mut payload).await.unwrap();
        payload
    }

    /// `n` bytes of the server's, across as many frames as they came in.
    async fn server_bytes(s: &mut TcpStream, buf: &mut Vec<u8>, n: usize) -> Vec<u8> {
        while buf.len() < n {
            buf.extend(frame(s).await);
        }
        buf.drain(..n).collect()
    }

    /// The bridge to a real VNC server (a lab VM's console): the RFB handshake through it, up to
    /// the server's framebuffer size. `CYBERCTF_TEST_SCREEN_VNC=127.0.0.1:5900 cargo test --lib --
    /// --ignored a_real_vnc_server`.
    #[tokio::test]
    #[ignore = "needs a VNC server with no password: CYBERCTF_TEST_SCREEN_VNC=<loopback host:port>"]
    async fn a_real_vnc_server_answers_through_the_bridge() {
        let at: SocketAddr = std::env::var("CYBERCTF_TEST_SCREEN_VNC").expect("CYBERCTF_TEST_SCREEN_VNC").parse().unwrap();
        let bridge = Bridge::bind().await.unwrap();
        let token = bridge.arm(Target::Tcp(at));
        let (mut viewer, head) = connect(bridge.port(), &token, "tauri://localhost").await;
        assert!(head.starts_with("HTTP/1.1 101"), "{head}");
        let mut buf = Vec::new();
        let version = server_bytes(&mut viewer, &mut buf, 12).await;
        assert!(version.starts_with(b"RFB 003.00"), "{version:?}");
        viewer.write_all(&masked(b"RFB 003.008\n")).await.unwrap();
        let count = server_bytes(&mut viewer, &mut buf, 1).await[0];
        let types = server_bytes(&mut viewer, &mut buf, usize::from(count)).await;
        assert!(types.contains(&1), "the server takes no-password: {types:?}");
        viewer.write_all(&masked(&[1])).await.unwrap();
        assert_eq!(server_bytes(&mut viewer, &mut buf, 4).await, [0, 0, 0, 0], "security result: OK");
        viewer.write_all(&masked(&[1])).await.unwrap();
        let init = server_bytes(&mut viewer, &mut buf, 24).await;
        let (w, h) = (u16::from_be_bytes([init[0], init[1]]), u16::from_be_bytes([init[2], init[3]]));
        let name_len = usize::try_from(u32::from_be_bytes([init[20], init[21], init[22], init[23]])).unwrap();
        let name = server_bytes(&mut viewer, &mut buf, name_len).await;
        println!("the screen through the bridge: {w}x{h}, {}", String::from_utf8_lossy(&name));
        assert!(w > 0 && h > 0);
    }
}
