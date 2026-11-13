//! This computer as the launcher's rules read it: its OS, its CPU and its package manager, each a
//! closed set, worked out once ([`Host::here`]) and handed to every rule that depends on them. A
//! rule matches on these enums, never on `std::env::consts` strings: an OS, CPU or package manager
//! added here is a compile error at every rule that doesn't say what it does there yet.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
compile_error!("the launcher builds for macOS, Windows and Linux: add the OS to `platform::host::Os`");
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("the launcher builds for x86_64 and aarch64: add the CPU to `platform::host::Arch`");

/// An OS the launcher runs on, by the id Rust gives it (`std::env::consts::OS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    /// macOS.
    Macos,
    /// Windows.
    Windows,
    /// Linux.
    Linux,
}

impl Os {
    /// Every OS, in the order the launcher lists them.
    pub const ALL: [Os; 3] = [Os::Macos, Os::Windows, Os::Linux];

    /// The OS this build runs on.
    #[cfg(target_os = "macos")]
    pub const HERE: Os = Os::Macos;
    /// The OS this build runs on.
    #[cfg(target_os = "windows")]
    pub const HERE: Os = Os::Windows;
    /// The OS this build runs on.
    #[cfg(target_os = "linux")]
    pub const HERE: Os = Os::Linux;

    /// Its id, `std::env::consts::OS`'s (`macos`), as the machine report and the webview write it.
    pub fn id(self) -> &'static str {
        match self {
            Os::Macos => "macos",
            Os::Windows => "windows",
            Os::Linux => "linux",
        }
    }

    /// Its name as the player reads it (`macOS`): the launcher's messages and the webview's
    /// (`OS_NAMES`, generated) name an OS from here.
    pub fn name(self) -> &'static str {
        match self {
            Os::Macos => "macOS",
            Os::Windows => "Windows",
            Os::Linux => "Linux",
        }
    }

    /// The package manager the guided setup installs through on this OS when the machine has it:
    /// what the machine check names when it is missing (Linux's first, apt).
    pub fn usual_packages(self) -> &'static str {
        match self {
            Os::Macos => "Homebrew",
            Os::Windows => "winget",
            Os::Linux => "apt",
        }
    }
}

/// A CPU the launcher runs on, by the name Rust gives it (`std::env::consts::ARCH`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub enum Arch {
    /// 64-bit x86 (Intel, AMD).
    #[serde(rename = "x86_64")]
    X86_64,
    /// 64-bit Arm (Apple silicon, Windows on Arm, Arm Linux).
    #[serde(rename = "aarch64")]
    Aarch64,
}

impl Arch {
    /// The CPU this build runs on.
    #[cfg(target_arch = "x86_64")]
    pub const HERE: Arch = Arch::X86_64;
    /// The CPU this build runs on.
    #[cfg(target_arch = "aarch64")]
    pub const HERE: Arch = Arch::Aarch64;

    /// Every CPU.
    #[cfg(test)]
    pub const ALL: [Arch; 2] = [Arch::X86_64, Arch::Aarch64];

    /// Its id, `std::env::consts::ARCH`'s (`aarch64`).
    pub fn id(self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
        }
    }

    /// Its name for Docker images and vendors' downloads (`amd64`, `arm64`).
    pub fn docker_name(self) -> &'static str {
        match self {
            Arch::X86_64 => "amd64",
            Arch::Aarch64 => "arm64",
        }
    }

    /// QEMU's system emulator for it (`qemu-system-x86_64`).
    pub fn qemu_system(self) -> &'static str {
        match self {
            Arch::X86_64 => "qemu-system-x86_64",
            Arch::Aarch64 => "qemu-system-aarch64",
        }
    }
}

/// The package manager the guided setup installs and removes tools through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packages {
    /// Homebrew (macOS), at this path.
    Homebrew(&'static str),
    /// winget (Windows).
    Winget,
    /// A Linux distribution's.
    Linux(LinuxPackages),
}

/// A Linux distribution's package manager the guided setup goes through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxPackages {
    /// apt-get: Debian, Ubuntu and the distributions built on them.
    Apt,
    /// dnf: Fedora and the distributions built on it.
    Dnf,
}

impl Packages {
    /// Its name, as the machine check reports it (`Homebrew`, `winget`, `apt`, `dnf`).
    pub fn name(self) -> &'static str {
        match self {
            Packages::Homebrew(_) => "Homebrew",
            Packages::Winget => "winget",
            Packages::Linux(LinuxPackages::Apt) => "apt",
            Packages::Linux(LinuxPackages::Dnf) => "dnf",
        }
    }

    /// The program that answers its `--version`.
    pub fn program(self) -> &'static str {
        match self {
            Packages::Homebrew(brew) => brew,
            Packages::Winget => "winget",
            Packages::Linux(LinuxPackages::Apt) => "apt-get",
            Packages::Linux(LinuxPackages::Dnf) => "dnf",
        }
    }

    /// The Linux distribution's, when it is one.
    pub fn linux(self) -> Option<LinuxPackages> {
        match self {
            Packages::Linux(packages) => Some(packages),
            Packages::Homebrew(_) | Packages::Winget => None,
        }
    }
}

/// This computer: its OS, its CPU, and the package manager the guided setup goes through here
/// (`None`: a Mac without Homebrew, a Linux distribution with neither apt nor dnf).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Host {
    /// The OS.
    pub os: Os,
    /// The CPU.
    pub arch: Arch,
    /// The package manager, when the machine has one the setup goes through.
    pub packages: Option<Packages>,
}

impl Host {
    /// This computer, its package manager looked up now (Homebrew where its installer puts it;
    /// apt-get, else dnf, on the PATH).
    pub fn here() -> Host {
        let packages = match Os::HERE {
            Os::Macos => brew_bin().map(Packages::Homebrew),
            Os::Windows => Some(Packages::Winget),
            Os::Linux if super::on_path("apt-get") => Some(Packages::Linux(LinuxPackages::Apt)),
            Os::Linux if super::on_path("dnf") => Some(Packages::Linux(LinuxPackages::Dnf)),
            Os::Linux => None,
        };
        Host { os: Os::HERE, arch: Arch::HERE, packages }
    }

    /// Whether it is a PC with Windows on Arm.
    pub fn is_windows_on_arm(self) -> bool {
        self.os == Os::Windows && self.arch == Arch::Aarch64
    }
}

/// Homebrew, where its installer puts it (Apple silicon, then Intel).
pub fn brew_bin() -> Option<&'static str> {
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].into_iter().find(|p| std::path::Path::new(p).exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_os_and_cpu_here_are_rusts() {
        assert_eq!(Os::HERE.id(), std::env::consts::OS);
        assert_eq!(Arch::HERE.id(), std::env::consts::ARCH);
        assert_eq!(Host::here().os, Os::HERE);
    }

    #[test]
    fn each_os_and_cpu_is_written_by_its_id() {
        for os in Os::ALL {
            assert_eq!(serde_json::to_value(os).unwrap(), os.id());
        }
        for arch in [Arch::X86_64, Arch::Aarch64] {
            assert_eq!(serde_json::to_value(arch).unwrap(), arch.id());
        }
        assert_eq!(Os::Macos.name(), "macOS");
    }

    #[test]
    fn the_package_manager_here_is_the_oss_own() {
        match (Host::here().os, Host::here().packages) {
            (Os::Macos, None | Some(Packages::Homebrew(_))) | (Os::Windows, Some(Packages::Winget)) | (Os::Linux, None | Some(Packages::Linux(_))) => {}
            other => panic!("{other:?}"),
        }
    }
}
