//! Bringing a lab up and tearing it down.

use std::path::Path;

use super::compose;
use crate::error::{Error, Result};
use crate::platform::exec::run;

/// Set (to "default") in a launch's env when the player chose the lab's default ports: each
/// service is published on its own container port (`CTFd` on 8000) instead of a random free one.
pub const PORTS_ENV: &str = "CYBERCTF_PORTS";

/// Docker's "Pool overlaps with other one on this address space", said as what to do: another
/// network (a lab, or one left behind) holds an address block this lab needs.
pub(crate) fn explain_pool_overlap(e: Error) -> Error {
    if !e.to_string().contains("Pool overlaps") {
        return e;
    }
    Error::Invalid(format!(
        "Another Docker network already uses the addresses this lab needs. Stop the other lab, or remove the network left behind (`docker network prune` removes the unused ones), then start this lab again. ({e})"
    ))
}

/// Removes containers, networks and volumes: the next start is a clean lab.
pub async fn stop(dir: &Path, id: &str, mut log: impl FnMut(String)) -> Result<()> {
    let project = compose::project(id);
    let down = compose::stream(dir, &project, &["down", "--volumes", "--remove-orphans"], &[], &mut log).await;
    compose::remove_all_containers(dir, &project).await;
    // Every bare docker call below goes to the engine the lab was pinned to at launch (if any), so
    // stop reaches the lab's containers and network even if the current context has since changed.
    let ctx = compose::context_prefix(dir);
    let docker = |rest: &[&str]| {
        let argv: Vec<String> = ctx.iter().cloned().chain(rest.iter().map(|s| (*s).to_string())).collect();
        async move { run("docker", &argv.iter().map(String::as_str).collect::<Vec<_>>(), None).await }
    };
    // Upgrade only: an attack box an older version ran beside the lab is a bare container with no
    // Compose project, so `down --remove-orphans` and the project-label removals never take it
    // out. Remove it by name, or it is left running and still read as this lab by the workload
    // scan. (Today's box is a service of the lab's project, gone with it.)
    for name in compose::legacy_attack_box_names(&project) {
        let _ = docker(&["rm", "-f", &name]).await;
    }
    // Belt and braces: whatever network of this lab is still around (something else plugged into
    // it, or Compose gave up), unplug everything from it and remove it, so nothing holds the subnet.
    let filter = format!("label=com.docker.compose.project={project}");
    if let Ok(listed) = docker(&["network", "ls", "--filter", &filter, "--format", "{{.Name}}"]).await {
        for net in listed.lines().map(str::trim).filter(|n| !n.is_empty()) {
            remove_network(&ctx, net).await;
        }
    }
    // The next start is a new lab: it picks its ports again.
    let _ = std::fs::remove_file(dir.join(compose::PORTS_FILE));
    down
}

/// Unplugs every container from `net` (force) and removes it, on the lab's engine (`ctx`). Best
/// effort: a network that is already gone, or that something unrelated won't let go of, is left.
async fn remove_network(ctx: &[String], net: &str) {
    let docker = |rest: &[&str]| {
        let argv: Vec<String> = ctx.iter().cloned().chain(rest.iter().map(|s| (*s).to_string())).collect();
        async move { run("docker", &argv.iter().map(String::as_str).collect::<Vec<_>>(), None).await }
    };
    if let Ok(attached) = docker(&["network", "inspect", "-f", "{{range .Containers}}{{.Name}} {{end}}", net]).await {
        for c in attached.split_whitespace() {
            let _ = docker(&["network", "disconnect", "-f", net, c]).await;
        }
    }
    let _ = docker(&["network", "rm", net]).await;
}

#[cfg(test)]
mod tests {
    use super::explain_pool_overlap;

    #[test]
    fn a_pool_overlap_says_what_to_do() {
        use crate::error::Error;
        let raw = Error::CommandFailed {
            command: "docker compose up".into(),
            stderr: "Error response from daemon: invalid pool request: Pool overlaps with other one on this address space".into(),
        };
        let said = explain_pool_overlap(raw).to_string();
        assert!(said.starts_with("Another Docker network already uses the addresses this lab needs.") && said.contains("Pool overlaps"), "{said}");
        assert_eq!(explain_pool_overlap(Error::Invalid("port taken".into())).to_string(), "port taken");
    }

    #[test]
    fn nothing_the_app_runs_listens_beyond_the_loopback() {
        // Regression (#230): the port check bound 0.0.0.0, and Windows Firewall asked the player
        // whether Cyber CTF may accept connections from public networks, mid-deploy.
        for src in crate::testing::source_scan::crate_sources() {
            for needle in ["\"0.0.0.0\"", "UNSPECIFIED", "\"::\"", "\"[::]"] {
                let binds = src.code.lines().filter(|l| l.contains("bind") && l.contains(needle)).collect::<Vec<_>>();
                assert!(binds.is_empty(), "{} binds beyond the loopback: {binds:?}", src.path.display());
            }
        }
    }
}
