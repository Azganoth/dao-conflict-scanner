#![allow(dead_code)]
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

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

impl ErfFile {
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let mut file = File::open(path)?;
        Self::from_reader(&mut file)
    }

    pub fn from_reader<R: Read + Seek>(reader: &mut R) -> io::Result<Self> {
        let mut file_type = [0u8; 8];
        let mut file_version = [0u8; 8];
        reader.read_exact(&mut file_type)?;
        reader.read_exact(&mut file_version)?;

        if decode_utf16le(&file_type) != "ERF " {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid file type: {:?}", file_type),
            ));
        }

        match decode_utf16le(&file_version).as_str() {
            "V2.0" => ErfFile::parse(reader, ErfVersion::V20),
            "V2.2" => ErfFile::parse(reader, ErfVersion::V22),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Unsupported ERF version: {:?}", file_version),
                ));
            }
        }
    }

    fn parse<R: Read + Seek>(reader: &mut R, version: ErfVersion) -> io::Result<Self> {
        // Read common header fields
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

        // Read TOC entries
        let mut toc = Vec::with_capacity(file_count as usize);
        let mut by_name = HashMap::new();

        for _ in 0..file_count {
            let mut entry_data = [0u8; 72]; // 64 bytes name + 8 bytes data
            reader.read_exact(&mut entry_data)?;

            // Decode UTF-16LE name
            let name_bytes = &entry_data[0..64];
            let name = decode_utf16le(name_bytes)
                .trim_end_matches('\0')
                .to_string();

            let offset = read_u32(&entry_data[64..68]);
            let lengths = match version {
                ErfVersion::V20 => (read_u32(&entry_data[68..72]), read_u32(&entry_data[68..72])),
                ErfVersion::V22 => (read_u32(&entry_data[68..72]), read_u32(&entry_data[72..76])),
            };

            toc.push(ErfTocEntry {
                name: name.clone(),
                offset,
                packed_length: lengths.0,
                length: lengths.1,
            });

            by_name.insert(name.to_lowercase(), toc.len() - 1);
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

    pub fn get_resource(&self, name: &str, reader: &mut (impl Read + Seek)) -> io::Result<Vec<u8>> {
        let index = self.by_name.get(&name.to_lowercase()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("Resource not found: {}", name),
            )
        })?;

        let entry = &self.toc[*index];
        reader.seek(SeekFrom::Start(entry.offset as u64))?;
        let mut data = vec![0u8; entry.length as usize];
        reader.read_exact(&mut data)?;

        Ok(data)
    }
}

fn decode_utf16le(bytes: &[u8]) -> String {
    let mut result = String::new();
    let mut chunks = bytes.chunks_exact(2);

    for chunk in &mut chunks {
        let val = u16::from_le_bytes([chunk[0], chunk[1]]);
        if val != 0 {
            result.push(char::from_u32(val as u32).unwrap_or('�'));
        }
    }

    result
}

// Helper functions for reading primitives (same as before)
fn read_u32(bytes: &[u8]) -> u32 {
    let mut buf = [0u8; 4];
    buf.copy_from_slice(bytes);
    u32::from_le_bytes(buf)
}

// Example usage updated for version handling
fn main() -> io::Result<()> {
    use std::env;
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <erf-file> [resource-name]", args[0]);
        std::process::exit(1);
    }

    let erf = ErfFile::open(&args[1])?;

    println!(
        "ERF 2.{} File:",
        match erf.version {
            ErfVersion::V20 => "0",
            ErfVersion::V22 => "2",
        }
    );
    println!(
        "Year: {}, Day: {}, Module ID: {}",
        erf.year, erf.day, erf.module_id
    );
    println!("Resources ({}):", erf.toc.len());
    for entry in &erf.toc {
        println!(
            "- {} (Packed: {} bytes, Unpacked: {} bytes)",
            entry.name, entry.packed_length, entry.length
        );
    }

    if args.len() > 2 {
        let mut file = File::open(&args[1])?;
        let data = erf.get_resource(&args[2], &mut file)?;
        println!("\nResource '{}' contents ({} bytes):", args[2], data.len());
        println!("{:02X?}", &data[..data.len().min(32)]);
    }

    Ok(())
}
