//! A tiny loopback control server for the co-location probe (the agent, `account::agent`, starts
//! one per website launch that runs here). When a lab is launched, the website must
//! decide whether this launcher is on the same machine as the browser before it trusts a
//! `127.0.0.1` lab URL. It does that by fetching `GET /health?token=<one-time token>` here
//! and checking the returned nonce. We bind loopback-only, require the token and a loopback
//! `Host` (a DNS-rebinding page names its own host), echo an allowed Origin so only the site can
//! read the response, and answer the nonce once: the token is spent by the first answer, and the
//! server ends then, when the lab stops, or after its time ([`SERVED`]), whichever comes first.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Notify;

use crate::error::{Error, Result};
use crate::platform::loopback_http::{ct_eq, header};

/// Site origins allowed to read the probe. Loopback origins are allowed too, in dev builds only
/// (the site's dev server): in a release any local web page could otherwise read it.
const ALLOWED_ORIGINS: [&str; 6] = [
    "https://www.cybercourses.com",
    "https://cybercourses.com",
    "https://www.cyberctf.org",
    "https://cyberctf.org",
    "https://www.cyberctf.com",
    "https://cyberctf.com",
];

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

/// How long a probe server waits, given by whoever starts one ([`serve`]'s are [`SERVED`]).
#[derive(Debug, Clone, Copy)]
struct Limits {
    /// How long a connection gets to send its request, and to take the answer: a client that
    /// connects and stays silent must not hold a task (and a socket) for the life of the app.
    io: Duration,
    /// How long the server waits for the website before it ends unused.
    ttl: Duration,
}

/// The app's probe servers' limits: the website asks right after the lab is reported running,
/// so the wait is generous.
const SERVED: Limits = Limits { io: Duration::from_secs(5), ttl: Duration::from_secs(10 * 60) };

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

/// Answers one connection's request, each way within `io`.
async fn answer(socket: &mut tokio::net::TcpStream, probe: &Probe, io: Duration) {
    let mut buf = [0u8; 2048];
    let Ok(Ok(n)) = tokio::time::timeout(io, socket.read(&mut buf)).await else { return };
    let req = String::from_utf8_lossy(&buf[..n]);
    let resp = probe.reply_to(&req);
    let _ = tokio::time::timeout(io, async {
        let _ = socket.write_all(resp.as_bytes()).await;
        let _ = socket.shutdown().await;
    })
    .await;
}

/// Accepts connections for `probe` until its token is spent or `limits`' time passes.
async fn accept_until_spent(listener: TcpListener, probe: Arc<Probe>, limits: Limits) {
    let accepting = async {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                // A persistent accept error (e.g. fd exhaustion) must not become a hot spin loop.
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            };
            let probe = probe.clone();
            tokio::spawn(async move { answer(&mut socket, &probe, limits.io).await });
        }
    };
    tokio::select! {
        () = accepting => {}
        () = probe.spent.notified() => {}
        () = tokio::time::sleep(limits.ttl) => {}
    }
}

/// The co-location server of each lab's latest launch. Labs launched at once each keep their
/// own (one launch never ends another lab's check); a lab's new launch retires its previous one,
/// and a server that ended (spent, or past its time, [`SERVED`]) is dropped at the next launch, so
/// listeners (and their ports/tokens) don't accumulate for the life of the app.
static SERVERS: Mutex<BTreeMap<String, tokio::task::JoinHandle<()>>> = Mutex::new(BTreeMap::new());

fn servers() -> std::sync::MutexGuard<'static, BTreeMap<String, tokio::task::JoinHandle<()>>> {
    SERVERS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Starts the control server for lab `lab` on a free loopback port and returns the port. That
/// lab's previous launch's server is aborted; other labs' are left running.
pub async fn serve(lab: &str, token: String, nonce: String) -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(Error::Io)?;
    let port = listener.local_addr().map_err(Error::Io)?.port();
    let probe = Arc::new(Probe::new(token, nonce, port));
    let handle = tokio::spawn(accept_until_spent(listener, probe, SERVED));
    let mut servers = servers();
    servers.retain(|_, server| !server.is_finished());
    if let Some(old) = servers.insert(lab.to_string(), handle) {
        old.abort();
    }
    Ok(port)
}

/// Ends lab `lab`'s co-location server, if it still runs: the lab is stopping.
pub fn retire(lab: &str) {
    if let Some(server) = servers().remove(lab) {
        server.abort();
    }
}

#[cfg(test)]
mod tests;
