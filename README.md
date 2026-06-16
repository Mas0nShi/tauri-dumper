# Tauri Dumper

[![CI](https://github.com/Mas0nShi/tauri-dumper/actions/workflows/ci.yml/badge.svg)](https://github.com/Mas0nShi/tauri-dumper/actions/workflows/ci.yml)
[![Release](https://github.com/Mas0nShi/tauri-dumper/actions/workflows/release.yml/badge.svg)](https://github.com/Mas0nShi/tauri-dumper/actions/workflows/release.yml)
[![Crates.io](https://img.shields.io/crates/v/tauri-dumper.svg)](https://crates.io/crates/tauri-dumper)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Tauri Dumper is a Rust CLI and library for inspecting, extracting, and
replace-patching embedded frontend assets in compiled Tauri applications.

It is designed for interoperability research, debugging, migration work, and
authorized patching of applications you are allowed to inspect or modify.

> [!WARNING]
> Use this project only on software you own or have explicit permission to
> analyze. Tauri Dumper does not grant rights to modify or redistribute third
> party applications.

## What This Tool Does

- Detects embedded Tauri frontend assets in PE, Mach-O, and ELF binaries.
- Decompresses Brotli-compressed assets and exports them to a directory.
- Lists assets as a readable directory tree.
- Writes a reproducible `tauri-dumper.manifest.json` with source metadata,
  asset offsets, compressed sizes, and hashes.
- Replaces existing embedded assets in a patched binary copy.
- Exposes the same primitives as a Rust library.

## What This Tool Does Not Do

Tauri Dumper is not a decompiler and it cannot reconstruct a full Tauri
project from extracted files.

A Tauri application is more than its frontend bundle. The executable also
contains Rust backend code, Tauri command handlers, IPC contracts,
permissions/capabilities, native integrations, window configuration, build
metadata, and platform packaging details. Extracted assets are useful for
inspection and patching, but they are not equivalent to the original source
repository.

Practical distinction:

- Patching an existing application by replacing known embedded assets is
  supported.
- Building a new complete application from dumped assets alone is not
  supported.
- Adding new embedded asset entries or deleting existing entries is not
  supported.

## Install

Install from crates.io:

```bash
cargo install tauri-dumper
```

Download prebuilt binaries from GitHub Releases:

```text
https://github.com/Mas0nShi/tauri-dumper/releases
```

Build from source:

```bash
git clone https://github.com/Mas0nShi/tauri-dumper.git
cd tauri-dumper
cargo build --release
```

## Quick Start

Inspect a binary:

```bash
tauri-dumper inspect ./App.exe
```

Verify that Tauri assets can be found:

```bash
tauri-dumper verify ./App.exe
```

List embedded assets:

```bash
tauri-dumper list ./App.exe
```

Example list output:

```text
Assets: 198
├── _app
│   ├── env.js (23 B compressed, 19 B decompressed)
│   └── immutable
│       ├── assets
│       │   └── app.css (40 KiB compressed, 224 KiB decompressed)
│       └── chunks
│           └── app.js (18 KiB compressed, 69 KiB decompressed)
└── index.html (1.3 KiB compressed, 7.5 KiB decompressed)
```

Extract assets:

```bash
tauri-dumper extract ./App.exe -o ./assets
```

The default shortcut is equivalent to `extract`:

```bash
tauri-dumper ./App.exe -o ./assets
```

Patch an existing asset in a binary copy:

```bash
tauri-dumper repack ./App.exe --assets ./assets -o ./App.patched.exe
```

Always test the result in an isolated environment before replacing an
application in place.

## Commands

| Command | Purpose |
| --- | --- |
| `tauri-dumper list <binary>` | Print embedded assets as a directory tree. |
| `tauri-dumper inspect <binary>` | Print binary metadata and aggregate asset statistics. |
| `tauri-dumper verify <binary>` | Fail fast if no valid embedded Tauri assets are found. |
| `tauri-dumper extract <binary> -o <dir>` | Decompress and export assets. |
| `tauri-dumper repack <binary> --assets <dir> -o <patched-binary>` | Replace existing assets in a patched binary copy. |

Common read options:

```bash
--json
--quiet
--verbose
```

Extraction options:

```bash
--include <glob>
--exclude <glob>
--overwrite
--skip-existing
--dry-run
```

Repack options:

```bash
--strict
--skip-oversized
--dry-run
--allow-source-mismatch
--ad-hoc-sign
```

Use `--json` when integrating with scripts or CI:

```bash
tauri-dumper list ./App.exe --json
tauri-dumper repack ./App.exe --assets ./assets -o ./App.patched.exe --json
```

## Replace-Only Repack

Tauri embeds frontend assets into the application binary as static data,
pointers, and runtime lookup structures. A safe cross-platform repack flow can
reuse existing offsets, but it cannot safely invent new asset table entries or
remove existing ones without rebuilding format-specific binary structures.

For that reason, repack is intentionally replace-only.

Supported:

- Replace an asset path that already exists in the scanned binary.
- Replace an asset with smaller or equal-size Brotli-compressed bytes.
- Replace an asset with empty or minimal valid content if the target app can
  tolerate that content.
- Write a patched binary copy without expanding sections.

Unsupported:

- Add a new asset path.
- Delete an existing asset entry.
- Expand binary sections.
- Rewrite application asset maps, lookup tables, relocations, or fixups.
- Rebuild a complete Tauri project from dumped frontend files.

### Size Rule

Replacement content is Brotli-compressed before it is written back. Tauri
Dumper tries multiple Brotli quality levels and selects the smallest compressed
output it can produce.

A replacement is accepted only if:

```text
new_compressed_size <= original_compressed_size
```

If the replacement is larger, repack fails by default:

```bash
tauri-dumper repack ./App.exe --assets ./assets -o ./App.patched.exe
```

Skip oversized replacements instead:

```bash
tauri-dumper repack ./App.exe --assets ./assets -o ./App.patched.exe --skip-oversized
```

### Additions And Missing Files

Files in the asset directory that do not match an existing embedded asset path
are reported as unsupported additions. They are ignored by default:

```bash
tauri-dumper repack ./App.exe --assets ./assets -o ./App.patched.exe
```

Make unsupported additions an error:

```bash
tauri-dumper repack ./App.exe --assets ./assets -o ./App.patched.exe --strict
```

Missing replacement files mean "leave the original asset unchanged".

### Manifest Validation

`extract` writes `tauri-dumper.manifest.json` by default. During `repack`, if
that manifest is present, Tauri Dumper checks that the source binary hash still
matches the binary being patched.

Override this check only when you know the asset directory is compatible with
the target binary:

```bash
tauri-dumper repack ./App.exe --assets ./assets -o ./App.patched.exe --allow-source-mismatch
```

### macOS Signing

Patching a Mach-O binary changes its contents and normally invalidates the
existing code signature. Sign the patched binary before launching:

```bash
codesign --force --deep --sign - ./App.patched
```

On macOS, `--ad-hoc-sign` can run the ad-hoc signing command after a successful
repack:

```bash
tauri-dumper repack ./App --assets ./assets -o ./App.patched --ad-hoc-sign
```

## Compatibility

Prebuilt Tauri Dumper binaries are published for:

| Host OS | Host architecture |
| --- | --- |
| macOS | x86_64, aarch64 |
| Windows | x86_64, aarch64 |
| Linux | x86_64, aarch64 |

Target application formats:

| Target application | Binary format | Status |
| --- | --- | --- |
| Windows Tauri desktop app | PE, 64-bit | Supported and covered by real fixtures. |
| macOS Tauri desktop app | Mach-O, 64-bit | Supported and covered by real fixtures. |
| Linux Tauri desktop app | ELF, 64-bit | Supported and covered by real x86_64 fixtures. |
| Android Tauri app library | ELF shared object, aarch64 | Supported and covered by real fixtures. |
| 32-bit binaries | PE/Mach-O/ELF | Not supported. |

Parsing is implemented through `object::File::parse` with format-specific
pointer resolution for PE, Mach-O, and ELF.

## Manifest

`extract` writes a manifest next to exported assets:

```text
assets/
├── index.html
├── _app/
└── tauri-dumper.manifest.json
```

The manifest records:

- source binary path, SHA-256, size, format, and architecture;
- asset names;
- header offsets and data offsets;
- original compressed sizes;
- decompressed sizes;
- compressed asset SHA-256 hashes.

This file is intended for auditability and for repack safety checks.

## Library API

```rust
use tauri_dumper::{AssetScanner, BinaryImage, ExportOptions, Repacker};

fn main() -> tauri_dumper::Result<()> {
    let image = BinaryImage::open("App.exe")?;
    let table = AssetScanner::scan(&image)?;
    table.export(&ExportOptions::new("assets"))?;

    let image = BinaryImage::open("App.exe")?;
    let table = AssetScanner::scan(&image)?;
    Repacker::new(image, table)
        .replace_from_dir("assets")
        .write("App.patched.exe")?;

    Ok(())
}
```

The library uses typed errors via `tauri_dumper::Error` and
`tauri_dumper::Result`.

## Development

Install the Rust stable toolchain, then run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

Real-world regression fixtures are configured in
`tests/fixtures/fixtures.toml`. Download them with:

```bash
./scripts/download-fixtures.sh
```

Run release-mode smoke tests:

```bash
cargo test --release -- --nocapture
```

Validate the crates.io package locally:

```bash
cargo package --allow-dirty
```

## Release Process

The GitHub release workflow is tag based:

```bash
git tag -a vX.Y.Z -m "Release vX.Y.Z"
git push origin vX.Y.Z
```

The release workflow builds and uploads platform artifacts for macOS, Windows,
and Linux on x86_64 and aarch64.

Publish the crate after the release tag has been validated:

```bash
cargo publish --dry-run
cargo publish
```

## Security And Responsible Use

Tauri Dumper is intended for legitimate analysis and authorized modification.
It does not bypass licensing, DRM, server-side authorization, or application
security controls. When patching software, keep backups of the original binary
and verify behavior in a controlled environment before distribution or use.

## License

[MIT](LICENSE)
