param(
    [Parameter(Mandatory = $true)][string]$Out,
    [Parameter(Mandatory = $true)][long]$Hwnd,
    [int]$MarginPx = 10
)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class GTShotBmp {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
[GTShotBmp]::SetProcessDPIAware() | Out-Null

$r = New-Object GTShotBmp+RECT
if (-not [GTShotBmp]::GetWindowRect([IntPtr]$Hwnd, [ref]$r)) { Write-Error "GetWindowRect failed"; exit 1 }
$x = [Math]::Max(0, $r.L - $MarginPx); $y = [Math]::Max(0, $r.T - $MarginPx)
$w = ($r.R + $MarginPx) - $x; $h = ($r.B + $MarginPx) - $y
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($x, $y, 0, 0, (New-Object System.Drawing.Size($w, $h)))
$g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Bmp)
$bmp.Dispose()
Write-Output "SHOT-BMP $Out ${w}x${h}+$x+$y"
