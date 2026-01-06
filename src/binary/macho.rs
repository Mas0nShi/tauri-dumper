//! Mach-O binary format parser.

use super::{BinaryParser, ScanRange, SectionInfo};
use anyhow::{anyhow, Context, Result};
use object::macho::{MachHeader64, SegmentCommand64, LC_DYLD_CHAINED_FIXUPS, LC_SEGMENT_64};
use object::read::macho::MachHeader;
use object::Endianness;

/// Mach-O pointer fixup format.
///
/// Modern macOS binaries use chained fixups, while older ones use traditional rebase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixupFormat {
    /// Modern chained fixups (LC_DYLD_CHAINED_FIXUPS).
    ///
    /// Pointer format: high bits contain metadata, low 43 bits contain offset from image base.
    ChainedFixups,

    /// Traditional rebase format (LC_DYLD_INFO_ONLY).
    ///
    /// Pointer value is the actual virtual address.
    Traditional,
}

/// Mach-O binary parser with support for both chained fixups and traditional formats.
pub struct MachOParser {
    sections: Vec<SectionInfo>,
    fixup_format: FixupFormat,
    image_base: u64,
}

impl MachOParser {
    /// Creates a new Mach-O parser from raw binary data.
    pub fn new(data: &[u8], sections: Vec<SectionInfo>) -> Result<Self> {
        let (fixup_format, image_base) = Self::detect_fixup_format(data)?;

        Ok(Self {
            sections,
            fixup_format,
            image_base,
        })
    }

    /// Detects the fixup format by analyzing load commands.
    fn detect_fixup_format(data: &[u8]) -> Result<(FixupFormat, u64)> {
        let header = MachHeader64::<Endianness>::parse(data, 0)
            .map_err(|e| anyhow!("Failed to parse Mach-O header: {}", e))?;

        let endian = header.endian().context("Failed to get endianness")?;

        let mut load_commands = header
            .load_commands(endian, data, 0)
            .map_err(|e| anyhow!("Failed to parse load commands: {}", e))?;

        let mut has_chained_fixups = false;
        let mut image_base = 0x100000000u64; // Default for 64-bit Mach-O

        while let Some(cmd) = load_commands.next()? {
            match cmd.cmd() {
                LC_DYLD_CHAINED_FIXUPS => {
                    has_chained_fixups = true;
                }
                LC_SEGMENT_64 => {
                    if let Ok(segment) = cmd.data::<SegmentCommand64<Endianness>>() {
                        if segment.segname == *b"__TEXT\0\0\0\0\0\0\0\0\0\0" {
                            image_base = segment.vmaddr.get(endian);
                        }
                    }
                }
                _ => {}
            }
        }

        let format = if has_chained_fixups {
            FixupFormat::ChainedFixups
        } else {
            FixupFormat::Traditional
        };

        Ok((format, image_base))
    }

    /// Decodes a raw pointer to get the actual virtual address.
    fn decode_pointer(&self, raw_ptr: u64) -> u64 {
        match self.fixup_format {
            FixupFormat::ChainedFixups => {
                // Chained fixups: low 43 bits contain offset from image base
                const TARGET_MASK: u64 = 0x7FFFFFFFFFF;
                let offset = raw_ptr & TARGET_MASK;
                self.image_base + offset
            }
            FixupFormat::Traditional => {
                // Traditional: pointer is the actual virtual address
                raw_ptr
            }
        }
    }

    /// Converts a virtual address to a file offset.
    fn va_to_file_offset(&self, va: u64) -> Result<u64> {
        self.sections
            .iter()
            .find(|s| va >= s.virtual_address && va < s.virtual_address + s.size)
            .map(|s| va - s.virtual_address + s.file_offset)
            .ok_or_else(|| anyhow!("Virtual address {:#X} not found in any section", va))
    }
}

impl BinaryParser for MachOParser {
    fn resolve_pointer(&self, raw_ptr: u64) -> Result<u64> {
        let va = self.decode_pointer(raw_ptr);
        self.va_to_file_offset(va)
    }

    fn scan_range(&self) -> Result<ScanRange> {
        // Asset headers are stored in __DATA_CONST,__const section (last in our list)
        let section = self
            .sections
            .last()
            .context("No sections found for scanning")?;

        Ok(ScanRange {
            start: section.file_offset as usize,
            length: section.size as usize,
        })
    }
}

