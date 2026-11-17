//! Setup self-tests: prove this machine can actually run labs, not just that the tools exist.
//!
//! - `docker`: isothrum-machine's engine self-test (`selftest::engine`), a throwaway
//!   two-container deployment (busybox, ~2 MB) through a runner of its own: the engine answers,
//!   the image is here, the deployment starts, its containers reach each other by name and a
//!   published port answers on this machine. Always cleaned up.
//! - `vm`: isothrum-machine's hypervisor self-test (`selftest::hypervisor`), a real throwaway VM
//!   on the first usable local hypervisor, through a runner of its own. It checks the hypervisor
//!   answers, gets a small test box (once, then cached), boots it on a private lab network, runs a
//!   command inside it, pings it from this machine, then destroys it.
//!
//! Progress goes to the UI as one event per step: `running`, then `ok` or `fail`.

mod docker;
mod vm;

use std::path::PathBuf;

use crate::error::Result;
use crate::platform::AppDirs;
use crate::runtime::providers::Provider;
use crate::runtime::selftest_report::{Event, Reporter};

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

/// Says one step's `progress` through `r`: the step named by `named`, what it found by `detail`,
/// why it failed by `error` (said through the reporter's explanation).
fn say<S, F, E>(
    r: &Reporter,
    progress: isothrum_machine::selftest::Progress<S, F, E>,
    named: fn(S) -> (&'static str, &'static str),
    detail: fn(F) -> Option<String>,
    error: fn(E) -> crate::error::Error,
) {
    use crate::runtime::selftest_report::StepState;
    use isothrum_machine::selftest::Progress;
    match progress {
        Progress::Running(step) => {
            let (id, label) = named(step);
            r.send(id, label, StepState::Running, None);
        }
        Progress::Passed(step, found) => {
            let (id, label) = named(step);
            r.send(id, label, StepState::Ok, detail(found));
        }
        Progress::Skipped(step, why) => {
            let (id, label) = named(step);
            r.send(id, label, StepState::Skip, detail(why));
        }
        Progress::Failed(step, why) => {
            let (id, label) = named(step);
            r.failed(id, label, &error(why).to_string());
        }
        _ => {}
    }
}

/// The self-test's own folder in the app's cache (`selftest/<kind>`), created when missing.
pub(crate) fn work_dir(dirs: &AppDirs, kind: &str) -> Result<PathBuf> {
    let dir = dirs.cache()?.join("selftest").join(kind);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Adds the test image or box a self-test fetched (`pulled`) to the app's runner record, for Storage to
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
            let Some(p) = vm::test_provider(provider).await.and_then(Provider::vagrant) else { return Ok(()) };
            let (bx, held) = isothrum_machine::selftest::hypervisor::box_for(&crate::runtime::providers::machine_selftest(), p).await;
            if held {
                return Ok(());
            }
            vm::ensure_box(&dirs, p, bx, &|_| {}).await.map(drop)
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
        Kind::Vm => Box::pin(vm::run_test(&dirs, &work_dir(&dirs, "vm")?, provider, &r)).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::test_app::Sink;

    fn states(seen: &Sink) -> Vec<String> {
        seen.messages().iter().map(|e| format!("{}:{}", e["step"].as_str().unwrap(), e["state"].as_str().unwrap())).collect()
    }

    /// This machine's reporter (its failures explained), whose events land in a sink.
    fn reporter() -> (Reporter, Sink) {
        let (events, sink) = events();
        (Reporter::explaining(events, explain), sink)
    }

    #[test]
    fn a_failed_step_says_the_hint_for_a_failure_people_hit() {
        let (r, seen) = reporter();
        r.failed("two", "Second", "Cannot connect to the Docker daemon");
        assert!(seen.messages()[0]["detail"].as_str().unwrap().starts_with("The container engine isn't reachable"));
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
        use crate::testing::tools::Tools;
        let t = crate::testing::test_app::TestApp::new();
        let tools = Tools::strict_rules(&[]);
        let first = one_at_a_time(Kind::Docker).lock().await;
        let (channel, seen) = events();
        let second = tokio::time::timeout(std::time::Duration::from_millis(200), machine_selftest(t.dirs(), Kind::Docker, None, None, channel)).await;
        assert!(second.is_err(), "it waits");
        assert!(tools.calls().is_empty() && seen.messages().is_empty());
        drop(first);
    }

    /// A machine whose hypervisors all answer, their plugins installed, the test box held, Vagrant
    /// there when `vagrant` (else missing): the VM boots, answers the command and the ping, and
    /// goes.
    fn hypervisors(vagrant: bool) -> isothrum_runner::fake::FakeProcesses {
        use isothrum_runner::fake::{FakeProcesses, Reply};
        let up = std::sync::Mutex::new(String::new());
        FakeProcesses::model(move |call| {
            let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
            let mut on = up.lock().unwrap();
            match (call.program.as_str(), args.as_slice()) {
                ("vagrant", _) if !vagrant => Reply::missing(),
                ("vagrant", ["--version"]) => Reply::ok("Vagrant 2.4.3"),
                ("vagrant", ["plugin", "list", ..]) => Reply::ok(
                    "1,,plugin-name,vagrant-qemu\n1,,plugin-name,vagrant-parallels\n1,,plugin-name,vagrant-vmware-desktop\n1,,plugin-name,vagrant-libvirt\n1,,plugin-name,vagrant_utm",
                ),
                ("vagrant", ["box", "list"]) => Reply::ok(
                    "generic/alpine319 (virtualbox, 4.3.12)\nbento/debian-12 (virtualbox, 1)\ngeneric/alpine319 (libvirt, 4.3.12)\nbento/debian-12 (parallels, 1)\nbento/debian-12 (vmware_desktop, 1)\ngeneric/alpine319 (hyperv, 1)\nutm/bookworm (utm, 1)\n",
                ),
                ("vagrant", ["up", "--provider", provider, ..]) => {
                    *on = (*provider).to_owned();
                    Reply::ok("==> default: Booting VM...")
                }
                ("vagrant", ["status", "--machine-readable"]) => {
                    let state = if on.is_empty() { "not_created" } else { "running" };
                    Reply::ok(&format!("1,default,provider-name,{on}\n1,default,state,{state}"))
                }
                ("vagrant", ["ssh", "default", "-c", ..]) => Reply::ok("Linux 6.6 x86_64"),
                ("vagrant", ["destroy", ..]) => {
                    on.clear();
                    Reply::ok("")
                }
                // Every test address is free until the VM boots on the first.
                ("ping", [.., address]) if !on.is_empty() && *address == "192.168.56.250" => {
                    Reply::ok("64 bytes from 192.168.56.250: icmp_seq=1 ttl=64 time=0.412 ms")
                }
                ("ping", _) => Reply::fail(1, "1 packets transmitted, 0 received"),
                _ => Reply::ok(""),
            }
        })
    }

    /// The VM test on [`hypervisors`]: how it went, its events, and the calls made.
    async fn vm_test(vagrant: bool) -> (Result<()>, Sink, std::sync::Arc<isothrum_runner::fake::FakeProcesses>, crate::testing::test_app::TestApp) {
        let machine = crate::testing::temp_dir("vm-test-machine");
        let tools = std::sync::Arc::new(hypervisors(vagrant));
        let runner: std::sync::Arc<dyn isothrum_runner::ProcessRunner> = tools.clone();
        let _machine = crate::runtime::providers::testing::Machine::set(runner, crate::runtime::providers::testing::host(&machine));
        let t = crate::testing::test_app::TestApp::new();
        let (channel, seen) = events();
        (machine_selftest(t.dirs(), Kind::Vm, None, None, channel).await, seen, tools, t)
    }

    #[tokio::test]
    async fn the_vm_test_says_each_step_in_the_apps_words_boots_pings_and_deletes_a_vm() {
        let (ran, seen, tools, t) = vm_test(true).await;
        let done: Vec<String> = seen
            .messages()
            .iter()
            .filter(|e| e["state"] != "running")
            .map(|e| {
                format!(
                    "{}:{}:{}:{}",
                    e["step"].as_str().unwrap(),
                    e["state"].as_str().unwrap(),
                    e["label"].as_str().unwrap(),
                    e["detail"].as_str().unwrap_or("")
                )
            })
            .collect();
        ran.unwrap();
        let provider = done[1].rsplit(':').next().unwrap().to_owned();
        // The box this CPU gets for that hypervisor.
        let boxed = done[3].rsplit(':').next().unwrap().trim_end_matches(" (ready)").to_owned();
        let mut said = vec!["vagrant:ok:Vagrant is installed:Vagrant 2.4.3".to_owned(), format!("provider:ok:A hypervisor is ready:{provider}")];
        assert_eq!(done[..2], said[..], "{done:?}");
        said.push(done[2].clone());
        said.extend([
            format!("box:ok:Get a small test VM image:{boxed} (ready)"),
            format!("boot:ok:Boot the test VM:{boxed} on {provider}"),
            "exec:ok:Run a command inside the VM:Linux 6.6 x86_64".to_owned(),
        ]);
        assert_eq!(done[..6], said[..], "{done:?}");
        assert!(done[2].starts_with("hypervisor:ok:Hypervisor responds"), "{done:?}");
        assert!(done[6].starts_with("network:"), "{done:?}");
        assert_eq!(done.last().map(String::as_str), Some("cleanup:ok:Delete the test VM:test box kept for next time"), "{done:?}");
        let calls: Vec<String> = tools.calls().iter().map(|c| format!("{} {}", c.program, c.args.join(" "))).collect();
        assert!(calls.iter().any(|c| c.starts_with("vagrant up --provider")) && calls.iter().any(|c| c.starts_with("vagrant destroy")), "{calls:?}");
        // The box is there already: nothing to prefetch.
        machine_selftest_prefetch(t.dirs(), Kind::Vm, None).await.unwrap();
        assert!(!tools.calls().iter().any(|c| c.args.join(" ").contains("box add")));
    }

    #[tokio::test]
    async fn without_vagrant_the_vm_test_stops_at_once() {
        let (ran, seen, _, _) = vm_test(false).await;
        assert!(ran.is_err());
        assert_eq!(states(&seen), ["vagrant:running", "vagrant:fail"]);
    }

    #[test]
    fn unknown_errors_pass_through_unchanged() {
        assert_eq!(explain("something else broke"), "something else broke");
    }
}
