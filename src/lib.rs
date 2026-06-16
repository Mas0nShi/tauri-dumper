//! Library for extracting and replacing embedded Tauri assets.

pub mod asset;
pub mod binary;
pub mod codec;
pub mod dumper;
pub mod error;
pub mod export;
pub mod extract;
pub mod image;
pub mod manifest;
pub mod repack;

pub use asset::{Asset, AssetId, AssetLocation, AssetTable};
pub use dumper::Dumper;
pub use error::{Error, Result};
pub use export::{ExportOptions, ExportSummary};
pub use extract::AssetScanner;
pub use image::BinaryImage;
pub use repack::{RepackSummary, Repacker};
