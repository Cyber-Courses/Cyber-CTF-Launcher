//! Isoloom's registry of running environments (`~/.isoloom/status.yml`): the launcher records
//! what it starts and forgets what it stops, so `isoloom status`, `connect` and `exec` see
//! launcher-run labs too. Best effort: a registry that can't be written never fails a launch.
//!
//! Optional interop, on by default: with the setting off (Settings > Labs) the launcher records
//! nothing there, and works the same. It still removes the entries of a lab that stops (one
//! recorded while the setting was on), and reads the file, to keep its labs' instance numbers
//! clear of environments the CLI runs.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use isoloom_core::{Spec, Target, registry};
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::AppHandle;
use crate::error::{Error, Result};

/// The setting's file, in the app's data folder (so the deploy worker sees the same choice).
static FILE: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
struct Setting {
    /// Whether the launcher records its labs in Isoloom's registry.
    enabled: bool,
}

impl Default for Setting {
    fn default() -> Self {
        Setting { enabled: true }
    }
}

/// Points the setting at the app's data folder. Called at start by the app and the deploy worker.
pub fn init(app: &AppHandle) {
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = FILE.set(dir.join("isoloom-registry.json"));
    }
}

/// The setting in `file`: on when there is no file or it doesn't read.
fn load_from(file: Option<&Path>) -> Setting {
    file.and_then(|f| std::fs::read_to_string(f).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Saves the setting into `file` (None until `init`).
fn save_to(file: Option<&Path>, enabled: bool) -> Result<()> {
    let file = file.ok_or_else(|| Error::Invalid("settings aren't ready yet".into()))?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    crate::atomic_file::write(file, serde_json::to_string_pretty(&Setting { enabled }).map_err(|e| Error::Invalid(e.to_string()))?)?;
    Ok(())
}

/// Whether the launcher records its labs in Isoloom's registry.
pub fn enabled() -> bool {
    load_from(FILE.get().map(PathBuf::as_path)).enabled
}

/// Whether the launcher records its labs in `~/.isoloom/status.yml`, for Isoloom's CLI.
#[tauri::command]
pub fn isoloom_registry_get() -> bool {
    enabled()
}

/// Turns recording labs in Isoloom's registry on or off; the setting as saved. Turned off, the
/// entries already there are removed as their labs stop.
#[tauri::command]
pub fn isoloom_registry_set(enabled: bool) -> Result<bool> {
    save_to(FILE.get().map(PathBuf::as_path), enabled)?;
    Ok(isoloom_registry_get())
}

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
    update(Change::Record(entry));
}

/// What the launcher does to the registry.
enum Change {
    /// A lab started: its entry added (or replaced).
    Record(registry::Entry),
    /// A lab stopped: every entry of its folder (a [`key`]) removed.
    Forget(PathBuf),
}

/// Applies `change` to Isoloom's registry, best effort. Unit tests apply it to an empty registry
/// instead: the real one is the user's own `~/.isoloom`, never a test's to write.
fn update(change: Change) {
    update_with(enabled(), change, |f| {
        #[cfg(not(test))]
        let _ = registry::update(f);
        #[cfg(test)]
        f(&mut registry::Registry::default());
    });
}

/// [`update`] with the setting `enabled` and `write` writing the registry. With the setting off
/// nothing is recorded, but a lab's entries are still forgotten: one recorded while it was on
/// must not stay behind as a ghost (`isoloom status`, its instance number held) once it stops.
fn update_with(enabled: bool, change: Change, write: impl FnOnce(Box<dyn FnOnce(&mut registry::Registry)>)) {
    match change {
        Change::Record(entry) if enabled => write(Box::new(move |reg| reg.upsert(entry))),
        Change::Record(_) => {}
        Change::Forget(dir) => write(Box::new(move |reg| drop_dir(reg, &dir))),
    }
}

/// The instance numbers the registry records for environments in other folders than `dir` (run
/// with the CLI), so a lab's instance stays clear of them.
pub fn instances_elsewhere(dir: &Path) -> Vec<u8> {
    let here = key(dir);
    registry::load().map(|reg| reg.environments.iter().filter(|e| e.dir != here).filter_map(|e| e.instance).collect()).unwrap_or_default()
}

/// The registry entry of the lab in `dir`, running on `target` since `started`.
fn entry(dir: &Path, spec: &Spec, target: Target, cloud: Option<&str>, project: Option<String>, started: String) -> registry::Entry {
    registry::Entry { name: spec.name.clone(), dir: key(dir), target, instance: super::instance(dir), cloud: cloud.map(str::to_string), project, started }
}

/// Forgets every entry of the lab (whatever target it ran on).
pub fn forget(dir: &Path) {
    update(Change::Forget(key(dir)));
}

/// Removes every entry of `dir` (a [`key`]) from the registry.
fn drop_dir(reg: &mut registry::Registry, dir: &Path) {
    reg.environments.retain(|e| e.dir != dir);
}

#[cfg(test)]
mod tests {
    use super::{Change, Setting, drop_dir, entry, isoloom_registry_get, isoloom_registry_set, key, load_from, save_to, update_with};
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

    #[test]
    fn the_setting_is_on_until_turned_off() {
        let root = std::env::temp_dir().join(format!("cyberctf-registry-setting-{}", rand::random::<u32>()));
        let file = root.join("data").join("isoloom-registry.json");
        assert!(load_from(None).enabled && load_from(Some(&file)).enabled, "on by default");
        save_to(Some(&file), false).unwrap();
        assert_eq!(load_from(Some(&file)), Setting { enabled: false });
        save_to(Some(&file), true).unwrap();
        assert!(load_from(Some(&file)).enabled);
        std::fs::write(&file, "garbage").unwrap();
        assert!(load_from(Some(&file)).enabled, "a file that doesn't read is the default");
        assert_eq!(save_to(None, true).unwrap_err().to_string(), "settings aren't ready yet");
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The commands, before `init` (no settings file in tests): on, and saving says why not.
    #[test]
    fn the_commands_read_and_save_the_setting() {
        assert!(isoloom_registry_get());
        assert_eq!(isoloom_registry_set(false).unwrap_err().to_string(), "settings aren't ready yet");
        assert!(isoloom_registry_get());
    }

    /// Applies `change` with the setting `enabled` to `reg`; whether anything was written.
    fn apply(reg: &mut Registry, enabled: bool, change: Change) -> bool {
        let mut wrote = false;
        update_with(enabled, change, |f| {
            wrote = true;
            f(reg)
        });
        wrote
    }

    #[test]
    fn off_records_nothing_but_still_forgets() {
        let spec = spec("a");
        let (a, b) = (std::path::PathBuf::from("lab-a"), std::path::PathBuf::from("lab-b"));
        let mut reg = Registry::default();
        // On: recorded.
        assert!(apply(&mut reg, true, Change::Record(entry(&a, &spec, Target::Docker, None, None, String::new()))));
        assert_eq!(reg.environments.len(), 1);
        // Off: a start writes nothing at all.
        assert!(!apply(&mut reg, false, Change::Record(entry(&b, &spec, Target::Docker, None, None, String::new()))));
        assert_eq!(reg.environments.len(), 1);
        // Off: the lab recorded while on is forgotten when it stops, no ghost left.
        assert!(apply(&mut reg, false, Change::Forget(key(&a))));
        assert!(reg.environments.is_empty());
    }
}
