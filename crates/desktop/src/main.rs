// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! Tawara for Windows, macOS and Linux.

// Release builds on Windows use the windows subsystem so no console window
// opens. Debug builds keep the console.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = tawara_app::run() {
        let text = format!("{}: {error}", tawara_app::DISPLAY_NAME);
        eprintln!("{text}");
        // Without a console, report the start failure in a system dialog.
        tawara_app::show_start_failure(&text);
        std::process::exit(1);
    }
}
