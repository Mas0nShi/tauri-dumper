mod common;

use std::fs;

use tauri_dumper::{AssetScanner, BinaryImage, Dumper};

#[test]
fn extracts_real_world_fixtures_when_downloaded() {
    let config = common::load_config();
    let mut tested = 0;

    for fixture in &config.fixture {
        let id = fixture.id();
        let path = fixture.binary_path();
        if !path.exists() {
            eprintln!("[{id}] skipped: run ./scripts/download-fixtures.sh");
            continue;
        }

        let dumper = Dumper::from_path(&path).unwrap_or_else(|err| panic!("[{id}] init: {err}"));
        let table = dumper
            .scan()
            .unwrap_or_else(|err| panic!("[{id}] scan: {err}"));
        assert_eq!(
            table.len(),
            fixture.expected_asset_count,
            "[{id}] asset count"
        );

        let index = table
            .find("/index.html")
            .unwrap_or_else(|| panic!("[{id}] missing /index.html"));
        let decompressed = dumper.decompress_asset(index).unwrap();
        assert_eq!(
            decompressed.len(),
            fixture.expected_index_html_size,
            "[{id}] index.html size"
        );
        assert!(
            String::from_utf8_lossy(&decompressed).contains("<!DOCTYPE")
                || String::from_utf8_lossy(&decompressed).contains("<html"),
            "[{id}] index.html does not look like HTML"
        );
        eprintln!(
            "[{id}@{}] {} assets, index.html = {} bytes",
            fixture.version,
            table.len(),
            decompressed.len()
        );
        tested += 1;
    }

    if config
        .fixture
        .iter()
        .any(|fixture| fixture.binary_path().exists())
    {
        assert!(tested > 0);
    }
}

#[test]
fn extracts_android_elf_with_rela_pointer_addends() {
    let image = BinaryImage::from_bytes(common::android_elf_with_rela()).unwrap();
    let table = AssetScanner::scan(&image).unwrap();
    assert_eq!(table.len(), 1);
    assert_eq!(table.assets()[0].name(), "/index.html");
}

#[test]
fn extracts_android_elf_from_later_data_rel_ro_section() {
    let image = BinaryImage::from_bytes(common::android_elf_with_later_asset_section()).unwrap();
    let table = AssetScanner::scan(&image).unwrap();
    assert_eq!(table.len(), 1);
    assert_eq!(table.assets()[0].name(), "/index.html");
}

#[test]
fn extracts_desktop_elf_with_direct_pointers() {
    let image = BinaryImage::from_bytes(common::desktop_elf()).unwrap();
    let table = AssetScanner::scan(&image).unwrap();
    assert_eq!(table.len(), 1);
    assert_eq!(table.assets()[0].name(), "/index.html");
}

#[test]
fn rejects_invalid_binaries() {
    assert!(BinaryImage::from_bytes(b"not a valid binary").is_err());
    let temp = tempfile::NamedTempFile::new().unwrap();
    fs::write(temp.path(), []).unwrap();
    assert!(BinaryImage::open(temp.path()).is_err());
}
