# Written by `cargo test` (machine/selftest/vm.rs) from the app's own removal: don't edit.
# Run by installer-hooks.nsh before the uninstaller removes the app's folders.
$ErrorActionPreference = 'Stop'
$vms = @(Get-VM -Name 'CyberCTF test VM' -ErrorAction SilentlyContinue | Where-Object { $_.Name -ceq 'CyberCTF test VM' })
if ($vms.Count -eq 0) { exit 0 }
foreach ($vm in $vms) {
  $disks = @(Get-VMHardDiskDrive -VM $vm | ForEach-Object { $_.Path })
  $outside = @($disks | Where-Object { -not $_.StartsWith("$env:LOCALAPPDATA\org.cyberctf.desktop\selftest\vm\", [System.StringComparison]::OrdinalIgnoreCase) })
  if ($disks.Count -eq 0 -or $outside.Count -gt 0) { exit 20 }
}
foreach ($vm in $vms) { Stop-VM -VM $vm -TurnOff -Force; Remove-VM -VM $vm -Force }
exit 10
