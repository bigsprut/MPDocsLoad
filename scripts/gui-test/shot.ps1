# Screenshot for GUI testing. ASCII-only (PS 5.1 reads .ps1 as ANSI without BOM).
# Usage: shot.ps1 -Out <png> [-Hwnd <hwnd>] [-MarginPx <n>]
#   default: full virtual screen; with -Hwnd: that window rect +- margin.
# DPI-aware (SetProcessDPIAware) => pixel coords == physical screen coords,
# consistent with click.ps1 and OCR bounding rects.
param(
    [Parameter(Mandatory = $true)][string]$Out,
    [long]$Hwnd = 0,
    [int]$MarginPx = 0
)
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class GTShot {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
[GTShot]::SetProcessDPIAware() | Out-Null

$x = 0; $y = 0; $w = 0; $h = 0
if ($Hwnd -gt 0) {
    $r = New-Object GTShot+RECT
    if (-not [GTShot]::GetWindowRect([IntPtr]$Hwnd, [ref]$r)) { Write-Error "GetWindowRect failed"; exit 1 }
    $x = [Math]::Max(0, $r.L - $MarginPx); $y = [Math]::Max(0, $r.T - $MarginPx)
    $w = ($r.R + $MarginPx) - $x; $h = ($r.B + $MarginPx) - $y
} else {
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $x = $b.X; $y = $b.Y; $w = $b.Width; $h = $b.Height
}
if ($w -le 0 -or $h -le 0) { Write-Error "empty rect"; exit 1 }
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($x, $y, 0, 0, (New-Object System.Drawing.Size($w, $h)))
$g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output ("OK {0} {1}x{2}+{3}+{4}" -f $Out, $w, $h, $x, $y)
