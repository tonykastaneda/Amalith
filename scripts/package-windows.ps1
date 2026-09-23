# Builds and zips Amalith for Windows. Run from the repo root, on Windows
# (this can't be cross-compiled reliably from macOS via rustup's std alone —
# see scripts/package.sh's cargo-xwin path for the cross-compile option;
# this script and the release CI job are the native-Windows equivalent).
#
# No code signing here — there's no Windows code-signing certificate yet,
# so the built .exe is unsigned and Windows SmartScreen will show an
# "unknown publisher" warning on first run until one is added later.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$Version = if ($env:VERSION) { $env:VERSION } else {
    (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"').Matches[0].Groups[1].Value
}

Write-Host "==> Building release ($Version)"
$env:AMALITH_VERSION = $Version
# amalith-script ships beside the app binary: the built-in terminal puts the
# app's own directory on the spawned shell's PATH, so scripts and coding agents
# can run it by bare name (see crates/amalith-shell/src/agent.rs).
cargo build --release -p amalith-shell -p amalith-script

$StageDir = "target/package/windows"
# No version in the filename (like the macOS .dmg) so the website can link
# to a stable releases/latest/download/Amalith-Windows.zip URL that never
# needs updating.
$ZipName = "Amalith-Windows.zip"
Remove-Item -Recurse -Force $StageDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
Copy-Item "target/release/Amalith.exe" "$StageDir/Amalith.exe"
Copy-Item "target/release/amalith-script.exe" "$StageDir/amalith-script.exe"

Write-Host "==> Zipping"
$ZipPath = "target/package/$ZipName"
Remove-Item -Force $ZipPath -ErrorAction SilentlyContinue
Compress-Archive -Path "$StageDir/*" -DestinationPath $ZipPath

Write-Host "==> Done: $ZipPath"
