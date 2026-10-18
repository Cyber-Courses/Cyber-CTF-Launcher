//! Setup self-tests: prove this machine can actually run labs, not just that the tools exist.
//!
//! - `docker`: a throwaway two-container lab (busybox, ~2 MB). It checks the engine answers,
//!   an image pulls, compose starts the lab, containers reach each other by name on the lab
//!   network, and a published port is reachable from this machine. Always cleaned up.
//! - `vm`: a real throwaway VM on the first usable local hypervisor. It checks the
//!   hypervisor answers, gets a small test box (once, then cached), boots it on a private
//!   lab network, runs a command inside it, pings it from this machine, then destroys it.
//!
//! Progress goes to the UI as one event per step: `running`, then `ok` or `fail`.

mod docker;
mod downloads;
mod vm;

use std::path::PathBuf;

use serde::Serialize;

use crate::error::Result;
use crate::machine::storage::owned;
use crate::platform::AppDirs;
use crate::runtime::providers::Provider;

use docker::IMAGE;
use vm::candidate_boxes;

#[derive(Clone, Copy, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename = "SelfTestKind")]
pub enum Kind {
    Docker,
    Vm,
}

#[derive(Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SelfTestEvent")]
pub struct Event {
    pub step: &'static str,
    pub label: &'static str,
    /// `running`, `ok`, `fail` or `skip`.
    #[ts(type = r#""running" | "ok" | "fail" | "skip""#)]
    pub state: &'static str,
    pub detail: Option<String>,
}

/// Longest live output line shown under a running step, in characters.
const PROGRESS_MAX: usize = 160;

/// Where a test's events go: the page that runs it.
struct Reporter(Box<dyn Fn(Event) + Send + Sync>);

impl Reporter {
    /// Live output for a step that is still running (shown as its detail line).
    fn progress(&self, step: &'static str, label: &'static str, line: &str) {
        let line = line.trim();
        if !line.is_empty() {
            self.send(step, label, "running", Some(line.chars().take(PROGRESS_MAX).collect()));
        }
    }

    fn send(&self, step: &'static str, label: &'static str, state: &'static str, detail: Option<String>) {
        (self.0)(Event { step, label, state, detail });
    }

    /// Runs one step, reporting it; a failure stops the test with that step's error.
    async fn step<T>(&self, step: &'static str, label: &'static str, fut: impl std::future::Future<Output = Result<(T, Option<String>)>>) -> Result<T> {
        self.send(step, label, "running", None);
        let outcome = fut.await;
        let (state, detail) = match &outcome {
            Ok((_, detail)) => ("ok", detail.clone()),
            Err(e) => ("fail", Some(explain(&e.to_string()))),
        };
        self.send(step, label, state, detail);
        outcome.map(|(v, _)| v)
    }

    /// The closing clean-up step, reported only when the test itself passed (a failed test
    /// already says what went wrong).
    fn cleanup<T>(&self, label: &'static str, test: &Result<()>, done: Result<T>, ok_detail: Option<String>) {
        match (test, done) {
            (Ok(()), Ok(_)) => self.send("cleanup", label, "ok", ok_detail),
            (Ok(()), Err(e)) => self.send("cleanup", label, "fail", Some(e.to_string())),
            (Err(_), _) => {}
        }
    }
}

/// A plain-language hint for the failures people actually hit, in front of the raw error (kept
/// as the detail). Unknown errors pass through unchanged.
fn explain(raw: &str) -> String {
    let hint = if raw.contains("type=kvm") || raw.contains("preferred machine") {
        "KVM isn't usable by this user. Check that /dev/kvm exists and that you are in the kvm and libvirt groups, then run the test again."
    } else if raw.contains("Cannot connect to the Docker daemon") || raw.contains("docker.sock") {
        "The container engine isn't reachable. Start Docker and check that your user can use it (the docker group), then run the test again."
    } else {
        return raw.to_string();
    };
    format!("{hint} Details: {raw}")
}

pub(crate) fn work_dir(dirs: &AppDirs, kind: &str) -> Result<PathBuf> {
    let dir = dirs.cache()?.join("selftest").join(kind);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Whether this machine's CPU is ARM (the test boxes differ).
fn arm() -> bool {
    std::env::consts::ARCH == "aarch64"
}

/// Runs `op`, a self-test of `kind`, recording what it downloaded as the app's (the test image,
/// or the test box of whichever provider it used), for Storage to clean up.
async fn tracked<T>(dirs: &AppDirs, kind: Kind, op: impl std::future::Future<Output = T>) -> T {
    match kind {
        Kind::Docker => owned::tracked(dirs.clone(), &[IMAGE.to_string()], &[], op).await,
        Kind::Vm => {
            let boxes: Vec<String> = vm::TEST_PROVIDERS.iter().flat_map(|p| candidate_boxes(*p, arm()).iter().map(ToString::to_string)).collect();
            owned::tracked(dirs.clone(), &[], &boxes, op).await
        }
    }
}

/// Starts the test downloads early (called when setup opens), recording what they download as
/// the app's. Errors are ignored by the UI: the test itself retries and reports them.
pub async fn machine_selftest_prefetch(dirs: AppDirs, kind: Kind, provider: Option<Provider>) -> Result<()> {
    match kind {
        Kind::Docker => owned::tracked(dirs, &[IMAGE.to_string()], &[], downloads::ensure_image()).await.map(|_| ()),
        Kind::Vm => {
            let Some(p) = vm::test_provider(provider).await else { return Ok(()) };
            let (bx, cached) = vm::pick_box(p, arm()).await;
            if cached {
                return Ok(());
            }
            let dir = work_dir(&dirs, "vm")?;
            owned::tracked(dirs, &[], &[bx.to_string()], downloads::ensure_box(&dir, p, bx, arm(), |_| {})).await.map(|_| ())
        }
    }
}

/// One self-test of a kind at a time in this app: the Machine window and the onboarding both run
/// them, and two runs share the test's Compose project, folder and VM, so each tore the other's
/// down. A second run waits for the first.
fn one_at_a_time(kind: Kind) -> &'static tokio::sync::Mutex<()> {
    static DOCKER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    static VM: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    match kind {
        Kind::Docker => &DOCKER,
        Kind::Vm => &VM,
    }
}

/// Runs the self-test of `kind` (on `provider`, for a VM), each step's event sent to `events`.
pub async fn machine_selftest(dirs: AppDirs, kind: Kind, provider: Option<Provider>, events: impl Fn(Event) + Send + Sync + 'static) -> Result<()> {
    let _alone = one_at_a_time(kind).lock().await;
    let r = Reporter(Box::new(events));
    // Boxed: the steps of a test VM's run make a large future.
    Box::pin(tracked(&dirs, kind, async {
        match kind {
            Kind::Docker => docker::run_test(&work_dir(&dirs, "docker")?, &r).await,
            Kind::Vm => vm::run_test(&work_dir(&dirs, "vm")?, provider, &r).await,
        }
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::testing::test_app::Sink;

    /// A reporter whose events land in a sink.
    fn reporter() -> (Reporter, Sink) {
        let (events, sink) = events();
        (Reporter(Box::new(events)), sink)
    }

    fn states(seen: &Sink) -> Vec<String> {
        seen.messages().iter().map(|e| format!("{}:{}", e["step"].as_str().unwrap(), e["state"].as_str().unwrap())).collect()
    }

    #[tokio::test]
    async fn a_step_reports_running_then_ok_or_fail() {
        let (r, seen) = reporter();
        let v = r.step("one", "First", async { Ok((7, Some("fine".into()))) }).await.unwrap();
        assert_eq!(v, 7);
        let err = r.step::<()>("two", "Second", async { Err(Error::Invalid("Cannot connect to the Docker daemon".into())) }).await.unwrap_err();
        assert!(err.to_string().contains("Docker daemon"));
        assert_eq!(states(&seen), ["one:running", "one:ok", "two:running", "two:fail"]);
        let events = seen.messages();
        assert_eq!(events[1]["detail"], "fine");
        assert!(events[3]["detail"].as_str().unwrap().starts_with("The container engine isn't reachable"));
    }

    #[test]
    fn progress_lines_are_trimmed_and_short_and_cleanup_reports_only_after_a_pass() {
        let (r, seen) = reporter();
        r.progress("box", "Box", "   ");
        r.progress("box", "Box", &format!("  {}  ", "x".repeat(500)));
        r.cleanup::<()>("Clean", &Ok(()), Ok(()), Some("kept".into()));
        r.cleanup::<()>("Clean", &Ok(()), Err(Error::Invalid("left over".into())), None);
        r.cleanup::<()>("Clean", &Err(Error::Invalid("x".into())), Ok(()), None);
        assert_eq!(states(&seen), ["box:running", "cleanup:ok", "cleanup:fail"]);
        let events = seen.messages();
        assert_eq!(events[0]["detail"].as_str().unwrap().chars().count(), PROGRESS_MAX);
        assert_eq!(events[2]["detail"], "left over");
        let _ = arm();
    }

    #[test]
    fn kvm_failure_gets_a_hint_and_keeps_the_details() {
        let raw = "could not get preferred machine for /usr/bin/qemu-system-x86_64 type=kvm";
        let out = explain(raw);
        assert!(out.starts_with("KVM isn't usable"));
        assert!(out.ends_with(raw));
    }

    /// Where events go to land in a sink, as the page's channel would take them.
    fn events() -> (impl Fn(Event) + Send + Sync + 'static, Sink) {
        let sink = Sink::default();
        let channel = sink.channel();
        (
            move |e| {
                let _ = channel.send(e);
            },
            sink,
        )
    }

    #[tokio::test]
    async fn the_container_test_runs_every_step_then_cleans_up() {
        use crate::platform::exec::fake::{Tools, ok};
        let t = crate::testing::test_app::TestApp::new();
        // The lab's published port: a loopback server serving the test page.
        let web =
            crate::platform::http::tests::serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\ncyberctf-selftest-ok".to_vec()).await;
        let addr = web.trim_start_matches("http://").trim_end_matches('/').to_string();
        let tools = Tools::new(move |_, args| match args {
            ["info", ..] => ok("27.3.1\n"),
            [.., "exec", "-T", "probe", "wget", _, _, _, _] => ok("cyberctf-selftest-ok\n"),
            [.., "port", "web", "80"] => ok(format!("{addr}\n")),
            _ => ok(""),
        });
        let (channel, seen) = events();
        machine_selftest(t.dirs(), Kind::Docker, None, channel).await.unwrap();
        let done: Vec<String> = states(&seen).into_iter().filter(|s| !s.ends_with(":running")).collect();
        assert_eq!(done, ["engine:ok", "pull:ok", "start:ok", "network:ok", "port:ok", "cleanup:ok"]);
        assert!(tools.called("down -v --remove-orphans"));
        machine_selftest_prefetch(t.dirs(), Kind::Docker, None).await.unwrap();
    }

    #[tokio::test]
    async fn a_second_test_of_a_kind_waits_for_the_first() {
        // Regression: the Machine window's and the onboarding's runs shared the test project and
        // tore each other's test lab down.
        use crate::platform::exec::fake::Tools;
        let t = crate::testing::test_app::TestApp::new();
        let tools = Tools::rules(&[]);
        let first = one_at_a_time(Kind::Docker).lock().await;
        let (channel, seen) = events();
        let second = tokio::time::timeout(std::time::Duration::from_millis(200), machine_selftest(t.dirs(), Kind::Docker, None, channel)).await;
        assert!(second.is_err(), "it waits");
        assert!(tools.calls().is_empty() && seen.messages().is_empty());
        drop(first);
    }

    #[tokio::test]
    async fn a_failed_container_test_stops_at_its_step_and_still_cleans_up() {
        use crate::platform::exec::fake::{Tools, fail};
        let t = crate::testing::test_app::TestApp::new();
        let tools = Tools::rules(&[("docker info", fail("Cannot connect to the Docker daemon at unix:///var/run/docker.sock"))]);
        let (channel, seen) = events();
        assert!(machine_selftest(t.dirs(), Kind::Docker, None, channel).await.is_err());
        assert_eq!(states(&seen), ["engine:running", "engine:fail"]);
        assert!(seen.messages()[1]["detail"].as_str().unwrap().starts_with("The container engine isn't reachable"));
        assert!(tools.called("down -v"), "the test lab is torn down even so");
    }

    #[tokio::test]
    async fn the_vm_test_boots_pings_and_deletes_a_vm() {
        use crate::platform::exec::fake::{Tools, ok};
        let t = crate::testing::test_app::TestApp::new();
        let boxes = "generic/alpine319 (virtualbox, 4.3.12)\nbento/debian-12 (virtualbox, 1)\ngeneric/alpine319 (libvirt, 4.3.12)\nbento/debian-12 (parallels, 1)\nbento/debian-12 (vmware_desktop, 1)\ngeneric/alpine319 (qemu, 1)\ngeneric/alpine319 (hyperv, 1)\nutm/bookworm (utm, 1)\n";
        let tools = Tools::new(move |program, args| match (program, args) {
            ("vagrant", ["plugin", "list"]) => {
                ok("vagrant-qemu (0.3.6)\nvagrant-parallels (2.4)\nvagrant-vmware-desktop (3.0)\nvagrant-libvirt (0.12)\nvagrant_utm (0.1)\n")
            }
            ("vagrant", ["box", "list"]) => ok(boxes),
            ("vagrant", ["--version"]) => ok("Vagrant 2.4.3\n"),
            ("vagrant", ["ssh", "-c", _]) => ok("cyberctf-uname:Linux 6.6 x86_64\n"),
            _ => ok(""),
        });
        let (channel, seen) = events();
        let r = machine_selftest(t.dirs(), Kind::Vm, None, channel).await;
        let done: Vec<String> = states(&seen).into_iter().filter(|s| !s.ends_with(":running")).collect();
        assert_eq!(&done[..2], ["vagrant:ok", "provider:ok"], "{done:?}");
        r.unwrap();
        assert_eq!(&done[2..6], ["hypervisor:ok", "box:ok", "boot:ok", "exec:ok"]);
        assert!(done[6] == "network:ok" || done[6] == "network:skip");
        assert_eq!(done.last().map(String::as_str), Some("cleanup:ok"));
        assert!(tools.called("vagrant up --provider") && tools.called("vagrant destroy -f"));
        // The box is there already: nothing to prefetch.
        machine_selftest_prefetch(t.dirs(), Kind::Vm, None).await.unwrap();
        assert!(!tools.called("box add"));
    }

    #[tokio::test]
    async fn without_vagrant_the_vm_test_stops_at_once() {
        let t = crate::testing::test_app::TestApp::new();
        let _tools = crate::platform::exec::fake::Tools::rules(&[("vagrant", None)]);
        let (channel, seen) = events();
        assert!(machine_selftest(t.dirs(), Kind::Vm, None, channel).await.is_err());
        assert_eq!(states(&seen), ["vagrant:running", "vagrant:fail"]);
    }

    #[test]
    fn unknown_errors_pass_through_unchanged() {
        assert_eq!(explain("something else broke"), "something else broke");
    }
}
