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

use crate::error::Result;
use crate::machine::storage::owned;
use crate::platform::AppDirs;
use crate::runtime::providers::Provider;
use crate::runtime::selftest_report::{Event, Reporter};

use docker::IMAGE;
use vm::candidate_boxes;

/// Which self-test to run.
#[derive(Clone, Copy, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename = "SelfTestKind")]
pub enum Kind {
    /// The container test: a throwaway two-container lab on the engine.
    Docker,
    /// The VM test: a throwaway VM on a local hypervisor.
    Vm,
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

/// The self-test's own folder in the app's cache (`selftest/<kind>`), created when missing.
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
            let boxes: Vec<String> = vm::test_providers().flat_map(|p| candidate_boxes(p, arm()).iter().map(ToString::to_string)).collect();
            owned::tracked(dirs.clone(), &[], &boxes, op).await
        }
    }
}

/// Starts the test downloads early (called when setup opens), recording what they download as
/// the app's. Errors are ignored by the UI: the test itself retries and reports them.
pub async fn machine_selftest_prefetch(dirs: AppDirs, kind: Kind, provider: Option<Provider>) -> Result<()> {
    match kind {
        Kind::Docker => owned::tracked(dirs, &[IMAGE.to_string()], &[], downloads::ensure_image(&[])).await.map(|_| ()),
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

/// Runs the self-test of `kind`: on `provider`, for a VM; on the engine behind the Docker CLI
/// `context`, for containers (the CLI's current one when none is named). Each step's event is
/// sent to `events`.
pub async fn machine_selftest(
    dirs: AppDirs,
    kind: Kind,
    provider: Option<Provider>,
    context: Option<String>,
    events: impl Fn(Event) + Send + Sync + 'static,
) -> Result<()> {
    let _alone = one_at_a_time(kind).lock().await;
    let r = Reporter::explaining(events, explain);
    // Boxed: the steps of a test VM's run make a large future.
    Box::pin(tracked(&dirs, kind, async {
        match kind {
            Kind::Docker => docker::run_test(&work_dir(&dirs, "docker")?, context.as_deref(), &r).await,
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

    fn states(seen: &Sink) -> Vec<String> {
        seen.messages().iter().map(|e| format!("{}:{}", e["step"].as_str().unwrap(), e["state"].as_str().unwrap())).collect()
    }

    /// This machine's reporter (its failures explained), whose events land in a sink.
    fn reporter() -> (Reporter, Sink) {
        let (events, sink) = events();
        (Reporter::explaining(events, explain), sink)
    }

    #[tokio::test]
    async fn a_failed_step_says_the_hint_for_a_failure_people_hit() {
        let (r, seen) = reporter();
        let err = r.step::<()>("two", "Second", async { Err(Error::Invalid("Cannot connect to the Docker daemon".into())) }).await.unwrap_err();
        assert!(err.to_string().contains("Docker daemon"));
        assert!(seen.messages()[1]["detail"].as_str().unwrap().starts_with("The container engine isn't reachable"));
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
        machine_selftest(t.dirs(), Kind::Docker, None, None, channel).await.unwrap();
        let done: Vec<String> = states(&seen).into_iter().filter(|s| !s.ends_with(":running")).collect();
        assert_eq!(done, ["engine:ok", "pull:ok", "start:ok", "network:ok", "port:ok", "cleanup:ok"]);
        assert!(tools.called("down -v --remove-orphans"));
        // The CLI's current engine: no context forced on the calls.
        assert_eq!(tools.env_of("compose", "DOCKER_CONTEXT"), None);
        machine_selftest_prefetch(t.dirs(), Kind::Docker, None).await.unwrap();
    }

    /// The Machine page tests one engine among several: every Docker call of the run, the
    /// probe, the pull, compose and the clean-up, goes to that engine's context.
    #[tokio::test]
    async fn the_container_test_runs_on_the_engine_asked_for() {
        use crate::platform::exec::fake::{Tools, fail, ok};
        let t = crate::testing::test_app::TestApp::new();
        let tools = Tools::rules(&[("docker info", ok("27.3.1\n")), ("image inspect", fail("No such image")), ("pull", fail("timed out"))]);
        let (channel, seen) = events();
        assert!(machine_selftest(t.dirs(), Kind::Docker, None, Some("orbstack".into()), channel).await.is_err());
        assert_eq!(states(&seen), ["engine:running", "engine:ok", "pull:running", "pull:fail"]);
        // (`image inspect busybox`, the test's own: the storage bookkeeping after the run inspects
        // the image on the CLI's engine, as Storage reads it.)
        for call in ["docker info", "image inspect busybox", "pull -q", "down -v"] {
            assert_eq!(tools.env_of(call, "DOCKER_CONTEXT").as_deref(), Some("orbstack"), "{call}: {:?}", tools.calls());
        }
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
        let second = tokio::time::timeout(std::time::Duration::from_millis(200), machine_selftest(t.dirs(), Kind::Docker, None, None, channel)).await;
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
        assert!(machine_selftest(t.dirs(), Kind::Docker, None, None, channel).await.is_err());
        assert_eq!(states(&seen), ["engine:running", "engine:fail"]);
        assert!(seen.messages()[1]["detail"].as_str().unwrap().starts_with("The container engine isn't reachable"));
        assert!(tools.called("down -v"), "the test lab is torn down even so");
    }

    /// Regression (the rig's Debian 13, `docker.io` 26.1.5): Docker 26 and older exit 0
    /// from `docker info --format …` with an empty line when the daemon doesn't answer, and the
    /// engine step passed ("Docker ") to fail at the pull. It fails with the reason `docker
    /// version` gives, asked of the same engine.
    #[tokio::test]
    async fn an_engine_whose_docker_info_answers_nothing_fails_the_engine_step() {
        use crate::platform::exec::fake::{Tools, fail, ok};
        let t = crate::testing::test_app::TestApp::new();
        let tools = Tools::new(|_, args| match args {
            ["info", ..] => ok("\n"),
            ["version", ..] => fail("Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?"),
            _ => ok(""),
        });
        let (channel, seen) = events();
        assert!(machine_selftest(t.dirs(), Kind::Docker, None, Some("default".into()), channel).await.is_err());
        assert_eq!(states(&seen), ["engine:running", "engine:fail"]);
        assert!(seen.messages()[1]["detail"].as_str().unwrap().starts_with("The container engine isn't reachable"));
        assert_eq!(tools.env_of("docker version", "DOCKER_CONTEXT").as_deref(), Some("default"));
        assert!(!tools.called("pull"), "{:?}", tools.calls());
    }

    #[tokio::test]
    async fn the_vm_test_boots_pings_and_deletes_a_vm() {
        use crate::platform::exec::fake::{Tools, ok};
        // The hypervisors are the fakes': this host's /dev/kvm and VirtualBox driver don't count
        // (a Linux CI runner has neither usable).
        crate::runtime::providers::assume_devices_ready();
        let t = crate::testing::test_app::TestApp::new();
        let boxes = "generic/alpine319 (virtualbox, 4.3.12)\nbento/debian-12 (virtualbox, 1)\ngeneric/alpine319 (libvirt, 4.3.12)\nbento/debian-12 (parallels, 1)\nbento/debian-12 (vmware_desktop, 1)\ngeneric/alpine319 (qemu, 1)\ngeneric/alpine319 (hyperv, 1)\nutm/bookworm (utm, 1)\n";
        let tools = Tools::new(move |program, args| match (program, args) {
            ("vagrant", ["plugin", "list"]) => {
                ok("vagrant-qemu (0.3.6)\nvagrant-parallels (2.4)\nvagrant-vmware-desktop (3.0)\nvagrant-libvirt (0.12)\nvagrant_utm (0.2.0)\n")
            }
            ("vagrant", ["box", "list"]) => ok(boxes),
            ("vagrant", ["--version"]) => ok("Vagrant 2.4.3\n"),
            ("vagrant", ["ssh", "-c", _]) => ok("cyberctf-uname:Linux 6.6 x86_64\n"),
            _ => ok(""),
        });
        let (channel, seen) = events();
        let r = machine_selftest(t.dirs(), Kind::Vm, None, None, channel).await;
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
        assert!(machine_selftest(t.dirs(), Kind::Vm, None, None, channel).await.is_err());
        assert_eq!(states(&seen), ["vagrant:running", "vagrant:fail"]);
    }

    #[test]
    fn unknown_errors_pass_through_unchanged() {
        assert_eq!(explain("something else broke"), "something else broke");
    }
}
