param([long]$Hwnd)
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class F2 {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
    [StructLayout(LayoutKind.Sequential)] public struct R { public int L,T,Rr,B; }
}
"@
[F2]::SetProcessDPIAware() | Out-Null
$h = [IntPtr]$Hwnd
[F2]::ShowWindow($h, 9) | Out-Null   # SW_RESTORE
# Trick 1: WScript AppActivate by hwnd
try { $ws = New-Object -ComObject WScript.Shell; $null = $ws.AppActivate($Hwnd) } catch {}
# Trick 2: AttachThreadInput to foreground + target, then SetForegroundWindow
$fg = [F2]::GetForegroundWindow()
$dummy = 0
$fgThread = [F2]::GetWindowThreadProcessId($fg, [ref]$dummy)
$tThread = [F2]::GetWindowThreadProcessId($h, [ref]$dummy)
$cur = [F2]::GetCurrentThreadId()
[F2]::AttachThreadInput($cur, $fgThread, $true) | Out-Null
[F2]::AttachThreadInput($cur, $tThread, $true) | Out-Null
[F2]::SetForegroundWindow($h) | Out-Null
[F2]::BringWindowToTop($h) | Out-Null
[F2]::AttachThreadInput($cur, $fgThread, $false) | Out-Null
[F2]::AttachThreadInput($cur, $tThread, $false) | Out-Null
Start-Sleep -Milliseconds 500
$now = [F2]::GetForegroundWindow()
$r = New-Object F2+R
[F2]::GetWindowRect($h, [ref]$r) | Out-Null
Write-Output ("FOREGROUND={0} RECT {1} {2} {3} {4}" -f ($now -eq $h), $r.L, $r.T, $r.Rr, $r.B)
