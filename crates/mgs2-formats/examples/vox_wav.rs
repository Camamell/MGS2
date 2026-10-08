//! Writes VOX.DAT clips as WAV files (to check the playback rate by ear).
//! `cargo run -p mgs2-formats --example vox_wav -- <iso> <out dir> <id> ...`
//! (ids as `0x2001e476`).

use std::io::{Read, Seek, SeekFrom};

fn main() {
    let mut args = std::env::args().skip(1);
    let iso = args.next().expect("usage: vox_wav <iso> <out dir> <id>...");
    let out = std::path::PathBuf::from(args.next().expect("out dir"));
    std::fs::create_dir_all(&out).expect("out dir");
    let iso = mgs2_formats::iso::Iso::open(std::path::Path::new(&iso)).expect("iso");
    let mut f = iso.open_file("MGS2/VOX.DAT").expect("VOX.DAT");
    for id in args {
        let id = u32::from_str_radix(id.trim_start_matches("0x"), 16).expect("hex id");
        let mut pos = u64::from(id.wrapping_sub(0x2000_0000)) * 0x800;
        let mut bytes = Vec::new();
        let mut head = [0u8; 8];
        for n in 0..4096 {
            f.seek(SeekFrom::Start(pos)).unwrap();
            f.read_exact(&mut head).unwrap();
            let kind = u32::from_le_bytes(head[..4].try_into().unwrap());
            let size = u32::from_le_bytes(head[4..].try_into().unwrap()) as u64;
            if !(16..=0x10_0000).contains(&size) || (kind == 0x10 && n > 2) {
                break;
            }
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
                println!("{} ({} ch, {:.1} s)", path.display(), c.channels, c.samples.len() as f32 / f32::from(c.channels) / c.rate as f32);
            }
            None => println!("{id:#x}: no audio"),
        }
    }
}
