//! Cutscenes (DEMO.DAT, see `mgs2_formats::demo`).
//!
//! A cutscene stage's script starts demos with `chara "demo" <name> -i
//! 0x40000000+n`: n is the demo's first sector (2048 bytes) in DEMO.DAT.
//! Each demo brings its camera track, the models it loads and an actor
//! stream that places and poses them every 5 frames; the models come from
//! the cutscene stage's own files (and the Tanker's resident ones). The
//! level is not part of the demo, so the stage being shown stays in view
//! where it fits. Audio is not played.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use bevy::prelude::*;
use mgs2_bevy::anim;
use mgs2_bevy::scene::{self, MaterialCache, Part, SCALE};
use mgs2_bevy::stage::StageData;
use mgs2_formats::demo::{self as fmt, Demo, Line};
use mgs2_formats::gcx::{Gcx, Stmt, Value};
use mgs2_formats::str_code;

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
                if is_demo
                    && let Some(Value::Int(_, id)) = options.iter().find(|(l, _)| *l == b'i').and_then(|(_, v)| v.first())
                    && !out.contains(&(*id as u32))
                {
                    out.push(*id as u32);
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

/// Reads one demo's chunks from DEMO.DAT.
pub fn read(iso: &Path, id: u32) -> anyhow::Result<Vec<u8>> {
    let mut f = mgs2_formats::iso::Iso::open(iso)?.open_file("MGS2/DEMO.DAT")?;
    let mut pos = u64::from(id & 0x00FF_FFFF) * 0x800;
    let mut out = Vec::new();
    let mut head = [0u8; 8];
    for n in 0..50_000 {
        f.seek(SeekFrom::Start(pos))?;
        if f.read_exact(&mut head).is_err() {
            break;
        }
        let kind = u32::from_le_bytes(head[..4].try_into().unwrap());
        let size = u32::from_le_bytes(head[4..].try_into().unwrap()) as u64;
        // The next demo's header, or nothing sensible: the end.
        if !(16..=0x10_0000).contains(&size) || (kind == 0x10 && n > 2) {
            break;
        }
        let mut chunk = vec![0u8; size as usize];
        f.seek(SeekFrom::Start(pos))?;
        f.read_exact(&mut chunk)?;
        out.extend_from_slice(&chunk);
        pos += size;
        if kind == 0xF0 {
            break;
        }
    }
    Ok(out)
}

/// A cutscene being shown.
#[derive(Resource, Default)]
pub struct Cutscene {
    /// Subtitles over the whole cutscene.
    pub lines: Vec<Line>,
    /// Its demos, the frame each starts at, and its parking spot.
    pub demos: Vec<(u32, Demo)>,
    parks: Vec<Option<Vec3>>,
    /// The cutscene stage's data (models and textures).
    pub stage: Option<StageData>,
    /// The scenery the cutscene stage's script places (its deck pieces,
    /// sky and sea): models of `stage` and their placement.
    pub scenery: Vec<(usize, Transform)>,
    /// Without scenery, the level of the stage the cutscene was reached
    /// from is shown behind the actors.
    pub backdrop: Option<StageData>,
    /// Frames since it began.
    pub frame: f32,
    pub length: f32,
    pub name: String,
    spawned: bool,
}

impl Cutscene {
    pub fn active(&self) -> bool {
        self.length > 0.0
    }
}

pub fn idle(c: Res<Cutscene>) -> bool {
    !c.active()
}

/// Loads every demo of a cutscene stage's script, played one after another.
pub fn load(iso: &Path, name: &str, script: &Gcx, stage: Option<StageData>) -> Cutscene {
    let mut lines = Vec::new();
    let mut demos = Vec::new();
    let mut offset = 0;
    for id in demo_ids(script) {
        match read(iso, id) {
            Ok(bytes) => {
                let d = fmt::parse(&bytes);
                lines.extend(d.subtitles.iter().map(|x| Line { start: x.start + offset, end: x.end + offset, text: x.text.clone() }));
                let len = d.length.max(d.subtitles.iter().map(|x| x.end).max().unwrap_or(0));
                demos.push((offset, d));
                offset += len + 30;
            }
            Err(e) => warn!("demo {id:#x}: {e:#}"),
        }
    }
    info!(
        "cutscene {name}: {} demos, {} subtitle lines, {} camera keys, {} frames",
        demos.len(),
        lines.len(),
        demos.iter().map(|d| d.1.cameras.len()).sum::<usize>(),
        offset
    );
    let parks = demos.iter().map(|(_, d)| park(d)).collect();
    Cutscene { lines, demos, parks, stage, scenery: Vec::new(), backdrop: None, frame: 0.0, length: offset as f32, name: name.to_string(), spawned: false }
}

/// Where a demo parks the actors it is not showing: the position most of
/// them share at the start (three or more). How the player hides actors
/// and attaches them to others is not decoded yet; parked actors and those
/// near the origin (placed relative to a parent) are hidden.
fn park(d: &Demo) -> Option<Vec3> {
    let mut counts: Vec<([i32; 3], usize)> = Vec::new();
    for keys in d.actors.values() {
        let Some(k) = keys.first() else { continue };
        let p = k.pos.map(|v| v.round() as i32);
        match counts.iter_mut().find(|(q, _)| *q == p) {
            Some((_, n)) => *n += 1,
            None => counts.push((p, 1)),
        }
    }
    counts.into_iter().filter(|(_, n)| *n >= 3).max_by_key(|(_, n)| *n).map(|(p, _)| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32))
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
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::End,
            align_items: AlignItems::Center,
            ..default()
        },
        GlobalZIndex(5),
        Visibility::Hidden,
    ));
}

/// The backdrop level's pieces.
#[derive(Component, Clone)]
pub struct Backdrop;

/// A model a demo loaded.
#[derive(Component)]
pub struct DemoActor {
    demo: usize,
    serial: u32,
    model: usize,
}

/// Advances the cutscene; Space skips to the next line, Esc ends it.
pub fn play(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut c: ResMut<Cutscene>,
    mut q: Query<(&mut Text, &mut Visibility, &mut BackgroundColor, &mut Node), With<CutsceneScreen>>,
) {
    let Ok((mut t, mut v, mut bg, mut node)) = q.single_mut() else { return };
    if !c.active() {
        *v = Visibility::Hidden;
        return;
    }
    *v = Visibility::Inherited;
    c.frame += time.delta_secs().min(0.1) * 60.0;
    if keys.just_pressed(KeyCode::Space) {
        c.frame = c.lines.iter().find(|l| l.start as f32 > c.frame).map_or(c.length, |l| l.start as f32);
    }
    if keys.just_pressed(KeyCode::Escape) || c.frame >= c.length {
        c.length = 0.0;
        **t = String::new();
        return;
    }
    let shown = c.stage.is_some() && c.demos.iter().any(|(_, d)| !d.cameras.is_empty());
    *bg = BackgroundColor(if shown { Color::NONE } else { Color::srgba(0.0, 0.0, 0.0, 0.92) });
    // Over the scene the subtitles sit at the bottom; without it the
    // screen stays dark.
    let top = if shown { Val::Auto } else { px(0) };
    if node.top != top {
        node.top = top;
    }
    let current = c.lines.iter().rev().find(|l| (l.start as f32) <= c.frame && c.frame <= l.end as f32);
    let note = if shown { "" } else { "[ the scene is not shown ]\n\n" };
    **t = format!("{note}{}\n\nSpace: next line   Esc: skip", current.map_or("", |l| l.text.as_str()));
}

/// The demo whose time span holds the cutscene frame, and the frame in it.
fn current(c: &Cutscene) -> Option<(usize, f32)> {
    let f = c.frame;
    c.demos.iter().enumerate().rev().find(|(_, (off, _))| (*off as f32) <= f).map(|(i, (off, _))| (i, f - *off as f32))
}

/// Spawns the demos' models once, hides the level while the cutscene runs
/// and restores it after.
#[allow(clippy::too_many_arguments)]
pub fn stage(
    mut commands: Commands,
    mut c: ResMut<Cutscene>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    actors: Query<Entity, Or<(With<DemoActor>, With<Backdrop>)>>,
    mut level: Query<&mut Visibility, (With<crate::LevelEntity>, Without<ChildOf>, Without<DemoActor>, Without<Backdrop>)>,
    mut cam: Query<&mut Projection, With<crate::player::FollowCam>>,
    mut normal_fov: Local<Option<f32>>,
    mut huds: Query<&mut Visibility, (Or<(With<crate::Hud>, With<crate::photo::PhotoHud>)>, Without<crate::LevelEntity>, Without<DemoActor>, Without<Backdrop>)>,
) {
    let hud = if c.active() { Visibility::Hidden } else { Visibility::Inherited };
    for mut v in &mut huds {
        if *v != hud {
            *v = hud;
        }
    }
    if !c.active() {
        if c.spawned || !actors.is_empty() {
            for e in &actors {
                commands.entity(e).despawn();
            }
            for mut v in &mut level {
                *v = Visibility::Inherited;
            }
            if let (Ok(mut p), Some(f)) = (cam.single_mut(), normal_fov.take())
                && let Projection::Perspective(pp) = p.as_mut()
            {
                pp.fov = f;
            }
            c.spawned = false;
            c.stage = None;
            c.scenery.clear();
            c.backdrop = None;
            c.demos.clear();
        }
        return;
    }
    if c.stage.is_none() || c.demos.iter().all(|(_, d)| d.cameras.is_empty()) {
        return;
    }
    for mut v in &mut level {
        if *v != Visibility::Hidden {
            *v = Visibility::Hidden;
        }
    }
    if c.spawned {
        return;
    }
    c.spawned = true;
    if let Ok(p) = cam.single()
        && let Projection::Perspective(pp) = p
    {
        *normal_fov = Some(pp.fov);
    }
    let c = c.into_inner();
    let data = c.stage.as_ref().unwrap();
    let mut cull = MaterialCache { cull: Some(bevy::render::render_resource::Face::Front), ..default() };
    if !c.scenery.is_empty() {
        for (model, tf) in &c.scenery {
            let e = scene::spawn_model(&mut commands, &data.models[*model].1, data, &mut cull, &mut meshes, &mut images, &mut materials, true, Backdrop);
            commands.entity(e).insert(*tf);
        }
    } else if let Some(b) = &c.backdrop {
        scene::spawn_level(&mut commands, b, &mut cull, &mut meshes, &mut images, &mut materials, true, Backdrop);
    }
    let mut cache = MaterialCache::default();
    let mut n = 0;
    for (di, (_, d)) in c.demos.iter().enumerate() {
        for m in &d.models {
            let hash = m.file & 0x00FF_FFFF;
            let Some(model) = data.models.iter().position(|(h, _)| *h & 0x00FF_FFFF == hash) else {
                debug!("demo model {:#x} not found", m.file);
                continue;
            };
            let e = scene::spawn_model(
                &mut commands,
                &data.models[model].1,
                data,
                &mut cache,
                &mut meshes,
                &mut images,
                &mut materials,
                true,
                DemoActor { demo: di, serial: m.serial, model },
            );
            commands.entity(e).insert(Visibility::Hidden);
            n += 1;
        }
    }
    info!("cutscene {}: {n} models spawned", c.name);
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> Vec3 {
    Vec3::from(a).lerp(Vec3::from(b), t)
}

/// Places the camera and poses the actors for the current frame.
pub fn animate(
    c: Res<Cutscene>,
    mut actors: Query<(Entity, &DemoActor, &mut Transform, &mut Visibility)>,
    children: Query<&Children>,
    parts: Query<(&Part, &Mesh3d)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cam: Query<(&mut Transform, &mut Projection), (With<crate::player::FollowCam>, Without<DemoActor>)>,
) {
    if !c.active() || !c.spawned {
        return;
    }
    let (Some(data), Some((di, f))) = (c.stage.as_ref(), current(&c)) else { return };
    let demo = &c.demos[di].1;

    // Camera: the keys come every 5 frames; ease between them.
    let k = demo.cameras.partition_point(|k| (k.frame as f32) <= f);
    if k > 0
        && let Ok((mut tf, mut proj)) = cam.single_mut()
    {
        let a = demo.cameras[k - 1];
        let b = demo.cameras.get(k).copied().unwrap_or(a);
        let t = if b.frame > a.frame { ((f - a.frame as f32) / (b.frame - a.frame) as f32).clamp(0.0, 1.0) } else { 0.0 };
        // A cut: do not ease across a jump.
        let t = if Vec3::from(a.eye).distance(Vec3::from(b.eye)) > 3000.0 { 0.0 } else { t };
        let eye = lerp3(a.eye, b.eye, t) * SCALE;
        let target = lerp3(a.target, b.target, t) * SCALE;
        let roll = a.roll + (b.roll - a.roll) * t;
        tf.translation = eye;
        tf.look_at(target, Vec3::Y);
        tf.rotate_local_z(roll);
        if let Projection::Perspective(pp) = proj.as_mut() {
            let zoom = (a.zoom + (b.zoom - a.zoom) * t).max(0.1);
            pp.fov = 2.0 * (1.0 / zoom).atan();
        }
    }

    for (e, a, mut tf, mut vis) in &mut actors {
        let model_info = demo.models.iter().find(|m| m.serial == a.serial);
        let alive = a.demo == di
            && model_info.is_some_and(|m| (m.from as f32) <= f && m.until.is_none_or(|u| f < u as f32));
        let keys = if alive { demo.actors.get(&a.serial) } else { None };
        let Some(keys) = keys.filter(|k| !k.is_empty()) else {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
            continue;
        };
        let i = keys.partition_point(|k| (k.frame as f32) <= f).max(1);
        let ka = &keys[i - 1];
        let kb = keys.get(i).unwrap_or(ka);
        let p = Vec3::from(ka.pos);
        let parked = c.parks[di].is_some_and(|q| q.distance(p) < 2.0) || p.length() < 1500.0;
        if parked {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
            continue;
        }
        let t = if kb.frame > ka.frame { ((f - ka.frame as f32) / (kb.frame - ka.frame) as f32).clamp(0.0, 1.0) } else { 0.0 };
        if *vis != Visibility::Inherited {
            *vis = Visibility::Inherited;
        }
        let ang = |k: &fmt::ActorKey, j: usize| f32::from(k.angles[j] as i16) / 4096.0 * std::f32::consts::TAU;
        let ra = Quat::from_euler(EulerRot::YXZ, ang(ka, 1), ang(ka, 0), ang(ka, 2));
        let rb = Quat::from_euler(EulerRot::YXZ, ang(kb, 1), ang(kb, 0), ang(kb, 2));
        tf.translation = lerp3(ka.pos, kb.pos, t) * SCALE;
        tf.rotation = ra.slerp(rb, t);

        let model = &data.models[a.model].1;
        let rots: Vec<Quat> = ka
            .joints
            .iter()
            .zip(kb.joints.iter().chain(std::iter::repeat(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0])))
            .take(model.objects.len())
            .map(|(x, y)| {
                let qa = Quat::from_xyzw(x[0], x[1], x[2], x[3]).normalize();
                let qb = Quat::from_xyzw(y[0], y[1], y[2], y[3]).normalize();
                qa.slerp(qb, t)
            })
            .collect();
        if rots.is_empty() {
            continue;
        }
        let root = ka.joints.first().map(|j| Vec3::new(j[4], j[5], j[6])).filter(|v| *v != Vec3::ZERO);
        let mats = anim::pose_joints(model, &rots, root);
        for child in children.iter_descendants(e) {
            let Ok((part, mesh3d)) = parts.get(child) else { continue };
            let Some(mut mesh) = meshes.get_mut(&mesh3d.0) else { continue };
            let (pos, nrm) = anim::skin(model, part.object, part.mesh, &mats);
            let pos: Vec<[f32; 3]> = pos.into_iter().map(|v| (Vec3::from(v) * SCALE).into()).collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
        }
    }
}
