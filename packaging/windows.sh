#!/bin/bash
# Copyright (c) 2026 Adequate Systems HK Limited
# Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.
# Makes the Windows installer for Tawara: Tawara-X.Y.Z-x86_64-setup.exe, a
# per-user NSIS installer for tawara-desktop.exe, with a SHA256SUMS file beside
# it. Run it in Git Bash on Windows, on the machine that holds the signing token.
# CI runs it with --dev, unsigned.
#
# The build uses NSIS 3.13 (its archive pinned by SHA-256), the static C
# runtime, a version resource made with rc.exe, and signtool for signing.
#
# Nothing here holds a key, certificate or password. The signing certificate is
# named by its thumbprint in the environment. A release refuses to run without
# it unless --unsigned is given, and a --dev build skips signing when it is
# unset, with a message.
#
# Usage: bash packaging/windows.sh [--dev | --unsigned] [--out DIR]
#        bash packaging/windows.sh --record-nsis
#
#   --dev           A local check build, not for distribution. The icon is
#                   optional, and the files are named as development builds.
#                   Signing still runs when TAWARA_WIN_CERT_SHA1 is set.
#   --unsigned      A release build that is not signed. Everything else a
#                   release requires still applies.
#   --out DIR       Where the installer is written. Default: target/package/windows
#                   under the repository root.
#   --record-nsis   Downloads the NSIS archive and prints its SHA-256, to
#                   record the pin. It makes no installer and runs on any
#                   system.
#
# The variables are listed in packaging/README.md.

set -eu
set -o pipefail

# Git Bash rewrites arguments that begin with a slash, such as /fd, into paths.
# The Windows tools need them unchanged, so the rewriting is switched off.
MSYS2_ARG_CONV_EXCL='*'
MSYS_NO_PATHCONV=1
export MSYS2_ARG_CONV_EXCL MSYS_NO_PATHCONV

# The NSIS pin. NSIS publishes no checksum or signature for its archive, so the
# SHA-256 is trusted on first use: run this script with --record-nsis on two
# networks, compare the hashes, and record the value in NSIS_SHA256. While it is
# empty, a release refuses to start and a --dev build stops before the
# installer, after its build and checks. SourceForge's published SHA-1 and MD5
# for the file match the recorded value.
NSIS_VERSION=3.13
NSIS_FILE=nsis-3.13.zip
NSIS_URL='https://prdownloads.sourceforge.net/nsis/nsis-3.13.zip?download'
NSIS_SHA256=ba63dffc4410ee89193e1cb5a41989991bd77c61068da17e3156d136b7b0b3d8

# The publisher name the installer shows unless overridden. SmartScreen shows
# the signing certificate's subject.
publisher_default="Adequate Systems HK Limited"

usage() {
  cat <<'USAGE'
usage: bash packaging/windows.sh [--dev | --unsigned] [--out DIR]
       bash packaging/windows.sh --record-nsis

  --dev           local check build: icon optional, not for distribution
  --unsigned      release build without code signing
  --out DIR       output folder (default: target/package/windows)
  --record-nsis   download NSIS and print the archive's SHA-256 (first-use record)

Environment: TAWARA_WIN_ICO, TAWARA_WIN_CERT_SHA1, TAWARA_WIN_TIMESTAMP_URL,
TAWARA_WIN_PUBLISHER, TAWARA_WINSDK_BIN, CARGO_TARGET_DIR.
packaging/README.md says what each one does.
USAGE
}

die() {
  echo "windows.sh: error: $*" >&2
  exit 1
}

warn() {
  echo "windows.sh: warning: $*" >&2
}

step() {
  echo "windows.sh: $*"
}

# Under Git Bash, cygpath converts paths between the shell's form and Windows'.
# Elsewhere the path is used as it is, made absolute where that is needed.
to_posix() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -a -u "$1"
  else
    case "$1" in
      /*) printf '%s\n' "$1" ;;
      *) printf '%s/%s\n' "$PWD" "$1" ;;
    esac
  fi
}

to_windows() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$1"
  else
    printf '%s\n' "$1"
  fi
}

# C:/ form, which cargo's rustflags take without escaping.
to_mixed() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -m "$1"
  else
    printf '%s\n' "$1"
  fi
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

# Downloads the NSIS archive to the path given. HTTPS only, from the address
# above.
download_nsis() {
  curl --fail --location --proto '=https' --proto-redir '=https' --tlsv1.2 --silent --show-error \
    -o "$(to_windows "$1")" "$NSIS_URL" || die "the NSIS download failed: $NSIS_URL"
}

# Prints the x64 folder of the newest Windows SDK 10 that holds both rc.exe and
# signtool.exe, as a POSIX path. Prints nothing when there is none.
find_sdk_bin() {
  local pf86 kits listing candidate
  pf86=$(printenv 'ProgramFiles(x86)' 2>/dev/null || true)
  if [ -z "$pf86" ]; then
    pf86='C:\Program Files (x86)'
  fi
  kits="$(to_posix "$pf86")/Windows Kits/10/bin"
  listing=$(find "$kits" -mindepth 1 -maxdepth 1 -type d -name '10.*' 2>/dev/null | sort -V -r || true)
  printf '%s\n' "$listing" | while IFS= read -r candidate; do
    if [ -f "$candidate/x64/rc.exe" ] && [ -f "$candidate/x64/signtool.exe" ]; then
      printf '%s\n' "$candidate/x64"
      break
    fi
  done
  return 0
}

# Signs one file with the certificate named by TAWARA_WIN_CERT_SHA1, timestamped
# at TAWARA_WIN_TIMESTAMP_URL, and checks the signature.
sign_file() {
  local win
  win=$(to_windows "$1")
  if ! "$signtool_exe" sign /sha1 "$cert" /fd sha256 /tr "$ts_url" /td sha256 "$win"; then
    rm -f "$1"
    die "signtool could not sign $1"
  fi
  if ! "$signtool_exe" verify /pa /v "$win"; then
    rm -f "$1"
    die "the signature on $1 does not verify"
  fi
}

# Makes sure the NSIS archive is in the tools folder with the recorded hash, and
# unpacks it. Sets makensis_exe to the program's POSIX path.
fetch_nsis() {
  local zip nsis_dir zip_win dest_win got version_text
  if [ -z "$NSIS_SHA256" ]; then
    die "the NSIS $NSIS_VERSION archive has no recorded SHA-256 yet; the hash is trusted on first use. Run: bash packaging/windows.sh --record-nsis, on one network and again on a second. When the two hashes agree, record the value in NSIS_SHA256."
  fi
  mkdir -p "$tools_dir" || die "cannot make $tools_dir"
  zip=$tools_dir/$NSIS_FILE
  if [ -f "$zip" ] && [ "$(sha256_of "$zip")" = "$NSIS_SHA256" ]; then
    step "NSIS $NSIS_VERSION: the archive in $tools_dir has the recorded hash"
  else
    rm -f "$zip"
    step "downloading NSIS $NSIS_VERSION from $NSIS_URL"
    download_nsis "$zip"
    got=$(sha256_of "$zip") || die "cannot read the SHA-256 of $zip"
    if [ "$got" != "$NSIS_SHA256" ]; then
      rm -f "$zip"
      die "the NSIS archive's SHA-256 is $got, not the recorded $NSIS_SHA256; the download was removed"
    fi
  fi
  nsis_dir=$tools_dir/nsis-$NSIS_VERSION
  rm -rf "$nsis_dir"
  mkdir -p "$nsis_dir" || die "cannot make $nsis_dir"
  zip_win=$(to_windows "$zip")
  dest_win=$(to_windows "$nsis_dir")
  case "$zip_win$dest_win" in
    *"'"*|*'"'*|*'$'*) die "the NSIS folders may not hold a quote or a dollar sign: $zip_win" ;;
  esac
  command -v powershell.exe >/dev/null 2>&1 || die "powershell.exe is not on PATH; it unpacks the NSIS archive"
  powershell.exe -NoProfile -NonInteractive -Command \
    "Expand-Archive -LiteralPath '$zip_win' -DestinationPath '$dest_win' -Force" \
    || die "could not unpack $NSIS_FILE"
  makensis_exe=$(find "$nsis_dir" -type f -name makensis.exe 2>/dev/null \
    | awk '{ print length($0), $0 }' | sort -n | sed -n '1p' | cut -d' ' -f2-)
  [ -n "$makensis_exe" ] || die "the NSIS archive holds no makensis.exe"
  version_text=$("$makensis_exe" -VERSION 2>/dev/null | tr -d '\r' | sed -n '1p' || true)
  case "$version_text" in
    *"$NSIS_VERSION"*) ;;
    *) die "makensis reports '$version_text', not NSIS $NSIS_VERSION" ;;
  esac
}

# Writes tawara.rc in the work folder: the version resource, and the icon when
# there is one. Its values come from the variables set in the main part.
write_rc() {
  {
    if [ -n "$icon" ]; then
      printf '%s\n' '1 ICON "tawara.ico"'
    fi
    printf '%s\n' \
      '1 VERSIONINFO' \
      "FILEVERSION $rc_version" \
      "PRODUCTVERSION $rc_version" \
      'FILEFLAGSMASK 0x3fL' \
      'FILEFLAGS 0x0L' \
      'FILEOS 0x40004L' \
      'FILETYPE 0x1L' \
      'FILESUBTYPE 0x0L' \
      'BEGIN' \
      '  BLOCK "StringFileInfo"' \
      '  BEGIN' \
      '    BLOCK "040904b0"' \
      '    BEGIN' \
      '      VALUE "FileDescription", "Tawara"' \
      "      VALUE \"FileVersion\", \"$file_version\"" \
      '      VALUE "InternalName", "tawara-desktop"' \
      '      VALUE "OriginalFilename", "tawara-desktop.exe"' \
      '      VALUE "ProductName", "Tawara"' \
      "      VALUE \"ProductVersion\", \"$display_version\"" \
      '    END' \
      '  END' \
      '  BLOCK "VarFileInfo"' \
      '  BEGIN' \
      '    VALUE "Translation", 0x409, 1200' \
      '  END' \
      'END'
  } > "$work/tawara.rc" || die "cannot write $work/tawara.rc"
}

dev=false
unsigned=false
record=false
out=
icon=${TAWARA_WIN_ICO:-}
cert=${TAWARA_WIN_CERT_SHA1:-}
ts_url=${TAWARA_WIN_TIMESTAMP_URL:-}
publisher=${TAWARA_WIN_PUBLISHER:-$publisher_default}
sdk_bin=${TAWARA_WINSDK_BIN:-}
makensis_exe=
signtool_exe=
rc_exe=

while [ $# -gt 0 ]; do
  case "$1" in
    --dev)
      dev=true
      ;;
    --unsigned)
      unsigned=true
      ;;
    --record-nsis)
      record=true
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

here=$(cd "$(dirname "$0")" && pwd) || die "cannot find the script's folder"
root=$(cd "$here/.." && pwd) || die "cannot find the repository root"

# Paths given relative to the caller's folder are made absolute before the
# script moves to the repository root.
if [ -n "$out" ]; then
  out=$(to_posix "$out")
fi
if [ -n "$icon" ]; then
  icon=$(to_posix "$icon")
fi
if [ -n "$sdk_bin" ]; then
  sdk_bin=$(to_posix "$sdk_bin")
fi

cd "$root"

# CARGO_TARGET_DIR is read as cargo reads it: a relative path is relative to the
# repository root.
target_dir=$root/target
if [ -n "${CARGO_TARGET_DIR:-}" ]; then
  case "$CARGO_TARGET_DIR" in
    /*|[A-Za-z]:*) target_dir=$(to_posix "$CARGO_TARGET_DIR") ;;
    *) target_dir=$root/$CARGO_TARGET_DIR ;;
  esac
fi
tools_dir=$target_dir/package/tools

if $record; then
  mkdir -p "$tools_dir" || die "cannot make $tools_dir"
  zip=$tools_dir/$NSIS_FILE
  rm -f "$zip"
  step "downloading NSIS $NSIS_VERSION from $NSIS_URL"
  download_nsis "$zip"
  got=$(sha256_of "$zip") || die "cannot read the SHA-256 of $zip"
  printf '%s  %s\n' "$got" "$NSIS_FILE"
  step "compare this with a second network's download before recording it in NSIS_SHA256"
  exit 0
fi

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) ;;
  *) die "this script makes the Windows installer and runs under Git Bash on Windows (--record-nsis runs anywhere)" ;;
esac

for tool in awk cargo curl cut find head od rustup sed sort tr; do
  command -v "$tool" >/dev/null 2>&1 || die "$tool is not on PATH"
done
command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 \
  || die "neither sha256sum nor shasum is on PATH"

# The version is the one [workspace.package] in Cargo.toml sets, which
# crates/desktop inherits.
version=$(awk '
  /^\[/ { in_package = ($0 == "[workspace.package]") }
  in_package && /^version[ \t]*=/ { split($0, parts, "\""); print parts[2]; exit }
' Cargo.toml)
printf '%s\n' "$version" | grep -E '^[0-9]+\.[0-9]+\.[0-9]+$' >/dev/null \
  || die "the version under [workspace.package] in Cargo.toml is not X.Y.Z: '$version'"

if $dev; then
  display_version="$version-dev"
  installer_name="Tawara-$version-dev-x86_64-setup.exe"
else
  display_version=$version
  installer_name="Tawara-$version-x86_64-setup.exe"
fi
# The numeric version the installer and the executable carry. The desktop has no
# build number, so the fourth field is 0.
file_version="$version.0"
rc_version="$(printf '%s' "$version" | tr '.' ','),0"

case "$publisher" in
  *'"'*|*'$'*|*'`'*) die "TAWARA_WIN_PUBLISHER may not hold a double quote, a dollar sign or a backquote" ;;
esac

if [ -n "$icon" ]; then
  [ -f "$icon" ] || die "TAWARA_WIN_ICO names no file: $icon"
  [ "$(head -c 4 "$icon" | od -An -tx1 | tr -d ' \n')" = 00000100 ] \
    || die "TAWARA_WIN_ICO is not an .ico file: $icon"
elif $dev; then
  warn "TAWARA_WIN_ICO is unset, so this development build has no icon"
else
  die "TAWARA_WIN_ICO is unset. A release needs the app icon: pass the path to an .ico file. Use --dev for a local build without one."
fi

for notice in LICENSE.md THIRD-PARTY-NOTICES.txt; do
  [ -f "$root/$notice" ] || die "$notice is missing at the repository root; a release ships it"
done

# The signing settings are checked before the build so a bad value fails early.
# The thumbprint and the timestamp address are written into the NSIS command
# line, so they are checked strictly.
will_sign=no
if $unsigned; then
  [ -z "$cert" ] || die "TAWARA_WIN_CERT_SHA1 is set and --unsigned was given; choose one"
  warn "--unsigned: the executable and the installer are not signed"
elif [ -n "$cert" ]; then
  printf '%s' "$cert" | grep -E '^[0-9A-Fa-f]{40}$' >/dev/null \
    || die "TAWARA_WIN_CERT_SHA1 is not a 40-digit SHA-1 thumbprint"
  [ -n "$ts_url" ] || die "TAWARA_WIN_TIMESTAMP_URL is unset: signing needs the certificate authority's RFC 3161 timestamp address"
  printf '%s' "$ts_url" | grep -E '^https?://[A-Za-z0-9._~:/?=+,@-]+$' >/dev/null \
    || die "TAWARA_WIN_TIMESTAMP_URL is not a plain http or https address: $ts_url"
  will_sign=yes
elif $dev; then
  warn "TAWARA_WIN_CERT_SHA1 is unset: the executable and the installer are not signed"
else
  die "TAWARA_WIN_CERT_SHA1 is unset. A release is signed with the code-signing certificate's SHA-1 thumbprint. Use --unsigned for an unsigned release, or --dev for a local build."
fi

installed=$(rustup target list --installed) || die "rustup cannot list the installed Rust targets"
printf '%s\n' "$installed" | grep -x -F x86_64-pc-windows-msvc >/dev/null \
  || die "the Rust target x86_64-pc-windows-msvc is not installed; run: rustup target add x86_64-pc-windows-msvc"

# RUSTFLAGS in the environment takes precedence over the configuration this
# script passes, which would drop the static runtime flag and the resource.
if [ -n "${RUSTFLAGS:-}${CARGO_ENCODED_RUSTFLAGS:-}${CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS:-}" ]; then
  if $dev; then
    warn "RUSTFLAGS is set in the environment and overrides the static runtime flag; a release refuses this"
  else
    die "RUSTFLAGS, CARGO_ENCODED_RUSTFLAGS or CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS is set. Cargo would ignore the static runtime flag and the resource, so unset it for a release."
  fi
fi

if [ -z "$sdk_bin" ]; then
  sdk_bin=$(find_sdk_bin)
fi
if [ -n "$sdk_bin" ]; then
  if [ -f "$sdk_bin/rc.exe" ]; then
    rc_exe=$sdk_bin/rc.exe
  fi
  if [ -f "$sdk_bin/signtool.exe" ]; then
    signtool_exe=$sdk_bin/signtool.exe
  fi
fi
if [ -z "$rc_exe" ]; then
  if $dev; then
    warn "rc.exe was not found in the Windows SDK: this development build has no version resource"
  else
    die "rc.exe was not found in the Windows SDK. A release needs it for the executable's icon and version information. Set TAWARA_WINSDK_BIN to the SDK's x64 folder."
  fi
fi
if [ "$will_sign" = yes ] && [ -z "$signtool_exe" ]; then
  die "signtool.exe was not found in the Windows SDK, and TAWARA_WIN_CERT_SHA1 is set. Set TAWARA_WINSDK_BIN to the SDK's x64 folder."
fi

# A release fetches the NSIS archive before its build, so a missing hash stops
# it early. A development build fetches it after its build and checks, so the
# executable can be checked where the hash is not yet recorded.
if ! $dev; then
  fetch_nsis
fi

if [ -z "$out" ]; then
  out=$target_dir/package/windows
fi
mkdir -p "$out" || die "cannot make $out"
rm -f "$out/$installer_name" "$out/SHA256SUMS"

work=$target_dir/package/windows-work
stage=$target_dir/package/windows-stage
rm -rf "$work" "$stage"
mkdir -p "$work" "$stage" || die "cannot make the working folders under $target_dir/package"

# The version resource. Without rc.exe (a development build only) the executable
# has none, and the flag that links it is left out.
resource=no
if [ -n "$rc_exe" ]; then
  if [ -n "$icon" ]; then
    cp "$icon" "$work/tawara.ico" || die "cannot copy the icon into $work"
  fi
  write_rc
  (cd "$work" && "$rc_exe" /nologo /fo tawara.res tawara.rc) \
    || die "rc.exe could not compile the version resource"
  resource=yes
fi
res_mixed=
if [ "$resource" = yes ]; then
  res_mixed=$(to_mixed "$work/tawara.res")
  case "$res_mixed" in
    *'"'*|*"'"*) die "the resource's path may not hold a quote: $res_mixed" ;;
  esac
fi

# The static C runtime, given on the command line. The resource's link flag
# joins the same list.
rustflags='"-C", "target-feature=+crt-static"'
if [ "$resource" = yes ]; then
  rustflags="$rustflags, \"-C\", \"link-arg=$res_mixed\""
fi
config="target.x86_64-pc-windows-msvc.rustflags=[$rustflags]"

step "cargo build --release --locked for x86_64-pc-windows-msvc"
cargo --config "$config" build --release --locked -p tawara-desktop --target x86_64-pc-windows-msvc \
  || die "cargo build failed"

bin=$target_dir/x86_64-pc-windows-msvc/release/tawara-desktop.exe
[ -f "$bin" ] || die "no executable at $bin"
[ "$(head -c 2 "$bin")" = MZ ] || die "$bin is not a Windows executable"
cp "$bin" "$stage/tawara-desktop.exe" || die "cannot copy the executable into $stage"
cp "$root/LICENSE.md" "$root/THIRD-PARTY-NOTICES.txt" "$stage/" \
  || die "cannot copy the notices into $stage"

# A development build gets the NSIS archive here, before the installer is made.
if $dev; then
  fetch_nsis
fi

signed=no
if [ "$will_sign" = yes ]; then
  step "signing tawara-desktop.exe"
  sign_file "$stage/tawara-desktop.exe"
fi

installer=$out/$installer_name
stage_w=$(to_windows "$stage")
installer_w=$(to_windows "$installer")
for value in "$stage_w" "$installer_w" "$display_version" "$file_version" "$publisher"; do
  case "$value" in
    *'"'*|*'$'*|*'`'*) die "a value for NSIS holds a quote, a dollar sign or a backquote: $value" ;;
  esac
done
nsis_args=(-V2 -WX "-DTAWARA_VERSION=$display_version" "-DTAWARA_FILE_VERSION=$file_version" "-DTAWARA_STAGE=$stage_w" "-DTAWARA_OUT=$installer_w" "-DTAWARA_PUBLISHER=$publisher")
if [ -n "$icon" ]; then
  nsis_args+=("-DTAWARA_ICO=$(to_windows "$icon")")
fi
uninst_stamp=$installer.uninstaller-signed
if [ "$will_sign" = yes ]; then
  nsis_args+=("-DTAWARA_WIN_CERT_SHA1=$cert" "-DTAWARA_TIMESTAMP_URL=$ts_url" "-DTAWARA_UNINST_STAMP=$(to_windows "$uninst_stamp")")
fi
nsis_script=$root/packaging/windows/tawara.nsi
[ -f "$nsis_script" ] || die "$nsis_script is missing"

# makensis does not fail when the command that signs the uninstaller fails, so
# the command writes a stamp file on success, and the stamp is checked below.
rm -f "$uninst_stamp"
step "making $installer_name with NSIS $NSIS_VERSION"
if ! (
  if [ -n "$sdk_bin" ]; then
    PATH="$sdk_bin:$PATH"
    export PATH
  fi
  "$makensis_exe" "${nsis_args[@]}" "$(to_windows "$nsis_script")"
); then
  rm -f "$installer" "$uninst_stamp"
  die "makensis could not make $installer_name"
fi
[ -f "$installer" ] || die "makensis made no $installer_name"
if [ "$will_sign" = yes ]; then
  if [ ! -f "$uninst_stamp" ]; then
    rm -f "$installer"
    die "the uninstaller was not signed: makensis finished, but signtool did not finish its command (see the output above)"
  fi
  rm -f "$uninst_stamp"
  step "the uninstaller is signed"
  step "signing $installer_name"
  sign_file "$installer"
  signed=yes
fi

if command -v sha256sum >/dev/null 2>&1; then
  ( cd "$out" && sha256sum "$installer_name" > SHA256SUMS && sha256sum -c SHA256SUMS >/dev/null ) \
    || die "could not write and check SHA256SUMS"
else
  ( cd "$out" && shasum -a 256 "$installer_name" > SHA256SUMS && shasum -a 256 -c SHA256SUMS >/dev/null ) \
    || die "could not write and check SHA256SUMS"
fi

rm -rf "$work" "$stage"

step "installer: $installer"
step "signed: $signed"
cat "$out/SHA256SUMS"
if $dev; then
  step "this is a development build, not for distribution"
elif [ "$signed" = no ]; then
  warn "this release is not signed, and an unsigned package must not be published"
fi
