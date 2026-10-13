//! What Cyber CTF is running on this machine, for the Machine page: running lab containers
//! (grouped per lab, with memory) and local lab VMs, each stoppable. Only things Cyber CTF
//! started: containers named `cyberctf-*`, VMs whose Vagrant home is under the app's lab or
//! self-test folders.

mod docker;
mod vm;

use crate::AppHandle;
use serde::Serialize;

use crate::error::{Error, Result};

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

#[tauri::command]
pub async fn machine_workloads(app: AppHandle) -> Vec<Workload> {
    let (mut d, v) = tokio::join!(docker::workloads(), vm::workloads(&app));
    d.extend(v);
    d
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Stops a workload from the Machine page: removes a lab's containers (incl. its attack
/// box) or destroys its local VMs. The lab can be started again from its page.
#[tauri::command]
pub async fn machine_workload_stop(app: AppHandle, kind: String, id: String) -> Result<()> {
    if !valid_id(&id) {
        return Err(Error::Invalid(format!("invalid id `{id}`")));
    }
    match kind.as_str() {
        "docker" => docker::stop(&id).await,
        "vm" => vm::stop(&app, &id).await,
        _ => Err(Error::Invalid(format!("unknown workload kind `{kind}`"))),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn workload_ids_are_plain_names() {
        assert!(super::valid_id("goad-light_2") && super::valid_id("selftest"));
        assert!(!super::valid_id("") && !super::valid_id("../x") && !super::valid_id(&"a".repeat(65)));
    }

    #[tokio::test]
    async fn the_machine_page_lists_and_stops_the_apps_containers_and_vms() {
        use crate::exec::fake::{Tools, ok};
        let t = crate::test_app::TestApp::new();
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
        let w = super::machine_workloads(t.handle().clone()).await;
        let ids: Vec<(&str, &str, u64)> = w.iter().map(|w| (w.id.as_str(), w.kind, w.mem_bytes)).collect();
        assert_eq!(ids, [("sqli", "docker", 10 * 1024 * 1024), ("goad", "vm", 0)]);
        super::machine_workload_stop(t.handle().clone(), "docker".into(), "sqli".into()).await.unwrap();
        assert!(tools.called("compose -p cyberctf-sqli down") && tools.called("docker rm -f bbb ccc"));
        super::machine_workload_stop(t.handle().clone(), "vm".into(), "goad".into()).await.unwrap();
        assert!(tools.called("vagrant destroy -f"));
        let err = super::machine_workload_stop(t.handle().clone(), "vm".into(), "other".into()).await.unwrap_err().to_string();
        assert_eq!(err, "no VMs found for `other`");
        assert!(super::machine_workload_stop(t.handle().clone(), "vm".into(), "selftest".into()).await.is_err());
        assert!(
            super::machine_workload_stop(t.handle().clone(), "tape".into(), "goad".into()).await.unwrap_err().to_string().starts_with("unknown workload kind")
        );
        assert!(super::machine_workload_stop(t.handle().clone(), "vm".into(), "../x".into()).await.is_err());
    }

    #[tokio::test]
    async fn nothing_running_lists_nothing() {
        let _tools = crate::exec::fake::Tools::rules(&[("docker ps", None)]);
        assert!(super::docker::workloads().await.is_empty());
    }
}
