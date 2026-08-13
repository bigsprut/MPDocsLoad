# OCR of a screenshot via Windows.Media.Ocr (built-in, offline).
# ASCII-only. Usage: ocr.ps1 <png> [-Words] [-Lang <tag>]
#   default: prints recognized plain text
#   -Words: prints "word|X|Y|W|H" per word (pixel coords in the image == screen
#           coords when captured by shot.ps1)
param(
    [Parameter(Mandatory = $true)][string]$Png,
    [switch]$Words,
    [string]$Lang
)
# Force UTF-8 stdout: without this, redirected output uses OEM (cp866) and
# Cyrillic words won't match UTF-8 patterns in bash (grep/awk).
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Media.Ocr.OcrEngine, Windows.Media.Ocr, ContentType = WindowsRuntime]
$null = [Windows.Graphics.Imaging.BitmapDecoder, Windows.Graphics.Imaging, ContentType = WindowsRuntime]
$null = [Windows.Graphics.Imaging.SoftwareBitmap, Windows.Graphics.Imaging, ContentType = WindowsRuntime]
$null = [Windows.Storage.StorageFile, Windows.Storage, ContentType = WindowsRuntime]
$null = [Windows.Storage.Streams.IRandomAccessStream, Windows.Storage.Streams, ContentType = WindowsRuntime]

# Async WinRT -> Task await helpers.
$asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() |
    Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and
        $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
function AwaitOp($op, $resultType) {
    $asTask = $asTaskGeneric.MakeGenericMethod($resultType)
    $netTask = $asTask.Invoke($null, @($op))
    $netTask.Wait(-1) | Out-Null
    return $netTask.Result
}

$path = (Resolve-Path $Png).Path
$file = AwaitOp ([Windows.Storage.StorageFile]::GetFileFromPathAsync($path)) ([Windows.Storage.StorageFile])
$stream = AwaitOp ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
$decoder = AwaitOp ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
$bmp = AwaitOp ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
# OcrEngine supports Bgra8 (and Gray8); convert if decoder gave something else.
if ($bmp.BitmapPixelFormat -ne [Windows.Graphics.Imaging.BitmapPixelFormat]::Bgra8) {
    $bmp = [Windows.Graphics.Imaging.SoftwareBitmap]::Convert($bmp, [Windows.Graphics.Imaging.BitmapPixelFormat]::Bgra8)
}
$engine = $null
if ($Lang) {
    $lg = [Windows.Globalization.Language]::new($Lang)
    if ([Windows.Media.Ocr.OcrEngine]::IsLanguageSupported($lg)) {
        $engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage($lg)
    } else { Write-Warning "lang $Lang not supported; fallback to profile" }
}
if (-not $engine) { $engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages() }
if (-not $engine) { Write-Error "no OCR engine (no language packs?)"; exit 1 }
$res = AwaitOp ($engine.RecognizeAsync($bmp)) ([Windows.Media.Ocr.OcrResult])

if ($Words) {
    foreach ($line in $res.Lines) {
        foreach ($w in $line.Words) {
            $rc = $w.BoundingRect
            Write-Output ("{0}|{1}|{2}|{3}|{4}" -f $w.Text, [int]$rc.X, [int]$rc.Y, [int]$rc.Width, [int]$rc.Height)
        }
    }
} else {
    Write-Output $res.Text
}
