# [DEBUG-157] Does the restored Engine keep the AppContainer read grant?
$dir = Get-ChildItem "$HOME/.tizen-sim/engines" -Directory | Where-Object Name -match '^3\.0-windows' | Select-Object -First 1
$exe = Join-Path $dir.FullName 'chrome-win32/chrome.exe'
Write-Host "[DEBUG-157] before:"; icacls $exe | ForEach-Object { "[DEBUG-157]   $_" }
icacls $dir.FullName /grant '*S-1-15-2-1:(OI)(CI)(RX)' '*S-1-15-2-2:(OI)(CI)(RX)' /T /Q | Out-Null
Write-Host "[DEBUG-157] after:"; icacls $exe | ForEach-Object { "[DEBUG-157]   $_" }
$d = Join-Path $env:RUNNER_TEMP 'm47-acl'
$p = Start-Process $exe -ArgumentList @('--no-first-run', '--disable-gpu-compositing', '--window-position=-20000,-20000', "--user-data-dir=$d", 'about:blank') -PassThru
Start-Sleep 20
$kids = Get-CimInstance Win32_Process -Filter "ParentProcessId=$($p.Id)"
Write-Host "[DEBUG-157] sandboxed after grant: children=$(@($kids).Count)"
taskkill /F /T /PID $p.Id 2>$null | Out-Null
