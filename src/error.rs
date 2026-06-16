use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("binary parse error: {0}")]
    Object(#[from] object::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("unsupported binary format: {0}")]
    UnsupportedFormat(String),

    #[error("no supported Tauri asset section found in {0}")]
    NoAssetSection(String),

    #[error("pointer offset out of bounds")]
    PointerOutOfBounds,

    #[error("scan range exceeds file bounds")]
    ScanRangeOutOfBounds,

    #[error("virtual address {0:#X} is not mapped to a file-backed section")]
    AddressNotMapped(u64),

    #[error("invalid asset header at {offset:#X}: {reason}")]
    InvalidAssetHeader { offset: usize, reason: String },

    #[error("asset name is invalid")]
    InvalidAssetName,

    #[error("asset data is not valid Brotli")]
    InvalidBrotli,

    #[error("asset path escapes output directory: {asset}")]
    PathTraversal { asset: String },

    #[error("output already exists: {0}")]
    OutputExists(PathBuf),

    #[error("source binary does not match manifest: expected {expected}, found {actual}")]
    SourceMismatch { expected: String, actual: String },

    #[error("replacement for {asset} is too large: {new_size} bytes > {max_size} bytes")]
    ReplacementTooLarge {
        asset: String,
        new_size: usize,
        max_size: usize,
    },

    #[error("replacement directory contains unsupported new asset: {0}")]
    UnsupportedAddition(PathBuf),

    #[error("manifest error: {0}")]
    Manifest(String),

    #[error("{0}")]
    Message(String),
}
