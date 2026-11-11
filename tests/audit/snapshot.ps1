<#
.SYNOPSIS
The leftover audit's snapshot on Windows (snapshot.sh: Linux and macOS): what this machine holds of
every kind of resource the launcher's stack can make (tests/leftovers.md), as one JSON document.

.DESCRIPTION
Read-only and quick: each tool is asked once, with a time limit, and a tool that's missing, refuses
or doesn't answer is recorded as such (the diff makes it a skip), never an error. The document is
the one snapshot.sh writes: {"audit": 1, "os": "windows", "host", "user", "taken", "roots",
"probes": [{"kind", "tool", "status", "note", "output"}]}. A kind with a common tool (Docker,
Vagrant, VirtualBox, the hosts file) records that tool's output as snapshot.sh does; Windows' own
kinds record one JSON object a line, which tests/audit/inventory.ts reads.

.PARAMETER Out
Where the snapshot goes (default: standard output).

.PARAMETER Root
The run's own folders (CYBERCTF_WDIO_ROOT, a runner's work folder): the .vagrant folders and pid
and lock files under them are recorded too.
#>
[CmdletBinding()]
param(
  [string]$Out = '-',
  [string[]]$Root = @()
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Limit = if ($env:CYBERCTF_AUDIT_TIMEOUT) { [int]$env:CYBERCTF_AUDIT_TIMEOUT } else { 20 }
$Probes = [System.Collections.Generic.List[object]]::new()

# Add-Probe: a probe's record.
function Add-Probe([string]$Kind, [string]$Tool, [string]$Status, [string]$Note, [string]$Output) {
  $Probes.Add([ordered]@{ kind = $Kind; tool = $Tool; status = $Status; note = $Note; output = $Output })
}

# Invoke-Probe: an external tool's output as the kind's probe, within the time limit; missing when
# the tool isn't on PATH, timeout or failed otherwise.
function Invoke-Probe([string]$Kind, [string]$Tool, [string[]]$Arguments) {
  $exe = Get-Command $Tool -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $exe) { Add-Probe $Kind $Tool 'missing' "no $Tool on PATH" ''; return }
  $outFile = [System.IO.Path]::GetTempFileName()
  $errFile = [System.IO.Path]::GetTempFileName()
  try {
    $p = Start-Process -FilePath $exe.Source -ArgumentList $Arguments -NoNewWindow -PassThru `
      -RedirectStandardOutput $outFile -RedirectStandardError $errFile
    if (-not $p.WaitForExit($Limit * 1000)) {
      Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
      Add-Probe $Kind $Tool 'timeout' "no answer in ${Limit}s" ''
      return
    }
    $text = Get-Content -Raw -LiteralPath $outFile
    if ($null -eq $text) { $text = '' }
    if ($p.ExitCode -eq 0) { Add-Probe $Kind $Tool 'ok' '' $text }
    else {
      $err = (Get-Content -LiteralPath $errFile | Where-Object { $_.Trim() } | Select-Object -Last 1)
      Add-Probe $Kind $Tool 'failed' "exit $($p.ExitCode): $err" $text
    }
  } finally {
    Remove-Item -LiteralPath $outFile, $errFile -ErrorAction SilentlyContinue
  }
}

# Invoke-JsonProbe: a PowerShell query's objects as the kind's probe, one compressed JSON object a
# line; missing when its module isn't there.
function Invoke-JsonProbe([string]$Kind, [string]$Tool, [string]$Command, [scriptblock]$Query) {
  if (-not (Get-Command $Command -ErrorAction SilentlyContinue)) { Add-Probe $Kind $Tool 'missing' "no $Command ($Tool)" ''; return }
  try {
    $lines = @(& $Query | ForEach-Object { $_ | ConvertTo-Json -Compress -Depth 3 })
    Add-Probe $Kind $Tool 'ok' '' ($lines -join "`n")
  } catch [System.UnauthorizedAccessException] {
    Add-Probe $Kind $Tool 'denied' "needs an administrator: $($_.Exception.Message)" ''
  } catch {
    Add-Probe $Kind $Tool 'failed' $_.Exception.Message ''
  }
}

# --- Docker: one JSON object a line, as snapshot.sh records them ----------------------------------

function Get-DockerProbe {
  $docker = Get-Command docker -ErrorAction SilentlyContinue
  $up = $false
  if ($docker) { & docker version --format '{{.Server.Version}}' *> $null; $up = ($LASTEXITCODE -eq 0) }
  if (-not $up) {
    $why = if ($docker) { "Docker isn't running (docker version has no server)" } else { 'no docker on PATH' }
    foreach ($k in 'docker-container', 'docker-network', 'docker-volume', 'docker-image') { Add-Probe $k 'docker' 'missing' $why '' }
    return
  }
  $ids = @(& docker ps -aq --no-trunc)
  if ($ids.Count -gt 0) {
    Invoke-Probe 'docker-container' 'docker' (@('inspect', '--format', '"{\"id\":{{json .Id}},\"name\":{{json .Name}},\"created\":{{json .Created}},\"labels\":{{json .Config.Labels}},\"state\":{{json .State.Status}}}"') + $ids)
  } else { Add-Probe 'docker-container' 'docker' 'ok' '' '' }
  $nets = @(& docker network ls -q --no-trunc)
  Invoke-Probe 'docker-network' 'docker' (@('network', 'inspect', '--format', '"{\"id\":{{json .Id}},\"name\":{{json .Name}},\"created\":{{json .Created}},\"labels\":{{json .Labels}},\"driver\":{{json .Driver}}}"') + $nets)
  $vols = @(& docker volume ls -q)
  if ($vols.Count -gt 0) {
    Invoke-Probe 'docker-volume' 'docker' (@('volume', 'inspect', '--format', '"{\"name\":{{json .Name}},\"created\":{{json .CreatedAt}},\"labels\":{{json .Labels}},\"mountpoint\":{{json .Mountpoint}}}"') + $vols)
  } else { Add-Probe 'docker-volume' 'docker' 'ok' '' '' }
  Invoke-Probe 'docker-image' 'docker' @('image', 'ls', '--no-trunc', '--format', '"{{json .}}"')
}

# --- Vagrant: its machine index and boxes, read from VAGRANT_HOME ---------------------------------

function Get-VagrantProbe {
  $vhome = if ($env:VAGRANT_HOME) { $env:VAGRANT_HOME } else { Join-Path $env:USERPROFILE '.vagrant.d' }
  if (-not (Test-Path -LiteralPath $vhome)) {
    Add-Probe 'vagrant-machine' 'vagrant' 'missing' "no Vagrant home ($vhome)" ''
    Add-Probe 'vagrant-box' 'vagrant' 'missing' "no Vagrant home ($vhome)" ''
    return
  }
  $index = Join-Path $vhome 'data\machine-index\index'
  $text = if (Test-Path -LiteralPath $index) { Get-Content -Raw -LiteralPath $index } else { '' }
  Add-Probe 'vagrant-machine' 'vagrant' 'ok' '' $text
  $boxes = Join-Path $vhome 'boxes'
  $found = @()
  if (Test-Path -LiteralPath $boxes) {
    $found = Get-ChildItem -LiteralPath $boxes -Recurse -Depth 4 -Filter metadata.json -File |
      ForEach-Object { $_.DirectoryName.Substring($boxes.Length + 1) -replace '\\', '/' }
  }
  Add-Probe 'vagrant-box' 'vagrant' 'ok' '' ($found -join "`n")
}

# --- The launcher's folders, temp folders, pid and lock files -------------------------------------

function Get-FileProbe {
  $dirs = @(
    (Join-Path $env:APPDATA 'org.cyberctf.desktop'),
    (Join-Path $env:LOCALAPPDATA 'org.cyberctf.desktop'),
    (Join-Path $env:USERPROFILE '.cyberctf'),
    (Join-Path $env:USERPROFILE '.isothrum')
  ) + $Root | Where-Object { $_ -and (Test-Path -LiteralPath $_) }
  $temp = @($env:TEMP, $env:TMP, "$env:SystemRoot\Temp") | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -Unique
  $tempItems = foreach ($t in $temp) {
    Get-ChildItem -LiteralPath $t -Force -ErrorAction SilentlyContinue |
      Where-Object { $_.Name -match '^(\.?cyberctf|org_cyberctf|isothrum|isoloom)' } | ForEach-Object FullName
  }
  Add-Probe 'temp-folder' 'Get-ChildItem' 'ok' '' (@($tempItems | Sort-Object -Unique) -join "`n")
  $dot = foreach ($d in $dirs) { Get-ChildItem -LiteralPath $d -Recurse -Depth 8 -Directory -Force -Filter .vagrant -ErrorAction SilentlyContinue | ForEach-Object FullName }
  Add-Probe 'vagrant-dotfolder' 'Get-ChildItem' 'ok' '' (@($dot) -join "`n")
  $runFiles = foreach ($d in $dirs) {
    Get-ChildItem -LiteralPath $d -Recurse -Depth 8 -File -Force -ErrorAction SilentlyContinue -Include *.pid, *.lock, *.sock, lock |
      Where-Object { $_.Name -notmatch '^(yarn|Pipfile|poetry|composer|Gemfile|Cargo|flake|package-lock|pnpm-lock)\.lock$' } | ForEach-Object FullName
  }
  Add-Probe 'run-file' 'Get-ChildItem' 'ok' '' (@($runFiles) -join "`n")
}

# --- Processes, listening ports, the network ------------------------------------------------------

# The stack's processes (tests/leftovers.md, "Processes"): only those are recorded.
$StackProcesses = 'cyber-ctf|cyberctf|org\.cyberctf|deploy --job|mstsc|isothrum|isoloom|vagrant|qemu-system|VBoxHeadless|VBoxNetDHCP|VBoxNetNAT|vmware-vmx|vmwp|terraform|websockify|socat|ssh(\.exe)? .*-[LRDNw]'

function Get-NetworkProbe {
  Invoke-JsonProbe 'process' 'CIM' 'Get-CimInstance' {
    Get-CimInstance Win32_Process | Where-Object { "$($_.Name) $($_.CommandLine)" -match $StackProcesses -and $_.CommandLine -notmatch 'tests[\\/]audit[\\/]snapshot' } |
      ForEach-Object { [ordered]@{ ProcessId = $_.ProcessId; Name = $_.Name; CommandLine = $_.CommandLine; CreationDate = $_.CreationDate.ToUniversalTime().ToString('o') } }
  }
  Invoke-JsonProbe 'listening-port' 'NetTCPIP' 'Get-NetTCPConnection' {
    $names = @{}
    Get-Process | ForEach-Object { $names[$_.Id] = $_.ProcessName }
    Get-NetTCPConnection -State Listen | ForEach-Object {
      [ordered]@{ LocalAddress = $_.LocalAddress; LocalPort = $_.LocalPort; OwningProcess = $_.OwningProcess; Name = $names[[int]$_.OwningProcess] }
    }
  }
  Invoke-JsonProbe 'route' 'NetTCPIP' 'Get-NetRoute' {
    Get-NetRoute -AddressFamily IPv4 | ForEach-Object { [ordered]@{ DestinationPrefix = $_.DestinationPrefix; NextHop = $_.NextHop; InterfaceAlias = $_.InterfaceAlias } }
  }
  $hosts = Join-Path $env:SystemRoot 'System32\drivers\etc\hosts'
  Add-Probe 'hosts-entry' 'hosts file' 'ok' '' (Get-Content -Raw -LiteralPath $hosts)
  Invoke-JsonProbe 'firewall-rule' 'NetSecurity' 'Get-NetFirewallRule' {
    Get-NetFirewallRule | ForEach-Object { [ordered]@{ Name = $_.Name; DisplayName = $_.DisplayName; Group = $_.Group } }
  }
  Invoke-Probe 'portproxy' 'netsh' @('interface', 'portproxy', 'show', 'v4tov4')
  # What the stack registers with Windows: the cyberctf:// handler, a RunOnce entry.
  Invoke-JsonProbe 'os-registration' 'registry' 'Get-ItemProperty' {
    if (Test-Path 'HKCU:\Software\Classes\cyberctf') { [ordered]@{ Name = 'HKCU\Software\Classes\cyberctf' } }
    $once = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce' -ErrorAction SilentlyContinue
    if ($once -and ($once.PSObject.Properties.Name -contains 'CyberCTF')) { [ordered]@{ Name = 'HKCU\...\RunOnce\CyberCTF' } }
  }
  Invoke-JsonProbe 'hyperv-nat' 'NetNat' 'Get-NetNat' {
    Get-NetNat | ForEach-Object { [ordered]@{ Name = $_.Name; Prefix = $_.InternalIPInterfaceAddressPrefix } }
  }
  # Windows' Credential Manager: the launcher's targets' names, never their secrets, nor others'.
  if (Get-Command cmdkey -ErrorAction SilentlyContinue) {
    $targets = @(& cmdkey /list | Where-Object { $_ -match '^\s*Target:' -and $_ -match 'cyberctf|cyber-ctf|org\.cyberctf' })
    Add-Probe 'keychain-item' 'cmdkey' 'ok' '' ($targets -join "`n")
  } else { Add-Probe 'keychain-item' 'cmdkey' 'missing' 'no cmdkey on PATH' '' }
}

# --- Hypervisors -------------------------------------------------------------------------------

function Get-HypervisorProbe {
  Invoke-JsonProbe 'hyperv-vm' 'Hyper-V' 'Get-VM' {
    Get-VM | ForEach-Object { [ordered]@{ Id = "$($_.Id)"; Name = $_.Name; State = "$($_.State)"; CreationTime = $_.CreationTime.ToUniversalTime().ToString('o'); Path = $_.Path } }
  }
  Invoke-JsonProbe 'hyperv-switch' 'Hyper-V' 'Get-VMSwitch' {
    Get-VMSwitch | ForEach-Object { [ordered]@{ Id = "$($_.Id)"; Name = $_.Name; SwitchType = "$($_.SwitchType)" } }
  }
  Invoke-JsonProbe 'hyperv-vhd' 'Hyper-V' 'Get-VMHost' {
    $folder = (Get-VMHost).VirtualHardDiskPath
    $disks = @(Get-VM | Get-VMHardDiskDrive | ForEach-Object Path)
    if ($folder -and (Test-Path -LiteralPath $folder)) { $disks += Get-ChildItem -LiteralPath $folder -File -Include *.vhd, *.vhdx, *.avhdx -Recurse | ForEach-Object FullName }
    $disks | Sort-Object -Unique | ForEach-Object {
      $file = Get-Item -LiteralPath $_ -ErrorAction SilentlyContinue
      [ordered]@{ Path = $_; CreationTime = if ($file) { $file.CreationTimeUtc.ToString('o') } else { '' } }
    }
  }
  if (-not (Get-Command VBoxManage -ErrorAction SilentlyContinue) -and $env:VBOX_MSI_INSTALL_PATH) { $env:PATH += ";$env:VBOX_MSI_INSTALL_PATH" }
  Invoke-Probe 'vbox-vm' 'VBoxManage' @('list', 'vms')
  Invoke-Probe 'vbox-hostonly' 'VBoxManage' @('list', 'hostonlyifs')
  Invoke-Probe 'vbox-natnet' 'VBoxManage' @('list', 'natnets')
  Invoke-Probe 'vbox-dhcp' 'VBoxManage' @('list', 'dhcpservers')
}

Get-DockerProbe
Get-VagrantProbe
Get-FileProbe
Get-NetworkProbe
Get-HypervisorProbe

$doc = [ordered]@{
  audit = 1
  os = 'windows'
  host = $env:COMPUTERNAME
  user = $env:USERNAME
  taken = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
  roots = @($Root)
  probes = $Probes
}
$json = $doc | ConvertTo-Json -Depth 4
if ($Out -eq '-') { $json } else { [System.IO.File]::WriteAllText($Out, $json, [System.Text.UTF8Encoding]::new($false)) }
