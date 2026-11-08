// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The store identifier, which ties a password kept on the device to the store
//! it opens.
//!
//! A password kept in the system's secure storage can outlive the application
//! that kept it, since an uninstall may leave the item behind. So the item is
//! labelled with this identifier, and an item whose label is not the store's is
//! deleted, not used. The identifier is 32 random bytes, drawn once and kept in
//! the application's own folder beside `preferences`, which is where the
//! interface finds it. It is not secret: it names no password and opens nothing.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

/// The file's name in the application's folder, beside `preferences`.
pub const FILE_NAME: &str = "store-id";

/// The identifier's length in bytes.
pub const LEN: usize = 32;

/// The identifier in `app_private`, or `None` when there is none to read: no
/// file, or a file that is not [`LEN`] bytes long, which is no identifier at
/// all. A password is then kept under a new one, or none is released.
///
/// # Errors
///
/// The file is there but cannot be read.
pub fn read(app_private: &Path) -> io::Result<Option<[u8; LEN]>> {
    match std::fs::read(app_private.join(FILE_NAME)) {
        Ok(bytes) => Ok(<[u8; LEN]>::try_from(bytes.as_slice()).ok()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// The identifier in `app_private`: the one [`read`] finds, or, when it finds
/// none, a new one drawn from the operating system's generator and written in
/// its place. The new one is written to a file of its own and renamed over the
/// old, so a crash leaves the old file or the new one, never part of either.
/// The file is private to the user on Unix (mode `0600`), as the library makes
/// the files of a store.
///
/// # Errors
///
/// The generator failed, or the file could not be written. Nothing is kept
/// under an identifier then.
pub fn make(app_private: &Path) -> io::Result<[u8; LEN]> {
    if let Some(id) = read(app_private)? {
        return Ok(id);
    }
    let id = *crate::entropy::os_bytes::<LEN>().map_err(io::Error::other)?;
    write_private(app_private, &id)?;
    Ok(id)
}

/// Write `id` to [`FILE_NAME`] in `app_private`, through a staging file, which
/// is made new every time so that nothing left by an earlier write is kept.
fn write_private(app_private: &Path, id: &[u8; LEN]) -> io::Result<()> {
    let mut folders = std::fs::DirBuilder::new();
    folders.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        folders.mode(0o700);
    }
    folders.create(app_private)?;
    let path = app_private.join(FILE_NAME);
    let mut staging = path.as_os_str().to_owned();
    staging.push(".new");
    let staging = PathBuf::from(staging);
    let _ = std::fs::remove_file(&staging);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&staging)?;
    file.write_all(id)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&staging, &path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own for one test, from its name and the clock.
    fn folder(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "tawara-store-id-{name}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn nothing_is_read_before_an_identifier_is_made() {
        let dir = folder("none");
        assert_eq!(read(&dir).unwrap(), None);
        assert!(!dir.join(FILE_NAME).exists(), "reading makes no file");
    }

    #[test]
    fn the_identifier_is_drawn_once_and_read_back() {
        let dir = folder("once");
        let made = make(&dir).unwrap();
        assert_ne!(made, [0u8; LEN], "drawn, not zeroed");
        assert_eq!(read(&dir).unwrap(), Some(made));
        assert_eq!(make(&dir).unwrap(), made, "a second make keeps the first");
        assert_eq!(std::fs::read(dir.join(FILE_NAME)).unwrap(), made);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_stores_draw_different_identifiers() {
        let (a, b) = (folder("a"), folder("b"));
        assert_ne!(make(&a).unwrap(), make(&b).unwrap());
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    #[test]
    fn a_file_that_is_not_an_identifier_is_replaced() {
        let dir = folder("short");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE_NAME), [7u8; LEN - 1]).unwrap();
        assert_eq!(read(&dir).unwrap(), None, "31 bytes are no identifier");
        let made = make(&dir).unwrap();
        assert_eq!(read(&dir).unwrap(), Some(made));
        assert_eq!(std::fs::read(dir.join(FILE_NAME)).unwrap().len(), LEN);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_staging_file_left_by_a_crash_does_not_stop_a_new_identifier() {
        let dir = folder("staging");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{FILE_NAME}.new")), b"half written").unwrap();
        let made = make(&dir).unwrap();
        assert_eq!(read(&dir).unwrap(), Some(made));
        assert!(!dir.join(format!("{FILE_NAME}.new")).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn the_identifier_file_is_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = folder("mode");
        make(&dir).unwrap();
        let mode = std::fs::metadata(dir.join(FILE_NAME))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "owner read and write only");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
