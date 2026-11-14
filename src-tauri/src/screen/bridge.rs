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
//!   the TCP connection, its bytes wrapped into binary frames back. A port that doesn't accept
//!   within [`CONNECT_LIMIT`], or a server that says nothing within [`ANSWER_LIMIT`] (a VNC
//!   server speaks first), closes the viewer's connection saying where it didn't answer: the
//!   viewer never waits on it for good.
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
use crate::platform::loopback_http::{ct_eq, header};

/// Where the bridge relays to.
#[derive(Clone)]
#[allow(dead_code, reason = "the screens come back with the runner's console offers")]
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

/// How much of the screen's stream one read takes to the viewer: frames come in bursts.
const SCREEN_CHUNK: usize = 64 * 1024;
/// How much of the viewer's input one read takes to the screen: key and pointer events, small.
const INPUT_CHUNK: usize = 16 * 1024;

/// The Next dev server a dev build's window loads: `tauri.conf.json`'s `devUrl`.
pub(crate) const DEV_SERVER_ORIGIN: &str = "http://127.0.0.1:3206";

/// The origins the app's own pages have: macOS's and Linux's `WebKit` (`tauri://localhost`),
/// Windows' `WebView2` (`http`, or `https` with Tauri's HTTPS scheme), and in a dev build (`dev`)
/// the Next dev server.
fn origin_allowed_in(origin: &str, dev: bool) -> bool {
    matches!(origin, "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost") || (dev && origin == DEV_SERVER_ORIGIN)
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

/// How long the machine's screen gets to accept the bridge's connection.
pub(crate) const CONNECT_LIMIT: Duration = Duration::from_secs(10);

/// How long a VNC server gets to say its version once connected, and Proxmox's websocket to
/// switch protocols.
pub(crate) const ANSWER_LIMIT: Duration = Duration::from_secs(15);

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

/// The VNC server at `at`, connected, and the first bytes it said: within [`CONNECT_LIMIT`] and
/// [`ANSWER_LIMIT`], else why not, in the player's words.
async fn answering(at: SocketAddr) -> std::result::Result<(TcpStream, Vec<u8>), String> {
    let mut vnc = match tokio::time::timeout(CONNECT_LIMIT, TcpStream::connect(at)).await {
        Ok(Ok(vnc)) => vnc,
        Ok(Err(e)) => return Err(format!("the VM's screen didn't answer at {at}: {e}")),
        Err(_) => return Err(format!("the VM's screen didn't answer at {at} within {} s", CONNECT_LIMIT.as_secs())),
    };
    let mut first = vec![0u8; 1024];
    match tokio::time::timeout(ANSWER_LIMIT, vnc.read(&mut first)).await {
        Ok(Ok(0)) => Err(format!("the VM's screen at {at} closed the connection before it said anything")),
        Ok(Ok(n)) => {
            first.truncate(n);
            Ok((vnc, first))
        }
        Ok(Err(e)) => Err(format!("the VM's screen at {at} failed: {e}")),
        Err(_) => Err(format!("the VM's screen at {at} accepted the connection but said nothing within {} s", ANSWER_LIMIT.as_secs())),
    }
}

/// Relays the viewer on `socket` (with `pending` bytes already read) to the VNC port `at`; one
/// that doesn't answer closes the viewer's connection saying why.
async fn relay_tcp(mut socket: TcpStream, mut pending: Vec<u8>, at: SocketAddr) -> std::io::Result<()> {
    let (vnc, first) = match answering(at).await {
        Ok(answered) => answered,
        Err(why) => {
            let _ = socket.write_all(&ws::close_because(&why)).await;
            return Err(std::io::Error::other(why));
        }
    };
    let (mut vnc_in, mut vnc_out) = vnc.into_split();
    let (mut viewer_in, viewer_out) = socket.into_split();
    let viewer_out = Arc::new(tokio::sync::Mutex::new(viewer_out));
    let to_viewer = {
        let viewer_out = Arc::clone(&viewer_out);
        async move {
            viewer_out.lock().await.write_all(&ws::binary(&first)).await?;
            let mut buf = vec![0u8; SCREEN_CHUNK];
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
        let mut chunk = vec![0u8; INPUT_CHUNK];
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
async fn relay_websocket(mut socket: TcpStream, pending: Vec<u8>, to: &Upstream<'_>) -> std::io::Result<()> {
    let opened = tokio::time::timeout(CONNECT_LIMIT + ANSWER_LIMIT, open_websocket(to.url, to.header, to.insecure, to.protocol)).await;
    let upstream = match opened {
        Ok(Ok(upstream)) => upstream,
        Ok(Err(e)) => {
            let why = format!("Proxmox VE's console didn't answer: {e}");
            let _ = socket.write_all(&ws::close_because(&why)).await;
            return Err(std::io::Error::other(why));
        }
        Err(_) => {
            let why = format!("Proxmox VE's console didn't answer within {} s", (CONNECT_LIMIT + ANSWER_LIMIT).as_secs());
            let _ = socket.write_all(&ws::close_because(&why)).await;
            return Err(std::io::Error::other(why));
        }
    };
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
mod tests;
