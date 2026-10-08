//! A GCL interpreter for stage scripts, following the game's VM
//! (`FUN_00153010` blocks, `FUN_00154080` expressions, `FUN_00153628`
//! variable access). Commands that create actors are recorded as [`Spawn`]s
//! for the game to instantiate; commands not implemented yet are skipped.

use std::collections::HashMap;

use mgs2_formats::gcx::{ExprItem, Gcx, Op, Stmt, Value, Var};
use mgs2_formats::str_code;

/// An evaluated argument.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Int(i32),
    Name(u32),
    Proc(u32),
    Str(String),
}

impl Val {
    pub fn int(&self) -> Option<i32> {
        match self {
            Val::Int(i) => Some(*i),
            Val::Name(h) | Val::Proc(h) => Some(*h as i32),
            Val::Str(_) => None,
        }
    }
}

/// `chara <kind> <args...> -x <values...>` with everything evaluated.
#[derive(Clone, Debug)]
pub struct Spawn {
    pub kind: u32,
    pub args: Vec<Val>,
    pub options: Vec<(u8, Vec<Val>)>,
}

impl Spawn {
    pub fn opt(&self, letter: u8) -> Option<&[Val]> {
        self.options.iter().find(|(l, _)| *l == letter).map(|(_, v)| v.as_slice())
    }

    pub fn opt_int(&self, letter: u8, i: usize) -> Option<i32> {
        self.opt(letter)?.get(i)?.int()
    }
}

/// `trap <box> <actor> -m <mode> [-b n] [-d dir range] -e { ... }`: a
/// handler for an `.hzx` trap box (`FUN_0014A448` dispatches to it).
#[derive(Clone, Debug)]
pub struct Trap {
    /// Trap box name.
    pub area: u32,
    /// Name of the actor it reacts to (Snake is 0x28dce6).
    pub actor: u32,
    /// Which events run the block, see [`trap_event`].
    pub mode: u32,
    /// `-b`: only while a button is pressed.
    pub button: bool,
    /// `-d`: only while the actor faces `dir` within `range` (4096 units).
    pub dir: Option<(i32, i32)>,
    pub body: Vec<Stmt>,
}

/// Trap events (the block's `$3`).
pub mod trap_event {
    pub const ENTER: u32 = 0x670D6B;
    pub const STAY: u32 = 0x54A56B;
    pub const LEAVE: u32 = 0x61D56B;
    /// Modes: enter and leave / enter, stay and leave.
    pub const ENTER_LEAVE: u32 = 0x14C9;
    pub const ALL: u32 = 0x1516;
}

/// Actors that call one of their proc options as soon as they are created,
/// as far as stage setup needs: `0xddf5ca` (enemy manager; `-w` creates the
/// "wccomm" actor) and `0x59eda8` "wccomm" (`-e` creates the guards).
const CALL_ON_CREATE: &[(u32, u8)] = &[(0xDDF5CA, b'w'), (0x59EDA8, b'e')];
/// Difficulty (bank B s16): 10 very easy ... 50 extreme.
pub const DIFFICULTY: u16 = 0x8;
/// Story marker (bank A u32) set by the stage that sent Snake here.
pub const PROGRESS: u16 = 0xC4;

/// Snake's position and facing for the next stage (bank A u32), as set by
/// a stage exit before `load` and read by the player actor's `-p`/`-d`.
pub const SNAKE_X: u16 = 0xBC;
pub const SNAKE_Y: u16 = 0x7F4;
pub const SNAKE_Z: u16 = 0xC0;
pub const SNAKE_DIR: u16 = 0x7F8;

/// Actors that run their callback right away when the VM runs a stage
/// exit (`eager`): `delay` timers (`-e` block) and the fade-out `0xe22388`
/// (`-p` proc), so a door's exit proc reaches its `load`.
const EAGER: &[u32] = &[0xE22388];

/// The two variable banks, kept across stages like the game does.
#[derive(Clone)]
pub struct Banks {
    pub a: Vec<u8>,
    pub b: Vec<u8>,
}

impl Default for Banks {
    fn default() -> Self {
        Self { a: vec![0; 0x10000], b: vec![0; 0x10000] }
    }
}

pub struct Vm<'a> {
    procs: HashMap<u32, &'a [Stmt]>,
    /// Bank at 0x191918 (`A` in disassembly) and 0x191910 (`B`).
    bank_a: Vec<u8>,
    bank_b: Vec<u8>,
    pub spawns: Vec<Spawn>,
    pub traps: Vec<Trap>,
    /// Stage named by the first `load` reached.
    pub load: Option<String>,
    /// Run timer/fade callbacks immediately and stop at `load`.
    pub eager: bool,
    last: i32,
    depth: u32,
}

impl Banks {
    pub fn a32(&self, offset: u16) -> i32 {
        let o = offset as usize;
        i32::from_le_bytes(self.a[o..o + 4].try_into().unwrap())
    }

    pub fn set_a32(&mut self, offset: u16, v: i32) {
        let o = offset as usize;
        self.a[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    pub fn set_b16(&mut self, offset: u16, v: i16) {
        let o = offset as usize;
        self.b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
}

impl<'a> Vm<'a> {
    pub fn with_banks(gcx: &'a Gcx, banks: Banks) -> Self {
        Self {
            procs: gcx.procs.iter().map(|p| (p.id, p.body.as_slice())).collect(),
            bank_a: banks.a,
            bank_b: banks.b,
            spawns: Vec::new(),
            traps: Vec::new(),
            load: None,
            eager: false,
            last: 0,
            depth: 0,
        }
    }

    pub fn banks(&self) -> Banks {
        Banks { a: self.bank_a.clone(), b: self.bank_b.clone() }
    }

    /// Runs a proc with integer arguments (an actor's callback).
    pub fn call_proc(&mut self, proc: u32, args: &[i32]) {
        self.call(proc, args);
    }

    /// Runs a trap's block with `[box, actor, event, x, y, z, 0]`.
    pub fn run_block(&mut self, body: &[Stmt], args: &[i32]) {
        self.block(body, args);
    }

    fn hash(&mut self, v: Option<&Value>, args: &[i32]) -> u32 {
        match v.map(|v| self.value(v, args)) {
            Some(Val::Str(s)) => str_code(s.as_bytes()),
            Some(v) => v.int().unwrap_or(0) as u32 & 0x00FF_FFFF,
            None => 0,
        }
    }

    fn halted(&self) -> bool {
        self.eager && self.load.is_some()
    }

    pub fn run(&mut self, stmts: &[Stmt]) {
        self.block(stmts, &[]);
    }

    /// Returns `Some` when a `return` ends the block.
    fn block(&mut self, stmts: &[Stmt], args: &[i32]) -> Option<i32> {
        if self.depth > 64 {
            return None;
        }
        self.depth += 1;
        let mut result = None;
        for s in stmts {
            if self.halted() {
                break;
            }
            if let Some(r) = self.stmt(s, args) {
                result = Some(r);
                break;
            }
        }
        self.depth -= 1;
        result
    }

    fn stmt(&mut self, s: &Stmt, args: &[i32]) -> Option<i32> {
        match s {
            Stmt::Call { proc, args: a } => {
                let vals: Vec<i32> = a.iter().map(|v| self.value(v, args).int().unwrap_or(0)).collect();
                self.call(*proc, &vals);
                None
            }
            Stmt::Command { name, args: a, options } => self.command(*name, a, options, args),
        }
    }

    fn call(&mut self, proc: u32, args: &[i32]) {
        if let Some(body) = self.procs.get(&proc).copied() {
            self.last = self.block(body, args).unwrap_or(0);
        }
    }

    fn command(&mut self, name: u32, a: &[Value], options: &[(u8, Vec<Value>)], args: &[i32]) -> Option<i32> {
        let is = |n: &str| name == str_code(n.as_bytes());
        if is("if") {
            // if <cond> {..} [-i <cond> {..}]* [-e {..}]
            let mut branches = vec![(Some(&a[0]), a.get(1))];
            for (l, v) in options {
                match l {
                    b'i' => branches.push((v.first(), v.get(1))),
                    b'e' => branches.push((None, v.first())),
                    _ => {}
                }
            }
            for (cond, body) in branches {
                let take = cond.is_none_or(|c| self.value(c, args).int().unwrap_or(0) != 0);
                if take {
                    if let Some(Value::Block(b)) = body {
                        return self.block(b, args);
                    }
                    return None;
                }
            }
            None
        } else if is("eval") {
            for v in a {
                self.last = self.value(v, args).int().unwrap_or(0);
            }
            None
        } else if is("return") {
            Some(a.first().map_or(0, |v| self.value(v, args).int().unwrap_or(0)))
        } else if is("chara") {
            let mut vals = a.iter().map(|v| self.value(v, args));
            let kind = match vals.next() {
                Some(Val::Str(s)) => str_code(s.as_bytes()) as i32,
                Some(v) => v.int()?,
                None => return None,
            };
            let spawn = Spawn {
                kind: kind as u32 & 0x00FF_FFFF,
                args: vals.collect(),
                options: options.iter().map(|(l, v)| (*l, v.iter().map(|x| self.value(x, args)).collect())).collect(),
            };
            let on_create = CALL_ON_CREATE
                .iter()
                .find(|(k, _)| *k == spawn.kind)
                .and_then(|(_, l)| spawn.opt(*l)?.first().cloned());
            let eager = (self.eager && EAGER.contains(&spawn.kind))
                .then(|| spawn.opt(b'p').and_then(|v| v.first().cloned()))
                .flatten();
            self.spawns.push(spawn);
            if let Some(Val::Proc(p)) = on_create {
                self.call(p, &[]);
            }
            if let Some(Val::Proc(p)) = eager {
                self.call(p, &[]);
            }
            None
        } else if is("trap") {
            let opt = |l: u8| options.iter().find(|(o, _)| *o == l).map(|(_, v)| v);
            let area = self.hash(a.first(), args);
            let actor = self.hash(a.get(1), args);
            let mode = self.hash(opt(b'm').and_then(|v| v.first()), args);
            let dir = opt(b'd').cloned().map(|v| {
                let d = v.first().map_or(0, |x| self.value(x, args).int().unwrap_or(0));
                let r = v.get(1).map_or(0x400, |x| self.value(x, args).int().unwrap_or(0x400));
                (d, r)
            });
            let body = match opt(b'e').and_then(|v| v.first()) {
                Some(Value::Block(b)) => b.clone(),
                _ => Vec::new(),
            };
            self.traps.push(Trap { area, actor, mode, button: opt(b'b').is_some(), dir, body });
            None
        } else if is("load") {
            if self.load.is_none() {
                self.load = match a.first().map(|v| self.value(v, args)) {
                    Some(Val::Str(s)) => Some(s),
                    Some(Val::Name(h)) => Some(format!("#{h:06x}")),
                    _ => None,
                };
            }
            None
        } else if is("command") {
            // `command <id> <map> -s { ... }`: the setup block runs now.
            if let Some((_, v)) = options.iter().find(|(l, _)| *l == b's')
                && let Some(Value::Block(b)) = v.first() {
                    return self.block(b, args);
                }
            // Other commands' results are not modelled; read as 0.
            self.last = 0;
            None
        } else {
            // mesg, trap, print, load, restart, command, ...: not yet.
            None
        }
    }

    fn value(&mut self, v: &Value, args: &[i32]) -> Val {
        match v {
            Value::Int(_, i) => Val::Int(*i),
            Value::Name(h) => Val::Name(*h),
            Value::ProcRef(h) => Val::Proc(*h),
            Value::Str(s) => Val::Str(s.clone()),
            Value::StrRef(i) => Val::Int(i32::from(*i)),
            Value::Var(var) => Val::Int(self.read(var, args)),
            Value::Arg(0) => Val::Int(self.last),
            Value::Arg(n) => Val::Int(args.get(*n as usize - 1).copied().unwrap_or(0)),
            Value::Expr(items) => Val::Int(self.expr(items, args)),
            Value::Block(b) => Val::Int(self.block(b, args).unwrap_or(self.last)),
        }
    }

    /// (bank B?, byte offset, bit) — bit variables address `index + bit`
    /// bits from the base byte (`FUN_00153628`).
    fn slot(&mut self, var: &Var, args: &[i32]) -> (bool, usize, u8) {
        let index = var.index.as_ref().map_or(0, |i| self.value(i, args).int().unwrap_or(0)).max(0) as usize;
        if var.ty == 4 {
            let total = index + var.bit as usize;
            return (var.bank_b, var.offset as usize + total / 8, (total % 8) as u8);
        }
        let size = match var.ty {
            1 => 2,
            6 | 8 | 9 => 4,
            _ => 1,
        };
        (var.bank_b, var.offset as usize + index * size, 0)
    }

    fn read(&mut self, var: &Var, args: &[i32]) -> i32 {
        let (b, o, bit) = self.slot(var, args);
        let bank = if b { &self.bank_b } else { &self.bank_a };
        let get = |n: usize| bank.get(o..o + n).map(|s| s.to_vec()).unwrap_or(vec![0; n]);
        match var.ty {
            1 => i16::from_le_bytes(get(2).try_into().unwrap()).into(),
            4 => i32::from(bank.get(o).copied().unwrap_or(0) >> bit & 1),
            6 | 9 => i32::from_le_bytes(get(4).try_into().unwrap()),
            8 => i32::from_le_bytes(get(4).try_into().unwrap()) & 0x00FF_FFFF,
            _ => i32::from(get(1)[0]),
        }
    }

    fn write(&mut self, var: &Var, args: &[i32], v: i32) {
        let (b, o, bit) = self.slot(var, args);
        let bank = if b { &mut self.bank_b } else { &mut self.bank_a };
        let mut put = |bytes: &[u8]| {
            if let Some(dst) = bank.get_mut(o..o + bytes.len()) {
                dst.copy_from_slice(bytes);
            }
        };
        match var.ty {
            1 => put(&(v as i16).to_le_bytes()),
            4 => {
                if let Some(byte) = bank.get_mut(o) {
                    let m = 1u8 << bit;
                    *byte = if v != 0 { *byte | m } else { *byte & !m };
                }
            }
            6 | 9 => put(&v.to_le_bytes()),
            8 => put(&(v & 0x00FF_FFFF).to_le_bytes()[..3]),
            _ => put(&[v as u8]),
        }
    }

    fn expr(&mut self, items: &[ExprItem], args: &[i32]) -> i32 {
        // Stack of (value, variable it came from) so `=` can assign.
        let mut stack: Vec<(i32, Option<Var>)> = Vec::new();
        for it in items {
            match it {
                ExprItem::Push(Value::Var(var)) => {
                    let v = self.read(var, args);
                    stack.push((v, Some(var.clone())));
                }
                ExprItem::Push(v) => {
                    let x = self.value(v, args).int().unwrap_or(0);
                    stack.push((x, None));
                }
                ExprItem::Op(op) => {
                    let (b, _) = stack.pop().unwrap_or((0, None));
                    if op.is_unary() {
                        stack.push((unary(*op, b), None));
                        continue;
                    }
                    let (a, avar) = stack.pop().unwrap_or((0, None));
                    if *op == Op::Assign {
                        if let Some(var) = avar {
                            self.write(&var, args, b);
                        }
                        stack.push((b, None));
                    } else {
                        stack.push((binary(*op, a, b), None));
                    }
                }
            }
        }
        stack.last().map_or(0, |s| s.0)
    }
}

fn unary(op: Op, b: i32) -> i32 {
    match op {
        Op::Neg => b.wrapping_neg(),
        Op::Not => i32::from(b == 0),
        Op::BitNot => !b,
        _ => b,
    }
}

/// `FUN_00153F40`.
fn binary(op: Op, a: i32, b: i32) -> i32 {
    match op {
        Op::Add => a.wrapping_add(b),
        Op::Sub => a.wrapping_sub(b),
        Op::Mul => a.wrapping_mul(b),
        Op::Div => a.checked_div(b).unwrap_or(0),
        Op::Mod => a.checked_rem(b).unwrap_or(0),
        Op::Shl => a.wrapping_shl(b as u32 & 31),
        Op::Shr => ((a as u32) >> (b as u32 & 31)) as i32,
        Op::Eq => i32::from(a == b),
        Op::Ne => i32::from(a != b),
        Op::Lt => i32::from(a < b),
        Op::Le => i32::from(a <= b),
        Op::Gt => i32::from(a > b),
        Op::Ge => i32::from(a >= b),
        Op::BitOr => a | b,
        Op::BitAnd => a & b,
        Op::BitXor => a ^ b,
        Op::Or => i32::from(a != 0 || b != 0),
        Op::And => i32::from(a != 0 && b != 0),
        Op::Second => b,
        _ => 0,
    }
}
