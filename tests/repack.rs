mod common;

use std::fs;

use tauri_dumper::{extract, AssetScanner, BinaryImage, Error, Repacker};

#[test]
fn replaces_existing_asset_with_smaller_content() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("app");
    let assets_dir = temp.path().join("assets");
    let output = temp.path().join("patched");
    fs::write(&input, common::desktop_elf()).unwrap();

    let image = BinaryImage::open(&input).unwrap();
    let table = AssetScanner::scan(&image).unwrap();
    table
        .export(&tauri_dumper::ExportOptions::new(&assets_dir))
        .unwrap();
    fs::write(assets_dir.join("index.html"), b"ok").unwrap();

    let summary = Repacker::new(image, table)
        .replace_from_dir(&assets_dir)
        .write(&output)
        .unwrap();
    assert_eq!(summary.replaced, 1);

    let patched = BinaryImage::open(&output).unwrap();
    let patched_table = AssetScanner::scan(&patched).unwrap();
    let data = extract::decompress_asset(patched_table.find("/index.html").unwrap()).unwrap();
    assert_eq!(data, b"ok");
}

#[test]
fn treats_empty_content_as_replacement_not_deletion() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("app");
    let assets_dir = temp.path().join("assets");
    let output = temp.path().join("patched");
    fs::write(&input, common::desktop_elf()).unwrap();

    let image = BinaryImage::open(&input).unwrap();
    let table = AssetScanner::scan(&image).unwrap();
    table
        .export(&tauri_dumper::ExportOptions::new(&assets_dir))
        .unwrap();
    fs::write(assets_dir.join("index.html"), []).unwrap();

    Repacker::new(image, table)
        .replace_from_dir(&assets_dir)
        .write(&output)
        .unwrap();
    let patched = BinaryImage::open(&output).unwrap();
    let patched_table = AssetScanner::scan(&patched).unwrap();
    assert_eq!(patched_table.len(), 1);
    let data = extract::decompress_asset(patched_table.find("/index.html").unwrap()).unwrap();
    assert!(data.is_empty());
}

#[test]
fn missing_replacement_leaves_original_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("app");
    let assets_dir = temp.path().join("assets");
    let output = temp.path().join("patched");
    fs::write(&input, common::desktop_elf()).unwrap();
    fs::create_dir(&assets_dir).unwrap();

    let image = BinaryImage::open(&input).unwrap();
    let table = AssetScanner::scan(&image).unwrap();
    let summary = Repacker::new(image, table)
        .replace_from_dir(&assets_dir)
        .write(&output)
        .unwrap();

    assert_eq!(summary.replaced, 0);
    assert_eq!(summary.unchanged, 1);
    let patched = BinaryImage::open(&output).unwrap();
    let patched_table = AssetScanner::scan(&patched).unwrap();
    let data = extract::decompress_asset(patched_table.find("/index.html").unwrap()).unwrap();
    assert_eq!(data, b"<!DOCTYPE html><html></html>");
}

#[test]
fn extra_file_is_reported_as_unsupported_addition() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("app");
    let assets_dir = temp.path().join("assets");
    let output = temp.path().join("patched");
    fs::write(&input, common::desktop_elf()).unwrap();
    fs::create_dir(&assets_dir).unwrap();
    fs::write(assets_dir.join("new.txt"), b"new").unwrap();

    let image = BinaryImage::open(&input).unwrap();
    let table = AssetScanner::scan(&image).unwrap();
    let summary = Repacker::new(image, table.clone())
        .replace_from_dir(&assets_dir)
        .write(&output)
        .unwrap();
    assert_eq!(
        summary.unsupported_additions,
        vec![std::path::PathBuf::from("new.txt")]
    );

    let err = Repacker::new(BinaryImage::open(&input).unwrap(), table)
        .replace_from_dir(&assets_dir)
        .strict(true)
        .write(temp.path().join("strict"))
        .unwrap_err();
    assert!(matches!(err, Error::UnsupportedAddition(_)));
}

#[test]
fn appended_asset_data_outside_scan_ranges_is_not_added() {
    let mut binary = common::desktop_elf();
    binary.extend_from_slice(&[0; 256]);
    let image = BinaryImage::from_bytes(binary).unwrap();
    let table = AssetScanner::scan(&image).unwrap();
    assert_eq!(table.len(), 1);
}

#[test]
fn oversized_replacement_fails_unless_skipped() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("app");
    let assets_dir = temp.path().join("assets");
    fs::write(&input, common::desktop_elf()).unwrap();

    let image = BinaryImage::open(&input).unwrap();
    let table = AssetScanner::scan(&image).unwrap();
    table
        .export(&tauri_dumper::ExportOptions::new(&assets_dir))
        .unwrap();
    let mut large = Vec::with_capacity(16 * 1024);
    let mut state = 0x1234_5678u32;
    for _ in 0..16 * 1024 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        large.push((state >> 24) as u8);
    }
    fs::write(assets_dir.join("index.html"), large).unwrap();

    let err = Repacker::new(BinaryImage::open(&input).unwrap(), table.clone())
        .replace_from_dir(&assets_dir)
        .write(temp.path().join("fail"))
        .unwrap_err();
    assert!(matches!(err, Error::ReplacementTooLarge { .. }));

    let summary = Repacker::new(BinaryImage::open(&input).unwrap(), table)
        .replace_from_dir(&assets_dir)
        .skip_oversized(true)
        .write(temp.path().join("skip"))
        .unwrap();
    assert_eq!(summary.replaced, 0);
    assert_eq!(summary.skipped_oversized, 1);
}
