//! Whether VirtualBox's kernel driver runs here (Linux). `VBoxManage --version` answers without
//! it (with a warning), but no VM starts ("the kernel module is not loaded"), so an installed
//! VirtualBox can run labs only once `vboxdrv` is loaded; why not otherwise, and whether a restart
//! mends it: the driver is built when the computer starts a kernel whose headers are installed
//! (VirtualBox's `vboxdrv.sh start`, DKMS when the kernel is installed).

use std::path::Path;

use super::Unavailable;

/// Why VirtualBox's kernel driver doesn't run on this machine; None when it is loaded, off Linux,
/// or when it can't be told (`/proc` unreadable).
pub(super) fn problem() -> Option<Unavailable> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let modules = std::fs::read_to_string("/proc/modules").ok()?;
    let running = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok()?;
    driver_problem(&modules, running.trim(), Path::new("/lib/modules"))
}

/// [`problem`], from the text of `/proc/modules` (the modules loaded), the running kernel
/// (`uname -r`) and the kernels' module folders (`lib_modules/<kernel>`, whose `build` link is
/// there once that kernel's headers are installed): loaded, nothing; else a restart when the
/// running kernel has no headers and a newer kernel has (its install brought it); else the driver
/// to build or sign.
fn driver_problem(modules: &str, running: &str, lib_modules: &Path) -> Option<Unavailable> {
    if modules.lines().any(|l| l.split_whitespace().next() == Some("vboxdrv")) {
        return None;
    }
    let has_headers = |kernel: &str| lib_modules.join(kernel).join("build").exists();
    if !has_headers(running) {
        let newer = std::fs::read_dir(lib_modules)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|k| has_headers(k) && release(k) > release(running))
            .max_by_key(|k| release(k));
        if let Some(kernel) = newer {
            return Some(Unavailable::KernelRestart { kernel });
        }
    }
    Some(Unavailable::VirtualboxDriver { kernel: running.to_owned() })
}

/// A kernel release's numbers, in order, up to its first letter: `6.12.116+deb13-amd64` is
/// 6.12.116, Ubuntu's `6.8.0-45-generic` 6.8.0-45 (its ABI number counts).
fn release(kernel: &str) -> Vec<u64> {
    let upto = kernel.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(kernel.len());
    kernel[..upto].split(|c: char| !c.is_ascii_digit()).filter(|n| !n.is_empty()).filter_map(|n| n.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/lib/modules` with `kernels`, each with its headers (`build`) or not.
    fn lib_modules(kernels: &[(&str, bool)]) -> crate::testing::TempDir {
        let dir = crate::testing::temp_dir("vbox-driver");
        for (kernel, headers) in kernels {
            std::fs::create_dir_all(dir.join(kernel).join(if *headers { "build" } else { "kernel" })).unwrap();
        }
        dir
    }

    const LOADED: &str = "vboxnetflt 32768 0 - Live 0x0000000000000000 (OE)\nvboxdrv 696320 2 vboxnetadp,vboxnetflt, Live 0x0000000000000000 (OE)\n";
    const NOT_LOADED: &str = "kvm_intel 413696 0 - Live 0x0000000000000000\nkvm 1404928 1 kvm_intel, Live 0x0000000000000000\n";

    #[test]
    fn a_loaded_driver_is_no_problem() {
        let dir = lib_modules(&[("6.12.86+deb13-amd64", false)]);
        assert_eq!(driver_problem(LOADED, "6.12.86+deb13-amd64", &dir), None);
    }

    #[test]
    fn a_driver_not_loaded_waits_for_the_newer_kernel_its_install_brought() {
        // Regression (the rig's bare Debian 13): the archive had moved on to 6.12.116, so the
        // running 6.12.86 had no headers; VirtualBox read as ready, and the self-test's VM then
        // found no driver.
        let dir = lib_modules(&[("6.12.86+deb13-amd64", false), ("6.12.116+deb13-amd64", true), ("6.12.94+deb13-amd64", true)]);
        assert_eq!(driver_problem(NOT_LOADED, "6.12.86+deb13-amd64", &dir), Some(Unavailable::KernelRestart { kernel: "6.12.116+deb13-amd64".into() }));
        // Ubuntu's ABI number counts.
        let ubuntu = lib_modules(&[("6.8.0-45-generic", false), ("6.8.0-47-generic", true)]);
        assert_eq!(driver_problem(NOT_LOADED, "6.8.0-45-generic", &ubuntu), Some(Unavailable::KernelRestart { kernel: "6.8.0-47-generic".into() }));
    }

    #[test]
    fn a_driver_not_loaded_with_nothing_a_restart_mends_is_to_build_or_sign() {
        // The running kernel's headers there (the build failed, or Secure Boot refused it).
        let built = lib_modules(&[("6.12.116+deb13-amd64", true)]);
        assert_eq!(driver_problem(NOT_LOADED, "6.12.116+deb13-amd64", &built), Some(Unavailable::VirtualboxDriver { kernel: "6.12.116+deb13-amd64".into() }));
        // No headers anywhere, or only an older kernel's: a restart wouldn't help.
        let older = lib_modules(&[("6.12.86+deb13-amd64", true), ("6.12.116+deb13-amd64", false)]);
        assert_eq!(driver_problem(NOT_LOADED, "6.12.116+deb13-amd64", &older), Some(Unavailable::VirtualboxDriver { kernel: "6.12.116+deb13-amd64".into() }));
    }

    #[test]
    fn kernel_releases_compare_by_their_numbers() {
        assert_eq!(release("6.12.116+deb13-amd64"), [6, 12, 116]);
        assert_eq!(release("6.8.0-45-generic"), [6, 8, 0, 45]);
        assert!(release("6.12.116+deb13-amd64") > release("6.12.86+deb13-amd64"));
    }
}
