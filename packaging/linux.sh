#!/bin/bash
# Copyright (c) 2026 Adequate Systems HK Limited
# Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.
# Makes the Linux packages for Tawara, for x86_64 and glibc 2.35 or later:
# Tawara-X.Y.Z-x86_64.AppImage and tawara-desktop_X.Y.Z_amd64.deb, with a
# SHA256SUMS file beside them.
#
# The build runs in an ubuntu:22.04 container on linux/amd64, pinned by digest,
# and packaging/linux/in-container.sh does the work inside it. The AppImage is
# made by appimagetool 1.9.1 with its type 2 runtime, and the .deb by dpkg-deb
# and dpkg-shlibdeps. glibc 2.35 is the floor.
#
# Nothing here holds a key, certificate or password. The GPG key is named by its
# identity in the environment, and is used only when that variable is set. A
# release runs on the machine that holds the keys. CI runs this script with
# --dev: unsigned, with a test icon, and nothing kept.
#
# Usage: bash packaging/linux.sh [--dev] [--out DIR]
#
#   --dev       A local check build, not for distribution. The icon and the
#               maintainer are optional, and the files are named as development
#               builds. Without an icon there is no AppImage.
#   --out DIR   Where the packages are written. Default: target/package/linux
#               under the repository root.
#
# The variables are listed in packaging/README.md.

set -eu
set -o pipefail

# The pins. Each download is checked against its SHA-256 before it runs.
# UBUNTU_IMAGE is the digest of the image index; docker resolves it to the
# linux/amd64 image.
UBUNTU_IMAGE=ubuntu:22.04@sha256:0a9887931b0e38af2e3487ed39e29436b0535c4f3f52e57745baaf79d1898956
RUSTUP_VERSION=1.29.1
RUSTUP_SHA256=dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71
APPIMAGETOOL_VERSION=1.9.1
APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
RUNTIME_VERSION=20251108
RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

# The maintainer a development build names when TAWARA_DEB_MAINTAINER is unset.
# A release refuses it.
placeholder_maintainer='Tawara Maintainer <maintainer@example.invalid>'

usage() {
  cat <<'USAGE'
usage: bash packaging/linux.sh [--dev] [--out DIR]

  --dev       local check build: icon and maintainer optional, no AppImage without an icon
  --out DIR   output folder (default: target/package/linux)

Environment: TAWARA_ICON_PNG, TAWARA_DEB_MAINTAINER, TAWARA_GPG_KEY.
packaging/README.md says what each one does.
USAGE
}

die() {
  echo "linux.sh: error: $*" >&2
  exit 1
}

warn() {
  echo "linux.sh: warning: $*" >&2
}

step() {
  echo "linux.sh: $*"
}

# Signs one file with the key in TAWARA_GPG_KEY, detached and armored, and checks
# the signature with the same keyring.
sign_file() {
  gpg --batch --yes --armor --local-user "$gpg_key" --detach-sign --output "$1.asc" "$1" \
    || die "gpg could not sign $1"
  gpg --batch --verify "$1.asc" "$1" >/dev/null 2>&1 || die "the signature on $1 does not verify"
}

dev=false
out=
icon=${TAWARA_ICON_PNG:-}
maintainer=${TAWARA_DEB_MAINTAINER:-}
gpg_key=${TAWARA_GPG_KEY:-}

while [ $# -gt 0 ]; do
  case "$1" in
    --dev)
      dev=true
      ;;
    --out)
      [ $# -ge 2 ] || die "--out needs a folder"
      out=$2
      shift
      ;;
    --out=*)
      out=${1#--out=}
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      die "unknown argument: $1"
      ;;
  esac
  shift
done

here=$(cd "$(dirname "$0")" && pwd) || die "cannot find the script's folder"
root=$(cd "$here/.." && pwd) || die "cannot find the repository root"

# Paths given relative to the caller's folder are made absolute before the
# script moves to the repository root.
case "$out" in
  ""|/*) ;;
  *) out=$PWD/$out ;;
esac
case "$icon" in
  ""|/*) ;;
  *) icon=$PWD/$icon ;;
esac

cd "$root"

for tool in awk cksum sed shasum; do
  command -v "$tool" >/dev/null 2>&1 || die "$tool is not on PATH"
done

# The version is the one [workspace.package] in Cargo.toml sets, which
# crates/desktop inherits.
version=$(awk '
  /^\[/ { in_package = ($0 == "[workspace.package]") }
  in_package && /^version[ \t]*=/ { split($0, parts, "\""); print parts[2]; exit }
' Cargo.toml)
printf '%s\n' "$version" | grep -E '^[0-9]+\.[0-9]+\.[0-9]+$' >/dev/null \
  || die "the version under [workspace.package] in Cargo.toml is not X.Y.Z: '$version'"

if $dev; then
  deb_version="${version}~dev"
  deb_file="tawara-desktop_${version}~dev_amd64.deb"
  appimage_file="Tawara-${version}-dev-x86_64.AppImage"
else
  deb_version=$version
  deb_file="tawara-desktop_${version}_amd64.deb"
  appimage_file="Tawara-${version}-x86_64.AppImage"
fi

work=$(mktemp -d "${TMPDIR:-/tmp}/tawara-linux.XXXXXX") || die "cannot make a working folder"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/icons"

# The icon's sizes are made with sips on macOS. Where sips is absent, as on a
# Linux CI runner, ImageMagick's identify and convert do the same work. The icon
# must be a square PNG of at least 1024 pixels.
has_icon=no
if [ -n "$icon" ]; then
  [ -f "$icon" ] || die "TAWARA_ICON_PNG names no file: $icon"
  if command -v sips >/dev/null 2>&1; then
    icon_tool=sips
    icon_info=$(sips -g format -g pixelWidth -g pixelHeight "$icon" 2>/dev/null) \
      || die "sips cannot read $icon"
    icon_format=$(printf '%s\n' "$icon_info" | awk '$1 == "format:" { print $2 }')
    icon_width=$(printf '%s\n' "$icon_info" | awk '$1 == "pixelWidth:" { print $2 }')
    icon_height=$(printf '%s\n' "$icon_info" | awk '$1 == "pixelHeight:" { print $2 }')
  elif command -v identify >/dev/null 2>&1 && command -v convert >/dev/null 2>&1; then
    icon_tool=imagemagick
    icon_info=$(identify -format '%m %w %h\n' "$icon" 2>/dev/null | sed -n '1p') \
      || die "identify cannot read $icon"
    icon_format=$(printf '%s\n' "$icon_info" | awk '{ print tolower($1) }')
    icon_width=$(printf '%s\n' "$icon_info" | awk '{ print $2 }')
    icon_height=$(printf '%s\n' "$icon_info" | awk '{ print $3 }')
  else
    die "making the icon sizes needs sips (macOS), or ImageMagick's identify and convert"
  fi
  [ "$icon_format" = png ] || die "TAWARA_ICON_PNG is not a PNG: $icon"
  case "$icon_width" in
    ''|*[!0-9]*) die "cannot read the width of $icon" ;;
  esac
  [ "$icon_width" = "$icon_height" ] \
    || die "TAWARA_ICON_PNG is not square: $icon is $icon_width by $icon_height"
  [ "$icon_width" -ge 1024 ] || die "TAWARA_ICON_PNG is smaller than 1024 pixels: $icon"
  for size in 16 32 48 64 128 256 512; do
    if [ "$icon_tool" = sips ]; then
      sips -z "$size" "$size" "$icon" --out "$work/icons/icon-$size.png" >/dev/null \
        || die "sips could not make the $size-pixel icon"
    else
      convert "$icon" -resize "${size}x${size}" "PNG:$work/icons/icon-$size.png" \
        || die "convert could not make the $size-pixel icon"
    fi
  done
  has_icon=yes
elif $dev; then
  warn "TAWARA_ICON_PNG is unset: this development build has no icons and no AppImage"
else
  die "TAWARA_ICON_PNG is unset. A release needs the app icon: a square PNG of at least 1024 pixels. Use --dev for a local build without one."
fi

if [ -z "$maintainer" ]; then
  if $dev; then
    warn "TAWARA_DEB_MAINTAINER is unset: the development .deb names the placeholder maintainer"
    maintainer=$placeholder_maintainer
  else
    die "TAWARA_DEB_MAINTAINER is unset. A release needs the .deb's maintainer, as 'Name <address>'."
  fi
fi
case "$maintainer" in
  *' <'*'@'*'>') ;;
  *) die "TAWARA_DEB_MAINTAINER is not in the form 'Name <address>': $maintainer" ;;
esac
case "$maintainer" in
  "$placeholder_maintainer"|*'.invalid>')
    if $dev; then
      warn "the .deb names a placeholder maintainer, which a release refuses"
    else
      die "TAWARA_DEB_MAINTAINER is still a placeholder: '$maintainer'. Set it to the maintainer's real name and address."
    fi
    ;;
esac

for notice in LICENSE.md THIRD-PARTY-NOTICES.txt; do
  [ -f "$root/$notice" ] || die "$notice is missing at the repository root; a release ships it"
done

# The signing key is checked before the build so a missing key fails early.
signed=no
if [ -n "$gpg_key" ]; then
  command -v gpg >/dev/null 2>&1 || die "TAWARA_GPG_KEY is set, but gpg is not on PATH"
  gpg --batch --list-secret-keys "$gpg_key" >/dev/null 2>&1 \
    || die "TAWARA_GPG_KEY names no secret key in this keyring: $gpg_key"
fi

if [ -z "$out" ]; then
  out=$root/target/package/linux
fi
mkdir -p "$out"
rm -f "$out/$deb_file" "$out/$deb_file.asc" "$out/$appimage_file" "$out/$appimage_file.asc" \
  "$out/SHA256SUMS" "$out/SHA256SUMS.asc"

# The build's volume is named for this checkout, so two worktrees never share a
# target directory. The next run reuses it.
key=$(printf '%s' "$root" | cksum | awk '{print $1}')
cache_volume="tawara-linux-$key"

command -v docker >/dev/null 2>&1 || die "docker is not on PATH"
docker info >/dev/null 2>&1 || die "docker does not answer: start the Docker daemon (Docker Desktop on macOS)"

step "building in $UBUNTU_IMAGE on linux/amd64, with 2 CPUs, 4 GiB and CARGO_BUILD_JOBS=2"
docker run --rm --platform linux/amd64 --cpus=2 --memory=4g \
  -e CARGO_BUILD_JOBS=2 \
  -e TAWARA_VERSION="$version" \
  -e TAWARA_PACKAGE_VERSION="$deb_version" \
  -e TAWARA_DEB_FILE="$deb_file" \
  -e TAWARA_APPIMAGE_FILE="$appimage_file" \
  -e TAWARA_DEB_MAINTAINER="$maintainer" \
  -e TAWARA_HAS_ICON="$has_icon" \
  -e RUSTUP_VERSION="$RUSTUP_VERSION" \
  -e RUSTUP_SHA256="$RUSTUP_SHA256" \
  -e APPIMAGETOOL_VERSION="$APPIMAGETOOL_VERSION" \
  -e APPIMAGETOOL_SHA256="$APPIMAGETOOL_SHA256" \
  -e RUNTIME_VERSION="$RUNTIME_VERSION" \
  -e RUNTIME_SHA256="$RUNTIME_SHA256" \
  -v "$root:/src:ro" \
  -v "$cache_volume:/cache" \
  -v "$out:/out" \
  -v "$work/icons:/icons:ro" \
  "$UBUNTU_IMAGE" bash /src/packaging/linux/in-container.sh \
  || die "the build in the container failed"

[ -f "$out/$deb_file" ] || die "the container made no $deb_file"
if [ "$has_icon" = yes ]; then
  [ -f "$out/$appimage_file" ] || die "the container made no $appimage_file"
  ( cd "$out" && shasum -a 256 "$deb_file" "$appimage_file" > SHA256SUMS ) \
    || die "could not write SHA256SUMS"
else
  ( cd "$out" && shasum -a 256 "$deb_file" > SHA256SUMS ) || die "could not write SHA256SUMS"
fi
( cd "$out" && shasum -a 256 -c SHA256SUMS >/dev/null ) || die "SHA256SUMS does not check"

if [ -n "$gpg_key" ]; then
  step "signing SHA256SUMS and the packages with the key in TAWARA_GPG_KEY"
  sign_file "$out/SHA256SUMS"
  sign_file "$out/$deb_file"
  if [ "$has_icon" = yes ]; then
    sign_file "$out/$appimage_file"
  fi
  signed=yes
fi

step "packages: $out/$deb_file"
if [ "$has_icon" = yes ]; then
  step "packages: $out/$appimage_file"
else
  step "no AppImage: it needs an icon"
fi
step "signed: $signed"
cat "$out/SHA256SUMS"
if $dev; then
  step "this is a development build, not for distribution"
fi
