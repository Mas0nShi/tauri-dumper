//! Binary format parsing abstractions.
//!
//! This module provides a unified interface for parsing different binary formats
//! (PE, Mach-O) with format-specific pointer resolution strategies.

mod macho;
mod pe;

use anyhow::Result;
use object::{BinaryFormat, Object, ObjectSection};

pub use macho::MachOParser;
pub use pe::PeParser;

/// Information about a section in the binary.
#[derive(Debug, Clone)]
pub struct SectionInfo {
    pub virtual_address: u64,
    pub file_offset: u64,
    pub size: u64,
}

/// Defines the scan range for asset searching.
#[derive(Debug, Clone, Copy)]
pub struct ScanRange {
    pub start: usize,
    pub length: usize,
}

/// Trait for binary format-specific parsing operations.
///
/// Different binary formats (PE, Mach-O) have different ways of storing
/// and resolving pointers. This trait abstracts those differences.
pub trait BinaryParser: Send + Sync {
    /// Converts a raw pointer value from the binary to a file offset.
    ///
    /// This handles format-specific pointer encoding (e.g., Mach-O chained fixups).
    fn resolve_pointer(&self, raw_ptr: u64) -> Result<u64>;

    /// Returns the scan range for searching assets in the binary.
    fn scan_range(&self) -> Result<ScanRange>;
}

/// Creates the appropriate binary parser based on the detected format.
pub fn create_parser(data: &[u8]) -> Result<Box<dyn BinaryParser>> {
    let obj = object::File::parse(data)?;

    match obj.format() {
        BinaryFormat::Pe => {
            let sections = collect_pe_sections(&obj);
            Ok(Box::new(PeParser::new(sections)?))
        }
        BinaryFormat::MachO => {
            let sections = collect_macho_sections(&obj);
            Ok(Box::new(MachOParser::new(data, sections)?))
        }
        other => anyhow::bail!("Unsupported binary format: {:?}", other),
    }
}

fn collect_pe_sections<'a>(obj: &object::File<'a>) -> Vec<SectionInfo> {
    obj.sections()
        .filter(|s| {
            s.name() == Ok(".rdata") && s.kind() == object::SectionKind::ReadOnlyData
        })
        .filter_map(|s| {
            Some(SectionInfo {
                virtual_address: s.address(),
                file_offset: s.file_range()?.0,
                size: s.size(),
            })
        })
        .collect()
}

fn collect_macho_sections<'a>(obj: &object::File<'a>) -> Vec<SectionInfo> {
    // Collect __const sections from relevant segments:
    // - __TEXT,__const: contains string literals (asset names and data)
    // - __DATA_CONST,__const: contains asset headers (modern layout)
    // - __DATA,__const: contains asset headers (alternative layout)
    obj.sections()
        .filter(|s| {
            matches!(
                s.segment_name(),
                Ok(Some("__TEXT")) | Ok(Some("__DATA_CONST")) | Ok(Some("__DATA"))
            )
        })
        .filter(|s| s.name() == Ok("__const"))
        .filter_map(|s| {
            Some(SectionInfo {
                virtual_address: s.address(),
                file_offset: s.file_range()?.0,
                size: s.size(),
            })
        })
        .collect()
}

