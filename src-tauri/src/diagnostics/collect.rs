//! What a diagnostics bundle says, read from the launcher's own data: the installed labs, the
//! last operations (the deploy workers' verdicts, and each run's last operation as the Isothrum
//! runner's API records it), the saved servers' kinds, the policy, and the recent logs. Read only:
//! nothing here starts, settles or changes anything. Values that could identify a host or an
//! account (a server's name, address and user, a run's params) are left out, not scrubbed.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;
use serde_json::Value;

use crate::platform::AppDirs;
use crate::runtime::{self, is_lab_id};

/// The most operations a bundle lists, newest first.
pub(crate) const OPERATIONS: usize = 20;

/// The most log files of each folder a bundle carries, newest first.
const LOG_FILES: usize = 8;

/// The end of a log file a bundle carries: its last lines, enough for the last runs.
const LOG_TAIL_BYTES: u64 = 256 * 1024;

/// A lab installed here.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstalledLab {
    /// Its id (its folder's name).
    pub id: String,
    /// The name its `isoloom.yml` gives, when it can be read.
    pub slug: Option<String>,
    /// The commit of its source installed (a catalogue lab's version), when recorded.
    pub commit: Option<String>,
    /// Whether it was imported from a folder on this computer.
    pub imported: bool,
}

/// One lab operation and how it ended.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Operation {
    /// Who recorded it: `launcher` (a deploy worker's verdict) or `isothrum` (the runner's record
    /// of the run's last operation).
    pub source: &'static str,
    /// Its id: the deploy worker's key, or the runner's run name.
    pub id: String,
    /// The lab it acted on.
    pub lab: String,
    /// What it was, when recorded (`up`, `down`, `park`…).
    pub op: Option<Value>,
    /// `succeeded`, `failed` or `inProgress`.
    pub outcome: &'static str,
    /// Its failure's code, when it failed with one.
    pub code: Option<Value>,
    /// When it ended (RFC 3339, UTC), or when it was last heard of.
    pub at: Option<String>,
}

/// The labs installed under the app data folder: an unreadable folder is none.
pub(crate) fn installed_labs(dirs: &AppDirs) -> Vec<InstalledLab> {
    let Ok(root) = runtime::paths::labs_root(dirs) else { return Vec::new() };
    let mut labs: Vec<InstalledLab> =
        names_in(&root).into_iter().filter(|id| is_lab_id(id) && root.join(id).is_dir()).map(|id| installed_lab(&root.join(&id), id)).collect();
    labs.sort_by(|a, b| a.id.cmp(&b.id));
    labs
}

/// The lab installed in `dir` under `id`.
fn installed_lab(dir: &Path, id: String) -> InstalledLab {
    let commit = std::fs::read_to_string(dir.join(runtime::lab::COMMIT_MARKER)).ok().map(|c| c.trim().to_owned()).filter(|c| !c.is_empty());
    let slug = runtime::lab::load(dir).ok().map(|spec| spec.name);
    let imported = id.starts_with(crate::labs::imported::PREFIX);
    InstalledLab { id, slug, commit, imported }
}

/// The file names in folder `dir`; none when it can't be read.
fn names_in(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir).map(|entries| entries.flatten().filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default()
}

/// The deploy workers' operations: each lab's last verdict (`<key>.status`), or one in progress
/// (its job file there, no verdict yet).
pub(crate) fn worker_operations(dirs: &AppDirs) -> Vec<Operation> {
    let Ok(base) = dirs.data() else { return Vec::new() };
    let dir = base.join("deploys");
    let mut keys: Vec<String> = names_in(&dir)
        .iter()
        .filter_map(|n| n.strip_suffix(".status").or_else(|| n.strip_suffix(".json")))
        .filter(|k| is_lab_id(k))
        .map(str::to_owned)
        .collect();
    keys.sort();
    keys.dedup();
    keys.into_iter().map(|key| worker_operation(&dir, key)).collect()
}

/// The last operation of the deploy worker of `key` in the workers' folder `dir`.
fn worker_operation(dir: &Path, key: String) -> Operation {
    let status = dir.join(format!("{key}.status"));
    let (outcome, code, at) = match std::fs::read_to_string(&status) {
        Ok(text) => {
            let (outcome, code) = verdict(&text);
            (outcome, code, modified(&status))
        }
        Err(_) => ("inProgress", None, modified(&dir.join(format!("{key}.json")))),
    };
    Operation { source: "launcher", lab: key.clone(), id: key, op: None, outcome, code, at }
}

/// A status file's verdict (`ok`, or `error: {"code":…}` as the worker writes it): the outcome
/// and the error's code when it gives one.
fn verdict(status: &str) -> (&'static str, Option<Value>) {
    let s = status.trim();
    if s == "ok" {
        return ("succeeded", None);
    }
    let body = s.strip_prefix("error: ").unwrap_or(s);
    let code = serde_json::from_str::<Value>(body).ok().and_then(|v| v.get("code").cloned());
    ("failed", code)
}

/// When `path` was last written, as RFC 3339; none when unknown.
fn modified(path: &Path) -> Option<String> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok().map(isothrum_runner::rfc3339)
}

/// Each run's last operation, as the Isothrum runner records it (its `runs()`, the launcher's
/// one way to read the runner's records); an error when the records can't be read.
pub(crate) async fn runner_operations(dirs: &AppDirs) -> crate::error::Result<Vec<Operation>> {
    let records = runtime::isothrum::records(&runtime::paths::runner_home(dirs)?).await?;
    Ok(records
        .into_iter()
        .filter_map(|r| {
            let last = r.last?;
            let name = r.name.as_str().to_owned();
            let lab = name.strip_prefix("cyberctf-").unwrap_or(&name).to_owned();
            let outcome = if last.ok { "succeeded" } else { "failed" };
            let code = last.code.and_then(|c| serde_json::to_value(c).ok());
            Some(Operation { source: "isothrum", id: name, lab, op: serde_json::to_value(last.op).ok(), outcome, code, at: Some(last.at) })
        })
        .collect())
}

/// The newest `n` of `operations`, newest first (one with no time last).
pub(crate) fn newest(mut operations: Vec<Operation>, n: usize) -> Vec<Operation> {
    operations.sort_by(|a, b| b.at.cmp(&a.at));
    operations.truncate(n);
    operations
}

/// The saved servers and cloud accounts by kind only (`proxmox`, `aws`…), and whether each is
/// the default: their names, addresses and users stay out.
pub(crate) fn servers(dirs: &AppDirs) -> crate::error::Result<Value> {
    let store = runtime::server::store::load(dirs)?;
    let hosts: Vec<Value> =
        store.hosts.iter().map(|h| serde_json::json!({ "provider": h.provider, "default": store.default.as_deref() == Some(h.id.as_str()) })).collect();
    Ok(Value::Array(hosts))
}

/// The values a running or lost deploy job gives its lab (its launch token, its flags): secrets
/// the bundle replaces wherever they show.
pub(crate) fn job_secrets(dirs: &AppDirs) -> Vec<String> {
    let Ok(base) = dirs.data() else { return Vec::new() };
    let dir = base.join("deploys");
    names_in(&dir)
        .into_iter()
        .filter(|n| Path::new(n).extension().is_some_and(|e| e.eq_ignore_ascii_case("json")))
        .filter_map(|n| serde_json::from_slice::<Value>(&std::fs::read(dir.join(n)).ok()?).ok())
        .flat_map(|job| env_values(&job))
        .collect()
}

/// The `value` of each entry of a job's launch `env`.
fn env_values(job: &Value) -> Vec<String> {
    let entries = job.pointer("/launch/env").and_then(Value::as_array).cloned().unwrap_or_default();
    entries.iter().filter_map(|e| e.get("value").and_then(Value::as_str).map(str::to_owned)).collect()
}

/// A log file a bundle carries: its name in the bundle and its last lines.
pub(crate) struct LogFile {
    /// Its path in the bundle.
    pub name: String,
    /// Its end ([`LOG_TAIL_BYTES`]), from a whole line.
    pub text: String,
}

/// The newest log files of folder `dir` (`.log` files, at most [`LOG_FILES`]), each named
/// `<under>/<file name>` in the bundle.
pub(crate) fn logs(dir: &Path, under: &str) -> Vec<LogFile> {
    let mut files: Vec<(SystemTime, PathBuf, String)> = names_in(dir)
        .into_iter()
        .filter(|n| Path::new(n).extension().is_some_and(|e| e.eq_ignore_ascii_case("log")))
        .filter_map(|n| {
            let path = dir.join(&n);
            Some((std::fs::metadata(&path).and_then(|m| m.modified()).ok()?, path, n))
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.into_iter().take(LOG_FILES).filter_map(|(_, path, n)| Some(LogFile { name: format!("{under}/{n}"), text: tail(&path).ok()? })).collect()
}

/// The last [`LOG_TAIL_BYTES`] of file `path`, from the first whole line in them; bytes that
/// aren't UTF-8 replaced.
fn tail(path: &Path) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(LOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(if start == 0 { text } else { text.split_once('\n').map_or(text.clone(), |(_, rest)| rest.to_owned()) })
}
