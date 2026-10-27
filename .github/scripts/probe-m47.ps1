# [DEBUG-157] Launches M47 directly in variants and counts its child processes.
$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force hang-out | Out-Null
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class J {
  [DllImport("kernel32.dll")] public static extern bool IsProcessInJob(IntPtr p, IntPtr j, out bool r);
  [DllImport("kernel32.dll")] public static extern IntPtr GetCurrentProcess();
  [DllImport("kernel32.dll")] public static extern bool QueryInformationJobObject(IntPtr j, int c, byte[] b, int l, IntPtr r);
}
'@
$in = $false; [J]::IsProcessInJob([J]::GetCurrentProcess(), [IntPtr]::Zero, [ref]$in) | Out-Null
$buf = New-Object byte[] 144
$ok = [J]::QueryInformationJobObject([IntPtr]::Zero, 9, $buf, 144, [IntPtr]::Zero)
$flags = [BitConverter]::ToUInt32($buf, 16)
Write-Host ("[DEBUG-157] in job: {0}; query ok: {1}; LimitFlags: 0x{2:X}" -f $in, $ok, $flags)
Write-Host "[DEBUG-157] session: $((Get-Process -Id $PID).SessionId); test mode: $(bcdedit /enum '{current}' 2>$null | Select-String testsigning)"
$exe = Get-ChildItem "$HOME/.tizen-sim/engines" -Recurse -Filter chrome.exe | Where-Object FullName -match '3.0-windows' | Select-Object -First 1
Write-Host "[DEBUG-157] exe: $($exe.FullName)"
$base = @('--no-first-run', '--no-default-browser-check', '--disable-gpu-compositing', '--window-position=-20000,-20000', '--remote-debugging-port=0')
$variants = [ordered]@{ plain = @(); nosandbox = @('--no-sandbox'); nogpu = @('--disable-gpu'); both = @('--no-sandbox', '--disable-gpu') }
foreach ($name in $variants.Keys) {
  $dir = Join-Path $env:RUNNER_TEMP "m47-$name"
  $args = $base + $variants[$name] + @("--user-data-dir=$dir", '--enable-logging', '--v=0', 'about:blank')
  $p = Start-Process $exe.FullName -ArgumentList $args -PassThru
  Start-Sleep 20
  $kids = Get-CimInstance Win32_Process -Filter "ParentProcessId=$($p.Id)"
  $types = ($kids | ForEach-Object { if ($_.CommandLine -match '--type=(\S+)') { $Matches[1] } else { '?' } }) -join ','
  Write-Host "[DEBUG-157] $name : alive=$(-not $p.HasExited) children=$(@($kids).Count) [$types]"
  if (Test-Path "$dir/chrome_debug.log") { Copy-Item "$dir/chrome_debug.log" "hang-out/debug-$name.log" }
  taskkill /F /T /PID $p.Id 2>$null | Out-Null
}
