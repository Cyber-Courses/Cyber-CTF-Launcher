//! The VM box's own Vagrant box, made once on this computer: Kali and its toolset (the attack box
//! repository's `vm/kali.sh` and `vm/toolset.sh`) installed in a one-machine environment of their
//! own, which Isoloom generates as it does a lab's (its OS's box for the hypervisor, the
//! hypervisor's settings), that machine then saved by the runner (`package`) and added to
//! Vagrant. A lab's box then boots it, and its own set-up is the player's account (and the
//! desktop): a start in about a minute, where installing the toolset took ten or more. The box is
//! named after what made it ([`name`]), so a changed script makes a new one, and it is recorded
//! with the runner's downloads, so Settings ▸ Storage lists and removes it. Only on this
//! computer's hypervisors: a server or a cloud account keeps the full set-up.

use std::fmt::Write as _;
use std::path::Path;

use sha2::{Digest as _, Sha256};

use super::{Cpu, DISK_GB, Scripts};
use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::runtime::isothrum::{self, Deployment, Inputs};
use crate::runtime::lab;

/// The build's one machine, in its environment.
const MACHINE: &str = "box";
/// Its set-up, a file of the build's environment.
const SCRIPT: &str = "build.sh";
/// The build VM's memory (MB) and CPUs: Kali's toolset installs faster with more.
const SIZE: (u32, u32) = (4096, 2);

/// What the build runs, as root, once: Kali ([`Scripts::kali`]), its toolset ([`Scripts::toolset`],
/// or none: a test run's bare box), then what of the build the box shouldn't keep (the package
/// cache, Isoloom's record of the build's own steps).
pub(crate) fn build_script(scripts: &Scripts, toolset: bool) -> String {
    let toolset = if toolset { scripts.toolset.as_str() } else { "# Kali's toolset left out: a test run's bare box.\n" };
    [scripts.kali.as_str(), "\n", toolset, "\n", "apt-get clean\nrm -rf /var/lib/isoloom\n"].concat()
}

/// The box's name: `cyberctf/attackbox-<cpu>-<12 hex of the build script's SHA-256>`.
pub(crate) fn name(cpu: Cpu, script: &str) -> String {
    let hex = Sha256::digest(script.as_bytes()).iter().take(6).fold(String::with_capacity(12), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    });
    let cpu = match cpu {
        Cpu::Lab => "lab",
        Cpu::Arm => "arm64",
    };
    format!("cyberctf/attackbox-{cpu}-{hex}")
}

/// The build's environment: [`MACHINE`] booted from Kali (from Debian where Kali has no box for
/// the CPU: `kali.sh` makes it Kali), on a network of its own, sized for the toolset, its set-up
/// [`SCRIPT`].
fn spec(cpu: Cpu) -> String {
    let (os, hardware) = match cpu {
        Cpu::Lab => ("kali@rolling", ""),
        Cpu::Arm => ("debian@12", "hardware: { arch: arm64 }, "),
    };
    let (memory, cpus) = SIZE;
    format!(
        "version: 1\nname: cyberctf-attackbox-build\nnetworks: {{ net: {{ cidr: 10.254.254.0/24 }} }}\nmachines:\n  {MACHINE}: {{ os: {os}, {hardware}networks: {{ net: 10 }}, resources: {{ disk_gb: {DISK_GB}, memory_mb: {memory}, cpus: {cpus} }}, app: {{ steps: [{{ run: {SCRIPT} }}] }} }}\n"
    )
}

/// The box wanted: for which hypervisor and CPU, made by which script, under which name.
#[derive(Clone, Copy)]
struct Wanted<'a> {
    provider: isothrum_runner::Provider,
    cpu: Cpu,
    script: &'a str,
    name: &'a str,
}

/// The box for `provider` on this computer, built first when Vagrant doesn't hold it ([`name`]
/// of `script`, [`build_script`]): its name. `log` says each step, and that the first build takes
/// a while.
///
/// # Errors
///
/// The build's environment not written or not started, the script failing, the machine not saved
/// or the box not added: the runner's or Vagrant's words.
pub(crate) async fn ready(dirs: &AppDirs, provider: isothrum_runner::Provider, cpu: Cpu, script: &str, log: &mut impl FnMut(String)) -> Result<String> {
    let name = name(cpu, script);
    if isothrum_machine::boxes::held_here(&crate::runtime::providers::machine(), provider, &name).await.is_some() {
        return Ok(name);
    }
    log("Building the attack box on this computer, once: about 10 minutes. Later starts boot it in about a minute.".into());
    let home = crate::runtime::paths::runner_home(dirs)?;
    let dir = dirs.cache()?.join("attack-box").join(name.replace('/', "-"));
    let wanted = Wanted { provider, cpu, script, name: &name };
    let built = build(&home, &dir, &wanted, log).await;
    // The build's environment, whatever became of it: the box is what is kept.
    if let Err(e) = std::fs::remove_dir_all(&dir)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("the attack box's build folder {} wasn't removed: {e}", dir.display());
    }
    built.map(|()| name)
}

/// Builds the `wanted` box in `dir`: the environment generated, started on its hypervisor, its machine
/// saved, the environment brought down and forgotten, the saved file added and recorded.
async fn build(home: &Path, dir: &Path, wanted: &Wanted<'_>, log: &mut impl FnMut(String)) -> Result<()> {
    let Wanted { provider, cpu, script, name } = *wanted;
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("isoloom.yml"), spec(cpu))?;
    std::fs::write(dir.join(SCRIPT), script)?;
    lab::prepare(dir, lab::Target::Vagrant)?;
    let id = name.replace('/', "-");
    let run = Deployment::vm(home, dir, &id)?;
    let file = dir.join("built.box");
    let saved = match run.up(Inputs::default(), Some(&isothrum_runner::wire_name(&provider)), log).await {
        Ok(()) => run.package(MACHINE, &file, log).await,
        Err(e) => Err(e),
    };
    let down = run.down(log).await;
    if let Err(e) = isothrum::forget_run(home, &id).await {
        log::warn!("the attack box's build run wasn't forgotten: {e}");
    }
    saved?;
    if let Err(e) = down {
        log(format!("The attack box's build VM wasn't removed: {e}"));
    }
    let cx = crate::runtime::providers::machine();
    let mut said = Vec::new();
    let added = isothrum_machine::boxes::add_file(&cx, provider, name, &file, &mut |l| said.push(l.to_owned())).await;
    said.into_iter().for_each(&mut *log);
    let added = added.map_err(|e| Error::Invalid(format!("The attack box built here couldn't be added to Vagrant: {e}")))?;
    if let Some(added) = added
        && let Err(e) = isothrum::adopt_downloads(home, vec![added]).await
    {
        log::warn!("the attack box's box wasn't added to the runner's record: {e}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_box_is_named_after_its_cpu_and_what_built_it() {
        let scripts = super::super::test_scripts();
        let full = build_script(&scripts, true);
        let bare = build_script(&scripts, false);
        assert!(full.starts_with(&scripts.kali) && full.contains(&scripts.toolset), "{full}");
        assert!(!bare.contains(&scripts.toolset), "a test run's bare box: {bare}");
        assert!(full.ends_with("rm -rf /var/lib/isoloom\n"), "{full}");
        let named = name(Cpu::Lab, &full);
        assert!(named.starts_with("cyberctf/attackbox-lab-") && named.len() == "cyberctf/attackbox-lab-".len() + 12, "{named}");
        assert_eq!(named, name(Cpu::Lab, &full), "the same script, the same box");
        assert_ne!(named, name(Cpu::Lab, &bare), "a bare box is another box");
        assert!(name(Cpu::Arm, &full).starts_with("cyberctf/attackbox-arm64-"));
    }

    #[test]
    fn the_build_environment_is_one_machine_isoloom_reads() {
        for cpu in [Cpu::Lab, Cpu::Arm] {
            let text = spec(cpu);
            let dir = crate::runtime::test_support::write_lab(&text);
            let spec = lab::load(&dir).unwrap_or_else(|e| panic!("{cpu:?}: {e}\n{text}"));
            assert_eq!(spec.machines.keys().collect::<Vec<_>>(), [MACHINE], "{text}");
        }
        assert!(spec(Cpu::Arm).contains("os: debian@12, hardware: { arch: arm64 }"));
        assert!(spec(Cpu::Lab).contains("os: kali@rolling, networks"));
    }
}
