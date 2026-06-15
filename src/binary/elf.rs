//! ELF binary format parser.

use super::{read_u64, BinaryParser, ScanRange, SectionInfo};
use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;

/// ELF binary parser with support for Android shared object RELA addends.
pub struct ElfParser {
    sections: Vec<SectionInfo>,
    scan_sections: Vec<SectionInfo>,
    relative_relocations: HashMap<u64, u64>,
}

impl ElfParser {
    /// Creates a new ELF parser with file-backed address sections and scan sections.
    pub fn new(
        sections: Vec<SectionInfo>,
        scan_sections: Vec<SectionInfo>,
        relative_relocations: HashMap<u64, u64>,
    ) -> Result<Self> {
        if sections.is_empty() {
            anyhow::bail!("No file-backed sections found in ELF file");
        }

        if scan_sections.is_empty() {
            anyhow::bail!("No supported asset section found in ELF file");
        }

        Ok(Self {
            sections,
            scan_sections,
            relative_relocations,
        })
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

impl BinaryParser for ElfParser {
    fn read_pointer(&self, data: &[u8], offset: usize) -> Result<u64> {
        if let Some(addend) = self.relative_relocations.get(&(offset as u64)) {
            return Ok(*addend);
        }

        read_u64(data, offset)
    }

    fn resolve_pointer(&self, raw_ptr: u64) -> Result<u64> {
        self.va_to_file_offset(raw_ptr)
    }

    fn scan_range(&self) -> Result<ScanRange> {
        let section = self
            .scan_sections
            .first()
            .context("No sections found for scanning")?;

        Ok(ScanRange {
            start: section.file_offset as usize,
            length: section.size as usize,
        })
    }
}
