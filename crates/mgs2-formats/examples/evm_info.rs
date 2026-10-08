//! Summarises `.evm` files: bones, meshes, vertices, bounds.
//! `cargo run -p mgs2-formats --example evm_info -- file.evm ...`

fn main() {
    for path in std::env::args().skip(1) {
        let data = std::fs::read(&path).expect("read");
        match mgs2_formats::evm::parse(&data) {
            Ok(m) => {
                let verts: usize = m.meshes.iter().map(|x| x.positions.len()).sum();
                let tris: usize = m.meshes.iter().map(|x| x.indices.len() / 3).sum();
                let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                for p in m.meshes.iter().flat_map(|x| &x.positions) {
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
                let wsum = m.meshes.iter().flat_map(|x| &x.skin).map(|s| s.iter().map(|w| w.1).sum::<f32>()).fold((f32::MAX, f32::MIN), |a, s| (a.0.min(s), a.1.max(s)));
                println!(
                    "{path}: animated {} bones {} meshes {} verts {verts} tris {tris} bounds {lo:?}..{hi:?} weight sums {wsum:?}",
                    m.animated,
                    m.bones.len(),
                    m.meshes.len()
                );
            }
            Err(e) => println!("{path}: {e}"),
        }
    }
}
