// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Where a store lives.
//!
//! Two questions, answered without touching the disk beyond reading what is
//! already there:
//!
//! - **Where the store goes by default.** On the desktop, the platform's
//!   per-user, per-machine application data directory: never a roaming or
//!   synced one. On Android and iOS the shell knows the app-private
//!   directory and passes it in ([`store_dir_in`]).
//! - **Whether a chosen directory is inside a folder a cloud service syncs.**
//!   A synced or restored copy of a store rolls back its one-time key index
//!   and any open reservation, which can lead to signing twice with one key.
//!   The default is outside every such folder, and choosing one gives a
//!   warning ([`sync_warning`]).
//!
//! The library's own checks still apply to whatever directory is used:
//! owner and mode on Unix, access lists on Windows, and refusal of a
//! directory named by a symbolic link. Nothing here replaces them.
//!
//! # What is not here
//!
//! Backup exclusion on mobile is set by the platform shells:
//! `android:allowBackup="false"` and `dataExtractionRules` on Android,
//! `NSURLIsExcludedFromBackupKey` on iOS. On the desktop, backups (Time
//! Machine, File History, shadow copies) may archive the store; nothing
//! excludes it.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

/// The platform whose conventions apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
    Android,
    Ios,
}

impl Platform {
    /// The platform this build is for.
    #[must_use]
    pub const fn current() -> Platform {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(target_os = "android") {
            Platform::Android
        } else if cfg!(target_os = "ios") {
            Platform::Ios
        } else {
            Platform::Linux
        }
    }

    /// Whether the platform's usual file systems ignore case, so that a
    /// folder name is matched without it (`OneDrive` and `onedrive` are one
    /// folder on Windows and on a default macOS volume).
    fn ignores_case(self) -> bool {
        matches!(self, Platform::Windows | Platform::MacOs | Platform::Ios)
    }
}

/// The application's directory name under the platform's data directory.
/// Capitalised where the platform's own applications are (Windows, macOS),
/// lower case under `$XDG_DATA_HOME`.
#[must_use]
pub fn app_dir_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Linux => "tawara",
        _ => "Tawara",
    }
}

/// The store's own directory under the application's directory.
pub const STORE_DIR_NAME: &str = "keystore";

/// The environment variables the platform conventions name, read once.
///
/// A type rather than `std::env::var` calls so that tests can state the
/// environment they mean.
#[derive(Clone, Debug, Default)]
pub struct Environment {
    vars: BTreeMap<String, OsString>,
}

impl Environment {
    /// This process's environment.
    #[must_use]
    pub fn from_process() -> Environment {
        Environment {
            vars: std::env::vars_os()
                .filter_map(|(k, v)| k.into_string().ok().map(|k| (k, v)))
                .collect(),
        }
    }

    /// An environment holding exactly these variables.
    #[must_use]
    pub fn of(pairs: &[(&str, &str)]) -> Environment {
        Environment {
            vars: pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), OsString::from(*v)))
                .collect(),
        }
    }

    /// A variable's value as an absolute path on `platform`, or `None` when
    /// it is unset, empty or relative. The XDG specification says a relative
    /// `$XDG_DATA_HOME` is to be ignored, and no other variable used here
    /// means anything relative.
    fn absolute(&self, name: &str, platform: Platform) -> Option<PathBuf> {
        let value = self.vars.get(name)?;
        if value.is_empty() {
            return None;
        }
        let path = PathBuf::from(value);
        is_absolute_on(&path, platform).then_some(path)
    }
}

/// Absolute by `platform`'s rules, whichever platform runs the code, as
/// [`parts`] splits by them (the unit tests describe all five platforms on
/// each of the three that run them): rooted at `/` on the Unix platforms;
/// on Windows, drive-rooted (`C:\`, `C:/`) or a UNC or verbatim path
/// (`\\server\share`, `\\?\C:\`).
fn is_absolute_on(path: &Path, platform: Platform) -> bool {
    let text = path.to_string_lossy();
    let bytes = text.as_bytes();
    match platform {
        Platform::Windows => {
            let drive = bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'\\' | b'/');
            drive || text.starts_with(r"\\") || text.starts_with("//")
        }
        Platform::MacOs | Platform::Linux | Platform::Android | Platform::Ios => {
            bytes.first() == Some(&b'/')
        }
    }
}

/// Why there is no default location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoDefaultLocation {
    /// The variable the platform's convention starts from is unset, empty or
    /// relative.
    Unset { variable: &'static str },
    /// On Android and iOS only the shell knows the app-private directory;
    /// it passes it to [`store_dir_in`].
    ShellSupplies,
}

impl fmt::Display for NoDefaultLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NoDefaultLocation::Unset { variable } => write!(
                f,
                "there is no default place for the wallet's keystore: {variable} is not set to an \
                 absolute path. Choose a folder for it."
            ),
            NoDefaultLocation::ShellSupplies => f.write_str(
                "on this platform the app's private directory is supplied by the app itself",
            ),
        }
    }
}

/// The default store directory on a desktop platform:
///
/// - Windows: `%LOCALAPPDATA%\Tawara\keystore`, never under the roaming
///   `%APPDATA%`, which a domain profile copies to other machines;
/// - macOS: `~/Library/Application Support/Tawara/keystore`;
/// - Linux: `$XDG_DATA_HOME/tawara/keystore`, with `~/.local/share` when
///   `$XDG_DATA_HOME` is unset or relative, as the specification says.
///
/// Nothing is created here. When a store is created, the worker makes the
/// application folder (and any folder above it that is missing), private to
/// the user on Unix, and the library makes the store's own folder inside
/// it, mode `0700` or with a private access list.
pub fn default_store_dir(
    platform: Platform,
    env: &Environment,
) -> Result<PathBuf, NoDefaultLocation> {
    let base = match platform {
        Platform::Windows => {
            env.absolute("LOCALAPPDATA", platform)
                .ok_or(NoDefaultLocation::Unset {
                    variable: "LOCALAPPDATA",
                })?
        }
        Platform::MacOs => env
            .absolute("HOME", platform)
            .ok_or(NoDefaultLocation::Unset { variable: "HOME" })?
            .join("Library")
            .join("Application Support"),
        Platform::Linux => match env.absolute("XDG_DATA_HOME", platform) {
            Some(data) => data,
            None => env
                .absolute("HOME", platform)
                .ok_or(NoDefaultLocation::Unset { variable: "HOME" })?
                .join(".local")
                .join("share"),
        },
        Platform::Android | Platform::Ios => return Err(NoDefaultLocation::ShellSupplies),
    };
    Ok(base.join(app_dir_name(platform)).join(STORE_DIR_NAME))
}

/// The person's Downloads folder, where "Save artifact" writes a signed
/// spend's bytes:
///
/// - Windows: `%USERPROFILE%\Downloads`, the folder's own place. A folder
///   the person has moved is recorded by the shell's known-folder list,
///   which only the Windows API reads, and is not followed;
/// - macOS: `~/Downloads`;
/// - Linux: the `XDG_DOWNLOAD_DIR` that `user-dirs.dirs` names (in
///   `$XDG_CONFIG_HOME`, or `~/.config`), when it names one under `$HOME`
///   or an absolute folder, and `~/Downloads` otherwise;
/// - Android and iOS: `None`, the shell's to decide.
///
/// `None` too when the variable a desktop starts from is unset, empty or
/// relative. Nothing is created here.
#[must_use]
pub fn downloads_dir(platform: Platform, env: &Environment) -> Option<PathBuf> {
    match platform {
        Platform::Windows => Some(env.absolute("USERPROFILE", platform)?.join("Downloads")),
        Platform::MacOs => Some(env.absolute("HOME", platform)?.join("Downloads")),
        Platform::Linux => {
            let home = env.absolute("HOME", platform)?;
            let config = env
                .absolute("XDG_CONFIG_HOME", platform)
                .unwrap_or_else(|| home.join(".config"));
            let named = std::fs::read_to_string(config.join("user-dirs.dirs"))
                .ok()
                .and_then(|text| user_dirs_download(&text, &home));
            Some(named.unwrap_or_else(|| home.join("Downloads")))
        }
        Platform::Android | Platform::Ios => None,
    }
}

/// The download folder `user-dirs.dirs` names, as `xdg-user-dirs` writes
/// the file: `XDG_DOWNLOAD_DIR="$HOME/Downloads"` or an absolute path in
/// quotes. `"$HOME/"` alone is the specification's way of saying the folder
/// is turned off, and anything else is not understood; both are `None`.
fn user_dirs_download(text: &str, home: &Path) -> Option<PathBuf> {
    let value = text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("XDG_DOWNLOAD_DIR=")
            .map(|v| v.trim().trim_matches('"'))
    })?;
    if let Some(rest) = value.strip_prefix("$HOME/") {
        let rest = rest.trim_end_matches('/');
        return (!rest.is_empty()).then(|| home.join(rest));
    }
    value.starts_with('/').then(|| PathBuf::from(value))
}

/// The store directory under a mobile app's private directory: on Android
/// the `no_backup` directory, which the system leaves out of Auto Backup,
/// on iOS Application Support in the app's container. The library makes the
/// store's own folder inside it with mode `0700`, so the parent's mode
/// (`0771` for Android's `files/`) does not matter.
#[must_use]
pub fn store_dir_in(app_private: &Path) -> PathBuf {
    app_private.join(STORE_DIR_NAME)
}

/// A chosen store directory is inside a folder a cloud service syncs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncWarning {
    /// The service, as its own name is written.
    pub service: &'static str,
    /// The synced folder the directory is inside.
    pub folder: PathBuf,
    /// Whether syncing depends on a setting this check cannot read (macOS
    /// Desktop and Documents, which iCloud syncs only when asked to).
    pub only_if_enabled: bool,
}

impl fmt::Display for SyncWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let when = if self.only_if_enabled {
            "may be synced"
        } else {
            "is synced"
        };
        write!(
            f,
            "{} {when} by {}. A synced or restored copy of the store brings back an older \
             one-time key index, and signing from it can use a key twice, which exposes the \
             funds it guards. Keep the store in a folder no service copies.",
            self.folder.display(),
            self.service
        )
    }
}

/// One folder a service syncs, relative to a base the environment names.
struct Synced {
    service: &'static str,
    /// Path components under the base; the last may match by prefix.
    under: &'static [&'static str],
    /// Whether the last component matches any folder name it begins
    /// (`OneDrive - Contoso`).
    prefix: bool,
    only_if_enabled: bool,
}

const fn synced(service: &'static str, under: &'static [&'static str]) -> Synced {
    Synced {
        service,
        under,
        prefix: false,
        only_if_enabled: false,
    }
}

const MACOS_HOME: &[Synced] = &[
    synced("iCloud Drive", &["Library", "Mobile Documents"]),
    synced("a cloud storage provider", &["Library", "CloudStorage"]),
    synced("Dropbox", &["Dropbox"]),
    synced("Google Drive", &["Google Drive"]),
    synced("Box", &["Box"]),
    synced("pCloud", &["pCloud Drive"]),
    Synced {
        service: "OneDrive",
        under: &["OneDrive"],
        prefix: true,
        only_if_enabled: false,
    },
    Synced {
        service: "iCloud Drive (Desktop and Documents)",
        under: &["Desktop"],
        prefix: false,
        only_if_enabled: true,
    },
    Synced {
        service: "iCloud Drive (Desktop and Documents)",
        under: &["Documents"],
        prefix: false,
        only_if_enabled: true,
    },
];

const WINDOWS_PROFILE: &[Synced] = &[
    synced("Dropbox", &["Dropbox"]),
    synced("Google Drive", &["Google Drive"]),
    synced("iCloud Drive", &["iCloudDrive"]),
    synced("Box", &["Box"]),
    Synced {
        service: "OneDrive",
        under: &["OneDrive"],
        prefix: true,
        only_if_enabled: false,
    },
];

const LINUX_HOME: &[Synced] = &[
    synced("Dropbox", &["Dropbox"]),
    synced("Nextcloud", &["Nextcloud"]),
    synced("ownCloud", &["ownCloud"]),
    synced("MEGA", &["MEGA"]),
    synced("pCloud", &["pCloudDrive"]),
    synced("Synology Drive", &["SynologyDrive"]),
    synced("Seafile", &["Seafile"]),
    synced("Google Drive", &["Google Drive"]),
    synced("OneDrive", &["OneDrive"]),
];

/// Whether `dir` is inside a folder a cloud service syncs, by the folders
/// each service creates by default and the variables Windows sets for
/// OneDrive. A roaming Windows profile (`%APPDATA%`) counts: it is copied to
/// and from other machines.
///
/// Checked on the path as given and, when part of it exists, on that part
/// with its links resolved, so a link into a synced folder is seen too. A
/// folder a service syncs under another name is not seen; this is a warning
/// for the common case, not a proof of the opposite.
#[must_use]
pub fn sync_warning(dir: &Path, platform: Platform, env: &Environment) -> Option<SyncWarning> {
    let mut candidates = vec![dir.to_path_buf()];
    if let Some(resolved) = resolve_existing(dir)
        && resolved != dir
    {
        candidates.push(resolved);
    }
    candidates
        .iter()
        .find_map(|path| sync_warning_lexical(path, platform, env))
}

fn sync_warning_lexical(dir: &Path, platform: Platform, env: &Environment) -> Option<SyncWarning> {
    let check = |base: &Path, list: &[Synced]| {
        list.iter().find_map(|s| {
            let folder = inside(dir, base, s.under, s.prefix, platform)?;
            Some(SyncWarning {
                service: s.service,
                folder,
                only_if_enabled: s.only_if_enabled,
            })
        })
    };
    match platform {
        Platform::MacOs => env
            .absolute("HOME", platform)
            .and_then(|home| check(&home, MACOS_HOME)),
        Platform::Windows => {
            for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
                if let Some(folder) = env.absolute(var, platform)
                    && starts_with(dir, &folder, platform)
                {
                    return Some(SyncWarning {
                        service: "OneDrive",
                        folder,
                        only_if_enabled: false,
                    });
                }
            }
            if let Some(roaming) = env.absolute("APPDATA", platform)
                && starts_with(dir, &roaming, platform)
            {
                return Some(SyncWarning {
                    service: "a roaming Windows profile",
                    folder: roaming,
                    only_if_enabled: false,
                });
            }
            env.absolute("USERPROFILE", platform)
                .and_then(|profile| check(&profile, WINDOWS_PROFILE))
        }
        Platform::Linux => env
            .absolute("HOME", platform)
            .and_then(|home| check(&home, LINUX_HOME)),
        Platform::Android | Platform::Ios => None,
    }
}

/// The synced folder `base/under..` when `dir` is inside it.
fn inside(
    dir: &Path,
    base: &Path,
    under: &[&str],
    prefix: bool,
    platform: Platform,
) -> Option<PathBuf> {
    let fold = platform.ignores_case();
    let dir_parts = parts(dir, platform);
    let base_parts = parts(base, platform);
    let rest = strip(&dir_parts, &base_parts, fold)?;
    let mut matched = Vec::new();
    for (i, want) in under.iter().enumerate() {
        let got = rest.get(i)?;
        let last = i + 1 == under.len();
        let ok = if last && prefix {
            begins(got, want, fold)
        } else {
            same(got, want, fold)
        };
        if !ok {
            return None;
        }
        matched.push(got.clone());
    }
    Some(join_parts(base, &matched, platform))
}

fn starts_with(dir: &Path, base: &Path, platform: Platform) -> bool {
    strip(
        &parts(dir, platform),
        &parts(base, platform),
        platform.ignores_case(),
    )
    .is_some()
}

/// A path's components by the platform's own rules, whichever platform runs
/// the code: Windows separates on `\` and `/`, the others on `/`. `.` and
/// empty components are dropped; nothing else is normalised, except that a
/// Windows verbatim prefix is read as the path it spells: `canonicalize`
/// returns `\\?\C:\Users\..` and `\\?\UNC\server\share\..`, which must
/// compare equal to `C:\Users\..` and `\\server\share\..`.
fn parts(path: &Path, platform: Platform) -> Vec<String> {
    let text = path.to_string_lossy();
    let split: Vec<&str> = if platform == Platform::Windows {
        let plain = text
            .strip_prefix(r"\\?\UNC\")
            .or_else(|| text.strip_prefix(r"\\?\"))
            .unwrap_or(&text);
        plain.split(['\\', '/']).collect()
    } else {
        text.split('/').collect()
    };
    split
        .into_iter()
        .filter(|c| !c.is_empty() && *c != ".")
        .map(str::to_owned)
        .collect()
}

/// `dir`'s components after `base`'s, compared exactly or without case.
fn strip<'a>(dir: &'a [String], base: &[String], fold: bool) -> Option<&'a [String]> {
    if dir.len() < base.len() {
        return None;
    }
    let (head, rest) = dir.split_at(base.len());
    head.iter()
        .zip(base)
        .all(|(a, b)| same(a, b, fold))
        .then_some(rest)
}

/// `base` with `extra` appended, in the platform's separator.
fn join_parts(base: &Path, extra: &[String], platform: Platform) -> PathBuf {
    let sep = if platform == Platform::Windows {
        '\\'
    } else {
        '/'
    };
    let mut out = base
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_owned();
    for part in extra {
        out.push(sep);
        out.push_str(part);
    }
    PathBuf::from(out)
}

fn same(a: &str, b: &str, fold: bool) -> bool {
    if fold {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
}

fn begins(name: &str, prefix: &str, fold: bool) -> bool {
    if fold {
        name.to_lowercase().starts_with(&prefix.to_lowercase())
    } else {
        name.starts_with(prefix)
    }
}

/// The longest existing prefix of `dir` with its links resolved, joined to
/// the part that does not exist yet.
fn resolve_existing(dir: &Path) -> Option<PathBuf> {
    let mut missing = Vec::new();
    let mut cur = dir;
    loop {
        if let Ok(real) = cur.canonicalize() {
            let mut out = real;
            for part in missing.iter().rev() {
                out.push(part);
            }
            return Some(out);
        }
        missing.push(cur.file_name()?.to_owned());
        cur = cur.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn windows_default_is_local_app_data_never_roaming() {
        let env = Environment::of(&[
            ("LOCALAPPDATA", r"C:\Users\ann\AppData\Local"),
            ("APPDATA", r"C:\Users\ann\AppData\Roaming"),
        ]);
        let dir = default_store_dir(Platform::Windows, &env).unwrap();
        assert_eq!(
            dir,
            p(r"C:\Users\ann\AppData\Local")
                .join("Tawara")
                .join("keystore")
        );
        assert_eq!(
            default_store_dir(Platform::Windows, &Environment::of(&[("APPDATA", r"C:\x")])),
            Err(NoDefaultLocation::Unset {
                variable: "LOCALAPPDATA"
            })
        );
    }

    #[test]
    fn macos_default_is_application_support() {
        let env = Environment::of(&[("HOME", "/Users/ann")]);
        assert_eq!(
            default_store_dir(Platform::MacOs, &env).unwrap(),
            p("/Users/ann/Library/Application Support/Tawara/keystore")
        );
    }

    #[test]
    fn linux_default_follows_xdg_and_ignores_a_relative_value() {
        let env = Environment::of(&[("HOME", "/home/ann"), ("XDG_DATA_HOME", "/data/ann")]);
        assert_eq!(
            default_store_dir(Platform::Linux, &env).unwrap(),
            p("/data/ann/tawara/keystore")
        );
        let relative = Environment::of(&[("HOME", "/home/ann"), ("XDG_DATA_HOME", "data")]);
        assert_eq!(
            default_store_dir(Platform::Linux, &relative).unwrap(),
            p("/home/ann/.local/share/tawara/keystore")
        );
        assert_eq!(
            default_store_dir(Platform::Linux, &Environment::default()),
            Err(NoDefaultLocation::Unset { variable: "HOME" })
        );
    }

    #[test]
    fn downloads_follow_each_desktop_convention() {
        let windows = Environment::of(&[("USERPROFILE", r"C:\Users\ann")]);
        assert_eq!(
            downloads_dir(Platform::Windows, &windows),
            Some(p(r"C:\Users\ann").join("Downloads"))
        );
        let mac = Environment::of(&[("HOME", "/Users/ann")]);
        assert_eq!(
            downloads_dir(Platform::MacOs, &mac),
            Some(p("/Users/ann/Downloads"))
        );
        assert_eq!(
            downloads_dir(Platform::MacOs, &Environment::default()),
            None
        );
        assert_eq!(downloads_dir(Platform::Android, &mac), None);
        assert_eq!(downloads_dir(Platform::Ios, &mac), None);
    }

    /// Linux reads `user-dirs.dirs` from the disk, so this one needs a home
    /// folder that is absolute by Linux's rules: a Unix host's.
    #[cfg(unix)]
    #[test]
    fn linux_downloads_are_what_user_dirs_names_or_the_default() {
        let home = std::env::temp_dir().join(format!("tawara-user-dirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".config")).unwrap();
        let home_text = home.display().to_string();
        let env = Environment::of(&[("HOME", home_text.as_str())]);
        assert_eq!(
            downloads_dir(Platform::Linux, &env),
            Some(home.join("Downloads"))
        );
        std::fs::write(
            home.join(".config").join("user-dirs.dirs"),
            "# written by xdg-user-dirs-update\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\n\
             XDG_DOWNLOAD_DIR=\"$HOME/Téléchargements\"\n",
        )
        .unwrap();
        assert_eq!(
            downloads_dir(Platform::Linux, &env),
            Some(home.join("Téléchargements"))
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn user_dirs_names_a_download_folder_or_nothing() {
        let home = Path::new("/home/ann");
        let read = |text: &str| user_dirs_download(text, home);
        assert_eq!(
            read("XDG_DOWNLOAD_DIR=\"$HOME/Downloads\""),
            Some(p("/home/ann/Downloads"))
        );
        assert_eq!(
            read("  XDG_DOWNLOAD_DIR=\"/data/ann/dl\"  "),
            Some(p("/data/ann/dl"))
        );
        // Turned off, relative, missing: none of them names a folder.
        assert_eq!(read("XDG_DOWNLOAD_DIR=\"$HOME/\""), None);
        assert_eq!(read("XDG_DOWNLOAD_DIR=\"dl\""), None);
        assert_eq!(read("XDG_MUSIC_DIR=\"$HOME/Music\""), None);
    }

    #[test]
    fn mobile_defaults_come_from_the_shell() {
        let env = Environment::of(&[("HOME", "/x")]);
        assert_eq!(
            default_store_dir(Platform::Android, &env),
            Err(NoDefaultLocation::ShellSupplies)
        );
        assert_eq!(
            default_store_dir(Platform::Ios, &env),
            Err(NoDefaultLocation::ShellSupplies)
        );
        assert_eq!(
            store_dir_in(Path::new("/data/user/0/app/no_backup")),
            p("/data/user/0/app/no_backup/keystore")
        );
    }

    #[test]
    fn the_desktop_defaults_raise_no_sync_warning() {
        for (platform, env) in [
            (Platform::MacOs, Environment::of(&[("HOME", "/Users/ann")])),
            (Platform::Linux, Environment::of(&[("HOME", "/home/ann")])),
        ] {
            let dir = default_store_dir(platform, &env).unwrap();
            assert_eq!(
                sync_warning_lexical(&dir, platform, &env),
                None,
                "{platform:?}"
            );
        }
        let env = Environment::of(&[
            ("LOCALAPPDATA", r"C:\Users\ann\AppData\Local"),
            ("APPDATA", r"C:\Users\ann\AppData\Roaming"),
            ("USERPROFILE", r"C:\Users\ann"),
            ("OneDrive", r"C:\Users\ann\OneDrive"),
        ]);
        let dir = default_store_dir(Platform::Windows, &env).unwrap();
        assert_eq!(sync_warning_lexical(&dir, Platform::Windows, &env), None);
    }

    #[test]
    fn macos_icloud_dropbox_and_onedrive_are_seen() {
        let env = Environment::of(&[("HOME", "/Users/ann")]);
        let w = sync_warning_lexical(
            Path::new("/Users/ann/Library/Mobile Documents/com~apple~CloudDocs/wallet"),
            Platform::MacOs,
            &env,
        )
        .unwrap();
        assert_eq!(w.service, "iCloud Drive");
        assert_eq!(w.folder, p("/Users/ann/Library/Mobile Documents"));
        let w =
            sync_warning_lexical(Path::new("/Users/ann/dropbox/w"), Platform::MacOs, &env).unwrap();
        assert_eq!(w.service, "Dropbox", "case is ignored on macOS");
        let w = sync_warning_lexical(
            Path::new("/Users/ann/OneDrive - Contoso/w"),
            Platform::MacOs,
            &env,
        )
        .unwrap();
        assert_eq!(w.service, "OneDrive");
        let w = sync_warning_lexical(Path::new("/Users/ann/Documents/w"), Platform::MacOs, &env)
            .unwrap();
        assert!(w.only_if_enabled);
        assert!(w.to_string().contains("may be synced"), "{w}");
    }

    #[test]
    fn windows_onedrive_variable_and_roaming_profile_are_seen() {
        let env = Environment::of(&[
            ("USERPROFILE", r"C:\Users\ann"),
            ("OneDriveCommercial", r"D:\Corp\OneDrive - Contoso"),
            ("APPDATA", r"C:\Users\ann\AppData\Roaming"),
        ]);
        let w = sync_warning_lexical(
            Path::new(r"D:\Corp\OneDrive - Contoso\w"),
            Platform::Windows,
            &env,
        )
        .unwrap();
        assert_eq!(w.service, "OneDrive");
        let w = sync_warning_lexical(
            Path::new(r"c:\users\ann\appdata\roaming\Tawara"),
            Platform::Windows,
            &env,
        )
        .unwrap();
        assert_eq!(w.service, "a roaming Windows profile");
        let w = sync_warning_lexical(
            Path::new(r"C:\Users\ann\Dropbox\w"),
            Platform::Windows,
            &env,
        )
        .unwrap();
        assert_eq!(w.service, "Dropbox");
    }

    #[test]
    fn absolute_is_judged_by_the_target_platform_not_the_host() {
        let cases: &[(&str, Platform, bool)] = &[
            ("/Users/ann", Platform::MacOs, true),
            ("/home/ann", Platform::Linux, true),
            ("/Users/ann", Platform::Windows, false),
            (r"C:\Users\ann", Platform::Windows, true),
            ("C:/Users/ann", Platform::Windows, true),
            (r"\\srv\share\ann", Platform::Windows, true),
            (r"\\?\C:\Users\ann", Platform::Windows, true),
            (r"C:\Users\ann", Platform::MacOs, false),
            (r"C:\Users\ann", Platform::Linux, false),
            ("C:", Platform::Windows, false),
            ("Users/ann", Platform::Linux, false),
            (r"Users\ann", Platform::Windows, false),
        ];
        for (text, platform, expected) in cases {
            assert_eq!(
                is_absolute_on(Path::new(text), *platform),
                *expected,
                "{text} on {platform:?}"
            );
        }
    }

    #[test]
    fn a_windows_verbatim_path_is_the_path_it_spells() {
        // What `canonicalize` returns for a folder reached through a link.
        let env = Environment::of(&[
            ("USERPROFILE", r"C:\Users\ann"),
            ("OneDrive", r"C:\Users\ann\OneDrive"),
        ]);
        let w = sync_warning_lexical(
            Path::new(r"\\?\C:\Users\ann\OneDrive\w"),
            Platform::Windows,
            &env,
        )
        .unwrap();
        assert_eq!(w.service, "OneDrive");
        assert_eq!(
            parts(Path::new(r"\\?\UNC\srv\share\w"), Platform::Windows),
            parts(Path::new(r"\\srv\share\w"), Platform::Windows)
        );
        assert_eq!(
            parts(Path::new(r"\\?\C:\Users"), Platform::Windows),
            ["C:", "Users"]
        );
    }

    #[test]
    fn linux_matches_case_exactly() {
        let env = Environment::of(&[("HOME", "/home/ann")]);
        assert_eq!(
            sync_warning_lexical(Path::new("/home/ann/Nextcloud/w"), Platform::Linux, &env)
                .unwrap()
                .service,
            "Nextcloud"
        );
        assert_eq!(
            sync_warning_lexical(Path::new("/home/ann/nextcloud/w"), Platform::Linux, &env),
            None
        );
        assert_eq!(
            sync_warning_lexical(Path::new("/home/ann/DropboxNot/w"), Platform::Linux, &env),
            None
        );
    }

    #[test]
    fn a_link_into_a_synced_folder_is_seen() {
        let root = std::env::temp_dir().join(format!("tawara-loc-{}", std::process::id()));
        let synced = root.join("home").join("Dropbox");
        std::fs::create_dir_all(&synced).unwrap();
        #[cfg(unix)]
        {
            let link = root.join("elsewhere");
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(&synced, &link).unwrap();
            let home = root.join("home").canonicalize().unwrap();
            let env = Environment::of(&[("HOME", home.to_str().unwrap())]);
            let w = sync_warning(&link.join("keystore"), Platform::Linux, &env).unwrap();
            assert_eq!(w.service, "Dropbox");
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
