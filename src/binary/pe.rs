//! PE (Portable Executable) binary format parser.

use super::{BinaryParser, ScanRange, SectionInfo};
use anyhow::{anyhow, Context, Result};

/// PE binary parser.
pub struct PeParser {
    sections: Vec<SectionInfo>,
}

impl PeParser {
    /// Creates a new PE parser with the given sections.
    pub fn new(sections: Vec<SectionInfo>) -> Result<Self> {
        if sections.is_empty() {
            anyhow::bail!("No .rdata section found in PE file");
        }
        Ok(Self { sections })
    }
}

impl BinaryParser for PeParser {
    fn resolve_pointer(&self, raw_ptr: u64) -> Result<u64> {
        // PE pointers are virtual addresses relative to image base
        let section = self.sections.first().context("No sections available")?;

        if raw_ptr >= section.virtual_address && raw_ptr < section.virtual_address + section.size {
            Ok(raw_ptr - section.virtual_address + section.file_offset)
        } else {
            Err(anyhow!(
                "Pointer {:#X} outside .rdata section bounds",
                raw_ptr
            ))
        }
    }

    fn scan_range(&self) -> Result<ScanRange> {
        let section = self
            .sections
            .first()
            .context("No sections found for scanning")?;

        Ok(ScanRange {
            start: section.file_offset as usize,
            length: section.size as usize,
        })
    }
}
