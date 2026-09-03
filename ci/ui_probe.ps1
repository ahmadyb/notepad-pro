# CI pixel probe: launches the real app on the runner's desktop, takes a
# screenshot and counts "text" pixels in the editor region so a blank-editor
# regression is visible in the check-run annotations (artifact downloads are
# not reachable from this sandbox).
#
# Modes:
#   wrap-on / wrap-off : static render probe (ASCII map of the editor region)
#   interact           : click + SendKeys probe. Verifies that
#                          (a) Enter in the middle of a word does not hang the
#                              app ($proc.Responding), and
#                          (b) the cursor-line wash lands on the text row that
#                              was clicked (overlay geometry drift detector).
#
# Usage: pwsh -File ci/ui_probe.ps1 [tag]
param([string]$Tag = "wrap-on")

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class ProbeMouse {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int X, int Y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, uint dx, uint dy, uint dwData, UIntPtr dwExtraInfo);
}
"@

function Click($x, $y) {
    [ProbeMouse]::SetCursorPos($x, $y) | Out-Null
    Start-Sleep -Milliseconds 150
    [ProbeMouse]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [ProbeMouse]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 400
}

function Screenshot {
    $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
    $g.Dispose()
    return $bmp
}

# Text-row tops: rows whose 4px cell scanline contains dark-text glyphs.
function TextRowTops($bmp, $bg, $x0, $x1, $y0, $y1) {
    $tops = @()
    $inText = $false
    for ($y = $y0; $y -lt $y1; $y += 2) {
        $hit = 0
        for ($x = $x0; $x -lt $x1; $x += 6) {
            $px = $bmp.GetPixel($x, $y)
            $d = [Math]::Abs([int]$px.R - [int]$bg.R) + [Math]::Abs([int]$px.G - [int]$bg.G) + [Math]::Abs([int]$px.B - [int]$bg.B)
            if ($d -gt 90) { $hit++; if ($hit -ge 3) { break } }
        }
        $row = $hit -ge 3
        if ($row -and -not $inText) { $tops += $y }
        $inText = $row
    }
    return $tops
}

# Cursor-wash row tops: the wash is a subtle full-width lightening (diff in a
# mid range), sampled right of any text column.
function WashTops($bmp, $bg, $xw, $y0, $y1) {
    $tops = @()
    $inWash = $false
    for ($y = $y0; $y -lt $y1; $y += 2) {
        $acc = 0
        for ($k = 0; $k -lt 6; $k++) {
            $px = $bmp.GetPixel($xw + $k * 3, $y)
            $acc += [Math]::Abs([int]$px.R - [int]$bg.R) + [Math]::Abs([int]$px.G - [int]$bg.G) + [Math]::Abs([int]$px.B - [int]$bg.B)
        }
        $avg = $acc / 6
        $row = ($avg -gt 12) -and ($avg -lt 140)
        if ($row -and -not $inWash) { $tops += $y }
        $inWash = $row
    }
    return $tops
}

$cfgDir = Join-Path $env:APPDATA "NotePadPro"
New-Item -ItemType Directory -Force -Path $cfgDir | Out-Null
Set-Content -Path (Join-Path $cfgDir "settings.json") -Value '{"theme": "dark", "wordWrap": true, "animations": false}'

if ($Tag -eq "interact") {
    # Long wrapping lines + short lines: forces multi-visual-line rows so any
    # drift between Rust geometry and the renderer accumulates fast.
    $long = ("morse " * 24).Trim()          # ~144 chars, wraps to 2+ visual lines
    $mid  = ("word " * 14).Trim()           # ~70 chars
    $doc = @()
    for ($n = 0; $n -lt 60; $n++) {
        $doc += $long
        $doc += "PROBE-$n short"
        $doc += $mid
    }
    Set-Content -Path "probe_interact.txt" -Value ($doc -join "`n")
    $arg = "probe_interact.txt"
} else {
    # Sample document: >1000 short morse-style lines like the user's file, so
    # large-repeater behaviour is probed too.
    $lines = @()
    for ($n = 0; $n -lt 1100; $n++) {
        $lines += "PROBE-$n .- -... -.-. -- --- .-. ..-. . -.-"
    }
    Set-Content -Path "probe.txt" -Value ($lines -join "`n")
    if ($Tag -eq "wrap-off") {
        Set-Content -Path (Join-Path $cfgDir "settings.json") -Value '{"theme": "dark", "wordWrap": false}'
    }
    $arg = "probe.txt"
}

$exe = ".\target\release\notepadpro.exe"
if ($Tag -eq "interact") { $env:NP_DEBUG_GEOM = "1" }
$proc = Start-Process -FilePath $exe -ArgumentList $arg -PassThru
Start-Sleep -Seconds 8

$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = Screenshot
if ($Tag -ne "interact") { $bmp.Save("ui_probe_$Tag.png") }

$bg = $bmp.GetPixel([Math]::Min($bounds.Width - 40, 1240), 700)

if ($Tag -eq "interact") {
    # ── Rust's own geometry table ─────────────────────────────────────────
    if (Test-Path "geodump.json") {
        $gd = Get-Content "geodump.json" -Raw | ConvertFrom-Json
        Write-Output "::warning::GEODUMP pitch=$($gd.pitch) char_w=$($gd.char_w) view_w=$($gd.view_w) zoom=$($gd.zoom) wrap=$($gd.wrap)"
        Write-Output "::warning::GEODUMP y=$(@($gd.y) -join ',')"
        Write-Output "::warning::GEODUMP h=$(@($gd.h) -join ',')"
    } else {
        Write-Output "::error::GEODUMP-MISSING geodump.json was not written"
    }

    # ── locate text rows in the editor region (merge split detections) ────
    $x0 = 40; $x1 = 700
    $y0 = 150; $y1 = [Math]::Min($bounds.Height - 60, 900)
    $raw = TextRowTops $bmp $bg $x0 $x1 $y0 $y1
    $tops = @()
    foreach ($t in $raw) {
        if ($tops.Count -eq 0 -or ($t - $tops[$tops.Count - 1]) -gt 10) { $tops += $t }
    }
    Write-Output "::warning::INTERACT text-row tops: $($tops[0..11] -join ',')"
    if ($tops.Count -lt 4) {
        Write-Output "::error::INTERACT-NO-TEXT only $($tops.Count) text rows found"
        $bmp.Dispose(); Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue; exit 1
    }

    # ── (b) drift detector: click row 3, wash must land on row 3 ──────────
    $clickRow = 3
    $rowY = $tops[$clickRow] + 8
    Click 300 $rowY
    Start-Sleep -Milliseconds 600
    $bmp2 = Screenshot
    $xw = [Math]::Min($bounds.Width - 140, 1150)
    $wash = WashTops $bmp2 $bg $xw $y0 $y1
    Write-Output "::warning::INTERACT wash tops after click row $clickRow (y=$rowY): $($wash[0..5] -join ',')"
    $drift = 999
    foreach ($wt in $wash) { $d = [Math]::Abs($wt - $tops[$clickRow]); if ($d -lt $drift) { $drift = $d } }
    Write-Output "PROBE[interact] wash-drift-px=$drift"
    if ($wash.Count -eq 0) {
        Write-Output "::error::WASH-MISSING no cursor wash row detected after click"
    } elseif ($drift -gt 6) {
        Write-Output "::error::WASH-DRIFT cursor wash is $drift px away from the clicked text row (overlay geometry disagrees with the renderer)"
    }

    # ── (a) Enter hang detector: click mid-word, press Enter, stay alive ──
    Click 320 ($tops[0] + 8)
    [System.Windows.Forms.SendKeys]::SendWait("abc")
    Start-Sleep -Milliseconds 800
    $proc.Refresh()
    $alive1 = $proc.Responding
    [System.Windows.Forms.SendKeys]::SendWait("{ENTER}")
    Start-Sleep -Seconds 3
    $proc.Refresh()
    $alive2 = $proc.Responding
    [System.Windows.Forms.SendKeys]::SendWait("xyz")
    Start-Sleep -Seconds 2
    $proc.Refresh()
    $alive3 = $proc.Responding
    Write-Output "PROBE[interact] responding after-type=$alive1 after-enter=$alive2 after-more=$alive3"
    if (-not $alive2 -or -not $alive3) {
        Write-Output "::error::ENTER-HANG app stopped responding after Enter in the middle of a word (after-enter=$alive2 after-more=$alive3)"
    }
    if (-not $alive1) {
        Write-Output "::error::TYPE-HANG app stopped responding after plain typing"
    }
    $bmpFinal = Screenshot
    $bmpFinal.Save("ui_probe_interact.png")
    $bmpFinal.Dispose()
    $bmp2.Dispose()
    $bmp.Dispose()
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    exit 0
}

# ── static modes: coarse ASCII map of the editor region ───────────────────
$x0 = 0; $x1 = 720
$y0 = 140; $y1 = 620
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
# Diagnostic: exact colours across the left edge to identify the gutter strip.
# Emitted first so they survive the annotations cap.
foreach ($px in @(4, 16, 30, 44, 60, 90, 130)) {
    $c = $bmp.GetPixel($px, 400)
    Write-Output "::warning::RGB[$Tag] x=$px : ($($c.R),$($c.G),$($c.B))"
}
$bmp.Dispose()

Write-Output "PROBE[$Tag] textcells=$textish bg=($($bg.R),$($bg.G),$($bg.B)) screen=$($bounds.Width)x$($bounds.Height)"
for ($r = 0; $r -lt $mapRows.Count; $r += 4) {
    $chunk = ($mapRows[$r..([Math]::Min($r + 3, $mapRows.Count - 1))] -join "|")
    Write-Output "::warning::MAP[$Tag] r$r : $chunk"
}
if ($textish -lt 150) {
    Write-Output "::error::EDITOR-BLANK[$Tag] only $textish text cells - editor is not painting document text"
}

Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
