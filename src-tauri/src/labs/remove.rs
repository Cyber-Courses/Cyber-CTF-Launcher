//! Removing a lab from this computer: the app's copy of it (the folder an import copied, or a
//! release unpacked) and every record the app keeps for it under its data folder: the runner's run
//! folder (its record, its event log, what it last saw, Vagrant's state of it), the deployments of
//! the launcher's own Terraform path, the deploy workers' job, logs and verdicts, an update's or an
//! import's leftover copies, and a catalogue lab's source kept for an offline start. The generated
//! project (Compose files, Vagrantfiles, run manifest) is inside the copy and goes with it.
//!
//! Nothing outside the app's data folder is touched. An imported lab's own folder, the one it was
//! imported from, is the player's: the copy keeps no link out of itself (`TreeLinks`), and a
//! removal deletes a link, never what it points at. Two things stay: the lab's lock file, which
//! every operation on the lab shares (deleting it would let two of them run at once), and the
//! images and boxes a start downloaded, which Storage removes once no lab uses them.
//!
//! Refused while anything of the lab is up, anywhere ([`runtime::status::still_up`]), parked
//! included, and while an operation runs on it: the run's manifest and what it bind-mounts are in
//! the copy, and its record is how the runner takes it down.

use std::path::{Path, PathBuf};

use super::{blocking, rename_retrying};
use crate::error::{Code, Error, Result};
use crate::platform::AppDirs;
use crate::platform::fs::{remove_dir_if_present, remove_if_present};
use crate::runtime::{self, paths};

/// What a removal left behind: each path it couldn't delete, and why. Empty when every trace went.
#[derive(Debug, Default, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabRemoval")]
pub struct Removal {
    /// The paths that are still there.
    pub left: Vec<Leftover>,
}

/// A path a removal couldn't delete.
#[derive(Debug, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabLeftover")]
pub struct Leftover {
    /// Where it is.
    pub path: String,
    /// Why it stayed, as the system says it.
    pub error: String,
}

/// Removes lab `id` from this computer (see the module's documentation). `worker_files` gives the
/// files of the lab's deploy worker once the lab is held, or refuses while a worker runs on it (the
/// app shell runs the workers). The lab's copy is moved aside first, all or nothing: a copy that
/// won't move (a file in it held open on Windows) refuses the removal with nothing deleted. What
/// can't be deleted after that is in the answer, the lab gone from the app all the same.
///
/// # Errors
///
/// An invalid id, a lab that isn't installed, one still up somewhere or busy, a copy that won't
/// move aside.
pub(crate) async fn remove(dirs: &AppDirs, id: &str, worker_files: impl FnOnce() -> Result<Vec<PathBuf>>) -> Result<Removal> {
    runtime::validate_id(id)?;
    // Held to the end: a start, a stop or a worker waits, and a new worker isn't started.
    let busy = || Error::coded(Code::AlreadyInProgress);
    if runtime::stopping(id) {
        return Err(busy());
    }
    let _lock = runtime::try_lock_lab(dirs, id)?.ok_or_else(busy)?;
    let dir = paths::lab_dir(dirs, id)?;
    if let Some(up) = runtime::status::still_up(dirs, id)? {
        return Err(runtime::status::still_running(&up));
    }
    // Parked by the launcher with the runner's record already gone (an older version's): its
    // machines are still there.
    if paths::parked(&dir).is_some() {
        return Err(Error::coded(Code::LabRunningHere));
    }
    let extra = worker_files()?;
    let (base, id) = (dirs.data()?.to_path_buf(), id.to_owned());
    blocking(move || remove_files(&base, &id, &extra, rename_retrying, delete)).await
}

/// The folder lab `id`'s copy is moved to under `labs` while it is deleted: hidden from every
/// listing of installed labs (a dot, which no lab id has).
fn aside(labs: &Path, id: &str) -> PathBuf {
    labs.join(format!(".{id}.removing"))
}

/// What the app keeps for lab `id` under its data folder `base`, beside its copy: an earlier
/// removal's, update's or import's leftover copy, the runner's run folder, the Terraform
/// deployments and the kept offline source.
fn traces(base: &Path, id: &str) -> Vec<PathBuf> {
    let labs = paths::labs_root_in(base);
    vec![
        labs.join(format!(".{id}.staging")),
        labs.join(format!(".{id}.old")),
        base.join("runs").join(runtime::compose_project(id)),
        base.join("deployments").join(id),
        super::offline::kept_path(base, id),
    ]
}

/// Moves lab `id`'s copy under `base` aside with `rename`, then deletes it and every trace of the
/// lab, `extra` too, with `remove`: what stays is in the answer. Nothing is deleted when the copy
/// doesn't move (a file in it held open): the lab stays whole.
fn remove_files(
    base: &Path,
    id: &str,
    extra: &[PathBuf],
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
    remove: impl Fn(&Path) -> std::io::Result<()>,
) -> Result<Removal> {
    let labs = paths::labs_root_in(base);
    let (dir, aside) = (labs.join(id), aside(&labs, id));
    // An earlier removal that left its copy: gone first, or the move below can't land.
    if let Err(e) = remove(&aside) {
        log::warn!("removing lab {id}: an earlier removal's {} stays: {e}", aside.display());
    }
    rename(&dir, &aside).map_err(|e| held(e, &dir))?;
    let mut left = Vec::new();
    for path in std::iter::once(aside).chain(traces(base, id)).chain(extra.iter().cloned()) {
        if let Err(e) = remove(&path) {
            log::warn!("removing lab {id}: {} stayed: {e}", path.display());
            left.push(leftover(&path, &e));
        }
    }
    Ok(Removal { left })
}

/// Deletes `path`, a folder with what is in it or a file; a link is deleted, never followed. One
/// already gone is fine.
fn delete(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => remove_dir_if_present(path),
        Ok(_) => remove_if_present(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// `path`, which stayed for `e`.
fn leftover(path: &Path, e: &std::io::Error) -> Leftover {
    Leftover { path: path.display().to_string(), error: e.to_string() }
}

/// The copy in `dir` that didn't move aside, said as what to do: nothing was deleted.
fn held(e: std::io::Error, dir: &Path) -> Error {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        Error::Refused(format!(
            "Couldn't remove the lab: a file in {} is open in another program (an editor, a terminal, or a VM). Close it and try again; nothing was deleted.",
            dir.display()
        ))
    } else {
        Error::Io(e)
    }
}

#[cfg(test)]
mod tests;
