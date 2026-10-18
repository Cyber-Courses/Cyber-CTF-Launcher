//! Can the launcher reach a host: the per-provider "Test connection" dispatch, and the probes
//! for the player's own servers (TCP, then the SSH banner on ESXi or an API sign-in on Proxmox).

use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

use super::checks::{TestResult, elapsed_ms};
use super::cloud_checks::{test_aws, test_azure, test_digitalocean, test_gcp, test_linode, test_oci};
use super::profile::HostProfile;
use crate::runtime::providers::Provider;

const TEST_TIMEOUT: Duration = Duration::from_secs(6);

pub(super) async fn test_host(h: &HostProfile, password: &str) -> TestResult {
    match h.provider {
        Provider::Aws => test_aws(h, password).await,
        Provider::Azure => test_azure(h).await,
        Provider::Gcp => test_gcp(h).await,
        Provider::DigitalOcean => test_digitalocean(password).await,
        Provider::Linode => test_linode(password).await,
        Provider::Oci => test_oci(h).await,
        _ => test_server(h, password).await,
    }
}

/// A server host: it must accept a TCP connection on its port, then answer as what labs use.
async fn test_server(h: &HostProfile, password: &str) -> TestResult {
    let started = Instant::now();
    let mut stream = match tokio::time::timeout(TEST_TIMEOUT, TcpStream::connect((h.host.as_str(), h.port))).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return TestResult::unreachable(format!("Can't reach {}:{}: {e}", h.host, h.port)),
        Err(_) => return TestResult::unreachable(format!("Timed out reaching {}:{}", h.host, h.port)),
    };
    let latency_ms = Some(elapsed_ms(started));

    match h.provider {
        // vagrant-vmware-esxi drives ESXi over SSH: check the port actually speaks SSH.
        Provider::VmwareEsxi => {
            let mut buf = [0u8; 64];
            match tokio::time::timeout(TEST_TIMEOUT, stream.read(&mut buf)).await {
                Ok(Ok(n)) if buf[..n].starts_with(b"SSH-") => TestResult::reached(
                    true,
                    None,
                    latency_ms,
                    "SSH is up. Make sure SSH is enabled on the ESXi host; the password is checked on first lab start.",
                ),
                _ => TestResult::reached(false, None, latency_ms, format!("Port {} is open but doesn't answer as SSH. Enable SSH on the ESXi host.", h.port)),
            }
        }
        // Proxmox: sign in to the API (token or user + password), then check what a lab
        // launch needs (node, storage content, bridge, SSH key for token setups).
        Provider::Proxmox => {
            drop(stream);
            let r = crate::runtime::proxmox::test(h, password).await;
            TestResult::reached(r.ok, r.authenticated, latency_ms, r.message)
        }
        _ => TestResult::reached(true, None, latency_ms, "Reachable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::server::profile::tests::profile;

    #[tokio::test]
    async fn closed_ports_are_unreachable() {
        // Nothing can listen on port 0, so the connect fails at once (a freed port could be
        // taken by a parallel test).
        let h = HostProfile { host: "127.0.0.1".into(), port: 0, ..profile(Provider::VmwareEsxi) };
        let r = test_host(&h, "").await;
        assert!(!r.ok && !r.reachable);
        assert!(r.message.starts_with("Can't reach 127.0.0.1:0"), "{}", r.message);
    }

    #[tokio::test]
    async fn esxi_needs_an_ssh_banner() {
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            for banner in [&b"SSH-2.0-OpenSSH_9.6\r\n"[..], b"HTTP/1.1 400 Bad Request\r\n\r\n"] {
                let (mut s, _) = listener.accept().await.unwrap();
                s.write_all(banner).await.unwrap();
            }
        });
        let h = HostProfile { host: "127.0.0.1".into(), port, ..profile(Provider::VmwareEsxi) };
        let ssh = test_host(&h, "").await;
        assert!(ssh.ok && ssh.reachable && ssh.latency_ms.is_some());
        let http = test_host(&h, "").await;
        assert!(!http.ok && http.reachable);
        assert!(http.message.contains("doesn't answer as SSH"));
    }
}
