use crate::asset::sha256_hex;
use crate::binary::{self, BinaryMetadata, BinaryParser};
use crate::error::Result;
use std::fs;
use std::path::{Path, PathBuf};

pub struct BinaryImage {
    data: Vec<u8>,
    parser: Box<dyn BinaryParser>,
    metadata: BinaryMetadata,
}

impl BinaryImage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let data = fs::read(path)?;
        Self::from_data(data, Some(path.to_path_buf()))
    }

    pub fn from_bytes(data: impl AsRef<[u8]>) -> Result<Self> {
        Self::from_data(data.as_ref().to_vec(), None)
    }

    fn from_data(data: Vec<u8>, source_path: Option<PathBuf>) -> Result<Self> {
        let parsed = binary::create_parser(&data)?;
        let metadata = BinaryMetadata {
            kind: parsed.kind,
            architecture: parsed.architecture,
            file_size: data.len(),
            sha256: sha256_hex(&data),
            source_path: source_path.map(|path| path.display().to_string()),
        };

        Ok(Self {
            data,
            parser: parsed.parser,
            metadata,
        })
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn metadata(&self) -> &BinaryMetadata {
        &self.metadata
    }

    pub(crate) fn parser(&self) -> &dyn BinaryParser {
        self.parser.as_ref()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }
}
