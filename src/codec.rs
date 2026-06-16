use crate::error::{Error, Result};
use std::io::{Read, Write};

const BROTLI_QUALITIES: [u32; 12] = [11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0];
const BROTLI_LGWIN: u32 = 22;

#[derive(Debug, Clone)]
pub struct CompressionResult {
    pub data: Vec<u8>,
    pub quality: u32,
    pub lgwin: u32,
}

pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut decompressor = brotli::Decompressor::new(data, data.len());
    let mut output = Vec::new();
    decompressor
        .read_to_end(&mut output)
        .map_err(|_| Error::InvalidBrotli)?;
    Ok(output)
}

pub fn compress(data: &[u8]) -> Result<Vec<u8>> {
    Ok(compress_best(data)?.data)
}

pub fn compress_best(data: &[u8]) -> Result<CompressionResult> {
    let mut best = None;

    for quality in BROTLI_QUALITIES {
        let candidate = compress_with_params(data, quality, BROTLI_LGWIN)?;
        if best
            .as_ref()
            .is_none_or(|best: &CompressionResult| candidate.data.len() < best.data.len())
        {
            best = Some(candidate);
        }
    }

    best.ok_or_else(|| Error::Message("failed to produce Brotli output".to_string()))
}

fn compress_with_params(data: &[u8], quality: u32, lgwin: u32) -> Result<CompressionResult> {
    let mut output = Vec::new();
    {
        let buffer_size = data.len().max(4096);
        let mut compressor =
            brotli::CompressorWriter::new(&mut output, buffer_size, quality, lgwin);
        compressor.write_all(data)?;
    }
    Ok(CompressionResult {
        data: output,
        quality,
        lgwin,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_compression_tries_multiple_quality_levels() {
        let data = b"function test(){return 'tauri-dumper'.repeat(128)}";
        let best = compress_best(data).unwrap();

        for quality in BROTLI_QUALITIES {
            let candidate = compress_with_params(data, quality, BROTLI_LGWIN).unwrap();
            assert!(
                best.data.len() <= candidate.data.len(),
                "best quality {} produced {} bytes, quality {} produced {} bytes",
                best.quality,
                best.data.len(),
                quality,
                candidate.data.len()
            );
        }

        assert_eq!(decompress(&best.data).unwrap(), data);
    }
}
