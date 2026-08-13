# Click / type for GUI testing. ASCII-only.
# Usage:
#   click.ps1 -X <px> -Y <px> [-Hwnd <hwnd>]      single left click at PHYSICAL px
#   click.ps1 -X <px> -Y <px> -Double [-Hwnd ..]  double click
#   click.ps1 -Text "abc"                          type text into focused widget
#   click.ps1 -Key <VK hex>                        press key (e.g. 0x09 TAB, 0x0D ENTER)
# DPI-aware: coords match shot.ps1 pixels and OCR bounding rects.
param(
    [int]$X = -1,
    [int]$Y = -1,
    [long]$Hwnd = 0,
    [switch]$Double,
    [string]$Text = "",
    [string]$Key = ""
)
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class GTInput {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] inp, int size);
    [StructLayout(LayoutKind.Sequential)]
    public struct INPUT { public uint type; public InputUnion u; }
    [StructLayout(LayoutKind.Explicit)]
    public struct InputUnion {
        [FieldOffset(0)] public MOUSEINPUT mi;
        [FieldOffset(0)] public KEYBDINPUT ki;
    }
    [StructLayout(LayoutKind.Sequential)]
    public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public UIntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)]
    public struct KEYBDINPUT { public ushort wVk, wScan; public uint dwFlags, time; public UIntPtr dwExtraInfo; }
    public const uint LEFTDOWN = 0x0002, LEFTUP = 0x0004;
    public const uint KEYEVENTF_KEYUP = 0x0002, KEYEVENTF_UNICODE = 0x0004;
}
"@
[GTInput]::SetProcessDPIAware() | Out-Null

if ($Hwnd -gt 0) { [GTInput]::SetForegroundWindow([IntPtr]$Hwnd) | Out-Null; Start-Sleep -Milliseconds 150 }

if ($X -ge 0 -and $Y -ge 0) {
    [GTInput]::SetCursorPos($X, $Y) | Out-Null
    Start-Sleep -Milliseconds 80
    $clicks = if ($Double) { 2 } else { 1 }
    for ($i = 0; $i -lt $clicks; $i++) {
        [GTInput]::mouse_event([GTInput]::LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
        [GTInput]::mouse_event([GTInput]::LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 60
    }
    Write-Output "OK click $X,$Y"
    exit 0
}

if ($Text -ne "") {
    # Unicode text input via KEYEVENTF_UNICODE.
    $inputs = New-Object 'System.Collections.Generic.List[GTInput+INPUT]'
    foreach ($ch in $Text.ToCharArray()) {
        $down = New-Object GTInput+INPUT
        $down.type = 1
        $down.u.ki.wVk = 0
        $down.u.ki.wScan = [uint16]$ch
        $down.u.ki.dwFlags = [GTInput]::KEYEVENTF_UNICODE
        $up = New-Object GTInput+INPUT
        $up.type = 1
        $up.u.ki.wVk = 0
        $up.u.ki.wScan = [uint16]$ch
        $up.u.ki.dwFlags = [GTInput]::KEYEVENTF_UNICODE -bor [GTInput]::KEYEVENTF_KEYUP
        $inputs.Add($down); $inputs.Add($up)
    }
    $arr = $inputs.ToArray()
    [GTInput]::SendInput([uint32]$arr.Count, $arr, [Runtime.InteropServices.Marshal]::SizeOf([type][GTInput+INPUT])) | Out-Null
    Write-Output "OK type '$Text'"
    exit 0
}

if ($Key -ne "") {
    $vk = [Convert]::ToUInt16($Key, 16)
    $down = New-Object GTInput+INPUT
    $down.type = 1; $down.u.ki.wVk = $vk
    $up = New-Object GTInput+INPUT
    $up.type = 1; $up.u.ki.wVk = $vk; $up.u.ki.dwFlags = [GTInput]::KEYEVENTF_KEYUP
    $arr = @($down, $up)
    [GTInput]::SendInput(2, $arr, [Runtime.InteropServices.Marshal]::SizeOf([type][GTInput+INPUT])) | Out-Null
    Write-Output "OK key 0x$($Key)"
    exit 0
}

Write-Error "nothing to do: pass -X/-Y, -Text or -Key"
exit 1
