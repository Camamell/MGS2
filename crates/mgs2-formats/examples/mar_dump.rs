//! Lists a motion archive's motions: length, flags and root travel.
//! `cargo run -p mgs2-formats --example mar_dump -- file.mar`

fn main() {
    let path = std::env::args().nth(1).expect("usage: mar_dump <file.mar>");
    let data = std::fs::read(path).expect("read");
    let mar = mgs2_formats::mar::parse(&data).expect("parse");
    for (i, m) in mar.motions.iter().enumerate() {
        let (first, last) = (m.root.first(), m.root.last());
        let travel = match (first, last) {
            (Some(a), Some(b)) => {
                let d: Vec<f32> = (0..3).map(|k| b.position[k] - a.position[k]).collect();
                format!("root d=({:7.1} {:7.1} {:7.1})", d[0], d[1], d[2])
            }
            _ => "no root".into(),
        };
        println!("{i:3} id {:#08x} flags {:#06x} len {:4} {travel}", m.id, m.flags, m.length);
    }
}
