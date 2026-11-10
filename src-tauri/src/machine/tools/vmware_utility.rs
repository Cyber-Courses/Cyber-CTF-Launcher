//! HashiCorp's Vagrant VMware Utility: the system service the `vagrant-vmware-desktop` plugin
//! drives VMware Fusion and Workstation through. It isn't a Vagrant plugin, nor in winget or
//! Homebrew's core taps in a form fit for Apple silicon, so it is installed from HashiCorp's own
//! signed package, pinned here with its checksum:
//!
//! - macOS: the disk image downloaded as the player, then, with macOS's administrator prompt,
//!   copied into a folder only root can write, its SHA-256 checked, its package checked as signed
//!   by HashiCorp's Developer ID and accepted by Gatekeeper (notarized), and installed with
//!   `installer`. The package's own script generates the certificates and starts the service.
//! - Windows: the MSI downloaded and its SHA-256 checked as the player, then, through Windows'
//!   UAC prompt, copied into a folder only administrators can write, checked again (SHA-256 and
//!   Authenticode signature by HashiCorp) and installed with `msiexec`. An install that is there
//!   already only has its service started again.
//! - Linux: HashiCorp's apt repository, its key checked against its pinned fingerprint
//!   (`scripts::vmware_utility_script`), as root through pkexec.
//!
//! Every path ends in a real result: a declined prompt, a checksum or a signature that doesn't
//! match, or an installer error fails the step, and the machine check then says whether the
//! service answers. Nothing reports success on the strength of an installer merely opened.

#[cfg(any(target_os = "macos", target_os = "windows", test))]
use super::steps::Step;

/// The release installed. Its files and checksums are HashiCorp's published ones
/// (`releases.hashicorp.com/vagrant-vmware-utility/<version>/…_SHA256SUMS`, signed with HashiCorp's
/// release key); a new release changes the version and every checksum below together.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
const VERSION: &str = "1.0.24";

/// Where HashiCorp publishes the release's files.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn url(file: &str) -> String {
    format!("https://releases.hashicorp.com/vagrant-vmware-utility/{VERSION}/{file}")
}

/// The macOS disk image for CPU `arch` (`std::env::consts::ARCH`), with its SHA-256. HashiCorp
/// publishes one universal image under both names.
#[cfg(any(target_os = "macos", test))]
fn mac_image(arch: &str) -> (String, &'static str) {
    let arch = if arch == "aarch64" { "arm64" } else { "amd64" };
    (format!("vagrant-vmware-utility_{VERSION}_darwin_{arch}.dmg"), "5f42cb017dd07270409eaee7c3b4a164ffa7c0f21d85c65840c4f81aab21d457")
}

/// The team of HashiCorp's Apple Developer ID, which signs the package (and is notarized).
#[cfg(any(target_os = "macos", test))]
const HASHICORP_TEAM: &str = "D38WU7D763";

/// The Windows installer, with its SHA-256.
#[cfg(any(target_os = "windows", test))]
const WINDOWS_MSI: (&str, &str) = ("vagrant-vmware-utility_1.0.24_windows_amd64.msi", "8ee7bea062d7552a484b7d9ade369744e5cd9ab1b55eed8b7bd35a1b006666d6");

/// That MSI's product code, which `msiexec /x` removes it by.
#[cfg(any(target_os = "windows", test))]
const WINDOWS_PRODUCT_CODE: &str = "{DE47C4D6-3BD9-4286-A93D-1DCA1CBADDA7}";

/// The root half of the macOS install, run by `/bin/sh -c` with the downloaded image as `$1`:
/// copied where only root writes (so nothing can swap it between the check and the install),
/// checked, mounted there, its package checked as HashiCorp's and installed.
#[cfg(any(target_os = "macos", test))]
fn mac_root_script(sha256: &str) -> String {
    format!(
        r#"set -e
d="$(mktemp -d /private/tmp/cyberctf-vmware-utility.XXXXXX)"
m=""
trap '[ -z "$m" ] || hdiutil detach -quiet "$m"; rm -rf "$d"' EXIT
cp "$1" "$d/utility.dmg"
[ "$(shasum -a 256 "$d/utility.dmg" | awk '{{print $1}}')" = "{sha256}" ] || {{ echo "The download isn't HashiCorp's Vagrant VMware Utility {VERSION} (its SHA-256 doesn't match). Nothing was installed." >&2; exit 1; }}
m="$(hdiutil attach -nobrowse -noautoopen -readonly -mountrandom "$d" "$d/utility.dmg" | awk -F'\t' '$NF ~ /^\// {{print $NF; exit}}')"
p="$m/VagrantVMwareUtility.pkg"
pkgutil --check-signature "$p" | grep -qF "Developer ID Installer: Hashicorp, Inc. ({HASHICORP_TEAM})" || {{ echo "The package isn't signed by HashiCorp. Nothing was installed." >&2; exit 1; }}
spctl --assess --type install "$p" || {{ echo "macOS doesn't accept the package (not notarized). Nothing was installed." >&2; exit 1; }}
installer -pkg "$p" -target /"#
    )
}

/// The player's half, run by `sh -c` with the root half as `$1`: the download into a private
/// folder, its checksum (a broken download stops before any prompt), then the root half through
/// macOS's administrator prompt, the image's path handed over as an argument (never spliced into
/// the script).
#[cfg(any(target_os = "macos", test))]
fn mac_script(file: &str, sha256: &str) -> String {
    let url = url(file);
    format!(
        r#"set -e
d="$(mktemp -d)"
trap 'rm -rf "$d"' EXIT
curl -fsSL --proto '=https' --tlsv1.2 "{url}" -o "$d/utility.dmg"
[ "$(shasum -a 256 "$d/utility.dmg" | awk '{{print $1}}')" = "{sha256}" ] || {{ echo "The download isn't HashiCorp's Vagrant VMware Utility {VERSION} (its SHA-256 doesn't match). Nothing was installed." >&2; exit 1; }}
echo "Downloaded and checked. Installing it: macOS asks for your password."
if ! out="$(osascript -e 'on run argv' -e 'do shell script "/bin/sh -c " & quoted form of (item 1 of argv) & " sh " & quoted form of (item 2 of argv) with administrator privileges' -e 'end run' "$1" "$d/utility.dmg" 2>&1)"; then
  case "$out" in
    *-128*) echo "The password prompt was cancelled, so nothing was installed. Try again and enter your password." >&2 ;;
    *) echo "$out" >&2 ;;
  esac
  exit 1
fi
echo "$out""#
    )
}

/// The macOS install: one step, as the player, raising macOS's administrator prompt itself.
#[cfg(any(target_os = "macos", test))]
pub fn mac_install(arch: &str) -> Step {
    let (file, sha256) = mac_image(arch);
    Step {
        program: "sh".into(),
        args: vec!["-c".into(), mac_script(&file, sha256), "sh".into(), mac_root_script(sha256)],
        note: Some(format!("Downloading HashiCorp's Vagrant VMware Utility {VERSION}…")),
    }
}

/// The macOS removal, as the package's own `uninstall.tool` does it: the service, then its folder.
#[cfg(target_os = "macos")]
pub const MAC_UNINSTALL: &str = "/opt/vagrant-vmware-desktop/bin/vagrant-vmware-utility service uninstall; rm -rf /opt/vagrant-vmware-desktop && (pkgutil --forget com.vagrant.vagrant-vmware-utility || true)";

/// `text` as a PowerShell single-quoted string.
#[cfg(any(target_os = "windows", test))]
fn ps_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The elevated half of the Windows install, with the downloaded MSI's path in `$msi` (set by
/// the player's half, before this): an install already there only has its service started
/// again; otherwise the MSI is copied into a folder only administrators write, checked again
/// (SHA-256, then Authenticode signature by HashiCorp) and installed. Exits 0 when done (3010,
/// "restart to finish", is done), 2 on a checksum and 3 on a signature that doesn't match, else
/// msiexec's own code.
#[cfg(any(target_os = "windows", test))]
fn windows_elevated(sha256: &str) -> String {
    format!(
        r#"$ErrorActionPreference = 'Stop'
if (Get-Service -Name 'vagrant-vmware-utility' -ErrorAction SilentlyContinue) {{ Restart-Service -Name 'vagrant-vmware-utility'; exit 0 }}
$d = Join-Path $env:SystemRoot ('Temp\cyberctf-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $d | Out-Null
try {{
  $copy = Join-Path $d 'vagrant-vmware-utility.msi'
  Copy-Item -LiteralPath $msi -Destination $copy
  if ((Get-FileHash -Algorithm SHA256 -LiteralPath $copy).Hash -ne '{sha256}') {{ exit 2 }}
  $sig = Get-AuthenticodeSignature -LiteralPath $copy
  if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.GetNameInfo('SimpleName', $false) -ne 'HashiCorp, Inc.') {{ exit 3 }}
  $p = Start-Process -FilePath msiexec.exe -ArgumentList '/i', ('"' + $copy + '"'), '/qn', '/norestart' -Wait -PassThru
  if ($p.ExitCode -eq 3010) {{ exit 0 }}
  exit $p.ExitCode
}} finally {{ Remove-Item -LiteralPath $d -Recurse -Force -ErrorAction SilentlyContinue }}"#
    )
}

/// The player's half of the Windows install: the download into a private folder and its
/// checksum, then the elevated half through the UAC prompt (`-Verb RunAs`), given the MSI's path
/// inside its own script (an elevated process gets neither this one's pipes nor its
/// environment). A declined prompt exits 1223 (`ERROR_CANCELLED`), which reads as cancelled.
#[cfg(any(target_os = "windows", test))]
fn windows_script(file: &str, sha256: &str) -> String {
    let url = url(file);
    let elevated = ps_quoted(&windows_elevated(sha256));
    format!(
        r"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$d = Join-Path ([IO.Path]::GetTempPath()) ('cyberctf-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $d | Out-Null
try {{
  $msi = Join-Path $d 'vagrant-vmware-utility.msi'
  Invoke-WebRequest -UseBasicParsing -Uri '{url}' -OutFile $msi
  if ((Get-FileHash -Algorithm SHA256 -LiteralPath $msi).Hash -ne '{sha256}') {{ [Console]::Error.WriteLine('The download isn''t HashiCorp''s Vagrant VMware Utility {VERSION} (its SHA-256 doesn''t match). Nothing was installed.'); exit 1 }}
  Write-Output 'Downloaded and checked. Installing it: Windows asks for an administrator''s approval.'
  $script = '$msi = ' + ([char]39) + $msi.Replace([string][char]39, [string][char]39 + [char]39) + ([char]39) + [Environment]::NewLine + {elevated}
  $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($script))
  try {{ $p = Start-Process -FilePath powershell.exe -ArgumentList '-NoProfile', '-NonInteractive', '-EncodedCommand', $encoded -Verb RunAs -Wait -PassThru }} catch {{ exit 1223 }}
  if ($p.ExitCode -eq 2) {{ [Console]::Error.WriteLine('The installer changed after it was checked. Nothing was installed.') }}
  if ($p.ExitCode -eq 3) {{ [Console]::Error.WriteLine('The installer isn''t signed by HashiCorp. Nothing was installed.') }}
  exit $p.ExitCode
}} finally {{ Remove-Item -LiteralPath $d -Recurse -Force -ErrorAction SilentlyContinue }}"
    )
}

/// The Windows install: one PowerShell step, raising the UAC prompt itself.
#[cfg(any(target_os = "windows", test))]
pub fn windows_install() -> Step {
    let (file, sha256) = WINDOWS_MSI;
    Step {
        program: "powershell".into(),
        args: vec!["-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), windows_script(file, sha256)],
        note: Some(format!("Downloading HashiCorp's Vagrant VMware Utility {VERSION}…")),
    }
}

/// The Windows removal: the pinned release's MSI removed by its product code, through UAC.
#[cfg(any(target_os = "windows", test))]
pub fn windows_uninstall() -> Step {
    Step {
        program: "powershell".into(),
        args: vec![
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            format!(
                "try {{ $p = Start-Process -FilePath msiexec.exe -ArgumentList '/x', '{WINDOWS_PRODUCT_CODE}', '/qn', '/norestart' -Verb RunAs -Wait -PassThru; if ($p.ExitCode -eq 3010) {{ exit 0 }}; exit $p.ExitCode }} catch {{ exit 1223 }}"
            ),
        ],
        note: Some("Removing the Vagrant VMware Utility (Windows asks for an administrator's approval)…".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mac_install_checks_hashicorps_file_and_signature_before_installing_as_root() {
        let step = mac_install("aarch64");
        assert_eq!(step.program, "sh");
        let [c, user, zero, root] = step.args.as_slice() else { panic!("{:?}", step.args) };
        assert_eq!((c.as_str(), zero.as_str()), ("-c", "sh"));
        assert!(crate::platform::sh_parses(user), "not valid sh: {user}");
        assert!(crate::platform::sh_parses(root), "not valid sh: {root}");
        assert!(user.contains("https://releases.hashicorp.com/vagrant-vmware-utility/1.0.24/vagrant-vmware-utility_1.0.24_darwin_arm64.dmg"), "{user}");
        assert!(user.contains("with administrator privileges") && user.contains("quoted form of (item 2 of argv)"), "{user}");
        // As root, in this order: a copy root owns, its checksum, the signature, Gatekeeper, the install.
        let at = |needle: &str| root.find(needle).unwrap_or_else(|| panic!("{needle} missing: {root}"));
        assert!(at("cp \"$1\"") < at("shasum -a 256") && at("shasum -a 256") < at("pkgutil --check-signature"));
        assert!(at("pkgutil --check-signature") < at("spctl --assess --type install") && at("spctl --assess") < at("installer -pkg"));
        assert!(root.contains("Hashicorp, Inc. (D38WU7D763)") && root.contains("mktemp -d /private/tmp/"), "{root}");
        // The image is the same file whatever the Mac's CPU: one checksum for both names.
        assert_eq!(mac_image("aarch64").1, mac_image("x86_64").1);
        assert!(mac_image("x86_64").0.ends_with("_darwin_amd64.dmg"));
    }

    /// Runs `script` with `sh -c`, `args` after it, with `bin` (stand-ins for the system's tools)
    /// first on PATH: its exit code and its error output.
    #[cfg(target_os = "macos")]
    fn run_sh(script: &str, args: &[&str], bin: &std::path::Path) -> (i32, String) {
        let path = format!("{}:/usr/bin:/bin", bin.display());
        let out = std::process::Command::new("sh").arg("-c").arg(script).arg("sh").args(args).env("PATH", path).output().unwrap();
        (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stderr).into_owned())
    }

    /// A stand-in for the tool `name`: a shell script `body`.
    #[cfg(target_os = "macos")]
    fn stand_in(bin: &std::path::Path, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let file = bin.join(name);
        std::fs::write(&file, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_download_that_isnt_hashicorps_file_fails_before_any_prompt() {
        let app = crate::testing::test_app::TestApp::new();
        let bin = app.data();
        stand_in(bin, "curl", r#"while [ $# -gt 0 ]; do [ "$1" = -o ] && echo tampered > "$2"; shift; done"#);
        stand_in(bin, "osascript", "echo prompted >&2; exit 0");
        let (file, sha256) = mac_image("aarch64");
        let (code, stderr) = run_sh(&mac_script(&file, sha256), &["true"], bin);
        assert_eq!(code, 1, "{stderr}");
        assert!(stderr.contains("SHA-256 doesn't match") && !stderr.contains("prompted"), "{stderr}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_declined_password_prompt_fails_the_step_and_says_so() {
        let app = crate::testing::test_app::TestApp::new();
        let bin = app.data();
        let empty_sha = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        stand_in(bin, "curl", r#"while [ $# -gt 0 ]; do [ "$1" = -o ] && : > "$2"; shift; done"#);
        stand_in(bin, "osascript", "echo '0:0: execution error: User canceled. (-128)' >&2; exit 1");
        let (code, stderr) = run_sh(&mac_script("x.dmg", empty_sha), &["true"], bin);
        assert_eq!(code, 1, "{stderr}");
        assert!(stderr.contains("password prompt was cancelled"), "{stderr}");
    }

    #[test]
    fn the_windows_install_checks_hashicorps_file_and_signature_and_elevates_through_uac() {
        let step = windows_install();
        assert_eq!(step.program, "powershell");
        let script = step.args.last().unwrap();
        assert!(script.contains("https://releases.hashicorp.com/vagrant-vmware-utility/1.0.24/vagrant-vmware-utility_1.0.24_windows_amd64.msi"));
        assert!(script.contains("-Verb RunAs") && script.contains("catch { exit 1223 }"), "a declined prompt reads as cancelled: {script}");
        let elevated = windows_elevated(WINDOWS_MSI.1);
        let at = |needle: &str| elevated.find(needle).unwrap_or_else(|| panic!("{needle} missing: {elevated}"));
        assert!(
            at("Copy-Item") < at("Get-FileHash") && at("Get-FileHash") < at("Get-AuthenticodeSignature") && at("Get-AuthenticodeSignature") < at("msiexec.exe")
        );
        assert!(elevated.contains("'HashiCorp, Inc.'") && elevated.contains("$env:SystemRoot"), "{elevated}");
        // The elevated script is one PowerShell string inside the player's: every quote doubled.
        assert!(script.contains(&ps_quoted(&elevated)));
        assert_eq!(ps_quoted("it's"), "'it''s'");
        assert!(windows_uninstall().args.last().unwrap().contains(WINDOWS_PRODUCT_CODE));
    }

    #[test]
    fn every_pinned_checksum_is_a_sha256() {
        for sha in [mac_image("aarch64").1, WINDOWS_MSI.1] {
            assert!(sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "{sha}");
        }
        assert!(WINDOWS_MSI.0.contains(VERSION));
    }
}
