#!/usr/bin/env bash
#
# Build x86_64 Linux release artifacts in dist/linux/.
#
# Required: cargo, tar, dpkg-deb, rpmbuild, appimagetool, sha256sum
# Run on an x86_64 Linux host: ./scripts/package-linux.sh
#
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

APP="Amalith"
APP_ID="org.amalith.Amalith"
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
VERSION="${VERSION:-0.1.0}"
out="$root/dist/linux"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if [ "$(uname -s)" != "Linux" ] || [ "$(uname -m)" != "x86_64" ]; then
  echo "package-linux.sh requires an x86_64 Linux host" >&2
  exit 1
fi

missing=()
for tool in cargo tar dpkg-deb rpmbuild appimagetool sha256sum; do
  command -v "$tool" >/dev/null || missing+=("$tool")
done
if [ "${#missing[@]}" -ne 0 ]; then
  echo "missing Linux packaging tools: ${missing[*]}" >&2
  exit 1
fi

rm -rf "$out"
mkdir -p "$out/arch" "$out/flatpak"

echo "==> cargo build --release"
cargo build --release -p amalith-shell
bin="$root/target/release/$APP"
[ -x "$bin" ] || { echo "missing $bin" >&2; exit 1; }

desktop="$work/$APP_ID.desktop"
cat > "$desktop" <<EOF
[Desktop Entry]
Type=Application
Name=$APP
Comment=Professional vector design application
Exec=$APP %F
Icon=$APP_ID
Terminal=false
Categories=Graphics;VectorGraphics;
MimeType=image/svg+xml;
EOF

# Portable tarball ------------------------------------------------------------
archive_root="$work/amalith-$VERSION-x86_64"
mkdir -p "$archive_root"
install -m 0755 "$bin" "$archive_root/$APP"
install -m 0644 "$root/crates/amalith-shell/assets/app-icon.png" "$archive_root/$APP_ID.png"
install -m 0644 "$desktop" "$archive_root/$APP_ID.desktop"
cat > "$archive_root/README.txt" <<EOF
$APP $VERSION — Linux x86_64

Run ./$APP, or install the binary somewhere on PATH. The .desktop file and
icon are included for desktop-menu integration.
EOF
tarball="$out/amalith-$VERSION-x86_64.tar.gz"
tar -C "$work" -czf "$tarball" "$(basename "$archive_root")"

# Debian package --------------------------------------------------------------
debroot="$work/deb"
mkdir -p "$debroot/DEBIAN" "$debroot/usr/bin" \
  "$debroot/usr/share/applications" "$debroot/usr/share/icons/hicolor/512x512/apps"
install -m 0755 "$bin" "$debroot/usr/bin/$APP"
install -m 0644 "$desktop" "$debroot/usr/share/applications/$APP_ID.desktop"
install -m 0644 "$root/crates/amalith-shell/assets/app-icon.png" \
  "$debroot/usr/share/icons/hicolor/512x512/apps/$APP_ID.png"
installed_size="$(du -sk "$debroot/usr" | cut -f1)"
cat > "$debroot/DEBIAN/control" <<EOF
Package: amalith
Version: $VERSION
Section: graphics
Priority: optional
Architecture: amd64
Installed-Size: $installed_size
Maintainer: Amalith Contributors
Description: Professional vector design application
 Amalith is a native, open-source vector design application.
EOF
dpkg-deb --build --root-owner-group "$debroot" "$out/amalith_${VERSION}_amd64.deb" >/dev/null

# RPM package -----------------------------------------------------------------
rpmroot="$work/rpmbuild"
mkdir -p "$rpmroot/BUILD" "$rpmroot/BUILDROOT" "$rpmroot/RPMS" \
  "$rpmroot/SOURCES" "$rpmroot/SPECS" "$rpmroot/SRPMS"
install -m 0755 "$bin" "$rpmroot/SOURCES/$APP"
install -m 0644 "$desktop" "$rpmroot/SOURCES/$APP_ID.desktop"
install -m 0644 "$root/crates/amalith-shell/assets/app-icon.png" "$rpmroot/SOURCES/$APP_ID.png"
cat > "$rpmroot/SPECS/amalith.spec" <<EOF
Name:           amalith
Version:        $VERSION
Release:        1%{?dist}
Summary:        Professional vector design application
License:        MIT OR Apache-2.0
URL:            https://www.amalith.app/
Source0:        $APP
Source1:        $APP_ID.desktop
Source2:        $APP_ID.png

%description
Amalith is a native, open-source vector design application.

%install
install -Dm755 %{SOURCE0} %{buildroot}%{_bindir}/$APP
install -Dm644 %{SOURCE1} %{buildroot}%{_datadir}/applications/$APP_ID.desktop
install -Dm644 %{SOURCE2} %{buildroot}%{_datadir}/icons/hicolor/512x512/apps/$APP_ID.png

%files
%{_bindir}/$APP
%{_datadir}/applications/$APP_ID.desktop
%{_datadir}/icons/hicolor/512x512/apps/$APP_ID.png
EOF
rpmbuild --define "_topdir $rpmroot" -bb "$rpmroot/SPECS/amalith.spec" >/dev/null
rpm_built="$(find "$rpmroot/RPMS" -type f -name '*.rpm' -print -quit)"
[ -n "$rpm_built" ] || { echo "rpmbuild produced no RPM" >&2; exit 1; }
cp "$rpm_built" "$out/amalith-$VERSION-1.x86_64.rpm"

# AppImage --------------------------------------------------------------------
appdir="$work/$APP.AppDir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" \
  "$appdir/usr/share/icons/hicolor/512x512/apps"
install -m 0755 "$bin" "$appdir/usr/bin/$APP"
install -m 0644 "$desktop" "$appdir/$APP_ID.desktop"
install -m 0644 "$desktop" "$appdir/usr/share/applications/$APP_ID.desktop"
install -m 0644 "$root/crates/amalith-shell/assets/app-icon.png" "$appdir/$APP_ID.png"
install -m 0644 "$root/crates/amalith-shell/assets/app-icon.png" \
  "$appdir/usr/share/icons/hicolor/512x512/apps/$APP_ID.png"
ln -s "usr/bin/$APP" "$appdir/AppRun"
ARCH=x86_64 appimagetool "$appdir" "$out/$APP-$VERSION-x86_64.AppImage" >/dev/null

# Arch Linux recipe -----------------------------------------------------------
tar_sha256="$(sha256sum "$tarball" | cut -d ' ' -f1)"
cat > "$out/arch/PKGBUILD" <<EOF
pkgname=amalith
pkgver=$VERSION
pkgrel=1
pkgdesc='Professional vector design application'
arch=('x86_64')
url='https://www.amalith.app/'
license=('MIT' 'Apache')
source=("../amalith-\${pkgver}-x86_64.tar.gz")
sha256sums=('$tar_sha256')

package() {
  install -Dm755 "\$srcdir/amalith-\$pkgver-x86_64/$APP" "\$pkgdir/usr/bin/$APP"
  install -Dm644 "\$srcdir/amalith-\$pkgver-x86_64/$APP_ID.desktop" \
    "\$pkgdir/usr/share/applications/$APP_ID.desktop"
  install -Dm644 "\$srcdir/amalith-\$pkgver-x86_64/$APP_ID.png" \
    "\$pkgdir/usr/share/icons/hicolor/512x512/apps/$APP_ID.png"
}
EOF

# Flatpak manifest ------------------------------------------------------------
cat > "$out/flatpak/$APP_ID.yml" <<EOF
app-id: $APP_ID
runtime: org.freedesktop.Platform
runtime-version: '24.08'
sdk: org.freedesktop.Sdk
command: $APP
finish-args:
  - --device=dri
  - --share=ipc
  - --socket=fallback-x11
  - --socket=wayland
modules:
  - name: amalith
    buildsystem: simple
    build-commands:
      - install -Dm755 amalith-$VERSION-x86_64/$APP /app/bin/$APP
      - install -Dm644 amalith-$VERSION-x86_64/$APP_ID.desktop /app/share/applications/$APP_ID.desktop
      - install -Dm644 amalith-$VERSION-x86_64/$APP_ID.png /app/share/icons/hicolor/512x512/apps/$APP_ID.png
    sources:
      - type: archive
        path: ../amalith-$VERSION-x86_64.tar.gz
EOF

echo
echo "done: $out"
find "$out" -type f -print | sort | sed 's/^/  /'
