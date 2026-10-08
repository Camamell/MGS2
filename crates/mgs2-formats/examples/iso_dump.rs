//! Copies a byte range of a file inside the game's ISO to a local file.
//!
//! `cargo run -p mgs2-formats --example iso_dump -- <iso> MGS2/CODEC.DAT <offset> <length> <out>`

use std::io::{Read, Seek, SeekFrom};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let [_, iso, name, offset, len, out] = a.as_slice() else {
        eprintln!("usage: iso_dump <iso> <path in iso> <offset> <length> <out>");
        std::process::exit(2);
    };
    let iso = mgs2_formats::iso::Iso::open(std::path::Path::new(iso)).expect("open iso");
    let mut f = iso.open_file(name).expect("file in iso");
    f.seek(SeekFrom::Start(offset.parse().unwrap())).unwrap();
    let mut buf = vec![0; len.parse().unwrap()];
    let n = f.read(&mut buf).unwrap();
    std::fs::write(out, &buf[..n]).unwrap();
    println!("{n} bytes");
}
