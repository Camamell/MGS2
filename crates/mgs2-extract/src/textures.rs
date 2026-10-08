use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use mgs2_formats::tri;

/// Converts one .tri, or every .tri under a directory, to PNGs in `out`,
/// mirroring the input's directory structure.
pub fn convert(input: &Path, out: &Path) -> Result<()> {
    let mut files = Vec::new();
    if input.is_dir() {
        collect(input, &mut files)?;
    } else {
        files.push(input.to_path_buf());
    }
    let root = if input.is_dir() { input } else { input.parent().unwrap_or(Path::new(".")) };
    let (mut ok, mut failed, mut count) = (0, 0, 0);
    for path in &files {
        let rel = path.strip_prefix(root).unwrap_or(path).with_extension("");
        let dir = out.join(rel);
        match convert_one(path, &dir) {
            Ok(n) => {
                ok += 1;
                count += n;
            }
            Err(e) => {
                failed += 1;
                eprintln!("{}: {e:#}", path.display());
            }
        }
    }
    println!("{ok} texture sets ({count} textures) converted, {failed} failed");
    Ok(())
}

fn convert_one(path: &Path, dir: &Path) -> Result<usize> {
    let data = fs::read(path)?;
    let textures = tri::parse(&data)?;
    fs::create_dir_all(dir)?;
    for t in &textures {
        let file = dir.join(format!("{:06x}.png", t.hash));
        let w = BufWriter::new(File::create(&file)?);
        let mut enc = png::Encoder::new(w, t.width, t.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(&t.rgba).with_context(|| file.display().to_string())?;
    }
    Ok(textures.len())
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let p = entry?.path();
        if p.is_dir() {
            collect(&p, out)?;
        } else if p.extension().is_some_and(|e| e == "tri") {
            out.push(p);
        }
    }
    Ok(())
}
