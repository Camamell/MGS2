//! `MGS2/STAGE.DAT`: the encrypted archive holding every stage's data.
//!
//! Layout (reverse engineered from `loader.c` code in SLUS_201.44):
//!
//! ```text
//! sector 0   u32 seed                       plaintext
//!            u32 header_info, u32 count, u32 ?   encrypted, Cipher::for_toc(seed)
//!            count x { char name[8]; u32 sector }  same key stream continues
//! stage      u32 n; n x { u32 a; u32 b }    encrypted, Cipher::for_stage(seed, name)
//!            pages, each starting on a sector boundary after the header
//! ```
//!
//! Stage entries form a flat list of pages:
//! - `a = 0x7F0000xx` (xx != 0) or `0x7F010000` opens a page; `b` = unpacked size.
//! - optional `a = 0x7E000000 | packed_size` follows: the page is zlib
//!   compressed and encrypted with [`Cipher::for_page`].
//! - file entries: `a = type << 24 | str_code(name)`, `b` = offset in page.
//! - `a = 0x7F000000` closes the page; its `b` is the page's end offset.

use std::io::{Read, Seek, SeekFrom};

use flate2::read::ZlibDecoder;

use crate::cipher::Cipher;
use crate::iso::SECTOR;
use crate::{Result, format_err};

const PAGE_END: u32 = 0x7F00_0000;

#[derive(Clone, Debug)]
pub struct StageEntry {
    pub name: String,
    /// Start sector relative to the start of STAGE.DAT.
    pub sector: u32,
    /// Size in sectors (distance to the next stage or end of file).
    pub sectors: u32,
}

pub struct StageDat<R> {
    reader: R,
    seed: u32,
    stages: Vec<StageEntry>,
}

impl<R: Read + Seek> StageDat<R> {
    pub fn open(mut reader: R) -> Result<Self> {
        let total = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;
        let mut head = [0u8; 16];
        reader.read_exact(&mut head)?;
        let seed = u32::from_le_bytes(head[0..4].try_into().unwrap());
        let mut cipher = Cipher::for_toc(seed);
        cipher.apply(&mut head[4..16]);
        let count = (le32(&head, 8) & 0xFFFF) as usize;

        let mut toc = vec![0u8; count * 12];
        reader.read_exact(&mut toc)?;
        cipher.apply(&mut toc);

        let mut stages: Vec<StageEntry> = toc
            .as_chunks::<12>().0.iter()
            .map(|e| StageEntry {
                name: String::from_utf8_lossy(&e[..8]).trim_end_matches('\0').to_string(),
                sector: le32(e, 8),
                sectors: 0,
            })
            .collect();
        let total_sectors = (total / SECTOR) as u32;
        for i in 0..stages.len() {
            let next = stages.get(i + 1).map_or(total_sectors, |s| s.sector);
            stages[i].sectors = next.saturating_sub(stages[i].sector);
        }
        Ok(Self { reader, seed, stages })
    }

    pub fn stages(&self) -> &[StageEntry] {
        &self.stages
    }

    pub fn stage(&self, name: &str) -> Option<&StageEntry> {
        self.stages.iter().find(|s| s.name == name)
    }

    /// Reads and decodes all pages of a stage.
    pub fn read_stage(&mut self, name: &str) -> Result<Stage> {
        let Some(entry) = self.stage(name).cloned() else {
            return format_err(format!("no stage named '{name}'"));
        };
        let base = u64::from(entry.sector) * SECTOR;
        let mut cipher = Cipher::for_stage(self.seed, name);

        self.reader.seek(SeekFrom::Start(base))?;
        let mut header = vec![0u8; SECTOR as usize];
        self.reader.read_exact(&mut header)?;
        cipher.apply(&mut header);
        let n = le32(&header, 0) as usize;
        let header_len = round_sector(4 + n as u64 * 8);
        if header_len > SECTOR {
            let mut rest = vec![0u8; (header_len - SECTOR) as usize];
            self.reader.read_exact(&mut rest)?;
            cipher.apply(&mut rest);
            header.extend_from_slice(&rest);
        }
        let entries: Vec<(u32, u32)> =
            (0..n).map(|i| (le32(&header, 4 + i * 8), le32(&header, 8 + i * 8))).collect();

        let mut pages = Vec::new();
        let mut pos = header_len;
        let mut i = 0;
        while i < n {
            let (kind, unpacked) = entries[i];
            if !is_page_start(kind) {
                i += 1;
                continue;
            }
            let packed = entries
                .get(i + 1)
                .filter(|(a, _)| a & 0xFF00_0000 == 0x7E00_0000)
                .map(|(a, _)| a & 0x00FF_FFFF);
            let stored = packed.unwrap_or(unpacked);

            self.reader.seek(SeekFrom::Start(base + pos))?;
            let mut raw = vec![0u8; stored as usize];
            self.reader.read_exact(&mut raw)?;
            let data = if packed.is_some() {
                inflate_page(raw, unpacked as usize)?
            } else {
                raw
            };

            let mut files = Vec::new();
            let mut j = i + 1 + usize::from(packed.is_some());
            while j < n && entries[j].0 != PAGE_END {
                let (id, offset) = entries[j];
                let end = entries.get(j + 1).map_or(unpacked, |e| e.1);
                files.push(FileEntry { id, offset, size: end.saturating_sub(offset) });
                j += 1;
            }
            pages.push(Page { kind, offset: pos, compressed: packed.is_some(), data, files });
            pos += round_sector(u64::from(stored));
            i = j + 1;
        }
        Ok(Stage { name: entry.name, pages })
    }
}

pub struct Stage {
    pub name: String,
    pub pages: Vec<Page>,
}

impl Stage {
    pub fn files(&self) -> impl Iterator<Item = (&Page, &FileEntry)> {
        self.pages.iter().flat_map(|p| p.files.iter().map(move |f| (p, f)))
    }
}

pub struct Page {
    /// `0x7F010000` (resident image loaded to 0x1F1480), `0x7F00000{1,2,3}`
    /// (cached data pages), `0x7F000010` (sound data, not handled by the loader).
    pub kind: u32,
    /// Byte offset of the page within the stage.
    pub offset: u64,
    pub compressed: bool,
    pub data: Vec<u8>,
    pub files: Vec<FileEntry>,
}

impl Page {
    pub fn file_data(&self, f: &FileEntry) -> &[u8] {
        let start = (f.offset as usize).min(self.data.len());
        let end = (start + f.size as usize).min(self.data.len());
        &self.data[start..end]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FileEntry {
    /// `type << 24 | str_code(name)`.
    pub id: u32,
    pub offset: u32,
    pub size: u32,
}

impl FileEntry {
    pub fn type_id(&self) -> u8 {
        ((self.id & 0x7FFF_FFFF) >> 24) as u8
    }

    pub fn name_hash(&self) -> u32 {
        self.id & 0x00FF_FFFF
    }

    /// File extension for this entry's type. The type is the extension's first
    /// letter minus 'a'; full names follow JoyDict's MGS2 notes. Type 17 covers
    /// several `r*` extensions (rat, row, rpd, ...) and stays as `r`.
    pub fn extension(&self) -> String {
        let t = self.type_id();
        let known = match t {
            0 => "anm",
            1 => "bin", // resident code image (page kind 0x7F010000)
            2 => "cv2",
            4 => "evm",
            5 => "far",
            6 => "gcx", // GCL script
            7 => "hzx", // collision
            10 => "kms", // model
            11 => "lt2", // lighting
            12 => "mar", // motion archive ("MARa")
            14 => "o2d",
            18 => "sar",
            19 => "tri", // texture set
            21 => "var",
            25 => "zms",
            _ => "",
        };
        if !known.is_empty() {
            known.to_string()
        } else if t < 26 {
            char::from(b'a' + t).to_string()
        } else {
            format!("t{t}")
        }
    }
}

/// Tanker chapter stages: playable areas w00*-w09*, plus the chapter's
/// cutscene (d0?t, d00?p??) and radio (r_tnk*) stages. The cutscene/radio
/// grouping is inferred from naming and still needs confirming in-game.
pub fn is_tanker_stage(name: &str) -> bool {
    let b = name.as_bytes();
    (b.len() == 4 && b[0] == b'w' && b[1] == b'0')
        || (b.len() == 4 && name.starts_with("d0") && b[3] == b't')
        || name.starts_with("d00")
        || name.starts_with("r_tnk")
}

fn is_page_start(a: u32) -> bool {
    (a & 0xFF00_0000 == 0x7F00_0000 && a != PAGE_END && a & 0x00FF_0000 == 0) || a == 0x7F01_0000
}

fn inflate_page(mut raw: Vec<u8>, unpacked: usize) -> Result<Vec<u8>> {
    if raw.len() < 2 {
        return format_err("compressed page too small");
    }
    let first = u16::from_le_bytes([raw[0], raw[1]]);
    Cipher::for_page(first).apply(&mut raw);
    let h = u16::from_le_bytes([raw[0], raw[1]]).wrapping_add(0x08F3);
    raw[..2].copy_from_slice(&h.to_le_bytes());
    let mut out = Vec::with_capacity(unpacked);
    ZlibDecoder::new(&raw[..]).read_to_end(&mut out)?;
    Ok(out)
}

fn round_sector(n: u64) -> u64 {
    n.div_ceil(SECTOR) * SECTOR
}

fn le32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}
