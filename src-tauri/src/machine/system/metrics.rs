//! Live machine health, polled by the Machine screen.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use sysinfo::{Disks, System};

use crate::exec::run_read;

/// CPU usage needs two samples a moment apart.
const CPU_SAMPLE_GAP: Duration = Duration::from_millis(220);

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct MachineMetrics {
    /// Overall CPU usage, 0-100.
    pub cpu: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub disk_used: u64,
    pub disk_total: u64,
    pub uptime_secs: u64,
    pub cores: usize,
    /// Running Docker containers right now (0 when the engine is down).
    pub containers: u32,
}

#[tauri::command]
pub async fn machine_metrics() -> MachineMetrics {
    let mut sys = System::new();
    sys.refresh_cpu_usage();
    tokio::time::sleep(CPU_SAMPLE_GAP).await;
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    let (disk_used, disk_total) = system_disk();
    MachineMetrics {
        cpu: sys.global_cpu_usage(),
        mem_used: sys.used_memory(),
        mem_total: sys.total_memory(),
        disk_used,
        disk_total,
        uptime_secs: System::uptime(),
        cores: sys.cpus().len(),
        containers: running_containers().await,
    }
}

/// (used, total) bytes of the root disk, else of the largest one.
fn system_disk() -> (u64, u64) {
    let disks = Disks::new_with_refreshed_list();
    let (total, available) = disks
        .iter()
        .find(|d| d.mount_point() == Path::new("/"))
        .or_else(|| disks.iter().max_by_key(|d| d.total_space()))
        .map(|d| (d.total_space(), d.available_space()))
        .unwrap_or((0, 0));
    (total.saturating_sub(available), total)
}

async fn running_containers() -> u32 {
    run_read("docker", &["ps", "--format", "{{.ID}}"], None).await.ok().map(|o| count_ids(&o)).unwrap_or(0)
}

/// The container ids in `docker ps` output, one per non-empty line.
fn count_ids(out: &str) -> u32 {
    out.lines().filter(|l| !l.trim().is_empty()).count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_metrics_describe_this_machine_and_its_running_containers() {
        let tools = crate::exec::fake::Tools::rules(&[("docker ps", crate::exec::fake::ok("a1\nb2\n"))]);
        let m = machine_metrics().await;
        assert!(m.cores > 0 && m.mem_total > 0 && m.mem_used <= m.mem_total, "{} {} {}", m.cores, m.mem_used, m.mem_total);
        assert!(m.disk_used <= m.disk_total);
        assert!((0.0..=100.0).contains(&m.cpu), "{}", m.cpu);
        assert_eq!(m.containers, 2);
        drop(tools);
        // The engine down: none.
        let _tools = crate::exec::fake::Tools::rules(&[("docker ps", crate::exec::fake::fail(DOCKER_DOWN))]);
        assert_eq!(machine_metrics().await.containers, 0);
    }

    const DOCKER_DOWN: &str = "Cannot connect to the Docker daemon";

    #[test]
    fn counts_one_container_per_non_empty_line() {
        assert_eq!(count_ids("abc\n\n def \n"), 2);
        assert_eq!(count_ids(""), 0);
    }

    #[test]
    fn the_system_disk_is_used_out_of_total() {
        let (used, total) = system_disk();
        assert!(used <= total);
        let m = MachineMetrics { cpu: 1.5, mem_used: 1, mem_total: 2, disk_used: used, disk_total: total, uptime_secs: 3, cores: 4, containers: 0 };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["memTotal"], 2);
        assert_eq!(v["uptimeSecs"], 3);
    }
}
