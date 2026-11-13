//! A lab machine's screen in its own window: its graphical console, which the runner reaches on
//! this computer's loopback (or through Proxmox's API), shown in the window's VNC viewer through
//! the window's own bridge ([`bridge`]), or handed to the system's Remote Desktop client
//! ([`rdp`]). One session per screen window, ended with it: its bridge stops, and a tunnel the
//! runner opened to the console closes.

pub(crate) mod bridge;
pub(crate) mod native;
pub(crate) mod rdp;
mod ws;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::runtime::Runtime;

/// The machine a screen window shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Which {
    /// The lab.
    pub(crate) id: String,
    /// The lab's runtime, as the page declares it.
    pub(crate) runtime: Runtime,
    /// The machine.
    pub(crate) machine: String,
}

/// A machine's console as the runner reached it, in the launcher's terms.
pub(crate) struct Reached {
    /// `vnc` or `rdp`.
    pub(crate) protocol: Protocol,
    /// Where it answers.
    pub(crate) at: bridge::Target,
    /// The account it signs in as (RDP).
    pub(crate) user: Option<String>,
    /// Its password: the VNC password, or the account's.
    pub(crate) password: Option<String>,
    /// What the run says of it.
    pub(crate) note: Option<String>,
    /// Whether an SSH tunnel carries it.
    pub(crate) tunnelled: bool,
    /// What keeps it reachable while the window lives (the runner's console, its tunnel).
    pub(crate) _keep: Option<Box<dyn Send>>,
}

/// A console's protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename = "ScreenProtocol")]
pub(crate) enum Protocol {
    /// VNC, shown in the window.
    Vnc,
    /// RDP, opened in the system's client.
    Rdp,
}

impl Reached {
    /// The runner's `console`, kept for the window's life.
    pub(crate) fn of(console: isothrum_runner::Console) -> Result<Reached> {
        let protocol = match crate::runtime::isothrum::protocol_word(console.protocol) {
            "vnc" => Protocol::Vnc,
            "rdp" => Protocol::Rdp,
            _ => return Err(Error::Invalid(format!("{} has no screen", console.machine))),
        };
        let at = match &console.endpoint {
            isothrum_runner::Endpoint::Loopback(at) => bridge::Target::Tcp(*at),
            isothrum_runner::Endpoint::ProxmoxVnc(vnc) => bridge::Target::WebSocket {
                url: vnc.websocket.expose().to_owned(),
                header: (vnc.authorization.0.clone(), vnc.authorization.1.expose().to_owned()),
                insecure: vnc.insecure,
            },
            _ => return Err(Error::Invalid("the runner reached the screen where this launcher can't show it".into())),
        };
        Ok(Reached {
            protocol,
            at,
            user: console.user.clone(),
            password: console.password.as_ref().map(|p| p.expose().to_owned()),
            note: console.note.clone(),
            tunnelled: console.tunnelled(),
            _keep: Some(Box::new(console)),
        })
    }
}

/// What the screen window is told of its machine's screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "ScreenConnection")]
pub(crate) struct Connection {
    /// `vnc` (shown in the window) or `rdp` (the system's client).
    pub(crate) protocol: Protocol,
    /// VNC: the bridge's URL the viewer connects to, once (its one-time token in it).
    pub(crate) url: Option<String>,
    /// VNC: the password the viewer answers the server with.
    pub(crate) password: Option<String>,
    /// Where the screen answers on this computer's loopback (the RDP client connects there; the
    /// VNC viewer's error names it); none for Proxmox's console, reached through its API.
    pub(crate) address: Option<String>,
    /// RDP: the account to sign in as.
    pub(crate) user: Option<String>,
    /// RDP: whether the launcher holds the account's password (to copy on request).
    pub(crate) has_password: bool,
    /// RDP: the system's client the launcher found, by name; none when it found none.
    pub(crate) client: Option<String>,
    /// What the run says of the screen (VirtualBox's extension pack).
    pub(crate) note: Option<String>,
    /// Whether an SSH tunnel carries it (it closes with the window).
    pub(crate) tunnelled: bool,
}

/// A screen window's session: its machine, its bridge, and once connected, what keeps its
/// console reachable and the `.rdp` file the client was given.
struct Session {
    which: Which,
    bridge: bridge::Bridge,
    reached: Option<Reached>,
    rdp_file: Option<PathBuf>,
}

/// The sessions, by window label.
static SESSIONS: Mutex<Option<HashMap<String, Session>>> = Mutex::new(None);

fn with_sessions<T>(f: impl FnOnce(&mut HashMap<String, Session>) -> T) -> T {
    let mut guard = SESSIONS.lock().unwrap_or_else(PoisonError::into_inner);
    f(guard.get_or_insert_with(HashMap::new))
}

/// A machine name a window label and a page's query take as it is.
fn plain(name: &str) -> bool {
    !name.is_empty() && name.len() <= 63 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Screen window `which`'s label and page (`screen-<lab>:<machine>`, under the screen
/// windows' capability, `screen-*`).
pub(crate) fn window(which: &Which) -> Result<(String, String)> {
    crate::runtime::validate_id(&which.id)?;
    if !plain(&which.machine) {
        return Err(Error::Invalid(format!("invalid machine name `{}`", which.machine)));
    }
    let page = format!("screen?id={}&runtime={}&machine={}", which.id, which.runtime.tag(), which.machine);
    Ok((format!("screen-{}:{}", which.id, which.machine), page))
}

/// The content security policy `csp` with the bridge's own address (`ws://127.0.0.1:<port>`)
/// added to `connect-src`: what the screen window's viewer may connect to, and nothing else.
pub(crate) fn csp_with_bridge(csp: &str, port: u16) -> String {
    let source = format!("ws://127.0.0.1:{port}");
    let mut found = false;
    let mut directives: Vec<String> = csp
        .split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| {
            if d.split_whitespace().next() == Some("connect-src") {
                found = true;
                format!("{d} {source}")
            } else {
                d.to_owned()
            }
        })
        .collect();
    if !found {
        directives.push(format!("connect-src 'self' {source}"));
    }
    directives.join("; ")
}

/// Starts screen window `label`'s session for `which`, its bridge bound (its port is what the
/// window's policy lets it connect to).
pub(crate) async fn start(label: &str, which: Which) -> Result<u16> {
    let bridge = bridge::Bridge::bind().await?;
    let port = bridge.port();
    with_sessions(|s| s.insert(label.to_owned(), Session { which, bridge, reached: None, rdp_file: None }));
    Ok(port)
}

/// Window `label`'s machine.
pub(crate) fn which(label: &str) -> Result<Which> {
    with_sessions(|s| s.get(label).map(|s| s.which.clone())).ok_or_else(|| Error::Invalid("this screen window has no session".into()))
}

/// Connects window `label` to its machine's console `reached`: VNC through its bridge (armed
/// with a fresh token, the window's URL), RDP with the client this computer has. The console is
/// kept until the window closes or connects again.
pub(crate) fn connect(label: &str, reached: Reached, client: Option<&rdp::Client>) -> Result<Connection> {
    with_sessions(|s| {
        let session = s.get_mut(label).ok_or_else(|| Error::Invalid("this screen window has no session".into()))?;
        let connection = match reached.protocol {
            Protocol::Vnc => {
                let token = session.bridge.arm(reached.at.clone());
                Connection {
                    protocol: Protocol::Vnc,
                    url: Some(format!("ws://127.0.0.1:{}/{token}", session.bridge.port())),
                    password: reached.password.clone(),
                    address: match &reached.at {
                        bridge::Target::Tcp(at) => Some(at.to_string()),
                        bridge::Target::WebSocket { .. } => None,
                    },
                    user: None,
                    has_password: false,
                    client: None,
                    note: reached.note.clone(),
                    tunnelled: reached.tunnelled,
                }
            }
            Protocol::Rdp => {
                let bridge::Target::Tcp(at) = &reached.at else {
                    return Err(Error::Invalid("a Remote Desktop screen answers on the loopback only".into()));
                };
                Connection {
                    protocol: Protocol::Rdp,
                    url: None,
                    password: None,
                    address: Some(at.to_string()),
                    user: reached.user.clone(),
                    has_password: reached.password.is_some(),
                    client: client.map(rdp::Client::name),
                    note: reached.note.clone(),
                    tunnelled: reached.tunnelled,
                }
            }
        };
        session.reached = Some(reached);
        Ok(connection)
    })
}

/// The RDP password of window `label`'s console, for the learner to paste (copied on request).
pub(crate) fn password(label: &str) -> Result<String> {
    with_sessions(|s| s.get(label).and_then(|s| s.reached.as_ref()).and_then(|r| r.password.clone()))
        .ok_or_else(|| Error::Invalid("this screen has no password to copy".into()))
}

/// Opens window `label`'s Remote Desktop in `client`: its `.rdp` file written owner-only in
/// `dir` (removed when the window closes), `FreeRDP` given the password on its input.
pub(crate) fn open_rdp(label: &str, client: &rdp::Client, dir: &std::path::Path) -> Result<()> {
    let (connection, password) = with_sessions(|s| {
        let r = s.get(label).and_then(|s| s.reached.as_ref()).filter(|r| r.protocol == Protocol::Rdp)?;
        let bridge::Target::Tcp(at) = &r.at else { return None };
        Some((rdp::Connection { address: at.to_string(), user: r.user.clone() }, r.password.clone()))
    })
    .ok_or_else(|| Error::Invalid("this screen isn't a Remote Desktop".into()))?;
    crate::platform::private_fs::dir(dir).map_err(Error::Io)?;
    let file = dir.join(format!("{}.rdp", label.replace([':', '/', '\\'], "-")));
    crate::platform::private_fs::write(&file, rdp::file(&connection)).map_err(Error::Io)?;
    with_sessions(|s| s.get_mut(label).map(|s| s.rdp_file = Some(file.clone())));
    let (program, args, stdin) = rdp::command(client, &file, &connection, password.is_some());
    let mut cmd = std::process::Command::new(&program);
    cmd.args(&args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    cmd.stdin(if stdin { std::process::Stdio::piped() } else { std::process::Stdio::null() });
    let mut child = cmd.spawn().map_err(|e| Error::Invalid(format!("couldn't open {}: {e}", client.name())))?;
    if let (true, Some(mut input), Some(p)) = (stdin, child.stdin.take(), password) {
        use std::io::Write as _;
        input.write_all(format!("{p}\n").as_bytes()).map_err(Error::Io)?;
    }
    Ok(())
}

/// Ends window `label`'s session: its bridge stops, its console's tunnel closes, its `.rdp`
/// file goes.
pub(crate) fn close(label: &str) {
    if let Some(session) = with_sessions(|s| s.remove(label))
        && let Some(file) = &session.rdp_file
        && let Err(e) = std::fs::remove_file(file)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("the screen's .rdp file wasn't removed: {e}");
    }
}

/// Ends every window's session, as the app quits: their bridges, tunnels and `.rdp` files go
/// with it (a quit left them, the review of 2026-11-13).
pub(crate) fn close_all() {
    let labels: Vec<String> = with_sessions(|s| s.keys().cloned().collect());
    for label in labels {
        close(&label);
    }
}

/// Whether window `label` has a session (for tests).
#[cfg(test)]
pub(crate) fn has_session(label: &str) -> bool {
    with_sessions(|s| s.contains_key(label))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn which(machine: &str) -> Which {
        Which { id: "goad".into(), runtime: Runtime::Vm, machine: machine.into() }
    }

    fn reached(protocol: Protocol, at: &str) -> Reached {
        Reached {
            protocol,
            at: bridge::Target::Tcp(at.parse().unwrap()),
            user: (protocol == Protocol::Rdp).then(|| "isoloom".to_owned()),
            password: Some("pw-1".into()),
            note: None,
            tunnelled: protocol == Protocol::Rdp,
            _keep: None,
        }
    }

    #[test]
    fn a_screen_window_is_named_by_lab_and_machine() {
        assert_eq!(window(&which("dc01")).unwrap(), ("screen-goad:dc01".into(), "screen?id=goad&runtime=VM&machine=dc01".into()));
        assert!(window(&which("a/b")).is_err());
        assert!(window(&which("x&machine=y")).is_err());
        assert!(window(&Which { id: "../x".into(), ..which("web") }).is_err());
    }

    #[test]
    fn the_window_may_connect_to_its_bridge_and_nowhere_else() {
        let csp = "default-src 'self'; connect-src ipc: http://ipc.localhost; img-src 'self'";
        assert_eq!(csp_with_bridge(csp, 4100), "default-src 'self'; connect-src ipc: http://ipc.localhost ws://127.0.0.1:4100; img-src 'self'");
        assert_eq!(csp_with_bridge("default-src 'self'", 4100), "default-src 'self'; connect-src 'self' ws://127.0.0.1:4100");
    }

    #[tokio::test]
    async fn a_vnc_screen_gets_a_one_time_url_and_an_rdp_one_its_address_and_client() {
        let label = "screen-goad:web";
        let port = start(label, which("web")).await.unwrap();
        let vnc = connect(label, reached(Protocol::Vnc, "127.0.0.1:5900"), None).unwrap();
        let url = vnc.url.unwrap();
        assert!(url.starts_with(&format!("ws://127.0.0.1:{port}/")) && url.len() > 30, "{url}");
        assert_eq!(vnc.password.as_deref(), Some("pw-1"));
        let again = connect(label, reached(Protocol::Vnc, "127.0.0.1:5900"), None).unwrap();
        assert_ne!(again.url.unwrap(), url, "a fresh token each time");
        let client = rdp::Client::FreeRdp("/usr/bin/xfreerdp".into());
        let rdp_ = connect(label, reached(Protocol::Rdp, "127.0.0.1:24001"), Some(&client)).unwrap();
        assert_eq!(
            (rdp_.address.as_deref(), rdp_.user.as_deref(), rdp_.has_password, rdp_.client.as_deref(), rdp_.url, rdp_.password),
            (Some("127.0.0.1:24001"), Some("isoloom"), true, Some("FreeRDP"), None, None),
            "the password stays in the launcher until copied"
        );
        assert_eq!(password(label).unwrap(), "pw-1");
        close(label);
        assert!(!has_session(label));
        assert!(connect(label, reached(Protocol::Vnc, "127.0.0.1:5900"), None).is_err(), "the session ended with the window");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_rdp_file_is_owner_only_and_goes_with_the_window() {
        use std::os::unix::fs::PermissionsExt as _;
        let label = "screen-goad:rdp01";
        start(label, which("rdp01")).await.unwrap();
        connect(label, reached(Protocol::Rdp, "127.0.0.1:24002"), None).unwrap();
        let dir = crate::testing::temp_dir("screen-rdp");
        // `true` stands in for the client: it opens nothing.
        open_rdp(label, &rdp::Client::Mstsc("true".into()), &dir).unwrap();
        let file = dir.join("screen-goad-rdp01.rdp");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("full address:s:127.0.0.1:24002") && !text.contains("pw-1"), "{text}");
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        close(label);
        assert!(!file.exists(), "removed with the window");
    }
}
