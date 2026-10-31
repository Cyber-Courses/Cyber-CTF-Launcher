# Launch the (wdio-feature) app on Windows so its embedded WebDriver server listens, then run the
# smoke. Must run in an interactive desktop session; the app must be built with `--features wdio`.
$ErrorActionPreference = "Continue"
$here = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$bin = if ($env:APP_BIN) { $env:APP_BIN } else { "$here\src-tauri\target\debug\cyberctf-desktop.exe" }
if (!(Test-Path $bin)) { "app binary not found: $bin"; exit 2 }
if (-not $env:TAURI_WEBDRIVER_PORT) { $env:TAURI_WEBDRIVER_PORT = "4445" }

$app = Start-Process -PassThru -FilePath $bin -RedirectStandardOutput "$env:TEMP\cyberctf-app.out" -RedirectStandardError "$env:TEMP\cyberctf-app.err"
$rc = 1
try {
  $up = $false
  for ($i = 0; $i -lt 40; $i++) {
    try { Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$($env:TAURI_WEBDRIVER_PORT)/status" -TimeoutSec 2 | Out-Null; $up = $true; break } catch { Start-Sleep -Seconds 1 }
  }
  if (-not $up) {
    "embedded WebDriver server never came up on $($env:TAURI_WEBDRIVER_PORT)"
    Get-Content "$env:TEMP\cyberctf-app.err" -Tail 15 -ErrorAction SilentlyContinue
    $rc = 3
  } else {
    node "$here\e2e-native\smoke-embedded.mjs"
    $rc = $LASTEXITCODE
  }
} finally {
  Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
}
exit $rc
