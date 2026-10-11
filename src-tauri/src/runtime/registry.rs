//! Isoloom's registry of running environments (`~/.isoloom/status.yml`): the launcher records
//! what it starts and forgets what it stops, so `isoloom status`, `connect` and `exec` see
//! launcher-run labs too. Best effort: a registry that can't be written never fails a launch.

use std::path::{Path, PathBuf};

use isoloom_core::{Spec, Target, registry};

fn key(dir: &Path) -> PathBuf {
    dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf())
}

/// Records the lab as running on `target` (and, for the clouds, in which one).
pub fn record(dir: &Path, spec: &Spec, target: Target, cloud: Option<&str>) {
    upsert(dir, spec, target, cloud, None);
}

/// Records a lab running on local Docker, with the Compose project it runs as (the launcher's
/// own name, not the file's), so Isoloom's `status`, `connect` and `exec` find it.
pub fn record_docker(dir: &Path, spec: &Spec, project: String) {
    upsert(dir, spec, Target::Docker, None, Some(project));
}

fn upsert(dir: &Path, spec: &Spec, target: Target, cloud: Option<&str>, project: Option<String>) {
    let entry = entry(dir, spec, target, cloud, project, registry::now());
    update(|reg| reg.upsert(entry));
}

/// Updates Isoloom's registry, best effort. Unit tests apply the change to an empty registry
/// instead: the real one is the user's own `~/.isoloom`, never a test's to write.
fn update(f: impl FnOnce(&mut registry::Registry)) {
    #[cfg(not(test))]
    let _ = registry::update(f);
    #[cfg(test)]
    f(&mut registry::Registry::default());
}

/// The registry entry of the lab in `dir`, running on `target` since `started`.
fn entry(dir: &Path, spec: &Spec, target: Target, cloud: Option<&str>, project: Option<String>, started: String) -> registry::Entry {
    registry::Entry { name: spec.name.clone(), dir: key(dir), target, instance: super::lab::instance(dir), cloud: cloud.map(str::to_string), project, started }
}

/// Forgets every entry of the lab (whatever target it ran on).
pub fn forget(dir: &Path) {
    let dir = key(dir);
    update(|reg| drop_dir(reg, &dir));
}

/// Removes every entry of `dir` (a [`key`]) from the registry.
fn drop_dir(reg: &mut registry::Registry, dir: &Path) {
    reg.environments.retain(|e| e.dir != dir);
}

#[cfg(test)]
mod tests {
    use super::{drop_dir, entry, key};
    use isoloom_core::{Target, registry::Registry};

    fn spec(name: &str) -> isoloom_core::Spec {
        serde_yaml_ng::from_str(&format!("version: 1\nname: {name}\nmachines: {{}}\n")).unwrap()
    }

    #[test]
    fn an_entry_names_the_lab_its_folder_target_and_project() {
        let dir = std::env::temp_dir().join(format!("cyberctf-registry-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(crate::runtime::lab::INSTANCE_MARKER), "3").unwrap();
        let spec = spec("sqli");
        let e = entry(&dir, &spec, Target::CloudDocker, Some("aws"), Some("cyberctf-sqli".into()), "2026-10-10T00:00:00Z".into());
        assert_eq!(e.name, "sqli");
        // The folder as Isoloom's CLI writes it: absolute, symlinks resolved.
        assert_eq!(e.dir, dir.canonicalize().unwrap());
        assert_eq!((e.target, e.instance, e.cloud.as_deref(), e.project.as_deref()), (Target::CloudDocker, Some(3), Some("aws"), Some("cyberctf-sqli")));
        assert_eq!(e.started, "2026-10-10T00:00:00Z");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_folder_that_is_gone_is_its_own_key() {
        let gone = std::env::temp_dir().join("cyberctf-registry-never-created").join("lab");
        assert_eq!(key(&gone), gone);
    }

    #[test]
    fn forgetting_a_lab_drops_only_its_entries() {
        let spec = spec("a");
        let (a, b) = (std::path::PathBuf::from("lab-a"), std::path::PathBuf::from("lab-b"));
        let mut reg = Registry::default();
        for (dir, target) in [(&a, Target::Docker), (&b, Target::Docker), (&a, Target::Vagrant)] {
            reg.environments.push(entry(dir, &spec, target, None, None, String::new()));
        }
        drop_dir(&mut reg, &a);
        assert_eq!(reg.environments.len(), 1);
        assert_eq!(reg.environments[0].dir, b);
    }
}
