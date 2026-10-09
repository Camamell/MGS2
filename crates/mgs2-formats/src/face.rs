//! FACE.DAT: the Codec's 3D faces.
//!
//! A header sector: `u32 stamp, u16 1, u16 1, u16 count, ...` and from +16
//! `count` entries of `char name[8], u32 sector`. Entries are Codec casts
//! (`f00a` the Tanker's Snake and Otacon, `f01a`..`f06a` the Plant's Raiden,
//! Rose, Campbell...) and menus (`node`, `photo`, `capture`, `mobile`). Each is a plaintext stage-style
//! page list (`u32 n`, `n` pairs `a, b`; see `stage_dat`): a cached page
//! (`0x7F000002`, `b` its size) holding the files `type << 24 | hash`. Faces
//! are `.evm` heads with their texture sets (`sna_radio_mh_mt`,
//! `otc_radio_mh_mt`, `otc_hair_mh`), `.kms` parts (`otc_glasses_mt`),
//! `cdc_<who>.mar` / `.sar` motions and `lip_*.rmt` lip data (names from
//! JoyDict).

use std::io::{Read, Seek, SeekFrom};

#[derive(Clone, Debug)]
pub struct FaceFile {
    pub type_id: u8,
    pub hash: u32,
    pub data: Vec<u8>,
}

fn le32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// Entry names and start sectors.
pub fn entries<R: Read + Seek>(r: &mut R) -> std::io::Result<Vec<(String, u64)>> {
    let mut head = vec![0u8; 0x800];
    r.seek(SeekFrom::Start(0))?;
    r.read_exact(&mut head)?;
    let count = u16::from_le_bytes([head[8], head[9]]) as usize;
    Ok((0..count.min(160))
        .map(|i| {
            let o = 16 + i * 12;
            let name = String::from_utf8_lossy(&head[o..o + 8]).trim_end_matches('\0').to_string();
            (name, u64::from(le32(&head, o + 8).unwrap_or(0)))
        })
        .collect())
}

/// The files of entry `name`.
pub fn read<R: Read + Seek>(r: &mut R, name: &str) -> std::io::Result<Vec<FaceFile>> {
    let list = entries(r)?;
    let Some(i) = list.iter().position(|(n, _)| n == name) else { return Ok(Vec::new()) };
    let sector = list[i].1;
    let mut head = vec![0u8; 0x800];
    r.seek(SeekFrom::Start(sector * 0x800))?;
    r.read_exact(&mut head)?;
    let n = le32(&head, 0).unwrap_or(0) as usize;
    let table = 4 + n * 8;
    if table > head.len() {
        head.resize(table.div_ceil(0x800) * 0x800, 0);
        r.seek(SeekFrom::Start(sector * 0x800))?;
        r.read_exact(&mut head)?;
    }
    let mut out = Vec::new();
    let mut pages = table.div_ceil(0x800) * 0x800;
    let mut page: Option<(usize, usize)> = None; // (start in entry, size)
    let mut files: Vec<(u8, u32, usize)> = Vec::new();
    let flush = |page: Option<(usize, usize)>, files: &mut Vec<(u8, u32, usize)>, r: &mut R, out: &mut Vec<FaceFile>| -> std::io::Result<()> {
        let Some((start, size)) = page else { return Ok(()) };
        let mut data = vec![0u8; size];
        r.seek(SeekFrom::Start(sector * 0x800 + start as u64))?;
        r.read_exact(&mut data)?;
        // Files are listed by type, not offset: each runs to the next
        // larger offset in the page.
        for (t, h, off) in files.iter() {
            let end = files.iter().map(|f| f.2).filter(|o| o > off).min().unwrap_or(size).min(size);
            let off = (*off).min(end);
            out.push(FaceFile { type_id: *t, hash: *h, data: data[off..end].to_vec() });
        }
        files.clear();
        Ok(())
    };
    for k in 0..n {
        let (a, b) = (le32(&head, 4 + k * 8).unwrap_or(0), le32(&head, 8 + k * 8).unwrap_or(0) as usize);
        if a & 0xFF00_0000 == 0x7F00_0000 && a != 0x7F00_0000 && a & 0x00FF_0000 == 0 {
            flush(page, &mut files, r, &mut out)?;
            page = Some((pages, b));
            pages += b.div_ceil(0x800) * 0x800;
        } else if a == 0x7F00_0000 || a & 0xFF00_0000 == 0x7E00_0000 {
            continue;
        } else {
            files.push(((a >> 24) as u8, a & 0x00FF_FFFF, b));
        }
    }
    flush(page, &mut files, r, &mut out)?;
    Ok(out)
}
