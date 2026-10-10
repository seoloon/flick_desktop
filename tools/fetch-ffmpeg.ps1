# Downloads a GPL ffmpeg build for Windows x64 into third_party/ffmpeg/windows-x64 (used to convert titles for AirPlay).
# Source: https://github.com/BtbN/FFmpeg-Builds (ffmpeg-master-latest-win64-gpl.zip).
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root "third_party\ffmpeg\windows-x64"
New-Item -ItemType Directory -Force $dest | Out-Null
$archive = Join-Path $env:TEMP "ffmpeg-win64-gpl.zip"
$extracted = Join-Path $env:TEMP "ffmpeg-win64-gpl"
Write-Host "Downloading ffmpeg (GPL build)"
Invoke-WebRequest "https://github.com/BtbN/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-win64-gpl.zip" -OutFile $archive
if (Test-Path $extracted) { Remove-Item -Recurse -Force $extracted }
Expand-Archive $archive -DestinationPath $extracted
$exe = Get-ChildItem -Recurse $extracted -Filter ffmpeg.exe | Select-Object -First 1
if (-not $exe) { throw "ffmpeg.exe not found in the archive" }
Copy-Item $exe.FullName (Join-Path $dest "ffmpeg.exe") -Force
Remove-Item $archive
Remove-Item -Recurse -Force $extracted
Write-Host "ffmpeg ready in $dest"
