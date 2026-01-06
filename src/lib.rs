//! Tauri asset dumper library.
//!
//! This library provides functionality to extract embedded assets from Tauri application binaries.

pub mod binary;
pub mod dumper;

pub use dumper::{Asset, Dumper};
