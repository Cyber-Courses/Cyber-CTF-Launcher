//! What the lab page reads in a deploy log: the marks a run's closing line starts with, and the
//! launcher's own lines it files under a step of their own. How each starts is said once, here,
//! and the lines are built from it here. The webview reads
//! the same starts from the file `ts_bindings` generates of them (`DeployLog.ts`), so a line
//! reworded here can't fall out of its step there.

use std::path::Path;

/// How a failed run's closing line starts (the lab page marks a run failed by it).
pub(crate) const FAILED_MARK: &str = "✗";
/// How a run's closing line starts when it went through (the webview writes those: "✓ Lab is
/// running").
pub(crate) const DONE_MARK: &str = "✓";

// The lines said while the lab is fetched and placed: the lab page's "Download the lab" step.
const DOWNLOADING: &str = "Downloading ";
/// The last line of a lab's install from the catalogue.
pub(crate) const INSTALLED: &str = "Lab installed";
const IMPORTING: &str = "Importing ";
/// The last line of a lab's import from a folder on this machine.
pub(crate) const IMPORTED: &str = "Lab imported";
const IN_LOCAL_VM: &str = "Running in a ";
const ON_SERVER_HOST: &str = "Running on server host ";
const IN_CLOUD_ACCOUNT: &str = "Running in your ";

/// How the lines said while the lab is fetched and placed start (for the generated file).
#[cfg(test)]
pub(crate) const FETCH: [&str; 7] = [DOWNLOADING, INSTALLED, IMPORTING, IMPORTED, IN_LOCAL_VM, ON_SERVER_HOST, IN_CLOUD_ACCOUNT];

/// The first line of a lab's install from the catalogue: its repository at `commit` (short).
pub(crate) fn downloading(repository: &str, commit: &str) -> String {
    format!("{DOWNLOADING}{repository}@{commit}")
}

/// The first line of lab `lab_id`'s import from the folder `from`.
pub(crate) fn importing(lab_id: &str, from: &Path) -> String {
    format!("{IMPORTING}{lab_id} from {}", from.display())
}

/// A container lab placed in one VM on this machine, on hypervisor `provider` (its id).
pub(crate) fn in_local_vm(provider: &str) -> String {
    format!("{IN_LOCAL_VM}{provider} VM on this machine")
}

/// A lab placed on the saved server `name`, a `provider` (its id) host.
pub(crate) fn on_server_host(name: &str, provider: &str) -> String {
    format!("{ON_SERVER_HOST}{name} ({provider})")
}

/// A cloud-services lab placed in the player's `cloud` (its name) account `name`, in `region`.
pub(crate) fn in_cloud_account(cloud: &str, name: &str, region: &str) -> String {
    format!("{IN_CLOUD_ACCOUNT}{cloud} account {name} ({region})")
}

// The lines said while this machine is readied for a lab's VMs, before Vagrant's own output: the
// lab page's "Prepare this machine" step when one comes first.
const LOW_MEMORY: &str = "This lab's VMs ask for about ";
const EMULATING: &str = "Emulating ";
/// UTM is opened before a lab's VMs start on it, and waited for until it answers.
pub(crate) const OPENING_UTM: &str = "Opening UTM and waiting for it to answer…";
/// The runner clears what a previous, stopped run left (its `LeftoversRemoved` warning).
pub(crate) const CLEARING_LEFTOVERS: &str = "Clearing a previous, stopped run…";
/// The runner clears what Docker kept of an earlier run and starts again (its `Recovering`
/// warning).
pub(crate) const RECOVERING: &str = "Docker kept something of an earlier run: clearing it, then starting again…";

/// How the lines said while this machine is readied for a lab's VMs start (for the generated
/// file).
#[cfg(test)]
pub(crate) const PREPARE: [&str; 5] = [LOW_MEMORY, EMULATING, OPENING_UTM, CLEARING_LEFTOVERS, RECOVERING];

/// A lab's VMs asking for `needed_mb` of memory with only `available_mb` free here.
pub(crate) fn low_memory(needed_mb: u64, available_mb: u64) -> String {
    format!(
        "{LOW_MEMORY}{needed_mb} MB, but only ~{available_mb} MB is free on this machine. It may run slowly or fail to boot; close other apps or stop other labs if it struggles."
    )
}

/// A lab's `arch` VMs emulated by QEMU on this `host` machine.
pub(crate) fn emulating(arch: &str, host: &str) -> String {
    format!(
        "{EMULATING}{arch} on this {host} machine with QEMU, many times slower than native: a Windows machine takes 15 to 40 minutes to boot, and the lab's setup can take hours."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lines as the deploy log shows them: the lab page's tests (`deploy-log.test.ts`) read
    /// these very lines.
    #[test]
    fn the_lines_read_as_the_lab_page_expects_them() {
        assert_eq!(downloading("CyberCTF/invoice", "0123456789ab"), "Downloading CyberCTF/invoice@0123456789ab");
        assert_eq!(importing("local-web", Path::new("/home/ada/labs/web")), "Importing local-web from /home/ada/labs/web");
        assert_eq!(in_local_vm("qemu"), "Running in a qemu VM on this machine");
        assert_eq!(on_server_host("esxi-1", "vmware_esxi"), "Running on server host esxi-1 (vmware_esxi)");
        assert_eq!(in_cloud_account("AWS", "Labs", "eu-west-1"), "Running in your AWS account Labs (eu-west-1)");
        assert!(emulating("x86_64", "aarch64").starts_with("Emulating x86_64 on this aarch64 machine with QEMU"));
        assert!(low_memory(8192, 2048).starts_with("This lab's VMs ask for about 8192 MB, but only ~2048 MB is free"));
    }

    /// Each line starts as its step's list says, and no start of one step is the start of the
    /// other's (a line would then be filed under both).
    #[test]
    fn each_line_starts_as_its_step_says() {
        let fetched = [
            downloading("r", "c"),
            INSTALLED.into(),
            importing("l", Path::new("/x")),
            IMPORTED.into(),
            in_local_vm("p"),
            on_server_host("n", "p"),
            in_cloud_account("c", "n", "r"),
        ];
        for line in &fetched {
            assert!(FETCH.iter().any(|s| line.starts_with(s)), "{line}");
            assert!(!PREPARE.iter().any(|s| line.starts_with(s)), "{line}");
        }
        let prepared = [low_memory(2, 1), emulating("a", "h"), OPENING_UTM.into(), CLEARING_LEFTOVERS.into(), RECOVERING.into()];
        for line in &prepared {
            assert!(PREPARE.iter().any(|s| line.starts_with(s)), "{line}");
            assert!(!FETCH.iter().any(|s| line.starts_with(s)), "{line}");
        }
    }
}
