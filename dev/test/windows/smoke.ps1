<#
Windows install-and-launch edge cases for the NSIS installer: what package-smoke.yml checks
(install, launch, still up), plus the ways a player's machine differs from a CI runner.

  pwsh -File dev\test\windows\smoke.ps1 -Installer src-tauri\target\release\bundle\nsis\Cyber.CTF_1.4.1_x64-setup.exe

Installs into a throwaway folder whose path has spaces and non-ASCII characters, runs each
scenario, prints a PASS/FAIL table, then uninstalls. The app's data and logs folders are left
in place (the uninstaller keeps them too); files this script corrupts on purpose are restored.
Exit code: the number of failed scenarios.
#>
param(
    [Parameter(Mandatory)] [string] $Installer,
    [int] $UpSeconds = 15,
    # Spaces and a non-ASCII "é", built from its code point: Windows PowerShell 5.1 reads a
    # BOM-less script as ANSI, so a literal one would arrive garbled.
    [string] $InstallDir = (Join-Path $env:TEMP "Cyber CTF smoke t$([char]0xE9)st\app")
)

$ErrorActionPreference = 'Stop'
$identifier = 'org.cyberctf.desktop'
$dataDir = Join-Path $env:APPDATA $identifier
$logFile = Join-Path $env:LOCALAPPDATA "$identifier\logs\cyberctf.log"
$results = [System.Collections.Generic.List[object]]::new()

function Record([string] $name, [bool] $ok, [string] $detail = '') {
    $results.Add([pscustomobject]@{ Scenario = $name; Result = $(if ($ok) { 'PASS' } else { 'FAIL' }); Detail = $detail })
    Write-Host ("[{0}] {1} {2}" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $name, $detail)
}

function AppProcesses { Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($InstallDir, 'OrdinalIgnoreCase') } }
function StopApp { AppProcesses | Stop-Process -Force -ErrorAction SilentlyContinue; Start-Sleep 2 }
function LogLines { if (Test-Path $logFile) { (Get-Content $logFile -Encoding UTF8).Count } else { 0 } }

function Launch([string[]] $arguments = @()) {
    $p = if ($arguments.Count) { Start-Process -FilePath $script:exe -ArgumentList $arguments -PassThru } else { Start-Process -FilePath $script:exe -PassThru }
    $p
}

function UpFor([System.Diagnostics.Process] $p, [int] $seconds) {
    Start-Sleep $seconds
    $p.Refresh()
    -not $p.HasExited
}

# --- Install ------------------------------------------------------------------------------
$Installer = (Resolve-Path $Installer).Path
if (Test-Path $InstallDir) { Remove-Item $InstallDir -Recurse -Force }
$setup = Start-Process -FilePath $Installer -ArgumentList '/S', "/D=$InstallDir" -Wait -PassThru
$script:exe = Get-ChildItem $InstallDir -Filter *.exe -ErrorAction SilentlyContinue | Where-Object { $_.Name -notmatch 'unins' } | Select-Object -First 1 -ExpandProperty FullName
Record 'silent install into a path with spaces and non-ASCII' ($setup.ExitCode -eq 0 -and $exe) "exit $($setup.ExitCode), $exe"
if (-not $exe) { $results | Format-Table -AutoSize; exit 1 }

# --- First launch -------------------------------------------------------------------------
StopApp
$lines = LogLines
$app = Launch
$up = UpFor $app $UpSeconds
$app.Refresh()
Record "first launch stays up ${UpSeconds}s" $up $(if ($up) { "pid $($app.Id), window '$($app.MainWindowTitle)'" } else { "exited $($app.ExitCode)" })
Record 'window is responding' ($up -and $app.Responding) ''
$started = @(Get-Content $logFile -Encoding UTF8 -ErrorAction SilentlyContinue | Select-Object -Skip $lines | Select-String 'app starting')
Record 'log file written, with the startup line' ($started.Count -ge 1) $logFile

# --- Second instance ----------------------------------------------------------------------
$second = Launch
$secondGone = $second.WaitForExit(10000)
$app.Refresh()
Record 'second launch hands over and exits' ($secondGone -and -not $app.HasExited) "second exit $(if ($secondGone) { $second.ExitCode } else { 'still running' })"
$count = @(AppProcesses).Count
Record 'one app process after the second launch' ($count -eq 1) "$count running"

# --- Malformed deep link while running -----------------------------------------------------
foreach ($link in @('cyberctf://', 'cyberctf://lab/..%2F..%2Fetc', "cyberctf://lab/$('x' * 5000)", 'cyberctf://lab/%E9%ZZ')) {
    $p = Launch @($link)
    $null = $p.WaitForExit(10000)
}
$app.Refresh()
Record 'malformed cyberctf:// links do not crash the app' (-not $app.HasExited) ''

# --- Deploy worker with a bad job --------------------------------------------------------
$tmp = Join-Path $env:TEMP "cyberctf-smoke-$PID"
New-Item -ItemType Directory -Force $tmp | Out-Null
$missing = Start-Process -FilePath $exe -ArgumentList 'deploy', '--job', (Join-Path $tmp 'missing.json') -PassThru
$exitedMissing = $missing.WaitForExit(15000)
Set-Content (Join-Path $tmp 'garbage.json') '{ not json' -Encoding UTF8
$garbage = Start-Process -FilePath $exe -ArgumentList 'deploy', '--job', (Join-Path $tmp 'garbage.json') -PassThru
$exitedGarbage = $garbage.WaitForExit(15000)
Record 'deploy worker with a missing/garbage job exits non-zero, no hang' ($exitedMissing -and $exitedGarbage -and $missing.ExitCode -ne 0 -and $garbage.ExitCode -ne 0) "exit $($missing.ExitCode) / $($garbage.ExitCode)"
$app.Refresh()
Record 'app unaffected by the failed workers' (-not $app.HasExited) ''

# --- Killed, then relaunched -------------------------------------------------------------
StopApp
$app = Launch
Record 'relaunch after a hard kill' (UpFor $app $UpSeconds) ''

# --- Corrupt settings files --------------------------------------------------------------
StopApp
$backups = @{}
foreach ($name in 'shared-folder.json', 'installed-tools.json') {
    $f = Join-Path $dataDir $name
    if (Test-Path $f) { $backups[$f] = [IO.File]::ReadAllBytes($f) }
    New-Item -ItemType Directory -Force $dataDir | Out-Null
    [IO.File]::WriteAllBytes($f, [byte[]](0xFF, 0xFE, 0x00, 0x7B, 0x22))
}
$lines = LogLines
$app = Launch
$up = UpFor $app $UpSeconds
Record 'launch with corrupt settings files' $up ''
StopApp
foreach ($name in 'shared-folder.json', 'installed-tools.json') {
    $f = Join-Path $dataDir $name
    if ($backups.ContainsKey($f)) { [IO.File]::WriteAllBytes($f, $backups[$f]) } else { Remove-Item $f -ErrorAction SilentlyContinue }
}

# --- Read-only app data folder -----------------------------------------------------------
$acl = Get-Acl $dataDir
try {
    $deny = New-Object System.Security.AccessControl.FileSystemAccessRule($env:USERNAME, 'Write,Delete,DeleteSubdirectoriesAndFiles', 'ContainerInherit,ObjectInherit', 'None', 'Deny')
    $locked = Get-Acl $dataDir; $locked.AddAccessRule($deny); Set-Acl $dataDir $locked
    $app = Launch
    Record 'launch with a read-only app data folder' (UpFor $app $UpSeconds) ''
    StopApp
} finally {
    Set-Acl $dataDir $acl
}

# --- No panic recorded during the run -----------------------------------------------------
$panics = @(Get-Content $logFile -Encoding UTF8 -ErrorAction SilentlyContinue | Select-String -SimpleMatch 'panic on thread')
Record 'no panic in the log' ($panics.Count -eq 0) $(if ($panics.Count) { $panics[0].Line } else { '' })

# --- Uninstall ----------------------------------------------------------------------------
StopApp
$uninstaller = Get-ChildItem $InstallDir -Filter 'uninstall*.exe' | Select-Object -First 1 -ExpandProperty FullName
$un = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -PassThru
Start-Sleep 3 # the NSIS uninstaller copies itself to %TEMP% and returns before it finishes
$gone = -not (Test-Path $exe)
Record 'silent uninstall removes the app' ($un.ExitCode -eq 0 -and $gone) "exit $($un.ExitCode)"
Record 'uninstall keeps the player''s data and logs' ((Test-Path $dataDir) -and (Test-Path $logFile)) ''
Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue

$results | Format-Table -AutoSize
$failed = @($results | Where-Object Result -eq 'FAIL').Count
Write-Host "$failed failed of $($results.Count). Log: $logFile"
exit $failed
