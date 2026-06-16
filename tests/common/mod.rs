#![allow(dead_code)]

use serde::Deserialize;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub fixture: Vec<Fixture>,
}

#[derive(Debug, Deserialize)]
pub struct Fixture {
    pub name: String,
    pub format: String,
    pub arch: String,
    pub version: String,
    pub extract_dir: String,
    pub binary: String,
    pub expected_asset_count: usize,
    pub expected_index_html_size: usize,
}

impl Fixture {
    pub fn id(&self) -> String {
        format!("{}-{}-{}", self.name, self.format, self.arch)
    }

    pub fn binary_path(&self) -> PathBuf {
        fixture_dir().join(&self.extract_dir).join(&self.binary)
    }
}

pub fn fixture_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
}

pub fn load_config() -> &'static Config {
    static CONFIG: OnceLock<Config> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let path = fixture_dir().join("fixtures.toml");
        let content = fs::read_to_string(&path).expect("failed to read fixtures.toml");
        toml::from_str(&content).expect("failed to parse fixtures.toml")
    })
}

pub fn desktop_elf() -> Vec<u8> {
    desktop_elf_with_assets(&[("/index.html", b"<!DOCTYPE html><html></html>" as &[u8])])
}

pub fn nested_desktop_elf() -> Vec<u8> {
    desktop_elf_with_assets(&[
        ("/index.html", b"<!DOCTYPE html><html></html>" as &[u8]),
        (
            "/_app/immutable/chunks/app.js",
            b"console.log('app');" as &[u8],
        ),
        (
            "/_app/immutable/assets/style.css",
            b"body{color:#111}" as &[u8],
        ),
    ])
}

fn desktop_elf_with_assets(assets: &[(&str, &[u8])]) -> Vec<u8> {
    const ELF_HEADER_SIZE: usize = 64;
    const SECTION_HEADER_SIZE: usize = 64;
    const RODATA_ADDR: u64 = 0x400000;
    const DATA_REL_RO_ADDR: u64 = 0x500000;
    const RODATA_OFF: usize = 0x1000;
    const DATA_REL_RO_OFF: usize = 0x2000;
    const SHSTRTAB_OFF: usize = 0x3000;

    let mut rodata = Vec::new();
    let mut headers = Vec::new();
    for (name, content) in assets {
        let name_addr = RODATA_ADDR + rodata.len() as u64;
        rodata.extend_from_slice(name.as_bytes());

        let compressed = brotli_compress(content);
        let data_addr = RODATA_ADDR + rodata.len() as u64;
        rodata.extend_from_slice(&compressed);

        headers.push((
            name_addr,
            name.len() as u64,
            data_addr,
            compressed.len() as u64,
        ));
    }

    let mut data_rel_ro = vec![0; 32 * headers.len()];
    for (index, (name_addr, name_len, data_addr, data_size)) in headers.into_iter().enumerate() {
        let offset = index * 32;
        write_u64(&mut data_rel_ro, offset, name_addr);
        write_u64(&mut data_rel_ro, offset + 8, name_len);
        write_u64(&mut data_rel_ro, offset + 16, data_addr);
        write_u64(&mut data_rel_ro, offset + 24, data_size);
    }

    let shstrtab = b"\0.rodata\0.data.rel.ro\0.shstrtab\0";
    let rodata_name = 1;
    let data_rel_ro_name = rodata_name + b".rodata\0".len();
    let shstrtab_name = data_rel_ro_name + b".data.rel.ro\0".len();
    let section_header_off = SHSTRTAB_OFF + shstrtab.len();
    let mut elf = vec![0; section_header_off + SECTION_HEADER_SIZE * 4];

    write_elf_header(
        &mut elf,
        ElfHeaderArgs {
            typ: 2,
            machine: 62,
            phnum: 1,
            shnum: 4,
            shstrndx: 3,
            section_header_off,
        },
    );
    write_program_header(
        &mut elf,
        ELF_HEADER_SIZE,
        ProgramHeader {
            typ: 1,
            flags: 6,
            offset: 0,
            vaddr: 0,
            paddr: 0,
            filesz: (DATA_REL_RO_OFF + data_rel_ro.len()) as u64,
            memsz: (DATA_REL_RO_OFF + data_rel_ro.len()) as u64,
            align: 0x1000,
        },
    );

    elf[RODATA_OFF..RODATA_OFF + rodata.len()].copy_from_slice(&rodata);
    elf[DATA_REL_RO_OFF..DATA_REL_RO_OFF + data_rel_ro.len()].copy_from_slice(&data_rel_ro);
    elf[SHSTRTAB_OFF..SHSTRTAB_OFF + shstrtab.len()].copy_from_slice(shstrtab);

    let shdr = section_header_off;
    write_section_header(
        &mut elf,
        shdr + SECTION_HEADER_SIZE,
        SectionHeader {
            name: rodata_name as u32,
            typ: 1,
            flags: 2,
            addr: RODATA_ADDR,
            offset: RODATA_OFF as u64,
            size: rodata.len() as u64,
            link: 0,
            info: 0,
            align: 1,
            entsize: 0,
        },
    );
    write_section_header(
        &mut elf,
        shdr + SECTION_HEADER_SIZE * 2,
        SectionHeader {
            name: data_rel_ro_name as u32,
            typ: 1,
            flags: 3,
            addr: DATA_REL_RO_ADDR,
            offset: DATA_REL_RO_OFF as u64,
            size: data_rel_ro.len() as u64,
            link: 0,
            info: 0,
            align: 8,
            entsize: 0,
        },
    );
    write_section_header(
        &mut elf,
        shdr + SECTION_HEADER_SIZE * 3,
        SectionHeader {
            name: shstrtab_name as u32,
            typ: 3,
            flags: 0,
            addr: 0,
            offset: SHSTRTAB_OFF as u64,
            size: shstrtab.len() as u64,
            link: 0,
            info: 0,
            align: 1,
            entsize: 0,
        },
    );

    elf
}

pub fn android_elf_with_rela() -> Vec<u8> {
    android_elf(false)
}

pub fn android_elf_with_later_asset_section() -> Vec<u8> {
    android_elf(true)
}

fn android_elf(use_empty_prefix: bool) -> Vec<u8> {
    const ELF_HEADER_SIZE: usize = 64;
    const SECTION_HEADER_SIZE: usize = 64;
    const RODATA_ADDR: u64 = 0x1000;
    const RELA_ADDR: u64 = 0x2000;
    const DATA_REL_RO_ADDR: u64 = 0x3000;
    const DATA_REL_RO_B_ADDR: u64 = 0x3100;
    const DYNAMIC_ADDR: u64 = 0x3800;
    const RODATA_OFF: usize = 0x1000;
    const RELA_OFF: usize = 0x2000;
    const DATA_REL_RO_OFF: usize = 0x3000;
    const DATA_REL_RO_B_OFF: usize = 0x3100;
    const DYNAMIC_OFF: usize = 0x3800;
    const SHSTRTAB_OFF: usize = 0x4000;

    let html = b"<!DOCTYPE html><html></html>";
    let compressed = brotli_compress(html);
    let mut rodata = Vec::new();
    rodata.extend_from_slice(b"/index.html");
    let data_addr = RODATA_ADDR + rodata.len() as u64;
    rodata.extend_from_slice(&compressed);

    let mut asset_section = vec![0; 32];
    write_u64(&mut asset_section, 8, 11);
    write_u64(&mut asset_section, 24, compressed.len() as u64);

    let shstrtab: &[u8] = if use_empty_prefix {
        b"\0.rodata\0.rela.dyn\0.data.rel.ro.a\0.data.rel.ro.b\0.shstrtab\0"
    } else {
        b"\0.rodata\0.rela.dyn\0.data.rel.ro\0.shstrtab\0"
    };
    let rodata_name = 1;
    let rela_name = rodata_name + b".rodata\0".len();
    let first_data_name = rela_name + b".rela.dyn\0".len();
    let second_data_name = first_data_name + b".data.rel.ro.a\0".len();
    let shstrtab_name = if use_empty_prefix {
        second_data_name + b".data.rel.ro.b\0".len()
    } else {
        first_data_name + b".data.rel.ro\0".len()
    };

    let section_count = if use_empty_prefix { 6 } else { 5 };
    let section_header_off = SHSTRTAB_OFF + shstrtab.len();
    let mut elf = vec![0; section_header_off + SECTION_HEADER_SIZE * section_count];
    write_elf_header(
        &mut elf,
        ElfHeaderArgs {
            typ: 3,
            machine: 183,
            phnum: 2,
            shnum: section_count as u16,
            shstrndx: (section_count - 1) as u16,
            section_header_off,
        },
    );

    write_program_header(
        &mut elf,
        ELF_HEADER_SIZE,
        ProgramHeader {
            typ: 1,
            flags: 6,
            offset: 0,
            vaddr: 0,
            paddr: 0,
            filesz: (DYNAMIC_OFF + 64) as u64,
            memsz: (DYNAMIC_OFF + 64) as u64,
            align: 0x1000,
        },
    );
    write_program_header(
        &mut elf,
        ELF_HEADER_SIZE + 56,
        ProgramHeader {
            typ: 2,
            flags: 6,
            offset: DYNAMIC_OFF as u64,
            vaddr: DYNAMIC_ADDR,
            paddr: DYNAMIC_ADDR,
            filesz: 64,
            memsz: 64,
            align: 8,
        },
    );

    elf[RODATA_OFF..RODATA_OFF + rodata.len()].copy_from_slice(&rodata);
    let real_addr = if use_empty_prefix {
        DATA_REL_RO_B_ADDR
    } else {
        DATA_REL_RO_ADDR
    };
    write_rela_entry(&mut elf, RELA_OFF, real_addr, RODATA_ADDR, 1027);
    write_rela_entry(&mut elf, RELA_OFF + 24, real_addr + 16, data_addr, 1027);
    if use_empty_prefix {
        elf[DATA_REL_RO_OFF..DATA_REL_RO_OFF + 32].copy_from_slice(&[0; 32]);
        elf[DATA_REL_RO_B_OFF..DATA_REL_RO_B_OFF + asset_section.len()]
            .copy_from_slice(&asset_section);
    } else {
        elf[DATA_REL_RO_OFF..DATA_REL_RO_OFF + asset_section.len()].copy_from_slice(&asset_section);
    }
    write_dynamic_entry(&mut elf, DYNAMIC_OFF, 7, RELA_ADDR);
    write_dynamic_entry(&mut elf, DYNAMIC_OFF + 16, 8, 48);
    write_dynamic_entry(&mut elf, DYNAMIC_OFF + 32, 9, 24);
    write_dynamic_entry(&mut elf, DYNAMIC_OFF + 48, 0, 0);
    elf[SHSTRTAB_OFF..SHSTRTAB_OFF + shstrtab.len()].copy_from_slice(shstrtab);

    let shdr = section_header_off;
    write_section_header(
        &mut elf,
        shdr + SECTION_HEADER_SIZE,
        SectionHeader {
            name: rodata_name as u32,
            typ: 1,
            flags: 2,
            addr: RODATA_ADDR,
            offset: RODATA_OFF as u64,
            size: rodata.len() as u64,
            link: 0,
            info: 0,
            align: 1,
            entsize: 0,
        },
    );
    write_section_header(
        &mut elf,
        shdr + SECTION_HEADER_SIZE * 2,
        SectionHeader {
            name: rela_name as u32,
            typ: 4,
            flags: 2,
            addr: RELA_ADDR,
            offset: RELA_OFF as u64,
            size: 48,
            link: 0,
            info: if use_empty_prefix { 4 } else { 3 },
            align: 8,
            entsize: 24,
        },
    );
    write_section_header(
        &mut elf,
        shdr + SECTION_HEADER_SIZE * 3,
        SectionHeader {
            name: first_data_name as u32,
            typ: 1,
            flags: 3,
            addr: DATA_REL_RO_ADDR,
            offset: DATA_REL_RO_OFF as u64,
            size: 32,
            link: 0,
            info: 0,
            align: 8,
            entsize: 0,
        },
    );
    if use_empty_prefix {
        write_section_header(
            &mut elf,
            shdr + SECTION_HEADER_SIZE * 4,
            SectionHeader {
                name: second_data_name as u32,
                typ: 1,
                flags: 3,
                addr: DATA_REL_RO_B_ADDR,
                offset: DATA_REL_RO_B_OFF as u64,
                size: asset_section.len() as u64,
                link: 0,
                info: 0,
                align: 8,
                entsize: 0,
            },
        );
    }
    write_section_header(
        &mut elf,
        shdr + SECTION_HEADER_SIZE * (section_count - 1),
        SectionHeader {
            name: shstrtab_name as u32,
            typ: 3,
            flags: 0,
            addr: 0,
            offset: SHSTRTAB_OFF as u64,
            size: shstrtab.len() as u64,
            link: 0,
            info: 0,
            align: 1,
            entsize: 0,
        },
    );

    elf
}

fn brotli_compress(data: &[u8]) -> Vec<u8> {
    let mut compressed = Vec::new();
    let mut compressor = brotli::CompressorWriter::new(&mut compressed, 4096, 5, 22);
    compressor.write_all(data).unwrap();
    drop(compressor);
    compressed
}

struct ElfHeaderArgs {
    typ: u16,
    machine: u16,
    phnum: u16,
    shnum: u16,
    shstrndx: u16,
    section_header_off: usize,
}

fn write_elf_header(data: &mut [u8], args: ElfHeaderArgs) {
    data[0..4].copy_from_slice(b"\x7fELF");
    data[4] = 2;
    data[5] = 1;
    data[6] = 1;
    write_u16(data, 16, args.typ as u64);
    write_u16(data, 18, args.machine as u64);
    write_u32(data, 20, 1);
    write_u64(data, 32, 64);
    write_u64(data, 40, args.section_header_off as u64);
    write_u16(data, 52, 64);
    write_u16(data, 54, 56);
    write_u16(data, 56, args.phnum as u64);
    write_u16(data, 58, 64);
    write_u16(data, 60, args.shnum as u64);
    write_u16(data, 62, args.shstrndx as u64);
}

struct ProgramHeader {
    typ: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    paddr: u64,
    filesz: u64,
    memsz: u64,
    align: u64,
}

fn write_program_header(data: &mut [u8], offset: usize, header: ProgramHeader) {
    write_u32(data, offset, header.typ);
    write_u32(data, offset + 4, header.flags);
    write_u64(data, offset + 8, header.offset);
    write_u64(data, offset + 16, header.vaddr);
    write_u64(data, offset + 24, header.paddr);
    write_u64(data, offset + 32, header.filesz);
    write_u64(data, offset + 40, header.memsz);
    write_u64(data, offset + 48, header.align);
}

struct SectionHeader {
    name: u32,
    typ: u32,
    flags: u64,
    addr: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    align: u64,
    entsize: u64,
}

fn write_section_header(data: &mut [u8], offset: usize, header: SectionHeader) {
    write_u32(data, offset, header.name);
    write_u32(data, offset + 4, header.typ);
    write_u64(data, offset + 8, header.flags);
    write_u64(data, offset + 16, header.addr);
    write_u64(data, offset + 24, header.offset);
    write_u64(data, offset + 32, header.size);
    write_u32(data, offset + 40, header.link);
    write_u32(data, offset + 44, header.info);
    write_u64(data, offset + 48, header.align);
    write_u64(data, offset + 56, header.entsize);
}

fn write_rela_entry(data: &mut [u8], offset: usize, r_offset: u64, r_addend: u64, r_type: u32) {
    write_u64(data, offset, r_offset);
    write_u64(data, offset + 8, u64::from(r_type));
    write_u64(data, offset + 16, r_addend);
}

fn write_dynamic_entry(data: &mut [u8], offset: usize, tag: u64, value: u64) {
    write_u64(data, offset, tag);
    write_u64(data, offset + 8, value);
}

fn write_u16(data: &mut [u8], offset: usize, value: u64) {
    data[offset..offset + 2].copy_from_slice(&(value as u16).to_le_bytes());
}

fn write_u32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(data: &mut [u8], offset: usize, value: u64) {
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
