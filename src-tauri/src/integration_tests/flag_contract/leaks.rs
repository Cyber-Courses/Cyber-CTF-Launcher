//! Where a launch's secret values must never show: the launcher's log of the run, the files under
//! the lab's folder (Isoloom's generated files, the launcher's marks) and the runner's home (its
//! records and event logs), and what Docker tells about the lab's containers (`docker inspect`: a
//! value in a container's environment or labels would show there).

use std::path::Path;

use crate::platform::exec::run;

/// Each place among `lines` (a log), the files under `roots` and the containers of Compose project
/// `project` that holds one of `secrets` (empty ones aside), named, never with the value.
pub(super) async fn places_holding(secrets: &[String], lines: &[String], roots: &[&Path], project: &str) -> Vec<String> {
    let secrets: Vec<&str> = secrets.iter().map(String::as_str).filter(|s| !s.is_empty()).collect();
    let holds = |text: &str| secrets.iter().any(|s| text.contains(s));
    let mut found: Vec<String> = lines.iter().enumerate().filter(|(_, l)| holds(l)).map(|(i, _)| format!("log line {i}")).collect();
    for root in roots {
        found.extend(
            files_under(root).into_iter().filter(|f| std::fs::read(f).is_ok_and(|b| holds(&String::from_utf8_lossy(&b)))).map(|f| f.display().to_string()),
        );
    }
    if holds(&inspect(project).await) {
        found.push(format!("docker inspect of {project}'s containers"));
    }
    found
}

/// Every file under `root` (none when it doesn't exist).
fn files_under(root: &Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    entries
        .filter_map(std::result::Result::ok)
        .flat_map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => files_under(&e.path()),
            Ok(t) if t.is_file() => vec![e.path()],
            _ => Vec::new(),
        })
        .collect()
}

/// `docker inspect` of every container of Compose project `project`, one-off ones included.
async fn inspect(project: &str) -> String {
    let filter = format!("label=com.docker.compose.project={project}");
    let ids = run("docker", &["ps", "-aq", "--filter", &filter], None).await.unwrap_or_default();
    let ids: Vec<&str> = ids.split_whitespace().collect();
    if ids.is_empty() {
        return String::new();
    }
    let args: Vec<&str> = ["inspect"].into_iter().chain(ids).collect();
    run("docker", &args, None).await.unwrap_or_default()
}
