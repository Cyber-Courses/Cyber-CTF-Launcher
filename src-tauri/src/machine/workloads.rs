//! What Cyber CTF is running on this machine, for the Machine page: running lab containers
//! (grouped per lab, with memory) and local lab VMs, each stoppable. Only things Cyber CTF
//! started: containers named `cyberctf-*`, VMs whose Vagrant home is under the app's lab or
//! self-test folders.

mod docker;
mod vm;

use std::future::Future;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::runtime::Runtime;

/// What stopping a workload needs from the labs' runtime: whether lab `id` is installed and runs
/// here (in a VM or not), and the lab's own stop.
pub trait LabRuntime {
    /// How lab `id` runs here, when it is installed and running on this machine.
    fn runtime_here(&self, id: &str, in_vm: bool) -> Option<Runtime>;

    /// Stops lab `id`, running on `runtime`, as its page would.
    fn stop_lab(&self, id: &str, runtime: Runtime) -> impl Future<Output = Result<()>> + Send;
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Workload {
    /// Lab id, or `selftest` for a setup test.
    pub id: String,
    /// `docker` or `vm`.
    #[ts(type = r#""docker" | "vm""#)]
    pub kind: &'static str,
    /// Containers running for it (VMs: 1 per machine).
    pub count: u32,
    /// Memory used, when the runtime reports it (Docker); 0 if unknown.
    pub mem_bytes: u64,
    /// Hypervisor, for VMs.
    pub provider: Option<String>,
}

/// What the app runs on this machine: its labs' containers and local VMs.
pub async fn machine_workloads(dirs: AppDirs) -> Vec<Workload> {
    let (mut d, v) = tokio::join!(docker::workloads(), vm::workloads(&dirs));
    d.extend(v);
    d
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Stops a workload from the Machine page. An installed lab running here gets its own Stop (the
/// lab page's: under its lock, with its volumes, pinned ports, networks and attack box or VM);
/// anything else of the app's (a lab removed since, the self-test) goes by name: its containers
/// (incl. its attack box), or its local VMs. The lab can be started again from its page.
pub async fn machine_workload_stop(labs: &impl LabRuntime, dirs: AppDirs, kind: String, id: String) -> Result<()> {
    if !valid_id(&id) {
        return Err(Error::Invalid(format!("invalid id `{id}`")));
    }
    let in_vm = match kind.as_str() {
        "docker" => false,
        "vm" => true,
        _ => return Err(Error::Invalid(format!("unknown workload kind `{kind}`"))),
    };
    if let Some(runtime) = labs.runtime_here(&id, in_vm) {
        return labs.stop_lab(&id, runtime).await;
    }
    if in_vm { vm::stop(&dirs, &id).await } else { docker::stop(&id).await }
}

#[cfg(test)]
mod tests {
    #[test]
    fn workload_ids_are_plain_names() {
        assert!(super::valid_id("goad-light_2") && super::valid_id("selftest"));
        assert!(!super::valid_id("") && !super::valid_id("../x") && !super::valid_id(&"a".repeat(65)));
    }

    /// A fake labs' runtime: an installed, running lab here gets its own Stop (the lab page's,
    /// now the Isothrum runner's); anything else goes by name through the Machine-page janitor.
    /// The runner-backed Stop itself is covered by the runner's tests and the real-lab tests.
    struct FakeLabs {
        installed: Vec<(&'static str, super::Runtime)>,
        stopped: std::sync::Mutex<Vec<(String, super::Runtime)>>,
    }
    impl super::LabRuntime for FakeLabs {
        fn runtime_here(&self, id: &str, in_vm: bool) -> Option<super::Runtime> {
            self.installed.iter().find(|(n, r)| *n == id && matches!(r, super::Runtime::Vm) == in_vm).map(|(_, r)| *r)
        }
        fn stop_lab(&self, id: &str, runtime: super::Runtime) -> impl std::future::Future<Output = crate::error::Result<()>> + Send {
            self.stopped.lock().unwrap().push((id.to_string(), runtime));
            std::future::ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn the_machine_page_lists_and_stops_the_apps_containers_and_vms() {
        use crate::platform::exec::fake::{Tools, ok};
        let t = crate::testing::test_app::TestApp::new();
        let lab = t.lab("goad", &[]);
        let vagrant = crate::runtime::lab::vagrant_dir(&lab, crate::runtime::Runtime::Vm);
        std::fs::create_dir_all(&vagrant).unwrap();
        std::fs::write(vagrant.join("Vagrantfile"), "").unwrap();
        let global = format!("1,,machine-id,a\n1,,provider-name,virtualbox\n1,,machine-home,{}\n1,,state,running\n", vagrant.display());
        let tools = Tools::rules(&[
            ("docker ps --filter", ok("aaa\tcyberctf-sqli-web-1\tcyberctf-sqli\n")),
            ("docker stats", ok("aaa\t10MiB / 1GiB\n")),
            ("global-status", ok(global)),
            ("ps -aq", ok("bbb\nccc\n")),
        ]);
        let w = super::machine_workloads(t.dirs()).await;
        let ids: Vec<(&str, &str, u64)> = w.iter().map(|w| (w.id.as_str(), w.kind, w.mem_bytes)).collect();
        assert_eq!(ids, [("sqli", "docker", 10 * 1024 * 1024), ("goad", "vm", 0)]);
        // `goad` is an installed VM lab running here: its Stop is the lab's own (the runner's).
        let labs = FakeLabs { installed: vec![("goad", super::Runtime::Vm)], stopped: std::sync::Mutex::new(Vec::new()) };
        // `sqli` is a stray container the app left: it goes by name through the janitor.
        super::machine_workload_stop(&labs, t.dirs(), "docker".into(), "sqli".into()).await.unwrap();
        assert!(tools.called("compose -p cyberctf-sqli down") && tools.called("docker rm -f bbb ccc"));
        super::machine_workload_stop(&labs, t.dirs(), "vm".into(), "goad".into()).await.unwrap();
        assert_eq!(*labs.stopped.lock().unwrap(), [("goad".to_string(), super::Runtime::Vm)], "the lab's own stop, not the janitor's");
        let err = super::machine_workload_stop(&labs, t.dirs(), "vm".into(), "other".into()).await.unwrap_err().to_string();
        assert_eq!(err, "no VMs found for `other`");
        assert!(super::machine_workload_stop(&labs, t.dirs(), "vm".into(), "selftest".into()).await.is_err());
        assert!(
            super::machine_workload_stop(&labs, t.dirs(), "tape".into(), "goad".into()).await.unwrap_err().to_string().starts_with("unknown workload kind")
        );
        assert!(super::machine_workload_stop(&labs, t.dirs(), "vm".into(), "../x".into()).await.is_err());
    }

    /// The real Docker: a lab's containers stopped from the Machine page go with their volume,
    /// networks and pinned ports, as from the lab page. Opt-in (needs Docker and the image named
    /// by `CYBERCTF_TEST_IMAGE`, default python:3.11-alpine, already local):
    /// `cargo test real_docker -- --ignored`.
    #[tokio::test]
    #[ignore = "needs Docker and a local test image"]
    async fn real_docker_machine_page_stop_removes_the_labs_volumes_and_networks() {
        use crate::platform::exec::run;
        let image = std::env::var("CYBERCTF_TEST_IMAGE").unwrap_or_else(|_| "python:3.11-alpine".into());
        let t = crate::testing::test_app::TestApp::new();
        let id = format!("e2e{}", rand::random::<u16>());
        let project = format!("cyberctf-{id}");
        let lab = t.lab(&id, &[]);
        let compose = crate::runtime::lab::compose_file(&lab);
        std::fs::create_dir_all(compose.parent().unwrap()).unwrap();
        std::fs::write(
            &compose,
            format!(
                "services:\n  web:\n    image: {image}\n    command: [\"sleep\", \"300\"]\n    labels: {{ cyberctf.e2e: stop }}\n    volumes: [\"data:/data\"]\n    networks: [lab]\nvolumes:\n  data: {{}}\nnetworks:\n  lab: {{}}\n"
            ),
        )
        .unwrap();
        std::fs::write(lab.join(".cyberctf-ports.yml"), "services: {}\n").unwrap();
        let file = compose.display().to_string();
        run("docker", &["compose", "-p", &project, "-f", &file, "up", "-d"], Some(&lab)).await.unwrap();
        let listed = super::machine_workloads(t.dirs()).await;
        assert!(listed.iter().any(|w| w.id == id && w.kind == "docker"), "{}", listed.iter().map(|w| w.id.as_str()).collect::<Vec<_>>().join(","));
        super::machine_workload_stop(t.handle(), t.dirs(), "docker".into(), id.clone()).await.unwrap();
        let label = format!("label=com.docker.compose.project={project}");
        let containers = run("docker", &["ps", "-aq", "--filter", &label], None).await.unwrap();
        let volumes = run("docker", &["volume", "ls", "-q", "--filter", &label], None).await.unwrap();
        let networks = run("docker", &["network", "ls", "-q", "--filter", &label], None).await.unwrap();
        // Whatever is left goes before the assertions, so a failure leaves nothing behind.
        let _ = run("docker", &["compose", "-p", &project, "-f", &file, "down", "--volumes", "-t", "0"], Some(&lab)).await;
        assert!(containers.trim().is_empty(), "containers left: {containers}");
        assert!(volumes.trim().is_empty(), "the lab's volume was left: {volumes}");
        assert!(networks.trim().is_empty(), "networks left: {networks}");
        assert!(!lab.join(".cyberctf-ports.yml").exists(), "the pinned ports went");
    }

    #[tokio::test]
    async fn nothing_running_lists_nothing() {
        let _tools = crate::platform::exec::fake::Tools::rules(&[("docker ps", None)]);
        assert!(super::docker::workloads().await.is_empty());
    }
}
