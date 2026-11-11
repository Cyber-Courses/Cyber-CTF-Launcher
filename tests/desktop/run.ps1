<#
The desktop end-to-end on Windows: run.sh's counterpart, the same specs in the same order, the
same knobs and the same summary (a line per spec, then one per check), for the test rig and a
developer alike. Must run in the logged-in desktop session (a scheduled task of that account, not
an SSH session): the app opens windows, and Docker Desktop and the credential store live there.

  pwsh -File tests\desktop\run.ps1 -All
  pwsh -File tests\desktop\run.ps1 -Specs "smoke settings" -Knobs $HOME\.config\cyberctf-test\e2e.env
  pwsh -File tests\desktop\run.ps1 -App C:\path\to\cyberctf-desktop.exe
  pwsh -File tests\desktop\run.ps1 -Package C:\path\to\Cyber.CTF_1.8.9_x64_en-US.msi

-All        every spec (the default)
-Specs      only these specs (space-separated, as ONLY)
-App        the app to drive, built with --features wdio (default: the debug build, built here)
-Package    an .msi (unpacked with msiexec /a, not installed) or an NSIS setup .exe (installed
            silently into the run's folder) of such a build
-Knobs      a KEY=value file (read literally), set in the environment first
-SkipBuild  use the debug build as it is

Knobs: as run.sh's (CYBERCTF_TEST_LOCAL_LAB, CYBERCTF_TEST_SESSION, CYBERCTF_TEST_VM_PROVIDER,
CYBERCTF_TEST_SERVER_ENV, ...; docs/testing/desktop-e2e.md). Exit status: 0 nothing failed (and
no skip under STRICT), 1 a spec failed, 2 the runner itself couldn't run.
#>
param(
  [switch]$All,
  [string]$Specs = "",
  [string]$App = "",
  [string]$Package = "",
  [string]$Knobs = "",
  [switch]$SkipBuild
)
$ErrorActionPreference = "Continue"
$Repo = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
Set-Location $Repo
$State = Join-Path $Repo "tests\desktop\.state"
$Logs = Join-Path $State "logs"
New-Item -ItemType Directory -Force -Path $Logs | Out-Null

function Stop-Runner([string]$why) { Write-Host "run.ps1: $why"; exit 2 }
function Say([string]$what) { Write-Host ""; Write-Host "==== $what" }

# --- Knobs ------------------------------------------------------------------------------------
if ($Knobs) {
  if (-not (Test-Path $Knobs)) { Stop-Runner "no knobs file: $Knobs" }
  foreach ($line in Get-Content $Knobs) {
    if (-not $line -or $line.StartsWith("#") -or -not $line.Contains("=")) { continue }
    $i = $line.IndexOf("=")
    [Environment]::SetEnvironmentVariable($line.Substring(0, $i), $line.Substring($i + 1), "Process")
  }
}
if ($Specs) { $env:ONLY = $Specs } elseif ($All) { $env:ONLY = "" }
if (-not $env:TAURI_WEBDRIVER_PORT) { $env:TAURI_WEBDRIVER_PORT = "4445" }
$Port = $env:TAURI_WEBDRIVER_PORT
$DevPort = if ($env:DEV_URL_PORT) { $env:DEV_URL_PORT } else { "3206" }
$CaseTimeout = if ($env:CASE_TIMEOUT) { [int]$env:CASE_TIMEOUT } else { 1800 }
$VmTimeout = 10800

function Wanted([string]$name) {
  if ($env:ONLY -and -not (" $($env:ONLY) ").Contains(" $name ")) { return $false }
  if ($env:SKIP -and (" $($env:SKIP) ").Contains(" $name ")) { return $false }
  return $true
}

# --- Results (scripts/lib/suite.sh's format, which the rig reads) ------------------------------
$Results = New-Object System.Collections.Generic.List[string]
$script:Failed = 0
$script:Skipped = 0
function Record([string]$name, [string]$status, [int]$secs, [string]$detail, [switch]$NotCounted) {
  $Results.Add(("{0,-20} {1,-5} {2,6}s  {3}" -f $name, $status, $secs, $detail))
  if ($NotCounted) { return }
  if ($status -eq "FAIL") { $script:Failed++ }
  if ($status -eq "skip") { $script:Skipped++ }
}
function Skip-Spec([string]$name, [string]$why) {
  if (-not (Wanted $name)) { return }
  Say "${name}: skipped ($why)"
  Record $name "skip" 0 $why
}

# --- The app ----------------------------------------------------------------------------------
if (-not (Get-Command node -ErrorAction SilentlyContinue)) { Stop-Runner "node is not installed" }
if (-not (Test-Path (Join-Path $Repo "node_modules\webdriverio"))) {
  & pnpm install --frozen-lockfile
  if ($LASTEXITCODE -ne 0) { Stop-Runner "pnpm install failed" }
}
if ($Package) {
  if (-not (Test-Path $Package)) { Stop-Runner "no package: $Package" }
  $Unpacked = Join-Path $State "package"
  Remove-Item -Recurse -Force $Unpacked -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force -Path $Unpacked | Out-Null
  $full = (Resolve-Path $Package).Path
  if ($full.EndsWith(".msi")) {
    Start-Process msiexec.exe -Wait -ArgumentList @("/a", "`"$full`"", "/qn", "TARGETDIR=`"$Unpacked`"")
  } elseif ($full.EndsWith(".exe")) {
    Start-Process $full -Wait -ArgumentList @("/S", "/D=$Unpacked")
  } else { Stop-Runner "not a package this runner unpacks (.msi, NSIS .exe): $Package" }
  $found = Get-ChildItem -Recurse -Path $Unpacked -Filter *.exe | Where-Object { $_.Name -notmatch "^(uninstall|unins)" } | Select-Object -First 1
  if (-not $found) { Stop-Runner "no app in $Package" }
  $App = $found.FullName
}
if (-not $App) {
  $App = Join-Path $Repo "src-tauri\target\debug\cyberctf-desktop.exe"
  if (-not $SkipBuild -and -not $env:SKIP_BUILD) {
    Say "cargo build --features wdio"
    & cargo build --manifest-path src-tauri\Cargo.toml --features wdio
    if ($LASTEXITCODE -ne 0) { Stop-Runner "the wdio build failed" }
  }
}
if (-not (Test-Path $App)) { Stop-Runner "app binary not found: $App" }
$env:APP_BIN = $App
$Debug = $App -match "\\debug\\"

function Stop-App {
  Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $App } | Stop-Process -Force -ErrorAction SilentlyContinue
}
function Serves([string]$port) {
  try { Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$port/" -TimeoutSec 3 | Out-Null; return $true } catch { return $false }
}

# --- The frontend a debug build loads (devUrl); a release build embeds it -----------------------
$NextDev = $null
if ($Debug -and -not (Serves $DevPort)) {
  Say "frontend: next dev on port $DevPort (a debug build loads devUrl)"
  $NextDev = Start-Process -PassThru -WindowStyle Hidden -FilePath "cmd.exe" -ArgumentList @("/c", "pnpm exec next dev -p $DevPort > `"$Logs\next-dev.log`" 2>&1")
  for ($i = 0; $i -lt 90 -and -not (Serves $DevPort); $i++) { Start-Sleep -Seconds 2 }
  if (-not (Serves $DevPort)) { Stop-Runner "next dev never answered on $DevPort" }
}

# --- A signed-in session (a debug build's file; a release build reads the credential store) -----
$SessionFile = Join-Path $env:USERPROFILE ".cyberctf\dev-session.json"
$SessionBackup = $null
$SessionPlaced = $false
if ($env:CYBERCTF_TEST_SESSION) {
  if (-not (Test-Path $env:CYBERCTF_TEST_SESSION)) { Stop-Runner "no session file: $($env:CYBERCTF_TEST_SESSION)" }
  if ($Debug) {
    New-Item -ItemType Directory -Force -Path (Split-Path $SessionFile) | Out-Null
    if (Test-Path $SessionFile) { $SessionBackup = Join-Path $State "dev-session.backup.json"; Copy-Item $SessionFile $SessionBackup -Force }
    Copy-Item $env:CYBERCTF_TEST_SESSION $SessionFile -Force
    $SessionPlaced = $true
    Say "session: placed $SessionFile for the run"
  } else { Say "session: a release build reads the credential store, CYBERCTF_TEST_SESSION is ignored" }
}

# --- Fixtures ---------------------------------------------------------------------------------
& docker info *> $null
$DockerOk = ($LASTEXITCODE -eq 0)
if (-not $env:CYBERCTF_TEST_LOCAL_LAB) {
  $lab = Join-Path $State "labs\invoice-portal-api"
  if (Test-Path (Join-Path $lab ".git")) { & git -C $lab pull -q } else { & git clone -q https://github.com/CyberCTF/invoice-portal-api $lab }
  $env:CYBERCTF_TEST_LOCAL_LAB = $lab
}
if (-not (Test-Path (Join-Path $env:CYBERCTF_TEST_LOCAL_LAB "isoloom.yml"))) { Stop-Runner "not a lab folder: $($env:CYBERCTF_TEST_LOCAL_LAB)" }
$LabName = ((Select-String -Path (Join-Path $env:CYBERCTF_TEST_LOCAL_LAB "isoloom.yml") -Pattern '^name:\s*(.+)$' | Select-Object -First 1).Matches[0].Groups[1].Value).Trim().Trim('"', "'")
$slug = ($LabName.ToLower() -replace '[^a-z0-9_-]', '-').Trim('-')
if ($slug.Length -gt 56) { $slug = $slug.Substring(0, 56) }
$LabId = "local-$slug"

function Clear-Lab([switch]$Final) {
  if ($Final -and $env:KEEP) { return }
  if (-not $DockerOk) { return }
  if ((& docker compose ls -q 2>$null) -contains "cyberctf-$LabId") {
    Say "cleanup: docker compose -p cyberctf-$LabId down"
    & docker compose -p "cyberctf-$LabId" down --remove-orphans *> $null
  }
  $left = & docker ps -q --filter "name=$LabId" 2>$null
  if ($left) { & docker rm -f $left *> $null }
}

# --- Running the specs ------------------------------------------------------------------------
# Invoke-Logged <name> <timeout> <knobs hashtable> <program> <args>: alone, its output in its log,
# killed past its timeout; exit 0 passes, 2 skips (its `skipped: spec` line says why).
function Invoke-Logged([string]$name, [int]$limit, [hashtable]$vars, [string]$program, [string[]]$arguments) {
  if (-not (Wanted $name)) { return }
  Stop-App
  Say $name
  $log = Join-Path $Logs "$name.log"
  $saved = @{}
  foreach ($k in $vars.Keys) { $saved[$k] = [Environment]::GetEnvironmentVariable($k, "Process"); [Environment]::SetEnvironmentVariable($k, $vars[$k], "Process") }
  $start = Get-Date
  $p = Start-Process -PassThru -NoNewWindow -FilePath $program -ArgumentList $arguments -RedirectStandardOutput $log -RedirectStandardError "$log.err"
  $null = $p.Handle
  if (-not $p.WaitForExit($limit * 1000)) {
    & taskkill /T /F /PID $p.Id *> $null
    Add-Content $log "run.ps1: $name exceeded its ${limit}s"
  }
  $p.WaitForExit()
  $rc = $p.ExitCode
  foreach ($k in $vars.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k], "Process") }
  if (Test-Path "$log.err") { Get-Content "$log.err" | Add-Content $log; Remove-Item "$log.err" }
  Stop-App
  $lines = Select-String -Path $log -Pattern '^(ok|FAILED|skipped): ' | ForEach-Object { $_.Line }
  if ($lines) { $lines | ForEach-Object { Write-Host "   $_" } } else { Get-Content $log -Tail 4 | ForEach-Object { Write-Host "   $_" } }
  $secs = [int]((Get-Date) - $start).TotalSeconds
  if ($rc -eq 0) { Record $name "ok" $secs $log }
  elseif ($rc -eq 2) {
    $why = ($lines | Where-Object { $_ -like "skipped: spec (0s) *" } | Select-Object -First 1)
    if ($why) { $why = $why.Substring("skipped: spec (0s) ".Length) } else { $why = "skipped (see $log)" }
    Record $name "skip" $secs $why
  } else { Record $name "FAIL" $secs "$log (exit $rc)" }
  foreach ($l in $lines) {
    if ($l -match '^(ok|FAILED|skipped): (\S+) \((\d+)s\) (.*)$' -and $Matches[2] -ne "spec") {
      $status = switch ($Matches[1]) { "ok" { "ok" } "FAILED" { "FAIL" } default { "skip" } }
      Record "$name/$($Matches[2])" $status ([int]$Matches[3]) $Matches[4] -NotCounted
    }
  }
}
function Invoke-Spec([string]$name, [string]$file = "", [int]$limit = 0, [hashtable]$vars = @{}) {
  if (-not $file) { $file = $name }
  if ($limit -eq 0) { $limit = $CaseTimeout }
  Invoke-Logged $name $limit $vars "node" @("tests/desktop/specs/$file.mjs")
}

try {
  Say "app: $App"
  Say "lab: $($env:CYBERCTF_TEST_LOCAL_LAB) ($LabName, id $LabId)"
  if (-not $DockerOk) { Say "docker is not answering: the specs that run a lab here are skipped" }

  # The embedded WebDriver server answers, or nothing below can.
  $pre = Start-Process -PassThru -FilePath $App -RedirectStandardOutput "$Logs\preflight.log" -RedirectStandardError "$Logs\preflight.err"
  $up = $false
  for ($i = 0; $i -lt 60 -and -not $up; $i++) {
    try { Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$Port/status" -TimeoutSec 2 | Out-Null; $up = $true } catch { Start-Sleep -Seconds 1 }
  }
  Stop-Process -Id $pre.Id -Force -ErrorAction SilentlyContinue
  Stop-App
  if (-not $up) { Stop-Runner "no embedded WebDriver server on ${Port}: the app must be built with --features wdio" }

  # The same order as run.sh.
  Invoke-Logged "smoke" $CaseTimeout @{} "powershell.exe" @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "tests\desktop\lib\smoke.ps1")
  foreach ($s in "title-bar", "settings", "onboarding", "setup-screens", "machine") { Invoke-Spec $s }
  Clear-Lab
  foreach ($s in "labs", "deep-link", "fail-offline", "fail-badlab") { Invoke-Spec $s }
  $signedOut = Join-Path $env:TEMP "cyberctf-signedout-$PID"
  New-Item -ItemType Directory -Force -Path $signedOut | Out-Null
  Invoke-Spec "fail-signedout" -vars @{ USERPROFILE = $signedOut; HOME = $signedOut }
  Remove-Item -Recurse -Force $signedOut -ErrorAction SilentlyContinue
  $dockerSpecs = "fail-docker", "launch", "restart", "stop", "launch-ui", "kill-mid-deploy", "attack-box", "attack-shell", "stop-with-box", "park-resume", "reimport-running", "attack-on-stopped", "lab-actions", "offline-start"
  if ($DockerOk) {
    Invoke-Spec "fail-docker"
    Clear-Lab
    foreach ($s in "launch", "restart", "stop") { Invoke-Spec $s }
    foreach ($s in ($dockerSpecs | Select-Object -Skip 4)) { Clear-Lab; Invoke-Spec $s }
    Invoke-Spec "form-container" "lab-form" 0 @{ CYBERCTF_TEST_FORM = "container" }
  } else {
    foreach ($s in ($dockerSpecs + "form-container")) { Skip-Spec $s "docker is not running" }
  }
  Invoke-Spec "form-docker-vm" "lab-form" $VmTimeout @{ CYBERCTF_TEST_FORM = "docker-vm" }
  Invoke-Spec "form-vm" "lab-form" $VmTimeout @{ CYBERCTF_TEST_FORM = "vm" }
  Invoke-Spec "form-per-machine" "lab-form" $VmTimeout @{ CYBERCTF_TEST_FORM = "per-machine" }
  Invoke-Spec "screens" "screens" $VmTimeout
  Skip-Spec "vm-scan" "its fixture VM is made on a Mac (UTM); on Hyper-V start a VM named <lab>-<machine> and run tests/desktop/specs/vm-scan.mjs with CYBERCTF_EXPECT_LAB"
  Invoke-Spec "server-lab" "server-lab" $VmTimeout
  Clear-Lab -Final
  Invoke-Spec "cleanup"
} finally {
  Stop-App
  if ($NextDev) { & taskkill /T /F /PID $NextDev.Id *> $null }
  if ($SessionPlaced) {
    if ($SessionBackup) { Move-Item -Force $SessionBackup $SessionFile } else { Remove-Item -Force $SessionFile -ErrorAction SilentlyContinue }
  }
}

Say "desktop e2e, logs in $Logs"
foreach ($l in $Results) { Write-Host "   $l" }
Write-Host ""
Write-Host "$($script:Failed) failed, $($script:Skipped) skipped"
if ($script:Failed -gt 0) { exit 1 }
if ($env:STRICT -and $script:Skipped -gt 0) { exit 1 }
exit 0
