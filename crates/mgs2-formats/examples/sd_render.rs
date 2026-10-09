//! Renders a stage's songs (sound driver music) to WAV files.
//! `cargo run --release -p mgs2-formats --example sd_render -- <iso> <stage> <out dir> [seconds] [level]`

fn main() {
    let mut args = std::env::args().skip(1);
    let iso = args.next().expect("usage: sd_render <iso> <stage> <out dir> [seconds] [level]");
    let stage = args.next().expect("stage");
    let out = std::path::PathBuf::from(args.next().expect("out dir"));
    let secs: f32 = args.next().map_or(60.0, |s| s.parse().expect("seconds"));
    let level: u8 = args.next().map_or(0, |s| s.parse().expect("level"));
    std::fs::create_dir_all(&out).unwrap();
    let f = mgs2_formats::iso::Iso::open(std::path::Path::new(&iso)).expect("iso").open_file("MGS2/STAGE.DAT").expect("STAGE.DAT");
    let mut dat = mgs2_formats::stage_dat::StageDat::open(f).expect("stage.dat");
    let st = dat.read_stage(&stage).expect("stage");
    let Some(page) = st.pages.iter().find(|p| p.kind == 0x7F00_0010) else {
        println!("{stage}: no sound page");
        return;
    };
    let pack = std::sync::Arc::new(mgs2_formats::sd::Pack::parse(&page.data).expect("pack"));
    println!("{stage}: {} songs", pack.song_count());
    for n in 1..=pack.song_count() {
        let pcm = pack.render(n, secs, level);
        let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        let path = out.join(format!("{stage}_song{n}_l{level}.wav"));
        std::fs::write(&path, mgs2_formats::sd::wav(&pcm)).unwrap();
        println!("{} {:.1} s peak {peak}", path.display(), pcm.len() as f32 / 2.0 / mgs2_formats::sd::RATE as f32);
    }
}
