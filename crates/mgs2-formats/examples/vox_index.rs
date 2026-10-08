//! Lists the clips in VOX.DAT: start sector, length (frames, from the end
//! chunk 0xf0) and the first subtitle line.
//! `cargo run -p mgs2-formats --example vox_index -- <iso> [file]`

use std::io::Read;

fn main() {
    let mut args = std::env::args().skip(1);
    let iso = args.next().expect("usage: vox_index <iso> [MGS2/VOX.DAT]");
    let name = args.next().unwrap_or_else(|| "MGS2/VOX.DAT".into());
    let mut f = mgs2_formats::iso::Iso::open(std::path::Path::new(&iso)).expect("iso").open_file(&name).expect("file");
    let mut data = Vec::new();
    f.read_to_end(&mut data).expect("read");
    let u32_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    let mut sector = 0usize;
    while (sector + 1) * 0x800 <= data.len() {
        let o = sector * 0x800;
        if !(u32_at(o) == 0x10 && u32_at(o + 4) == 0x10) {
            sector += 1;
            continue;
        }
        let mut p = o;
        let (mut frames, mut text) = (0, String::new());
        while p + 16 <= data.len() {
            let (kind, size, frame) = (u32_at(p), u32_at(p + 4) as usize, u32_at(p + 8));
            if !(8..=0x10_0000).contains(&size) {
                break;
            }
            if kind == 4 && text.is_empty() && p + 36 <= data.len() {
                // u32 size, then records `u32 start, end, ?, len, text`.
                let rec = p + 20;
                let len = u32_at(rec + 12) as usize;
                let t = &data[rec + 16..(rec + len).min(data.len())];
                text = String::from_utf8_lossy(t.split(|b| *b == 0).next().unwrap_or(&[])).into_owned();
            }
            if kind == 0xF0 {
                frames = frame;
                p += size;
                break;
            }
            p += size;
        }
        println!("{sector:6} {:#010x} frames {frames:5}  {}", 0x2000_0000 + sector, text.replace('\n', " "));
        sector = p.div_ceil(0x800).max(sector + 1);
    }
}
