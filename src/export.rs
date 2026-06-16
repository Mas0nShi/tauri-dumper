use crate::asset::{safe_relative_path, Asset, AssetTable};
use crate::error::{Error, Result};
use crate::extract::decompress_asset;
use crate::manifest::{Manifest, MANIFEST_FILE_NAME};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub output_dir: PathBuf,
    pub overwrite: bool,
    pub skip_existing: bool,
    pub dry_run: bool,
    pub write_manifest: bool,
    include: GlobSet,
    exclude: GlobSet,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportRecord {
    pub name: String,
    pub path: PathBuf,
    pub status: ExportStatus,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportStatus {
    Exported,
    SkippedExisting,
    SkippedFilter,
    DryRun,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportSummary {
    pub output_dir: PathBuf,
    pub exported: usize,
    pub skipped_existing: usize,
    pub skipped_filter: usize,
    pub dry_run: bool,
    pub records: Vec<ExportRecord>,
}

impl ExportOptions {
    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
            overwrite: true,
            skip_existing: false,
            dry_run: false,
            write_manifest: true,
            include: GlobSetBuilder::new().build().expect("empty globset"),
            exclude: GlobSetBuilder::new().build().expect("empty globset"),
        }
    }

    pub fn include_globs(mut self, globs: &[String]) -> Result<Self> {
        self.include = build_globset(globs)?;
        Ok(self)
    }

    pub fn exclude_globs(mut self, globs: &[String]) -> Result<Self> {
        self.exclude = build_globset(globs)?;
        Ok(self)
    }

    fn includes(&self, asset: &Asset) -> bool {
        (self.include.is_empty() || self.include.is_match(asset.name()))
            && !self.exclude.is_match(asset.name())
    }
}

impl AssetTable {
    pub fn export(&self, options: &ExportOptions) -> Result<ExportSummary> {
        let mut summary = ExportSummary {
            output_dir: options.output_dir.clone(),
            exported: 0,
            skipped_existing: 0,
            skipped_filter: 0,
            dry_run: options.dry_run,
            records: Vec::new(),
        };

        if !options.dry_run {
            fs::create_dir_all(&options.output_dir)?;
        }

        for asset in self.assets() {
            let path = asset_output_path(&options.output_dir, asset)?;

            if !options.includes(asset) {
                summary.skipped_filter += 1;
                summary.records.push(ExportRecord {
                    name: asset.name().to_string(),
                    path,
                    status: ExportStatus::SkippedFilter,
                });
                continue;
            }

            if path.exists() && !options.overwrite {
                if options.skip_existing {
                    summary.skipped_existing += 1;
                    summary.records.push(ExportRecord {
                        name: asset.name().to_string(),
                        path,
                        status: ExportStatus::SkippedExisting,
                    });
                    continue;
                }
                return Err(Error::OutputExists(path));
            }

            if options.dry_run {
                summary.records.push(ExportRecord {
                    name: asset.name().to_string(),
                    path,
                    status: ExportStatus::DryRun,
                });
                continue;
            }

            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, decompress_asset(asset)?)?;
            summary.exported += 1;
            summary.records.push(ExportRecord {
                name: asset.name().to_string(),
                path,
                status: ExportStatus::Exported,
            });
        }

        if options.write_manifest && !options.dry_run {
            let manifest = Manifest::from_asset_table(self);
            manifest.write(options.output_dir.join(MANIFEST_FILE_NAME))?;
        }

        Ok(summary)
    }
}

pub fn asset_output_path(base: &Path, asset: &Asset) -> Result<PathBuf> {
    let relative = safe_relative_path(asset.name()).ok_or_else(|| Error::PathTraversal {
        asset: asset.name().to_string(),
    })?;
    Ok(base.join(relative))
}

fn build_globset(globs: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for glob in globs {
        builder.add(Glob::new(glob).map_err(|err| Error::Message(err.to_string()))?);
    }
    builder
        .build()
        .map_err(|err| Error::Message(err.to_string()))
}
