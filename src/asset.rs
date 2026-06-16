use crate::binary::{BinaryMetadata, ScanRange};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

pub const ASSET_HEADER_SIZE: usize = size_of::<AssetHeader>();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetHeader {
    pub name_ptr: u64,
    pub name_len: u64,
    pub data_ptr: u64,
    pub data_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetId(String);

impl AssetId {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetLocation {
    pub header_offset: usize,
    pub name_offset: usize,
    pub data_offset: usize,
    pub data_size_offset: usize,
    pub original_compressed_size: usize,
    pub scan_range: ScanRange,
}

#[derive(Debug, Clone)]
pub struct Asset {
    id: AssetId,
    name: String,
    compressed_data: Vec<u8>,
    decompressed_size: usize,
    location: AssetLocation,
    compressed_sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetSummary {
    pub name: String,
    pub compressed_size: usize,
    pub decompressed_size: usize,
    pub compressed_sha256: String,
    pub location: AssetLocation,
}

impl Asset {
    pub fn new(
        name: String,
        compressed_data: Vec<u8>,
        decompressed_size: usize,
        location: AssetLocation,
    ) -> Self {
        let compressed_sha256 = sha256_hex(&compressed_data);
        Self {
            id: AssetId::new(name.clone()),
            name,
            compressed_data,
            decompressed_size,
            location,
            compressed_sha256,
        }
    }

    pub fn id(&self) -> &AssetId {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn compressed_data(&self) -> &[u8] {
        &self.compressed_data
    }

    pub fn compressed_size(&self) -> usize {
        self.compressed_data.len()
    }

    pub fn decompressed_size(&self) -> usize {
        self.decompressed_size
    }

    pub fn location(&self) -> &AssetLocation {
        &self.location
    }

    pub fn compressed_sha256(&self) -> &str {
        &self.compressed_sha256
    }

    pub fn safe_relative_path(&self) -> Option<PathBuf> {
        safe_relative_path(&self.name)
    }

    pub fn summary(&self) -> AssetSummary {
        AssetSummary {
            name: self.name.clone(),
            compressed_size: self.compressed_size(),
            decompressed_size: self.decompressed_size,
            compressed_sha256: self.compressed_sha256.clone(),
            location: self.location.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AssetTable {
    metadata: BinaryMetadata,
    assets: Vec<Asset>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetTableSummary {
    pub binary: BinaryMetadata,
    pub asset_count: usize,
    pub total_compressed_size: usize,
    pub total_decompressed_size: usize,
    pub assets: Vec<AssetSummary>,
}

impl AssetTable {
    pub fn new(metadata: BinaryMetadata, assets: Vec<Asset>) -> Self {
        Self { metadata, assets }
    }

    pub fn metadata(&self) -> &BinaryMetadata {
        &self.metadata
    }

    pub fn assets(&self) -> &[Asset] {
        &self.assets
    }

    pub fn len(&self) -> usize {
        self.assets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    pub fn find(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.name() == name)
    }

    pub fn summary(&self) -> AssetTableSummary {
        AssetTableSummary {
            binary: self.metadata.clone(),
            asset_count: self.len(),
            total_compressed_size: self.assets.iter().map(Asset::compressed_size).sum(),
            total_decompressed_size: self.assets.iter().map(Asset::decompressed_size).sum(),
            assets: self.assets.iter().map(Asset::summary).collect(),
        }
    }
}

pub(crate) fn read_header(data: &[u8], offset: usize) -> Option<AssetHeader> {
    Some(AssetHeader {
        name_ptr: read_u64(data, offset)?,
        name_len: read_u64(data, offset + 8)?,
        data_ptr: read_u64(data, offset + 16)?,
        data_size: read_u64(data, offset + 24)?,
    })
}

pub(crate) fn write_u64(data: &mut [u8], offset: usize, value: u64) -> bool {
    let Some(bytes) = data.get_mut(offset..offset + 8) else {
        return false;
    };
    bytes.copy_from_slice(&value.to_le_bytes());
    true
}

pub(crate) fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

pub fn safe_relative_path(asset_name: &str) -> Option<PathBuf> {
    let stripped = asset_name.strip_prefix('/').unwrap_or(asset_name);
    if stripped.is_empty() {
        return None;
    }

    let path = Path::new(stripped);
    if path.is_absolute() {
        return None;
    }

    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

fn read_u64(data: &[u8], offset: usize) -> Option<u64> {
    let bytes = data.get(offset..offset + 8)?;
    Some(u64::from_le_bytes(bytes.try_into().ok()?))
}
