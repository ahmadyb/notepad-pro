# CI pixel probe: launches the real app on the runner's desktop, takes a
# screenshot and counts "text" pixels in the editor region so a blank-editor
# regression is visible in the check-run annotations (artifact downloads are
# not reachable from this sandbox).
#
# Usage: pwsh -File ci/ui_probe.ps1 [tag]
param([string]$Tag = "wrap-on")

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

# Sample document: lines that must be visible in the editor.
$sample = "PROBE-ALPHA the quick brown fox jumps over the lazy dog 0123456789`nPROBE-BRAVO second line of the pixel probe`nPROBE-CHARLIE third line`n"
Set-Content -Path "probe.txt" -Value $sample

# For the wrap-off probe, seed the settings file before first launch.
if ($Tag -eq "wrap-off") {
    $cfgDir = Join-Path $env:APPDATA "NotePadPro"
    New-Item -ItemType Directory -Force -Path $cfgDir | Out-Null
    Set-Content -Path (Join-Path $cfgDir "settings.json") -Value '{"wordWrap": false}'
}

$exe = ".\target\release\notepadpro.exe"
$proc = Start-Process -FilePath $exe -ArgumentList "probe.txt" -PassThru
Start-Sleep -Seconds 8

$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
$graphics = [System.Drawing.Graphics]::FromImage($bmp)
$graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
$graphics.Dispose()
$bmp.Save("ui_probe_$Tag.png")

# Editor region: right of the sidebar / toolbar chrome. Sample a grid and
# count pixels that differ strongly from the editor background colour taken
# at a known-empty spot near the right edge.
$bg = $bmp.GetPixel([Math]::Min($bounds.Width - 40, 1240), 700)
$textish = 0
$maxrun = 0
for ($y = 200; $y -lt [Math]::Min($bounds.Height - 60, 950); $y += 2) {
    $run = 0
    for ($x = 430; $x -lt [Math]::Min($bounds.Width - 20, 1260); $x += 2) {
        $px = $bmp.GetPixel($x, $y)
        $d = [Math]::Abs([int]$px.R - [int]$bg.R) + [Math]::Abs([int]$px.G - [int]$bg.G) + [Math]::Abs([int]$px.B - [int]$bg.B)
        if ($d -gt 90) { $textish++; $run++ ; if ($run -gt $maxrun) { $maxrun = $run } } else { $run = 0 }
    }
}
$bmp.Dispose()

Write-Output "PROBE[$Tag] textish=$textish maxrun=$maxrun bg=($($bg.R),$($bg.G),$($bg.B)) screen=$($bounds.Width)x$($bounds.Height)"
Write-Output "::warning::PROBE[$Tag] textish=$textish maxrun=$maxrun"
if ($textish -lt 150) {
    Write-Output "::error::EDITOR-BLANK[$Tag] only $textish text pixels (maxrun $maxrun) - editor is not painting document text"
}

Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
