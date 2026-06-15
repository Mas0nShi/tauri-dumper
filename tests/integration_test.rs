//! Integration tests for tauri-dumper.
//!
//! These tests verify that the dumper works correctly across different:
//!   - Binary formats (Mach-O, PE, ELF)
//!   - CPU architectures (x64, aarch64)
//!
//! Fixtures are configured in `tests/fixtures/fixtures.toml`.
//! For local testing, run: `./scripts/download-fixtures.sh`

use std::fs::{self, File};
use std::io::Write;
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
    fn extracts_assets_from_android_elf_with_rela_pointer_addends() {
        let elf = make_android_tauri_elf_fixture();
        let temp = tempfile::NamedTempFile::new().unwrap();
        fs::write(temp.path(), elf).unwrap();

        let file = File::open(temp.path()).unwrap();
        let dumper = Dumper::new(file).unwrap();
        let assets = dumper.scan_assets().unwrap();

        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].name, "/index.html");

        let decompressed = dumper.decompress_asset(&assets[0]).unwrap();
        assert_eq!(decompressed, b"<!DOCTYPE html><html></html>");
    }

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

    fn make_android_tauri_elf_fixture() -> Vec<u8> {
        const ELF_HEADER_SIZE: usize = 64;
        const PROGRAM_HEADER_SIZE: usize = 56;
        const SECTION_HEADER_SIZE: usize = 64;

        const RODATA_ADDR: u64 = 0x1000;
        const RELA_ADDR: u64 = 0x2000;
        const DATA_REL_RO_ADDR: u64 = 0x3000;

        const RODATA_OFF: usize = 0x1000;
        const RELA_OFF: usize = 0x2000;
        const DATA_REL_RO_OFF: usize = 0x3000;
        const SHSTRTAB_OFF: usize = 0x4000;

        let html = b"<!DOCTYPE html><html></html>";
        let compressed = brotli_compress(html);

        let mut rodata = Vec::new();
        rodata.extend_from_slice(b"/index.html");
        let data_addr = RODATA_ADDR + rodata.len() as u64;
        rodata.extend_from_slice(&compressed);

        let mut data_rel_ro = vec![0; 32];
        write_u64(&mut data_rel_ro, 8, 11);
        write_u64(&mut data_rel_ro, 24, compressed.len() as u64);

        let shstrtab = b"\0.rodata\0.rela.dyn\0.data.rel.ro\0.shstrtab\0";
        let rodata_name = 1;
        let rela_name = rodata_name + b".rodata\0".len();
        let data_rel_ro_name = rela_name + b".rela.dyn\0".len();
        let shstrtab_name = data_rel_ro_name + b".data.rel.ro\0".len();

        let section_header_off = SHSTRTAB_OFF + shstrtab.len();
        let mut elf = vec![0; section_header_off + SECTION_HEADER_SIZE * 5];

        elf[0..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2; // 64-bit
        elf[5] = 1; // little endian
        elf[6] = 1; // ELF version
        write_u16(&mut elf, 16, 3); // ET_DYN
        write_u16(&mut elf, 18, 183); // EM_AARCH64
        write_u32(&mut elf, 20, 1);
        write_u64(&mut elf, 32, ELF_HEADER_SIZE as u64);
        write_u64(&mut elf, 40, section_header_off as u64);
        write_u16(&mut elf, 52, ELF_HEADER_SIZE as u64);
        write_u16(&mut elf, 54, PROGRAM_HEADER_SIZE as u64);
        write_u16(&mut elf, 56, 1);
        write_u16(&mut elf, 58, SECTION_HEADER_SIZE as u64);
        write_u16(&mut elf, 60, 5);
        write_u16(&mut elf, 62, 4);

        write_program_header(
            &mut elf,
            ELF_HEADER_SIZE,
            1,
            6,
            0,
            0,
            0,
            (DATA_REL_RO_OFF + data_rel_ro.len()) as u64,
            (DATA_REL_RO_OFF + data_rel_ro.len()) as u64,
            0x1000,
        );

        elf[RODATA_OFF..RODATA_OFF + rodata.len()].copy_from_slice(&rodata);
        write_rela_entry(&mut elf, RELA_OFF, DATA_REL_RO_ADDR, RODATA_ADDR, 1027);
        write_rela_entry(
            &mut elf,
            RELA_OFF + 24,
            DATA_REL_RO_ADDR + 16,
            data_addr,
            1027,
        );
        elf[DATA_REL_RO_OFF..DATA_REL_RO_OFF + data_rel_ro.len()].copy_from_slice(&data_rel_ro);
        elf[SHSTRTAB_OFF..SHSTRTAB_OFF + shstrtab.len()].copy_from_slice(shstrtab);

        let shdr = section_header_off;
        write_section_header(
            &mut elf,
            shdr + SECTION_HEADER_SIZE,
            rodata_name as u32,
            1,
            2,
            RODATA_ADDR,
            RODATA_OFF as u64,
            rodata.len() as u64,
            0,
            0,
            1,
            0,
        );
        write_section_header(
            &mut elf,
            shdr + SECTION_HEADER_SIZE * 2,
            rela_name as u32,
            4,
            2,
            RELA_ADDR,
            RELA_OFF as u64,
            48,
            0,
            0,
            8,
            24,
        );
        write_section_header(
            &mut elf,
            shdr + SECTION_HEADER_SIZE * 3,
            data_rel_ro_name as u32,
            1,
            3,
            DATA_REL_RO_ADDR,
            DATA_REL_RO_OFF as u64,
            data_rel_ro.len() as u64,
            0,
            0,
            8,
            0,
        );
        write_section_header(
            &mut elf,
            shdr + SECTION_HEADER_SIZE * 4,
            shstrtab_name as u32,
            3,
            0,
            0,
            SHSTRTAB_OFF as u64,
            shstrtab.len() as u64,
            0,
            0,
            1,
            0,
        );

        elf
    }

    fn brotli_compress(data: &[u8]) -> Vec<u8> {
        let mut compressed = Vec::new();
        {
            let mut compressor = brotli::CompressorWriter::new(&mut compressed, 4096, 5, 22);
            compressor.write_all(data).unwrap();
        }
        compressed
    }

    fn write_program_header(
        data: &mut [u8],
        offset: usize,
        p_type: u32,
        p_flags: u32,
        p_offset: u64,
        p_vaddr: u64,
        p_paddr: u64,
        p_filesz: u64,
        p_memsz: u64,
        p_align: u64,
    ) {
        write_u32(data, offset, p_type);
        write_u32(data, offset + 4, p_flags);
        write_u64(data, offset + 8, p_offset);
        write_u64(data, offset + 16, p_vaddr);
        write_u64(data, offset + 24, p_paddr);
        write_u64(data, offset + 32, p_filesz);
        write_u64(data, offset + 40, p_memsz);
        write_u64(data, offset + 48, p_align);
    }

    fn write_section_header(
        data: &mut [u8],
        offset: usize,
        sh_name: u32,
        sh_type: u32,
        sh_flags: u64,
        sh_addr: u64,
        sh_offset: u64,
        sh_size: u64,
        sh_link: u32,
        sh_info: u32,
        sh_addralign: u64,
        sh_entsize: u64,
    ) {
        write_u32(data, offset, sh_name);
        write_u32(data, offset + 4, sh_type);
        write_u64(data, offset + 8, sh_flags);
        write_u64(data, offset + 16, sh_addr);
        write_u64(data, offset + 24, sh_offset);
        write_u64(data, offset + 32, sh_size);
        write_u32(data, offset + 40, sh_link);
        write_u32(data, offset + 44, sh_info);
        write_u64(data, offset + 48, sh_addralign);
        write_u64(data, offset + 56, sh_entsize);
    }

    fn write_rela_entry(data: &mut [u8], offset: usize, r_offset: u64, r_addend: u64, r_type: u32) {
        write_u64(data, offset, r_offset);
        write_u64(data, offset + 8, u64::from(r_type));
        write_u64(data, offset + 16, r_addend);
    }

    fn write_u16(data: &mut [u8], offset: usize, value: u64) {
        data[offset..offset + 2].copy_from_slice(&(value as u16).to_le_bytes());
    }

    fn write_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u64(data: &mut [u8], offset: usize, value: u64) {
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
}
