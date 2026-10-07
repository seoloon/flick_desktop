//! IPC commands.
//!
//! A plain `#[tauri::command] fn` runs on the main thread, which also runs
//! the window and the WebView: anything slow there freezes the UI. Commands
//! that touch the keychain, the disk, argon2 (PINs) or mpv's synchronous API
//! are `async` (or `#[tauri::command(async)]`). Only cheap commands whose
//! order matters stay synchronous: player commands and the video viewport
//! (a seek or a resize applied out of order would be wrong), and window state.

pub mod admin;
pub mod cast;
pub mod catalog;
pub mod downloads;
pub mod flicksync;
pub mod offline;
pub mod people;
pub mod playback;
pub mod profiles;
pub mod servers;
pub mod system;
pub mod updates;
