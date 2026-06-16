# Tauri Dumper

> [!WARNING]
> This tool is only for educational purposes. I am not responsible for any illegal use of this tool.

Tauri Dumper is a tool to dump Tauri applications.

## Support

| OS | Architecture | File Type | Status |
| --- | --- | --- | --- |
| Windows | x86_64 | PE | ✅ |
| Windows | x86 | PE | ❌ |
| Windows | arm64 | PE | ❓ |
| macOS | x86_64 | Mach-O | ✅ |
| macOS | arm64 | Mach-O | ✅ |
| Android | arm64 | ELF shared object (.so) | ✅ |
| Linux | x86_64 | ELF | ❌ |
| Linux | x86 | ELF | ❌ |
| Linux | arm64 | ELF | ❌ |

Description:

- ✅: Supported
- ❌: Not Supported
- ❓: Not Tested

## Installation

```bash
cargo install tauri-dumper
```

## Usage

```bash
tauri-dumper -i [path/to/app] -o [path/to/output]
```

## License

[MIT](LICENSE)
