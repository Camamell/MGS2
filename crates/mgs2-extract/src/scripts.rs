use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use mgs2_formats::gcx::{self, ExprItem, Gcx, Stmt, Value};
use mgs2_formats::names::{self, Names};

/// Disassembles a .gcx (or every .gcx under a stage directory tree) to
/// readable text. Hash names are resolved from built-in words plus identifiers
/// found in the same stage's files.
pub fn convert(input: &Path, out: &Path, dict: Option<&Path>) -> Result<()> {
    let files = find(input, "gcx");
    let mut base = Names::with_builtin();
    if let Some(d) = dict.filter(|d| d.is_dir()) {
        let n = base.load_dir(d)?;
        println!("loaded {n} dictionary names from {}", d.display());
    }
    for path in &files {
        let stage_dir = path.ancestors().nth(2).unwrap_or(Path::new("."));
        let mut words = Vec::new();
        for f in find(stage_dir, "bin").iter().chain(find(stage_dir, "gcx").iter()) {
            names::harvest(&fs::read(f)?, &mut words);
        }
        let mut names = base.clone();
        names.extend(&words);

        let g = gcx::parse(&fs::read(path)?).with_context(|| path.display().to_string())?;
        let text = render(&g, &names);
        let stage = stage_dir.file_name().map_or("script".into(), |s| s.to_string_lossy().into_owned());
        fs::create_dir_all(out)?;
        let dst = out.join(format!("{stage}.gcl.txt"));
        fs::write(&dst, &text)?;
        let (known, total) = coverage(&g, &names);
        println!("{}: {} procs, {} strings, names resolved {known}/{total}", dst.display(), g.procs.len(), g.strings.len());
    }
    Ok(())
}

/// CODEC.DAT is a run of GCL script records, each starting with the same
/// 32-bit stamp (`0x3bb29171`, in the timestamp slot of a `.gcx`). Writes
/// `NNN.gcl.txt` per record and `index.txt` (offset, size, first line).
pub fn codec(iso: &Path, out: &Path, dict: &Path) -> Result<()> {
    use std::io::Read as _;
    let mut data = Vec::new();
    mgs2_formats::iso::Iso::open(iso)?.open_file("MGS2/CODEC.DAT")?.read_to_end(&mut data)?;
    let stamp = &data[..4];
    let mut starts: Vec<usize> = (0..data.len().saturating_sub(4)).step_by(16).filter(|&i| &data[i..i + 4] == stamp).collect();
    starts.push(data.len());
    let mut names = Names::with_builtin();
    if dict.is_dir() {
        names.load_dir(dict)?;
    }
    fs::create_dir_all(out)?;
    let mut index = String::new();
    for (n, w) in starts.windows(2).enumerate() {
        let rec = &data[w[0]..w[1]];
        let line = match gcx::parse(rec) {
            Ok(g) => {
                fs::write(out.join(format!("{n:03}.gcl.txt")), render(&g, &names))?;
                g.strings.iter().find(|s| s.trim().len() > 3).map_or(String::new(), |s| s.replace('\n', " "))
            }
            Err(e) => format!("(not parsed: {e})"),
        };
        writeln!(index, "{n:03} {:#09x} {:7} {}", w[0], rec.len(), line.chars().take(90).collect::<String>())?;
    }
    fs::write(out.join("index.txt"), &index)?;
    println!("{} records -> {}", starts.len() - 1, out.display());
    Ok(())
}

fn find(dir: &Path, ext: &str) -> Vec<PathBuf> {
    if dir.is_file() {
        return vec![dir.to_path_buf()];
    }
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

fn coverage(g: &Gcx, n: &Names) -> (usize, usize) {
    let mut hashes = Vec::new();
    fn walk(stmts: &[Stmt], out: &mut Vec<u32>) {
        for s in stmts {
            match s {
                Stmt::Command { name, args, options } => {
                    out.push(*name);
                    args.iter().chain(options.iter().flat_map(|o| &o.1)).for_each(|v| val(v, out));
                }
                Stmt::Call { proc, args } => {
                    out.push(*proc);
                    args.iter().for_each(|v| val(v, out));
                }
            }
        }
    }
    fn val(v: &Value, out: &mut Vec<u32>) {
        match v {
            Value::Name(h) | Value::ProcRef(h) => out.push(*h),
            Value::Block(b) => walk(b, out),
            Value::Expr(e) => e.iter().for_each(|i| if let ExprItem::Push(v) = i { val(v, out) }),
            _ => {}
        }
    }
    walk(&g.main, &mut hashes);
    g.procs.iter().for_each(|p| walk(&p.body, &mut hashes));
    hashes.sort();
    hashes.dedup();
    (hashes.iter().filter(|h| n.get(**h).is_some()).count(), hashes.len())
}

pub fn render(g: &Gcx, n: &Names) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "// timestamp {:#x}", g.timestamp);
    for (i, st) in g.strings.iter().enumerate() {
        let _ = writeln!(s, "// str[{i}] = {st:?}");
    }
    let _ = writeln!(s, "\nmain {{");
    block(&mut s, &g.main, n, 1);
    s.push_str("}\n");
    for p in &g.procs {
        let _ = writeln!(s, "\nproc {} {{", n.show(p.id));
        block(&mut s, &p.body, n, 1);
        s.push_str("}\n");
    }
    s
}

fn block(s: &mut String, stmts: &[Stmt], n: &Names, depth: usize) {
    let ind = "    ".repeat(depth);
    for st in stmts {
        s.push_str(&ind);
        match st {
            Stmt::Command { name, args, options } => {
                s.push_str(&n.show(*name));
                for a in args {
                    s.push(' ');
                    value(s, a, n, depth);
                }
                for (letter, vals) in options {
                    let _ = write!(s, " -{}", *letter as char);
                    for v in vals {
                        s.push(' ');
                        value(s, v, n, depth);
                    }
                }
            }
            Stmt::Call { proc, args } => {
                let _ = write!(s, "call {}", n.show(*proc));
                for a in args {
                    s.push(' ');
                    value(s, a, n, depth);
                }
            }
        }
        s.push('\n');
    }
}

fn value(s: &mut String, v: &Value, n: &Names, depth: usize) {
    match v {
        Value::Int(_, i) => {
            let _ = write!(s, "{i}");
        }
        Value::Name(h) => {
            let _ = write!(s, "\"{}\"", n.show(*h));
        }
        Value::ProcRef(h) => {
            let _ = write!(s, "proc:{}", n.show(*h));
        }
        Value::Str(t) => {
            let _ = write!(s, "{t:?}");
        }
        Value::StrRef(i) => {
            let _ = write!(s, "str[{i}]");
        }
        Value::Var(v) => {
            let _ = write!(s, "{}{}:{:#x}", if v.bank_b { "B" } else { "A" }, var_ty(v.ty), v.offset);
            if v.ty == 4 {
                let _ = write!(s, ".{}", v.bit);
            }
            if let Some(i) = &v.index {
                s.push('[');
                value(s, i, n, depth);
                s.push(']');
            }
        }
        Value::Arg(i) => {
            let _ = write!(s, "${i}");
        }
        Value::Expr(items) => {
            s.push('(');
            s.push_str(&infix(items, n));
            s.push(')');
        }
        Value::Block(b) => {
            s.push_str("{\n");
            block(s, b, n, depth + 1);
            s.push_str(&"    ".repeat(depth));
            s.push('}');
        }
    }
}

fn var_ty(t: u8) -> &'static str {
    match t {
        1 => "s16",
        2 | 3 => "u8",
        4 => "bit",
        6 | 9 => "u32",
        8 => "u24",
        _ => "?",
    }
}

/// Converts the RPN expression to infix text.
fn infix(items: &[ExprItem], n: &Names) -> String {
    let mut stack: Vec<String> = Vec::new();
    for it in items {
        match it {
            ExprItem::Push(v) => {
                let mut t = String::new();
                value(&mut t, v, n, 0);
                stack.push(t);
            }
            ExprItem::Op(op) => {
                let b = stack.pop().unwrap_or_default();
                if op.is_unary() {
                    stack.push(format!("{}{b}", op.symbol()));
                } else {
                    let a = stack.pop().unwrap_or_default();
                    stack.push(format!("{a} {} {b}", op.symbol()));
                }
            }
        }
    }
    stack.join(", ")
}
