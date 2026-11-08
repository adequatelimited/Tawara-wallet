// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The desktop folder dialog behind the Browse button on the keystore folder
//! screens. A phone keeps its store in the app's own folder, so on Android
//! and iOS there is no dialog and no Browse.
//!
//! The dialog is the system's own: IFileOpenDialog on Windows, NSOpenPanel on
//! macOS, and the desktop portal's file chooser on Linux. Its result is put
//! in the folder field as if typed, so the same checks apply. Closing the
//! dialog without a choice leaves the field unchanged.

use std::path::{Path, PathBuf};

use iced::Task;

use crate::app::Message;

/// Whether this build offers Browse: desktop builds do, phone builds do not.
pub const OFFERED: bool = cfg!(not(any(target_os = "android", target_os = "ios")));

/// Where the dialog opens: the field's folder if it exists, else the nearest
/// existing ancestor. The default folder is not created until the phrase is
/// confirmed.
fn start(typed: &str) -> Option<PathBuf> {
    let mut at = Path::new(typed.trim());
    loop {
        if at.as_os_str().is_empty() {
            return None;
        }
        if at.is_dir() {
            return Some(at.to_path_buf());
        }
        at = at.parent()?;
    }
}

/// Opens the folder dialog at `typed`, and answers with [`Message::Dir`] when
/// a folder is chosen, or with nothing when the dialog is closed.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn pick(typed: &str) -> Task<Message> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title("Choose the keystore's folder");
    if let Some(dir) = start(typed) {
        dialog = dialog.set_directory(dir);
    }
    Task::future(async move {
        dialog
            .pick_folder()
            .await
            .map(|chosen| chosen.path().display().to_string())
    })
    .and_then(|dir| Task::done(Message::Dir(dir)))
}

/// Phones have no folder dialog; returns no task.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn pick(_typed: &str) -> Task<Message> {
    let _ = start;
    Task::none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_opens_at_the_nearest_folder_that_exists() {
        let here = std::env::temp_dir();
        let missing = here.join("tawara-folder-test-not-made").join("keystore");
        assert_eq!(start(&missing.display().to_string()), Some(here.clone()));
        assert_eq!(start(&here.display().to_string()), Some(here));
        assert_eq!(start("   "), None);
    }
}
