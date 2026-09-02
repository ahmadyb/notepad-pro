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

# Sample document: >1000 short morse-style lines like the user's file, so
# large-repeater behaviour is probed too.
$lines = @()
for ($n = 0; $n -lt 1100; $n++) {
    $lines += "PROBE-$n .- -... -.-. -- --- .-. ..-. . -.-"
}
Set-Content -Path "probe.txt" -Value ($lines -join "`n")

# Seed the settings file before first launch: dark theme like the user's
# environment, word wrap per tag.
$cfgDir = Join-Path $env:APPDATA "NotePadPro"
New-Item -ItemType Directory -Force -Path $cfgDir | Out-Null
if ($Tag -eq "wrap-off") {
    Set-Content -Path (Join-Path $cfgDir "settings.json") -Value '{"theme": "dark", "wordWrap": false}'
} else {
    Set-Content -Path (Join-Path $cfgDir "settings.json") -Value '{"theme": "dark", "wordWrap": true}'
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

# Coarse ASCII map of the editor region so the actual layout (column width,
# band positions) is readable from the check-run annotations.
$x0 = 0; $x1 = 720
$y0 = 140; $y1 = 620
$bg = $bmp.GetPixel([Math]::Min($bounds.Width - 40, 1240), 700)
$textish = 0
$cell = 4
$mapRows = @()
for ($y = $y0; $y -lt $y1; $y += $cell) {
    $line = ""
    for ($x = $x0; $x -lt $x1; $x += $cell) {
        $hit = 0
        for ($dy = 0; $dy -lt $cell; $dy += 2) {
            for ($dx = 0; $dx -lt $cell; $dx += 2) {
                $px = $bmp.GetPixel([Math]::Min($x + $dx, $bounds.Width - 1), [Math]::Min($y + $dy, $bounds.Height - 1))
                $d = [Math]::Abs([int]$px.R - [int]$bg.R) + [Math]::Abs([int]$px.G - [int]$bg.G) + [Math]::Abs([int]$px.B - [int]$bg.B)
                if ($d -gt 60) { $hit++ }
            }
        }
        if ($hit -ge 2) { $line += "#"; $textish++ } elseif ($hit -ge 1) { $line += "+" } else { $line += "." }
    }
    $mapRows += $line
}
$bmp.Dispose()

Write-Output "PROBE[$Tag] textcells=$textish bg=($($bg.R),$($bg.G),$($bg.B)) screen=$($bounds.Width)x$($bounds.Height)"
for ($r = 0; $r -lt $mapRows.Count; $r += 4) {
    $chunk = $mapRows[$r..([Math]::Min($r + 3, $mapRows.Count - 1))] -join "|"
    Write-Output "::warning::MAP[$Tag] r$r : $chunk"
}
if ($textish -lt 150) {
    Write-Output "::error::EDITOR-BLANK[$Tag] only $textish text cells - editor is not painting document text"
}

Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
