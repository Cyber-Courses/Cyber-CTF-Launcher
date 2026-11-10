//! Server self-test: prove a host can actually run a lab, not just that it answers.
//!
//! Like the machine VM self-test, but on the player's own server: it provisions a real
//! throwaway VM on the host, waits for it to boot and accept SSH, then destroys it.
//!
//! - `proxmox`: a one-machine Isoloom lab generated for the `proxmox` target and run by the
//!   Isothrum runner with the server's profile, exactly as a launch runs a lab (see `proxmox`).
//! - `esxi`: a minimal Vagrantfile booted with `vmware_esxi`, a command run over SSH, then
//!   `vagrant destroy`.
//!
//! Progress goes to the UI as one event per step (`running`, then `ok`/`fail`), as the machine
//! self-test's does (`selftest_report`).

mod esxi;
mod proxmox;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::platform::AppDirs;

use super::providers::Provider;
use super::selftest_report::{Event, Reporter};
use super::server;
use crate::error::{Error, Result};

/// Step labels shared by both hosts.
const PREPARE: &str = "Prepare a test VM definition";

/// One self-test per host at a time in this process: two runs would share the test's run on the
/// server and its folder, each tearing the other's down. A second one waits for the first.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn work_dir(dirs: &AppDirs, id: &str) -> Result<PathBuf> {
    Ok(dirs.data()?.join("selftest").join("server").join(id))
}

/// Runs a real-VM self-test against host `id`, streaming each step to `sink` (the
/// `server_selftest` command wraps the webview's channel in one).
pub(crate) async fn run(dirs: &AppDirs, id: &str, sink: impl Fn(Event) + Send + Sync + 'static) -> Result<()> {
    let _alone = ONE_AT_A_TIME.lock().await;
    let r = Reporter::new(sink);
    // `connect` resolves the host and its secret, which proves the profile and keychain are
    // usable before anything is created on the host.
    let conn = r
        .step("connect", "Reach the host", async {
            let c = server::connection(dirs, id)?;
            let name = c.name.clone();
            Ok((c, Some(name)))
        })
        .await?;

    let work = work_dir(dirs, id)?;
    // A folder still there is a previous run whose VM couldn't be removed: its state is the only
    // way to find that VM, so destroy it from there before anything else, and keep the folder
    // when that fails too.
    if work.exists() {
        let left = r
            .step("prepare", PREPARE, async {
                r.progress("prepare", PREPARE, "Removing the VM a previous test left on the host…");
                match conn.provider {
                    Provider::Proxmox => Box::pin(proxmox::leftover(dirs, id, &conn, &work)).await?,
                    Provider::VmwareEsxi => esxi::leftover(&r, &conn, &work),
                    _ => {}
                }
                Ok(((), None))
            })
            .await;
        if let Err(e) = left {
            return kept(&work, Err(e));
        }
        remove_work(&work);
    }

    let ran = match conn.provider {
        // Boxed: a whole lab's run on the server makes a large future.
        Provider::Proxmox => Box::pin(proxmox::run(dirs, id, &r, &conn, &work)).await,
        Provider::VmwareEsxi => esxi::run(&r, &conn, &work).await,
        Provider::Aws => Ran::nothing(Error::Invalid("the VM test is for server hosts; AWS labs are billed, so it isn't run as a test".into())),
        _ => Ran::nothing(Error::Invalid("the VM test runs only on a Proxmox or ESXi host".into())),
    };
    finish(&work, ran)
}

/// How a run ended: its result, and whether its VM is gone (removed, or never created).
pub(super) struct Ran {
    pub result: Result<()>,
    pub cleaned: bool,
}

impl Ran {
    fn nothing(e: Error) -> Self {
        Ran { result: Err(e), cleaned: true }
    }
}

/// The run's folder goes only with its VM: when the cleanup failed, the folder (its Terraform
/// state or Vagrant machine) is kept so the next run can remove the VM, and the result says so.
fn finish(work: &Path, ran: Ran) -> Result<()> {
    if ran.cleaned {
        remove_work(work);
        ran.result
    } else {
        kept(work, ran.result)
    }
}

/// Removes the run's folder, its VM gone. One that can't be removed is logged: the next run
/// takes it for a VM left behind and tries to remove that first.
fn remove_work(work: &Path) {
    if let Err(e) = std::fs::remove_dir_all(work)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("server self-test: couldn't remove {}: {e}", work.display());
    }
}

/// `result`, failed with a note that the test VM is still on the host and where its state is kept.
fn kept(work: &Path, result: Result<()>) -> Result<()> {
    let note = format!(
        "the test VM couldn't be removed from the host. Its state is kept in {}; the next test removes it first, or delete the VM on the host yourself",
        work.display()
    );
    Err(Error::Invalid(match result {
        Ok(()) => note,
        Err(e) => format!("{e}; {note}"),
    }))
}

/// Retries `probe` every 3 s until it succeeds or `within` has passed.
async fn retry_until<F: Future<Output = bool>>(within: Duration, mut probe: impl FnMut() -> F) -> bool {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        if probe().await {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

/// Blocks until `host:port` accepts a TCP connection, or the deadline passes.
async fn wait_tcp(host: &str, port: u16, within: Duration) -> Result<()> {
    let open =
        retry_until(within, || async { matches!(tokio::time::timeout(Duration::from_secs(4), tokio::net::TcpStream::connect((host, port))).await, Ok(Ok(_))) })
            .await;
    if open { Ok(()) } else { Err(Error::Invalid(format!("no answer on {host}:{port}"))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wait_tcp_sees_an_open_port_and_gives_up_on_a_closed_one() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let open = listener.local_addr().unwrap().port();
        assert!(wait_tcp("127.0.0.1", open, Duration::ZERO).await.is_ok());
        drop(listener);
        // Port 0 never accepts (a freed port could be taken by a parallel test).
        let err = wait_tcp("127.0.0.1", 0, Duration::ZERO).await.unwrap_err();
        assert_eq!(err.to_string(), "no answer on 127.0.0.1:0");
    }

    #[tokio::test]
    async fn retry_until_stops_at_the_first_success() {
        let tries = std::sync::atomic::AtomicU32::new(0);
        assert!(
            retry_until(Duration::from_secs(60), || {
                let n = tries.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move { n == 0 }
            })
            .await
        );
        assert_eq!(tries.load(std::sync::atomic::Ordering::SeqCst), 1);
        // Past the deadline, a failing probe is tried once and given up on.
        assert!(!retry_until(Duration::ZERO, || async { false }).await);
    }

    #[test]
    fn a_failed_cleanup_keeps_the_runs_state_and_says_so() {
        let work = std::env::temp_dir().join(format!("cyberctf-srv-selftest-{}", rand::random::<u32>()));
        std::fs::create_dir_all(work.join("state")).unwrap();
        std::fs::write(work.join("state").join("terraform.tfstate"), "{}").unwrap();

        // The VM is still on the host: its state must survive, and the result must say so even
        // when the test itself passed.
        let err = finish(&work, Ran { result: Ok(()), cleaned: false }).unwrap_err().to_string();
        assert!(work.join("state").join("terraform.tfstate").is_file());
        assert!(err.contains("couldn't be removed") && err.contains(&work.display().to_string()), "{err}");
        let err = finish(&work, Ran { result: Err(Error::Invalid("no address".into())), cleaned: false }).unwrap_err().to_string();
        assert!(err.starts_with("no address; the test VM couldn't be removed"), "{err}");
        assert!(work.exists());

        // Removed: the folder goes, and the run's own result comes back as is.
        assert!(finish(&work, Ran { result: Ok(()), cleaned: true }).is_ok());
        assert!(!work.exists());
        std::fs::create_dir_all(&work).unwrap();
        assert_eq!(finish(&work, Ran::nothing(Error::Invalid("boom".into()))).unwrap_err().to_string(), "boom");
        assert!(!work.exists());
    }
}
