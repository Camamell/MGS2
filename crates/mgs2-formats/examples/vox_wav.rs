//! Writes VOX.DAT clips and DEMO.DAT cutscene audio as WAV files.
//! `cargo run -p mgs2-formats --example vox_wav -- <iso> <out dir> <id> ...`
//! (ids as `0x2001e476` for VOX.DAT, `0x40006188` for DEMO.DAT).

use std::io::{Read, Seek, SeekFrom};

fn main() {
    let mut args = std::env::args().skip(1);
    let iso = args.next().expect("usage: vox_wav <iso> <out dir> <id>...");
    let out = std::path::PathBuf::from(args.next().expect("out dir"));
    std::fs::create_dir_all(&out).expect("out dir");
    let open = || mgs2_formats::iso::Iso::open(std::path::Path::new(&iso)).expect("iso");
    let mut vox = open().open_file("MGS2/VOX.DAT").expect("VOX.DAT");
    let mut demo = open().open_file("MGS2/DEMO.DAT").expect("DEMO.DAT");
    for id in args {
        let id = u32::from_str_radix(id.trim_start_matches("0x"), 16).expect("hex id");
        let f = if id >> 28 == 4 { &mut demo } else { &mut vox };
        let mut pos = u64::from(id & 0x0FFF_FFFF) * 0x800;
        let mut bytes = Vec::new();
        let mut head = [0u8; 8];
        let mut seen = false;
        for _ in 0..50_000 {
            f.seek(SeekFrom::Start(pos)).unwrap();
            f.read_exact(&mut head).unwrap();
            let kind = u32::from_le_bytes(head[..4].try_into().unwrap());
            let size = u32::from_le_bytes(head[4..].try_into().unwrap()) as u64;
            if !(16..=0x10_0000).contains(&size) || (kind == 0x10 && seen) {
                break;
            }
            seen |= kind != 0x10;
            let mut chunk = vec![0u8; size as usize];
            f.seek(SeekFrom::Start(pos)).unwrap();
            f.read_exact(&mut chunk).unwrap();
            bytes.extend_from_slice(&chunk);
            pos += size;
            if kind == 0xF0 {
                break;
            }
        }
        match mgs2_formats::vox::decode_clip(&bytes) {
            Some(c) => {
                let path = out.join(format!("{id:08x}.wav"));
                std::fs::write(&path, c.wav()).unwrap();
                println!("{} ({} ch, {:.1} s, end chunk {:.1} s)", path.display(), c.channels, c.samples.len() as f32 / f32::from(c.channels) / c.rate as f32, c.frames as f32 / 300.0);
            }
            None => println!("{id:#x}: no audio"),
        }
    }
}
