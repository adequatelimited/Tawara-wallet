// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! What the application remembers between runs: the node the person chose,
//! the store they last made or opened, the unit amounts are shown in, and the
//! auto-lock period.
//!
//! Nothing secret is kept here, and nothing the library reads: the file is
//! `preferences` in the application's folder, beside the store's own
//! `keystore` folder and never inside it. It is a few `key = value` lines,
//! so a person can read it. A line this version does not know, or a value
//! it cannot use, is ignored and the default stands; a damaged file costs
//! the person their choices, never the application its start.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::location::{Environment, NoDefaultLocation, Platform, default_store_dir};

/// The file's name in the application's folder.
pub const FILE_NAME: &str = "preferences";

/// The node a person who has not chosen one is given, so that the wallet
/// works for someone who does not know what a node is. It is the default of
/// [`Preferences::node`] and no more: a node the
/// person chose, or [`FORGOTTEN_NODE`] for one they forgot, is kept, and
/// nothing puts this default back over either. It is never written to the
/// file, so that a person who has not chosen keeps following the default if
/// a later version changes it (a new host, or a compromised one).
pub const DEFAULT_NODE: &str = "https://api.mochimo.org";

/// The word the preferences file holds for no node, as the person leaves it
/// by forgetting the node. A node line that is
/// absent or empty means [`DEFAULT_NODE`], not no node, so forgetting needs
/// a word of its own, and that word survives a restart. It is named apart
/// from the worker's notice for a store opened with no node, which says
/// something else.
pub const FORGOTTEN_NODE: &str = "none";

/// The auto-lock periods offered, in minutes. There is no "never": an
/// unlocked store holds the seed.
pub const IDLE_LOCK_MINUTES: [u64; 5] = [1, 2, 5, 10, 15];

/// The unit amounts are shown in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AmountUnit {
    /// MCM, with nine decimal places.
    #[default]
    Mcm,
    /// Whole nanoMCM.
    NanoMcm,
}

/// Whether the store is unlocked with the device's own check, and whether the
/// person has been offered it. It is off
/// until the person accepts an offer, which comes once, after a password
/// unlock. The offer's being made is kept here too, so that it is not made
/// again. The password the device keeps is in the system's secure storage and
/// never in this file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeviceUnlock {
    /// Not offered yet. Nothing is written for it, so an absent line reads as
    /// this.
    #[default]
    Unoffered,
    /// Offered, and not taken, or turned off. No password is kept.
    Off,
    /// The store's password is kept, and released only by the device's check.
    On,
}

/// What is remembered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preferences {
    /// The node last chosen, as typed, or `None` when the person forgot it.
    /// It starts as [`DEFAULT_NODE`]. The worker
    /// checks it again when it is set (`Command::SetNode`), so a stored value
    /// is never trusted.
    pub node: Option<String>,
    /// The folder of the store last made or opened, as an absolute path, so
    /// the application offers to unlock it at the next start wherever it
    /// is. A path, nothing more: the library checks the folder again when
    /// it is opened.
    pub store: Option<String>,
    pub unit: AmountUnit,
    /// One of [`IDLE_LOCK_MINUTES`].
    pub idle_lock: Duration,
    /// Whether the store is unlocked with the device's own check, and whether
    /// that was offered (see [`DeviceUnlock`]).
    pub device_unlock: DeviceUnlock,
}

impl Default for Preferences {
    fn default() -> Preferences {
        Preferences {
            node: Some(DEFAULT_NODE.to_owned()),
            store: None,
            unit: AmountUnit::Mcm,
            idle_lock: crate::DEFAULT_IDLE_LOCK,
            device_unlock: DeviceUnlock::Unoffered,
        }
    }
}

/// The preferences file for the default location: `preferences` beside the
/// default store's folder.
pub fn default_file(platform: Platform, env: &Environment) -> Result<PathBuf, NoDefaultLocation> {
    let store = default_store_dir(platform, env)?;
    Ok(store
        .parent()
        .map_or_else(|| PathBuf::from(FILE_NAME), |app| app.join(FILE_NAME)))
}

/// Read preferences from text, line by line. Anything not understood is
/// left at its default.
#[must_use]
pub fn parse(text: &str) -> Preferences {
    let mut out = Preferences::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            // An empty value is not a node, so the default stands, as for any
            // line not understood. `none` is what a forgotten node is written
            // as.
            "node" => match value {
                "" => {}
                FORGOTTEN_NODE => out.node = None,
                node => out.node = Some(node.to_owned()),
            },
            "store" if !value.is_empty() => out.store = Some(value.to_owned()),
            "unit" => match value {
                "mcm" => out.unit = AmountUnit::Mcm,
                "nanomcm" => out.unit = AmountUnit::NanoMcm,
                _ => {}
            },
            "auto-lock-minutes" => {
                if let Ok(m) = value.parse::<u64>()
                    && IDLE_LOCK_MINUTES.contains(&m)
                {
                    out.idle_lock = Duration::from_secs(m * 60);
                }
            }
            // Only the two words are written; any other value is not understood,
            // and the offer stands as it was (see `DeviceUnlock`).
            "device-unlock" => match value {
                "off" => out.device_unlock = DeviceUnlock::Off,
                "on" => out.device_unlock = DeviceUnlock::On,
                _ => {}
            },
            _ => {}
        }
    }
    out
}

/// The file's text for `prefs`.
#[must_use]
pub fn render(prefs: &Preferences) -> String {
    let mut out = String::from(
        "# Tawara's preferences. Nothing secret is kept here, and nothing the wallet\n\
         # library reads; deleting this file resets them.\n",
    );
    // One line each: a line break would end the value and start a new key.
    let one_line = |s: &str| -> String { s.chars().filter(|c| !c.is_control()).collect() };
    // A forgotten node is written as `none`, since an absent line reads back
    // as the default node. The default itself is not written: a file that
    // named it would hold it as a choice, and a later version's default would
    // never reach the person who had not chosen. A person who types the
    // default's own address is treated as following the default.
    match prefs.node.as_deref() {
        Some(DEFAULT_NODE) => {}
        Some(node) => out.push_str(&format!("node = {}\n", one_line(node))),
        None => out.push_str(&format!("node = {FORGOTTEN_NODE}\n")),
    }
    if let Some(store) = &prefs.store {
        out.push_str(&format!("store = {}\n", one_line(store)));
    }
    out.push_str(match prefs.unit {
        AmountUnit::Mcm => "unit = mcm\n",
        AmountUnit::NanoMcm => "unit = nanomcm\n",
    });
    out.push_str(&format!(
        "auto-lock-minutes = {}\n",
        prefs.idle_lock.as_secs() / 60
    ));
    // Unoffered is written as nothing, so a device that never offers the
    // unlock (the desktop, Android) keeps the file it had.
    match prefs.device_unlock {
        DeviceUnlock::Unoffered => {}
        DeviceUnlock::Off => out.push_str("device-unlock = off\n"),
        DeviceUnlock::On => out.push_str("device-unlock = on\n"),
    }
    out
}

/// The preferences in `path`, or the defaults when it does not exist. See
/// [`load_checked`] for a file that is there but cannot be read.
#[must_use]
pub fn load(path: &Path) -> Preferences {
    load_checked(path).prefs
}

/// What [`load_checked`] read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    /// The preferences to start with.
    pub prefs: Preferences,
    /// The file is there but could not be read. It may hold a node the
    /// person chose, or `none` for one they forgot, so the caller should not
    /// write over it for an ordinary change: only once the person chooses or
    /// forgets a node again.
    pub unreadable: bool,
}

/// The preferences in `path`, and whether a file there could not be read.
///
/// A file that does not exist gives the defaults, with [`DEFAULT_NODE`]. A
/// file that is there but cannot be read gives the defaults with no node:
/// the default must never take the place of a node the person chose, or of
/// one they forgot. With no node the wallet says so, and the person can
/// choose again.
#[must_use]
pub fn load_checked(path: &Path) -> Loaded {
    match std::fs::read_to_string(path) {
        Ok(text) => Loaded {
            prefs: parse(&text),
            unreadable: false,
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Loaded {
            prefs: Preferences::default(),
            unreadable: false,
        },
        Err(_) => Loaded {
            prefs: Preferences {
                node: None,
                ..Preferences::default()
            },
            unreadable: true,
        },
    }
}

/// Write `prefs` to `path`, replacing the file whole: written to a new file
/// beside it, flushed, then renamed over it, so a crash leaves the old file
/// or the new one and never half of one. The folder is made when missing,
/// private to the user on Unix, as the worker makes it for a store.
pub fn save(path: &Path, prefs: &Preferences) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let mut folders = std::fs::DirBuilder::new();
        folders.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            folders.mode(0o700);
        }
        folders.create(parent)?;
    }
    let mut staging = path.as_os_str().to_owned();
    staging.push(".new");
    let staging = PathBuf::from(staging);
    let mut file = std::fs::File::create(&staging)?;
    file.write_all(render(prefs).as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&staging, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_written_reads_back() {
        let prefs = Preferences {
            node: Some("https://node.example".into()),
            store: Some("/home/p/wallets/savings".into()),
            unit: AmountUnit::NanoMcm,
            idle_lock: Duration::from_secs(10 * 60),
            device_unlock: DeviceUnlock::On,
        };
        assert_eq!(parse(&render(&prefs)), prefs);
        assert_eq!(
            parse(&render(&Preferences::default())),
            Preferences::default()
        );
    }

    #[test]
    fn the_device_unlock_is_written_only_once_it_is_offered() {
        // Unoffered writes nothing, so a shell that never offers the unlock keeps
        // the file it had.
        assert!(!render(&Preferences::default()).contains("device-unlock"));
        for (state, line) in [
            (DeviceUnlock::Off, "device-unlock = off\n"),
            (DeviceUnlock::On, "device-unlock = on\n"),
        ] {
            let prefs = Preferences {
                device_unlock: state,
                ..Preferences::default()
            };
            assert!(render(&prefs).contains(line), "{state:?}");
            assert_eq!(parse(&render(&prefs)).device_unlock, state);
        }
    }

    #[test]
    fn an_absent_or_unknown_device_unlock_line_is_not_offered() {
        assert_eq!(parse("").device_unlock, DeviceUnlock::Unoffered);
        for text in [
            "device-unlock = yes\n",
            "device-unlock = \n",
            "device-unlock = ON\n",
        ] {
            assert_eq!(
                parse(text).device_unlock,
                DeviceUnlock::Unoffered,
                "{text:?} is not understood"
            );
        }
    }

    #[test]
    fn what_is_not_understood_keeps_the_defaults() {
        let prefs = parse(
            "garbage\nunit = parsecs\nauto-lock-minutes = 0\nauto-lock-minutes = \
             never\nauto-lock-minutes = 7\ncolour = green\n",
        );
        assert_eq!(prefs, Preferences::default());
    }

    #[test]
    fn a_fresh_start_chooses_the_default_node() {
        assert_eq!(Preferences::default().node.as_deref(), Some(DEFAULT_NODE));
        assert_eq!(parse(""), Preferences::default(), "no file is the defaults");
        assert_eq!(
            parse("unit = nanomcm\n").node.as_deref(),
            Some(DEFAULT_NODE),
            "a file with no node line chooses the default API"
        );
    }

    #[test]
    fn an_empty_node_line_keeps_the_default_node_and_is_not_none() {
        for text in ["node =\n", "node =    \n"] {
            assert_eq!(parse(text).node.as_deref(), Some(DEFAULT_NODE), "{text:?}");
        }
    }

    #[test]
    fn none_is_no_node_and_reads_back_as_none() {
        assert_eq!(parse("node = none\n").node, None);
        let forgotten = Preferences {
            node: None,
            ..Preferences::default()
        };
        let text = render(&forgotten);
        assert!(text.contains("node = none\n"), "{text}");
        assert_eq!(parse(&text), forgotten);
    }

    #[test]
    fn a_chosen_node_is_kept_and_is_not_the_default() {
        let chosen = parse("node = https://mine.example\n");
        assert_eq!(chosen.node.as_deref(), Some("https://mine.example"));
        assert_eq!(parse(&render(&chosen)), chosen);
    }

    #[test]
    fn forgetting_the_node_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("tawara-prefs-forget-{}", std::process::id()));
        let path = dir.join("app").join(FILE_NAME);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            load(&path).node.as_deref(),
            Some(DEFAULT_NODE),
            "until the person chooses, the default is in use"
        );
        let forgotten = Preferences {
            node: None,
            ..Preferences::default()
        };
        save(&path, &forgotten).unwrap();
        assert_eq!(load(&path).node, None, "the next start still has no API");
        let chosen = Preferences {
            node: Some("https://mine.example".into()),
            ..Preferences::default()
        };
        save(&path, &chosen).unwrap();
        assert_eq!(load(&path), chosen, "an API chosen after it persists");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_cannot_be_read_gives_no_node_not_the_default() {
        let dir = std::env::temp_dir().join(format!("tawara-prefs-unread-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // A folder where the file should be, and a file that is not text:
        // each is there, and neither can be read as preferences.
        let folder = dir.join("folder").join(FILE_NAME);
        std::fs::create_dir_all(&folder).unwrap();
        let binary = dir.join(FILE_NAME);
        std::fs::write(&binary, b"node = none\n\xff\xfe\n").unwrap();
        for path in [&folder, &binary] {
            assert_eq!(
                load_checked(path),
                Loaded {
                    prefs: Preferences {
                        node: None,
                        ..Preferences::default()
                    },
                    unreadable: true,
                },
                "{}",
                path.display()
            );
        }
        assert!(
            !load_checked(&dir.join("missing")).unreadable,
            "a file that is not there is not unreadable"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_default_node_is_not_written() {
        // Nothing chosen: no node line, so a later default reaches the person.
        let text = render(&Preferences::default());
        assert!(!text.contains("node"), "{text}");
        assert_eq!(parse(&text), Preferences::default());
        // A file from before, naming the default, reads as the default and
        // is written again without it.
        let named = parse(&format!("node = {DEFAULT_NODE}\n"));
        assert_eq!(named, Preferences::default());
        assert!(!render(&named).contains("node"));
        // A node the person chose, and a forgotten one, are still written.
        let chosen = Preferences {
            node: Some("https://mine.example".into()),
            ..Preferences::default()
        };
        assert!(render(&chosen).contains("node = https://mine.example\n"));
        let forgotten = Preferences {
            node: None,
            ..Preferences::default()
        };
        assert!(render(&forgotten).contains("node = none\n"));
    }

    #[test]
    fn a_node_with_a_line_break_cannot_add_a_key() {
        let prefs = Preferences {
            node: Some("https://a.example\nunit = nanomcm".into()),
            ..Preferences::default()
        };
        let back = parse(&render(&prefs));
        assert_eq!(back.unit, AmountUnit::Mcm);
        assert_eq!(
            back.node.as_deref(),
            Some("https://a.exampleunit = nanomcm")
        );
    }

    #[test]
    fn the_file_sits_beside_the_default_store() {
        let env = Environment::of(&[("HOME", "/home/p")]);
        assert_eq!(
            default_file(Platform::Linux, &env).ok(),
            Some(PathBuf::from("/home/p/.local/share/tawara/preferences"))
        );
    }

    #[test]
    fn saving_replaces_the_file_whole() {
        let dir = std::env::temp_dir().join(format!("tawara-prefs-{}", std::process::id()));
        let path = dir.join("app").join(FILE_NAME);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            load(&path),
            Preferences::default(),
            "no file is the defaults"
        );
        let prefs = Preferences {
            node: Some("https://node.example".into()),
            ..Preferences::default()
        };
        save(&path, &prefs).unwrap();
        assert_eq!(load(&path), prefs);
        save(&path, &Preferences::default()).unwrap();
        assert_eq!(load(&path), Preferences::default());
        assert!(!path.with_file_name("preferences.new").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
