#!/usr/bin/env bash
#
# CI step: builds the x86_64 Linux release artifacts, plus the single
# Amalith-Linux.zip that bundles every install method for the release page,
# for the Linux job in .github/workflows/release.yml. Releases are built only
# in CI; this isn't meant to be run by hand.
#
# Required: cargo, tar, dpkg-deb, rpmbuild, appimagetool, sha256sum, zip
#
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

APP="Amalith"
APP_ID="org.amalith.Amalith"
# A pre-set VERSION (release CI passes the git tag) wins over Cargo.toml,
# so a release's version number always matches its git tag regardless of
# whether Cargo.toml's own version field was bumped for that commit.
VERSION="${VERSION:-$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)}"
VERSION="${VERSION:-0.1.0}"
out="$root/dist/linux"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if [ "$(uname -s)" != "Linux" ] || [ "$(uname -m)" != "x86_64" ]; then
  echo "package-linux.sh requires an x86_64 Linux host" >&2
  exit 1
fi

missing=()
for tool in cargo tar dpkg-deb rpmbuild appimagetool sha256sum zip; do
  command -v "$tool" >/dev/null || missing+=("$tool")
done
if [ "${#missing[@]}" -ne 0 ]; then
  echo "missing Linux packaging tools: ${missing[*]}" >&2
  exit 1
fi

rm -rf "$out"
mkdir -p "$out/arch"

echo "==> cargo build --release ($VERSION)"
export AMALITH_VERSION="$VERSION"
# One binary: the headless .jsx engine is the `Amalith script` subcommand
# rather than a second executable (see crates/amalith-shell/src/main.rs).
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

Run headless .jsx automation with:  ./$APP script yourscript.jsx
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

# One zip with every install method ------------------------------------------
# The release page carries a single Linux asset instead of four, and the Arch
# recipe travels with the tarball it checksums — its `source=` is relative to
# the PKGBUILD, so `arch/` has to stay one level below the tarball. No version
# in the zip's own name (like the Windows zip and the macOS dmg) so the site
# can link a stable releases/latest/download URL.
echo "==> $APP-Linux.zip"
stage="$work/$APP-Linux-$VERSION"
mkdir -p "$stage"
# Copied before the zip is written, so it can't end up inside itself.
cp -R "$out"/* "$stage/"
(cd "$stage" && sha256sum ./*.tar.gz ./*.deb ./*.rpm ./*.AppImage > SHA256SUMS)
cat > "$stage/INSTALL.txt" <<EOF
$APP $VERSION — Linux x86_64

Every install method is in this archive; pick whichever suits your system.
Check the downloads first with:  sha256sum -c SHA256SUMS

AppImage — nothing to install, runs anywhere
  chmod +x $APP-$VERSION-x86_64.AppImage
  ./$APP-$VERSION-x86_64.AppImage

Debian / Ubuntu
  sudo apt install ./amalith_${VERSION}_amd64.deb

Fedora / RHEL / openSUSE
  sudo dnf install ./amalith-$VERSION-1.x86_64.rpm

Arch Linux
  cd arch && makepkg -si
  (builds from the .tar.gz beside this file — keep them together)

Portable tarball — no root needed
  tar -xzf amalith-$VERSION-x86_64.tar.gz
  cd amalith-$VERSION-x86_64 && ./$APP
  To integrate it with your desktop, put $APP somewhere on PATH and copy
  $APP_ID.desktop into ~/.local/share/applications and $APP_ID.png into
  ~/.local/share/icons/hicolor/512x512/apps.

The headless .jsx runner is built into the app: run
  $APP script yourscript.jsx
Amalith's built-in terminal puts $APP itself on PATH, so that works there too.
EOF
(cd "$work" && zip -qr "$out/$APP-Linux.zip" "$APP-Linux-$VERSION")

echo
echo "done: $out"
find "$out" -type f -print | sort | sed 's/^/  /'
