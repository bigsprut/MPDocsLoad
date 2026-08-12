# Скриншот основного экрана. Путь сохранения — первый аргумент (Windows-путь).
# Использование из bash: powershell.exe -NoProfile -File screenshot.ps1 'C:\path\out.png'
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
$out = $args[0]
if (-not $out) { Write-Error "путь не задан (аргумент 1)"; exit 1 }
$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = New-Object System.Drawing.Bitmap($b.Width, $b.Height)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
$bmp.Save($out)
$g.Dispose()
$bmp.Dispose()
Write-Output "saved: $out"
