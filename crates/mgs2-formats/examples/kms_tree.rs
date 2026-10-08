//! Prints a `.kms` model's object hierarchy: index, parent, bind offset (mm)
//! and the accumulated model-space position.
//!
//! `cargo run -p mgs2-formats --example kms_tree -- extracted/r_tnk0/p01/413aa8.kms`

fn main() {
    let path = std::env::args().nth(1).expect("usage: kms_tree <file.kms>");
    let data = std::fs::read(&path).expect("read");
    let model = mgs2_formats::kms::parse(&data).expect("parse");
    let mut world: Vec<[f32; 3]> = Vec::new();
    println!("origin {:?}, {} objects", model.origin, model.objects.len());
    for (i, o) in model.objects.iter().enumerate() {
        let base = usize::try_from(o.parent).ok().and_then(|p| world.get(p).copied()).unwrap_or([0.0; 3]);
        let w = [base[0] + o.offset[0], base[1] + o.offset[1], base[2] + o.offset[2]];
        world.push(w);
        println!(
            "{i:2} parent {:3} offset {:7.1} {:7.1} {:7.1}  at {:7.1} {:7.1} {:7.1}  meshes {}",
            o.parent, o.offset[0], o.offset[1], o.offset[2], w[0], w[1], w[2], o.meshes.len()
        );
    }
}
