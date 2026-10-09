//! Lists a stage's sound data pages (`0x7F000010`) and their files.
//! `cargo run -p mgs2-formats --example sound_pages -- <iso> <stage> [out dir]`
//! (with an out dir, writes each file as `<hash>.bin`).

fn main() {
    let mut args = std::env::args().skip(1);
    let iso = args.next().expect("usage: sound_pages <iso> <stage> [out dir]");
    let stage = args.next().expect("stage");
    let out = args.next().map(std::path::PathBuf::from);
    let f = mgs2_formats::iso::Iso::open(std::path::Path::new(&iso)).expect("iso").open_file("MGS2/STAGE.DAT").expect("STAGE.DAT");
    let mut dat = mgs2_formats::stage_dat::StageDat::open(f).expect("stage.dat");
    let st = dat.read_stage(&stage).expect("stage");
    for p in &st.pages {
        println!("page {:#010x} at {:#x} compressed {} size {:#x} files {}", p.kind, p.offset, p.compressed, p.data.len(), p.files.len());
        if p.kind != 0x7F00_0010 {
            continue;
        }
        for fe in &p.files {
            let d = p.file_data(fe);
            let head: Vec<String> = d.iter().take(32).map(|b| format!("{b:02x}")).collect();
            println!("  {:#010x} +{:#x} size {:#x}  {}", fe.id, fe.offset, fe.size, head.join(" "));
            if let Some(o) = &out {
                std::fs::create_dir_all(o).unwrap();
                std::fs::write(o.join(format!("{stage}_{:08x}.bin", fe.id)), d).unwrap();
            }
        }
    }
}
