# Packaging

The scripts that build Tawara's desktop packages, and what each one needs.

## Layout

| Path | What it is |
|---|---|
| `packaging/macos.sh` | the macOS packages: a universal `Tawara.app`, `Tawara-X.Y.Z.dmg`, `SHA256SUMS` |
| `packaging/macos/Info.plist` | the template for the bundle's `Info.plist`; the script sets its version, build number and minimum system version |
| `packaging/macos/entitlements.plist` | the hardened runtime's entitlements: an empty dictionary, so no exception is granted |
| `packaging/windows.sh` | the Windows installer: `tawara-desktop.exe` for x86_64 with the static C runtime and a version resource, and `Tawara-X.Y.Z-x86_64-setup.exe`, a per-user NSIS installer, with `SHA256SUMS`. Signed when a certificate is configured |
| `packaging/windows/tawara.nsi` | the NSIS script the installer is made from |
| `packaging/linux.sh` | the AppImage and the `.deb` for x86_64, built in a pinned `ubuntu:22.04` container, with `SHA256SUMS` and optional GPG signatures |
| `packaging/linux/in-container.sh` | what runs inside that container: the build, the glibc floor check, the `.deb` and the AppImage |
| `packaging/linux/AppRun` | the AppImage's entry point: it runs the executable beside it |
| `packaging/linux/*.desktop` | the desktop entry, the same in both Linux packages |
| `packaging/check.sh` | static checks on the packages the three scripts make. It reads what it is given and changes nothing |
| `.github/workflows/packages.yml` | the packaging job: each host builds and checks its packages, and a tag publishes a release |

The two plist files carry no XML comments: `codesign` refuses an entitlements
file with one (`AMFIUnserializeXML: syntax error`).

Each target has its own standalone script. The tools it uses are the
platform's own, or pinned by version and SHA-256. The scripts run under
macOS's `/bin/bash` 3.2 and pass `bash -n` and `shellcheck -S warning`.
`packaging/linux/in-container.sh` runs under the container image's bash, and
`packaging/windows.sh` runs under Git Bash on Windows; both pass the same checks.

## `packaging/macos.sh`

Run it from the repository root, on the Mac that holds the signing keys:

```sh
bash packaging/macos.sh --dev                      # a local check build, not for distribution
bash packaging/macos.sh --unsigned --build-number N  # a release without a Developer ID: ad-hoc signed, not notarised
bash packaging/macos.sh --build-number 7 --out DIR # a release build (needs the variables below)
```

It runs these steps in order and stops at the first that fails:

1. A release build of `tawara-desktop` for `aarch64-apple-darwin` and
   `x86_64-apple-darwin`: `cargo build --release --locked -p tawara-desktop
   --target …` with `MACOSX_DEPLOYMENT_TARGET=11.0`.
2. A check that each build reports `minos 11.0` in `vtool -show-build`.
3. One universal binary, by `lipo -create`, checked for both architectures.
4. `Tawara.app`, with:
   - `Contents/MacOS/tawara-desktop`, the universal binary;
   - `Contents/Info.plist` from the template: `CFBundleIdentifier` as the
     template sets it, `CFBundleShortVersionString` from the
     `[workspace.package]` version in `Cargo.toml`, `CFBundleVersion` from the
     build number, `LSMinimumSystemVersion` 11.0, and `NSHighResolutionCapable`
     true;
   - `Contents/PkgInfo`;
   - `Contents/Resources/LICENSE.md` and `THIRD-PARTY-NOTICES.txt`, copied from
     the repository root;
   - `Contents/Resources/AppIcon.icns`, when `TAWARA_ICON` is set.
5. Signing: `codesign --options runtime --timestamp` with the empty entitlements
   file, then `codesign --verify --deep --strict`. A release signs with the
   Developer ID identity in `TAWARA_MAC_SIGN_ID`, which it requires. A `--dev`
   build signs when the variable is set; an ad-hoc `-` takes no timestamp.
   When `TAWARA_TEAM_ID` is also set, the signature must name that team.
6. `Tawara-X.Y.Z.dmg` (`Tawara-X.Y.Z-dev.dmg` with `--dev`), made by
   `hdiutil create` from a folder holding the app and a link to
   `/Applications`, and signed the same way when an identity is set.
7. Notarisation, in a release: `notarytool submit --wait` with the keychain
   profile in `TAWARA_NOTARY_PROFILE`, which a release requires. It must report
   `status: Accepted`; then `stapler staple` and `stapler validate`; then
   `spctl` must accept the image with `source=Notarized Developer ID`. A `--dev`
   build is not notarised.
8. `SHA256SUMS` beside the image, checked with `shasum -a 256 -c`.

The packages go to `target/package/macos` unless `--out` says otherwise. The
script ends with `signed: yes|ad-hoc|no; notarised: yes|no`. A release refuses
to start without an identity and a notary profile, so it always ends with
`signed: yes; notarised: yes`. A `--dev` package ends with `ad-hoc` or `no`, and
`notarised: no`, and must not be published.

The script first checks that both Rust targets are installed
(`rustup target list --installed`) and, when one is missing, prints the
`rustup target add` command to run.

## `packaging/linux.sh`

Run it from the repository root, on the machine that holds the keys:

```sh
bash packaging/linux.sh --dev              # a local check build, not for distribution
bash packaging/linux.sh --out DIR          # a release build (needs the variables below)
```

It needs a Docker daemon that can run `linux/amd64` images. The image is
`ubuntu:22.04` pinned by its index digest
`sha256:0a9887931b0e38af2e3487ed39e29436b0535c4f3f52e57745baaf79d1898956`,
which Docker resolves to the `linux/amd64` manifest (`sha256:98510ac07bb4545e389be502a8918b385c018bfe4ceeb5ac7e0375c782268b04`).
On an Apple silicon Mac the container runs under emulation, which is slower but
changes none of the checks.

It runs these steps in order and stops at the first that fails:

1. It checks its inputs before Docker is used: the icon, the maintainer, the
   two notices at the repository root, and the GPG key when one is named.
2. It starts the container with `--platform linux/amd64 --cpus=2 --memory=4g`
   and `CARGO_BUILD_JOBS=2`. The repository is mounted read-only at `/src`. A
   volume named `tawara-linux-N`, with `N` derived from the checkout's path, is
   mounted at `/cache`, so the next run compiles only what changed. The
   packages go to `/out`.
3. Inside the container, `packaging/linux/in-container.sh` installs the build
   packages, plus `file` and `squashfs-tools` for the AppImage step. It checks
   rustup-init 1.29.1 against its SHA-256, installs the toolchain
   `rust-toolchain.toml` names, and runs `cargo build --release --locked -p
   tawara-desktop --target x86_64-unknown-linux-gnu`.
4. The glibc floor check: `objdump -T` lists the `GLIBC_` symbol versions the
   executable needs. The highest must be 2.35 or lower.
5. The `.deb`, from a tree holding `usr/bin/tawara-desktop`, the desktop entry
   under `usr/share/applications/`, the hicolor icons at 16, 32, 48, 64, 128,
   256 and 512 pixels, `LICENSE.md` and `THIRD-PARTY-NOTICES.txt` under
   `usr/share/doc/tawara-desktop/`, and `DEBIAN/control`. `Depends` is the value
   `dpkg-shlibdeps -O` prints for the executable, followed by the libraries the
   app opens at run time, listed by hand. `Recommends` is `libvulkan1`. The
   package is made with `dpkg-deb --root-owner-group --build`, and its `--info`
   and `--contents` are printed.
6. The AppImage, only when an icon is given: an AppDir with `AppRun`, the
   desktop entry at its top and in `usr/share/applications`, the 256-pixel icon,
   and the notices beside the executable. appimagetool 1.9.1 and the type 2
   runtime 20251108 are each checked against their SHA-256. appimagetool is
   itself an AppImage, and an emulated container's binfmt rule refuses to
   execute an AppImage's ELF header (`Exec format error`). So its squashfs part,
   which begins where the runtime ends, is unpacked with `unsquashfs`, and that
   tree's `AppRun` runs the tool with `--runtime-file`, which needs no FUSE. The
   new AppImage is checked by listing its squashfs part for `AppRun` and
   `usr/bin/tawara-desktop`. A native x86_64 host runs the same steps.
7. On the host, `SHA256SUMS` lists the packages and is checked with
   `shasum -a 256 -c`. With `TAWARA_GPG_KEY`, `SHA256SUMS`, the `.deb` and the
   AppImage each get a detached armored signature (`.asc`), checked with
   `gpg --verify`.

The packages go to `target/package/linux` unless `--out` says otherwise:

| Release | `--dev` |
|---|---|
| `tawara-desktop_X.Y.Z_amd64.deb` | `tawara-desktop_X.Y.Z~dev_amd64.deb` |
| `Tawara-X.Y.Z-x86_64.AppImage` (with an icon) | `Tawara-X.Y.Z-dev-x86_64.AppImage` (with an icon) |
| `SHA256SUMS` | `SHA256SUMS` |

The version is the one `[workspace.package]` sets in `Cargo.toml`. The pins
are at the top of `packaging/linux.sh`: the image digest, rustup-init 1.29.1,
appimagetool 1.9.1 and runtime 20251108, each with its SHA-256. A change to any
pin names the new version and hash.

The build volume is kept between runs. Remove it with
`docker volume rm tawara-linux-N` when its cache is no longer wanted; `N` is
the `cksum` of the checkout's path.

The apt packages are not pinned: `apt-get` installs the archive's current
versions. The image digest pins the base system only.

## `packaging/windows.sh`

Run it from the repository root, in Git Bash on Windows, on the machine that
holds the signing token:

```sh
bash packaging/windows.sh --record-nsis   # prints the NSIS archive's SHA-256
bash packaging/windows.sh --dev           # a local check build, not for distribution
bash packaging/windows.sh --out DIR       # a release build (needs the variables below)
bash packaging/windows.sh --unsigned      # a release build without code signing
```

It runs these steps in order and stops at the first that fails:

1. The NSIS 3.13 archive, `nsis-3.13.zip`, downloaded from SourceForge over
   HTTPS when `target/package/tools` has no copy matching `NSIS_SHA256` in the
   script. A release refuses to start when `NSIS_SHA256` is empty. A `--dev`
   build takes this step after step 4, so the executable is built and checked
   first. The archive is unpacked with `Expand-Archive`, and
   `makensis -VERSION` must report 3.13.
2. The Rust target `x86_64-pc-windows-msvc`, and the Windows SDK's `rc.exe` and
   `signtool.exe`: from `TAWARA_WINSDK_BIN`, or else from the newest
   `Windows Kits\10\bin\10.*\x64` under `Program Files (x86)` that holds both.
3. The version resource: `rc.exe` compiles the icon, when `TAWARA_WIN_ICO` is
   set, and the version information, with `FileVersion` `X.Y.Z.0` and
   `ProductVersion` `X.Y.Z`. A `--dev` build without `rc.exe` has no resource.
4. The build: `cargo build --release --locked -p tawara-desktop --target
   x86_64-pc-windows-msvc`, with `target.x86_64-pc-windows-msvc.rustflags=["-C",
   "target-feature=+crt-static"]` on the command line and the resource's link
   flag in the same list. A release refuses when `RUSTFLAGS`,
   `CARGO_ENCODED_RUSTFLAGS` or `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS`
   is set, because cargo would then ignore that list.
5. When `TAWARA_WIN_CERT_SHA1` is set, `signtool sign /sha1 THUMBPRINT /fd sha256
   /tr ADDRESS /td sha256` on `tawara-desktop.exe`, then `signtool verify /pa /v`.
6. The installer, by `makensis -V2 -WX` on `packaging/windows/tawara.nsi`. It
   installs per user into `%LOCALAPPDATA%\Programs\Tawara`, with a Start menu
   entry named "Tawara", an uninstaller and an Installed apps entry, and places
   `LICENSE.md` and `THIRD-PARTY-NOTICES.txt` beside the executable. When
   signing is on, NSIS signs the uninstaller as it makes it (`!uninstfinalize`).
   makensis does not fail when that command fails, so the command also writes a
   stamp file; when the stamp is missing, the script removes the installer and
   stops.
7. When signing is on, `signtool sign` and `signtool verify /pa /v` on the
   installer.
8. `SHA256SUMS` beside the installer, checked with `sha256sum -c`.

The package goes to `target/package/windows` unless `--out` says otherwise, and
the script ends with `signed: yes` or `signed: no`. A signed release needs
`TAWARA_WIN_CERT_SHA1` and ends with `signed: yes`. An `--unsigned` release
ends with `signed: no` and is named as a release. A `--dev` package ends with
`signed: no` and must not be published. After an install,
check the uninstaller's signature with
`signtool verify /pa /v "%LOCALAPPDATA%\Programs\Tawara\uninstall.exe"`. The
uninstaller removes the program's files, its Start menu entry and its Installed
apps entry, and never the wallet store under `%LOCALAPPDATA%\Tawara`. It does
not stop a running Tawara, so close the app first.

## Environment

No identity, team, key, certificate or password is read from a file or written
into one. Each signing or notarisation step reads its name from the environment
and is skipped, with a message, when that name is unset.

| Variable | What it is for | Release | `--dev` |
|---|---|---|---|
| `DEVELOPER_DIR` | the Xcode to build with. When unset and `/Applications/Xcode_26.6.app` exists, the script uses that one | required; its macOS SDK must be 26.x | optional; a warning if the SDK is not 26.x |
| `TAWARA_ICON` | path to the app icon as an `.icns` file. `iconutil -c icns` makes it from an `.iconset` folder of the standard sizes, made from a 1024-pixel PNG | required | optional; without it the build has no icon |
| `TAWARA_BUILD_NUMBER`, or `--build-number` | the bundle's `CFBundleVersion` | required | defaults to `1` |
| `TAWARA_MAC_SIGN_ID` | the Developer ID Application identity, by the name `codesign` reads, such as `Developer ID Application: Name (TEAMID)` | required; `-` (ad-hoc) is refused | signs when set; unset: the packages are unsigned, with a warning |
| `TAWARA_TEAM_ID` | the team the signature must name, checked after signing | optional | optional |
| `TAWARA_NOTARY_PROFILE` | the `notarytool` keychain profile, made with `xcrun notarytool store-credentials` on this Mac | required | not used |
| `CARGO_TARGET_DIR` | where cargo writes its output, as cargo reads it | optional | optional |

`TAWARA_MAC_SIGN_ID=-` makes an ad-hoc signature. It exercises the signing
steps for a local check and its output is not for distribution. A release
refuses it, and a `--dev` build prints `signed: ad-hoc`.

The Linux script reads three variables. It never uses `CARGO_TARGET_DIR`: its
target directory is the build volume described above.

| Variable | What it is for | Release | `--dev` |
|---|---|---|---|
| `TAWARA_ICON_PNG` | the app icon as a square PNG of at least 1024 pixels. `sips` makes the sizes on macOS, with ImageMagick's `identify` and `convert` where `sips` is absent | required | optional; without it the `.deb` has no icons and there is no AppImage |
| `TAWARA_DEB_MAINTAINER` | the `.deb`'s `Maintainer:` field, as `Name <address>`. A release refuses the placeholder `Tawara Maintainer <maintainer@example.invalid>` and any `.invalid` address | required | optional; the placeholder when unset |
| `TAWARA_GPG_KEY` | the GPG key that signs `SHA256SUMS` and the packages, named by an identity in the keyring (`gpg --local-user`). Checked before the build starts | unset: unsigned | signs when set |

The Windows script reads these variables:

| Variable | What it is for | Release | `--dev` |
|---|---|---|---|
| `TAWARA_WIN_ICO` | the app icon as an `.ico` file, used for the executable and the installer | required | optional; without it the build has no icon |
| `TAWARA_WIN_CERT_SHA1` | the SHA-1 thumbprint of the code-signing certificate, which signtool finds in the certificate store that holds the token | required, unless `--unsigned` (which refuses it) | signs when set; unset: unsigned, with a warning |
| `TAWARA_WIN_TIMESTAMP_URL` | the certificate authority's RFC 3161 timestamp address. It must be a plain `http` or `https` address made of letters, digits and `._~:/?=+,@-` | required when signing | required when signing |
| `TAWARA_WIN_PUBLISHER` | the publisher name that Installed apps shows; the script's default when unset. It may not hold a double quote, a dollar sign or a backquote | optional | optional |
| `TAWARA_WINSDK_BIN` | the Windows SDK's `x64` folder holding `rc.exe` and `signtool.exe`, when the newest one found is not the one to use | optional | optional |
| `CARGO_TARGET_DIR` | where cargo writes its output; a relative path is relative to the repository root | optional | optional |

## The packaging job

`.github/workflows/packages.yml` builds the packages on three hosts for every
push, pull request and tag, checks them, and keeps them as workflow artifacts
for 30 days:

| Host | Builds | Checks | Starts |
|---|---|---|---|
| `windows-latest` | `windows.sh --unsigned`, with the icon in `packaging/icons/` | the executable: x86-64, no Visual C++ runtime DLL, the version resource; the installer | the executable is not run |
| `ubuntu-24.04` | `linux.sh` in the pinned `ubuntu:22.04` container | the `.deb`'s control fields and contents, the glibc floor, the AppImage's contents | the `.deb`, installed with apt, then removed; the AppImage's executable, once extracted |
| `macos-26` | `macos.sh --unsigned`: both Rust targets, the universal app, the disk image, ad-hoc signed | the two slices and `minos 11.0`, `Info.plist`, the notices, the disk image and the app in it | the app, from the bundle, on wgpu and on tiny-skia |

A tag `vX.Y.Z` that matches the workspace version publishes the Windows,
macOS and Linux packages as a GitHub release, with one `SHA256SUMS` for all of them.
The release job signs `SHA256SUMS` with the Tawara release key, a secret of
the `release` environment that only `v*` tags can use, after checking that it
is the key in `packaging/tawara-release-key.asc` at the tagged commit. It
publishes the detached signature `SHA256SUMS.asc` and that public key with the
packages. Every hour the job also builds against the library's newest `main`
commit, when no earlier run built it; those artifacts are named
`upstream-<commit>` and are never released. Nothing in the job code-signs or
notarises the packages themselves.

## Checks

`packaging/check.sh` runs these, in the job and by hand. It reads the packages
and changes nothing; it needs no key, certificate or network.

### macOS checks

- `bash -n packaging/macos.sh` and `shellcheck -S warning packaging/macos.sh`.
- `bash packaging/macos.sh --dev`, then `bash packaging/check.sh macos
  target/package/macos`: `lipo -archs` (both architectures), `vtool -arch … -show-build`
  (`minos 11.0` for each), `plutil` on the bundle's `Info.plist` (identifier,
  executable, package type, minimum system version and version), the notices
  against the repository's copies, and the disk image, which must verify and
  hold the same executable, attached read-only.
- A launch: `LIVENESS_SECONDS=10 bash ci/liveness.sh
  target/package/macos/Tawara.app/Contents/MacOS/tawara-desktop`. The liveness
  script starts the app with a fresh `HOME`, once on wgpu and once on
  tiny-skia. Each launch must still be running after the wait, with no panic in
  its log, and is stopped afterwards.

### Linux checks

- `bash -n` and `shellcheck -S warning` on `packaging/linux.sh` and
  `packaging/linux/in-container.sh`, and `/bin/bash -n` (bash 3.2) on the
  first.
- `bash packaging/check.sh linux target/package/linux`: the `.deb`'s `Package`,
  `Architecture`, `Version` (with `~dev` in a development build), `Maintainer`,
  `Depends` (a `libc6` version and the hand-listed libraries) and
  `Recommends`, read with `dpkg-deb -f`; its contents (`dpkg-deb -c`), including
  the executable at mode 0755 and seven icons; the GLIBC symbol versions of the
  unpacked executable (`objdump -T`), at most 2.35; and the AppImage, extracted
  with `--appimage-extract`, which must hold `AppRun`, the desktop entry, and the
  same executable as the `.deb`. `SHA256SUMS` must list both and check.
- The CI job installs the `.deb` on the runner with apt, starts it under Xvfb
  with `ci/liveness.sh` for 20 seconds on each renderer, and removes it. It
  extracts the AppImage and runs the same check on the executable inside.
- The input checks stop before Docker is used: a missing or wrong icon, an
  unset, placeholder or malformed maintainer, a `TAWARA_GPG_KEY` that names no
  secret key, and an unknown argument each exit with an error and a message.
- `bash packaging/linux.sh --dev` with a 1024-pixel PNG as `TAWARA_ICON_PNG`
  makes the `.deb` and the AppImage; its log holds the glibc check and the
  `dpkg-deb --info` and `--contents` output. Without an icon it makes the
  `.deb` alone, and `SHA256SUMS` lists the `.deb` alone.
- On a fresh `ubuntu:22.04` container of the same digest, `--platform
  linux/amd64`, with `xvfb`, `xauth` and `mesa-vulkan-drivers` added, the
  `.deb` installs by its path, `ci/liveness.sh` passes on
  `/usr/bin/tawara-desktop`, and the package is removed. The repository is
  mounted read-only at `/src` for the script.
- For the AppImage: in another fresh container, its squashfs part is unpacked
  (from the offset the runtime's size gives), `AppRun` is run under `xvfb-run`
  for 20 seconds, and `ci/liveness.sh` passes on the executable inside. The
  AppImage itself cannot be executed in an emulated container, so this check
  runs the same files from their squashfs part.

### Windows checks

- `bash -n` and `shellcheck -S warning` on `packaging/windows.sh`, and
  `/bin/bash -n` (bash 3.2) on the same file.
- `bash packaging/check.sh windows-exe target/x86_64-pc-windows-msvc/release/tawara-desktop.exe`:
  the file is a Windows executable with an x86-64 machine type, names no Visual
  C++ runtime DLL (so the static runtime applied), and its version resource reads
  `FileVersion` `X.Y.Z.0`, `ProductName` Tawara, `ProductVersion` `X.Y.Z` (or
  `X.Y.Z-dev`) and `FileDescription` Tawara, read by PowerShell.
- `bash packaging/check.sh windows target/package/windows`, once the installer is
  made: one installer, listed in `SHA256SUMS`, which checks.
- `makensis` compiles `packaging/windows/tawara.nsi` with the defines the script
  passes, with `-WX` (warnings are errors), both unsigned and with signing on,
  using a stand-in `signtool`. With signing on, the uninstaller's signing command
  runs with the thumbprint and the timestamp address in it, and the stamp file
  exists only after it succeeds. On macOS, NSIS needs a UTF-8 locale
  (`LC_ALL=en_US.UTF-8`): with the C locale, the Homebrew build aborts with
  `std::bad_alloc`.
- To change the pinned NSIS archive, run `bash packaging/windows.sh
  --record-nsis` from two different networks; the two hashes must agree before
  `NSIS_SHA256` is updated.
- On a clean Windows machine, after `bash packaging/windows.sh --dev`: the
  installer runs without an administrator prompt, the Start menu entry starts
  the app, uninstalling leaves the wallet store and preferences in place, and
  any SmartScreen prompt names the certificate's subject.

## Release inputs

A release needs these, supplied on the release machine:

- the app icon in each platform's format;
- the macOS Developer ID identity and notary profile, and Xcode 26.6;
- the Windows code-signing certificate and the certificate authority's
  timestamp address;
- the Linux `.deb` maintainer and, optionally, a GPG key.

The Linux container's caps (2 CPUs, 4 GiB, two cargo jobs) are set in
`packaging/linux.sh`; four jobs in an 8 GiB Docker VM can run `rustc` out of
memory.
