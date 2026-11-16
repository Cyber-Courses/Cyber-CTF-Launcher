//! HashiCorp's Vagrant VMware Utility: the system service the `vagrant-vmware-desktop` plugin
//! drives VMware Fusion and Workstation through. It isn't a Vagrant plugin, nor in winget or
//! Homebrew's core taps in a form fit for Apple silicon, so it is installed from HashiCorp's own
//! signed package, pinned here with its checksum:
//!
//! - macOS and Linux: isothrum-machine's (`install::macos`, `install::linux`).
//! - Windows: the MSI downloaded and its SHA-256 checked as the player, then, through Windows'
//!   UAC prompt, copied into a folder only administrators can write, checked again (SHA-256 and
//!   Authenticode signature by HashiCorp) and installed with `msiexec`. An install that is there
//!   already only has its service started again.
//!
//! Every path ends in a real result: a declined prompt, a checksum or a signature that doesn't
//! match, or an installer error fails the step, and the machine check then says whether the
//! service answers. Nothing reports success on the strength of an installer merely opened.

use super::steps::Step;
use crate::platform::exec::ps_quote;

/// The release installed. Its files and checksums are HashiCorp's published ones
/// (`releases.hashicorp.com/vagrant-vmware-utility/<version>/…_SHA256SUMS`, signed with HashiCorp's
/// release key); a new release changes the version and every checksum below together.
const VERSION: &str = "1.0.24";

/// Where HashiCorp publishes the release's files.
fn url(file: &str) -> String {
    format!("https://releases.hashicorp.com/vagrant-vmware-utility/{VERSION}/{file}")
}

/// The Windows installer, with its SHA-256.
const WINDOWS_MSI: (&str, &str) = ("vagrant-vmware-utility_1.0.24_windows_amd64.msi", "8ee7bea062d7552a484b7d9ade369744e5cd9ab1b55eed8b7bd35a1b006666d6");

/// That MSI's product code, which `msiexec /x` removes it by.
const WINDOWS_PRODUCT_CODE: &str = "{DE47C4D6-3BD9-4286-A93D-1DCA1CBADDA7}";

/// The elevated half of the Windows install, with the downloaded MSI's path in `$msi` (set by
/// the player's half, before this): an install already there only has its service started
/// again; otherwise the MSI is copied into a folder only administrators write, checked again
/// (SHA-256, then Authenticode signature by HashiCorp) and installed. Exits 0 when done (3010,
/// "restart to finish", is done), 2 on a checksum and 3 on a signature that doesn't match, else
/// msiexec's own code.
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
fn windows_script(file: &str, sha256: &str) -> String {
    let url = url(file);
    let elevated = ps_quote(&windows_elevated(sha256));
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
pub fn windows_install() -> Step {
    let (file, sha256) = WINDOWS_MSI;
    Step {
        program: "powershell".into(),
        args: vec!["-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), windows_script(file, sha256)],
        note: Some(format!("Downloading HashiCorp's Vagrant VMware Utility {VERSION}…")),
    }
}

/// The Windows removal: the pinned release's MSI removed by its product code, through UAC.
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
        assert!(script.contains(&ps_quote(&elevated)));
        assert!(windows_uninstall().args.last().unwrap().contains(WINDOWS_PRODUCT_CODE));
    }

    #[test]
    fn every_pinned_checksum_is_a_sha256() {
        for sha in [WINDOWS_MSI.1] {
            assert!(sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "{sha}");
        }
        assert!(WINDOWS_MSI.0.contains(VERSION));
    }
}
