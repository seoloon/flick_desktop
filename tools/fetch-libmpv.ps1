# Downloads the latest LGPL libmpv build for Windows x64 into third_party/mpv/windows-x64.
# Source: https://github.com/zhongfly/mpv-winbuild (mpv-dev-lgpl-x86_64-*.7z). Requires 7-Zip.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root "third_party\mpv\windows-x64"
$release = Invoke-RestMethod "https://api.github.com/repos/zhongfly/mpv-winbuild/releases/latest"
$asset = $release.assets | Where-Object { $_.name -like "mpv-dev-lgpl-x86_64-2*.7z" } | Select-Object -First 1
if (-not $asset) { throw "No LGPL x86_64 dev build in the latest release" }
New-Item -ItemType Directory -Force $dest | Out-Null
$archive = Join-Path $env:TEMP $asset.name
Write-Host "Downloading $($asset.name)"
Invoke-WebRequest $asset.browser_download_url -OutFile $archive
$sevenZip = @("$env:ProgramFiles\7-Zip\7z.exe", (Get-Command 7z -ErrorAction SilentlyContinue).Source) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $sevenZip) { throw "7-Zip is required to extract $archive" }
& $sevenZip x -y "-o$dest" $archive | Out-Null
Remove-Item $archive
Write-Host "libmpv ready in $dest"
