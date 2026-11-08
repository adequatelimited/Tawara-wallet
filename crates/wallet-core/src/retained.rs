// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The signed bytes of each spend, kept beside the keystore so that a spend
//! reverted by a chain reorganisation can be resent without signing again.
//!
//! The keystore keeps a settled spend's digest and two figures, not its
//! signature (`keystore::format`, version 4), so the library can resend a
//! reverted settle only from a copy of its bytes
//! (`recon::resubmit_reverted_settle`). The worker writes that copy here as
//! soon as a spend is signed, before it is submitted, whether or not the user
//! saves one. Before anything is written the library checks the whole copy:
//! its digest, its signature, the chain's position for the account, the
//! balance and the block-to-live. No key signs to resend it.
//!
//! A copy is the spend's bytes, which the network sees once submitted, so it
//! is not secret. It lives in the keystore's folder under [`FOLDER`], one
//! file per spend, named by the account's tag and the signing key index. The
//! library reads only its own files there, so this folder does not affect it.
//!
//! A copy is released once the spend is final: when the keystore opens and
//! reads the account in sync, more than [`FINAL_DEPTH`] blocks have passed
//! since the height recorded with the copy, and the keystore no longer holds
//! the spend's digest, which it keeps until the account's next spend. The
//! copy of an account's last spend therefore stays until that next spend,
//! whatever height the node reports.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use mochimo_crypto::addr::Tag;

/// The folder inside the keystore's folder the copies are kept in.
pub const FOLDER: &str = "tawara-retained";

/// How many blocks after its send a spend is taken as final and its copy
/// released.
pub const FINAL_DEPTH: u64 = 256;

/// One kept copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Retained {
    /// The account it was spent from.
    pub tag: Tag,
    /// The key index that signed it.
    pub spent_index: u32,
    /// The chain's height when it was signed, when the node said.
    pub height: Option<u64>,
    /// Where it is kept.
    pub path: PathBuf,
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn file_name(tag: &Tag, spent_index: u32) -> String {
    format!("{}-{spent_index}.txt", hex(tag))
}

/// Keeps `wire`, the signed bytes of a spend from `tag` at key `spent_index`,
/// signed at `height`, in `dir`'s folder of copies. Written to a temporary
/// file and renamed over the name, so a copy is whole or absent. A copy
/// already kept with the same bytes is kept as it is, with its height: a
/// re-sign gives the same bytes again. A file under the same name holding
/// other bytes, such as one left by an earlier keystore in the same folder,
/// is replaced by `wire`.
pub fn keep(
    dir: &Path,
    tag: &Tag,
    spent_index: u32,
    height: Option<u64>,
    wire: &[u8],
) -> std::io::Result<()> {
    let folder = dir.join(FOLDER);
    let path = folder.join(file_name(tag, spent_index));
    if let Ok(text) = fs::read_to_string(&path)
        && fields(&text).1.as_deref() == Some(hex(wire).as_str())
    {
        return Ok(());
    }
    write(&folder, &path, height, wire)
}

fn write(folder: &Path, path: &Path, height: Option<u64>, wire: &[u8]) -> std::io::Result<()> {
    fs::create_dir_all(folder)?;
    let temp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&temp)?;
        let height = height.map_or_else(String::new, |h| h.to_string());
        write!(file, "height={height}\nbytes={}\n", hex(wire))?;
        file.sync_all()?;
    }
    fs::rename(&temp, path)
}

fn parse_name(name: &str) -> Option<(Tag, u32)> {
    let stem = name.strip_suffix(".txt")?;
    let (tag, index) = stem.split_once('-')?;
    let bytes = mochimo_crypto::mesh::hex::decode(tag, "tag").ok()?;
    let tag: Tag = bytes.try_into().ok()?;
    Some((tag, index.parse().ok()?))
}

fn fields(text: &str) -> (Option<u64>, Option<String>) {
    let mut height = None;
    let mut bytes = None;
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("height=") {
            height = h.trim().parse().ok();
        } else if let Some(b) = line.strip_prefix("bytes=") {
            bytes = Some(b.trim().to_owned());
        }
    }
    (height, bytes)
}

/// Every copy kept in `dir`'s folder of copies. A file whose name is not a
/// copy's is passed over.
#[must_use]
pub fn list(dir: &Path) -> Vec<Retained> {
    let Ok(entries) = fs::read_dir(dir.join(FOLDER)) else {
        return Vec::new();
    };
    let mut out: Vec<Retained> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let (tag, spent_index) = parse_name(&name)?;
            let (height, _) = fields(&fs::read_to_string(e.path()).ok()?);
            Some(Retained {
                tag,
                spent_index,
                height,
                path: e.path(),
            })
        })
        .collect();
    out.sort_by_key(|r| (r.tag, r.spent_index));
    out
}

/// The signed bytes of the copy kept for `tag`'s spend at `spent_index`, as
/// hex, if one is kept.
#[must_use]
pub fn bytes_of(dir: &Path, tag: &Tag, spent_index: u32) -> Option<String> {
    let text = fs::read_to_string(dir.join(FOLDER).join(file_name(tag, spent_index))).ok()?;
    fields(&text).1.filter(|b| !b.is_empty())
}

/// Records `height` for a copy kept with none, so that its age can be told
/// from then.
pub fn set_height(copy: &Retained, height: u64) -> std::io::Result<()> {
    let text = fs::read_to_string(&copy.path)?;
    let (_, bytes) = fields(&text);
    let wire = mochimo_crypto::mesh::hex::decode(bytes.as_deref().unwrap_or_default(), "copy")
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let folder = copy
        .path
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf);
    write(&folder, &copy.path, Some(height), &wire)
}

/// Deletes a copy.
pub fn release(copy: &Retained) -> std::io::Result<()> {
    fs::remove_file(&copy.path)
}

/// Whether a copy signed at `height` is final at the chain's `tip`: more
/// than [`FINAL_DEPTH`] blocks on.
#[must_use]
pub fn is_final(height: u64, tip: u64) -> bool {
    tip.saturating_sub(height) > FINAL_DEPTH
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(n: u8) -> Tag {
        [n; 20]
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tawara-retained-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_copy_is_kept_read_listed_aged_and_let_go() {
        let dir = scratch("cycle");
        keep(&dir, &tag(1), 49, Some(1_000), &[0xab, 0xcd]).unwrap();
        keep(&dir, &tag(2), 7, None, &[0x01]).unwrap();
        assert_eq!(bytes_of(&dir, &tag(1), 49).as_deref(), Some("abcd"));
        assert_eq!(bytes_of(&dir, &tag(1), 50), None);

        // A re-sign keeps the copy as it was, its height with it.
        keep(&dir, &tag(1), 49, Some(2_000), &[0xab, 0xcd]).unwrap();
        assert_eq!(list(&dir)[0].height, Some(1_000));
        let kept = list(&dir);
        assert_eq!(kept.len(), 2);
        assert_eq!(
            (kept[0].tag, kept[0].spent_index, kept[0].height),
            (tag(1), 49, Some(1_000))
        );
        assert_eq!(kept[1].height, None);

        // A height recorded later.
        set_height(&kept[1], 5_000).unwrap();
        assert_eq!(list(&dir)[1].height, Some(5_000));
        assert_eq!(bytes_of(&dir, &tag(2), 7).as_deref(), Some("01"));

        // Final after 256 blocks, not at them.
        assert!(!is_final(1_000, 1_256));
        assert!(is_final(1_000, 1_257));

        release(&kept[0]).unwrap();
        assert_eq!(list(&dir).len(), 1);
        // A stray file is passed over.
        fs::write(dir.join(FOLDER).join("notes.txt"), "x").unwrap();
        assert_eq!(list(&dir).len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_with_other_bytes_under_the_name_is_replaced() {
        let dir = scratch("replace");
        keep(&dir, &tag(3), 7, Some(1_000), &[0x11, 0x22]).unwrap();
        keep(&dir, &tag(3), 7, Some(9_000), &[0x33]).unwrap();
        assert_eq!(bytes_of(&dir, &tag(3), 7).as_deref(), Some("33"));
        let kept = list(&dir);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].height, Some(9_000));
        // A file with no bytes under the name is replaced as well.
        fs::write(&kept[0].path, "height=5\n").unwrap();
        keep(&dir, &tag(3), 7, None, &[0x44]).unwrap();
        assert_eq!(bytes_of(&dir, &tag(3), 7).as_deref(), Some("44"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_folder_is_no_copies() {
        let dir = scratch("none");
        assert!(list(&dir).is_empty());
        assert_eq!(bytes_of(&dir, &tag(1), 1), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
