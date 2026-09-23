# CI step: builds Amalith and its installer (Amalith-Setup.exe) for the
# Windows job in .github/workflows/release.yml. Releases are built only in
# CI; this isn't meant to be run by hand.
#
# No code signing here — there's no Windows code-signing certificate yet,
# so the installer is unsigned and Windows SmartScreen will show an
# "unknown publisher" warning when it's run, until one is added later.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$Version = if ($env:VERSION) { $env:VERSION } else {
    (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"').Matches[0].Groups[1].Value
}

Write-Host "==> Building release ($Version)"
$env:AMALITH_VERSION = $Version
# Amalith.exe is the app (and the `Amalith script` headless engine, see
# crates/amalith-shell/src/main.rs); amalith-console becomes Amalith.com, the
# console front door terminals resolve first (see crates/amalith-console).
cargo build --release -p amalith-shell -p amalith-console
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$StageDir = "target/package/windows"
Remove-Item -Recurse -Force $StageDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
Copy-Item "target/release/Amalith.exe" "$StageDir/Amalith.exe"
# A PE executable runs the same whatever its extension; .com just makes
# Windows pick it over Amalith.exe when someone types `Amalith`.
Copy-Item "target/release/amalith-console.exe" "$StageDir/Amalith.com"

Write-Host "==> Building installer"
$Iscc = Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"
if (-not (Test-Path $Iscc)) {
    choco install innosetup --no-progress -y
    if ($LASTEXITCODE -ne 0) { throw "installing Inno Setup failed" }
}
# The output name has no version (like the macOS .dmg) so the website can
# always find it by its "-setup.exe" suffix.
& $Iscc /Qp "/DAppVersion=$Version" "scripts/windows-installer.iss"
if ($LASTEXITCODE -ne 0) { throw "ISCC failed" }

Write-Host "==> Done: target/package/Amalith-Setup.exe"
