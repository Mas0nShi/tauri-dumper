use crate::asset::{safe_relative_path, write_u64, AssetTable};
use crate::codec;
use crate::error::{Error, Result};
use crate::image::BinaryImage;
use crate::manifest::{Manifest, MANIFEST_FILE_NAME};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Repacker {
    image: BinaryImage,
    table: AssetTable,
}

pub struct RepackPlan {
    image: BinaryImage,
    table: AssetTable,
    assets_dir: PathBuf,
    strict: bool,
    skip_oversized: bool,
    dry_run: bool,
    allow_source_mismatch: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepackSummary {
    pub output: Option<PathBuf>,
    pub replaced: usize,
    pub unchanged: usize,
    pub skipped_oversized: usize,
    pub oversized: Vec<OversizedReplacement>,
    pub unsupported_additions: Vec<PathBuf>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct OversizedReplacement {
    pub asset: String,
    pub original_compressed_size: usize,
    pub new_compressed_size: usize,
    pub delta: usize,
}

impl Repacker {
    pub fn new(image: BinaryImage, table: AssetTable) -> Self {
        Self { image, table }
    }

    pub fn replace_from_dir(self, assets_dir: impl Into<PathBuf>) -> RepackPlan {
        RepackPlan {
            image: self.image,
            table: self.table,
            assets_dir: assets_dir.into(),
            strict: false,
            skip_oversized: false,
            dry_run: false,
            allow_source_mismatch: false,
        }
    }
}

impl RepackPlan {
    pub fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    pub fn skip_oversized(mut self, skip_oversized: bool) -> Self {
        self.skip_oversized = skip_oversized;
        self
    }

    pub fn dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    pub fn allow_source_mismatch(mut self, allow: bool) -> Self {
        self.allow_source_mismatch = allow;
        self
    }

    pub fn write(self, output: impl Into<PathBuf>) -> Result<RepackSummary> {
        let output = output.into();
        self.validate_manifest()?;

        let known_paths = known_asset_paths(&self.table);
        let unsupported_additions = find_unsupported_additions(&self.assets_dir, &known_paths)?;
        if self.strict && !unsupported_additions.is_empty() {
            return Err(Error::UnsupportedAddition(unsupported_additions[0].clone()));
        }

        let mut patched = self.image.data().to_vec();
        let mut replaced = 0;
        let mut unchanged = 0;
        let mut skipped_oversized = 0;
        let mut oversized = Vec::new();

        for asset in self.table.assets() {
            let Some(relative) = safe_relative_path(asset.name()) else {
                unchanged += 1;
                continue;
            };
            let replacement_path = self.assets_dir.join(relative);
            if !replacement_path.is_file() {
                unchanged += 1;
                continue;
            }

            let replacement = fs::read(&replacement_path)?;
            let compressed = codec::compress_best(&replacement)?;
            let max_size = asset.location().original_compressed_size;
            if compressed.data.len() > max_size {
                let record = OversizedReplacement {
                    asset: asset.name().to_string(),
                    original_compressed_size: max_size,
                    new_compressed_size: compressed.data.len(),
                    delta: compressed.data.len() - max_size,
                };
                if self.skip_oversized {
                    skipped_oversized += 1;
                    oversized.push(record);
                    continue;
                }
                return Err(Error::ReplacementTooLarge {
                    asset: record.asset,
                    new_size: record.new_compressed_size,
                    max_size: record.original_compressed_size,
                });
            }

            if !self.dry_run {
                let start = asset.location().data_offset;
                let end = start + max_size;
                let target = patched
                    .get_mut(start..end)
                    .ok_or(Error::ScanRangeOutOfBounds)?;
                target[..compressed.data.len()].copy_from_slice(&compressed.data);
                target[compressed.data.len()..].fill(0);

                if !write_u64(
                    &mut patched,
                    asset.location().data_size_offset,
                    compressed.data.len() as u64,
                ) {
                    return Err(Error::ScanRangeOutOfBounds);
                }
            }
            replaced += 1;
        }

        if !self.dry_run {
            write_atomic(&output, &patched)?;
        }

        Ok(RepackSummary {
            output: (!self.dry_run).then_some(output),
            replaced,
            unchanged,
            skipped_oversized,
            oversized,
            unsupported_additions,
            dry_run: self.dry_run,
        })
    }

    fn validate_manifest(&self) -> Result<()> {
        let manifest_path = self.assets_dir.join(MANIFEST_FILE_NAME);
        if !manifest_path.is_file() {
            return Ok(());
        }

        let manifest = Manifest::read(manifest_path)?;
        if manifest.source.sha256 != self.image.metadata().sha256 && !self.allow_source_mismatch {
            return Err(Error::SourceMismatch {
                expected: manifest.source.sha256,
                actual: self.image.metadata().sha256.clone(),
            });
        }
        Ok(())
    }
}

fn known_asset_paths(table: &AssetTable) -> HashMap<PathBuf, String> {
    table
        .assets()
        .iter()
        .filter_map(|asset| {
            safe_relative_path(asset.name()).map(|path| (path, asset.name().to_string()))
        })
        .collect()
}

fn find_unsupported_additions(
    assets_dir: &Path,
    known_paths: &HashMap<PathBuf, String>,
) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_files(assets_dir, assets_dir, &mut files)?;

    let known = known_paths.keys().cloned().collect::<HashSet<_>>();
    Ok(files
        .into_iter()
        .filter(|path| path != Path::new(MANIFEST_FILE_NAME))
        .filter(|path| !known.contains(path))
        .collect())
}

fn collect_files(root: &Path, current: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !current.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else if path.is_file() {
            if let Ok(relative) = path.strip_prefix(root) {
                out.push(relative.to_path_buf());
            }
        }
    }
    Ok(())
}

fn write_atomic(output: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp = output.with_extension(format!(
        "{}tmp",
        output
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| format!("{ext}."))
            .unwrap_or_default()
    ));
    fs::write(&tmp, data)?;
    fs::rename(tmp, output)?;
    Ok(())
}
