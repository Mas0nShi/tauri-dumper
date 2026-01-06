//! Integration tests for tauri-dumper.
//!
//! These tests verify that the dumper works correctly across different:
//!   - Binary formats (Mach-O, PE, ELF)
//!   - CPU architectures (x64, aarch64)
//!
//! Fixtures are configured in `tests/fixtures/fixtures.toml`.
//! For local testing, run: `./scripts/download-fixtures.sh`

use std::fs::{self, File};
use std::path::PathBuf;
use std::sync::OnceLock;

use serde::Deserialize;
use tauri_dumper::Dumper;

// ============================================================================
// Configuration
// ============================================================================

#[derive(Debug, Deserialize)]
struct Config {
    fixture: Vec<Fixture>,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    name: String,
    format: String,
    arch: String,
    version: String,
    extract_dir: String,
    binary: String,
    expected_asset_count: usize,
    expected_index_html_size: usize,
}

impl Fixture {
    /// Unique identifier: `{name}-{format}-{arch}`
    fn id(&self) -> String {
        format!("{}-{}-{}", self.name, self.format, self.arch)
    }

    fn binary_path(&self) -> PathBuf {
        fixture_dir().join(&self.extract_dir).join(&self.binary)
    }
}

// ============================================================================
// Logging
// ============================================================================

mod log {
    pub fn skip(id: &str, reason: &str) {
        eprintln!("⏭️  [{id}] Skipped: {reason}");
    }

    pub fn success(id: &str, version: &str, assets: usize, index_size: usize) {
        eprintln!("✅ [{id}@{version}] {assets} assets, index.html = {index_size} bytes");
    }
}

// ============================================================================
// Infrastructure
// ============================================================================

fn fixture_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
}

fn load_config() -> &'static Config {
    static CONFIG: OnceLock<Config> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let path = fixture_dir().join("fixtures.toml");
        let content = fs::read_to_string(&path).expect("Failed to read fixtures.toml");
        toml::from_str(&content).expect("Failed to parse fixtures.toml")
    })
}

// ============================================================================
// Test Runner
// ============================================================================

/// Run asset extraction test on a single fixture.
/// Returns `true` if tested, `false` if skipped (binary not found).
fn test_fixture(f: &Fixture) -> bool {
    let id = f.id();
    let binary_path = f.binary_path();

    if !binary_path.exists() {
        log::skip(&id, "Binary not found. Run: ./scripts/download-fixtures.sh");
        return false;
    }

    let file = File::open(&binary_path).unwrap_or_else(|e| panic!("[{id}] Open failed: {e}"));
    let dumper = Dumper::new(file).unwrap_or_else(|e| panic!("[{id}] Dumper init failed: {e}"));
    let assets = dumper
        .scan_assets()
        .unwrap_or_else(|e| panic!("[{id}] Scan failed: {e}"));

    // Verify asset count
    assert_eq!(
        assets.len(),
        f.expected_asset_count,
        "[{id}] Asset count mismatch"
    );

    // Verify index.html
    let index_html = assets
        .iter()
        .find(|a| a.name == "/index.html")
        .unwrap_or_else(|| panic!("[{id}] index.html not found"));

    let decompressed = dumper
        .decompress_asset(index_html)
        .unwrap_or_else(|e| panic!("[{id}] Decompress failed: {e}"));

    assert_eq!(
        decompressed.len(),
        f.expected_index_html_size,
        "[{id}] index.html size mismatch"
    );

    let content = String::from_utf8_lossy(&decompressed);
    assert!(
        content.contains("<!DOCTYPE") || content.contains("<html"),
        "[{id}] Content doesn't look like HTML"
    );

    // Verify all asset paths are valid
    for asset in &assets {
        assert!(
            asset.name.starts_with('/'),
            "[{id}] Invalid path (no leading '/'): {}",
            asset.name
        );
        assert!(
            !asset.name.contains('\0'),
            "[{id}] Path contains null byte: {}",
            asset.name
        );
    }

    log::success(&id, &f.version, assets.len(), decompressed.len());
    true
}

// ============================================================================
// Tests
// ============================================================================

#[test]
fn test_all_fixtures() {
    let config = load_config();
    let total = config.fixture.len();
    let tested: usize = config
        .fixture
        .iter()
        .map(|f| test_fixture(f) as usize)
        .sum();

    // Ensure at least one fixture was tested if any binary exists
    let any_exists = config.fixture.iter().any(|f| f.binary_path().exists());
    if any_exists {
        assert!(tested > 0, "All fixtures failed despite binaries existing");
    }

    eprintln!("📊 Summary: {tested}/{total} fixtures tested");
}

// ============================================================================
// Edge Cases
// ============================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn reject_invalid_binary() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        fs::write(temp.path(), b"not a valid binary").unwrap();

        let file = File::open(temp.path()).unwrap();
        assert!(Dumper::new(file).is_err());
    }

    #[test]
    fn reject_empty_file() {
        let temp = tempfile::NamedTempFile::new().unwrap();

        let file = File::open(temp.path()).unwrap();
        assert!(Dumper::new(file).is_err());
    }
}
