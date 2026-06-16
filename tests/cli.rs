mod common;

use assert_cmd::Command;
use predicates::str::contains;
use std::fs;

#[test]
fn cli_lists_assets_as_json() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("app");
    fs::write(&input, common::desktop_elf()).unwrap();

    Command::cargo_bin("tauri-dumper")
        .unwrap()
        .args(["list", input.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(contains("\"asset_count\": 1"))
        .stdout(contains("/index.html"));
}

#[test]
fn cli_inspects_binary() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("app");
    fs::write(&input, common::desktop_elf()).unwrap();

    Command::cargo_bin("tauri-dumper")
        .unwrap()
        .args(["inspect", input.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("Assets"));
}

#[test]
fn cli_extracts_with_default_shortcut() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("app");
    let output = temp.path().join("out");
    fs::write(&input, common::desktop_elf()).unwrap();

    Command::cargo_bin("tauri-dumper")
        .unwrap()
        .args([input.to_str().unwrap(), "-o", output.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("Export complete"));

    assert_eq!(
        fs::read(output.join("index.html")).unwrap(),
        b"<!DOCTYPE html><html></html>"
    );
}

#[test]
fn cli_repack_dry_run_reports_no_write() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("app");
    let output = temp.path().join("patched");
    let assets = temp.path().join("assets");
    fs::write(&input, common::desktop_elf()).unwrap();

    Command::cargo_bin("tauri-dumper")
        .unwrap()
        .args([
            "extract",
            input.to_str().unwrap(),
            "-o",
            assets.to_str().unwrap(),
            "--quiet",
        ])
        .assert()
        .success();
    fs::write(assets.join("index.html"), b"ok").unwrap();

    Command::cargo_bin("tauri-dumper")
        .unwrap()
        .args([
            "repack",
            input.to_str().unwrap(),
            "--assets",
            assets.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(contains("dry run"));
    assert!(!output.exists());
}

#[test]
fn cli_rejects_invalid_binary() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("bad");
    fs::write(&input, b"not a binary").unwrap();

    Command::cargo_bin("tauri-dumper")
        .unwrap()
        .args(["verify", input.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(contains("error:"));
}
