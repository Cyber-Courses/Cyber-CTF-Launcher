//! A lab's Isoloom side: every lab describes itself with an `isoloom.yml` at its root, and the
//! launcher generates the files of the target it runs on under the lab's output folder
//! (Compose, Vagrant, Terraform) right before running them. Nothing is read from the lab
//! besides the spec and the files it names, so a lab is portable to every target Isoloom
//! supports.
//!
//! Each installed lab is an Isoloom *instance* (a number, kept in `.cyberctf-instance`): its
//! Docker networks move to their own blocks and its names get a suffix, so two labs that
//! declare the same addresses run at once. The generated files then live in `.isoloom-<n>/`.
//!
//! This module is the launcher's one door to Isoloom's spec and generators (`isoloom-spec`,
//! `isoloom-targets`: used nowhere else, their types re-exported from here): the spec, the
//! generated files, the resolved snapshot and the checks go through it, and so do Isoloom's path and naming conventions (`names`). The native
//! files those generators write (a Compose file, a Vagrantfile, a Terraform module) are what the
//! Isothrum runner opens, by their folder. The launcher's own conventions sit on top:
//! its overrides of the generated files (`overrides`, applied after Isoloom's defaults) and the
//! variables it gives Isoloom's Terraform modules from its hosts and cloud accounts (`vars`).
//! Isoloom only converts: it keeps no registry of running labs and probes no host, so neither
//! does this module.

use std::path::Path;

use super::Interface;
use crate::error::{Error, Result};

mod instances;
mod names;
pub(crate) mod overlay;
mod overrides;
mod run_options;
mod vars;

pub use instances::*;
pub use names::*;
#[cfg(test)]
pub use run_options::Refusal;
pub use run_options::{RefusalCause, RunOptions, RunPath, emulated_arches, run_options};
pub use vars::{NODE_KEY, TLS_PIN, esxi_env, host_vars};

// What the rest of the launcher uses of Isoloom's types: through here, like its functions.
pub use isoloom_spec::model::{Arch, Machine, Spec, Target};
pub use isoloom_targets::Lab;

/// The lab in `dir` as its author wrote it, checked (its fields, its lowered form's and the files
/// it names), with this machine's Isoloom defaults' image table and cloud regions.
pub fn lab(dir: &Path) -> Result<Lab> {
    // With the launcher's additions (`overlay`) merged in, when it made any.
    let source = overlay::spec_text(dir)?.and_then(|text| isoloom_spec::parse(&text)).map_err(|e| Error::Invalid(load_error(&e)))?;
    let d = defaults(dir)?;
    let lab = Lab::new(source).with_images(d.images.clone()).with_regions(d.regions()).with_host_access(granted());
    let problems: Vec<String> = lab.problems_with_files(dir).iter().map(|p| p.render(isoloom_spec::SPEC_FILE)).collect();
    if !problems.is_empty() {
        return Err(Error::Invalid(format!("this lab's isoloom.yml has mistakes:\n{}", problems.join("\n"))));
    }
    Ok(lab)
}

/// What of this computer a lab may reach beyond its own machines, which Isoloom refuses unless
/// its runner allows each: what the launcher's labs have always reached. Folders of this computer
/// (the attack box's shared folder, which it writes in), the container engine's socket and a
/// privileged container, the last one turned off by the `RefusePrivilegedLabs` policy with the
/// launcher's own words (`launch::policy_allows`). A VM booted from this computer's files, its
/// devices and a network bridged to its own stay refused: no lab of the launcher's uses them.
fn granted() -> isoloom_spec::host_access::Grants {
    use isoloom_spec::host_access::{Grants, HostAccess};
    Grants::of([HostAccess::HostFolders, HostAccess::WritableHostFolders, HostAccess::DockerSocket, HostAccess::Privileged])
}

/// The lab in `dir` resolved as its instance runs it (Isoloom's snapshot: each machine's addresses,
/// the networks), as JSON.
pub fn resolved(dir: &Path) -> Result<serde_json::Value> {
    isoloom_targets::resolved::resolve_with(&lab(dir)?, instance(dir)).map_err(|e| Error::Invalid(e.to_string()))
}

/// The lab's message (how to start, in Markdown) as it reads on `target`, its placeholders filled
/// from its spec; none when it has none.
pub fn message_on(dir: &Path, target: Target) -> Result<Option<String>> {
    isoloom_targets::resolved::render_message_on(&lab(dir)?, instance(dir), target).map_err(|e| Error::Invalid(e.to_string()))
}

/// The VM a VM lab's checks that stand on its networks run in: Isoloom's controller, which halts
/// once it has set the lab up.
pub const CONTROLLER: &str = isoloom_spec::contract::CONTROLLER;

/// A spec as written, unchecked (what a launch dialog previews): `isoloom.yml` in `dir`.
pub fn load(dir: &Path) -> Result<Spec> {
    overlay::spec_text(dir)?
        .and_then(|text| isoloom_spec::parse(&text))
        .map(|s| Lab::new(s).spec())
        .map_err(|e| Error::Invalid(format!("this lab's isoloom.yml: {e}")))
}

/// Whether the lab has a machine with kernel-level access (`privileged: true`): a container that
/// can reach past its own isolation into the host's kernel.
pub fn is_privileged(spec: &Spec) -> bool {
    spec.machines.values().any(|m| m.privileged)
}

/// The launch values the lab's spec marks `sensitive: false` (a URL, a mode), by name: the one
/// rule for a value's sensitivity. These reach the lab's tools as plain values and stay readable
/// in the logs; every other value (a sensitive input, or one the spec doesn't declare) is a
/// secret, never stored and masked wherever a tool prints it.
pub fn plain_inputs(spec: &Spec) -> Vec<String> {
    spec.inputs.iter().filter(|i| !i.sensitive).map(|i| i.name.clone()).collect()
}

/// A spec from its text, unchecked.
#[cfg(test)]
pub fn parse(text: &str) -> Result<Spec> {
    isoloom_spec::parse(text).map(|s| Lab::new(s).spec()).map_err(|e| Error::Invalid(format!("this lab's isoloom.yml: {e}")))
}

/// Copies a lab checkout `src` into `dst`, leaving out generated output and VCS/VM state
/// (`.isoloom`, `.git`, `.vagrant`). Used to install a lab imported from a local directory, and in
/// tests to stage a corpus lab. Copied in Rust, not with `cp -R`, so it runs on Windows too.
pub(crate) fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let mut links = crate::platform::tree_link::TreeLinks::default();
    copy_tree_rooted(src, dst, src, &mut links)?;
    links.recreate(dst)
}

/// [`copy_tree`], carrying the tree root so a symlink's path can be judged against it, and the
/// links found, recreated once the whole copy is in place.
fn copy_tree_rooted(src: &Path, dst: &Path, root: &Path, links: &mut crate::platform::tree_link::TreeLinks) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if [super::isothrum::OUTPUT_DIR, ".git", ".vagrant"].iter().any(|skip| name == **skip) {
            continue;
        }
        let (from, to) = (entry.path(), dst.join(&name));
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            // An internal symlink a lab relies on (e.g. client/src/contracts -> ../../contracts),
            // recreated as the GitHub-tarball install does: never one pointing outside the copy.
            let rel = from.strip_prefix(root).unwrap_or(from.as_path());
            links.push(rel, &std::fs::read_link(&from)?);
            continue;
        }
        if kind.is_dir() {
            copy_tree_rooted(&from, &to, root, links)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// A spec this build can't read. A key the format doesn't have means the lab uses a newer format
/// than this launcher (labs adopt spec additions before every player has updated), so say what
/// to do instead of showing only Isoloom's problems. A missing or unreadable file, or a mistake
/// of the lab's own, is shown as Isoloom words it.
fn load_error(e: &isoloom_spec::Error) -> String {
    let newer = matches!(e, isoloom_spec::Error::Invalid { problems, .. } if problems.iter().any(|p| p.code == isoloom_spec::syntax::Code::UnknownField));
    if newer {
        format!("This lab needs a newer version of Cyber CTF. Update it (Settings > About), then start the lab again.\n(isoloom.yml: {e})")
    } else {
        format!("this lab's isoloom.yml: {e}")
    }
}

/// This machine's Isoloom defaults for the lab: the user's `~/.isoloom/defaults.yml`, the
/// lab's own `isoloom.defaults.yml`, then `ISOLOOM_*` in the environment (the same hierarchy
/// the `isoloom` CLI reads), so a user's image table or cloud regions hold here too.
fn defaults(dir: &Path) -> Result<isoloom_spec::defaults::Defaults> {
    let sources = isoloom_spec::defaults::Sources { project: dir, images_file: None, profile: None, sets: &[], host: host() };
    isoloom_spec::defaults::load(&sources).map(|r| r.defaults).map_err(|e| Error::Invalid(format!("this machine's Isoloom defaults: {e}")))
}

/// This machine as Isoloom reads it: its home and the `ISOLOOM_*` variables.
fn host() -> isoloom_spec::host::Host {
    isoloom_spec::host::Host::new(std::env::vars_os())
}

/// `spec` as instance `n` of it (unchanged without one).
fn as_instance(spec: Spec, n: Option<u8>) -> Result<Spec> {
    match n {
        Some(n) => isoloom_spec::instance::apply(&spec, n).map_err(|e| Error::Invalid(format!("this lab as instance {n}: {e}"))),
        None => Ok(spec),
    }
}

/// An observer the lab puts beside itself (`tools:` in its spec): a toolbox or a capture box
/// on every network, outside the lab's contract.
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabTool")]
pub struct Observer {
    /// Its name in the lab's spec.
    pub name: String,
    /// The container image it runs from, when the spec gives one.
    pub image: Option<String>,
    /// Its address on each lab network, where the lab runs.
    pub addresses: Vec<Interface>,
    /// The loopback port its web UI is published on, when it has one.
    pub publish: Option<u16>,
}

/// The observers of the lab in `dir` (as its instance) started in `form`, at their addresses: the
/// container ones when it runs as containers, the ones the spec writes when it runs as VMs, and
/// none beside cloud services.
pub fn tools(dir: &Path, form: super::Runtime) -> Result<Vec<Observer>> {
    let key = match form {
        super::Runtime::Cloud => return Ok(Vec::new()),
        super::Runtime::Docker => "docker_addresses",
        super::Runtime::Vm => "addresses",
    };
    let snapshot = isoloom_targets::resolved::resolve_with(&lab(dir)?, instance(dir)).map_err(|e| Error::Invalid(e.to_string()))?;
    Ok(snapshot["tools"]
        .as_object()
        .map(|tools| {
            tools
                .iter()
                .map(|(name, t)| Observer {
                    name: name.clone(),
                    image: t["image"].as_str().map(str::to_string),
                    addresses: interfaces(&t[key]),
                    publish: t["publish"].as_u64().and_then(|p| u16::try_from(p).ok()),
                })
                .collect()
        })
        .unwrap_or_default())
}

/// A snapshot's `{ network: address }` as interfaces.
fn interfaces(addresses: &serde_json::Value) -> Vec<Interface> {
    addresses
        .as_object()
        .map(|a| a.iter().filter_map(|(net, ip)| ip.as_str().map(|ip| Interface { network: net.clone(), ip: ip.to_string() })).collect())
        .unwrap_or_default()
}

/// A lab ready to run on one target: its spec as written (what Isoloom resolves its message and
/// refusals from) and as the target runs it, this lab's instance of it.
#[derive(Debug)]
pub struct Prepared {
    /// The lab as its author wrote it (boxed: an authored spec is large, and a start holds it
    /// across its awaits).
    pub lab: Box<Lab>,
    /// The spec as the target runs it.
    pub spec: Spec,
}

/// Generates `target`'s files into the lab (overwriting the previous ones), and returns the lab
/// ready to run there. Fails with Isoloom's own reason when the lab can't run there (e.g.
/// Windows machines on a target without Windows images). The launcher's own overrides (see
/// `overrides`) are applied to the files.
pub fn prepare(dir: &Path, target: Target) -> Result<Prepared> {
    let lab = lab(dir)?;
    let n = instance(dir);
    let files = generate(&lab, target, n).map_err(|e| Error::Invalid(format!("this lab can't run there: {e}")))?;
    for f in overrides::apply(files, &launcher_overrides(dir, target)) {
        let path = dir.join(f.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, f.contents)?;
    }
    let spec = as_instance(lab.for_target(target), n)?;
    Ok(Prepared { lab: Box::new(lab), spec })
}

/// `target`'s files for `lab`, as Isoloom generates them: the plan rendered by the target's
/// backend and the resolved snapshot;
/// for instance `n`, the lab as that instance and everything under `.isoloom-<n>/`. Refused,
/// with the backend's reason, when the lab's machines don't allow the target or the plan needs
/// what the backend can't do.
fn generate(lab: &Lab, target: Target, n: Option<u8>) -> isoloom_targets::Result<Vec<isoloom_targets::GeneratedFile>> {
    match n {
        Some(n) => isoloom_targets::generate_instance(lab, target, n),
        None => isoloom_targets::generate(lab, target),
    }
}

/// The launcher's overrides of a lab's files for `target`: on local Docker, its Compose project
/// (`cyberctf-<lab id>`, the lab's folder being its id).
fn launcher_overrides(dir: &Path, target: Target) -> overrides::Overrides {
    let id = dir.file_name().and_then(|n| n.to_str()).filter(|_| target == Target::Docker);
    overrides::Overrides { compose_project: id.map(super::docker::project) }
}

/// The memory the lab's machines ask for, in MB.
pub fn memory_mb(spec: &Spec) -> u32 {
    isoloom_spec::totals(spec).memory_mb
}

/// A size the lab's machines add up to: CPUs, memory and disk.
#[derive(serde::Serialize, Clone, Copy, PartialEq, Eq, Debug, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabResources")]
pub struct Resources {
    /// CPU cores.
    pub cpus: u32,
    /// Memory, in MB.
    pub memory_mb: u32,
    /// Disk, in GB.
    pub disk_gb: u32,
}

impl From<isoloom_spec::Totals> for Resources {
    fn from(t: isoloom_spec::Totals) -> Self {
        Resources { cpus: t.cpus, memory_mb: t.memory_mb, disk_gb: t.disk_gb }
    }
}

/// What a lab needs to run, as a range across the ways it runs: the lightest form (its machines
/// as containers) to the heaviest (one VM per machine). `min == max` when a lab has a single form.
#[derive(serde::Serialize, Clone, Copy, PartialEq, Eq, Debug, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabRequirements")]
pub struct Requirements {
    /// What the lightest form the lab runs in takes.
    pub min: Resources,
    /// What the heaviest form takes.
    pub max: Resources,
}

/// The lab's minimum-to-run requirements, sized per target it can run on ([`Lab::possible`], as
/// Isoloom generates it: a container reserves less than a VM) and reported as the
/// lightest-to-heaviest range. `None` for a lab with no machines (cloud services), which has no
/// footprint of its own to size.
pub fn requirements(lab: &Lab) -> Option<Requirements> {
    let spec = lab.spec();
    let mut sizes: Vec<isoloom_spec::Totals> = lab.possible().into_iter().map(|t| isoloom_spec::totals_for(&spec, t)).filter(|t| t.machines > 0).collect();
    sizes.sort_by_key(|t| t.memory_mb);
    Some(Requirements { min: (*sizes.first()?).into(), max: (*sizes.last()?).into() })
}

/// Where a lab is published: a GitHub `repository` at a `commit`, and the caller's rule for which
/// of them may be fetched (`check`, asked before anything is): the runtime fetches what it is
/// handed, the layer above knows which sources are the catalogue's.
#[derive(Clone, Copy)]
pub struct Source<'a> {
    /// The GitHub repository, `owner/name`.
    pub repository: &'a str,
    /// The commit of it to fetch.
    pub commit: &'a str,
    /// Whether that repository at that commit may be fetched (`repository`, `commit`): an error
    /// refuses it, before anything is fetched.
    pub check: fn(&str, &str) -> Result<()>,
}

/// The marker in a lab's folder naming the commit of its source installed there.
pub const COMMIT_MARKER: &str = ".cyberctf-commit";

/// Whether the lab installed in `dir` is the one at `commit` (its [`COMMIT_MARKER`] says so).
pub fn installed_at(dir: &Path, commit: &str) -> bool {
    std::fs::read_to_string(dir.join(COMMIT_MARKER)).is_ok_and(|c| c.trim() == commit)
}

/// A lab's spec at a commit ([`lab_at`]'s, as the launcher reads it).
pub async fn spec_at(dirs: &crate::platform::AppDirs, lab_id: &str, source: Source<'_>) -> Result<Spec> {
    Ok(lab_at(dirs, lab_id, source).await?.spec())
}

/// A lab as its author wrote it at its `source`'s commit: the installed copy when it is that
/// commit, else its `isoloom.yml` fetched from GitHub (a source its `check` allows), once per
/// repository and commit for the app's session. The one way the launcher reads a catalogue lab's
/// spec before it is installed: a lab page's requirements, its run options and its start place
/// read it once between them.
pub async fn lab_at(dirs: &crate::platform::AppDirs, lab_id: &str, source: Source<'_>) -> Result<Lab> {
    lab_at_with(dirs, lab_id, source, fetch_spec).await
}

/// [`lab_at`], a spec not read yet fetched from its URL by `fetch`.
async fn lab_at_with<F>(dirs: &crate::platform::AppDirs, lab_id: &str, source: Source<'_>, fetch: impl FnOnce(String) -> F) -> Result<Lab>
where
    F: Future<Output = Result<String>>,
{
    let Source { repository, commit, check } = source;
    crate::runtime::paths::validate_id(lab_id)?;
    check(repository, commit)?;
    if let Ok(dir) = crate::runtime::paths::lab_dir(dirs, lab_id)
        && installed_at(&dir, commit)
        && let Ok(Ok(text)) = overlay::spec_text(&dir)
        && let Ok(lab) = parsed(&text)
    {
        return Ok(lab);
    }
    let key = (repository.to_owned(), commit.to_owned());
    let cached = FETCHED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key).cloned();
    let text = if let Some(text) = cached {
        text
    } else {
        let text = fetch(format!("https://raw.githubusercontent.com/{repository}/{commit}/isoloom.yml")).await?;
        FETCHED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(key, text.clone());
        text
    };
    parsed(&text)
}

/// The specs fetched this session, by repository and commit: a commit's spec never changes.
static FETCHED: std::sync::Mutex<std::collections::BTreeMap<(String, String), String>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// A lab from its spec's text, unchecked.
fn parsed(text: &str) -> Result<Lab> {
    isoloom_spec::parse(text).map(Lab::new).map_err(|e| Error::Invalid(format!("this lab's isoloom.yml: {e}")))
}

/// The `isoloom.yml` at `url`, read through the API client with its size limit.
async fn fetch_spec(url: String) -> Result<String> {
    use crate::platform::http::{MAX_LAB_SPEC, api_client, body_limited};
    let unread = |e: &dyn std::fmt::Display| Error::Invalid(format!("couldn't read the lab's spec: {e}"));
    let res = api_client()?.get(&url).send().await.map_err(|e| unread(&e))?;
    if !res.status().is_success() {
        return Err(unread(&format!("HTTP {}", res.status())));
    }
    let body = body_limited(res, MAX_LAB_SPEC).await.map_err(|e| unread(&e))?;
    String::from_utf8(body).map_err(|e| unread(&e))
}

/// Why Isoloom won't run `lab` as `target` on `provider` (a hypervisor, a server's kind or a
/// cloud), if it won't: its VMs on a Vagrant provider (`qemu`, `hyperv`, …: a TPM, data disks; on
/// `utm`, none at all), a cloud whose `cloud-vm` module it drops
/// (a Windows machine given secrets on AWS or Azure), or the target itself. Isoloom's own rule, so
/// the launcher refuses exactly what the generated files would. A lab Isoloom can neither refuse
/// nor generate there is refused with what Isoloom says of it: its generation would fail too.
pub fn refusal_on(lab: &Lab, target: Target, provider: crate::runtime::providers::Provider) -> Option<String> {
    let decided = if target == Target::CloudVm {
        isoloom_targets::refusal(lab, target).and_then(|refused| match refused {
            Some(reason) => Ok(Some(reason)),
            None => Ok(isoloom_targets::cloud_refusals(lab, target)?
                .and_then(|clouds| clouds.refused.into_iter().find(|r| r.cloud == provider.id()).map(|r| r.reason))),
        })
    } else {
        isoloom_targets::refusal_on(lab, target, (target == Target::Vagrant).then(|| provider.id()))
    };
    refused_or_failed(decided)
}

/// Isoloom's refusal, or what it says when it can't decide (its generation would fail too).
pub fn refused_or_failed(decided: std::result::Result<Option<String>, isoloom_targets::GenerateError>) -> Option<String> {
    decided.unwrap_or_else(|e| Some(e.to_string()))
}

#[cfg(test)]
mod tests;
