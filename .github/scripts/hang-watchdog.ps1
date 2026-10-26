# [DEBUG-157] Runs the M47 tests; if they outlive the budget, dumps what is running.
$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force hang-out | Out-Null
$cmd = if ($env:SCOPE -eq 'avplay') { 'test -p tizen-sim-core --test avplay' } else { 'test -p tizen-sim-core' }
$p = Start-Process cargo -ArgumentList $cmd -NoNewWindow -PassThru `
  -RedirectStandardOutput hang-out/stdout.txt -RedirectStandardError hang-out/stderr.txt
$budget = if ($env:SCOPE -eq 'avplay') { 240 } else { 1500 }
if ($p.WaitForExit($budget * 1000)) {
  Get-Content hang-out/stdout.txt -Tail 40
  Write-Host "[DEBUG-157] finished, exit $($p.ExitCode)"
  exit $p.ExitCode
}
Write-Host "[DEBUG-157] HUNG after $budget s"
Get-Content hang-out/stdout.txt -Tail 40
Get-CimInstance Win32_Process |
  Select-Object ProcessId, ParentProcessId, Name, WorkingSetSize, CommandLine |
  Sort-Object ParentProcessId | Format-Table -AutoSize -Wrap | Out-String -Width 4000 |
  Tee-Object hang-out/processes.txt
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
[System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
$bmp.Save("$PWD/hang-out/desktop.png")
Get-WinEvent -LogName Application -MaxEvents 40 -ErrorAction SilentlyContinue |
  Format-List TimeCreated, ProviderName, Id, Message | Out-File hang-out/app-events.txt
taskkill /F /T /PID $p.Id | Out-Null
exit 1
