# Tawara

A graphical wallet for Mochimo, for Windows, macOS, Linux, Android and iOS,
written in Rust with the [iced](https://iced.rs) toolkit.

Copyright Adequate Systems HK Limited.

Tawara is maintained, owned and published by **Adequate Systems HK Limited**.

- Original Author: Kami Sama
- Maintainer, owner and publisher: Adequate Systems HK Limited
- Contact: support@adequatelimited.com

Tawara is built on `mochimo-crypto`, the wallet library of the Mochimo
command-line wallet
([adequatelimited/mcm-rust-cli-windows](https://github.com/adequatelimited/mcm-rust-cli-windows)),
pinned to an exact commit. The library holds the wallet's core safety
properties: the encrypted keystore whose one-time key index only ever moves
forward, reconciliation against the chain, and the refusals that stop a key
from signing twice. This repository holds the application around it and
never modifies the library. The application adds its own protections: the
idle and background locks, wiping typed secrets, confining the store on
phones, and the clipboard and paste rules. See
[docs/THREAT_MODEL.md](docs/THREAT_MODEL.md).

## Status

The desktop application runs on Windows, macOS and Linux, and CI builds and
tests it on all three. The packaging scripts for each desktop platform are
in [packaging/](packaging/README.md). The Android and iOS shells live in a
separate repository, Tawara-mobile, which uses `tawara-app` from this one.

## Verifying a download

Each [release](https://github.com/adequatelimited/Tawara-wallet/releases)
has a `SHA256SUMS` file covering every package, signed by the Tawara release
key in [packaging/tawara-release-key.asc](packaging/tawara-release-key.asc):

```
Tawara Releases (Adequate Systems HK Limited) <support@adequatelimited.com>
ed25519, fingerprint 52A1 21B9 9B6C 7EDB 85F3  338D 9C0D 88F0 F91C CC8C
```

```
gpg --import tawara-release-key.asc
gpg --verify SHA256SUMS.asc SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS
```

`gpg` must report a good signature from that fingerprint. On macOS, use
`shasum -a 256 --ignore-missing -c SHA256SUMS` for the last step. The Windows
and macOS packages are not yet code-signed.

## Layout

```
crates/
  wallet-core/   everything that touches the library; no interface dependency
  app/           the iced application: state, messages, screens, theme
  desktop/       the Windows, macOS and Linux entry point
docs/
  THREAT_MODEL.md   assets, adversaries, controls and known gaps
packaging/          desktop packages: macOS, Windows, Linux
ci/                 helper scripts the CI workflows run
```

## Building the desktop application

The same steps work on Windows, macOS and Linux.

1. **Install Rust** with [rustup](https://rustup.rs). The compiler version
   is pinned in `rust-toolchain.toml`, and `rustup` installs it the first
   time you run `cargo` here.
2. **Install a C compiler.** TLS uses `ring`, which compiles C:
   - Windows: the Visual Studio Build Tools, with the "Desktop development
     with C++" workload (MSVC);
   - macOS: Xcode's command-line tools, `xcode-select --install`;
   - Linux: the system's compiler, for example `sudo apt install
     build-essential` on Debian or Ubuntu.
3. **Get the source:**

   ```
   git clone https://github.com/adequatelimited/Tawara-wallet.git
   cd Tawara-wallet
   ```

4. **Build it:**

   ```
   cargo build --release --locked -p tawara-desktop
   ```

   The first build downloads the dependencies, the wallet library among
   them (from GitHub, at the commit `Cargo.toml` pins), so it needs the
   network and takes several minutes. `--locked` builds exactly the
   versions in `Cargo.lock`.
5. **Run it:** the program is `target/release/tawara-desktop`
   (`target\release\tawara-desktop.exe` on Windows). It is self-contained
   and can be copied anywhere. `cargo run --release -p tawara-desktop`
   builds and runs in one step.

Build with `--release`: in a debug build, unlocking a store (Argon2id at
64 MiB, three passes) takes many seconds.

On Linux the application needs an X11 or Wayland session with
`libxkbcommon` (present on most desktops). It draws with the GPU through
wgpu and falls back to software rendering when there is none; to force
software rendering, for example in a virtual machine, run it with
`ICED_BACKEND=tiny-skia`.

## Where data goes

The store goes, by default, under `%LOCALAPPDATA%\Tawara` on Windows,
`~/Library/Application Support/Tawara` on macOS, and
`$XDG_DATA_HOME/tawara` (or `~/.local/share/tawara`) on Linux.

Unless another node is chosen, or the node is forgotten on the node screen,
the wallet uses `https://api.mochimo.org`, whose requests pass through
Cloudflare. If the preferences file cannot be read, the wallet uses no node
until one is chosen or forgotten, on the node screen or in Settings. Nothing
is asked of the node at start-up: only once a wallet is opened, or when the
person asks.

## Developing

The checks CI runs (`.github/workflows/ci.yml`) on Windows, macOS and Linux:

```
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo deny check            # needs cargo-deny 0.20.2 and the network
```

`cargo run -p tawara-app --example screenshots` draws every screen
headlessly into `target/screenshots/`.

## Licence

Copyright Adequate Systems HK Limited. Tawara is distributed under the
Mochimo Cryptocurrency Engine License Agreement, version 1.0
([LICENSE.md](LICENSE.md)), the same licence as the wallet library.
Third-party dependencies are held to permissive licences by
[deny.toml](deny.toml), and their licence texts are in
[THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt), which the Linux CI job
checks against `Cargo.lock`.
