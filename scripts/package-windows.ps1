# CI step: builds and zips Amalith for the Windows job in
# .github/workflows/release.yml. Releases are built only in CI; this isn't
# meant to be run by hand.
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
# One binary: the headless .jsx engine is the `Amalith script` subcommand
# rather than a second executable (see crates/amalith-shell/src/main.rs).
cargo build --release -p amalith-shell

$StageDir = "target/package/windows"
# No version in the filename (like the macOS .dmg) so the website can link
# to a stable releases/latest/download/Amalith-Windows.zip URL that never
# needs updating.
$ZipName = "Amalith-Windows.zip"
Remove-Item -Recurse -Force $StageDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
Copy-Item "target/release/Amalith.exe" "$StageDir/Amalith.exe"

Write-Host "==> Zipping"
$ZipPath = "target/package/$ZipName"
Remove-Item -Force $ZipPath -ErrorAction SilentlyContinue
Compress-Archive -Path "$StageDir/*" -DestinationPath $ZipPath

Write-Host "==> Done: $ZipPath"
