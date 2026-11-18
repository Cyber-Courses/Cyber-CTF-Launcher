//! VMware's OVF Tool (`ovftool`), which the ESXi Vagrant plugin uploads lab VMs with: found, asked
//! its version and placed by isothrum-machine ([`isothrum_machine::tools::ovftool`]). The launcher
//! keeps the folder a picked download is copied into ([`managed_dir`]), on the app's `PATH` from
//! startup, so a lab's run (Vagrant, and the Isothrum runner's ovftool wrapper, which hands over
//! to the first `ovftool` on `PATH`) uses the same copy; the administrator's policy on tool
//! installs; and the tool's `--version` kept a while by the machine check's runner
//! (`platform::remembered`), forgotten once a copy is placed.

use std::path::{Path, PathBuf};

use isothrum_machine::Context;
use isothrum_machine::tools::Tool;
use isothrum_machine::tools::ovftool::{self, PlaceError};
pub use isothrum_machine::tools::ovftool::{Location, Outcome};

use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::platform::policy::{self, Key};
use crate::runtime::providers;

#[cfg(test)]
mod tests;

/// The folder Cyber CTF places the OVF Tool in, under the app data folder `data`.
pub fn managed_dir(data: &Path) -> PathBuf {
    data.join("tools").join("ovftool")
}

/// The machine check's answer for the OVF Tool, asked with `cx`: its version (when `--version`
/// runs) and where it was found, a hypervisor's own copy first, then the app's (under `data`),
/// then the first on the `PATH` ([`providers::search_path`]).
pub async fn probe(cx: &Context, data: Option<&Path>) -> (Tool, Option<Location>) {
    let managed = data.map(managed_dir);
    ovftool::probe(cx, managed.as_deref(), providers::search_path().as_deref()).await
}

/// The OVF Tool a lab on ESXi uploads its VMs with, as the machine check finds it ([`probe`]):
/// its version, or why it can't be used (`data` is the app data folder). The ESXi server test
/// checks it before it runs anything on the host.
pub async fn usable(data: Option<PathBuf>) -> std::result::Result<String, String> {
    let (tool, location) = probe(&providers::machine_check(), data.as_deref()).await;
    ovftool::usable(tool, location.as_ref())
}

/// Places the OVF Tool unpacked at `picked` (the folder holding `ovftool`, or the one folder
/// inside it that does) in the app's own folder, replacing an earlier copy, once verified as
/// VMware's ([`ovftool::place`]). The picked folder is only read.
///
/// # Errors
///
/// When this computer's policy turns tool installs off, the app data folder is unknown, or the
/// copy can't be made or swapped in (a full disk, a permission). A folder that isn't VMware's
/// OVF Tool is an [`Outcome::Refused`], not an error.
pub async fn place(dirs: &AppDirs, picked: &Path) -> Result<Outcome> {
    policy::current().check(Key::DisableToolInstalls)?;
    let dest = managed_dir(dirs.data()?);
    let outcome = ovftool::place(&providers::machine(), picked, &dest).await.map_err(|e| match e {
        PlaceError::Io(e) => Error::Io(e),
        e => Error::Invalid(e.to_string()),
    })?;
    if matches!(outcome, Outcome::Placed { .. }) {
        // The next check asks the new copy, not a kept answer of the old one.
        crate::platform::remembered::forget_all();
        log::info!("placed the OVF Tool from {} in {}", picked.display(), dest.display());
    }
    Ok(outcome)
}
