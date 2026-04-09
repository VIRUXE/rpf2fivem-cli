#![allow(dead_code)]
pub mod crypto;
pub mod keys;

use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use keys::GtaKeys;
use crypto::{decrypt_aes, decrypt_ng, decompress};

const RPF7_MAGIC: u32 = 0x52504637;
const RESOURCE_IDENT: u32 = 0x37435352;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RpfEncryption {
    None,
    Open,
    Aes,
    Ng,
}

impl RpfEncryption {
    fn from_u32(v: u32) -> Self {
        match v {
            0x00000000 => Self::None,
            0x4E45504F => Self::Open,  // "OPEN" - no encryption
            0x0FFFFFF9 => Self::Aes,
            0x0FEFFFFF => Self::Ng,
            _ => Self::Ng,             // default to NG for unknown values
        }
    }
}

#[derive(Debug)]
pub struct RpfEntry {
    pub name: String,
    pub name_lower: String,
    pub kind: RpfEntryKind,
}

#[derive(Debug)]
pub enum RpfEntryKind {
    Directory {
        entries_index: u32,
        entries_count: u32,
    },
    BinaryFile {
        file_offset: u32,
        file_size: u32,
        uncompressed_size: u32,
        is_encrypted: bool,
    },
    ResourceFile {
        file_offset: u32,
        file_size: u32,
        system_flags: u32,
        graphics_flags: u32,
        is_encrypted: bool,
    },
}

pub struct RpfArchive {
    pub path: String,
    pub start_pos: u64,
    pub encryption: RpfEncryption,
    pub entries: Vec<RpfEntry>,
}

impl RpfArchive {
    /// Open and parse an RPF7 archive header.
    pub fn open(path: &str) -> Result<Self> {
        let mut f = fs::File::open(path)
            .with_context(|| format!("Cannot open RPF: {}", path))?;
        Self::read_from(&mut f, path, 0)
    }

    fn read_from<R: Read + Seek>(reader: &mut R, path: &str, start_pos: u64) -> Result<Self> {
        reader.seek(SeekFrom::Start(start_pos))?;

        let version = read_u32(reader)?;
        if version != RPF7_MAGIC {
            bail!("Not a valid RPF7 archive (magic={:#010x})", version);
        }

        let entry_count = read_u32(reader)? as usize;
        let names_length = read_u32(reader)? as usize;
        let encryption_raw = read_u32(reader)?;
        let encryption = RpfEncryption::from_u32(encryption_raw);

        let mut entries_data = vec![0u8; entry_count * 16];
        reader.read_exact(&mut entries_data)?;

        let mut names_data = vec![0u8; names_length];
        reader.read_exact(&mut names_data)?;

        // Decrypt entries/names if needed (keys not required for header-only open)
        // Decryption happens in open_with_keys

        let entries = parse_entries(&entries_data, &names_data, entry_count)?;

        Ok(Self {
            path: path.to_string(),
            start_pos,
            encryption,
            entries,
        })
    }

    /// Open an RPF that requires decryption.
    pub fn open_with_keys(path: &str, keys: Option<&GtaKeys>) -> Result<Self> {
        let data = fs::read(path)
            .with_context(|| format!("Cannot read RPF: {}", path))?;
        Self::parse_from_bytes(&data, path, 0, keys)
    }

    pub fn parse_from_bytes(
        data: &[u8],
        path: &str,
        start_pos: usize,
        keys: Option<&GtaKeys>,
    ) -> Result<Self> {
        let d = &data[start_pos..];

        if d.len() < 16 {
            bail!("RPF data too short");
        }

        let version = u32::from_le_bytes(d[0..4].try_into().unwrap());
        if version != RPF7_MAGIC {
            bail!("Not a valid RPF7 (magic={:#010x})", version);
        }

        let entry_count = u32::from_le_bytes(d[4..8].try_into().unwrap()) as usize;
        let names_length = u32::from_le_bytes(d[8..12].try_into().unwrap()) as usize;
        let encryption_raw = u32::from_le_bytes(d[12..16].try_into().unwrap());
        let encryption = RpfEncryption::from_u32(encryption_raw);

        let header_size = 16;
        let entries_offset = header_size;
        let entries_size = entry_count * 16;
        let names_offset = entries_offset + entries_size;

        if d.len() < names_offset + names_length {
            bail!("RPF header truncated");
        }

        let mut entries_data = d[entries_offset..entries_offset + entries_size].to_vec();
        let mut names_data = d[names_offset..names_offset + names_length].to_vec();

        // Decrypt if needed
        let (is_aes, is_ng) = match (encryption, keys) {
            (RpfEncryption::Aes, Some(k)) => {
                entries_data = decrypt_aes(&entries_data, &k.aes_key);
                names_data = decrypt_aes(&names_data, &k.aes_key);
                (true, false)
            }
            (RpfEncryption::Ng, Some(k)) => {
                let fname = Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                let fsize = data.len() as u32;
                entries_data = decrypt_ng(&entries_data, k, fname, fsize);
                names_data = decrypt_ng(&names_data, k, fname, fsize);
                (false, true)
            }
            _ => (false, false),
        };
        let _ = (is_aes, is_ng); // used for future extension

        let entries = parse_entries(&entries_data, &names_data, entry_count)?;

        Ok(Self {
            path: path.to_string(),
            start_pos: start_pos as u64,
            encryption,
            entries,
        })
    }

    /// Extract all files from this RPF, calling the callback for each file's data.
    pub fn extract_all(
        &self,
        data: &[u8],
        keys: Option<&GtaKeys>,
        mut on_file: impl FnMut(&str, Vec<u8>),
    ) -> Result<()> {
        self.extract_all_inner(data, keys, &mut on_file)
    }

    fn extract_all_inner(
        &self,
        data: &[u8],
        keys: Option<&GtaKeys>,
        on_file: &mut dyn FnMut(&str, Vec<u8>),
    ) -> Result<()> {
        let is_aes = self.encryption == RpfEncryption::Aes;

        for entry in &self.entries {
            match &entry.kind {
                RpfEntryKind::BinaryFile {
                    file_offset,
                    file_size,
                    uncompressed_size,
                    is_encrypted,
                } => {
                    let byte_offset = self.start_pos as usize + (*file_offset as usize * 512);
                    let size = if *file_size == 0 {
                        *uncompressed_size as usize
                    } else {
                        *file_size as usize
                    };

                    if byte_offset + size > data.len() {
                        eprintln!("[RPF] Binary file {} out of bounds, skipping", entry.name_lower);
                        continue;
                    }

                    let raw = &data[byte_offset..byte_offset + size];
                    let mut buf = raw.to_vec();

                    if *is_encrypted {
                        if let Some(k) = keys {
                            buf = if is_aes {
                                decrypt_aes(&buf, &k.aes_key)
                            } else {
                                decrypt_ng(&buf, k, &entry.name, *uncompressed_size)
                            };
                        }
                    }

                    let out = if *file_size > 0 {
                        match decompress(&buf) {
                            Ok(d) => d,
                            Err(_) => buf,
                        }
                    } else {
                        buf
                    };

                    // Recursively extract nested RPF archives
                    if entry.name_lower.ends_with(".rpf") {
                        match RpfArchive::parse_from_bytes(&out, &entry.name_lower, 0, keys) {
                            Ok(nested) => {
                                if let Err(e) = nested.extract_all_inner(&out, keys, on_file) {
                                    eprintln!("[RPF] Error extracting nested {}: {}", entry.name_lower, e);
                                }
                            }
                            Err(e) => {
                                eprintln!("[RPF] Failed to parse nested {}: {}", entry.name_lower, e);
                            }
                        }
                    } else {
                        on_file(&entry.name_lower, out);
                    }
                }

                RpfEntryKind::ResourceFile {
                    file_offset,
                    file_size,
                    system_flags,
                    graphics_flags,
                    is_encrypted,
                } => {
                    if *file_size == 0 {
                        continue;
                    }

                    let byte_offset = self.start_pos as usize + (*file_offset as usize * 512);
                    let header_skip = 16usize;
                    let total = *file_size as usize;

                    if total <= header_skip || byte_offset + total > data.len() {
                        eprintln!("[RPF] Resource {} out of bounds, skipping", entry.name_lower);
                        continue;
                    }

                    let raw = &data[byte_offset + header_skip..byte_offset + total];
                    let mut buf = raw.to_vec();

                    if *is_encrypted {
                        if let Some(k) = keys {
                            buf = if is_aes {
                                decrypt_aes(&buf, &k.aes_key)
                            } else {
                                decrypt_ng(&buf, k, &entry.name, *file_size)
                            };
                        }
                    }

                    let deflated = decompress(&buf).unwrap_or(buf);

                    // Add resource header (RSC7 header)
                    let mut out = Vec::with_capacity(deflated.len() + 16);
                    out.extend_from_slice(&RESOURCE_IDENT.to_le_bytes());
                    // Version field from system_flags top nibble
                    let version = (*system_flags >> 28) & 0xF;
                    out.extend_from_slice(&version.to_le_bytes());
                    out.extend_from_slice(&system_flags.to_le_bytes());
                    out.extend_from_slice(&graphics_flags.to_le_bytes());

                    // Re-compress for FiveM streaming
                    let recompressed = crypto::compress(&deflated);
                    out.extend_from_slice(&recompressed);

                    on_file(&entry.name_lower, out);
                }

                RpfEntryKind::Directory { .. } => {}
            }
        }

        Ok(())
    }
}

fn parse_entries(entries_data: &[u8], names_data: &[u8], count: usize) -> Result<Vec<RpfEntry>> {
    let mut entries = Vec::with_capacity(count);

    for i in 0..count {
        let offset = i * 16;
        if offset + 16 > entries_data.len() {
            break;
        }

        let chunk = &entries_data[offset..offset + 16];
        let h1 = u32::from_le_bytes(chunk[0..4].try_into().unwrap());
        let h2 = u32::from_le_bytes(chunk[4..8].try_into().unwrap());

        if h2 == 0x7FFFFF00 {
            // Directory entry
            let name_offset = h1 as usize;
            let entries_index = u32::from_le_bytes(chunk[8..12].try_into().unwrap());
            let entries_count = u32::from_le_bytes(chunk[12..16].try_into().unwrap());
            let name = read_cstring(names_data, name_offset);
            let name_lower = name.to_lowercase();
            entries.push(RpfEntry {
                name,
                name_lower,
                kind: RpfEntryKind::Directory { entries_index, entries_count },
            });
            continue;
        } else if (h2 & 0x80000000) == 0 {
            // Binary file entry
            let buf = chunk;
            let name_offset = u16::from_le_bytes(buf[0..2].try_into().unwrap()) as usize;
            let file_size = (buf[2] as u32) | ((buf[3] as u32) << 8) | ((buf[4] as u32) << 16);
            let file_offset = (buf[5] as u32) | ((buf[6] as u32) << 8) | ((buf[7] as u32) << 16);
            let uncompressed_size = u32::from_le_bytes(buf[8..12].try_into().unwrap());
            let encryption_type = u32::from_le_bytes(buf[12..16].try_into().unwrap());
            let is_encrypted = encryption_type == 1;
            let name = read_cstring(names_data, name_offset);
            let name_lower = name.to_lowercase();
            entries.push(RpfEntry {
                name,
                name_lower,
                kind: RpfEntryKind::BinaryFile {
                    file_offset,
                    file_size,
                    uncompressed_size,
                    is_encrypted,
                },
            });
            continue;
        } else {
            // Resource file entry
            let buf = chunk;
            let name_offset = u16::from_le_bytes(buf[0..2].try_into().unwrap()) as usize;
            let file_size = (buf[2] as u32) | ((buf[3] as u32) << 8) | ((buf[4] as u32) << 16);
            let file_offset_raw = (buf[5] as u32) | ((buf[6] as u32) << 8) | ((buf[7] as u32) << 16);
            let file_offset = file_offset_raw & 0x7FFFFF;
            let system_flags = u32::from_le_bytes(buf[8..12].try_into().unwrap());
            let graphics_flags = u32::from_le_bytes(buf[12..16].try_into().unwrap());
            let name = read_cstring(names_data, name_offset);
            let name_lower = name.to_lowercase();
            let is_encrypted = name_lower.ends_with(".ysc");
            entries.push(RpfEntry {
                name,
                name_lower,
                kind: RpfEntryKind::ResourceFile {
                    file_offset,
                    file_size,
                    system_flags,
                    graphics_flags,
                    is_encrypted,
                },
            });
            continue;
        };
    }

    Ok(entries)
}

fn read_cstring(data: &[u8], offset: usize) -> String {
    if offset >= data.len() {
        return String::new();
    }
    let end = data[offset..]
        .iter()
        .position(|&b| b == 0)
        .map(|p| offset + p)
        .unwrap_or(data.len());
    String::from_utf8_lossy(&data[offset..end]).into_owned()
}

fn name_lower_is_ysc(name: &str) -> bool {
    name.to_lowercase().ends_with(".ysc")
}

fn read_u32<R: Read>(r: &mut R) -> Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

/// Find all .rpf files nested within extracted data and return their byte ranges.
pub fn find_nested_rpfs(data: &[u8], start: usize) -> Vec<usize> {
    let mut offsets = Vec::new();
    let magic = RPF7_MAGIC.to_le_bytes();
    let mut i = start;
    while i + 4 <= data.len() {
        if data[i..i + 4] == magic {
            offsets.push(i);
            i += 512;
        } else {
            i += 4;
        }
    }
    offsets
}
