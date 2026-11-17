//! Can the launcher reach a host: the per-provider "Test connection" dispatch, and the probes
//! for the player's own servers (TCP, then the SSH banner on ESXi or an API sign-in on Proxmox).

use isothrum_machine::place::{self, REACH_TIMEOUT, Unreachable, esxi};

use super::checks::TestResult;
use super::cloud_checks::{test_aws, test_azure, test_digitalocean, test_gcp, test_linode, test_oci};
use super::profile::HostProfile;
use crate::platform::AppDirs;
use crate::runtime::providers::Provider;

/// "Test connection" of host `h`, signing in with `password`; a Proxmox node is checked over SSH
/// with the launcher's key in the app's folders `dirs` (made when there is none).
pub(crate) async fn test_host(dirs: &AppDirs, h: &HostProfile, password: &str) -> TestResult {
    match h.provider {
        Provider::Aws => test_aws(h, password).await,
        Provider::Azure => test_azure(h).await,
        Provider::Gcp => test_gcp(h).await,
        Provider::DigitalOcean => test_digitalocean(password).await,
        Provider::Linode => test_linode(password).await,
        Provider::Oci => test_oci(h).await,
        _ => test_server(dirs, h, password).await,
    }
}

/// Why the server couldn't be reached, in the player's words.
fn unreachable(e: &Unreachable) -> TestResult {
    TestResult::unreachable(match e {
        Unreachable::TimedOut { host, port } => format!("Timed out reaching {host}:{port}"),
        Unreachable::Failed { host, port, why } => format!("Can't reach {host}:{port}: {why}"),
        other => format!("Can't reach the server: {other}"),
    })
}

/// A connection's time, as the row shows it.
fn ms(latency: std::time::Duration) -> u64 {
    u64::try_from(latency.as_millis()).unwrap_or(u64::MAX)
}

/// A server host: it must accept a TCP connection on its port (isothrum-machine's
/// [`place::reach`]), then answer as what labs use.
async fn test_server(dirs: &AppDirs, h: &HostProfile, password: &str) -> TestResult {
    // vagrant-vmware-esxi drives ESXi over SSH: check the port actually speaks SSH.
    if h.provider == Provider::VmwareEsxi {
        return match esxi::ssh(&h.host, h.port, REACH_TIMEOUT).await {
            Ok(esxi::Ssh::Answers { latency, .. }) => TestResult::reached(
                true,
                None,
                Some(ms(latency)),
                "SSH is up. Make sure SSH is enabled on the ESXi host; the password is checked on first lab start.",
            ),
            Ok(esxi::Ssh::NotSsh { latency }) => {
                TestResult::reached(false, None, Some(ms(latency)), format!("Port {} is open but doesn't answer as SSH. Enable SSH on the ESXi host.", h.port))
            }
            Ok(other) => TestResult::reached(false, None, None, format!("Port {} answered unexpectedly: {other:?}", h.port)),
            Err(e) => unreachable(&e),
        };
    }
    let latency_ms = match place::reach(&h.host, h.port, REACH_TIMEOUT).await {
        Ok(latency) => Some(ms(latency)),
        Err(e) => return unreachable(&e),
    };
    match h.provider {
        // Proxmox: sign in to the API (token or user + password), then check what a lab
        // launch needs (node, storage content, bridge, SSH key for token setups).
        Provider::Proxmox => {
            let key = match crate::runtime::ssh::ensure_key(dirs).await {
                Ok((key, _)) => key,
                Err(e) => return TestResult::reached(false, None, latency_ms, format!("The launcher's SSH key couldn't be made ({e}).")),
            };
            let r = crate::runtime::proxmox::test(h, password, &key).await;
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
        let t = crate::testing::test_app::TestApp::new();
        let h = HostProfile { host: "127.0.0.1".into(), port: 0, ..profile(Provider::VmwareEsxi) };
        let r = test_host(&t.dirs(), &h, "").await;
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
        let t = crate::testing::test_app::TestApp::new();
        let h = HostProfile { host: "127.0.0.1".into(), port, ..profile(Provider::VmwareEsxi) };
        let ssh = test_host(&t.dirs(), &h, "").await;
        assert!(ssh.ok && ssh.reachable && ssh.latency_ms.is_some());
        let http = test_host(&t.dirs(), &h, "").await;
        assert!(!http.ok && http.reachable);
        assert!(http.message.contains("doesn't answer as SSH"));
    }
}
