#!/usr/bin/env bash
#
# Build Amalith.app for macOS.
#
#   ./scripts/package-macos.sh                 # unsigned bundle in dist/mac/
#   SIGN_IDENTITY="Developer ID Application: NAME (TEAMID)" \
#       ./scripts/package-macos.sh             # + codesign (hardened runtime)
#   SIGN_IDENTITY=...  NOTARY_PROFILE=amalith \
#       ./scripts/package-macos.sh             # + notarize + staple + .dmg
#
# One-time setup for signing / notarising:
#   1. Install a "Developer ID Application" cert (Xcode ▸ Settings ▸ Accounts
#      ▸ your team ▸ Manage Certificates ▸ + ▸ Developer ID Application).
#      Confirm:  security find-identity -p basic -v | grep "Developer ID"
#   2. Store notary credentials once:
#        xcrun notarytool store-credentials amalith \
#          --apple-id you@example.com --team-id TEAMID \
#          --password <app-specific-password>
#      (app-specific password: appleid.apple.com ▸ Sign-In and Security)
#
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

APP_NAME="Amalith"
BUNDLE_ID="${BUNDLE_ID:-com.tonykastaneda.amalith}"
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
VERSION="${VERSION:-0.1.0}"
MIN_MACOS="${MIN_MACOS:-11.0}"
ICON_SRC="$root/branding/app-icon.png"

out="$root/dist/mac"
app="$out/$APP_NAME.app"
contents="$app/Contents"

mkdir -p "$out"

echo "==> cargo build --release"
cargo build --release -p amalith-shell
bin="$root/target/release/$APP_NAME"
[ -x "$bin" ] || { echo "missing $bin"; exit 1; }

echo "==> assembling $app"
rm -rf "$app"
mkdir -p "$contents/MacOS" "$contents/Resources"
cp "$bin" "$contents/MacOS/$APP_NAME"
strip -x "$contents/MacOS/$APP_NAME" 2>/dev/null || true

echo "==> $APP_NAME.icns"
iconset="$(mktemp -d)/$APP_NAME.iconset"
mkdir -p "$iconset"
for s in 16 32 128 256 512; do
  sips -z "$s"   "$s"   "$ICON_SRC" --out "$iconset/icon_${s}x${s}.png"        >/dev/null
  sips -z $((s*2)) $((s*2)) "$ICON_SRC" --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
done
icns="$contents/Resources/$APP_NAME.icns"
if ! iconutil -c icns "$iconset" -o "$icns"; then
  # Some macOS releases reject otherwise valid iconsets. Build the modern ICNS
  # container directly from its PNG chunks as a compatible fallback.
  echo "==> iconutil rejected the iconset; using ICNS fallback"
  LC_ALL=C perl -e '
    use strict;
    my ($out, @pairs) = @ARGV;
    my $body = q{};
    while (@pairs) {
      my $type = shift @pairs;
      my $file = shift @pairs;
      open my $in, q{<:raw}, $file or die qq{$file: $!\n};
      local $/;
      my $png = <$in>;
      $body .= $type . pack(q{N}, length($png) + 8) . $png;
    }
    open my $out_file, q{>:raw}, $out or die qq{$out: $!\n};
    print {$out_file} q{icns}, pack(q{N}, length($body) + 8), $body;
  ' "$icns" \
    icp4 "$iconset/icon_16x16.png" \
    icp5 "$iconset/icon_32x32.png" \
    icp6 "$iconset/icon_32x32@2x.png" \
    ic07 "$iconset/icon_128x128.png" \
    ic08 "$iconset/icon_128x128@2x.png" \
    ic09 "$iconset/icon_256x256@2x.png" \
    ic10 "$iconset/icon_512x512@2x.png"
fi
rm -rf "$(dirname "$iconset")"

echo "==> Info.plist"
cat > "$contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>              <string>$APP_NAME</string>
  <key>CFBundleDisplayName</key>       <string>$APP_NAME</string>
  <key>CFBundleExecutable</key>        <string>$APP_NAME</string>
  <key>CFBundleIdentifier</key>        <string>$BUNDLE_ID</string>
  <key>CFBundleVersion</key>           <string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundlePackageType</key>       <string>APPL</string>
  <key>CFBundleIconFile</key>          <string>$APP_NAME</string>
  <key>LSMinimumSystemVersion</key>    <string>$MIN_MACOS</string>
  <key>NSHighResolutionCapable</key>   <true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>LSApplicationCategoryType</key> <string>public.app-category.graphics-design</string>
</dict>
</plist>
PLIST

# --- optional: codesign -------------------------------------------------------
if [ -n "${SIGN_IDENTITY:-}" ]; then
  echo "==> codesign ($SIGN_IDENTITY)"
  ents="$(mktemp).plist"
  cat > "$ents" <<'ENT'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>com.apple.security.cs.allow-jit</key><true/>
</dict></plist>
ENT
  codesign --force --deep --timestamp --options runtime \
    --entitlements "$ents" --sign "$SIGN_IDENTITY" "$app"
  codesign --verify --strict --verbose=2 "$app"
  rm -f "$ents"
else
  echo "==> (unsigned — set SIGN_IDENTITY to codesign)"
fi

# --- optional: notarize app + staple ----------------------------------------
dmg="$out/$APP_NAME.dmg"
if [ -n "${SIGN_IDENTITY:-}" ] && [ -n "${NOTARY_PROFILE:-}" ]; then
  zip="$(mktemp -d)/$APP_NAME.zip"
  echo "==> notarize app ($NOTARY_PROFILE)"
  ditto -c -k --keepParent "$app" "$zip"
  xcrun notarytool submit "$zip" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$app"
  rm -rf "$(dirname "$zip")"

fi

# --- dmg --------------------------------------------------------------------
echo "==> build $dmg"
rm -f "$dmg"
stage="$(mktemp -d)"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -volname "$APP_NAME" -srcfolder "$stage" -ov -format UDZO "$dmg" >/dev/null
rm -rf "$stage"

if [ -n "${SIGN_IDENTITY:-}" ] && [ -n "${NOTARY_PROFILE:-}" ]; then
  echo "==> notarize $dmg"
  # The dmg needs its own notarization ticket to pass Gatekeeper offline.
  xcrun notarytool submit "$dmg" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$dmg"
fi

echo
echo "done: $app"
[ -f "$dmg" ] && echo "      $dmg"
