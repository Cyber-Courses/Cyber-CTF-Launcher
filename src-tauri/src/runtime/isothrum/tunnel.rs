//! The ways through a Proxmox node to a lab whose bridge is internal to it (a NAT bridge only the
//! node reaches), each an `ssh` through the node with the runner's key, which the node lets in for
//! the jump, its host key checked against the ones the player confirmed for it
//! ([`ssh::node_known_hosts`]):
//!
//! - [`Tunnel`]: a SOCKS5 proxy for Terraform, which signs in to the lab's router to wait for each
//!   machine's set-up and to seal its networks. The module's `ssh_proxy_host` and
//!   `ssh_proxy_port` name it, and Terraform's SSH goes to the proxy first, then to the router.
//!   Open for one operation that applies the module, and ends when it drops.
//! - [`Forward`]: a local port forwarded to one `host:port` the lab publishes (the router's uplink
//!   and the published port): the lab's link while it runs ([`super::reach`] keeps them).

use std::path::Path;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::platform::exec::{Background, background};
use crate::runtime::ssh;

/// Where the proxy and the forwards listen: this computer only.
pub(crate) const LOOPBACK: &str = "127.0.0.1";

/// How long the node may take to let an ssh in (ssh's own connect timeout is 10s).
const READY_WITHIN: Duration = Duration::from_secs(30);

/// How often the port of an ssh opening is tried.
const POLL: Duration = Duration::from_millis(200);

/// A SOCKS5 proxy on this computer through a node, until dropped (its ssh is then ended).
pub(crate) struct Tunnel {
    /// The proxy's port on [`LOOPBACK`].
    port: u16,
    /// The ssh holding it open.
    _ssh: Background,
}

impl Tunnel {
    /// Opens the proxy through node login `jump` (`root@<node>`), signed in with `identity` (the
    /// runner's key); ready once its port takes connections.
    pub(crate) async fn open(jump: &str, identity: &Path) -> Result<Tunnel> {
        Tunnel::open_within(jump, identity, READY_WITHIN).await
    }

    /// [`Tunnel::open`], given up after `within`.
    async fn open_within(jump: &str, identity: &Path, within: Duration) -> Result<Tunnel> {
        let node = ssh::Target::login(jump, identity.to_path_buf())?;
        Tunnel::open_on(&node, jump, identity, free_port()?, within).await
    }

    /// [`Tunnel::open_within`] on `port` of [`LOOPBACK`]: a test's, held until its ssh listens.
    async fn open_on(node: &ssh::Target, jump: &str, identity: &Path, port: u16, within: Duration) -> Result<Tunnel> {
        let args = node.socks_args(&ssh::node_known_hosts(identity), port)?;
        let what = format!("the SSH tunnel through the Proxmox node {jump} to the lab's network");
        let ssh = listening(&args, port, within, &what).await?;
        Ok(Tunnel { port, _ssh: ssh })
    }

    /// The module's variables that name the proxy.
    pub(crate) fn values(&self) -> [(String, String); 2] {
        [("ssh_proxy_host".to_owned(), LOOPBACK.to_owned()), ("ssh_proxy_port".to_owned(), self.port.to_string())]
    }
}

/// A port on this computer forwarded through a node to one `host:port` behind it, until dropped
/// (its ssh is then ended).
pub(crate) struct Forward {
    /// Its port on [`LOOPBACK`].
    pub(crate) port: u16,
    /// The ssh holding it open.
    ssh: Background,
}

impl Forward {
    /// Opens a forward through node login `jump`, signed in with `identity` (the runner's key), to
    /// `to` (`host`, `port` as the node reaches it), on `port` when it is free (the one it had
    /// before), else on a free one; ready once its port takes connections.
    pub(crate) async fn open(jump: &str, identity: &Path, to: (&str, u16), port: Option<u16>) -> Result<Forward> {
        let node = ssh::Target::login(jump, identity.to_path_buf())?;
        Forward::open_on(&node, jump, identity, to, pick_port(port)?).await
    }

    /// [`Forward::open`] on `port` of [`LOOPBACK`]: a test's, held until its ssh listens.
    async fn open_on(node: &ssh::Target, jump: &str, identity: &Path, (host, to): (&str, u16), port: u16) -> Result<Forward> {
        let args = node.forward_args(&ssh::node_known_hosts(identity), port, (host, to))?;
        let what = format!("the forward through the Proxmox node {jump} to the lab's {host}:{to}");
        let ssh = listening(&args, port, READY_WITHIN, &what).await?;
        Ok(Forward { port, ssh })
    }

    /// Whether its ssh still runs.
    pub(crate) async fn runs(&mut self) -> bool {
        self.ssh.stopped_within(Duration::ZERO).await.is_none()
    }
}

/// Starts `ssh args`, and waits up to `within` for it to listen on `port`: `what` (lowercase) then
/// fails, saying why, when ssh ends first or the port doesn't open in time.
async fn listening(args: &[String], port: u16, within: Duration, what: &str) -> Result<Background> {
    let mut ssh = background("ssh", &args.iter().map(String::as_str).collect::<Vec<_>>())?;
    let deadline = tokio::time::Instant::now() + within;
    loop {
        if tokio::net::TcpStream::connect((LOOPBACK, port)).await.is_ok() {
            return Ok(ssh);
        }
        if let Some(e) = ssh.stopped_within(POLL).await {
            return Err(Error::Invalid(format!("Couldn't open {what}: {e}")));
        }
        if tokio::time::Instant::now() >= deadline {
            let mut said = what.to_owned();
            if let Some(first) = said.get_mut(..1) {
                first.make_ascii_uppercase();
            }
            return Err(Error::Invalid(format!("{said} didn't open within {}s", within.as_secs())));
        }
    }
}

/// `wanted` when it is free now, else a free port of [`LOOPBACK`].
fn pick_port(wanted: Option<u16>) -> Result<u16> {
    match wanted.filter(|p| std::net::TcpListener::bind((LOOPBACK, *p)).is_ok()) {
        Some(port) => Ok(port),
        None => free_port(),
    }
}

/// A port of [`LOOPBACK`] free now. Another program could take it before ssh does: ssh then ends
/// (`ExitOnForwardFailure`), and the tunnel says so rather than running as no proxy.
fn free_port() -> Result<u16> {
    Ok(std::net::TcpListener::bind((LOOPBACK, 0))?.local_addr()?.port())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;

    use tokio::net::{TcpListener, TcpSocket};

    use super::*;
    use crate::platform::exec::fake::{Reply, Tools};

    /// The runner's key, as the app data folder holds it.
    fn key() -> PathBuf {
        PathBuf::from("/data/ssh/id_ed25519")
    }

    /// A port of [`LOOPBACK`] the test holds: bound, so nothing else on this computer can take it,
    /// and not listening, so nothing connects until the scripted ssh listens on it. A port picked
    /// free and let go (`free_port`) was taken by another test's listener in between, under load:
    /// "address already in use", or a tunnel taken as open that no ssh opened.
    fn held_port() -> (TcpSocket, u16) {
        let socket = TcpSocket::new_v4().unwrap();
        socket.bind((std::net::Ipv4Addr::LOCALHOST, 0).into()).unwrap();
        let port = socket.local_addr().unwrap().port();
        (socket, port)
    }

    /// The tunnel through `root@node.lab` on the test's `port`, given `within` to open.
    async fn open(port: u16, within: Duration) -> Result<Tunnel> {
        let node = ssh::Target::login("root@node.lab", key()).unwrap();
        Tunnel::open_on(&node, "root@node.lab", &key(), port, within).await
    }

    /// Scripted tools whose `ssh -N -D 127.0.0.1:<port>` answers `reply`, first listening on the
    /// held `socket` when given (as ssh does once the node let it in); the listener is kept with
    /// the tools.
    fn ssh_answering(reply: Option<Reply>, socket: Option<(TcpSocket, u16)>) -> Tools {
        let socket = RefCell::new(socket);
        let kept: RefCell<Vec<TcpListener>> = RefCell::default();
        Tools::new(move |program, args| {
            assert_eq!((program, &args[..2]), ("ssh", &["-N", "-D"][..]), "only the tunnel's ssh runs");
            if let Some((socket, port)) = socket.borrow_mut().take() {
                assert_eq!(args[2], format!("127.0.0.1:{port}"), "ssh listens on the port the tunnel was given");
                kept.borrow_mut().push(socket.listen(8).unwrap());
            }
            reply.clone()
        })
    }

    #[tokio::test]
    async fn the_tunnel_is_a_socks_proxy_through_the_node_with_the_runners_key_and_the_nodes_known_hosts() {
        let (socket, held) = held_port();
        let tools = ssh_answering(Some(Reply::Hang), Some((socket, held)));
        let tunnel = open(held, READY_WITHIN).await.unwrap();
        let [(host_var, host), (port_var, port)] = tunnel.values();
        assert_eq!((host_var.as_str(), host.as_str(), port_var.as_str()), ("ssh_proxy_host", "127.0.0.1", "ssh_proxy_port"));
        assert_eq!(port, held.to_string());
        let made = tools.made("ssh -N -D");
        assert_eq!(made.len(), 1);
        let args = &made[0];
        assert_eq!(args[2], format!("127.0.0.1:{port}"));
        let line = args.join(" ");
        assert!(line.contains("-i /data/ssh/id_ed25519"), "{line}");
        let known = ssh::ssh_config_path(&ssh::node_known_hosts(&key()).to_string_lossy());
        assert!(line.contains(&format!("UserKnownHostsFile={known}")), "{line}");
        assert!(line.contains("StrictHostKeyChecking=yes") && line.contains("BatchMode=yes"), "{line}");
        assert!(line.contains("-o ExitOnForwardFailure=yes"), "{line}");
        assert_eq!(args.last().map(String::as_str), Some("root@node.lab"));
    }

    #[tokio::test]
    async fn a_node_that_refuses_the_tunnel_says_why() {
        let (_socket, port) = held_port();
        let _tools = ssh_answering(Some(Reply::Fail("root@node.lab: Permission denied (publickey).".into())), None);
        let why = open(port, READY_WITHIN).await.err().unwrap().to_string();
        assert!(why.contains("Permission denied (publickey)") && why.contains("root@node.lab"), "{why}");
    }

    #[tokio::test]
    async fn a_tunnel_that_never_opens_is_given_up_on() {
        let (_socket, port) = held_port();
        let _tools = ssh_answering(Some(Reply::Hang), None);
        let why = open(port, Duration::from_millis(500)).await.err().unwrap().to_string();
        assert!(why.contains("didn't open"), "{why}");
    }

    #[tokio::test]
    async fn an_odd_node_login_is_refused_before_ssh_runs() {
        let tools = Tools::script().build();
        for bad in ["node.lab", "root@-oProxyCommand=x", "root@a b"] {
            assert!(Tunnel::open(bad, &key()).await.is_err(), "{bad}");
        }
        assert!(tools.calls().is_empty(), "no ssh ran: {:?}", tools.calls());
    }

    #[tokio::test]
    async fn a_forward_goes_through_the_node_to_one_published_address() {
        let (socket, held) = held_port();
        let socket = RefCell::new(Some(socket));
        let kept: RefCell<Vec<TcpListener>> = RefCell::default();
        let tools = Tools::new(move |program, args| {
            assert_eq!((program, &args[..2]), ("ssh", &["-N", "-L"][..]), "only the forward's ssh runs");
            if let Some(socket) = socket.borrow_mut().take() {
                kept.borrow_mut().push(socket.listen(8).unwrap());
            }
            Some(Reply::Hang)
        });
        let node = ssh::Target::login("root@node.lab", key()).unwrap();
        let mut forward = Forward::open_on(&node, "root@node.lab", &key(), ("192.0.2.10", 8080), held).await.unwrap();
        assert_eq!(forward.port, held);
        assert!(forward.runs().await);
        let made = tools.made("ssh -N -L");
        assert_eq!(made[0][2], format!("127.0.0.1:{held}:192.0.2.10:8080"));
        let line = made[0].join(" ");
        assert!(line.contains("-i /data/ssh/id_ed25519") && line.contains("ExitOnForwardFailure=yes"), "{line}");
        assert_eq!(made[0].last().map(String::as_str), Some("root@node.lab"));
    }

    #[test]
    fn a_forward_keeps_its_port_when_free_else_takes_another() {
        let (_socket, held) = held_port();
        assert_ne!(pick_port(Some(held)).unwrap(), held, "taken: another");
        let free = free_port().unwrap();
        assert_eq!(pick_port(Some(free)).unwrap(), free);
    }

    #[tokio::test]
    async fn a_forward_to_an_odd_address_is_refused_before_ssh_runs() {
        let tools = Tools::script().build();
        for bad in [("-oProxyCommand=x", 80), ("a b", 80), ("::1", 80)] {
            assert!(Forward::open("root@node.lab", &key(), bad, None).await.is_err(), "{bad:?}");
        }
        assert_eq!(tools.calls().len(), 0, "no ssh ran");
    }
}
