use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, Read, Seek, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use mgs2_formats::iso::Iso;
use mgs2_formats::stage_dat::{Stage, StageDat};

mod models;
mod scripts;
mod textures;

#[derive(Parser)]
#[command(about = "Extract stage data from Metal Gear Solid 2 (PS2, SLUS-20144)")]
struct Cli {
    /// The game's .iso or an extracted STAGE.DAT; for `textures`, a .tri file
    /// or a directory searched recursively for them.
    input: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List every stage in STAGE.DAT.
    Stages,
    /// List the pages and files of one stage.
    List { stage: String },
    /// Extract stages to a directory.
    Extract {
        /// Stage names; omit and pass --tanker for the whole Tanker chapter.
        stages: Vec<String>,
        /// Extract every Tanker-chapter stage.
        #[arg(long)]
        tanker: bool,
        #[arg(short, long, default_value = "extracted")]
        out: PathBuf,
    },
    /// Convert .tri texture sets to PNGs (one per texture, named by hash).
    Textures {
        #[arg(short, long, default_value = "extracted-png")]
        out: PathBuf,
    },
    /// Convert .kms models (from `extract` output) to OBJ + MTL.
    Models {
        /// Directory written by the `textures` command.
        #[arg(long, default_value = "extracted-png")]
        png_dir: PathBuf,
        #[arg(short, long, default_value = "extracted-obj")]
        out: PathBuf,
    },
    /// Split CODEC.DAT (from the .iso) into its script records and
    /// disassemble each to text.
    Codec {
        #[arg(short, long, default_value = "extracted-codec")]
        out: PathBuf,
        #[arg(long, default_value = "re/third_party/JoyDict-master/mgs2")]
        dict: PathBuf,
    },
    /// Disassemble .gcx scripts (from `extract` output) to text.
    Scripts {
        #[arg(short, long, default_value = "extracted-gcl")]
        out: PathBuf,
        /// Directory of name lists (e.g. a JoyDict checkout) for hash lookup.
        #[arg(long, default_value = "re/third_party/JoyDict-master/mgs2")]
        dict: PathBuf,
    },
}

trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

fn open_stage_dat(input: &Path) -> Result<StageDat<Box<dyn ReadSeek>>> {
    let is_iso = input.extension().is_some_and(|e| e.eq_ignore_ascii_case("iso"));
    let reader: Box<dyn ReadSeek> = if is_iso {
        Box::new(Iso::open(input)?.open_file("MGS2/STAGE.DAT")?)
    } else {
        Box::new(BufReader::new(File::open(input)?))
    };
    StageDat::open(reader).context("reading STAGE.DAT table of contents")
}

use mgs2_formats::stage_dat::is_tanker_stage as is_tanker;

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.cmd {
        Cmd::Textures { out } => return textures::convert(&cli.input, out),
        Cmd::Models { png_dir, out } => return models::convert(&cli.input, png_dir, out),
        Cmd::Scripts { out, dict } => return scripts::convert(&cli.input, out, Some(dict)),
        Cmd::Codec { out, dict } => return scripts::codec(&cli.input, out, dict),
        _ => {}
    }
    let mut dat = open_stage_dat(&cli.input)?;
    match cli.cmd {
        Cmd::Textures { .. } | Cmd::Models { .. } | Cmd::Scripts { .. } | Cmd::Codec { .. } => unreachable!(),
        Cmd::Stages => {
            for s in dat.stages() {
                let tag = if is_tanker(&s.name) { "  [tanker]" } else { "" };
                println!("{:<8} sector {:#07x}  {:>7} KiB{tag}", s.name, s.sector, s.sectors * 2);
            }
        }
        Cmd::List { stage } => {
            let st = dat.read_stage(&stage)?;
            print_stage(&st);
        }
        Cmd::Extract { mut stages, tanker, out } => {
            if tanker {
                stages.extend(dat.stages().iter().filter(|s| is_tanker(&s.name)).map(|s| s.name.clone()));
            }
            if stages.is_empty() {
                bail!("no stages given (name some, or pass --tanker)");
            }
            for name in &stages {
                let st = dat.read_stage(name).with_context(|| format!("stage {name}"))?;
                let n = extract_stage(&st, &out)?;
                println!("{name}: {n} files");
            }
        }
    }
    Ok(())
}

fn print_stage(st: &Stage) {
    println!("stage {} ({} pages)", st.name, st.pages.len());
    for (i, p) in st.pages.iter().enumerate() {
        println!(
            "page {i}: kind {:#010x} at {:#x}, {} bytes{}, {} files",
            p.kind,
            p.offset,
            p.data.len(),
            if p.compressed { " (zlib)" } else { "" },
            p.files.len()
        );
        let mut by_ext: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for f in &p.files {
            let e = by_ext.entry(f.extension()).or_default();
            e.0 += 1;
            e.1 += f.size as usize;
        }
        for (ext, (count, bytes)) in by_ext {
            println!("    .{ext:<4} {count:>4} files {bytes:>10} bytes");
        }
    }
}

/// Writes `<out>/<stage>/p<N>/<hash>.<ext>` plus a `files.tsv` index.
fn extract_stage(st: &Stage, out: &Path) -> Result<usize> {
    let dir = out.join(&st.name);
    fs::create_dir_all(&dir)?;
    let mut index = File::create(dir.join("files.tsv"))?;
    writeln!(index, "page\tpage_kind\tid\ttype\text\toffset\tsize\tpath")?;
    let mut n = 0;
    for (pi, page) in st.pages.iter().enumerate() {
        let pdir = dir.join(format!("p{pi:02}"));
        fs::create_dir_all(&pdir)?;
        for f in &page.files {
            let file_name = format!("{:06x}.{}", f.name_hash(), f.extension());
            fs::write(pdir.join(&file_name), page.file_data(f))?;
            writeln!(
                index,
                "{pi}\t{:#010x}\t{:#010x}\t{}\t{}\t{:#x}\t{}\tp{pi:02}/{file_name}",
                page.kind,
                f.id,
                f.type_id(),
                f.extension(),
                f.offset,
                f.size
            )?;
            n += 1;
        }
    }
    Ok(n)
}
