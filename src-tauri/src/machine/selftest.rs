//! Setup self-tests: prove this machine can actually run labs, not just that the tools exist.
//!
//! - `docker`: isothrum-machine's engine self-test (`selftest::engine`), a throwaway
//!   two-container deployment (busybox, ~2 MB) through a runner of its own: the engine answers,
//!   the image is here, the deployment starts, its containers reach each other by name and a
//!   published port answers on this machine. Always cleaned up.
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
use crate::platform::host::Arch;
use crate::runtime::providers::Provider;
use crate::runtime::selftest_report::{Event, Reporter};

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

/// Runs `op`, the VM self-test, recording what it downloaded as the app's (the test box of
/// whichever provider it used), for Storage to clean up.
async fn tracked<T>(dirs: &AppDirs, op: impl std::future::Future<Output = T>) -> T {
    let boxes: Vec<String> = vm::test_providers().flat_map(|p| candidate_boxes(p, Arch::HERE).iter().map(ToString::to_string)).collect();
    owned::tracked(dirs.clone(), &[], &boxes, op).await
}

/// Adds the test image a self-test pulled (`pulled`) to the app's runner record, for Storage to
/// clean up. Best effort: a failure is logged, and only means it isn't cleaned up later.
async fn keep(dirs: &AppDirs, pulled: Option<isothrum_runner::DownloadToAdopt>) {
    let Some(pulled) = pulled else { return };
    let added = match crate::runtime::paths::runner_home(dirs) {
        Ok(home) => crate::runtime::isothrum::adopt_downloads(&home, vec![pulled]).await.map(drop),
        Err(e) => Err(e),
    };
    if let Err(e) = added {
        log::warn!("the self-test's image couldn't be added to the runner's record: {e}");
    }
}

/// Starts the test downloads early (called when setup opens), recording what they download as
/// the app's. Errors are ignored by the UI: the test itself retries and reports them.
pub async fn machine_selftest_prefetch(dirs: AppDirs, kind: Kind, provider: Option<Provider>) -> Result<()> {
    match kind {
        Kind::Docker => docker::prefetch(&dirs).await,
        Kind::Vm => {
            let Some(p) = vm::test_provider(provider).await else { return Ok(()) };
            let (bx, cached) = vm::pick_box(p, Arch::HERE).await;
            if cached {
                return Ok(());
            }
            let dir = work_dir(&dirs, "vm")?;
            owned::tracked(dirs, &[], &[bx.to_string()], downloads::ensure_box(&dir, p, bx, Arch::HERE, |_| {})).await.map(|_| ())
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
    match kind {
        Kind::Docker => Box::pin(docker::run_test(&dirs, &work_dir(&dirs, "docker")?, context.as_deref(), &r)).await,
        Kind::Vm => Box::pin(tracked(&dirs, async { vm::run_test(&work_dir(&dirs, "vm")?, provider, &r).await })).await,
    }
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
        let _ = Arch::HERE;
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

    /// A container engine as isothrum-machine's engine test asks it: answering (or not), the
    /// test image held, its deployment up with the web server's port published at `port`.
    fn engine(answers: bool, port: u16) -> isothrum_runner::fake::FakeProcesses {
        use isothrum_runner::fake::{FakeProcesses, Reply};
        FakeProcesses::model(move |call| {
            let line = call.args.join(" ");
            if line.contains("info --format") {
                return if answers { Reply::ok("27.3.1") } else { Reply::fail(1, "Cannot connect to the Docker daemon at unix:///var/run/docker.sock") };
            }
            if line.contains("image inspect") {
                Reply::ok("sha256:busybox")
            } else if line.contains("compose version") {
                Reply::ok("2.29.7")
            } else if line.contains("config --format json") {
                Reply::ok(r#"{"services": {"web": {"image": "busybox:1.36"}, "probe": {"image": "busybox:1.36"}}}"#)
            } else if line.contains(" ps ") {
                let published = format!(r#"[{{"URL":"127.0.0.1","TargetPort":80,"PublishedPort":{port},"Protocol":"tcp"}}]"#);
                let probe = r#"{"Service":"probe","State":"running","Health":"","ExitCode":0,"Publishers":[],"Labels":""}"#;
                Reply::ok(&format!(r#"{{"Service":"web","State":"running","Health":"","ExitCode":0,"Publishers":{published},"Labels":""}}{}{probe}"#, "\n"))
            } else if line.contains("exec") {
                Reply::ok("isothrum-selftest-ok")
            } else {
                Reply::ok("")
            }
        })
    }

    /// The container test on [`engine`], its events.
    async fn engine_test(answers: bool) -> (Result<()>, Sink) {
        let t = crate::testing::test_app::TestApp::new();
        let web = crate::platform::http::tests::serve_once(b"HTTP/1.0 200 OK\r\nConnection: close\r\n\r\nisothrum-selftest-ok".to_vec()).await;
        let port = web.trim_start_matches("http://127.0.0.1:").trim_end_matches('/').parse().unwrap();
        let runner: std::sync::Arc<dyn isothrum_runner::ProcessRunner> = std::sync::Arc::new(engine(answers, port));
        let host = crate::runtime::providers::testing::host(std::path::Path::new("/nonexistent/cyberctf-test-machine"));
        let _machine = crate::runtime::providers::testing::Machine::set(runner, host);
        let (channel, seen) = events();
        (machine_selftest(t.dirs(), Kind::Docker, None, None, channel).await, seen)
    }

    #[tokio::test]
    async fn the_container_test_says_each_step_in_the_apps_words_then_cleans_up() {
        let (ran, seen) = engine_test(true).await;
        ran.unwrap();
        let done: Vec<(String, String)> = seen
            .messages()
            .iter()
            .filter(|e| e["state"] != "running")
            .map(|e| {
                (
                    format!("{}:{}:{}", e["step"].as_str().unwrap(), e["state"].as_str().unwrap(), e["label"].as_str().unwrap()),
                    e["detail"].as_str().unwrap_or("").to_owned(),
                )
            })
            .collect();
        let said: Vec<(&str, &str)> = done.iter().map(|(s, d)| (s.as_str(), d.as_str())).collect();
        assert_eq!(
            &said[..4],
            [
                ("engine:ok:Container engine answers", "Docker 27.3.1"),
                ("pull:ok:Download a test image", "busybox:1.36 (ready)"),
                ("start:ok:Start a two-container test lab", ""),
                ("network:ok:Containers reach each other", "probe → web on the lab network"),
            ]
        );
        assert!(said[4].0 == "port:ok:Lab port reachable from this machine" && said[4].1.starts_with("127.0.0.1:"), "{said:?}");
        assert_eq!(said[5], ("cleanup:ok:Clean up", ""));
    }

    #[tokio::test]
    async fn a_failed_container_test_says_the_hint_and_stops_at_its_step() {
        let (ran, seen) = engine_test(false).await;
        assert!(ran.is_err());
        assert_eq!(states(&seen), ["engine:running", "engine:fail"]);
        assert!(seen.messages()[1]["detail"].as_str().unwrap().starts_with("The container engine isn't reachable"));
    }

    #[tokio::test]
    async fn a_second_test_of_a_kind_waits_for_the_first() {
        // Regression: the Machine window's and the onboarding's runs shared the test project and
        // tore each other's test lab down.
        use crate::platform::exec::fake::Tools;
        let t = crate::testing::test_app::TestApp::new();
        let tools = Tools::strict_rules(&[]);
        let first = one_at_a_time(Kind::Docker).lock().await;
        let (channel, seen) = events();
        let second = tokio::time::timeout(std::time::Duration::from_millis(200), machine_selftest(t.dirs(), Kind::Docker, None, None, channel)).await;
        assert!(second.is_err(), "it waits");
        assert!(tools.calls().is_empty() && seen.messages().is_empty());
        drop(first);
    }

    #[tokio::test]
    async fn the_vm_test_boots_pings_and_deletes_a_vm() {
        use crate::platform::exec::fake::{Tools, ok};
        // The hypervisors are the fakes', every one answering, none needing a plugin listed: this
        // host's /dev/kvm and VirtualBox driver don't count (a Linux CI runner has neither usable).
        let machine = crate::testing::temp_dir("vm-test-machine");
        let tools = isothrum_runner::fake::FakeProcesses::model(|_| isothrum_runner::fake::Reply::ok(""));
        let _machine = crate::runtime::providers::testing::Machine::set(std::sync::Arc::new(tools), crate::runtime::providers::testing::host(&machine));
        let t = crate::testing::test_app::TestApp::new();
        let boxes = "generic/alpine319 (virtualbox, 4.3.12)\nbento/debian-12 (virtualbox, 1)\ngeneric/alpine319 (libvirt, 4.3.12)\nbento/debian-12 (parallels, 1)\nbento/debian-12 (vmware_desktop, 1)\ngeneric/alpine319 (qemu, 1)\ngeneric/alpine319 (hyperv, 1)\nutm/bookworm (utm, 1)\n";
        let tools = Tools::new(move |program, args| match (program, args) {
            ("vagrant", ["plugin", "list"]) => {
                ok("vagrant-qemu (0.3.6)\nvagrant-parallels (2.4)\nvagrant-vmware-desktop (3.0)\nvagrant-libvirt (0.12)\nvagrant_utm (0.2.0)\n")
            }
            ("vagrant", ["box", "list"]) => ok(boxes),
            ("vagrant", ["--version"]) => ok("Vagrant 2.4.3\n"),
            ("vagrant", ["ssh", "-c", _]) => ok("cyberctf-uname:Linux 6.6 x86_64\n"),
            // Every address answers, as a ping's reply line says (an exit 0 alone isn't a reply).
            ("ping", _) => ok("64 bytes from 192.168.56.250: icmp_seq=1 ttl=64 time=0.412 ms\n"),
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
        let _tools = crate::platform::exec::fake::Tools::strict_rules(&[("VBoxManage list vms", crate::platform::exec::fake::ok("")), ("vagrant", None)]);
        let (channel, seen) = events();
        assert!(machine_selftest(t.dirs(), Kind::Vm, None, None, channel).await.is_err());
        assert_eq!(states(&seen), ["vagrant:running", "vagrant:fail"]);
    }

    #[test]
    fn unknown_errors_pass_through_unchanged() {
        assert_eq!(explain("something else broke"), "something else broke");
    }
}
