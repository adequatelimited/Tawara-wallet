#!/bin/bash
# Copyright (c) 2026 Adequate Systems HK Limited
# Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.
# Makes the macOS packages for Tawara: Tawara.app, one universal binary for
# Apple silicon and Intel Macs, and Tawara-X.Y.Z.dmg, the disk image that holds
# it, with a SHA256SUMS file beside the image.
#
# The build uses Xcode 26.6, macOS 11.0 as the minimum, the hardened runtime
# with no entitlements, notarisation with notarytool, and hdiutil for the image.
#
# Nothing here holds a key, certificate or password. Identities come from the
# environment. A release refuses to run without its identity and notary profile,
# and a --dev build skips a step whose variable is unset, with a message. Run a
# release on the Mac that holds the keys. --unsigned makes a release without a
# Developer ID: the app is ad-hoc signed and not notarised.
#
# Usage: bash packaging/macos.sh [--dev | --unsigned] [--build-number N] [--out DIR]
#
#   --dev             A local check build, not for distribution. The icon is
#                     optional, notarisation is skipped, and the image is named
#                     as a development build. Signing still runs when
#                     TAWARA_MAC_SIGN_ID is set, so the signing steps can be
#                     checked before a release.
#   --unsigned        A release without a Developer ID: ad-hoc signed, not
#                     notarised. Everything else a release requires applies.
#   --build-number N  The bundle's CFBundleVersion. Required for a release;
#                     --dev uses 1. The desktop has no build number of its
#                     own, but macOS needs one in the bundle.
#   --out DIR         Where the packages are written. Default:
#                     target/package/macos under the repository root.
#
# The variables are listed in packaging/README.md.

set -eu
set -o pipefail

usage() {
  cat <<'USAGE'
usage: bash packaging/macos.sh [--dev | --unsigned] [--build-number N] [--out DIR]

  --dev             local check build: icon optional, no notarisation
  --unsigned        release build, ad-hoc signed, not notarised
  --build-number N  CFBundleVersion (required for a release; --dev uses 1)
  --out DIR         output folder (default: target/package/macos)

Environment: DEVELOPER_DIR, TAWARA_ICON, TAWARA_BUILD_NUMBER, TAWARA_MAC_SIGN_ID,
TAWARA_TEAM_ID, TAWARA_NOTARY_PROFILE, CARGO_TARGET_DIR. packaging/README.md
says what each one does.
USAGE
}

die() {
  echo "macos.sh: error: $*" >&2
  exit 1
}

warn() {
  echo "macos.sh: warning: $*" >&2
}

step() {
  echo "macos.sh: $*"
}

dev=false
unsigned=false
build_number=${TAWARA_BUILD_NUMBER:-}
out=
icon=${TAWARA_ICON:-}

while [ $# -gt 0 ]; do
  case "$1" in
    --dev)
      dev=true
      ;;
    --unsigned)
      unsigned=true
      ;;
    --build-number)
      [ $# -ge 2 ] || die "--build-number needs a value"
      build_number=$2
      shift
      ;;
    --build-number=*)
      build_number=${1#--build-number=}
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

if $dev && $unsigned; then
  die "--dev and --unsigned cannot be combined"
fi

[ "$(uname -s)" = Darwin ] || die "this script makes the macOS packages and runs only on macOS"

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

here=$(cd "$(dirname "$0")" && pwd) || die "cannot find the script's folder"
root=$(cd "$here/.." && pwd) || die "cannot find the repository root"
cd "$root"

for tool in awk cargo codesign ditto hdiutil head lipo plutil rustup shasum spctl vtool xcrun; do
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

if [ -z "$build_number" ]; then
  if $dev; then
    build_number=1
  else
    die "a release needs a build number: pass --build-number N or set TAWARA_BUILD_NUMBER"
  fi
fi
case "$build_number" in
  ''|*[!0-9]*) die "the build number is not a whole number: '$build_number'" ;;
esac

# The minimum is also the deployment target the binaries are built for, so the
# two cannot disagree.
min_os=11.0

if [ -n "$icon" ]; then
  [ -f "$icon" ] || die "TAWARA_ICON names no file: $icon"
  [ "$(head -c 4 "$icon")" = icns ] || die "TAWARA_ICON is not an .icns file: $icon"
elif $dev; then
  warn "TAWARA_ICON is unset, so this development build has no app icon"
else
  die "TAWARA_ICON is unset. A release needs the app icon: pass the path to an .icns file. Use --dev for a local build without one."
fi

for notice in LICENSE.md THIRD-PARTY-NOTICES.txt; do
  [ -f "$root/$notice" ] || die "$notice is missing at the repository root; a release ships it"
done

# A release is signed with a Developer ID identity and notarised. Both are
# checked before the build, so a release without them stops at once. A --dev
# build may leave them unset, or make an ad-hoc signature with '-' for a local
# check.
sign_id=${TAWARA_MAC_SIGN_ID:-}
notary_profile=${TAWARA_NOTARY_PROFILE:-}
if $unsigned; then
  [ -z "$sign_id" ] || [ "$sign_id" = - ] || die "TAWARA_MAC_SIGN_ID is set and --unsigned was given; choose one"
  sign_id=-
  warn "--unsigned: Tawara.app is ad-hoc signed and the disk image is not notarised"
elif ! $dev; then
  case "$sign_id" in
    "") die "TAWARA_MAC_SIGN_ID is unset. A release is signed with a Developer ID Application identity. Use --dev for an unsigned local build." ;;
    -) die "TAWARA_MAC_SIGN_ID is '-', an ad-hoc signature that is not for distribution. A release is signed with a Developer ID Application identity." ;;
  esac
  [ -n "$notary_profile" ] || die "TAWARA_NOTARY_PROFILE is unset. A release is notarised with a notarytool keychain profile. Use --dev for a local build without one."
fi

# Xcode 26.6 may be installed beside a newer Xcode; it is selected with
# DEVELOPER_DIR, leaving the Mac's default Xcode unchanged.
if [ -z "${DEVELOPER_DIR:-}" ] && [ -d /Applications/Xcode_26.6.app/Contents/Developer ]; then
  DEVELOPER_DIR=/Applications/Xcode_26.6.app/Contents/Developer
  export DEVELOPER_DIR
fi
sdk=$(xcrun --sdk macosx --show-sdk-version 2>/dev/null) || sdk=
case "$sdk" in
  26.*)
    step "building against the macOS $sdk SDK"
    ;;
  *)
    if $dev; then
      warn "the macOS SDK is '${sdk:-missing}', not 26.x; a release is built with Xcode 26.6"
    else
      die "a release is built with Xcode 26.6, whose macOS SDK is 26.x; this one is '${sdk:-missing}'. Set DEVELOPER_DIR=/Applications/Xcode_26.6.app/Contents/Developer"
    fi
    ;;
esac

targets="aarch64-apple-darwin x86_64-apple-darwin"
installed=$(rustup target list --installed) || die "rustup cannot list the installed Rust targets"
for target in $targets; do
  printf '%s\n' "$installed" | grep -x -F "$target" >/dev/null \
    || die "the Rust target $target is not installed; run: rustup target add $target"
done

target_dir=${CARGO_TARGET_DIR:-$root/target}
case "$target_dir" in
  /*) ;;
  *) target_dir=$root/$target_dir ;;
esac
if [ -z "$out" ]; then
  out=$target_dir/package/macos
fi
mkdir -p "$out"

for target in $targets; do
  step "cargo build --release --locked for $target, MACOSX_DEPLOYMENT_TARGET=$min_os"
  MACOSX_DEPLOYMENT_TARGET=$min_os cargo build --release --locked -p tawara-desktop --target "$target" \
    || die "cargo build failed for $target"
done

arm_bin=$target_dir/aarch64-apple-darwin/release/tawara-desktop
x86_bin=$target_dir/x86_64-apple-darwin/release/tawara-desktop
for bin in "$arm_bin" "$x86_bin"; do
  [ -x "$bin" ] || die "no binary at $bin"
  build_info=$(vtool -show-build "$bin") || die "vtool cannot read $bin"
  printf '%s\n' "$build_info" | grep -E "^[[:space:]]*minos $min_os\$" >/dev/null \
    || die "$bin does not say minos $min_os in vtool -show-build"
done

app=$out/Tawara.app
exe=$app/Contents/MacOS/tawara-desktop
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

step "joining the two halves into one universal binary"
lipo -create -output "$exe" "$arm_bin" "$x86_bin" || die "lipo failed"
archs=$(lipo -archs "$exe") || die "lipo cannot read the universal binary"
case " $archs " in
  *" arm64 "*) ;;
  *) die "the universal binary has no arm64 slice: '$archs'" ;;
esac
case " $archs " in
  *" x86_64 "*) ;;
  *) die "the universal binary has no x86_64 slice: '$archs'" ;;
esac
lipo -info "$exe"

plist=$app/Contents/Info.plist
cp "$root/packaging/macos/Info.plist" "$plist"
plutil -replace CFBundleShortVersionString -string "$version" "$plist"
plutil -replace CFBundleVersion -string "$build_number" "$plist"
plutil -replace LSMinimumSystemVersion -string "$min_os" "$plist"
if [ -n "$icon" ]; then
  plutil -replace CFBundleIconFile -string AppIcon "$plist"
fi
plutil -lint "$plist" >/dev/null || die "Info.plist does not lint"
[ "$(plutil -extract CFBundleShortVersionString raw -o - "$plist")" = "$version" ] \
  || die "Info.plist does not carry the version $version"
[ "$(plutil -extract CFBundleVersion raw -o - "$plist")" = "$build_number" ] \
  || die "Info.plist does not carry the build number $build_number"
printf 'APPL????' > "$app/Contents/PkgInfo"

cp "$root/LICENSE.md" "$root/THIRD-PARTY-NOTICES.txt" "$app/Contents/Resources/"
if [ -n "$icon" ]; then
  cp "$icon" "$app/Contents/Resources/AppIcon.icns"
fi

entitlements=$root/packaging/macos/entitlements.plist
signed=no
if [ -n "$sign_id" ]; then
  if [ "$sign_id" = - ]; then
    step "ad-hoc signing Tawara.app: no Developer ID, so Gatekeeper asks before the first start"
    codesign --force --options runtime --entitlements "$entitlements" --sign - "$app" \
      || die "codesign failed for Tawara.app"
    signed=ad-hoc
  else
    step "signing Tawara.app with the hardened runtime and a secure timestamp, no entitlements"
    codesign --force --options runtime --timestamp --entitlements "$entitlements" \
      --sign "$sign_id" "$app" || die "codesign failed for Tawara.app"
    signed=yes
  fi
  codesign --verify --deep --strict "$app" || die "the signature on Tawara.app does not verify"
  details=$(codesign -dv --verbose=4 "$app" 2>&1) || die "codesign cannot read the signature on Tawara.app"
  if [ -n "${TAWARA_TEAM_ID:-}" ]; then
    case "$details" in
      *"TeamIdentifier=$TAWARA_TEAM_ID"*) ;;
      *) die "the signature on Tawara.app does not name the team in TAWARA_TEAM_ID" ;;
    esac
  fi
else
  warn "Tawara.app is not signed: TAWARA_MAC_SIGN_ID is unset"
fi

if $dev; then
  dmg_name=Tawara-$version-dev.dmg
else
  dmg_name=Tawara-$version.dmg
fi
dmg=$out/$dmg_name
staging=$out/dmg-staging
rm -rf "$staging" "$dmg"
mkdir -p "$staging"
ditto "$app" "$staging/Tawara.app"
ln -s /Applications "$staging/Applications"
step "making $dmg_name"
hdiutil create -quiet -ov -volname "Tawara $version" -srcfolder "$staging" \
  -format UDZO -imagekey zlib-level=9 "$dmg" || die "hdiutil could not make $dmg_name"
rm -rf "$staging"

if [ -n "$sign_id" ]; then
  if [ "$sign_id" = - ]; then
    codesign --force --options runtime --sign - "$dmg" || die "codesign failed for $dmg_name"
  else
    codesign --force --options runtime --timestamp --sign "$sign_id" "$dmg" \
      || die "codesign failed for $dmg_name"
  fi
  codesign --verify --verbose=2 "$dmg" || die "the signature on $dmg_name does not verify"
fi

notarised=no
if [ -z "$sign_id" ] || [ "$sign_id" = - ]; then
  step "$dmg_name is not notarised: notarisation needs a Developer ID signature"
elif $dev; then
  step "$dmg_name is not notarised: notarisation is skipped in a --dev build"
else
  # A release has a Developer ID identity and a notary profile: both were checked
  # before the build.
  step "notarising $dmg_name with the keychain profile in TAWARA_NOTARY_PROFILE"
  log=$out/notarytool.log
  xcrun notarytool submit "$dmg" --keychain-profile "$notary_profile" --wait >"$log" 2>&1 || true
  cat "$log"
  if ! grep -E 'status: Accepted' "$log" >/dev/null; then
    notary_id=$(sed -n 's/^[[:space:]]*id: //p' "$log" | head -n 1)
    die "notarisation of $dmg_name did not succeed. For the reason: xcrun notarytool log ${notary_id:-<id>} --keychain-profile <profile>"
  fi
  xcrun stapler staple "$dmg" || die "stapling $dmg_name failed"
  xcrun stapler validate "$dmg" || die "the stapled ticket on $dmg_name does not validate"
  assessment=$(spctl -a -vvv -t open --context context:primary-signature "$dmg" 2>&1) || true
  case "$assessment" in
    *accepted*"source=Notarized Developer ID"*) ;;
    *) die "Gatekeeper does not accept $dmg_name: $assessment" ;;
  esac
  notarised=yes
fi

( cd "$out" && shasum -a 256 "$dmg_name" > SHA256SUMS && shasum -a 256 -c SHA256SUMS >/dev/null ) \
  || die "could not write and check SHA256SUMS"

step "Tawara.app: $app"
step "$dmg_name: $dmg"
step "signed: $signed; notarised: $notarised"
cat "$out/SHA256SUMS"
if $dev; then
  step "this is a development build, not for distribution"
fi
