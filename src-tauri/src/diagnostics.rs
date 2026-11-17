//! The diagnostics bundle: one file a player attaches to a bug report (Settings > About > Save
//! diagnostics, or the command line), a `.tar.gz` holding
//!
//! - `README.txt`: what the bundle is;
//! - `summary.json`: the launcher's version and the engines it bundles, the OS and its
//!   architecture, the settings that matter (the window's, and the policy, the update channel and
//!   the saved servers' kinds), the installed labs (id, slug, commit), the last operations (id,
//!   lab, what, outcome, failure code, when, as the Isothrum runner records each run's last), and
//!   what couldn't be read;
//! - `host-report.json`: the machine check (the Machine page's report: tools and versions,
//!   engines, hypervisors);
//! - `logs/`: the end of the newest log files (the app's: `cyberctf.log`);
//! - `runner/service.log`: the end of the log of the runner service, which runs the labs'
//!   operations;
//! - `operations/<lab>.json`: each lab's run's operations as the runner service journals them
//!   (what `cyberctf labs logs <lab> --json` lists).
//!
//! Every text in it passes through [`scrub::Scrubber`] (tokens, passwords, keys, flags, the values
//! each installed lab's run was launched with, e-mail addresses; the home folder written `~`), and
//! what would identify a host or an account (a server's address, a run's params) is never read into
//! it. It needs no window or Tauri app: the app's command and the command line build the same
//! bundle.

mod collect;
mod scrub;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::platform::AppDirs;

/// The bundle format's version (`summary.json`'s `bundle`).
const FORMAT: u32 = 1;

/// What a bundle is built from: the app's folders, its log folder, the user's home folder (written
/// `~`), and the settings the app's window keeps (none from the command line).
pub(crate) struct Sources {
    /// The app's folders.
    pub dirs: AppDirs,
    /// The app's log folder, when known.
    pub log_dir: Option<PathBuf>,
    /// The user's home folder, when known.
    pub home: Option<PathBuf>,
    /// The window's settings that matter for a report, by name.
    pub settings: BTreeMap<String, String>,
}

/// Checks this machine, then writes the bundle of `sources` to `out` (see the module).
///
/// # Errors
///
/// The file can't be written.
pub(crate) async fn export(sources: &Sources, out: &Path) -> Result<()> {
    let report = crate::machine::system::system_check(sources.dirs.clone()).await;
    let report = serde_json::to_value(report).map_err(|e| Error::Invalid(format!("the machine check can't be written down: {e}")))?;
    let runner = collect::runner_operations(&sources.dirs).await;
    write(sources, &report, runner, out)
}

/// Writes the bundle of `sources`, with the machine check `host_report` and the runs' operations
/// as the runner service lists them (`runner`), to `out`.
///
/// # Errors
///
/// The file can't be written.
pub(crate) fn write(sources: &Sources, host_report: &Value, runner: Result<collect::Runner>, out: &Path) -> Result<()> {
    let scrubber = scrub::Scrubber::new(sources.home.as_deref(), collect::launch_values(&sources.dirs));
    let collect::Runner { last, journals, mut problems } =
        runner.unwrap_or_else(|e| collect::Runner { problems: vec![format!("the runs' records: {e}")], ..collect::Runner::default() });
    let mut files = vec![
        ("README.txt".to_owned(), README.to_owned()),
        ("summary.json".to_owned(), pretty(&summary(sources, last, &mut problems))),
        ("host-report.json".to_owned(), pretty(host_report)),
    ];
    let logs = sources.log_dir.as_deref().map(|d| collect::logs(d, "logs")).unwrap_or_default();
    files.extend(logs.into_iter().chain(collect::service_log(&sources.dirs)).map(|f| (f.name, f.text)));
    files.extend(journals.into_iter().map(|j| (format!("operations/{}.json", j.lab), pretty(&j.operations))));
    let clean: Vec<(String, String)> = files.into_iter().map(|(name, text)| (name, scrubber.scrub(&text))).collect();
    let bytes = archive(&clean).map_err(|e| Error::Invalid(format!("the diagnostics bundle couldn't be built: {e}")))?;
    crate::platform::atomic_file::write_private(out, bytes)
        .map_err(|e| Error::Invalid(format!("the diagnostics bundle couldn't be saved to {}: {e}", out.display())))
}

/// What the bundle's `README.txt` says.
const README: &str = "Cyber CTF diagnostics\n\nAttach this file to your bug report or support request. It holds the launcher's version,\nthis computer's OS and the tools the machine check found, the installed labs, the last lab\noperations and the end of the launcher's logs and of its runner service's. Tokens,\npasswords, keys, flags and e-mail addresses were removed, and your home folder is written ~.\n";

/// `value` as indented JSON.
fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// `summary.json` (see the module), with the runs' last operations `last`: what couldn't be read
/// listed under `problems`, after those already there.
fn summary(sources: &Sources, last: Vec<collect::Operation>, problems: &mut Vec<String>) -> Value {
    let dirs = &sources.dirs;
    let operations = collect::newest(last, collect::OPERATIONS);
    let servers = collect::servers(dirs).unwrap_or_else(|e| {
        problems.push(format!("the saved servers: {e}"));
        Value::Null
    });
    let policy = crate::platform::policy::current();
    json!({
        "bundle": FORMAT,
        "createdAt": isothrum_runner::rfc3339(std::time::SystemTime::now()),
        "app": { "version": env!("CARGO_PKG_VERSION"), "isoloom": crate::version::ISOLOOM, "isothrum": crate::version::ISOTHRUM },
        "system": { "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "osVersion": sysinfo::System::long_os_version() },
        "settings": {
            "window": sources.settings,
            "policy": policy.on(),
            "updates": crate::app::updates::channel(policy),
            "servers": servers,
        },
        "labs": collect::installed_labs(dirs),
        "operations": operations,
        "problems": problems,
    })
}

/// `files` (path in the archive, text) as a gzipped tar.
fn archive(files: &[(String, String)]) -> std::io::Result<Vec<u8>> {
    let mtime = crate::platform::clock::unix_secs();
    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default()));
    for (name, text) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(text.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(mtime);
        header.set_cksum();
        tar.append_data(&mut header, format!("cyberctf-diagnostics/{name}"), text.as_bytes())?;
    }
    tar.into_inner()?.finish()
}
