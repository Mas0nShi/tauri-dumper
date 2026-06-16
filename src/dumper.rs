use crate::asset::{Asset, AssetTable};
use crate::error::Result;
use crate::extract::{decompress_asset, AssetScanner};
use crate::image::BinaryImage;
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub struct Dumper {
    image: BinaryImage,
}

impl Dumper {
    pub fn new(mut file: File) -> Result<Self> {
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        Self::from_bytes(&data)
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            image: BinaryImage::open(path)?,
        })
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Ok(Self {
            image: BinaryImage::from_bytes(data)?,
        })
    }

    pub fn scan(&self) -> Result<AssetTable> {
        AssetScanner::scan(&self.image)
    }

    pub fn scan_assets(&self) -> Result<Vec<Asset>> {
        Ok(self.scan()?.assets().to_vec())
    }

    pub fn decompress_asset(&self, asset: &Asset) -> Result<Vec<u8>> {
        decompress_asset(asset)
    }

    pub fn image(&self) -> &BinaryImage {
        &self.image
    }
}
