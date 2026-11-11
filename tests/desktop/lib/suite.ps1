<#
What run.ps1 shares with its unit test (tests/desktop/run.test.ts), as scripts/lib/suite.sh is
run.sh's: which cases run (ONLY / SKIP), the summary's lines (suite.sh's format), the JSON record
of each case and check for a caller that reads them (tests/README.md, "Results"), the cases'
table (cases.tsv), and the run's own folder (lib/run-root.sh's counterpart). Dot-sourced, never run.
#>

$script:SummaryLines = New-Object System.Collections.Generic.List[string]
$script:Failed = 0
$script:Skipped = 0
$script:ResultsFile = ""
$script:Cases = @()

# Whether ONLY / SKIP let the case run.
function Test-Wanted([string]$name) {
  if ($env:ONLY -and -not (" $($env:ONLY) ").Contains(" $name ")) { return $false }
  if ($env:SKIP -and (" $($env:SKIP) ").Contains(" $name ")) { return $false }
  return $true
}

# The cases' table: id, spec, form and knobs ("-" for none), in the order the runners run them.
function Read-Case([string]$path) {
  $script:Cases = @(Get-Content $path | Where-Object { $_ -and -not $_.StartsWith("#") } | ForEach-Object {
      $f = $_ -split "`t"
      [pscustomobject]@{
        Case  = $f[0]
        Spec  = $f[1]
        Form  = $(if ($f[2] -eq "-") { "" } else { $f[2] })
        Knobs = @($(if ($f[3] -eq "-") { @() } else { $f[3] -split " " }))
      }
    })
}

# A JSON line per case, as run.sh --list prints them.
function Get-CaseList {
  foreach ($c in $script:Cases) {
    ConvertTo-Json -Compress -InputObject ([ordered]@{ suite = "desktop"; case = $c.Case; spec = $c.Spec; form = $c.Form; knobs = @($c.Knobs) })
  }
}

# The form a case runs a lab in, and the hypervisor for a case that reads one ("" for none).
function Get-CaseForm([string]$name) {
  $c = $script:Cases | Where-Object { $_.Case -eq $name } | Select-Object -First 1
  if ($c) { return [string]$c.Form } else { return "" }
}
function Get-CaseProvider([string]$name) {
  $c = $script:Cases | Where-Object { $_.Case -eq $name } | Select-Object -First 1
  if ($c -and $c.Knobs -contains "CYBERCTF_TEST_VM_PROVIDER") { return [string]$env:CYBERCTF_TEST_VM_PROVIDER } else { return "" }
}

# This run's records go to $path (appended; made absolute, as the runner changes folders).
function Initialize-ResultsFile([string]$path) {
  $full = [IO.Path]::GetFullPath([IO.Path]::Combine((Get-Location).Path, $path))
  New-Item -ItemType Directory -Force -Path (Split-Path $full) | Out-Null
  $script:ResultsFile = $full
}

# A case's or a check's JSON record, when the run writes them: UTF-8 without a byte-order mark,
# one line each.
function Write-Record([string]$name, [string]$status, [int]$secs, [string]$reason, [string]$form, [string]$provider, [string]$log) {
  if (-not $script:ResultsFile) { return }
  $result = switch ($status) { "ok" { "pass" } "FAIL" { "fail" } default { "skip" } }
  if ($status -eq "ok") { $reason = "" }
  $record = [ordered]@{ suite = "desktop"; case = $name; provider = $provider; form = $form; result = $result; reason = $reason; seconds = $secs; log = $log }
  $line = ConvertTo-Json -Compress -InputObject $record
  [IO.File]::AppendAllText($script:ResultsFile, "$line`n", (New-Object Text.UTF8Encoding $false))
}

# One line of the summary and its record; a check's line (-NotCounted) isn't counted again.
function Add-Result([string]$name, [string]$status, [int]$secs, [string]$detail, [string]$form = "", [string]$provider = "", [string]$log = "", [switch]$NotCounted) {
  $script:SummaryLines.Add(("{0,-20} {1,-5} {2,6}s  {3}" -f $name, $status, $secs, $detail))
  Write-Record $name $status $secs $detail $form $provider $log
  if ($NotCounted) { return }
  if ($status -eq "FAIL") { $script:Failed++ }
  if ($status -eq "skip") { $script:Skipped++ }
}

# A summary line and a record per check a spec printed (`ok|FAILED|skipped: <check> (<n>s) <text>`),
# named `<case>/<check>`; the `spec` line (a whole spec skipped) is the case's own.
function Add-CheckResult([string]$name, [string[]]$lines, [string]$form, [string]$provider, [string]$log) {
  foreach ($l in $lines) {
    if ($l -match '^(ok|FAILED|skipped): (\S+) \((\d+)s\) (.*)$' -and $Matches[2] -ne "spec") {
      $status = switch ($Matches[1]) { "ok" { "ok" } "FAILED" { "FAIL" } default { "skip" } }
      Add-Result "$name/$($Matches[2])" $status ([int]$Matches[3]) $Matches[4] $form $provider $log -NotCounted
    }
  }
}

# --- The run's own folder (lib/run-root.sh says what goes where) ------------------------------

# A fresh folder for the run under the system's temporary folder; its full path.
function Initialize-RunRoot {
  $dir = Join-Path ([IO.Path]::GetTempPath()) ("cyberctf-e2e-" + [IO.Path]::GetRandomFileName().Replace(".", ""))
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  return (Get-Item $dir).FullName
}

# The session where the run's app reads it, whatever the build: home\.cyberctf\dev-session.json.
function Copy-RunSession([string]$root, [string]$session) {
  $dir = Join-Path $root "home\.cyberctf"
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  Copy-Item $session (Join-Path $dir "dev-session.json") -Force
}

# Removes the run's folder, unless CYBERCTF_TEST_DESKTOP_KEEP is set; says where it is when it stays.
function Clear-RunRoot([string]$root) {
  if ($env:CYBERCTF_TEST_DESKTOP_KEEP) { return "kept: the run's folder $root (CYBERCTF_TEST_DESKTOP_KEEP)" }
  Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
  if (Test-Path $root) { return "couldn't remove all of the run's folder $root" }
}
