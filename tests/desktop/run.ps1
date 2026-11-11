<#
The desktop end-to-end on Windows: run.sh's counterpart, the same specs with the same needs, the
same knobs and the same summary (tests\desktop\run.mjs runs the specs; this builds or unpacks the
app and serves a debug build's frontend). Must run in the logged-in desktop session (a scheduled
task of that account, not an SSH session): the app opens windows, and Docker Desktop and the
credential store live there.

  pwsh -File tests\desktop\run.ps1 -All
  pwsh -File tests\desktop\run.ps1 -Group lab
  pwsh -File tests\desktop\run.ps1 -Specs "smoke settings" -Knobs $HOME\e2e.env
  pwsh -File tests\desktop\run.ps1 -App C:\path\to\cyberctf-desktop.exe
  pwsh -File tests\desktop\run.ps1 -Package C:\path\to\Cyber.CTF_1.8.9_x64_en-US.msi
  pwsh -File tests\desktop\run.ps1 -Results results.jsonl
  pwsh -File tests\desktop\run.ps1 -List

-All        every spec (the default)
-Group      only that group's specs (-List says each case's groups; run.sh's --group)
-Specs      only these specs (space-separated, as ONLY)
-App        the app to drive, built with --features wdio (CYBERCTF_TEST_DESKTOP_APP; default: the
            debug build, built here)
-Package    an .msi (unpacked with msiexec /a, not installed) or an NSIS setup .exe (installed
            silently into the run's folder) of such a build
-Knobs      a KEY=value file (read literally), set in the environment first
-SkipBuild  use the debug build as it is
-Results    append a JSON record per spec and per check to this file (run.sh's --results;
            tests/README.md, "Results")
-List       print a JSON line per case (its id, spec, form, knobs, groups, needs) and exit (run.sh's
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
  [string]$Group = "",
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
$Runner = Join-Path $Repo "tests\desktop\run.mjs"
if ($List) {
  $listArgs = @("--list")
  if ($Group) { $listArgs += @("--group", $Group) }
  & node $Runner @listArgs
  exit $LASTEXITCODE
}
Set-Location $Repo
$State = Join-Path $Repo "tests\desktop\.state"
$Logs = if ($env:CYBERCTF_TEST_DESKTOP_LOGS) { $env:CYBERCTF_TEST_DESKTOP_LOGS } else { Join-Path $State "logs" }
New-Item -ItemType Directory -Force -Path $Logs | Out-Null

function Stop-Runner([string]$why) {
  Write-Host "run.ps1: $why"
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
# Where a debug build loads its frontend from: tauri.conf.json's devUrl, compiled into the app.
$DevPort = ((Get-Content (Join-Path $Repo "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json).build.devUrl -replace '^.*:(\d+).*$', '$1')

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

function Serves([string]$port) {
  try { Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$port/" -TimeoutSec 3 | Out-Null; return $true } catch { return $false }
}

# --- The frontend a debug build loads (devUrl); a release build embeds it -----------------------
$NextDev = $null
if ($Debug -and -not (Serves $DevPort)) {
  Say "frontend: next dev on port $DevPort (a debug build loads devUrl)"
  $NextDev = Start-Process -PassThru -WindowStyle Hidden -FilePath "cmd.exe" -ArgumentList @("/c", "pnpm exec next dev --hostname 127.0.0.1 --port $DevPort > `"$Logs\next-dev.log`" 2>&1")
  for ($i = 0; $i -lt 90 -and -not (Serves $DevPort); $i++) { Start-Sleep -Seconds 2 }
  if (-not (Serves $DevPort)) { Stop-Runner "next dev never answered on $DevPort" }
}

# --- The specs --------------------------------------------------------------------------------
$env:CYBERCTF_TEST_DESKTOP_LOGS = $Logs
$runArgs = @()
if ($All) { $runArgs += "--all" }
if ($Group) { $runArgs += @("--group", $Group) }
if ($Specs) { $runArgs += @("--specs", $Specs) }
if ($Results) { $runArgs += @("--results", $Results) }
try {
  & node $Runner @runArgs
  $rc = $LASTEXITCODE
} finally {
  if ($NextDev) { & taskkill /T /F /PID $NextDev.Id *> $null }
}
exit $rc
