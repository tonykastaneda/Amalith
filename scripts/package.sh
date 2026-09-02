#!/usr/bin/env bash
#
# Build every shippable artifact into dist/:
#
#   dist/Amalith.dmg      — macOS, signed + notarised when the env vars below
#                           are set (otherwise an unsigned .app + .dmg)
#   dist/Windows/         — Amalith.exe (self-contained, double-click to run)
#                           + the icon + a README, ready to zip and hand over
#
# Run from anywhere:  ./scripts/package.sh
#
# macOS signing (optional — see scripts/package-macos.sh header for setup):
#   SIGN_IDENTITY="Developer ID Application: NAME (TEAMID)" \
#   NOTARY_PROFILE=amalith \
#     ./scripts/package.sh
#
# Windows cross-compile needs (one-time):
#   rustup target add x86_64-pc-windows-msvc
#   cargo install cargo-xwin
#   brew install llvm            # provides llvm-rc for the embedded icon
#
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

APP="Amalith"
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
VERSION="${VERSION:-0.1.0}"
WIN_TARGET="x86_64-pc-windows-msvc"
dist="$root/dist"
mkdir -p "$dist"

# ---------------------------------------------------------------- macOS -------
echo "########## macOS ##########"
"$root/scripts/package-macos.sh"   # writes dist/Amalith.dmg

# -------------------------------------------------------------- Windows -------
echo
echo "########## Windows ##########"
if ! [ -x "$HOME/.cargo/bin/cargo-xwin" ] && ! command -v cargo-xwin >/dev/null; then
  echo "!! cargo-xwin not installed — skipping Windows build."
  echo "   cargo install cargo-xwin && rustup target add $WIN_TARGET"
else
  # A standalone Homebrew `rust` formula shadows rustup and has no cross
  # targets — force the rustup toolchain (which carries the Windows std)
  # plus ~/.cargo/bin for cargo-xwin, plus llvm-rc for the embedded icon.
  rustup_bin="$(rustup which cargo 2>/dev/null | xargs -r dirname 2>/dev/null || true)"
  [ -n "$rustup_bin" ] && export PATH="$rustup_bin:$HOME/.cargo/bin:$PATH"
  for p in /opt/homebrew/opt/llvm/bin /usr/local/opt/llvm/bin; do
    [ -x "$p/llvm-rc" ] && export PATH="$p:$PATH"
  done
  echo "==> $(cargo --version)"

  echo "==> cargo xwin build --release --target $WIN_TARGET"
  cargo xwin build --release --target "$WIN_TARGET" -p amalith-shell

  win="$dist/Windows"
  rm -rf "$win"
  mkdir -p "$win"
  cp "$root/target/$WIN_TARGET/release/$APP.exe" "$win/$APP.exe"
  cp "$root/branding/amalith.ico" "$win/$APP.ico"
  cat > "$win/README.txt" <<EOF
Amalith $VERSION — Windows

Double-click Amalith.exe to run. Nothing to install; it's self-contained.

First launch on a machine that downloaded this may show a blue SmartScreen
notice ("Windows protected your PC") because the build isn't code-signed.
Click "More info" -> "Run anyway".

Settings live in %APPDATA%\\Amalith\\.
EOF

  echo "==> $win/$APP.exe  ($(du -h "$win/$APP.exe" | cut -f1))"
fi

echo
echo "done:"
[ -f "$dist/$APP.dmg" ]        && echo "  $dist/$APP.dmg"
[ -f "$dist/Windows/$APP.exe" ] && echo "  $dist/Windows/$APP.exe"
