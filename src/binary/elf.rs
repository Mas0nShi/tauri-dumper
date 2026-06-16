//! ELF binary format parser.

use super::{read_u64, BinaryParser, ScanRange, SectionInfo};
use anyhow::{anyhow, Result};
use std::collections::HashMap;

const ELF_MAGIC: &[u8; 4] = b"\x7fELF";
const ELF_CLASS_64: u8 = 2;
const ELF_DATA_LITTLE: u8 = 1;
const ET_DYN: u16 = 3;

const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_GNU_RELRO: u32 = 0x6474e552;

const DT_NULL: i64 = 0;
const DT_RELA: i64 = 7;
const DT_RELASZ: i64 = 8;
const DT_RELAENT: i64 = 9;

/// ELF parser with support for Android Tauri shared object RELA addends.
pub struct ElfParser {
    sections: Vec<SectionInfo>,
    scan_sections: Vec<SectionInfo>,
    relative_relocations: HashMap<u64, u64>,
}

#[derive(Debug, Clone, Copy)]
struct ElfHeader {
    e_type: u16,
    e_machine: u16,
    phoff: u64,
    shoff: u64,
    phentsize: u16,
    phnum: u16,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
}

#[derive(Debug, Clone, Copy)]
struct ProgramHeader {
    typ: u32,
    offset: u64,
    vaddr: u64,
    filesz: u64,
}

#[derive(Debug, Clone)]
struct NamedSection {
    name: String,
    info: SectionInfo,
}

impl ElfParser {
    /// Creates a new ELF parser with file-backed address sections and scan sections.
    pub fn new(
        sections: Vec<SectionInfo>,
        scan_sections: Vec<SectionInfo>,
        relative_relocations: HashMap<u64, u64>,
    ) -> Result<Self> {
        if sections.is_empty() {
            anyhow::bail!("No file-backed LOAD segments found in ELF file");
        }

        if scan_sections.is_empty() {
            anyhow::bail!("No supported Android Tauri asset section found in ELF shared object");
        }

        Ok(Self {
            sections,
            scan_sections,
            relative_relocations,
        })
    }

    /// Parses an Android Tauri ELF shared object.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let header = parse_elf_header(data)?;

        if header.e_type != ET_DYN {
            anyhow::bail!(
                "Unsupported ELF format: only Android Tauri shared libraries are supported"
            );
        }

        let program_headers = parse_program_headers(data, header)?;
        let sections = collect_load_segments(&program_headers);
        let scan_sections = collect_scan_sections(data, header, &program_headers, &sections)?;
        let relative_relocations =
            collect_relative_relocations(data, header, &program_headers, &sections)?;

        Self::new(sections, scan_sections, relative_relocations)
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

    fn scan_ranges(&self) -> Result<Vec<ScanRange>> {
        Ok(self
            .scan_sections
            .iter()
            .map(|section| ScanRange {
                start: section.file_offset as usize,
                length: section.size as usize,
            })
            .collect())
    }
}

pub fn is_elf(data: &[u8]) -> bool {
    data.get(..4) == Some(ELF_MAGIC)
}

fn parse_elf_header(data: &[u8]) -> Result<ElfHeader> {
    if !is_elf(data) {
        anyhow::bail!("Not an ELF file");
    }

    if data.get(4) != Some(&ELF_CLASS_64) || data.get(5) != Some(&ELF_DATA_LITTLE) {
        anyhow::bail!("Unsupported ELF format: only 64-bit little-endian files are supported");
    }

    Ok(ElfHeader {
        e_type: read_u16(data, 16)?,
        e_machine: read_u16(data, 18)?,
        phoff: read_u64(data, 32)?,
        shoff: read_u64(data, 40)?,
        phentsize: read_u16(data, 54)?,
        phnum: read_u16(data, 56)?,
        shentsize: read_u16(data, 58)?,
        shnum: read_u16(data, 60)?,
        shstrndx: read_u16(data, 62)?,
    })
}

fn parse_program_headers(data: &[u8], header: ElfHeader) -> Result<Vec<ProgramHeader>> {
    if header.phentsize < 56 {
        anyhow::bail!("Unsupported ELF program header size");
    }

    (0..usize::from(header.phnum))
        .map(|index| {
            let offset = checked_table_offset(header.phoff, header.phentsize, index)?;
            Ok(ProgramHeader {
                typ: read_u32(data, offset)?,
                offset: read_u64(data, offset + 8)?,
                vaddr: read_u64(data, offset + 16)?,
                filesz: read_u64(data, offset + 32)?,
            })
        })
        .collect()
}

fn collect_load_segments(program_headers: &[ProgramHeader]) -> Vec<SectionInfo> {
    program_headers
        .iter()
        .filter(|ph| ph.typ == PT_LOAD && ph.filesz > 0)
        .map(|ph| SectionInfo {
            virtual_address: ph.vaddr,
            file_offset: ph.offset,
            size: ph.filesz,
        })
        .collect()
}

fn collect_scan_sections(
    data: &[u8],
    header: ElfHeader,
    program_headers: &[ProgramHeader],
    load_segments: &[SectionInfo],
) -> Result<Vec<SectionInfo>> {
    let named_sections = parse_named_sections(data, header).unwrap_or_default();
    let mut scan_sections = collect_named_scan_sections(&named_sections);

    if scan_sections.is_empty() {
        scan_sections = collect_program_header_scan_sections(program_headers, load_segments);
    }

    Ok(scan_sections)
}

fn collect_named_scan_sections(sections: &[NamedSection]) -> Vec<SectionInfo> {
    let mut sections = sections
        .iter()
        .filter_map(|section| {
            let priority =
                if section.name == ".data.rel.ro" || section.name.starts_with(".data.rel.ro.") {
                    0
                } else if section.name == ".rodata" {
                    1
                } else if section.name == ".data" {
                    2
                } else {
                    return None;
                };

            Some((priority, section.info.clone()))
        })
        .collect::<Vec<_>>();

    sections.sort_by_key(|(priority, section)| (*priority, section.file_offset));
    sections
        .into_iter()
        .map(|(_, section)| section)
        .collect::<Vec<_>>()
}

fn parse_named_sections(data: &[u8], header: ElfHeader) -> Result<Vec<NamedSection>> {
    if header.shoff == 0 || header.shnum == 0 || header.shstrndx >= header.shnum {
        return Ok(Vec::new());
    }

    if header.shentsize < 64 {
        anyhow::bail!("Unsupported ELF section header size");
    }

    let shstrtab_offset =
        checked_table_offset(header.shoff, header.shentsize, usize::from(header.shstrndx))?;
    let shstrtab_file_offset = read_u64(data, shstrtab_offset + 24)?;
    let shstrtab_size = read_u64(data, shstrtab_offset + 32)?;
    let shstrtab = slice_u64(data, shstrtab_file_offset, shstrtab_size)?;

    (0..usize::from(header.shnum))
        .filter_map(|index| parse_named_section(data, header, shstrtab, index).transpose())
        .collect()
}

fn parse_named_section(
    data: &[u8],
    header: ElfHeader,
    shstrtab: &[u8],
    index: usize,
) -> Result<Option<NamedSection>> {
    let offset = checked_table_offset(header.shoff, header.shentsize, index)?;
    let name_offset = read_u32(data, offset)? as usize;
    let addr = read_u64(data, offset + 16)?;
    let file_offset = read_u64(data, offset + 24)?;
    let size = read_u64(data, offset + 32)?;

    if addr == 0 || size == 0 {
        return Ok(None);
    }

    let Some(name) = read_string(shstrtab, name_offset) else {
        return Ok(None);
    };

    Ok(Some(NamedSection {
        name,
        info: SectionInfo {
            virtual_address: addr,
            file_offset,
            size,
        },
    }))
}

fn collect_program_header_scan_sections(
    program_headers: &[ProgramHeader],
    load_segments: &[SectionInfo],
) -> Vec<SectionInfo> {
    let mut sections = program_headers
        .iter()
        .filter(|ph| ph.typ == PT_GNU_RELRO && ph.filesz > 0)
        .map(|ph| SectionInfo {
            virtual_address: ph.vaddr,
            file_offset: ph.offset,
            size: ph.filesz,
        })
        .collect::<Vec<_>>();

    if sections.is_empty() {
        sections = load_segments
            .iter()
            .filter(|section| section.size > 0)
            .cloned()
            .collect();
    }

    sections
}

fn collect_relative_relocations(
    data: &[u8],
    header: ElfHeader,
    program_headers: &[ProgramHeader],
    load_segments: &[SectionInfo],
) -> Result<HashMap<u64, u64>> {
    let relocations = collect_dynamic_relative_relocations(
        data,
        header.e_machine,
        program_headers,
        load_segments,
    )?;
    if !relocations.is_empty() {
        return Ok(relocations);
    }

    let named_sections = parse_named_sections(data, header).unwrap_or_default();
    collect_section_relative_relocations(data, header.e_machine, &named_sections, load_segments)
}

fn collect_dynamic_relative_relocations(
    data: &[u8],
    machine: u16,
    program_headers: &[ProgramHeader],
    load_segments: &[SectionInfo],
) -> Result<HashMap<u64, u64>> {
    let Some(dynamic) = program_headers.iter().find(|ph| ph.typ == PT_DYNAMIC) else {
        return Ok(HashMap::new());
    };

    let mut rela_va = None;
    let mut rela_size = None;
    let mut rela_ent = 24;
    let dynamic_offset = dynamic.offset as usize;
    let dynamic_size = dynamic.filesz as usize;

    for offset in (dynamic_offset..dynamic_offset.saturating_add(dynamic_size)).step_by(16) {
        let tag = read_i64(data, offset)?;
        let value = read_u64(data, offset + 8)?;

        match tag {
            DT_NULL => break,
            DT_RELA => rela_va = Some(value),
            DT_RELASZ => rela_size = Some(value),
            DT_RELAENT => rela_ent = value,
            _ => {}
        }
    }

    let Some(rela_va) = rela_va else {
        return Ok(HashMap::new());
    };
    let Some(rela_size) = rela_size else {
        return Ok(HashMap::new());
    };

    if rela_ent < 24 {
        anyhow::bail!("Unsupported ELF RELA entry size");
    }

    let rela_offset = va_to_file_offset(load_segments, rela_va)
        .ok_or_else(|| anyhow!("ELF RELA table is outside LOAD segments"))?;
    let mut relocations = HashMap::new();

    collect_rela_entries(
        data,
        machine,
        rela_offset,
        rela_size,
        rela_ent,
        load_segments,
        &mut relocations,
    )?;

    Ok(relocations)
}

fn collect_section_relative_relocations(
    data: &[u8],
    machine: u16,
    sections: &[NamedSection],
    load_segments: &[SectionInfo],
) -> Result<HashMap<u64, u64>> {
    let mut relocations = HashMap::new();

    for section in sections
        .iter()
        .filter(|section| section.name == ".rela.dyn" || section.name.starts_with(".rela."))
    {
        collect_rela_entries(
            data,
            machine,
            section.info.file_offset,
            section.info.size,
            24,
            load_segments,
            &mut relocations,
        )?;
    }

    Ok(relocations)
}

fn collect_rela_entries(
    data: &[u8],
    machine: u16,
    rela_offset: u64,
    rela_size: u64,
    rela_ent: u64,
    load_segments: &[SectionInfo],
    relocations: &mut HashMap<u64, u64>,
) -> Result<()> {
    let count = rela_size / rela_ent;

    for index in 0..count {
        let offset = rela_offset
            .checked_add(
                index
                    .checked_mul(rela_ent)
                    .ok_or_else(|| anyhow!("ELF RELA table offset overflow"))?,
            )
            .ok_or_else(|| anyhow!("ELF RELA table offset overflow"))?
            as usize;
        let relocation_va = read_u64(data, offset)?;
        let r_info = read_u64(data, offset + 8)?;
        let addend = read_i64(data, offset + 16)?;
        let r_type = (r_info & 0xffff_ffff) as u32;

        if !is_relative_relocation(machine, r_type) {
            continue;
        }

        let Some(relocation_offset) = va_to_file_offset(load_segments, relocation_va) else {
            continue;
        };
        let Ok(addend) = u64::try_from(addend) else {
            continue;
        };

        relocations.insert(relocation_offset, addend);
    }

    Ok(())
}

fn is_relative_relocation(machine: u16, r_type: u32) -> bool {
    match machine {
        object::elf::EM_386 => r_type == object::elf::R_386_RELATIVE,
        object::elf::EM_ARM => r_type == object::elf::R_ARM_RELATIVE,
        object::elf::EM_AARCH64 => r_type == object::elf::R_AARCH64_RELATIVE,
        object::elf::EM_X86_64 => {
            r_type == object::elf::R_X86_64_RELATIVE || r_type == object::elf::R_X86_64_RELATIVE64
        }
        object::elf::EM_RISCV => r_type == object::elf::R_RISCV_RELATIVE,
        _ => false,
    }
}

fn va_to_file_offset(sections: &[SectionInfo], va: u64) -> Option<u64> {
    sections
        .iter()
        .find(|s| va >= s.virtual_address && va < s.virtual_address + s.size)
        .map(|s| va - s.virtual_address + s.file_offset)
}

fn checked_table_offset(table_offset: u64, entry_size: u16, index: usize) -> Result<usize> {
    let index = u64::try_from(index)?;
    let byte_offset = index
        .checked_mul(u64::from(entry_size))
        .and_then(|offset| table_offset.checked_add(offset))
        .ok_or_else(|| anyhow!("ELF table offset overflow"))?;
    Ok(usize::try_from(byte_offset)?)
}

fn slice_u64(data: &[u8], offset: u64, len: u64) -> Result<&[u8]> {
    let offset = usize::try_from(offset)?;
    let len = usize::try_from(len)?;
    let end = offset
        .checked_add(len)
        .ok_or_else(|| anyhow!("ELF file range overflow"))?;
    data.get(offset..end)
        .ok_or_else(|| anyhow!("ELF file range out of bounds"))
}

fn read_string(data: &[u8], offset: usize) -> Option<String> {
    let bytes = data.get(offset..)?;
    let end = bytes.iter().position(|&b| b == 0)?;
    std::str::from_utf8(&bytes[..end]).ok().map(str::to_owned)
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16> {
    let bytes = data
        .get(offset..offset + 2)
        .ok_or_else(|| anyhow!("ELF read out of bounds"))?;
    Ok(u16::from_le_bytes(bytes.try_into()?))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| anyhow!("ELF read out of bounds"))?;
    Ok(u32::from_le_bytes(bytes.try_into()?))
}

fn read_i64(data: &[u8], offset: usize) -> Result<i64> {
    let bytes = data
        .get(offset..offset + 8)
        .ok_or_else(|| anyhow!("ELF read out of bounds"))?;
    Ok(i64::from_le_bytes(bytes.try_into()?))
}
