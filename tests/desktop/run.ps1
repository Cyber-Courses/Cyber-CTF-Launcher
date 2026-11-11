<#
The desktop end-to-end on Windows: run.sh's counterpart, the same specs in the same order, the
same knobs and the same summary (a line per spec, then one per check), for the test rig and a
developer alike. Must run in the logged-in desktop session (a scheduled task of that account, not
an SSH session): the app opens windows, and Docker Desktop and the credential store live there.

  pwsh -File tests\desktop\run.ps1 -All
  pwsh -File tests\desktop\run.ps1 -Specs "smoke settings" -Knobs $HOME\.config\cyberctf-test\e2e.env
  pwsh -File tests\desktop\run.ps1 -App C:\path\to\cyberctf-desktop.exe
  pwsh -File tests\desktop\run.ps1 -Package C:\path\to\Cyber.CTF_1.8.9_x64_en-US.msi
  pwsh -File tests\desktop\run.ps1 -Results results.jsonl
  pwsh -File tests\desktop\run.ps1 -List

-All        every spec (the default)
-Specs      only these specs (space-separated, as ONLY)
-App        the app to drive, built with --features wdio (CYBERCTF_TEST_DESKTOP_APP; default: the
            debug build, built here)
-Package    an .msi (unpacked with msiexec /a, not installed) or an NSIS setup .exe (installed
            silently into the run's folder) of such a build
-Knobs      a KEY=value file (read literally), set in the environment first
-SkipBuild  use the debug build as it is
-Results    append a JSON record per spec and per check to this file (run.sh's --results;
            tests/README.md, "Results")
-List       print a JSON line per case (cases.tsv: its id, spec, form and knobs) and exit (run.sh's
            --list)

Safe on a machine someone uses, as run.sh: the app keeps all of its own in a folder made for the
run (CYBERCTF_WDIO_ROOT, under %TEMP%), removed at the end, and every clean-up acts only on what
the run made, by the records it wrote (lib\owned.mjs).

Knobs: as run.sh's (CYBERCTF_TEST_LOCAL_LAB, CYBERCTF_TEST_ACCOUNT_SESSION,
CYBERCTF_TEST_VM_PROVIDER, CYBERCTF_TEST_SERVER_ENV, ...; tests/README.md lists each). Exit status: 0 nothing failed (and
no skip under STRICT), 1 a spec failed, 2 the runner itself couldn't run.
#>
param(
  [switch]$All,
  [string]$Specs = "",
  [string]$App = "",
  [string]$Package = "",
  [string]$Knobs = "",
  [switch]$SkipBuild,
  [string]$Results = "",
  [switch]$List
)
$ErrorActionPreference = "Continue"
$Repo = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
. (Join-Path $PSScriptRoot "lib\suite.ps1")
Read-Case (Join-Path $PSScriptRoot "cases.tsv")
if ($List) { Get-CaseList; exit 0 }
if ($Results) { Initialize-ResultsFile $Results }
Set-Location $Repo
$State = Join-Path $Repo "tests\desktop\.state"
$Logs = Join-Path $State "logs"
New-Item -ItemType Directory -Force -Path $Logs | Out-Null

# Before the specs run, the run's folder goes with it (once they run, the finally below sees to it).
function Stop-Runner([string]$why) {
  Write-Host "run.ps1: $why"
  if ($script:RunRoot -and -not $script:Running) { Close-RunRoot }
  exit 2
}
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
# Where a debug build loads its frontend from: tauri.conf.json's devUrl, compiled into the app.
$DevPort = ((Get-Content (Join-Path $Repo "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json).build.devUrl -replace '^.*:(\d+).*$', '$1')
$CaseTimeout = if ($env:CYBERCTF_TEST_DESKTOP_TIMEOUT) { [int]$env:CYBERCTF_TEST_DESKTOP_TIMEOUT } else { 1800 }
$VmTimeout = 10800

function Skip-Spec([string]$name, [string]$why) {
  if (-not (Test-Wanted $name)) { return }
  Say "${name}: skipped ($why)"
  Add-Result $name "skip" 0 $why (Get-CaseForm $name) (Get-CaseProvider $name)
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
if (-not $App -and $env:CYBERCTF_TEST_DESKTOP_APP) { $App = $env:CYBERCTF_TEST_DESKTOP_APP }
if (-not $App) {
  $App = Join-Path $Repo "src-tauri\target\debug\cyberctf-desktop.exe"
  if (-not $SkipBuild) {
    Say "cargo build --features wdio"
    & cargo build --manifest-path src-tauri\Cargo.toml --features wdio
    if ($LASTEXITCODE -ne 0) { Stop-Runner "the wdio build failed" }
  }
}
if (-not (Test-Path $App)) { Stop-Runner "app binary not found: $App" }
$env:CYBERCTF_TEST_DESKTOP_APP = $App
$Debug = $App -match "\\debug\\"

# --- The run's own folder ---------------------------------------------------------------------
# The app (a wdio build) keeps everything of its own under CYBERCTF_WDIO_ROOT (src-tauri's
# platform/test_root.rs; lib\run-root.sh says what goes where), removed at the end unless
# CYBERCTF_TEST_DESKTOP_KEEP.
$RunRoot = Initialize-RunRoot
$env:CYBERCTF_WDIO_ROOT = $RunRoot
$Owned = Join-Path $Repo "tests\desktop\lib\owned.mjs"
function Close-RunRoot {
  $said = Clear-RunRoot $RunRoot
  if ($said) { Say $said }
}
# What is on the machine before the run: what the run downloads is told apart from it.
& node $Owned record $RunRoot
if ($LASTEXITCODE -ne 0) { Stop-Runner "couldn't record what is on the machine" }

# The run's own apps still up, stopped, never another copy of the same binary: the ones the run
# launched (records\apps\<pid>, by support.mjs, smoke.ps1 and the preflight; fail-signedout's
# under signed-out\), while that pid is still the app, and the deploy workers whose job is in the
# run's folder.
function Stop-App {
  foreach ($dir in (Join-Path $RunRoot "records\apps"), (Join-Path $RunRoot "signed-out\records\apps")) {
    Get-ChildItem -Path $dir -File -ErrorAction SilentlyContinue | ForEach-Object {
      $p = Get-Process -Id ([int]$_.Name) -ErrorAction SilentlyContinue
      if ($p -and $p.Path -eq $App) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
      Remove-Item $_.FullName -Force -ErrorAction SilentlyContinue
    }
  }
  Get-CimInstance Win32_Process -Filter "Name = '$(Split-Path -Leaf $App)'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -and $_.CommandLine.Contains("deploy --job") -and $_.CommandLine.Contains($RunRoot) } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
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

# --- A signed-in session: the run's app keeps it in a file under the run's folder whatever the
# build, never the user's file or credential store.
if ($env:CYBERCTF_TEST_ACCOUNT_SESSION) {
  if (-not (Test-Path $env:CYBERCTF_TEST_ACCOUNT_SESSION)) { Stop-Runner "no session file: $($env:CYBERCTF_TEST_ACCOUNT_SESSION)" }
  Copy-RunSession $RunRoot $env:CYBERCTF_TEST_ACCOUNT_SESSION
  Say "session: placed in the run's folder"
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

# The Compose projects here, `ours` (the run's: a lab installed in its folder, started from there)
# or `theirs` (any other, the user's own copy of a lab of the same name included): lib\owned.mjs.
function Get-Project([string]$whose) {
  @(& node $Owned projects $RunRoot | ForEach-Object { $f = $_ -split "`t"; if ($f[0] -eq $whose) { $f[1] } })
}
# What a run leaves behind: its labs' containers, only the run's projects (as run.sh's cleanup_lab).
function Clear-RunProject {
  if (-not $DockerOk) { return }
  foreach ($p in (Get-Project "ours")) {
    Say "cleanup: docker compose -p $p down (the run's)"
    & docker compose -p $p down --remove-orphans *> $null
  }
}
function Clear-Lab([switch]$Final) {
  if ($Final -and $env:CYBERCTF_TEST_DESKTOP_KEEP) { return }
  Clear-RunProject
  $left = if ($DockerOk) { Get-Project "ours" } else { @() }
  if ($left) { Stop-Runner "the run's Compose projects are left after the clean-up: $($left -join ' ')" }
}

# --- Running the specs ------------------------------------------------------------------------
# Invoke-Logged <name> <timeout> <knobs hashtable> <program> <args>: alone, its output in its log,
# killed past its timeout; exit 0 passes, 2 skips (its `skipped: spec` line says why).
function Invoke-Logged([string]$name, [int]$limit, [hashtable]$vars, [string]$program, [string[]]$arguments) {
  if (-not (Test-Wanted $name)) { return }
  # A case that imports the fixture lab leaves alone a copy of it the run didn't start (the user's).
  $c = $script:Cases | Where-Object { $_.Case -eq $name } | Select-Object -First 1
  if ($DockerOk -and $c -and $c.Knobs -contains "CYBERCTF_TEST_LOCAL_LAB" -and (Get-Project "theirs") -contains "cyberctf-$LabId") {
    Skip-Spec $name "a copy of $LabName this run didn't start runs here (Compose project cyberctf-$LabId): left as it is"
    return
  }
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
  $form = Get-CaseForm $name
  $provider = Get-CaseProvider $name
  if ($rc -eq 0) { Add-Result $name "ok" $secs $log $form $provider $log }
  elseif ($rc -eq 2) {
    $why = ($lines | Where-Object { $_ -like "skipped: spec (0s) *" } | Select-Object -First 1)
    if ($why) { $why = $why.Substring("skipped: spec (0s) ".Length) } else { $why = "skipped (see $log)" }
    Add-Result $name "skip" $secs $why $form $provider $log
  } else { Add-Result $name "FAIL" $secs "$log (exit $rc)" $form $provider $log }
  Add-CheckResult $name $lines $form $provider $log
}
function Invoke-Spec([string]$name, [string]$file = "", [int]$limit = 0, [hashtable]$vars = @{}) {
  if (-not $file) { $file = $name }
  if ($limit -eq 0) { $limit = $CaseTimeout }
  Invoke-Logged $name $limit $vars "node" @("tests/desktop/specs/$file.mjs")
}

$script:Running = $true
try {
  Say "app: $App"
  Say "lab: $($env:CYBERCTF_TEST_LOCAL_LAB) ($LabName, id $LabId)"
  if (-not $DockerOk) { Say "docker is not answering: the specs that run a lab here are skipped" }

  # The embedded WebDriver server answers, or nothing below can.
  $pre = Start-Process -PassThru -FilePath $App -RedirectStandardOutput "$Logs\preflight.log" -RedirectStandardError "$Logs\preflight.err"
  New-Item -ItemType Directory -Force -Path (Join-Path $RunRoot "records\apps") | Out-Null
  Set-Content -Path (Join-Path $RunRoot "records\apps\$($pre.Id)") -Value $App
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
  # Signed out: a run folder of its own, with no session in it.
  Invoke-Spec "fail-signedout" -vars @{ CYBERCTF_WDIO_ROOT = (Join-Path $RunRoot "signed-out") }
  $dockerSpecs = "fail-docker", "launch", "restart", "stop", "launch-ui", "kill-mid-deploy", "attack-box", "attack-shell", "stop-with-box", "park-resume", "reimport-running", "attack-on-stopped", "lab-actions", "offline-start"
  if ($DockerOk) {
    Invoke-Spec "fail-docker"
    Clear-Lab
    foreach ($s in "launch", "restart", "stop") { Invoke-Spec $s }
    foreach ($s in ($dockerSpecs | Select-Object -Skip 4)) { Clear-Lab; Invoke-Spec $s }
    Invoke-Spec "lab-form-container" "lab-form" 0 @{ CYBERCTF_TEST_LAB_FORM = "container" }
  } else {
    foreach ($s in ($dockerSpecs + "lab-form-container")) { Skip-Spec $s "docker is not running" }
  }
  Invoke-Spec "lab-form-docker-vm" "lab-form" $VmTimeout @{ CYBERCTF_TEST_LAB_FORM = "docker-vm" }
  Invoke-Spec "lab-form-vm" "lab-form" $VmTimeout @{ CYBERCTF_TEST_LAB_FORM = "vm" }
  Invoke-Spec "lab-form-per-machine" "lab-form" $VmTimeout @{ CYBERCTF_TEST_LAB_FORM = "per-machine" }
  Invoke-Spec "screens" "screens" $VmTimeout
  Skip-Spec "vm-scan" "its fixture VM is made on a Mac (UTM); on Hyper-V start a VM named <lab>-<machine> and run tests/desktop/specs/vm-scan.mjs with CYBERCTF_TEST_SCAN_LAB"
  Invoke-Spec "server-lab" "server-lab" $VmTimeout
  Clear-Lab -Final
  Invoke-Spec "cleanup"
} finally {
  Stop-App
  if ($NextDev) { & taskkill /T /F /PID $NextDev.Id *> $null }
  # An interrupted run leaves none of its labs running on a folder about to go.
  if (-not $env:CYBERCTF_TEST_DESKTOP_KEEP) { Clear-RunProject }
  Close-RunRoot
}

Say "desktop e2e, logs in $Logs"
foreach ($l in $script:SummaryLines) { Write-Host "   $l" }
Write-Host ""
Write-Host "$($script:Failed) failed, $($script:Skipped) skipped"
if ($script:Failed -gt 0) { exit 1 }
if ($env:STRICT -and $script:Skipped -gt 0) { exit 1 }
exit 0
