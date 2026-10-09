//! Renders a stage's songs (sound driver music), or sound effects, to WAV
//! files, with the Tanker's resident pack (`r_tnk0`) loaded first.
//! `cargo run --release -p mgs2-formats --example sd_render -- <iso> <stage> <out dir> [seconds] [level]`
//! `cargo run --release -p mgs2-formats --example sd_render -- <iso> <stage> <out dir> fx <id>...`
//! (effect ids in hex, played at full volume, centred).

use std::sync::Arc;

use mgs2_formats::sd::{self, Pack};

fn sound_page(dat: &mut mgs2_formats::stage_dat::StageDat<impl std::io::Read + std::io::Seek>, stage: &str) -> Option<Vec<u8>> {
    let st = dat.read_stage(stage).ok()?;
    st.pages.into_iter().find(|p| p.kind == 0x7F00_0010).map(|p| p.data)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let iso = args.next().expect("usage: sd_render <iso> <stage> <out dir> [seconds] [level] | fx <id>...");
    let stage = args.next().expect("stage");
    let out = std::path::PathBuf::from(args.next().expect("out dir"));
    let rest: Vec<String> = args.collect();
    std::fs::create_dir_all(&out).unwrap();
    let f = mgs2_formats::iso::Iso::open(std::path::Path::new(&iso)).expect("iso").open_file("MGS2/STAGE.DAT").expect("STAGE.DAT");
    let mut dat = mgs2_formats::stage_dat::StageDat::open(f).expect("stage.dat");
    let resident = sound_page(&mut dat, "r_tnk0");
    let Some(page) = sound_page(&mut dat, &stage) else {
        println!("{stage}: no sound page");
        return;
    };
    let pack = Arc::new(Pack::with_resident(resident.as_deref(), &page).expect("pack"));
    let save = |name: String, pcm: Vec<i16>| {
        let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        let path = out.join(name);
        std::fs::write(&path, sd::wav(&pcm)).unwrap();
        println!("{} {:.2} s peak {peak}", path.display(), pcm.len() as f32 / 2.0 / sd::RATE as f32);
    };
    if rest.first().map(String::as_str) == Some("fx") {
        for id in &rest[1..] {
            let id = u32::from_str_radix(id.trim_start_matches("0x"), 16).expect("hex id");
            save(format!("{stage}_fx{id:03x}.wav"), pack.render_effect(0x20 << 18 | 0x3F << 12 | id, 6.0));
        }
        return;
    }
    let secs: f32 = rest.first().map_or(60.0, |s| s.parse().expect("seconds"));
    let level: u8 = rest.get(1).map_or(0, |s| s.parse().expect("level"));
    println!("{stage}: {} songs", pack.song_count());
    for n in 1..=pack.song_count() {
        save(format!("{stage}_song{n}_l{level}.wav"), pack.render(n, secs, level));
    }
}
