//! Prints the raw RPN of every expression in a stage script that reads a
//! given variable (bank A offset), to check how conditions are built.
//! `cargo run -p mgs2-formats --example gcx_rpn -- <iso> <stage> <offset hex>`

use mgs2_formats::gcx::{self, ExprItem, Stmt, Value};

fn walk(stmts: &[Stmt], want: u16) {
    for s in stmts {
        if let Stmt::Command { args, options, .. } = s {
            for v in args.iter().chain(options.iter().flat_map(|(_, v)| v)) {
                val(v, want);
            }
        }
    }
}

fn val(v: &Value, want: u16) {
    match v {
        Value::Expr(items) => {
            if items.iter().any(|i| matches!(i, ExprItem::Push(Value::Var(var)) if var.offset == want)) {
                println!("{items:?}");
            }
            for i in items {
                if let ExprItem::Push(v) = i {
                    val(v, want);
                }
            }
        }
        Value::Block(b) => walk(b, want),
        _ => {}
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let iso = args.next().expect("usage: gcx_rpn <iso> <stage> <offset hex>");
    let stage = args.next().expect("stage");
    let want = u16::from_str_radix(args.next().expect("offset").trim_start_matches("0x"), 16).expect("hex");
    let f = mgs2_formats::iso::Iso::open(std::path::Path::new(&iso)).expect("iso").open_file("MGS2/STAGE.DAT").expect("STAGE.DAT");
    let mut dat = mgs2_formats::stage_dat::StageDat::open(f).expect("stage.dat");
    let st = dat.read_stage(&stage).expect("stage");
    let (page, fe) = st.files().find(|(_, f)| f.type_id() == 6).expect("script");
    let g = gcx::parse(page.file_data(fe)).expect("gcx");
    walk(&g.main, want);
    for p in &g.procs {
        walk(&p.body, want);
    }
}
