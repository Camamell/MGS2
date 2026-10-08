//! Cutscenes (DEMO.DAT). Not played yet beyond their subtitles.
//!
//! A cutscene stage's script starts demos with `chara "demo" <name> -i
//! 0x40000000+n`: n is the demo's first sector (2048 bytes) in DEMO.DAT.
//! A demo is a chain of chunks `u32 kind, u32 size, u32 frame, u32 0, ...`:
//! three kind-0x10 header chunks, then kind 2 (event packets every few
//! frames: cameras and actors, not decoded), kind 1 (16 KiB streamed
//! audio) and kind 4 (subtitles). A subtitle chunk's payload is `u32 size`
//! then records `u32 start, u32 end, u32 0, u32 length (with this header),
//! text` where `0x80 '|'` breaks the line.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use bevy::prelude::*;
use mgs2_formats::gcx::{Gcx, Stmt, Value};
use mgs2_formats::str_code;

/// One subtitle: frames (60 per second) and text.
#[derive(Clone, Debug)]
pub struct Line {
    pub start: u32,
    pub end: u32,
    pub text: String,
}

/// Demo ids a cutscene stage's script starts, in order.
pub fn demo_ids(script: &Gcx) -> Vec<u32> {
    fn walk(stmts: &[Stmt], out: &mut Vec<u32>) {
        for s in stmts {
            if let Stmt::Command { name, args, options } = s {
                let is_demo = *name == str_code(b"chara")
                    && match args.first() {
                        Some(Value::Str(k)) => k == "demo",
                        Some(Value::Name(h)) => *h == str_code(b"demo"),
                        _ => false,
                    };
                if is_demo {
                    if let Some(Value::Int(_, id)) = options.iter().find(|(l, _)| *l == b'i').and_then(|(_, v)| v.first()) {
                        if !out.contains(&(*id as u32)) {
                            out.push(*id as u32);
                        }
                    }
                }
                for v in args.iter().chain(options.iter().flat_map(|(_, v)| v)) {
                    if let Value::Block(b) = v {
                        walk(b, out);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&script.main, &mut out);
    script.procs.iter().for_each(|p| walk(&p.body, &mut out));
    out
}

/// Reads a demo's subtitles.
pub fn subtitles(iso: &Path, id: u32) -> anyhow::Result<Vec<Line>> {
    let mut f = mgs2_formats::iso::Iso::open(iso)?.open_file("MGS2/DEMO.DAT")?;
    let base = u64::from(id & 0x00FF_FFFF) * 0x800;
    let mut pos = base;
    let mut out = Vec::new();
    let mut head = [0u8; 8];
    for n in 0..20_000 {
        f.seek(SeekFrom::Start(pos))?;
        if f.read_exact(&mut head).is_err() {
            break;
        }
        let kind = u32::from_le_bytes(head[..4].try_into().unwrap());
        let size = u32::from_le_bytes(head[4..].try_into().unwrap()) as u64;
        // The next demo's header, or nothing sensible: the end.
        if size < 8 || size > 0x10_0000 || (kind == 0x10 && n > 2) {
            break;
        }
        if kind == 4 {
            let mut chunk = vec![0u8; size as usize];
            f.seek(SeekFrom::Start(pos))?;
            f.read_exact(&mut chunk)?;
            parse_subtitles(&chunk, &mut out);
        }
        pos += size;
    }
    Ok(out)
}

fn parse_subtitles(chunk: &[u8], out: &mut Vec<Line>) {
    let u32_at = |o: usize| chunk.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let Some(payload) = u32_at(16) else { return };
    let end = (16 + payload as usize).min(chunk.len());
    let mut o = 20;
    while o + 16 <= end {
        let (Some(start), Some(stop), Some(len)) = (u32_at(o), u32_at(o + 4), u32_at(o + 12)) else { break };
        if len < 16 {
            break;
        }
        let text = &chunk[o + 16..(o + len as usize).min(end)];
        let text = text.split(|b| *b == 0).next().unwrap_or_default();
        let text = String::from_utf8_lossy(text).replace("\u{fffd}|", "\n").replace('|', "\n");
        out.push(Line { start, end: stop, text });
        o += len as usize;
    }
}

/// A cutscene being shown (its subtitles, in time).
#[derive(Resource, Default)]
pub struct Cutscene {
    pub lines: Vec<Line>,
    /// Frames since it began.
    pub frame: f32,
    pub name: String,
}

impl Cutscene {
    pub fn active(&self) -> bool {
        !self.lines.is_empty()
    }
}

pub fn idle(c: Res<Cutscene>) -> bool {
    !c.active()
}

/// Loads every demo of a cutscene stage's script.
pub fn load(iso: &Path, name: &str, script: &Gcx) -> Cutscene {
    let mut lines = Vec::new();
    let mut offset = 0;
    for id in demo_ids(script) {
        match subtitles(iso, id) {
            Ok(l) => {
                // Demos play one after another.
                let len = l.iter().map(|x| x.end).max().unwrap_or(0);
                lines.extend(l.into_iter().map(|x| Line { start: x.start + offset, end: x.end + offset, text: x.text }));
                offset += len + 30;
            }
            Err(e) => warn!("demo {id:#x}: {e:#}"),
        }
    }
    info!("cutscene {name}: {} subtitle lines", lines.len());
    Cutscene { lines, frame: 0.0, name: name.to_string() }
}

#[derive(Component)]
pub struct CutsceneScreen;

pub fn setup_screen(mut commands: Commands) {
    commands.spawn((
        CutsceneScreen,
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(24.0), ..default() },
        TextColor(Color::srgb(0.95, 0.95, 0.9)),
        TextLayout::justify(Justify::Center),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.92)),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            padding: UiRect::all(px(40)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::End,
            ..default()
        },
        Visibility::Hidden,
    ));
}

/// Advances the subtitles; Space skips to the next line, Esc ends it.
pub fn play(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut c: ResMut<Cutscene>,
    mut q: Query<(&mut Text, &mut Visibility), With<CutsceneScreen>>,
) {
    let Ok((mut t, mut v)) = q.single_mut() else { return };
    if !c.active() {
        *v = Visibility::Hidden;
        return;
    }
    *v = Visibility::Inherited;
    c.frame += time.delta_secs().min(0.1) * 60.0;
    if keys.just_pressed(KeyCode::Space) {
        if let Some(next) = c.lines.iter().find(|l| l.start as f32 > c.frame) {
            c.frame = next.start as f32;
        } else {
            c.frame = f32::MAX;
        }
    }
    let done = keys.just_pressed(KeyCode::Escape) || c.lines.iter().all(|l| (l.end as f32) < c.frame - 30.0);
    if done {
        c.lines.clear();
        **t = String::new();
        return;
    }
    let current = c.lines.iter().rev().find(|l| (l.start as f32) <= c.frame && c.frame <= l.end as f32);
    **t = format!(
        "[ cutscene {} - the scene itself is not decoded yet ]\n\n{}\n\nSpace: next line   Esc: skip",
        c.name,
        current.map_or("", |l| l.text.as_str())
    );
}
