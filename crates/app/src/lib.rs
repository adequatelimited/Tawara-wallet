// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The Tawara application: iced state, messages, `update`, `view`,
//! subscriptions, screens and theme.
//!
//! It never calls the wallet library. Every library call goes through
//! `tawara-wallet-core`'s worker thread, and what comes back is a view model
//! that holds no secret. Where a design choice conflicts with the wallet's
//! security rules, the security rules win.
//!
//! - [`app`]: the model, the messages, the worker bridge and `update`.
//! - [`host`]: what a mobile shell supplies: notice when the application
//!   leaves the foreground, notice of screen capture, and the device unlock
//!   where the platform offers one.
//! - [`screens`]: every screen, a function of the model alone, so that the
//!   screenshot example draws any screen from sample data.
//! - [`paste`]: a phone's Paste button, and the cleaning of a pasted
//!   recovery phrase.
//! - [`scan`]: the application side of the QR scanner: the scanner a shell
//!   provides, and what a scanned text must be before a destination row
//!   takes it.
//! - [`tap`]: a row or card that acts on release, so touch scrolling works.
//! - [`overflow`]: the check that nothing a phone screen draws passes the
//!   edge of its window.
//! - [`theme`], [`fonts`], [`icon`], [`ui`]: the design tokens, the bundled
//!   faces, the line icons and the pieces the screens are built from.

pub mod app;
pub mod folder;
pub mod fonts;
pub mod history;
pub mod host;
pub mod icon;
mod keyboard;
pub mod overflow;
pub mod paste;
pub mod scan;
pub mod screens;
pub mod tap;
pub mod theme;
pub mod ui;

/// The test harness: an interface drawn and driven by hand, for the tests of
/// the widgets and screens.
#[cfg(test)]
mod harness;

/// The application's display name.
pub const DISPLAY_NAME: &str = "Tawara";

/// The window's size at start, and the smallest it may be (the medium width
/// class).
pub const WINDOW: (f32, f32) = (1440.0, 900.0);
pub const WINDOW_MIN: (f32, f32) = (1024.0, 700.0);

pub use host::{Host, Leaving, capture_changed, left_foreground, screenshot_taken};

/// Run the application in a mobile shell (`crates/mobile`): Android or iOS,
/// in the window the system gives it, with what `host` supplies.
pub fn run_in(host: Host) -> iced::Result {
    let mut application = iced::application(
        move || app::App::boot_in(&host),
        app::App::update,
        app::App::view,
    )
    .subscription(app::App::subscription)
    .theme(|_: &app::App| theme::theme())
    .default_font(fonts::BODY)
    .title(DISPLAY_NAME);
    for bytes in fonts::FILES {
        application = application.font(bytes);
    }
    application.run()
}

/// Says why Tawara could not start, in a system error dialog: a release build
/// on Windows has no console to print to.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn show_start_failure(text: &str) {
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(DISPLAY_NAME)
        .set_description(text)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

/// Run the application on the desktop.
pub fn run() -> iced::Result {
    let mut application = iced::application(app::App::boot, app::App::update, app::App::view)
        .subscription(app::App::subscription)
        .theme(|_: &app::App| theme::theme())
        .default_font(fonts::BODY)
        .title(DISPLAY_NAME)
        .window_size(WINDOW)
        .window(iced::window::Settings {
            size: iced::Size::new(WINDOW.0, WINDOW.1),
            min_size: Some(iced::Size::new(WINDOW_MIN.0, WINDOW_MIN.1)),
            ..iced::window::Settings::default()
        });
    for bytes in fonts::FILES {
        application = application.font(bytes);
    }
    application.run()
}
