//! Asset dumper for Tauri applications.

use crate::binary::{self, BinaryParser};
use anyhow::{anyhow, Result};
use memmap2::Mmap;
use std::fs::File;
use std::io::{Cursor, Read};

/// Size of the asset header structure in bytes.
const ASSET_HEADER_SIZE: usize = size_of::<AssetHeader>();

/// Raw asset header as stored in the binary.
#[repr(C)]
#[derive(Debug)]
struct AssetHeader {
    name_ptr: u64,
    name_len: u64,
    data_ptr: u64,
    data_size: u64,
}

/// A parsed asset with its name and compressed data.
#[derive(Debug)]
pub struct Asset {
    pub name: String,
    pub data: Vec<u8>,
}

/// Extracts embedded assets from Tauri application binaries.
pub struct Dumper {
    mmap: Mmap,
    parser: Box<dyn BinaryParser>,
}

impl Dumper {
    /// Creates a new dumper for the given file.
    pub fn new(file: File) -> Result<Self> {
        let mmap = unsafe { Mmap::map(&file)? };
        let parser = binary::create_parser(&mmap)?;

        Ok(Self { mmap, parser })
    }

    /// Scans the binary for embedded assets.
    pub fn scan_assets(&self) -> Result<Vec<Asset>> {
        let mut assets = Vec::new();

        for range in self.parser.scan_ranges()? {
            self.scan_assets_in_range(range, &mut assets)?;
        }

        Ok(assets)
    }

    fn scan_assets_in_range(
        &self,
        range: binary::ScanRange,
        assets: &mut Vec<Asset>,
    ) -> Result<()> {
        let end = range
            .start
            .checked_add(range.length)
            .ok_or_else(|| anyhow!("Scan range exceeds file bounds"))?;

        if end > self.mmap.len() {
            return Err(anyhow!("Scan range exceeds file bounds"));
        }

        let mut offset = range.start;
        let mut step = 8; // Initial alignment

        while offset + ASSET_HEADER_SIZE <= end {
            if let Ok(asset) = self.try_parse_asset(offset) {
                assets.push(asset);
                step = ASSET_HEADER_SIZE; // Align to header size after finding an asset
            }
            offset += step;
        }

        Ok(())
    }

    /// Attempts to parse an asset at the given file offset.
    fn try_parse_asset(&self, offset: usize) -> Result<Asset> {
        let header = self.read_header(offset)?;

        let name_offset = self.parser.resolve_pointer(header.name_ptr)?;
        let data_offset = self.parser.resolve_pointer(header.data_ptr)?;

        self.validate_pointers(name_offset, header.name_len, data_offset, header.data_size)?;

        let name = self.read_name(name_offset as usize, header.name_len as usize)?;
        let data = self.read_data(data_offset as usize, header.data_size as usize)?;

        Ok(Asset { name, data })
    }

    /// Reads an asset header from the given offset.
    fn read_header(&self, offset: usize) -> Result<AssetHeader> {
        if offset + ASSET_HEADER_SIZE > self.mmap.len() {
            return Err(anyhow!("Header offset out of bounds"));
        }

        Ok(AssetHeader {
            name_ptr: self.parser.read_pointer(&self.mmap, offset)?,
            name_len: binary::read_u64(&self.mmap, offset + 8)?,
            data_ptr: self.parser.read_pointer(&self.mmap, offset + 16)?,
            data_size: binary::read_u64(&self.mmap, offset + 24)?,
        })
    }

    /// Validates that the pointers point to valid data.
    fn validate_pointers(
        &self,
        name_offset: u64,
        name_len: u64,
        data_offset: u64,
        data_size: u64,
    ) -> Result<()> {
        let name_off = name_offset as usize;
        let data_off = data_offset as usize;
        let name_len = name_len as usize;
        let data_size = data_size as usize;

        // Bounds check
        if name_off >= self.mmap.len()
            || name_off.saturating_add(name_len) > self.mmap.len()
            || data_off >= self.mmap.len()
            || data_off.saturating_add(data_size) > self.mmap.len()
        {
            return Err(anyhow!("Pointer out of file bounds"));
        }

        // Name must start with '/'
        if self.mmap[name_off] != b'/' {
            return Err(anyhow!("Invalid asset name format"));
        }

        // Data must be valid brotli-compressed
        self.verify_brotli(&self.mmap[data_off..data_off + data_size])?;

        Ok(())
    }

    /// Verifies that data is valid brotli-compressed content.
    fn verify_brotli(&self, data: &[u8]) -> Result<()> {
        let mut decompressor = brotli::Decompressor::new(data, data.len());
        let mut buf = Vec::new();
        decompressor
            .read_to_end(&mut buf)
            .map_err(|_| anyhow!("Invalid brotli data"))?;
        Ok(())
    }

    /// Reads the asset name from the given offset.
    fn read_name(&self, offset: usize, len: usize) -> Result<String> {
        let bytes = &self.mmap[offset..offset + len];

        if !bytes.iter().all(|&b| b.is_ascii()) {
            return Err(anyhow!("Asset name contains non-ASCII characters"));
        }

        String::from_utf8(bytes.to_vec()).map_err(Into::into)
    }

    /// Reads the compressed asset data from the given offset.
    fn read_data(&self, offset: usize, len: usize) -> Result<Vec<u8>> {
        Ok(self.mmap[offset..offset + len].to_vec())
    }

    /// Decompresses an asset's data.
    pub fn decompress_asset(&self, asset: &Asset) -> Result<Vec<u8>> {
        let reader = Cursor::new(&asset.data);
        let mut decompressor = brotli::Decompressor::new(reader, asset.data.len());
        let mut decompressed = Vec::new();
        decompressor.read_to_end(&mut decompressed)?;
        Ok(decompressed)
    }
}
