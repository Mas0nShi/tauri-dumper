use crate::asset::{read_header, Asset, AssetLocation, AssetTable, ASSET_HEADER_SIZE};
use crate::codec;
use crate::error::{Error, Result};
use crate::image::BinaryImage;

pub struct AssetScanner;

impl AssetScanner {
    pub fn scan(image: &BinaryImage) -> Result<AssetTable> {
        let mut assets = Vec::new();

        for range in image.parser().scan_ranges()? {
            let end = range
                .start
                .checked_add(range.length)
                .ok_or(Error::ScanRangeOutOfBounds)?;

            if end > image.data().len() {
                return Err(Error::ScanRangeOutOfBounds);
            }

            let mut offset = range.start;
            let mut step = 8;
            while offset + ASSET_HEADER_SIZE <= end {
                if let Ok(asset) = Self::parse_asset(image, offset, range) {
                    assets.push(asset);
                    step = ASSET_HEADER_SIZE;
                }
                offset += step;
            }
        }

        Ok(AssetTable::new(image.metadata().clone(), assets))
    }

    fn parse_asset(
        image: &BinaryImage,
        offset: usize,
        range: crate::binary::ScanRange,
    ) -> Result<Asset> {
        let header =
            read_header(image.data(), offset).ok_or_else(|| Error::InvalidAssetHeader {
                offset,
                reason: "header is out of bounds".to_string(),
            })?;

        let name_ptr = image.parser().read_pointer(image.data(), offset)?;
        let data_ptr = image.parser().read_pointer(image.data(), offset + 16)?;
        let name_offset = image.parser().resolve_pointer(name_ptr)? as usize;
        let data_offset = image.parser().resolve_pointer(data_ptr)? as usize;
        let name_len = usize::try_from(header.name_len).map_err(|_| Error::InvalidAssetHeader {
            offset,
            reason: "name length does not fit in usize".to_string(),
        })?;
        let data_size =
            usize::try_from(header.data_size).map_err(|_| Error::InvalidAssetHeader {
                offset,
                reason: "data size does not fit in usize".to_string(),
            })?;

        if name_len == 0 || name_len > 4096 {
            return Err(Error::InvalidAssetHeader {
                offset,
                reason: "name length is not plausible".to_string(),
            });
        }
        if data_size == 0 {
            return Err(Error::InvalidAssetHeader {
                offset,
                reason: "data size is zero".to_string(),
            });
        }

        let name_end =
            name_offset
                .checked_add(name_len)
                .ok_or_else(|| Error::InvalidAssetHeader {
                    offset,
                    reason: "name range overflows".to_string(),
                })?;
        let data_end =
            data_offset
                .checked_add(data_size)
                .ok_or_else(|| Error::InvalidAssetHeader {
                    offset,
                    reason: "data range overflows".to_string(),
                })?;

        let name_bytes =
            image
                .data()
                .get(name_offset..name_end)
                .ok_or_else(|| Error::InvalidAssetHeader {
                    offset,
                    reason: "name range is outside the file".to_string(),
                })?;
        let compressed =
            image
                .data()
                .get(data_offset..data_end)
                .ok_or_else(|| Error::InvalidAssetHeader {
                    offset,
                    reason: "data range is outside the file".to_string(),
                })?;

        if name_bytes.first() != Some(&b'/') || !name_bytes.iter().all(u8::is_ascii) {
            return Err(Error::InvalidAssetName);
        }

        let name = String::from_utf8(name_bytes.to_vec()).map_err(|_| Error::InvalidAssetName)?;
        let decompressed = codec::decompress(compressed)?;
        let location = AssetLocation {
            header_offset: offset,
            name_offset,
            data_offset,
            data_size_offset: offset + 24,
            original_compressed_size: data_size,
            scan_range: range,
        };

        Ok(Asset::new(
            name,
            compressed.to_vec(),
            decompressed.len(),
            location,
        ))
    }
}

pub fn decompress_asset(asset: &Asset) -> Result<Vec<u8>> {
    codec::decompress(asset.compressed_data())
}
