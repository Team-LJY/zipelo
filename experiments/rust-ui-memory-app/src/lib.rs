#![forbid(unsafe_code)]

#[cfg(all(feature = "ui-fixture", debug_assertions))]
extern crate self as zipelo_ui_memory_app;

pub mod archive_ui;

#[cfg(feature = "ui")]
pub mod archive_screen;

pub mod archive_process;

pub mod archive_present;

pub mod archive_browser;
#[cfg(feature = "ui")]
pub mod archive_dialog;
#[cfg(feature = "ui")]
pub mod archive_theme;
#[cfg(feature = "ui")]
pub mod window_frame;

pub mod archive_path;

#[cfg(feature = "ui")]
pub mod app_icon;
