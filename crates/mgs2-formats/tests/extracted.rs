//! Parses every extracted file of the supported types. Needs game data, so it
//! is skipped unless `extracted/` (from `mgs2-extract extract`) exists; set
//! `MGS2_EXTRACTED` to use another directory.

use std::path::{Path, PathBuf};

use mgs2_formats::{gcx, hzx, kms, mar, tri};

fn data_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("MGS2_EXTRACTED")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extracted"));
    dir.is_dir().then_some(dir)
}

fn files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == ext) {
                out.push(p);
            }
        }
    }
    out
}

fn check<T>(ext: &str, parse: impl Fn(&[u8]) -> mgs2_formats::Result<T>) {
    let Some(dir) = data_dir() else {
        eprintln!("no extracted data; skipping .{ext}");
        return;
    };
    let all = files(&dir, ext);
    let failed: Vec<_> = all
        .iter()
        .filter_map(|p| parse(&std::fs::read(p).unwrap()).err().map(|e| format!("{}: {e}", p.display())))
        .collect();
    assert!(failed.is_empty(), "{} of {} .{ext} failed:\n{}", failed.len(), all.len(), failed.join("\n"));
}

#[test]
fn all_textures_parse() {
    check("tri", tri::parse);
}

#[test]
fn all_models_parse() {
    check("kms", kms::parse);
}

#[test]
fn all_collision_parses() {
    check("hzx", |d| {
        let h = hzx::parse(d)?;
        let floors: usize = h.areas.iter().flat_map(|a| &a.groups).map(|g| g.floors.len()).sum();
        assert!(floors > 0, "collision with no floors");
        Ok(h)
    });
}

#[test]
fn all_scripts_parse() {
    check("gcx", gcx::parse);
}

#[test]
fn all_motions_parse() {
    check("mar", |d| {
        if d.get(..4) != Some(b"MARa".as_slice()) {
            return Ok(None); // .mdl physics models share the letter
        }
        let m = mar::parse(d)?;
        for (i, mo) in m.motions.iter().enumerate() {
            for (j, k) in mo.joints.iter().enumerate().flat_map(|(j, ks)| ks.iter().map(move |k| (j, k))) {
                for q in [k.a, k.b] {
                    let n: f32 = q.iter().map(|c| c * c).sum::<f32>().sqrt();
                    assert!((n - 1.0).abs() < 0.02, "motion {i} joint {j} frame {}: |q| = {n}", k.frame);
                }
            }
        }
        Ok(Some(m))
    });
}
