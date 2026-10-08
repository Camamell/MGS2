//! Minimal read-only ISO 9660 access, enough to locate files on the game disc.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::{Result, format_err};

pub const SECTOR: u64 = 0x800;

pub struct Iso {
    file: BufReader<File>,
    root: (u32, u32),
}

impl Iso {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut file = BufReader::new(File::open(path)?);
        let mut pvd = [0u8; 0x800];
        file.seek(SeekFrom::Start(16 * SECTOR))?;
        file.read_exact(&mut pvd)?;
        if &pvd[1..6] != b"CD001" || pvd[0] != 1 {
            return format_err("not an ISO 9660 image (no primary volume descriptor)");
        }
        let root = parse_record(&pvd[156..190]).1;
        Ok(Self { file, root })
    }

    /// Finds a file by path such as `MGS2/STAGE.DAT` (case-insensitive) and
    /// returns a reader limited to that file's extent.
    pub fn open_file(mut self, path: &str) -> Result<SubFile<BufReader<File>>> {
        let mut dir = self.root;
        let parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
        for (i, part) in parts.iter().enumerate() {
            let last = i + 1 == parts.len();
            dir = self
                .find_in_dir(dir, part, !last)?
                .ok_or_else(|| crate::Error::Format(format!("'{path}' not found in ISO")))?;
        }
        SubFile::new(self.file, u64::from(dir.0) * SECTOR, u64::from(dir.1))
    }

    fn find_in_dir(&mut self, (lba, size): (u32, u32), name: &str, want_dir: bool) -> Result<Option<(u32, u32)>> {
        let mut buf = vec![0u8; size as usize];
        self.file.seek(SeekFrom::Start(u64::from(lba) * SECTOR))?;
        self.file.read_exact(&mut buf)?;
        let mut off = 0;
        while off < buf.len() {
            let len = buf[off] as usize;
            if len == 0 {
                // Records never cross sectors; skip to the next one.
                off = (off / SECTOR as usize + 1) * SECTOR as usize;
                continue;
            }
            let rec = &buf[off..off + len];
            let (is_dir, extent) = parse_record(rec);
            let ident = &rec[33..33 + rec[32] as usize];
            let ident = String::from_utf8_lossy(ident);
            let ident = ident.split(';').next().unwrap_or_default();
            if is_dir == want_dir && ident.eq_ignore_ascii_case(name) {
                return Ok(Some(extent));
            }
            off += len;
        }
        Ok(None)
    }
}

fn parse_record(rec: &[u8]) -> (bool, (u32, u32)) {
    let lba = u32::from_le_bytes(rec[2..6].try_into().unwrap());
    let size = u32::from_le_bytes(rec[10..14].try_into().unwrap());
    (rec[25] & 2 != 0, (lba, size))
}

/// A `Read + Seek` window onto part of another reader.
pub struct SubFile<R> {
    inner: R,
    start: u64,
    len: u64,
    pos: u64,
}

impl<R: Read + Seek> SubFile<R> {
    pub fn new(mut inner: R, start: u64, len: u64) -> Result<Self> {
        inner.seek(SeekFrom::Start(start))?;
        Ok(Self { inner, start, len, pos: 0 })
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<R: Read + Seek> Read for SubFile<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.len.saturating_sub(self.pos) as usize;
        let take = buf.len().min(left);
        let n = self.inner.read(&mut buf[..take])?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl<R: Read + Seek> Seek for SubFile<R> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let new = match to {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::Current(d) => self.pos as i64 + d,
            SeekFrom::End(d) => self.len as i64 + d,
        };
        if new < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = new as u64;
        self.inner.seek(SeekFrom::Start(self.start + self.pos))?;
        Ok(self.pos)
    }
}
