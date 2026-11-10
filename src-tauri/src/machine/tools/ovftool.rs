//! VMware's OVF Tool (`ovftool`), which the ESXi Vagrant plugin uploads lab VMs with. It isn't
//! installed the way the other tools are: it comes inside VMware Fusion (Workstation on Windows),
//! or as a standalone download from Broadcom, which unpacks to a loose folder nothing looks in
//! (and, on macOS, quarantined). [`place`] copies such a folder, once verified as VMware's, into
//! a folder of the app's own ([`managed_dir`]), which is on the app's `PATH` from startup.
//!
//! [`locate`] finds the one the machine check reports, in this order: the hypervisor's own copy,
//! then the app's, then the first on `PATH`. The app's `PATH` lists the hypervisor's folder
//! before the app's (`platform::env_path`), so a lab's run (Vagrant, and the Isothrum runner's
//! ovftool wrapper, which hands over to the first `ovftool` on `PATH`) uses the same one, unless
//! another copy sits in a folder already on the user's own `PATH`.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::machine::system::Tool;
use crate::platform::exec::run_read_env;
use crate::platform::policy::{self, Key};
use crate::platform::remembered::{forget_all, remembered};
use crate::platform::{AppDirs, random_hex};

#[cfg(test)]
mod tests;

/// The tool's file name on this OS.
const EXE: &str = if cfg!(windows) { "ovftool.exe" } else { "ovftool" };

/// The team identifier of VMware's Apple Developer ID (Broadcom still signs with it).
const VMWARE_TEAM: &str = "EG7KH642X6";

/// The most a picked folder may hold: the tool is about 60 files and 60 MB, so anything far
/// bigger is the wrong folder (a whole Downloads folder holding it), not a copy to make.
const MAX_FILES: usize = 2_000;
const MAX_BYTES: u64 = 1 << 30;

/// A placement left behind by a crash (a copy being made, or one being replaced) is removed once
/// it is this old: younger, it may be another placement still running.
const LEFTOVER_AFTER: Duration = Duration::from_secs(600);

/// Where the OVF Tool the machine check found comes from.
#[derive(Serialize, ts_rs::TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "OvftoolSource")]
pub enum Source {
    /// Inside VMware Fusion (macOS).
    Fusion,
    /// Inside VMware Workstation (Windows).
    Workstation,
    /// The copy Cyber CTF placed in its own folder ([`place`]).
    Managed,
    /// The first on `PATH`.
    Path,
}

/// The OVF Tool the machine check found: where it comes from and its file.
#[derive(Serialize, ts_rs::TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "OvftoolLocation")]
pub struct Location {
    pub source: Source,
    pub path: String,
    /// It is built for Intel Macs only, this Mac has Apple Silicon and Rosetta isn't installed:
    /// it can't run until Rosetta is (`softwareupdate --install-rosetta`).
    pub needs_rosetta: bool,
}

/// Why a picked folder wasn't placed.
#[derive(Serialize, ts_rs::TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "OvftoolRefusal")]
pub enum Refusal {
    /// No `ovftool` program in the folder (nor in the one folder inside it).
    NoTool,
    /// The program is there, but not its libraries (`lib/` beside it): a partial copy.
    NoLibraries,
    /// The folder holds far more than the tool: the wrong folder.
    TooLarge,
    /// macOS: a program or library in it isn't validly signed.
    Unsigned,
    /// macOS: it is signed, but not by VMware.
    NotVmware,
}

/// What [`place`] did with the picked folder.
#[derive(Serialize, ts_rs::TS, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(rename = "OvftoolPlaced")]
pub enum Outcome {
    /// Copied into the app's folder, where the next machine check finds it; `path` is the program.
    Placed { path: String, needs_rosetta: bool },
    /// Not placed, for `reason`; `detail` is what macOS said of the signature, or the team that
    /// signed it instead of VMware, when known.
    Refused { reason: Refusal, detail: Option<String> },
}

impl Outcome {
    fn refused(reason: Refusal, detail: Option<String>) -> Outcome {
        Outcome::Refused { reason, detail }
    }
}

/// The folder Cyber CTF places the OVF Tool in, under the app data folder `data`.
pub fn managed_dir(data: &Path) -> PathBuf {
    data.join("tools").join("ovftool")
}

/// The copies VMware's hypervisors install, each with its source, from the environment `var`
/// reads (the Program Files folders on Windows).
fn bundled(var: impl Fn(&str) -> Option<OsString>) -> Vec<(Source, PathBuf)> {
    let mut found = Vec::new();
    if cfg!(target_os = "macos") {
        found.push((Source::Fusion, PathBuf::from("/Applications/VMware Fusion.app/Contents/Library/VMware OVF Tool").join(EXE)));
    }
    if cfg!(windows) {
        // Workstation installs under the 32-bit Program Files; the newest under the 64-bit one.
        for pf in ["ProgramFiles(x86)", "ProgramFiles"].into_iter().filter_map(&var) {
            found.push((Source::Workstation, workstation_dir(Path::new(&pf)).join(EXE)));
        }
    }
    found
}

/// The OVF Tool folder inside VMware Workstation, installed under Program Files folder `pf`.
pub fn workstation_dir(pf: &Path) -> PathBuf {
    pf.join("VMware").join("VMware Workstation").join("OVFTool")
}

/// The OVF Tool to report, in order: a hypervisor's own copy (`bundled`), the app's (under
/// `data`), then the first on `path`. None when there is none.
fn locate_in(bundled: Vec<(Source, PathBuf)>, data: Option<&Path>, path: Option<&OsStr>) -> Option<(Source, PathBuf)> {
    let managed = data.map(|d| (Source::Managed, managed_dir(d).join(EXE)));
    let on_path = path.and_then(|p| std::env::split_paths(p).map(|d| d.join(EXE)).find(|f| f.is_file())).map(|f| (Source::Path, f));
    bundled.into_iter().chain(managed).find(|(_, f)| f.is_file()).or(on_path)
}

/// [`locate_in`] on this machine: its hypervisors, the app data folder `data`, and the `PATH`.
pub fn locate(data: Option<&Path>) -> Option<(Source, PathBuf)> {
    locate_in(bundled(|name| std::env::var_os(name)), data, std::env::var_os("PATH").as_deref())
}

/// The machine check's answer for the OVF Tool: its version (when `--version` runs) and where it
/// was found. Its `--version` is kept a while, by file, like the other tools'.
pub async fn probe(data: Option<&Path>) -> (Tool, Option<Location>) {
    let Some((source, file)) = locate(data) else {
        return (Tool { installed: false, version: None }, None);
    };
    let tool = version_of(&file).await;
    let location = Location { source, path: file.display().to_string(), needs_rosetta: needs_rosetta(&file) };
    (tool, Some(location))
}

/// `ovftool --version` of the program `file`: run by name with its folder first on `PATH` (the
/// exec layer starts tools by name), so it is that file that answers.
async fn version_of(file: &Path) -> Tool {
    let dir = file.parent().unwrap_or(file);
    let current = std::env::var_os("PATH");
    let folders = std::iter::once(dir.to_path_buf()).chain(current.iter().flat_map(std::env::split_paths));
    let path = std::env::join_paths(folders).unwrap_or_else(|_| dir.as_os_str().to_owned());
    let env = [("PATH".to_owned(), path.to_string_lossy().into_owned())];
    let ask = run_read_env("ovftool", &["--version"], None, &env);
    // Boxed: kept in the machine check's future, it would double its size.
    match Box::pin(remembered(&format!("{} --version", file.display()), ask)).await {
        Ok(out) => Tool { installed: true, version: crate::machine::system::first_line(&out) },
        Err(_) => Tool { installed: false, version: None },
    }
}

/// Places the OVF Tool unpacked at `picked` (the folder holding `ovftool`, or the one folder
/// inside it that does) in the app's own folder, replacing an earlier copy: it is copied first,
/// the copy checked (on macOS, signed by VMware), and only then swapped in. The picked folder is
/// only read.
///
/// # Errors
///
/// When this computer's policy turns tool installs off, the app data folder is unknown, or the
/// copy can't be made or swapped in (a full disk, a permission). A folder that isn't VMware's
/// OVF Tool is an [`Outcome::Refused`], not an error.
pub async fn place(dirs: &AppDirs, picked: &Path) -> Result<Outcome> {
    policy::current().check(Key::DisableToolInstalls)?;
    let dest = managed_dir(dirs.data()?);
    let Some(root) = tool_root(picked) else {
        return Ok(Outcome::refused(Refusal::NoTool, None));
    };
    if !has_libraries(&root) {
        return Ok(Outcome::refused(Refusal::NoLibraries, None));
    }
    let parent = dest.parent().map(Path::to_path_buf).unwrap_or_default();
    std::fs::create_dir_all(&parent)?;
    remove_leftovers(&parent);
    // Beside the destination, so swapping it in is a rename on one file system.
    let staged = parent.join(format!(".ovftool-new-{}", random_hex(6)));
    let copied = {
        let (root, staged) = (root.clone(), staged.clone());
        tokio::task::spawn_blocking(move || copy_tree(&root, &staged, &mut Budget::default()))
            .await
            .map_err(|e| Error::Invalid(format!("copying the OVF Tool stopped: {e}")))?
    };
    let outcome = match copied {
        Ok(()) => finish(&staged, &dest).await,
        Err(e) if e.kind() == std::io::ErrorKind::FileTooLarge => Ok(Outcome::refused(Refusal::TooLarge, None)),
        Err(e) => Err(e.into()),
    };
    // Whatever happened, no copy stays half made: once swapped in, there is nothing left here.
    if staged.exists() {
        let _ = std::fs::remove_dir_all(&staged);
    }
    if matches!(outcome, Ok(Outcome::Placed { .. })) {
        // The next check asks the new copy, not a kept answer of the old one.
        forget_all();
        log::info!("placed the OVF Tool from {} in {}", root.display(), dest.display());
    }
    outcome
}

/// Checks the copy `staged` and swaps it in at `dest`: the checks run on the copy, so what was
/// checked is exactly what is placed.
async fn finish(staged: &Path, dest: &Path) -> Result<Outcome> {
    let exe = staged.join(EXE);
    if !is_program(&exe) {
        return Ok(Outcome::refused(Refusal::NoTool, None));
    }
    // macOS only: Windows and Linux carry no such signature check, nor a quarantine.
    if cfg!(target_os = "macos") {
        if let Some((reason, detail)) = signature_refusal(staged).await? {
            return Ok(Outcome::refused(reason, detail));
        }
        lift_quarantine(staged).await?;
    }
    swap_in(staged, dest)?;
    let placed = dest.join(EXE);
    Ok(Outcome::Placed { path: placed.display().to_string(), needs_rosetta: needs_rosetta(&placed) })
}

/// The folder holding the tool: `picked` itself, or the one folder inside it that holds it (the
/// Windows and Linux downloads unpack into an `ovftool` folder).
fn tool_root(picked: &Path) -> Option<PathBuf> {
    if picked.join(EXE).is_file() {
        return Some(picked.to_path_buf());
    }
    let inside: Vec<PathBuf> = std::fs::read_dir(picked).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir() && p.join(EXE).is_file()).collect();
    match inside.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    }
}

/// Whether the tool at `root` has its libraries: a `lib/` folder beside it (macOS, and the
/// newer Linux downloads), or, on Windows and Linux, the libraries themselves beside it.
fn has_libraries(root: &Path) -> bool {
    if root.join("lib").is_dir() {
        return true;
    }
    let library = |name: &str| if cfg!(windows) { name.to_ascii_lowercase().ends_with(".dll") } else { cfg!(target_os = "linux") && name.contains(".so") };
    std::fs::read_dir(root).is_ok_and(|entries| entries.flatten().any(|e| library(&e.file_name().to_string_lossy())))
}

/// Whether `file` is a program this OS will start: a file, executable on Unix.
fn is_program(file: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(file) else { return false };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

/// What a copy may still take, so a wrong folder (a whole Downloads) isn't copied whole.
struct Budget {
    files: usize,
    bytes: u64,
}

impl Default for Budget {
    fn default() -> Budget {
        Budget { files: MAX_FILES, bytes: MAX_BYTES }
    }
}

impl Budget {
    /// Takes one entry of `len` bytes; `FileTooLarge` once the budget is spent.
    fn take(&mut self, len: u64) -> std::io::Result<()> {
        if self.files == 0 || len > self.bytes {
            return Err(std::io::Error::new(std::io::ErrorKind::FileTooLarge, "the folder holds far more than the OVF Tool"));
        }
        self.files -= 1;
        self.bytes -= len;
        Ok(())
    }
}

/// Copies the folder `from` to `to` (a new folder), its files with their permissions. A symbolic
/// link is copied as one when it points inside its own folder (the versioned names of a Linux
/// library); any other is left out.
fn copy_tree(from: &Path, to: &Path, budget: &mut Budget) -> std::io::Result<()> {
    std::fs::create_dir(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            budget.take(0)?;
            copy_tree(&src, &dst, budget)?;
        } else if kind.is_file() {
            budget.take(entry.metadata()?.len())?;
            std::fs::copy(&src, &dst)?;
        } else if kind.is_symlink() {
            budget.take(0)?;
            copy_link(&src, &dst)?;
        }
    }
    Ok(())
}

/// Copies the symbolic link `src` as `dst` when its target stays beside it (no `..`, not
/// absolute); otherwise leaves it out.
fn copy_link(src: &Path, dst: &Path) -> std::io::Result<()> {
    let target = std::fs::read_link(src)?;
    let stays_inside = target.components().all(|c| matches!(c, Component::Normal(_)));
    if !stays_inside {
        log::warn!("left out {} from the OVF Tool's copy: it links outside its folder", src.display());
        return Ok(());
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, dst)
    }
    #[cfg(not(unix))]
    {
        let _ = dst;
        log::warn!("left out the link {} from the OVF Tool's copy", src.display());
        Ok(())
    }
}

/// Swaps the folder `staged` in at `dest`: the earlier copy is moved aside, the new one renamed
/// in its place (one rename each, on one file system), then the earlier one removed. If the new
/// one can't go in, the earlier one is put back.
fn swap_in(staged: &Path, dest: &Path) -> std::io::Result<()> {
    let aside = dest.with_file_name(format!(".ovftool-old-{}", random_hex(6)));
    let had = match std::fs::rename(dest, &aside) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    if let Err(e) = std::fs::rename(staged, dest) {
        if had {
            let _ = std::fs::rename(&aside, dest);
        }
        return Err(e);
    }
    // A copy still running (Windows keeps its files locked) is removed with the next leftovers.
    if had && let Err(e) = std::fs::remove_dir_all(&aside) {
        log::warn!("couldn't remove the earlier OVF Tool at {}: {e}", aside.display());
    }
    Ok(())
}

/// Removes, from `parent`, the copies an interrupted placement left behind (being made, or being
/// replaced), once they are old enough not to be another placement's still running.
fn remove_leftovers(parent: &Path) {
    let Ok(entries) = std::fs::read_dir(parent) else { return };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let old = entry.metadata().and_then(|m| m.modified()).is_ok_and(|at| now.duration_since(at).unwrap_or_default() >= LEFTOVER_AFTER);
        if name.to_string_lossy().starts_with(".ovftool-") && old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// macOS: why the copy `staged` isn't placed, when one of its programs or libraries isn't
/// validly signed by VMware's team. Every Mach-O file in it is checked, so the quarantine lifted
/// next is lifted from VMware's code only.
///
/// `codesign --verify --strict` checks the signature itself; with `-R`, that it chains to Apple
/// and names VMware's team in its leaf certificate, which only Apple can issue. That is what
/// decides; `codesign -dv` says the same team on stderr, which a tool's answer here doesn't keep,
/// so the team a refusal names comes from the signature's designated requirement (`-d -r-`).
async fn signature_refusal(staged: &Path) -> Result<Option<(Refusal, Option<String>)>> {
    const CODESIGN: &str = "/usr/bin/codesign";
    let requirement = format!("-R=anchor apple generic and certificate leaf[subject.OU] = \"{VMWARE_TEAM}\"");
    for file in mach_o_files(staged) {
        let shown = file.strip_prefix(staged).unwrap_or(&file).display().to_string();
        let path = text(&file)?;
        match crate::platform::exec::run_read(CODESIGN, &["--verify", "--strict", path], None).await {
            Ok(_) => {}
            Err(Error::CommandFailed { stderr, .. }) => return Ok(Some((Refusal::Unsigned, Some(format!("{shown}: {}", stderr.trim()))))),
            Err(e) => return Err(e),
        }
        match crate::platform::exec::run_read(CODESIGN, &["--verify", "--strict", &requirement, path], None).await {
            Ok(_) => {}
            Err(Error::CommandFailed { .. }) => {
                let team = crate::platform::exec::run_read(CODESIGN, &["-d", "-r-", path], None).await.ok().and_then(|dr| signing_team(&dr));
                return Ok(Some((Refusal::NotVmware, team)));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(None)
}

/// The team a designated requirement (`codesign -d -r-`) names: its leaf certificate's
/// organizational unit.
fn signing_team(requirement: &str) -> Option<String> {
    let (_, after) = requirement.split_once("leaf[subject.OU] = ")?;
    let team: String = after.trim_start_matches('"').chars().take_while(char::is_ascii_alphanumeric).collect();
    (!team.is_empty()).then_some(team)
}

/// `path` as the text a tool's argument is.
fn text(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| Error::Invalid(format!("the path {} isn't UTF-8", path.display())))
}

/// Every Mach-O file (a program or library) under `dir`, by its first bytes.
fn mach_o_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return found };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            found.extend(mach_o_files(&entry.path()));
        } else if kind.is_file() && head(&entry.path()).is_some_and(|h| is_mach_o(&h)) {
            found.push(entry.path());
        }
    }
    found.sort();
    found
}

/// macOS: lifts the quarantine from the copy `staged`, so Gatekeeper lets it run.
///
/// Only once [`signature_refusal`] passed on that very copy, and only on the copy: the player
/// chose this folder with the "Use a downloaded OVF Tool" button, and every program and library
/// in it is verified as signed by VMware's Developer ID team. The tool isn't notarized (no
/// hardened runtime), so with the quarantine on, Gatekeeper refuses to start it; with it lifted,
/// it runs as VMware signed it. The player's own download keeps its quarantine.
async fn lift_quarantine(staged: &Path) -> Result<()> {
    crate::platform::exec::run("/usr/bin/xattr", &["-d", "-r", "com.apple.quarantine", text(staged)?], None).await.map(drop)
}

/// The CPU kinds a Mach-O file is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cpu {
    X86_64,
    Arm64,
    Other,
}

/// The first bytes of `file`, enough for a Mach-O header and a universal binary's list.
fn head(file: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::with_capacity(512);
    std::fs::File::open(file).ok()?.take(512).read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn word(bytes: &[u8], at: usize, big_endian: bool) -> Option<u32> {
    let b: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
    Some(if big_endian { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
}

const MH_MAGIC_64: u32 = 0xfeed_facf;
const MH_MAGIC: u32 = 0xfeed_face;
const FAT_MAGIC: u32 = 0xcafe_babe;
const FAT_MAGIC_64: u32 = 0xcafe_babf;

fn is_mach_o(head: &[u8]) -> bool {
    word(head, 0, false).is_some_and(|m| m == MH_MAGIC_64 || m == MH_MAGIC) || word(head, 0, true).is_some_and(|m| m == FAT_MAGIC || m == FAT_MAGIC_64)
}

fn cpu(kind: u32) -> Cpu {
    match kind {
        0x0100_0007 => Cpu::X86_64,
        0x0100_000c => Cpu::Arm64,
        _ => Cpu::Other,
    }
}

/// The CPUs the Mach-O file whose first bytes are `head` runs on: one for a thin file, each of a
/// universal binary's. Empty for anything else.
fn cpus(head: &[u8]) -> Vec<Cpu> {
    match (word(head, 0, false), word(head, 0, true)) {
        (Some(MH_MAGIC_64 | MH_MAGIC), _) => word(head, 4, false).map(cpu).into_iter().collect(),
        (_, Some(magic @ (FAT_MAGIC | FAT_MAGIC_64))) => {
            let entry = if magic == FAT_MAGIC { 20 } else { 32 };
            let count = word(head, 4, true).unwrap_or(0) as usize;
            (0..count.min(16)).filter_map(|i| word(head, 8 + i * entry, true)).map(cpu).collect()
        }
        _ => Vec::new(),
    }
}

/// Whether a program built for `cpus` runs on an Apple Silicon Mac only through Rosetta.
fn intel_only(cpus: &[Cpu]) -> bool {
    !cpus.is_empty() && cpus.iter().all(|c| *c == Cpu::X86_64)
}

/// Whether Rosetta 2 is installed (its runtime is there).
fn rosetta_installed() -> bool {
    Path::new("/Library/Apple/usr/libexec/oah/libRosettaRuntime").exists()
}

/// Whether the program `file` can't run here until Rosetta is installed: it is built for Intel
/// Macs only (the standalone OVF Tool is), this Mac has Apple Silicon, and Rosetta isn't there.
fn needs_rosetta(file: &Path) -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64")) && !rosetta_installed() && head(file).is_some_and(|h| intel_only(&cpus(&h)))
}
