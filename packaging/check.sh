#!/bin/bash
# Copyright (c) 2026 Adequate Systems HK Limited
# Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.
# Checks the packages that packaging/macos.sh, packaging/linux.sh and
# packaging/windows.sh make. .github/workflows/packages.yml runs each check
# on its host.
#
# The checks read the packages and the executable, and change nothing. None of
# them needs a key, a certificate or a network. A check that fails says why and
# exits 1; each one that passes prints a line.
#
# Usage: bash packaging/check.sh macos DIR
#        bash packaging/check.sh linux DIR
#        bash packaging/check.sh windows-exe FILE
#        bash packaging/check.sh windows DIR
#
#   macos        the folder with Tawara.app and the disk image (macos.sh)
#   linux        the folder with the .deb and the AppImage (linux.sh)
#   windows-exe  tawara-desktop.exe as cargo builds it, before the installer
#   windows      the folder with the installer (windows.sh)
#
# Each package must carry the version that [workspace.package] sets in
# Cargo.toml. A development build's names carry "dev", as the scripts give them.

set -eu
set -o pipefail

# Git Bash rewrites arguments that begin with a slash. The PowerShell command
# below takes none, but this matches windows.sh.
MSYS2_ARG_CONV_EXCL='*'
MSYS_NO_PATHCONV=1
export MSYS2_ARG_CONV_EXCL MSYS_NO_PATHCONV

# Ubuntu 22.04's glibc: the highest GLIBC symbol version the executable may need.
glibc_floor=2.35

usage() {
  cat <<'USAGE'
usage: bash packaging/check.sh macos DIR | linux DIR | windows-exe FILE | windows DIR
USAGE
}

die() {
  echo "check.sh: error: $*" >&2
  exit 1
}

ok() {
  echo "check.sh: ok: $*"
}

here=$(cd "$(dirname "$0")" && pwd) || die "cannot find the script's folder"
root=$(cd "$here/.." && pwd) || die "cannot find the repository root"

# Working files, and a disk image this script mounts, are removed on exit.
tmp_root=$(mktemp -d "${TMPDIR:-/tmp}/tawara-check.XXXXXX") || die "cannot make a working folder"
dmg_mount=
cleanup() {
  if [ -n "$dmg_mount" ]; then
    hdiutil detach "$dmg_mount" >/dev/null 2>&1 || true
    rmdir "$dmg_mount" 2>/dev/null || true
  fi
  rm -rf "$tmp_root"
}
trap cleanup EXIT

# Sets version to the version [workspace.package] sets, which must be X.Y.Z.
version=
read_version() {
  version=$(awk '
    /^\[/ { in_package = ($0 == "[workspace.package]") }
    in_package && /^version[ \t]*=/ { split($0, parts, "\""); print parts[2]; exit }
  ' "$root/Cargo.toml") || die "cannot read the version in $root/Cargo.toml"
  printf '%s\n' "$version" | grep -E '^[0-9]+\.[0-9]+\.[0-9]+$' >/dev/null \
    || die "the version under [workspace.package] in Cargo.toml is not X.Y.Z: '$version'"
}

# Sets one to the single file in DIR whose name matches PATTERN. Dies when there
# is not exactly one.
one=
find_one() {
  local dir=$1 pattern=$2 f count=0
  one=
  # The pattern is meant to be a glob.
  # shellcheck disable=SC2086
  for f in "$dir"/$pattern; do
    if [ -f "$f" ]; then
      one=$f
      count=$((count + 1))
    fi
  done
  [ "$count" -eq 1 ] || die "expected one file matching $pattern in $dir, found $count"
}

# Dies unless DIR's SHA256SUMS lists NAME, and every file it lists checks.
check_sums() {
  local dir=$1 name=$2
  # sha256sum writes binary mode under Git Bash, as "hash *name", so the name may
  # carry a leading asterisk. The name is compared without it.
  awk -v n="$name" '{ f = $2; sub(/^[*]/, "", f); if (f == n) found = 1 } END { exit !found }' "$dir/SHA256SUMS" \
    || die "SHA256SUMS in $dir does not list $name"
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$dir" && sha256sum -c SHA256SUMS >/dev/null 2>&1) \
      || die "SHA256SUMS in $dir does not check"
  else
    (cd "$dir" && shasum -a 256 -c SHA256SUMS >/dev/null 2>&1) \
      || die "SHA256SUMS in $dir does not check"
  fi
}

# Dies unless the property list FILE holds VALUE under KEY.
expect_plist() {
  local file=$1 key=$2 value=$3 got
  got=$(plutil -extract "$key" raw -o - "$file") || die "Info.plist has no $key"
  [ "$got" = "$value" ] || die "Info.plist $key is '$got', not '$value'"
}

check_macos() {
  local dir=$1 app exe plist archs arch build notice dmg
  app=$dir/Tawara.app
  exe=$app/Contents/MacOS/tawara-desktop
  plist=$app/Contents/Info.plist
  [ -x "$exe" ] || die "no executable at $exe"

  archs=$(lipo -archs "$exe") || die "lipo cannot read $exe"
  case " $archs " in
    *" arm64 "*) ;;
    *) die "the executable has no arm64 slice: '$archs'" ;;
  esac
  case " $archs " in
    *" x86_64 "*) ;;
    *) die "the executable has no x86_64 slice: '$archs'" ;;
  esac
  ok "the executable holds the arm64 and the x86_64 slices"

  for arch in arm64 x86_64; do
    build=$(vtool -arch "$arch" -show-build "$exe") || die "vtool cannot read the $arch slice"
    printf '%s\n' "$build" | grep -E '^[[:space:]]*minos 11[.]0$' >/dev/null \
      || die "the $arch slice does not say minos 11.0 in vtool -show-build"
    ok "the $arch slice says minos 11.0"
  done

  plutil -lint "$plist" >/dev/null || die "Info.plist does not lint"
  expect_plist "$plist" CFBundleIdentifier com.adequatelimited.tawara
  expect_plist "$plist" CFBundleExecutable tawara-desktop
  expect_plist "$plist" CFBundlePackageType APPL
  expect_plist "$plist" LSMinimumSystemVersion 11.0
  expect_plist "$plist" CFBundleShortVersionString "$version"
  ok "Info.plist names the identifier, the executable, the minimum 11.0 and version $version"

  for notice in LICENSE.md THIRD-PARTY-NOTICES.txt; do
    cmp -s "$root/$notice" "$app/Contents/Resources/$notice" \
      || die "$notice in the app differs from the repository's copy"
  done
  ok "the app carries the repository's LICENSE.md and THIRD-PARTY-NOTICES.txt"

  find_one "$dir" "Tawara-${version}*.dmg"
  dmg=$one
  hdiutil verify "$dmg" >/dev/null 2>&1 || die "hdiutil does not verify $(basename "$dmg")"
  dmg_mount=$(mktemp -d "$tmp_root/mount.XXXXXX") || die "cannot make a mount point"
  hdiutil attach -readonly -nobrowse -noautoopen -noverify -mountpoint "$dmg_mount" "$dmg" \
    >/dev/null 2>&1 || die "hdiutil cannot attach $(basename "$dmg")"
  [ -x "$dmg_mount/Tawara.app/Contents/MacOS/tawara-desktop" ] \
    || die "the disk image holds no Tawara.app executable"
  [ -L "$dmg_mount/Applications" ] || die "the disk image holds no link to Applications"
  cmp -s "$exe" "$dmg_mount/Tawara.app/Contents/MacOS/tawara-desktop" \
    || die "the executable in the disk image differs from the one in $app"
  hdiutil detach "$dmg_mount" >/dev/null 2>&1 || die "hdiutil cannot detach $dmg_mount"
  rmdir "$dmg_mount" || die "cannot remove $dmg_mount"
  dmg_mount=
  ok "$(basename "$dmg") verifies, and holds the same Tawara.app"

  check_sums "$dir" "$(basename "$dmg")"
  ok "SHA256SUMS lists $(basename "$dmg") and checks"
}

# Dies unless a listing from dpkg-deb -c holds PATH (relative to the package's
# root) with MODE, a mode string as ls prints it.
has_entry() {
  printf '%s\n' "$1" | awk -v mode="$2" -v want="$3" '
    { path = $NF; sub(/^[.]\//, "", path) }
    $1 == mode && path == want { found = 1 }
    END { exit !found }
  '
}

# Dies unless the executable needs no GLIBC symbol version newer than the floor.
check_glibc_floor() {
  local exe=$1 versions highest top
  versions=$(objdump -T "$exe" | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/^GLIBC_//' | sort -uV || true)
  [ -n "$versions" ] || die "$exe names no GLIBC symbol version (objdump -T)"
  highest=$(printf '%s\n' "$versions" | tail -n 1)
  top=$(printf '%s\n%s\n' "$highest" "$glibc_floor" | sort -V | tail -n 1)
  [ "$top" = "$glibc_floor" ] \
    || die "$exe needs GLIBC_$highest, above the floor of GLIBC_$glibc_floor"
  ok "$(basename "$exe") needs GLIBC_$highest at most, the floor being GLIBC_$glibc_floor"
}

check_linux() {
  local dir=$1 deb appimage expect deb_version pkg arch maintainer depends recommends
  local contents tree appdir lib size doc
  find_one "$dir" "tawara-desktop_${version}*_amd64.deb"
  deb=$one
  find_one "$dir" "Tawara-${version}*-x86_64.AppImage"
  appimage=$one

  case "$(basename "$deb")" in
    "tawara-desktop_${version}~dev_amd64.deb") expect="${version}~dev" ;;
    "tawara-desktop_${version}_amd64.deb") expect=$version ;;
    *) die "$(basename "$deb") is not a name that linux.sh gives a .deb" ;;
  esac

  pkg=$(dpkg-deb -f "$deb" Package) || die "dpkg-deb cannot read $(basename "$deb")"
  arch=$(dpkg-deb -f "$deb" Architecture) || die "the .deb has no Architecture field"
  deb_version=$(dpkg-deb -f "$deb" Version) || die "the .deb has no Version field"
  maintainer=$(dpkg-deb -f "$deb" Maintainer) || die "the .deb has no Maintainer field"
  depends=$(dpkg-deb -f "$deb" Depends) || die "the .deb has no Depends field"
  recommends=$(dpkg-deb -f "$deb" Recommends) || die "the .deb has no Recommends field"

  [ "$pkg" = tawara-desktop ] || die "the .deb's Package is '$pkg', not tawara-desktop"
  [ "$arch" = amd64 ] || die "the .deb's Architecture is '$arch', not amd64"
  [ "$deb_version" = "$expect" ] || die "the .deb's Version is '$deb_version', not '$expect'"
  printf '%s\n' "$maintainer" | grep -E '^[^<>]+ <[^<> ]+@[^<> ]+>$' >/dev/null \
    || die "the .deb's Maintainer is not 'Name <address>': '$maintainer'"
  case "$depends" in
    *'libc6 (>= '*) ;;
    *) die "the .deb's Depends names no libc6 version: '$depends'" ;;
  esac
  # The libraries the app opens by name at run time.
  for lib in libx11-6 libx11-xcb1 libxcursor1 libxi6 libxkbcommon-x11-0 libwayland-client0 libxkbcommon0; do
    case ", $depends, " in
      *", $lib, "*) ;;
      *) die "the .deb's Depends does not name $lib, which the app opens at run time" ;;
    esac
  done
  [ "$recommends" = libvulkan1 ] || die "the .deb's Recommends is '$recommends', not libvulkan1"
  ok ".deb control fields: $pkg $deb_version, $arch, Recommends $recommends"

  contents=$(dpkg-deb -c "$deb") || die "dpkg-deb cannot list $(basename "$deb")"
  has_entry "$contents" -rwxr-xr-x usr/bin/tawara-desktop \
    || die "the .deb holds no executable usr/bin/tawara-desktop with mode 0755"
  has_entry "$contents" -rw-r--r-- usr/share/applications/com.adequatelimited.tawara.desktop \
    || die "the .deb holds no desktop entry under usr/share/applications"
  for doc in LICENSE.md THIRD-PARTY-NOTICES.txt; do
    has_entry "$contents" -rw-r--r-- "usr/share/doc/tawara-desktop/$doc" \
      || die "the .deb holds no usr/share/doc/tawara-desktop/$doc"
  done
  for size in 16 32 48 64 128 256 512; do
    has_entry "$contents" -rw-r--r-- \
      "usr/share/icons/hicolor/${size}x${size}/apps/com.adequatelimited.tawara.png" \
      || die "the .deb holds no $size-pixel icon"
  done
  ok ".deb contents: the executable, the desktop entry, the notices and seven icons"

  tree=$tmp_root/deb
  dpkg-deb -x "$deb" "$tree" || die "dpkg-deb cannot unpack $(basename "$deb")"
  check_glibc_floor "$tree/usr/bin/tawara-desktop"

  appdir=$tmp_root/appimage
  mkdir -p "$appdir" || die "cannot make $appdir"
  [ -x "$appimage" ] || die "$(basename "$appimage") is not executable"
  (cd "$appdir" && "$appimage" --appimage-extract >/dev/null 2>&1) \
    || die "$(basename "$appimage") does not extract"
  [ -x "$appdir/squashfs-root/AppRun" ] || die "the AppImage holds no executable AppRun"
  [ -x "$appdir/squashfs-root/usr/bin/tawara-desktop" ] \
    || die "the AppImage holds no executable usr/bin/tawara-desktop"
  [ -f "$appdir/squashfs-root/com.adequatelimited.tawara.desktop" ] \
    || die "the AppImage holds no desktop entry at its top"
  cmp -s "$tree/usr/bin/tawara-desktop" "$appdir/squashfs-root/usr/bin/tawara-desktop" \
    || die "the AppImage's executable differs from the .deb's"
  ok "$(basename "$appimage") holds AppRun, the desktop entry and the .deb's executable"

  check_sums "$dir" "$(basename "$deb")"
  check_sums "$dir" "$(basename "$appimage")"
  ok "SHA256SUMS lists both packages and checks"
}

# Prints one property of a file's version information, read by PowerShell.
windows_version_value() {
  powershell.exe -NoProfile -NonInteractive -Command \
    "(Get-Item -LiteralPath '$1').VersionInfo.$2" | tr -d '\r'
}

check_windows_exe() {
  local exe=$1 win pe machine file_version product_name product_version description
  [ -f "$exe" ] || die "no executable at $exe"
  [ "$(head -c 2 "$exe")" = MZ ] || die "$exe is not a Windows executable"
  pe=$(od -An -t u4 -j 60 -N 4 "$exe" | tr -d ' ') || die "cannot read the PE header offset of $exe"
  case "$pe" in
    ''|*[!0-9]*) die "$exe has no PE header offset" ;;
  esac
  [ "$(od -An -t x1 -j "$pe" -N 4 "$exe" | tr -d ' ')" = 50450000 ] \
    || die "$exe has no PE signature"
  machine=$(od -An -t x1 -j $((pe + 4)) -N 2 "$exe" | tr -d ' ') \
    || die "cannot read the machine type of $exe"
  [ "$machine" = 6486 ] || die "$exe is not an x86-64 executable (machine type $machine)"
  ok "$(basename "$exe") is an x86-64 Windows executable"

  if grep -a -i -q 'vcruntime140[^[:space:]]*[.]dll' "$exe"; then
    die "$exe names a Visual C++ runtime DLL: the static runtime flag did not apply"
  fi
  ok "$(basename "$exe") names no Visual C++ runtime DLL, so its C runtime is static"

  win=$exe
  if command -v cygpath >/dev/null 2>&1; then
    win=$(cygpath -w "$exe") || die "cannot convert $exe to a Windows path"
  fi
  file_version=$(windows_version_value "$win" FileVersion) || die "PowerShell cannot read $exe"
  product_name=$(windows_version_value "$win" ProductName) || die "PowerShell cannot read $exe"
  product_version=$(windows_version_value "$win" ProductVersion) || die "PowerShell cannot read $exe"
  description=$(windows_version_value "$win" FileDescription) || die "PowerShell cannot read $exe"
  [ "$file_version" = "$version.0" ] || die "FileVersion is '$file_version', not $version.0"
  [ "$product_name" = Tawara ] || die "ProductName is '$product_name', not Tawara"
  case "$product_version" in
    "$version"|"$version"-*) ;;
    *) die "ProductVersion is '$product_version', not $version or $version-dev" ;;
  esac
  [ "$description" = Tawara ] || die "FileDescription is '$description', not Tawara"
  ok "version resource: FileVersion $file_version, ProductName $product_name, ProductVersion $product_version"
}

check_windows() {
  local dir=$1 installer
  find_one "$dir" "Tawara-${version}*-x86_64-setup.exe"
  installer=$one
  [ "$(head -c 2 "$installer")" = MZ ] || die "$(basename "$installer") is not a Windows executable"
  check_sums "$dir" "$(basename "$installer")"
  ok "$(basename "$installer") is an installer, and SHA256SUMS lists it and checks"
}

[ $# -ge 1 ] || { usage >&2; die "expected a check: macos, linux, windows-exe or windows"; }
cmd=$1
case "$cmd" in
  -h|--help)
    usage
    exit 0
    ;;
  macos|linux|windows|windows-exe)
    ;;
  *)
    usage >&2
    die "unknown check: $cmd"
    ;;
esac
[ $# -eq 2 ] || { usage >&2; die "$cmd needs one path"; }
target=$2
case "$target" in
  /*|[A-Za-z]:*) ;;
  *) target=$PWD/$target ;;
esac

read_version
case "$cmd" in
  macos) check_macos "$target" ;;
  linux) check_linux "$target" ;;
  windows-exe) check_windows_exe "$target" ;;
  windows) check_windows "$target" ;;
esac
