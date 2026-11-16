//! The way into a Proxmox lab whose bridge is internal to its node (a NAT bridge only the node
//! reaches): Terraform signs in to the lab's router to wait for each machine's set-up and to seal
//! its networks, and the router's uplink address is out of this computer's reach. A SOCKS5 proxy
//! through the node (`ssh -N -D`, with the runner's key, which the node lets in for the jump) is
//! the module's way in: its `ssh_proxy_host` and `ssh_proxy_port` name it, and Terraform's SSH goes
//! to the proxy first, then to the router.
//!
//! A [`Tunnel`] is open for one operation that applies the module, and ends when it drops.

use std::path::Path;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::platform::exec::{Background, background};
use crate::runtime::ssh;

/// Where the proxy listens: this computer only.
const LOOPBACK: &str = "127.0.0.1";

/// How long the node may take to let the tunnel in (ssh's own connect timeout is 10s).
const READY_WITHIN: Duration = Duration::from_secs(30);

/// How often the proxy's port is tried while it opens.
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
    /// runner's key), the node's host key checked against the ones the player confirmed for it
    /// ([`ssh::node_known_hosts`]); ready once its port takes connections.
    pub(crate) async fn open(jump: &str, identity: &Path) -> Result<Tunnel> {
        Tunnel::open_within(jump, identity, READY_WITHIN).await
    }

    /// [`Tunnel::open`], given up after `within`.
    async fn open_within(jump: &str, identity: &Path, within: Duration) -> Result<Tunnel> {
        let node = ssh::Target::login(jump, identity.to_path_buf())?;
        let port = free_port()?;
        let args = node.socks_args(&ssh::node_known_hosts(identity), port)?;
        let mut ssh = background("ssh", &args.iter().map(String::as_str).collect::<Vec<_>>())?;
        let deadline = tokio::time::Instant::now() + within;
        loop {
            if tokio::net::TcpStream::connect((LOOPBACK, port)).await.is_ok() {
                return Ok(Tunnel { port, _ssh: ssh });
            }
            if let Some(e) = ssh.stopped_within(POLL).await {
                return Err(Error::Invalid(format!("Couldn't open the SSH tunnel through the Proxmox node {jump} to the lab's network: {e}")));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Invalid(format!(
                    "The SSH tunnel through the Proxmox node {jump} to the lab's network didn't open within {}s",
                    within.as_secs()
                )));
            }
        }
    }

    /// The module's variables that name the proxy.
    pub(crate) fn values(&self) -> [(String, String); 2] {
        [("ssh_proxy_host".to_owned(), LOOPBACK.to_owned()), ("ssh_proxy_port".to_owned(), self.port.to_string())]
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
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::rc::Rc;

    use super::*;
    use crate::platform::exec::fake::{Reply, Tools};

    /// The runner's key, as the app data folder holds it.
    fn key() -> PathBuf {
        PathBuf::from("/data/ssh/id_ed25519")
    }

    /// Scripted tools whose `ssh -N -D 127.0.0.1:<port>` answers `reply`, listening on that port
    /// first when `listens` (as ssh does once the node let it in); the listeners are kept with
    /// the tools.
    fn ssh_answering(reply: Option<Reply>, listens: bool) -> Tools {
        let kept: Rc<RefCell<Vec<TcpListener>>> = Rc::default();
        Tools::new(move |program, args| {
            assert_eq!((program, &args[..2]), ("ssh", &["-N", "-D"][..]), "only the tunnel's ssh runs");
            if listens {
                kept.borrow_mut().push(TcpListener::bind(args[2]).unwrap());
            }
            reply.clone()
        })
    }

    #[tokio::test]
    async fn the_tunnel_is_a_socks_proxy_through_the_node_with_the_runners_key_and_the_nodes_known_hosts() {
        let tools = ssh_answering(Some(Reply::Hang), true);
        let tunnel = Tunnel::open("root@node.lab", &key()).await.unwrap();
        let [(host_var, host), (port_var, port)] = tunnel.values();
        assert_eq!((host_var.as_str(), host.as_str(), port_var.as_str()), ("ssh_proxy_host", "127.0.0.1", "ssh_proxy_port"));
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
        let _tools = ssh_answering(Some(Reply::Fail("root@node.lab: Permission denied (publickey).".into())), false);
        let why = Tunnel::open("root@node.lab", &key()).await.err().unwrap().to_string();
        assert!(why.contains("Permission denied (publickey)") && why.contains("root@node.lab"), "{why}");
    }

    #[tokio::test]
    async fn a_tunnel_that_never_opens_is_given_up_on() {
        let _tools = ssh_answering(Some(Reply::Hang), false);
        let why = Tunnel::open_within("root@node.lab", &key(), Duration::from_millis(500)).await.err().unwrap().to_string();
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
}
