use crate::asset::{AssetLocation, AssetTable};
use crate::binary::{BinaryKind, BinaryMetadata};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const MANIFEST_FILE_NAME: &str = "tauri-dumper.manifest.json";
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub source: ManifestSource,
    pub assets: Vec<ManifestAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestSource {
    pub path: Option<String>,
    pub sha256: String,
    pub file_size: usize,
    pub binary_kind: BinaryKind,
    pub architecture: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestAsset {
    pub name: String,
    pub location: AssetLocation,
    pub original_compressed_size: usize,
    pub decompressed_size: usize,
    pub compressed_sha256: String,
}

impl Manifest {
    pub fn from_asset_table(table: &AssetTable) -> Self {
        let metadata: &BinaryMetadata = table.metadata();
        Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            source: ManifestSource {
                path: metadata.source_path.clone(),
                sha256: metadata.sha256.clone(),
                file_size: metadata.file_size,
                binary_kind: metadata.kind,
                architecture: metadata.architecture.clone(),
            },
            assets: table
                .assets()
                .iter()
                .map(|asset| ManifestAsset {
                    name: asset.name().to_string(),
                    location: asset.location().clone(),
                    original_compressed_size: asset.location().original_compressed_size,
                    decompressed_size: asset.decompressed_size(),
                    compressed_sha256: asset.compressed_sha256().to_string(),
                })
                .collect(),
        }
    }

    pub fn read(path: impl AsRef<Path>) -> Result<Self> {
        let data = fs::read(path)?;
        let manifest: Self = serde_json::from_slice(&data)?;
        if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(Error::Manifest(format!(
                "unsupported schema version {}",
                manifest.schema_version
            )));
        }
        Ok(manifest)
    }

    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        let data = serde_json::to_vec_pretty(self)?;
        fs::write(path, data)?;
        Ok(())
    }
}
