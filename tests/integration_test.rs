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
    fn extracts_assets_from_later_android_elf_asset_section() {
        let elf = make_android_tauri_elf_fixture_with_empty_prefix_section();
        let temp = tempfile::NamedTempFile::new().unwrap();
        fs::write(temp.path(), elf).unwrap();

        let file = File::open(temp.path()).unwrap();
        let dumper = Dumper::new(file).unwrap();
        let assets = dumper.scan_assets().unwrap();

        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].name, "/index.html");
    }

    #[test]
    fn extracts_assets_from_desktop_elf_with_direct_pointers() {
        let elf = make_desktop_tauri_elf_fixture();
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
            ProgramHeader {
                typ: 1,
                flags: 6,
                offset: 0,
                vaddr: 0,
                paddr: 0,
                filesz: (DATA_REL_RO_OFF + data_rel_ro.len()) as u64,
                memsz: (DATA_REL_RO_OFF + data_rel_ro.len()) as u64,
                align: 0x1000,
            },
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
            SectionHeader {
                name: rodata_name as u32,
                typ: 1,
                flags: 2,
                addr: RODATA_ADDR,
                offset: RODATA_OFF as u64,
                size: rodata.len() as u64,
                link: 0,
                info: 0,
                align: 1,
                entsize: 0,
            },
        );
        write_section_header(
            &mut elf,
            shdr + SECTION_HEADER_SIZE * 2,
            SectionHeader {
                name: rela_name as u32,
                typ: 4,
                flags: 2,
                addr: RELA_ADDR,
                offset: RELA_OFF as u64,
                size: 48,
                link: 0,
                info: 0,
                align: 8,
                entsize: 24,
            },
        );
        write_section_header(
            &mut elf,
            shdr + SECTION_HEADER_SIZE * 3,
            SectionHeader {
                name: data_rel_ro_name as u32,
                typ: 1,
                flags: 3,
                addr: DATA_REL_RO_ADDR,
                offset: DATA_REL_RO_OFF as u64,
                size: data_rel_ro.len() as u64,
                link: 0,
                info: 0,
                align: 8,
                entsize: 0,
            },
        );
        write_section_header(
            &mut elf,
            shdr + SECTION_HEADER_SIZE * 4,
            SectionHeader {
                name: shstrtab_name as u32,
                typ: 3,
                flags: 0,
                addr: 0,
                offset: SHSTRTAB_OFF as u64,
                size: shstrtab.len() as u64,
                link: 0,
                info: 0,
                align: 1,
                entsize: 0,
            },
        );

        elf
    }

    fn make_android_tauri_elf_fixture_with_empty_prefix_section() -> Vec<u8> {
        const ELF_HEADER_SIZE: usize = 64;
        const PROGRAM_HEADER_SIZE: usize = 56;
        const SECTION_HEADER_SIZE: usize = 64;

        const RODATA_ADDR: u64 = 0x1000;
        const RELA_ADDR: u64 = 0x2000;
        const EMPTY_DATA_REL_RO_ADDR: u64 = 0x3000;
        const DATA_REL_RO_ADDR: u64 = 0x3100;

        const RODATA_OFF: usize = 0x1000;
        const RELA_OFF: usize = 0x2000;
        const EMPTY_DATA_REL_RO_OFF: usize = 0x3000;
        const DATA_REL_RO_OFF: usize = 0x3100;
        const SHSTRTAB_OFF: usize = 0x4000;

        let html = b"<!DOCTYPE html><html></html>";
        let compressed = brotli_compress(html);

        let mut rodata = Vec::new();
        rodata.extend_from_slice(b"/index.html");
        let data_addr = RODATA_ADDR + rodata.len() as u64;
        rodata.extend_from_slice(&compressed);

        let empty_data_rel_ro = vec![0; 32];
        let mut data_rel_ro = vec![0; 32];
        write_u64(&mut data_rel_ro, 8, 11);
        write_u64(&mut data_rel_ro, 24, compressed.len() as u64);

        let shstrtab = b"\0.rodata\0.rela.dyn\0.data.rel.ro.a\0.data.rel.ro.b\0.shstrtab\0";
        let rodata_name = 1;
        let rela_name = rodata_name + b".rodata\0".len();
        let empty_data_rel_ro_name = rela_name + b".rela.dyn\0".len();
        let data_rel_ro_name = empty_data_rel_ro_name + b".data.rel.ro.a\0".len();
        let shstrtab_name = data_rel_ro_name + b".data.rel.ro.b\0".len();

        let section_header_off = SHSTRTAB_OFF + shstrtab.len();
        let mut elf = vec![0; section_header_off + SECTION_HEADER_SIZE * 6];

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
        write_u16(&mut elf, 60, 6);
        write_u16(&mut elf, 62, 5);

        write_program_header(
            &mut elf,
            ELF_HEADER_SIZE,
            ProgramHeader {
                typ: 1,
                flags: 6,
                offset: 0,
                vaddr: 0,
                paddr: 0,
                filesz: (DATA_REL_RO_OFF + data_rel_ro.len()) as u64,
                memsz: (DATA_REL_RO_OFF + data_rel_ro.len()) as u64,
                align: 0x1000,
            },
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
        elf[EMPTY_DATA_REL_RO_OFF..EMPTY_DATA_REL_RO_OFF + empty_data_rel_ro.len()]
            .copy_from_slice(&empty_data_rel_ro);
        elf[DATA_REL_RO_OFF..DATA_REL_RO_OFF + data_rel_ro.len()].copy_from_slice(&data_rel_ro);
        elf[SHSTRTAB_OFF..SHSTRTAB_OFF + shstrtab.len()].copy_from_slice(shstrtab);

        let shdr = section_header_off;
        write_section_header(
            &mut elf,
            shdr + SECTION_HEADER_SIZE,
            SectionHeader {
                name: rodata_name as u32,
                typ: 1,
                flags: 2,
                addr: RODATA_ADDR,
                offset: RODATA_OFF as u64,
                size: rodata.len() as u64,
                link: 0,
                info: 0,
                align: 1,
                entsize: 0,
            },
        );
        write_section_header(
            &mut elf,
            shdr + SECTION_HEADER_SIZE * 2,
            SectionHeader {
                name: rela_name as u32,
                typ: 4,
                flags: 2,
                addr: RELA_ADDR,
                offset: RELA_OFF as u64,
                size: 48,
                link: 0,
                info: 0,
                align: 8,
                entsize: 24,
            },
        );
        write_section_header(
            &mut elf,
            shdr + SECTION_HEADER_SIZE * 3,
            SectionHeader {
                name: empty_data_rel_ro_name as u32,
                typ: 1,
                flags: 3,
                addr: EMPTY_DATA_REL_RO_ADDR,
                offset: EMPTY_DATA_REL_RO_OFF as u64,
                size: empty_data_rel_ro.len() as u64,
                link: 0,
                info: 0,
                align: 8,
                entsize: 0,
            },
        );
        write_section_header(
            &mut elf,
            shdr + SECTION_HEADER_SIZE * 4,
            SectionHeader {
                name: data_rel_ro_name as u32,
                typ: 1,
                flags: 3,
                addr: DATA_REL_RO_ADDR,
                offset: DATA_REL_RO_OFF as u64,
                size: data_rel_ro.len() as u64,
                link: 0,
                info: 0,
                align: 8,
                entsize: 0,
            },
        );
        write_section_header(
            &mut elf,
            shdr + SECTION_HEADER_SIZE * 5,
            SectionHeader {
                name: shstrtab_name as u32,
                typ: 3,
                flags: 0,
                addr: 0,
                offset: SHSTRTAB_OFF as u64,
                size: shstrtab.len() as u64,
                link: 0,
                info: 0,
                align: 1,
                entsize: 0,
            },
        );

        elf
    }

    fn make_desktop_tauri_elf_fixture() -> Vec<u8> {
        const ELF_HEADER_SIZE: usize = 64;
        const PROGRAM_HEADER_SIZE: usize = 56;
        const SECTION_HEADER_SIZE: usize = 64;

        const RODATA_ADDR: u64 = 0x400000;
        const DATA_REL_RO_ADDR: u64 = 0x500000;

        const RODATA_OFF: usize = 0x1000;
        const DATA_REL_RO_OFF: usize = 0x2000;
        const SHSTRTAB_OFF: usize = 0x3000;

        let html = b"<!DOCTYPE html><html></html>";
        let compressed = brotli_compress(html);

        let mut rodata = Vec::new();
        rodata.extend_from_slice(b"/index.html");
        let data_addr = RODATA_ADDR + rodata.len() as u64;
        rodata.extend_from_slice(&compressed);

        let mut data_rel_ro = vec![0; 32];
        write_u64(&mut data_rel_ro, 0, RODATA_ADDR);
        write_u64(&mut data_rel_ro, 8, 11);
        write_u64(&mut data_rel_ro, 16, data_addr);
        write_u64(&mut data_rel_ro, 24, compressed.len() as u64);

        let shstrtab = b"\0.rodata\0.data.rel.ro\0.shstrtab\0";
        let rodata_name = 1;
        let data_rel_ro_name = rodata_name + b".rodata\0".len();
        let shstrtab_name = data_rel_ro_name + b".data.rel.ro\0".len();

        let section_header_off = SHSTRTAB_OFF + shstrtab.len();
        let mut elf = vec![0; section_header_off + SECTION_HEADER_SIZE * 4];

        elf[0..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2; // 64-bit
        elf[5] = 1; // little endian
        elf[6] = 1; // ELF version
        write_u16(&mut elf, 16, 2); // ET_EXEC
        write_u16(&mut elf, 18, 62); // EM_X86_64
        write_u32(&mut elf, 20, 1);
        write_u64(&mut elf, 32, ELF_HEADER_SIZE as u64);
        write_u64(&mut elf, 40, section_header_off as u64);
        write_u16(&mut elf, 52, ELF_HEADER_SIZE as u64);
        write_u16(&mut elf, 54, PROGRAM_HEADER_SIZE as u64);
        write_u16(&mut elf, 56, 1);
        write_u16(&mut elf, 58, SECTION_HEADER_SIZE as u64);
        write_u16(&mut elf, 60, 4);
        write_u16(&mut elf, 62, 3);

        write_program_header(
            &mut elf,
            ELF_HEADER_SIZE,
            ProgramHeader {
                typ: 1,
                flags: 6,
                offset: 0,
                vaddr: 0,
                paddr: 0,
                filesz: (DATA_REL_RO_OFF + data_rel_ro.len()) as u64,
                memsz: (DATA_REL_RO_OFF + data_rel_ro.len()) as u64,
                align: 0x1000,
            },
        );

        elf[RODATA_OFF..RODATA_OFF + rodata.len()].copy_from_slice(&rodata);
        elf[DATA_REL_RO_OFF..DATA_REL_RO_OFF + data_rel_ro.len()].copy_from_slice(&data_rel_ro);
        elf[SHSTRTAB_OFF..SHSTRTAB_OFF + shstrtab.len()].copy_from_slice(shstrtab);

        let shdr = section_header_off;
        write_section_header(
            &mut elf,
            shdr + SECTION_HEADER_SIZE,
            SectionHeader {
                name: rodata_name as u32,
                typ: 1,
                flags: 2,
                addr: RODATA_ADDR,
                offset: RODATA_OFF as u64,
                size: rodata.len() as u64,
                link: 0,
                info: 0,
                align: 1,
                entsize: 0,
            },
        );
        write_section_header(
            &mut elf,
            shdr + SECTION_HEADER_SIZE * 2,
            SectionHeader {
                name: data_rel_ro_name as u32,
                typ: 1,
                flags: 3,
                addr: DATA_REL_RO_ADDR,
                offset: DATA_REL_RO_OFF as u64,
                size: data_rel_ro.len() as u64,
                link: 0,
                info: 0,
                align: 8,
                entsize: 0,
            },
        );
        write_section_header(
            &mut elf,
            shdr + SECTION_HEADER_SIZE * 3,
            SectionHeader {
                name: shstrtab_name as u32,
                typ: 3,
                flags: 0,
                addr: 0,
                offset: SHSTRTAB_OFF as u64,
                size: shstrtab.len() as u64,
                link: 0,
                info: 0,
                align: 1,
                entsize: 0,
            },
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

    struct ProgramHeader {
        typ: u32,
        flags: u32,
        offset: u64,
        vaddr: u64,
        paddr: u64,
        filesz: u64,
        memsz: u64,
        align: u64,
    }

    fn write_program_header(data: &mut [u8], offset: usize, header: ProgramHeader) {
        write_u32(data, offset, header.typ);
        write_u32(data, offset + 4, header.flags);
        write_u64(data, offset + 8, header.offset);
        write_u64(data, offset + 16, header.vaddr);
        write_u64(data, offset + 24, header.paddr);
        write_u64(data, offset + 32, header.filesz);
        write_u64(data, offset + 40, header.memsz);
        write_u64(data, offset + 48, header.align);
    }

    struct SectionHeader {
        name: u32,
        typ: u32,
        flags: u64,
        addr: u64,
        offset: u64,
        size: u64,
        link: u32,
        info: u32,
        align: u64,
        entsize: u64,
    }

    fn write_section_header(data: &mut [u8], offset: usize, header: SectionHeader) {
        write_u32(data, offset, header.name);
        write_u32(data, offset + 4, header.typ);
        write_u64(data, offset + 8, header.flags);
        write_u64(data, offset + 16, header.addr);
        write_u64(data, offset + 24, header.offset);
        write_u64(data, offset + 32, header.size);
        write_u32(data, offset + 40, header.link);
        write_u32(data, offset + 44, header.info);
        write_u64(data, offset + 48, header.align);
        write_u64(data, offset + 56, header.entsize);
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
