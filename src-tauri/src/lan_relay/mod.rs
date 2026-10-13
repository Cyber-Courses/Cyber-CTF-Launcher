//! macOS keeps the third-party tools the app starts off the local network (an app allowed under
//! Privacy & Security > Local Network still can't lend that to Vagrant's Ruby or ovftool: they
//! get "no route to host" to a LAN server the app itself reaches). Apple's own binaries are
//! exempt, and the app process is allowed. So for an ESXi host, Vagrant talks to loopback relays
//! this process runs (SSH and HTTPS to the host), ovftool gets the HTTPS relay's port through a
//! small wrapper (`ovftool`), Vagrant's WinRM to Windows guests goes through an in-app HTTP proxy
//! (`http_proxy`), and Vagrant's SSH to the guests goes through Apple's `/usr/bin/nc`.

mod http_proxy;
mod ovftool;

use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use self::http_proxy::start_http_proxy;
use self::ovftool::{real_ovftool, wrapper_dir};

/// A loopback port forwarding to a LAN endpoint while it lives.
pub struct Relay {
    pub port: u16,
    task: JoinHandle<()>,
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Listens on 127.0.0.1 (a free port) and pipes every connection to `host:port`.
pub async fn start(host: String, port: u16) -> std::io::Result<Relay> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let local = listener.local_addr()?.port();
    let task = tokio::spawn(async move {
        while let Ok((mut inbound, _)) = listener.accept().await {
            let target = (host.clone(), port);
            tokio::spawn(async move {
                if let Ok(mut outbound) = TcpStream::connect(target).await {
                    let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                }
            });
        }
    });
    Ok(Relay { port: local, task })
}

/// What a relayed run needs kept alive.
pub struct Relayed {
    pub env: Vec<(String, String)>,
    _relays: Vec<Relay>,
}

/// The last value of `k` in `env`.
fn get<'a>(env: &'a [(String, String)], k: &str) -> Option<&'a str> {
    env.iter().rev().find(|(key, _)| key == k).map(|(_, v)| v.as_str())
}

/// Sets `k` to `v`, replacing every earlier value.
fn set(env: &mut Vec<(String, String)>, k: &str, v: String) {
    env.retain(|(key, _)| key != k);
    env.push((k.to_string(), v));
}

/// The first of `keys` set in `env` (Isoloom's name before the launcher's).
fn first_of<'a>(env: &'a [(String, String)], keys: [&str; 2]) -> Option<&'a str> {
    keys.into_iter().find_map(|k| get(env, k))
}

const HOSTNAME_KEYS: [&str; 2] = ["ESXI_HOSTNAME", "CYBERCTF_ESXI_HOSTNAME"];
const HOSTPORT_KEYS: [&str; 2] = ["ESXI_HOSTPORT", "CYBERCTF_ESXI_HOSTPORT"];

/// For a Vagrant run against an ESXi host on macOS: relays up, the environment pointed at them.
/// Anything else (another tool or OS, no ESXi host in `env`) runs as it was.
pub async fn prepare(program: &str, env: &[(String, String)]) -> Relayed {
    let unchanged = || Relayed { env: env.to_vec(), _relays: Vec::new() };
    if !cfg!(target_os = "macos") || program != "vagrant" {
        return unchanged();
    }
    let Some(host) = first_of(env, HOSTNAME_KEYS).filter(|h| !h.is_empty() && *h != "127.0.0.1") else {
        return unchanged();
    };
    let ssh_port = first_of(env, HOSTPORT_KEYS).and_then(|p| p.parse().ok()).unwrap_or(22);
    let (Ok(ssh), Ok(https)) = (start(host.to_string(), ssh_port).await, start(host.to_string(), 443).await) else {
        return unchanged();
    };
    let mut out = env.to_vec();
    for k in HOSTNAME_KEYS {
        set(&mut out, k, "127.0.0.1".into());
    }
    for k in HOSTPORT_KEYS {
        set(&mut out, k, ssh.port.to_string());
    }
    set(&mut out, crate::runtime::lab::SSH_PROXY_COMMAND_ENV, "/usr/bin/nc %h %p".into());
    // Windows guests: Vagrant's WinRM (plain HTTP to the guest) through the in-app proxy. Only
    // the lowercase http_proxy: HTTPS (box downloads) doesn't read it.
    let winrm = start_http_proxy().await.ok();
    if let Some(p) = &winrm {
        set(&mut out, "http_proxy", format!("http://127.0.0.1:{}", p.port));
        set(&mut out, "no_proxy", "localhost,127.0.0.1".into());
    }
    if let (Some(real), Ok(dir)) = (real_ovftool(), wrapper_dir()) {
        set(&mut out, "CYBERCTF_REAL_OVFTOOL", real);
        set(&mut out, "CYBERCTF_OVFTOOL_PORT", https.port.to_string());
        let path = std::env::var("PATH").unwrap_or_default();
        set(&mut out, "PATH", format!("{}:{path}", dir.display()));
    }
    let mut relays = vec![ssh, https];
    relays.extend(winrm);
    Relayed { env: out, _relays: relays }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn relay_pipes_both_ways() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let echo = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let echo_port = echo.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = echo.accept().await.unwrap();
            let mut b = [0u8; 4];
            s.read_exact(&mut b).await.unwrap();
            s.write_all(&b).await.unwrap();
        });
        let relay = start("127.0.0.1".into(), echo_port).await.unwrap();
        let mut c = TcpStream::connect(("127.0.0.1", relay.port)).await.unwrap();
        c.write_all(b"ping").await.unwrap();
        let mut back = [0u8; 4];
        c.read_exact(&mut back).await.unwrap();
        assert_eq!(&back, b"ping");
    }

    #[tokio::test]
    async fn only_vagrant_with_an_esxi_host_is_relayed() {
        let env = vec![("ESXI_HOSTNAME".to_string(), "10.0.0.5".to_string()), ("ESXI_HOSTPORT".to_string(), "22".to_string())];
        assert_eq!(prepare("docker", &env).await.env, env);
        assert_eq!(prepare("vagrant", &[]).await.env, Vec::<(String, String)>::new());
        let r = prepare("vagrant", &env).await;
        if cfg!(target_os = "macos") {
            assert_eq!(get(&r.env, "ESXI_HOSTNAME"), Some("127.0.0.1"));
            assert_ne!(get(&r.env, "ESXI_HOSTPORT"), Some("22"));
            assert_eq!(get(&r.env, "ISOLOOM_SSH_PROXY_COMMAND"), Some("/usr/bin/nc %h %p"));
            assert!(get(&r.env, "http_proxy").is_some_and(|p| p.starts_with("http://127.0.0.1:")));
        } else {
            assert_eq!(r.env, env);
        }
    }

    #[test]
    fn env_helpers_take_the_last_value_and_prefer_isoloom_names() {
        let mut env = vec![("A".to_string(), "1".to_string()), ("A".to_string(), "2".to_string())];
        assert_eq!(get(&env, "A"), Some("2"));
        set(&mut env, "A", "3".into());
        assert_eq!(env, [("A".to_string(), "3".to_string())]);
        let both = vec![("CYBERCTF_ESXI_HOSTNAME".to_string(), "b".to_string()), ("ESXI_HOSTNAME".to_string(), "a".to_string())];
        assert_eq!(first_of(&both, HOSTNAME_KEYS), Some("a"));
        assert_eq!(first_of(&both[..1], HOSTNAME_KEYS), Some("b"));
        assert_eq!(first_of(&[], HOSTPORT_KEYS), None);
    }
}
