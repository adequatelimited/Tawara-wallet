#!/bin/bash
# Copyright (c) 2026 Adequate Systems HK Limited
# Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.
# Runs inside the linux/amd64 ubuntu:22.04 container started by
# packaging/linux.sh. Builds tawara-desktop for x86_64-unknown-linux-gnu, checks
# the glibc floor, and writes the .deb and, when an icon is given, the AppImage
# to /out.
#
# The repository is mounted read-only at /src; build output goes to the reused
# /cache volume. All settings come from the environment. No keys, certificates
# or passwords are read.

set -eu
set -o pipefail

die() {
  echo "in-container: error: $*" >&2
  exit 1
}

step() {
  echo "in-container: $*"
}

: "${TAWARA_VERSION:?TAWARA_VERSION is not set}"
: "${TAWARA_PACKAGE_VERSION:?TAWARA_PACKAGE_VERSION is not set}"
: "${TAWARA_DEB_FILE:?TAWARA_DEB_FILE is not set}"
: "${TAWARA_DEB_MAINTAINER:?TAWARA_DEB_MAINTAINER is not set}"
: "${TAWARA_HAS_ICON:?TAWARA_HAS_ICON is not set}"
: "${TAWARA_APPIMAGE_FILE:-}"
: "${RUSTUP_VERSION:?RUSTUP_VERSION is not set}"
: "${RUSTUP_SHA256:?RUSTUP_SHA256 is not set}"
: "${APPIMAGETOOL_VERSION:?APPIMAGETOOL_VERSION is not set}"
: "${APPIMAGETOOL_SHA256:?APPIMAGETOOL_SHA256 is not set}"
: "${RUNTIME_VERSION:?RUNTIME_VERSION is not set}"
: "${RUNTIME_SHA256:?RUNTIME_SHA256 is not set}"

# Ubuntu 22.04's glibc. The executable may require no newer symbol version.
glibc_floor=2.35

# Package description shown by package managers.
description_summary='Mochimo wallet for the desktop'
description_detail=' Tawara is a wallet for the Mochimo network.'

[ "$(uname -m)" = x86_64 ] || die "the container is $(uname -m), not x86_64"

step "installing the build packages"
export DEBIAN_FRONTEND=noninteractive
apt-get update -q
apt-get install -y -q --no-install-recommends ca-certificates curl gcc libc6-dev dpkg-dev \
  file squashfs-tools
command -v objdump >/dev/null 2>&1 || die "objdump is not installed"
command -v dpkg-deb >/dev/null 2>&1 || die "dpkg-deb is not installed"
command -v dpkg-shlibdeps >/dev/null 2>&1 || die "dpkg-shlibdeps is not installed"

rustup_init=/tmp/rustup-init
step "rustup-init $RUSTUP_VERSION, checked against its SHA-256"
curl -fsSL -o "$rustup_init" \
  "https://static.rust-lang.org/rustup/archive/$RUSTUP_VERSION/x86_64-unknown-linux-gnu/rustup-init"
printf '%s  %s\n' "$RUSTUP_SHA256" "$rustup_init" | sha256sum -c - \
  || die "rustup-init does not match its SHA-256"
chmod 0755 "$rustup_init"

export RUSTUP_HOME=/cache/rustup
export CARGO_HOME=/cache/cargo
export PATH="$CARGO_HOME/bin:$PATH"
if [ ! -x "$CARGO_HOME/bin/rustup" ]; then
  "$rustup_init" -y --no-modify-path --profile minimal --default-toolchain none
fi

# Install the toolchain pinned in rust-toolchain.toml.
cd /src
step "the toolchain rust-toolchain.toml names"
rustup toolchain install --no-self-update
rustc -vV
cargo -V
host=$(rustc -vV | sed -n 's/^host: //p')
[ "$host" = x86_64-unknown-linux-gnu ] || die "the compiler's host is '$host', not x86_64-unknown-linux-gnu"

step "cargo build --release --locked for x86_64-unknown-linux-gnu"
cargo build --release --locked -p tawara-desktop --target x86_64-unknown-linux-gnu \
  --target-dir /cache/target

bin=/cache/target/x86_64-unknown-linux-gnu/release/tawara-desktop
[ -x "$bin" ] || die "cargo made no executable at $bin"

# The highest GLIBC symbol version the executable needs must not exceed the
# floor.
versions=$(objdump -T "$bin" | grep -o 'GLIBC_[0-9][0-9.]*' | sort -uV) || true
[ -n "$versions" ] || die "the executable names no GLIBC symbol version"
highest=$(printf '%s\n' "$versions" | sed 's/^GLIBC_//' | sort -V | tail -n 1)
top=$(printf '%s\n%s\n' "$highest" "$glibc_floor" | sort -V | tail -n 1)
step "GLIBC symbol versions the executable needs: $(printf '%s' "$versions" | tr '\n' ' ')"
[ "$top" = "$glibc_floor" ] \
  || die "the executable needs GLIBC_$highest, above the floor of GLIBC_$glibc_floor"
step "the highest is GLIBC_$highest, at or below the floor of GLIBC_$glibc_floor"
step "shared libraries the executable names (DT_NEEDED):"
objdump -p "$bin" | grep NEEDED || true

stage=/cache/stage
rm -rf "$stage"
mkdir -p "$stage"

# The .deb tree: the executable, the desktop entry, the hicolor icons, and the
# notices under usr/share/doc.
pkg=$stage/deb/tawara-desktop
install -d -m 0755 "$pkg/DEBIAN" "$pkg/usr/bin" "$pkg/usr/share/applications" \
  "$pkg/usr/share/doc/tawara-desktop"
install -m 0755 "$bin" "$pkg/usr/bin/tawara-desktop"
install -m 0644 /src/packaging/linux/com.adequatelimited.tawara.desktop \
  "$pkg/usr/share/applications/com.adequatelimited.tawara.desktop"
install -m 0644 /src/LICENSE.md /src/THIRD-PARTY-NOTICES.txt "$pkg/usr/share/doc/tawara-desktop/"
if [ "$TAWARA_HAS_ICON" = yes ]; then
  for size in 16 32 48 64 128 256 512; do
    icon_dir=$pkg/usr/share/icons/hicolor/${size}x${size}/apps
    install -d -m 0755 "$icon_dir"
    install -m 0644 "/icons/icon-$size.png" "$icon_dir/com.adequatelimited.tawara.png"
  done
else
  step "no icon was given: the .deb carries no icons"
fi

# dpkg-shlibdeps reads debian/control from its working directory even with -O,
# so it runs in a separate workspace rather than in the package tree.
workspace=$stage/workspace
mkdir -p "$workspace/debian"
printf 'Source: tawara-desktop\n\nPackage: tawara-desktop\nArchitecture: amd64\n' \
  > "$workspace/debian/control"
shlibs=$(cd "$workspace" && dpkg-shlibdeps -O "$pkg/usr/bin/tawara-desktop") \
  || die "dpkg-shlibdeps failed"
libs=$(printf '%s\n' "$shlibs" | sed -n 's/^shlibs:Depends=//p')
[ -n "$libs" ] || die "dpkg-shlibdeps printed no shlibs:Depends"
step "shlibs:Depends from dpkg-shlibdeps: $libs"

# Libraries the app loads at run time, which dpkg-shlibdeps cannot detect.
# libvulkan1 is only recommended: without it the app renders in software.
hand_libs='libx11-6, libx11-xcb1, libxcursor1, libxi6, libxkbcommon-x11-0, libwayland-client0, libxkbcommon0'
installed_kib=$(du -sk "$pkg/usr" | awk '{print $1}')
{
  printf 'Package: tawara-desktop\n'
  printf 'Version: %s\n' "$TAWARA_PACKAGE_VERSION"
  printf 'Architecture: amd64\n'
  printf 'Maintainer: %s\n' "$TAWARA_DEB_MAINTAINER"
  printf 'Installed-Size: %s\n' "$installed_kib"
  printf 'Depends: %s, %s\n' "$libs" "$hand_libs"
  printf 'Recommends: libvulkan1\n'
  printf 'Section: misc\n'
  printf 'Priority: optional\n'
  printf 'Description: %s\n' "$description_summary"
  printf '%s\n' "$description_detail"
} > "$pkg/DEBIAN/control"
chmod 0644 "$pkg/DEBIAN/control"

step "dpkg-deb --root-owner-group --build $TAWARA_DEB_FILE"
dpkg-deb --root-owner-group --build "$pkg" "/out/$TAWARA_DEB_FILE" || die "dpkg-deb failed"
step "dpkg-deb --info $TAWARA_DEB_FILE"
dpkg-deb --info "/out/$TAWARA_DEB_FILE"
step "dpkg-deb --contents $TAWARA_DEB_FILE"
dpkg-deb --contents "/out/$TAWARA_DEB_FILE"

if [ "$TAWARA_HAS_ICON" = yes ]; then
  appimagetool=/tmp/appimagetool-x86_64.AppImage
  runtime=/tmp/runtime-x86_64
  step "appimagetool $APPIMAGETOOL_VERSION and the type 2 runtime $RUNTIME_VERSION, each checked against its SHA-256"
  curl -fsSL -o "$appimagetool" \
    "https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL_VERSION/appimagetool-x86_64.AppImage"
  printf '%s  %s\n' "$APPIMAGETOOL_SHA256" "$appimagetool" | sha256sum -c - \
    || die "appimagetool does not match its SHA-256"
  curl -fsSL -o "$runtime" \
    "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_VERSION/runtime-x86_64"
  printf '%s  %s\n' "$RUNTIME_SHA256" "$runtime" | sha256sum -c - \
    || die "the runtime does not match its SHA-256"
  chmod 0755 "$appimagetool"

  # appimagetool is itself an AppImage (type 2 runtime followed by a squashfs
  # image). Emulated x86_64 containers reject its ELF header's AppImage magic
  # with "Exec format error", so the squashfs part is extracted and the pinned
  # tool is run through its AppRun instead.
  runtime_bytes=$(stat -c %s "$runtime")
  tail -c +$((runtime_bytes + 1)) "$appimagetool" > /tmp/appimagetool.squashfs
  rm -rf /tmp/appimagetool-root
  unsquashfs -no-progress -d /tmp/appimagetool-root /tmp/appimagetool.squashfs >/dev/null \
    || die "the squashfs part of appimagetool does not unpack"
  [ -x /tmp/appimagetool-root/AppRun ] || die "the unpacked appimagetool has no AppRun"

  # The AppDir: AppRun, the desktop entry at the top and in usr/share, the
  # 256-pixel icon, and the notices beside the executable.
  appdir=$stage/Tawara.AppDir
  icon256=/icons/icon-256.png
  install -d -m 0755 "$appdir/usr/bin" "$appdir/usr/share/applications" \
    "$appdir/usr/share/icons/hicolor/256x256/apps"
  install -m 0755 "$bin" "$appdir/usr/bin/tawara-desktop"
  install -m 0644 /src/LICENSE.md /src/THIRD-PARTY-NOTICES.txt "$appdir/usr/bin/"
  install -m 0644 /src/packaging/linux/com.adequatelimited.tawara.desktop \
    "$appdir/usr/share/applications/com.adequatelimited.tawara.desktop"
  install -m 0644 /src/packaging/linux/com.adequatelimited.tawara.desktop \
    "$appdir/com.adequatelimited.tawara.desktop"
  install -m 0644 "$icon256" \
    "$appdir/usr/share/icons/hicolor/256x256/apps/com.adequatelimited.tawara.png"
  install -m 0644 "$icon256" "$appdir/com.adequatelimited.tawara.png"
  ln -s com.adequatelimited.tawara.png "$appdir/.DirIcon"
  install -m 0755 /src/packaging/linux/AppRun "$appdir/AppRun"

  step "appimagetool makes $TAWARA_APPIMAGE_FILE from its unpacked tree, without FUSE"
  (cd /tmp && ARCH=x86_64 VERSION="$TAWARA_VERSION" /tmp/appimagetool-root/AppRun \
    --runtime-file "$runtime" "$appdir" "/out/$TAWARA_APPIMAGE_FILE") \
    || die "appimagetool failed"
  chmod 0755 "/out/$TAWARA_APPIMAGE_FILE"

  # The new AppImage's squashfs part, which starts where the runtime ends, must
  # contain AppRun and the executable.
  tail -c +$((runtime_bytes + 1)) "/out/$TAWARA_APPIMAGE_FILE" > /tmp/appimage.squashfs
  listing=$(unsquashfs -l /tmp/appimage.squashfs) || die "the squashfs part of $TAWARA_APPIMAGE_FILE does not list"
  printf '%s\n' "$listing" | grep -q '/AppRun$' || die "$TAWARA_APPIMAGE_FILE holds no AppRun"
  printf '%s\n' "$listing" | grep -q '/usr/bin/tawara-desktop$' \
    || die "$TAWARA_APPIMAGE_FILE holds no usr/bin/tawara-desktop"
  step "$TAWARA_APPIMAGE_FILE holds AppRun and usr/bin/tawara-desktop"
else
  step "no icon was given: no AppImage (a development build without TAWARA_ICON_PNG)"
fi

step "the packages in /out:"
ls -l /out
