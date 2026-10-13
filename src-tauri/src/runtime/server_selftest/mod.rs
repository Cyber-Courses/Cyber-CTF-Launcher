//! Server self-test: prove a host can actually run a lab, not just that it answers.
//!
//! Like the machine VM self-test, but on the player's own server: it provisions a real
//! throwaway VM on the host, waits for it to boot and accept SSH, then destroys it.
//!
//! - `proxmox`: a minimal Terraform module (written at run time from `templates/`, so no lab is
//!   needed) clones a Debian cloud image, cloud-init installs the guest agent and the launcher's
//!   key, Terraform waits for the agent to report an address, then `destroy` removes it.
//! - `esxi`: a minimal Vagrantfile booted with `vmware_esxi`, a command run over SSH, then
//!   `vagrant destroy`.
//!
//! Progress goes to the UI as one event per step (`running`, then `ok`/`fail`), reusing the
//! machine self-test's `Event` so both share one frontend type.

mod esxi;
mod proxmox;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::AppHandle;
use tauri::Manager;
use tauri::ipc::Channel;

use super::providers::Provider;
use super::{server, ssh};
use crate::error::{Error, Result};
use crate::machine::selftest::Event;

/// A throwaway VM sized to boot quickly; the point is that it boots and networks, not that
/// it is roomy.
const CORES: &str = "1";
const MEMORY_MB: &str = "1536";
const DISK_GB: &str = "4";

/// Step labels shared by both hosts.
const PREPARE: &str = "Prepare a test VM definition";
const BOOT: &str = "Create and boot the VM on the host";
const CLEANUP: &str = "Destroy the test VM";

struct Reporter(Channel<Event>);

impl Reporter {
    fn send(&self, step: &'static str, label: &'static str, state: &'static str, detail: Option<String>) {
        let _ = self.0.send(Event { step, label, state, detail });
    }

    /// A line of a tool's output as the running step's detail (trimmed, at most 160 chars).
    fn progress(&self, step: &'static str, label: &'static str, line: String) {
        let line = line.trim();
        if !line.is_empty() {
            self.send(step, label, "running", Some(line.chars().take(160).collect()));
        }
    }

    /// Runs one step: `running`, then `ok` with the future's detail or `fail` with its error.
    async fn step<T>(&self, step: &'static str, label: &'static str, fut: impl Future<Output = Result<(T, Option<String>)>>) -> Result<T> {
        self.send(step, label, "running", None);
        match fut.await {
            Ok((v, detail)) => {
                self.send(step, label, "ok", detail);
                Ok(v)
            }
            Err(e) => {
                self.send(step, label, "fail", Some(e.to_string()));
                Err(e)
            }
        }
    }
}

fn work_dir(app: &AppHandle, id: &str) -> Result<PathBuf> {
    Ok(app.path().app_data_dir().map_err(|e| Error::Invalid(e.to_string()))?.join("selftest").join("server").join(id))
}

/// Runs a real-VM self-test against host `id`, streaming step events.
#[tauri::command]
pub async fn server_selftest(app: AppHandle, id: String, events: Channel<Event>) -> Result<()> {
    let r = Reporter(events);
    // `connect` resolves the host and its secret, which proves the profile and keychain are
    // usable before anything is created on the host.
    let conn = r
        .step("connect", "Reach the host", async {
            let c = server::connection(&app, &id)?;
            let name = c.name.clone();
            Ok((c, Some(name)))
        })
        .await?;

    let work = work_dir(&app, &id)?;
    // A folder still there is a previous run whose VM couldn't be removed: its state is the only
    // way to find that VM, so destroy it from there before anything else, and keep the folder
    // when that fails too.
    if work.exists() {
        let left = r
            .step("prepare", PREPARE, async {
                r.progress("prepare", PREPARE, "Removing the VM a previous test left on the host…".into());
                match conn.provider {
                    Provider::Proxmox => proxmox::leftover(&r, &conn, &work).await?,
                    Provider::VmwareEsxi => esxi::leftover(&r, &conn, &work).await?,
                    _ => {}
                }
                Ok(((), None))
            })
            .await;
        if let Err(e) = left {
            return kept(&work, Err(e));
        }
        let _ = std::fs::remove_dir_all(&work);
    }

    let ran = match conn.provider {
        Provider::Proxmox => proxmox::run(&app, &id, &r, &conn, &work).await,
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
        let _ = std::fs::remove_dir_all(work);
        ran.result
    } else {
        kept(work, ran.result)
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

/// `wait_tcp`, run on a jump host (`user@host`) over SSH with the launcher's key: for a VM only
/// that host can reach.
async fn wait_tcp_from(login: &str, identity: &Path, host: &str, port: u16, within: Duration) -> Result<()> {
    let (user, node) = login.split_once('@').ok_or_else(|| Error::Invalid("unexpected jump host".into()))?;
    let target = ssh::Target::direct(node, user, identity.to_path_buf());
    let known = identity.with_file_name("known_hosts");
    let probe = format!("timeout 4 bash -c {}", ssh::sh_quote(&format!("</dev/tcp/{host}/{port}")));
    if retry_until(within, || async { target.exec_timed(&known, &probe, Duration::from_secs(20)).await.is_ok() }).await {
        Ok(())
    } else {
        Err(Error::Invalid(format!("no answer on {host}:{port} (from {node})")))
    }
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

    use std::sync::{Arc, Mutex};

    /// A reporter whose events land in the returned list, as the JSON the UI would get.
    fn reporter() -> (Reporter, Arc<Mutex<Vec<serde_json::Value>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let channel = Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(json) = body {
                sink.lock().unwrap().push(serde_json::from_str(&json).unwrap());
            }
            Ok(())
        });
        (Reporter(channel), events)
    }

    #[tokio::test]
    async fn a_step_reports_running_then_its_outcome() {
        let (r, events) = reporter();
        let v = r.step("prepare", PREPARE, async { Ok((7, Some("done".to_string()))) }).await.unwrap();
        assert_eq!(v, 7);
        let err = r.step("up", BOOT, async { Err::<((), Option<String>), _>(Error::Invalid("no space".into())) }).await.unwrap_err();
        assert_eq!(err.to_string(), "no space");
        let events = events.lock().unwrap();
        let states: Vec<(&str, &str)> = events.iter().map(|e| (e["step"].as_str().unwrap(), e["state"].as_str().unwrap())).collect();
        assert_eq!(states, [("prepare", "running"), ("prepare", "ok"), ("up", "running"), ("up", "fail")]);
        assert_eq!(events[0]["label"], PREPARE);
        assert_eq!(events[1]["detail"], "done");
        assert_eq!(events[3]["detail"], "no space");
    }

    #[tokio::test]
    async fn progress_lines_are_trimmed_short_and_never_blank() {
        let (r, events) = reporter();
        r.progress("up", BOOT, "   ".into());
        r.progress("up", BOOT, format!("  {}  ", "x".repeat(300)));
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["state"], "running");
        assert_eq!(events[0]["detail"].as_str().unwrap().len(), 160);
    }

    #[tokio::test]
    async fn progress_lines_are_cut_by_characters_not_bytes() {
        // Terraform and Vagrant print accented and box-drawing text: a cut inside a character
        // would be a panic (or a replacement glyph), and 160 bytes would be far fewer letters.
        let (r, events) = reporter();
        r.progress("up", BOOT, "é".repeat(300));
        r.progress("up", BOOT, "✓ ".repeat(100));
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["detail"].as_str().unwrap().chars().count(), 160);
        assert!(events[0]["detail"].as_str().unwrap().chars().all(|c| c == 'é'));
        assert_eq!(events[1]["detail"].as_str().unwrap().chars().count(), 160);
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

    #[tokio::test]
    async fn wait_tcp_from_needs_a_login() {
        assert!(wait_tcp_from("pve.lan", Path::new("/k/id"), "10.0.0.5", 22, Duration::ZERO).await.is_err());
    }

    /// Each event's step and state, as the UI gets them.
    fn steps(events: &crate::test_app::Sink) -> Vec<(String, String)> {
        events.messages().iter().map(|e| (e["step"].as_str().unwrap().to_string(), e["state"].as_str().unwrap().to_string())).collect()
    }

    #[tokio::test]
    async fn the_vm_test_runs_on_proxmox_and_esxi_hosts_only() {
        let t = crate::test_app::TestApp::new();
        let tools = crate::exec::fake::Tools::rules(&[]);
        let app = || t.handle().clone();
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());

        // No such host: the first step fails and nothing else runs.
        let events = crate::test_app::Sink::default();
        assert!(server_selftest(app(), "nohost".into(), events.channel()).await.is_err());
        assert_eq!(steps(&events), [pair("connect", "running"), pair("connect", "fail")]);

        // A cloud account: reached, then refused.
        t.host("az1", "azure", serde_json::json!({}));
        let events = crate::test_app::Sink::default();
        let err = server_selftest(app(), "az1".into(), events.channel()).await.unwrap_err();
        assert_eq!(err.to_string(), "the VM test runs only on a Proxmox or ESXi host");
        assert_eq!(steps(&events), [pair("connect", "running"), pair("connect", "ok")]);
        assert_eq!(events.messages()[1]["detail"], "Host az1");

        // AWS is billed: never a test. A folder a previous run left is cleared first.
        t.host("aws1", "aws", serde_json::json!({ "useCliCreds": true }));
        let work = work_dir(t.handle(), "aws1").unwrap();
        std::fs::create_dir_all(&work).unwrap();
        let events = crate::test_app::Sink::default();
        let err = server_selftest(app(), "aws1".into(), events.channel()).await.unwrap_err();
        assert!(err.to_string().contains("AWS labs are billed"), "{err}");
        assert!(steps(&events).contains(&pair("prepare", "ok")), "{:?}", steps(&events));
        assert!(!work.exists());
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    }
}
