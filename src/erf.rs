#![allow(dead_code)]
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

use anyhow::{Context, Result as AnyhowResult};
use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum ErfError {
    #[error("Invalid file header: expected {expected:?}, found {found:?}")]
    InvalidHeader { expected: String, found: String },

    #[error("Unsupported ERF version: {0}")]
    UnsupportedVersion(String),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Invalid resource name: {0}")]
    InvalidResourceName(String),

    #[error("Invalid UTF-16 character in string")]
    InvalidStringEncoding,
}

#[derive(Debug)]
pub struct ErfFile {
    pub version: ErfVersion,
    pub year: u32,
    pub day: u32,
    pub module_id: u32,
    pub toc: Vec<ErfTocEntry>,
    pub by_name: HashMap<String, usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ErfVersion {
    V20,
    V22,
}

#[derive(Debug)]
pub struct ErfTocEntry {
    pub name: String,
    pub offset: u32,
    pub packed_length: u32,
    pub length: u32,
}

#[derive(Debug)]
pub struct ResourceEntry {
    pub resref: String,
    pub resid: u16,
    pub restype: u16,
    pub offset: u32,
    pub length: u32,
}

pub type ErfResult<T> = Result<T, ErfError>;

impl ErfFile {
    pub fn open<P: AsRef<Path>>(path: P) -> AnyhowResult<Self> {
        let path_ref = path.as_ref();
        let mut file = File::open(path_ref)
            .with_context(|| format!("Failed to open ERF file at {}", path_ref.display()))?;

        Self::from_reader(&mut file)
            .with_context(|| format!("Failed to parse ERF file at {}", path_ref.display()))
    }

    pub fn get_resource<R: Read + Seek>(
        &self,
        name: &str,
        reader: &mut R,
    ) -> AnyhowResult<Vec<u8>> {
        let key = name.to_lowercase();
        let index = self
            .by_name
            .get(&key)
            .ok_or_else(|| ErfError::InvalidResourceName(name.to_string()))?;

        let entry = &self.toc[*index];

        reader
            .seek(SeekFrom::Start(entry.offset as u64))
            .context("Failed to seek to resource offset")?;

        let mut data = vec![0u8; entry.length as usize];
        reader
            .read_exact(&mut data)
            .context("Failed to read resource data")?;

        Ok(data)
    }

    fn from_reader<R: Read + Seek>(reader: &mut R) -> ErfResult<Self> {
        let (magic, version_str) = Self::read_header(reader)?;

        let version = match (magic.as_str(), version_str.as_str()) {
            ("ERF ", "V2.0") => ErfVersion::V20,
            ("ERF ", "V2.2") => ErfVersion::V22,
            ("ERF ", _) => return Err(ErfError::UnsupportedVersion(version_str)),
            (found, _) => {
                return Err(ErfError::InvalidHeader {
                    expected: "ERF ".to_string(),
                    found: found.to_string(),
                });
            }
        };

        Self::parse(reader, version)
    }

    fn read_header<R: Read>(reader: &mut R) -> ErfResult<(String, String)> {
        let mut header = [0u8; 16];
        reader.read_exact(&mut header)?;

        let magic = decode_utf16le(&header[0..8])?;
        let version = decode_utf16le(&header[8..16])?;

        Ok((magic, version))
    }

    fn parse<R: Read + Seek>(reader: &mut R, version: ErfVersion) -> ErfResult<Self> {
        let mut header = [0u8; 16];
        reader.read_exact(&mut header)?;

        let file_count = read_u32(&header[0..4]);
        let year = read_u32(&header[4..8]);
        let day = read_u32(&header[8..12]);
        let module_id = if version == ErfVersion::V22 {
            read_u32(&header[12..16])
        } else {
            0
        };

        let mut toc = Vec::with_capacity(file_count as usize);
        let mut by_name = HashMap::with_capacity(file_count as usize);

        for i in 0..file_count {
            let entry_size = if version == ErfVersion::V22 { 76 } else { 72 };
            let mut entry_data = vec![0u8; entry_size];

            reader.read_exact(&mut entry_data)?;

            let name = decode_utf16le(&entry_data[0..64])?;

            if name.is_empty() {
                return Err(ErfError::InvalidResourceName(format!(
                    "Empty resource name in TOC at index {i}"
                )));
            }

            let offset = read_u32(&entry_data[64..68]);
            let packed_length = read_u32(&entry_data[68..72]);
            let length = if version == ErfVersion::V22 {
                read_u32(&entry_data[72..76])
            } else {
                packed_length
            };

            toc.push(ErfTocEntry {
                name: name.clone(),
                offset,
                packed_length,
                length,
            });

            by_name.insert(name.to_lowercase(), i as usize);
        }

        Ok(Self {
            version,
            year,
            day,
            module_id,
            toc,
            by_name,
        })
    }
}

fn decode_utf16le(bytes: &[u8]) -> ErfResult<String> {
    if bytes.len() % 2 != 0 {
        return Err(ErfError::InvalidStringEncoding);
    }

    let u16_values: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();

    // Gracefully handle invalid UTF-16 sequences (critical for V2.2 compatibility)
    let mut result = String::from_utf16_lossy(&u16_values);
    if result.ends_with('\0') {
        result.truncate(result.trim_end_matches('\0').len());
    }

    Ok(result)
}

fn read_u32(bytes: &[u8]) -> u32 {
    let mut buf = [0u8; 4];
    buf.copy_from_slice(bytes);
    u32::from_le_bytes(buf)
}
