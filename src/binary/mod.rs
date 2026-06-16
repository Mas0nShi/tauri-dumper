//! Binary format parsing abstractions.
//!
//! This module provides a unified interface for parsing different binary formats
//! (PE, Mach-O, ELF) with format-specific pointer resolution strategies.

mod elf;
mod macho;
mod pe;

use anyhow::Result;
use object::{BinaryFormat, Object, ObjectSection, Relocation, RelocationFlags};
use std::collections::HashMap;

pub use elf::ElfParser;
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
    /// Reads a pointer-sized field from the binary.
    ///
    /// Most formats store the pointer value directly in the file. ELF files can
    /// store zeroes in relocated pointer fields and keep the target
    /// address in RELA addends, so parsers may override this.
    fn read_pointer(&self, data: &[u8], offset: usize) -> Result<u64> {
        read_u64(data, offset)
    }

    /// Converts a raw pointer value from the binary to a file offset.
    ///
    /// This handles format-specific pointer encoding (e.g., Mach-O chained fixups).
    fn resolve_pointer(&self, raw_ptr: u64) -> Result<u64>;

    /// Returns the scan ranges for searching assets in the binary.
    fn scan_ranges(&self) -> Result<Vec<ScanRange>>;
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
        BinaryFormat::Elf => {
            let sections = collect_elf_sections(&obj);
            let scan_sections = collect_elf_scan_sections(&obj);
            let relative_relocations = collect_elf_relative_relocations(&obj, &sections);
            Ok(Box::new(ElfParser::new(
                sections,
                scan_sections,
                relative_relocations,
            )?))
        }
        other => anyhow::bail!("Unsupported binary format: {:?}", other),
    }
}

pub(crate) fn read_u64(data: &[u8], offset: usize) -> Result<u64> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| anyhow::anyhow!("Pointer offset out of bounds"))?;
    let bytes = data
        .get(offset..end)
        .ok_or_else(|| anyhow::anyhow!("Pointer offset out of bounds"))?;
    Ok(u64::from_le_bytes(bytes.try_into()?))
}

fn collect_pe_sections<'a>(obj: &object::File<'a>) -> Vec<SectionInfo> {
    obj.sections()
        .filter(|s| s.name() == Ok(".rdata") && s.kind() == object::SectionKind::ReadOnlyData)
        .filter_map(|s| {
            Some(SectionInfo {
                virtual_address: s.address(),
                file_offset: s.file_range()?.0,
                size: s.size(),
            })
        })
        .collect()
}

fn collect_elf_sections<'a>(obj: &object::File<'a>) -> Vec<SectionInfo> {
    obj.sections()
        .filter(|s| s.address() != 0)
        .filter_map(section_info)
        .collect()
}

fn collect_elf_scan_sections<'a>(obj: &object::File<'a>) -> Vec<SectionInfo> {
    let mut sections = obj
        .sections()
        .filter_map(|s| {
            let name = s.name().ok()?;
            let priority = if name == ".data.rel.ro" || name.starts_with(".data.rel.ro.") {
                0
            } else if name == ".rodata" {
                1
            } else if name == ".data" {
                2
            } else {
                return None;
            };

            Some((priority, section_info(s)?))
        })
        .collect::<Vec<_>>();

    sections.sort_by_key(|(priority, section)| (*priority, section.file_offset));
    sections.into_iter().map(|(_, section)| section).collect()
}

fn collect_elf_relative_relocations<'a>(
    obj: &object::File<'a>,
    sections: &[SectionInfo],
) -> HashMap<u64, u64> {
    obj.dynamic_relocations()
        .into_iter()
        .flatten()
        .filter_map(|(address, relocation)| {
            if !is_elf_relative_relocation(&relocation) || relocation.has_implicit_addend() {
                return None;
            }

            let addend = u64::try_from(relocation.addend()).ok()?;
            let file_offset = va_to_file_offset(sections, address)?;
            Some((file_offset, addend))
        })
        .collect()
}

fn is_elf_relative_relocation(relocation: &Relocation) -> bool {
    match relocation.flags() {
        RelocationFlags::Elf { r_type } => {
            r_type == object::elf::R_386_RELATIVE
                || r_type == object::elf::R_ARM_RELATIVE
                || r_type == object::elf::R_AARCH64_RELATIVE
                || r_type == object::elf::R_X86_64_RELATIVE
                || r_type == object::elf::R_X86_64_RELATIVE64
                || r_type == object::elf::R_RISCV_RELATIVE
        }
        _ => false,
    }
}

fn va_to_file_offset(sections: &[SectionInfo], va: u64) -> Option<u64> {
    sections
        .iter()
        .find(|s| va >= s.virtual_address && va < s.virtual_address + s.size)
        .map(|s| va - s.virtual_address + s.file_offset)
}

fn section_info<'data, S>(section: S) -> Option<SectionInfo>
where
    S: ObjectSection<'data>,
{
    let (file_offset, file_size) = section.file_range()?;
    let size = section.size().min(file_size);

    if size == 0 {
        return None;
    }

    Some(SectionInfo {
        virtual_address: section.address(),
        file_offset,
        size,
    })
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
