# Tauri Dumper

> [!WARNING]
> This tool is for educational and interoperability research. Only inspect or
> modify applications you are authorized to work with.

Tauri Dumper extracts embedded frontend assets from compiled Tauri binaries and
can replace existing embedded assets in a binary copy.

## Features

- Extract Brotli-compressed Tauri assets from PE, Mach-O, Android ELF, and Linux ELF binaries.
- List, inspect, verify, and export assets from a professional command-line UI.
- Write `tauri-dumper.manifest.json` with source hash, binary metadata, asset offsets, and sizes.
- Replace existing embedded assets in-place with `repack`.
- Refuse unsupported add/delete semantics by design.

## Support

| OS | Architecture | File Type | Status |
| --- | --- | --- | --- |
| Windows | x86_64 | PE | Supported |
| Windows | x86 | PE | Not supported |
| Windows | arm64 | PE | Not tested |
| macOS | x86_64 | Mach-O | Supported |
| macOS | arm64 | Mach-O | Supported |
| Android | arm64 | ELF shared object (.so) | Supported |
| Linux | x86_64 | ELF | Supported |
| Linux | x86 | ELF | Not supported |
| Linux | arm64 | ELF | Supported |

## Install

```bash
cargo install tauri-dumper
```

## Commands

```bash
tauri-dumper extract <binary> -o <dir>
tauri-dumper list <binary>
tauri-dumper inspect <binary>
tauri-dumper verify <binary>
tauri-dumper repack <binary> --assets <dir> -o <patched-binary>
```

The shortcut below is equivalent to `extract`:

```bash
tauri-dumper <binary> -o <dir>
```

Useful flags:

```bash
--json
--quiet
--verbose
--include <glob>
--exclude <glob>
--overwrite
--skip-existing
--dry-run
```

## Replace-Only Repack

Tauri assets are encoded into the application binary as static data, pointers,
and runtime lookup structures. This means the supported repack operation is
intentionally limited to replacing assets that already exist.

Supported:

- Replace `/index.html` with modified content.
- Replace an asset with empty or minimal valid content if the application can handle it.
- Write a patched binary copy without changing section layout.

Unsupported:

- Add a new asset path.
- Delete an existing asset entry.
- Expand binary sections or rewrite application asset indexes.

Replacement files are Brotli-compressed before writing. Repack tries multiple
Brotli quality levels and uses the smallest compressed result it can produce.
A replacement is accepted only when that best compressed size is less than or
equal to the original compressed size. Oversized replacements fail by default:

```bash
tauri-dumper repack app.exe --assets extracted -o app.patched.exe
```

Use `--skip-oversized` to leave oversized replacements unchanged:

```bash
tauri-dumper repack app.exe --assets extracted -o app.patched.exe --skip-oversized
```

Extra files in the asset directory are reported as unsupported additions. Use
`--strict` to make them an error.

macOS patched binaries generally need signing before launch:

```bash
codesign --force --deep --sign - <patched-binary>
```

`--ad-hoc-sign` can run that command for you on macOS.

## Library API

```rust
use tauri_dumper::{AssetScanner, BinaryImage, ExportOptions, Repacker};

# fn main() -> tauri_dumper::Result<()> {
let image = BinaryImage::open("app.exe")?;
let table = AssetScanner::scan(&image)?;
table.export(&ExportOptions::new("assets"))?;

let image = BinaryImage::open("app.exe")?;
let table = AssetScanner::scan(&image)?;
Repacker::new(image, table)
    .replace_from_dir("assets")
    .write("app.patched.exe")?;
# Ok(())
# }
```

## Fixtures

Integration fixtures are configured in `tests/fixtures/fixtures.toml`.

```bash
./scripts/download-fixtures.sh
./scripts/download-fixtures.sh elf
```

The script delegates to the Rust `xtask` fixture downloader.

## License

[MIT](LICENSE)
