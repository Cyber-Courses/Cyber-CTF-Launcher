use std::sync::{Arc, Mutex};

use isothrum_runner::fake::{FakeProcesses, Reply};
use isothrum_runner::store::RunRow;

use super::{Runtime, WorkloadKind};

/// Run `name`'s row from `project` (Isoloom's folder for its target) in `format`, its machines
/// last seen as `seen` (name, state).
fn row(name: &str, project: &str, format: &str, provider: Option<&str>, seen: &[(&str, &str)]) -> RunRow {
    let machines: Vec<serde_json::Value> = seen.iter().map(|(m, state)| serde_json::json!({"name": m, "state": state, "published": []})).collect();
    let record = serde_json::json!({"version": 1, "name": name, "project": project, "format": format, "state": "ready", "provider": provider});
    serde_json::from_value(serde_json::json!({
        "name": name, "project": project, "source_digest": "", "desired": "running", "state": "ready", "failed_cause": null,
        "state_op": null, "state_at": "t", "record": record, "observed": {"machines": machines}, "observed_at": "t",
    }))
    .unwrap()
}

#[test]
fn a_run_is_its_labs_containers_or_vms_counted_as_last_seen_running() {
    let docker = row("cyberctf-web", "/labs/web/.isoloom/docker", "compose", None, &[("web", "running"), ("db", "unhealthy"), ("init", "stopped")]);
    let w = super::workload("web".into(), &docker).unwrap();
    assert_eq!((w.kind, w.count, w.mem_bytes, w.provider), (WorkloadKind::Docker, 2, 0, None));
    let vms = row("cyberctf-goad", "/labs/goad/.isoloom/vagrant", "vagrant", Some("virtualbox"), &[("dc01", "running"), ("srv02", "stopped")]);
    let w = super::workload("goad".into(), &vms).unwrap();
    assert_eq!((w.kind, w.count, w.provider.as_deref()), (WorkloadKind::Vm, 1, Some("virtualbox")));
    // The helper VM an earlier version ran a container lab inside is a VM.
    let helper = row("cyberctf-old", "/labs/old/.isoloom/docker-vm", "vagrant", Some("hyperv"), &[("default", "running")]);
    assert_eq!(super::workload("old".into(), &helper).map(|w| (w.kind, w.provider)), Some((WorkloadKind::Vm, Some("hyperv".into()))));
    // A module's run is neither.
    assert!(super::workload("mod".into(), &row("cyberctf-mod", "/labs/mod/.isoloom/proxmox", "terraform", None, &[])).is_none());
}

/// The app's runner, its tools a model of Docker: the lab `web`'s Compose project runs two
/// containers, the engine says what each uses (and another project's container besides).
fn served(t: &crate::testing::test_app::TestApp) -> Arc<FakeProcesses> {
    let fake = Arc::new(FakeProcesses::model(|call| match call.args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["compose", .., "ps", "--all", "--format", "json"] => Reply::ok(concat!(
            r#"{"ID":"aaa111","Service":"web","State":"running","Labels":""}"#,
            "\n",
            r#"{"ID":"bbb222","Service":"db","State":"running","Labels":""}"#,
            "\n"
        )),
        ["stats", "--no-stream", ..] => Reply::ok(concat!(
            r#"{"ID":"aaa111","CPUPerc":"1.00%","MemUsage":"10MiB / 1GiB"}"#,
            "\n",
            r#"{"ID":"bbb222","CPUPerc":"--","MemUsage":"1MiB / 1GiB"}"#,
            "\n",
            r#"{"ID":"ccc333","CPUPerc":"5.00%","MemUsage":"1GiB / 2GiB"}"#,
            "\n"
        )),
        _ => Reply::ok(""),
    }));
    let mut config = isothrum_runner::RunnerConfig::new(crate::runtime::paths::runner_home(&t.dirs()).unwrap());
    config.processes = Arc::clone(&fake) as Arc<dyn isothrum_runner::ProcessRunner>;
    crate::testing::service::serve_home(config);
    fake
}

#[tokio::test]
async fn the_machine_page_lists_the_runs_running_here_with_what_their_containers_use() {
    use crate::runtime::test_support::record_run;
    let t = crate::testing::test_app::TestApp::new();
    let _fake = served(&t);
    assert!(super::machine_workloads(t.dirs()).await.unwrap().is_empty(), "nothing recorded, nothing running");
    let project = t.lab("web", &[(".isoloom/docker/compose.yml", "services: {}\n")]);
    assert!(project.join(".isoloom/docker/compose.yml").is_file());
    for (lab, host, target, state) in [
        ("web", None, "docker", "ready"),
        ("goad", None, "vagrant", "ready"),
        ("parked", None, "docker", "parked"),
        ("cloud", Some("aws1"), "cloud-docker", "ready"),
    ] {
        record_run(t.data(), lab, host, target, state);
    }
    let listed = super::machine_workloads(t.dirs()).await.unwrap();
    let seen: Vec<(&str, WorkloadKind, u64)> = listed.iter().map(|w| (w.id.as_str(), w.kind, w.mem_bytes)).collect();
    // Its containers' memory summed, never another project's; VMs report none.
    assert_eq!(seen, [("goad", WorkloadKind::Vm, 0), ("web", WorkloadKind::Docker, 11 * 1024 * 1024)]);
}

#[tokio::test]
async fn runs_that_cant_be_listed_are_an_error_not_nothing_running() {
    let t = crate::testing::test_app::TestApp::new();
    crate::testing::runner::damage_store(&crate::runtime::paths::runner_home(&t.dirs()).unwrap());
    assert!(super::machine_workloads(t.dirs()).await.is_err());
}

/// A fake labs' runtime: an installed lab running here gets its own Stop (the lab page's, the
/// runner's), which the runner's tests and the integration tests cover.
struct FakeLabs {
    installed: Vec<(&'static str, Runtime)>,
    stopped: Mutex<Vec<(String, Runtime)>>,
}

impl super::LabRuntime for FakeLabs {
    fn runtime_here(&self, id: &str, in_vm: bool) -> impl Future<Output = Option<Runtime>> + Send {
        std::future::ready(self.installed.iter().find(|(n, r)| *n == id && matches!(r, Runtime::Vm) == in_vm).map(|(_, r)| *r))
    }

    fn stop_lab(&self, id: &str, runtime: Runtime) -> impl Future<Output = crate::error::Result<()>> + Send {
        self.stopped.lock().unwrap().push((id.to_owned(), runtime));
        std::future::ready(Ok(()))
    }
}

#[tokio::test]
async fn a_stop_from_the_machine_page_is_the_labs_own_stop() {
    let labs = FakeLabs { installed: vec![("goad", Runtime::Vm), ("sqli", Runtime::Docker)], stopped: Mutex::new(Vec::new()) };
    super::machine_workload_stop(&labs, WorkloadKind::Vm, "goad".into()).await.unwrap();
    super::machine_workload_stop(&labs, WorkloadKind::Docker, "sqli".into()).await.unwrap();
    assert_eq!(*labs.stopped.lock().unwrap(), [("goad".to_owned(), Runtime::Vm), ("sqli".to_owned(), Runtime::Docker)]);
    let err = super::machine_workload_stop(&labs, WorkloadKind::Vm, "other".into()).await.unwrap_err().to_string();
    assert!(err.contains("nothing of `other` runs on this machine"), "{err}");
    assert!(super::machine_workload_stop(&labs, WorkloadKind::Vm, "../x".into()).await.is_err());
    // An unknown kind never reaches the stop: the command's argument doesn't parse.
    assert!(serde_json::from_value::<WorkloadKind>(serde_json::json!("tape")).is_err());
    assert_eq!(serde_json::from_value::<WorkloadKind>(serde_json::json!("vm")).unwrap(), WorkloadKind::Vm);
}
