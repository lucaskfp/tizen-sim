# [DEBUG-157] Where is the sandboxed M47 browser process stuck?
New-Item -ItemType Directory -Force hang-out | Out-Null
$cdb = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\Debuggers\x86\cdb.exe' -ErrorAction SilentlyContinue
Write-Host "[DEBUG-157] cdb: $($cdb.FullName)"
$dir = Get-ChildItem "$HOME/.tizen-sim/engines" -Directory | Where-Object Name -match '^3\.0-windows' | Select-Object -First 1
$exe = Join-Path $dir.FullName 'chrome-win32/chrome.exe'
$d = Join-Path $env:RUNNER_TEMP 'm47-stack'
$p = Start-Process $exe -ArgumentList @('--no-first-run', '--disable-gpu-compositing', '--window-position=-20000,-20000', "--user-data-dir=$d", '--enable-logging', '--v=1', 'about:blank') -PassThru
Start-Sleep 20
Write-Host "[DEBUG-157] children=$(@(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($p.Id)").Count) alive=$(-not $p.HasExited)"
if ($cdb) {
  & $cdb.FullName -p $p.Id -y 'srv*C:\symbols*https://msdl.microsoft.com/download/symbols' -c '.reload /f ntdll.dll kernel32.dll kernelbase.dll user32.dll; ~*kv 30; q' 2>&1 |
    Tee-Object hang-out/stacks.txt | Select-String -Pattern '^\s*\.?\s*\d+\s+Id:|ntdll!|KERNELBASE!|USER32!|kernel32!' |
    Select-Object -First 120 | ForEach-Object { "[DEBUG-157] $_" }
}
if (Test-Path "$d/chrome_debug.log") { Copy-Item "$d/chrome_debug.log" hang-out/ ; Get-Content "$d/chrome_debug.log" -Tail 30 | ForEach-Object { "[DEBUG-157] log: $_" } }
taskkill /F /T /PID $p.Id 2>$null | Out-Null
