//! Lists FACE.DAT (Codec faces): a table of `char name[8], u32 sector`
//! entries, each a plaintext stage-style page list. With an out dir, writes
//! every texture of one entry as PNG.
//! `cargo run -p mgs2-formats --example face_dump -- <iso> [entry out_dir]`

use std::io::{Read, Seek, SeekFrom};

fn le32(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let iso = args.next().expect("usage: face_dump <iso> [entry out_dir]");
    let want = args.next();
    let out = args.next().map(std::path::PathBuf::from);
    let mut f = mgs2_formats::iso::Iso::open(std::path::Path::new(&iso)).expect("iso").open_file("MGS2/FACE.DAT").expect("FACE.DAT");
    let mut head = vec![0u8; 0x800];
    f.read_exact(&mut head).unwrap();
    let count = u16::from_le_bytes([head[8], head[9]]) as usize;
    let mut entries = Vec::new();
    for i in 0..count {
        let o = 16 + i * 12;
        let name = String::from_utf8_lossy(&head[o..o + 8]).trim_end_matches('\0').to_string();
        entries.push((name, le32(&head, o + 8) as u64));
    }
    for (i, (name, sector)) in entries.iter().enumerate() {
        let end = entries.get(i + 1).map_or(sector + 1, |e| e.1);
        f.seek(SeekFrom::Start(sector * 0x800)).unwrap();
        let mut d = vec![0u8; ((end - sector) * 0x800) as usize];
        f.read_exact(&mut d).unwrap();
        let n = le32(&d, 0) as usize;
        let mut files = Vec::new();
        let mut page_start = 0usize;
        let mut page = 0usize;
        for k in 0..n {
            let (a, b) = (le32(&d, 4 + k * 8), le32(&d, 8 + k * 8));
            if a & 0xFF00_0000 == 0x7F00_0000 && a != 0x7F00_0000 && a & 0x00FF_0000 == 0 {
                page_start = (4 + n * 8).div_ceil(0x800) * 0x800 + page;
                page += (b as usize).div_ceil(0x800) * 0x800;
                continue;
            }
            if a == 0x7F00_0000 || a & 0xFF00_0000 == 0x7E00_0000 {
                continue;
            }
            files.push(((a >> 24) as u8, a & 0xFF_FFFF, page_start + b as usize));
        }
        println!("{name}: sector {sector:#x}, {} files: {}", files.len(), files.iter().map(|(t, h, _)| format!("{t}:{h:06x}")).collect::<Vec<_>>().join(" "));
        if want.as_deref() == Some(name.as_str())
            && let Some(o) = &out
        {
            std::fs::create_dir_all(o).unwrap();
            for (j, (t, h, off)) in files.iter().enumerate() {
                let next = files.get(j + 1).map_or(d.len(), |x| x.2);
                let bytes = &d[*off..next.max(*off).min(d.len())];
                std::fs::write(o.join(format!("{name}_{h:06x}.t{t}")), bytes).unwrap();
                if *t == 19 {
                    match mgs2_formats::tri::parse(bytes) {
                        Ok(set) => {
                            for tex in set {
                                let file = std::fs::File::create(o.join(format!("{name}_{h:06x}_{:06x}.png", tex.hash))).unwrap();
                                let mut enc = png::Encoder::new(std::io::BufWriter::new(file), tex.width, tex.height);
                                enc.set_color(png::ColorType::Rgba);
                                enc.set_depth(png::BitDepth::Eight);
                                enc.write_header().unwrap().write_image_data(&tex.rgba).unwrap();
                            }
                        }
                        Err(e) => println!("  {h:06x}: {e}"),
                    }
                }
            }
        }
    }
}
