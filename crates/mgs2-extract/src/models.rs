use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use mgs2_formats::kms;

/// Converts the .kms models of each stage under `input` (an `extract`
/// output directory, or one stage inside it) to Wavefront OBJ files. Textures
/// are looked up in `png_dir`, the output of the `textures` command.
pub fn convert(input: &Path, png_dir: &Path, out: &Path) -> Result<()> {
    let stages: Vec<PathBuf> = if input.join("files.tsv").exists() {
        vec![input.to_path_buf()]
    } else {
        fs::read_dir(input)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.join("files.tsv").exists()).collect()
    };
    let (mut ok, mut failed) = (0, 0);
    for stage in stages {
        let name = stage.file_name().unwrap().to_string_lossy().to_string();
        let textures = index_textures(&png_dir.join(&name));
        let dir = out.join(&name);
        fs::create_dir_all(&dir)?;
        for kms_path in find(&stage, "kms") {
            let stem = kms_path.file_stem().unwrap().to_string_lossy().to_string();
            match fs::read(&kms_path).map_err(anyhow::Error::from).and_then(|d| Ok(kms::parse(&d)?)) {
                Ok(model) => {
                    write_obj(&model, &stem, &dir, &textures)?;
                    ok += 1;
                }
                Err(e) => {
                    failed += 1;
                    eprintln!("{}: {e:#}", kms_path.display());
                }
            }
        }
    }
    println!("{ok} models converted, {failed} failed");
    Ok(())
}

fn find(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == ext) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Texture hash -> PNG path, for every texture set converted for a stage.
fn index_textures(dir: &Path) -> HashMap<u32, PathBuf> {
    find(dir, "png")
        .into_iter()
        .filter_map(|p| {
            let h = u32::from_str_radix(p.file_stem()?.to_str()?, 16).ok()?;
            Some((h, std::path::absolute(&p).unwrap_or(p)))
        })
        .collect()
}

fn write_obj(model: &kms::Model, stem: &str, dir: &Path, textures: &HashMap<u32, PathBuf>) -> Result<()> {
    let (mut obj, mut mtl) = (String::new(), String::new());
    writeln!(obj, "# MGS2 kms {stem}, texture set {:06x}\nmtllib {stem}.mtl", model.texture_set)?;
    let mut base = 1u32;
    let mut materials = Vec::new();
    let offsets = model.bind_offsets();
    for (oi, o) in model.objects.iter().enumerate() {
        writeln!(obj, "o obj{oi}_parent{}", o.parent)?;
        let d = offsets[oi];
        for (mi, m) in o.meshes.iter().enumerate() {
            for p in &m.positions {
                writeln!(obj, "v {} {} {}", p[0] + d[0], p[1] + d[1], p[2] + d[2])?;
            }
            for uv in &m.uvs {
                writeln!(obj, "vt {} {}", uv[0], 1.0 - uv[1])?;
            }
            for n in &m.normals {
                writeln!(obj, "vn {} {} {}", n[0], n[1], n[2])?;
            }
            let tex = m.textures[0];
            if !materials.contains(&tex) {
                materials.push(tex);
            }
            writeln!(obj, "g obj{oi}_mesh{mi}\nusemtl t{tex:06x}")?;
            let (has_uv, has_n) = (!m.uvs.is_empty(), !m.normals.is_empty());
            for t in m.indices.chunks_exact(3) {
                obj.push('f');
                for &i in t {
                    let v = base + i;
                    match (has_uv, has_n) {
                        (true, true) => write!(obj, " {v}/{v}/{v}")?,
                        (true, false) => write!(obj, " {v}/{v}")?,
                        (false, true) => write!(obj, " {v}//{v}")?,
                        (false, false) => write!(obj, " {v}")?,
                    }
                }
                obj.push('\n');
            }
            // OBJ shares one index space per attribute, so pad missing ones.
            let n = m.positions.len();
            if !has_uv {
                (0..n).for_each(|_| obj.push_str("vt 0 0\n"));
            }
            if !has_n {
                (0..n).for_each(|_| obj.push_str("vn 0 1 0\n"));
            }
            base += n as u32;
        }
    }
    for tex in materials {
        writeln!(mtl, "newmtl t{tex:06x}\nKd 1 1 1")?;
        if let Some(p) = textures.get(&tex) {
            writeln!(mtl, "map_Kd {}\nmap_d {}", p.display(), p.display())?;
        }
    }
    fs::write(dir.join(format!("{stem}.obj")), obj)?;
    fs::write(dir.join(format!("{stem}.mtl")), mtl)?;
    Ok(())
}
