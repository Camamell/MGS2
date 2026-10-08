//! Viewer for Metal Gear Solid 2 Tanker stages, read straight from your own
//! disc image.
//!
//! Controls: right mouse + move to look, WASD/QE to fly (Shift = fast),
//! [ / ] previous/next stage, C collision overlay, V second collision set,
//! L lit/unlit, B model browser (N / P next/previous model), F overview,
//! G jump to a standing spot on the main floor.

use std::path::PathBuf;

use mgs2_bevy::{anim, camera, scene, stage};

use anyhow::{Context, Result};
use bevy::prelude::*;
use clap::Parser;
use mgs2_formats::hzx;

use camera::FlyCam;
use scene::{MaterialCache, SCALE};
use stage::{Disc, StageData};

#[derive(Parser)]
#[command(about = "View Metal Gear Solid 2 (PS2, SLUS-20144) Tanker stages")]
struct Cli {
    /// The game's .iso (or set MGS2_ISO).
    #[arg(long)]
    iso: Option<PathBuf>,
    /// Stage to open first.
    #[arg(default_value = "w00a")]
    stage: String,
    /// Start with the collision overlay on.
    #[arg(long)]
    collision: bool,
    /// Start in the model browser at this model index.
    #[arg(long)]
    browse: Option<usize>,
    /// Start in the model browser on this model (hex name hash, e.g. 413aa8
    /// for Snake's sna_def).
    #[arg(long)]
    model: Option<String>,
    /// Start with lighting enabled.
    #[arg(long)]
    lit: bool,
    /// Camera position and look-at target in game units (mm): x,y,z,tx,ty,tz.
    #[arg(long, value_delimiter = ',', allow_negative_numbers = true)]
    cam: Option<Vec<f32>>,
    /// Save a screenshot to this path after a few frames, then exit.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// Load every Tanker stage, print a summary and exit (no window).
    #[arg(long)]
    check: bool,
    /// In the model browser, start this motion of the matching archive.
    #[arg(long)]
    motion: Option<usize>,
    /// Hold the motion at this frame instead of playing it.
    #[arg(long)]
    frame: Option<f32>,
    /// Apply joint quaternions conjugated (handedness experiment).
    #[arg(long)]
    conj: bool,
}

/// Motion playback state for the model browser.
#[derive(Default)]
struct Anim {
    /// Index into `StageData::motions`.
    set: Option<usize>,
    motion: usize,
    frame: f32,
    playing: bool,
    conjugate: bool,
}

#[derive(Resource)]
struct Capture {
    path: PathBuf,
    frame: u32,
}

#[derive(Resource)]
struct CamOverride(Vec3, Vec3);

#[derive(Resource)]
struct Viewer {
    disc: Disc,
    stages: Vec<String>,
    index: usize,
    data: StageData,
    cache: MaterialCache,
    unlit: bool,
    show_collision: bool,
    show_collision2: bool,
    /// `Some(i)`: browsing model i instead of showing the level.
    browse: Option<usize>,
    reload: bool,
    reframe: bool,
    /// Next reframe goes to a standing spot on the main floor.
    ground_view: bool,
    anim: Anim,
}

/// Everything spawned for the current stage/model; despawned on reload.
#[derive(Component)]
struct SceneRoot;

#[derive(Component)]
struct Hud;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let iso = cli.iso.or_else(stage::default_iso).context("pass --iso <path to MGS2 .iso> or set MGS2_ISO")?;
    let mut disc = stage::open_disc(&iso)?;
    let stages = stage::tanker_stages(&disc);
    if cli.check {
        for s in &stages {
            let d = stage::load(&mut disc, s)?;
            let placed = d.pieces.iter().filter(|p| d.models.iter().any(|(h, _)| *h == p.model)).count();
            let missing: std::collections::HashSet<u32> = d
                .pieces
                .iter()
                .filter_map(|p| d.models.iter().find(|(h, _)| *h == p.model))
                .flat_map(|(_, m)| m.objects.iter().flat_map(|o| o.meshes.iter().map(|x| x.textures[0])))
                .filter(|t| *t != 0 && !d.textures.contains_key(t))
                .collect();
            println!(
                "{s:8} pieces {placed}/{}  models {:3}  textures {:3}  level textures missing {:2}  collision {}  warnings {}",
                d.pieces.len(),
                d.models.len(),
                d.textures.len(),
                missing.len(),
                d.collision.is_some(),
                d.warnings.len()
            );
        }
        return Ok(());
    }
    let index = stages.iter().position(|s| *s == cli.stage).context("unknown or non-Tanker stage")?;
    let data = stage::load(&mut disc, &stages[index])?;
    let browse = match &cli.model {
        Some(h) => {
            let h = u32::from_str_radix(h.trim_start_matches("0x"), 16).context("--model takes a hex hash")?;
            Some(data.models.iter().position(|(m, _)| *m == h).context("no model with that hash in this stage")?)
        }
        None => cli.browse,
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "MGS2 Tanker viewer".into(), ..default() }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.05, 0.06, 0.08)))
    .insert_resource(Viewer {
        disc,
        stages,
        index,
        data,
        cache: MaterialCache::default(),
        unlit: !cli.lit,
        show_collision: cli.collision,
        show_collision2: false,
        browse,
        reload: true,
        reframe: true,
        ground_view: false,
        anim: Anim {
            set: None,
            motion: cli.motion.unwrap_or(0),
            frame: cli.frame.unwrap_or(0.0),
            playing: cli.frame.is_none(),
            conjugate: cli.conj,
        },
    })
    .add_systems(Startup, setup)
    .add_systems(Update, (input, rebuild, animate, apply_cam_override, camera::fly, draw_collision, hud).chain());
    if let Some(c) = cli.cam.filter(|c| c.len() == 6) {
        app.insert_resource(CamOverride(Vec3::new(c[0], c[1], c[2]) * SCALE, Vec3::new(c[3], c[4], c[5]) * SCALE));
    }
    if let Some(path) = cli.screenshot {
        app.insert_resource(Capture { path, frame: 0 }).add_systems(Last, capture);
    }
    app.run();
    Ok(())
}

fn apply_cam_override(
    mut commands: Commands,
    o: Option<Res<CamOverride>>,
    mut cam: Query<(&mut Transform, &mut FlyCam)>,
) {
    let Some(o) = o else { return };
    if let Ok((mut tf, mut fc)) = cam.single_mut() {
        fc.look_at(&mut tf, o.0, o.1);
        commands.remove_resource::<CamOverride>();
    }
}

/// Waits for shaders to compile and assets to upload, saves one screenshot,
/// then exits.
fn capture(mut commands: Commands, time: Res<Time>, mut c: ResMut<Capture>, mut exit: MessageWriter<AppExit>) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    c.frame += 1;
    let t = time.elapsed_secs();
    if c.frame < 1_000_000 && t > 8.0 {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(c.path.clone()));
        c.frame = 1_000_000;
    }
    if t > 10.0 {
        exit.write(AppExit::Success);
    }
}

fn setup(mut commands: Commands) {
    commands.spawn((Camera3d::default(), Transform::default(), FlyCam::default()));
    commands.insert_resource(GlobalAmbientLight { brightness: 400.0, ..default() });
    commands.spawn((
        DirectionalLight { illuminance: 6000.0, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 0.6, -0.9, 0.0)),
    ));
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(14.0), ..default() },
        Node { position_type: PositionType::Absolute, top: px(8), left: px(8), ..default() },
    ));
}

fn input(keys: Res<ButtonInput<KeyCode>>, mut v: ResMut<Viewer>) {
    let n = v.stages.len();
    if keys.just_pressed(KeyCode::BracketRight) {
        v.index = (v.index + 1) % n;
        v.browse = None;
        reload_stage(&mut v);
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        v.index = (v.index + n - 1) % n;
        v.browse = None;
        reload_stage(&mut v);
    }
    if keys.just_pressed(KeyCode::KeyC) {
        v.show_collision = !v.show_collision;
    }
    // Motion playback (model browser).
    if let Some(set) = v.anim.set {
        let count = v.data.motions[set].1.motions.len().max(1);
        let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        if keys.just_pressed(KeyCode::KeyM) {
            v.anim.motion = if shift { (v.anim.motion + count - 1) % count } else { (v.anim.motion + 1) % count };
            v.anim.frame = 0.0;
        }
        if keys.just_pressed(KeyCode::KeyJ) {
            if let Some((_, model)) = v.browse.and_then(|i| v.data.models.get(i)) {
                let sets = v.data.motion_sets_for(model);
                let pos = sets.iter().position(|s| *s == set).map_or(0, |p| (p + 1) % sets.len());
                v.anim.set = sets.get(pos).copied();
                v.anim.motion = 0;
                v.anim.frame = 0.0;
            }
        }
    }
    if keys.just_pressed(KeyCode::Space) {
        v.anim.playing = !v.anim.playing;
    }
    if keys.just_pressed(KeyCode::KeyK) {
        v.anim.conjugate = !v.anim.conjugate;
    }
    if keys.just_pressed(KeyCode::KeyV) {
        v.show_collision2 = !v.show_collision2;
    }
    if keys.just_pressed(KeyCode::KeyL) {
        v.unlit = !v.unlit;
        v.cache = MaterialCache::default();
        v.reload = true;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        v.reload = true;
        v.reframe = true;
    }
    if keys.just_pressed(KeyCode::KeyG) {
        v.reload = true;
        v.reframe = true;
        v.ground_view = true;
    }
    if keys.just_pressed(KeyCode::KeyB) {
        v.browse = if v.browse.is_some() { None } else { Some(0) };
        v.reload = true;
        v.reframe = true;
    }
    let models = v.data.models.len().max(1);
    if let Some(i) = v.browse {
        let step = if keys.just_pressed(KeyCode::KeyN) {
            1
        } else if keys.just_pressed(KeyCode::KeyP) {
            models - 1
        } else {
            0
        };
        if step != 0 {
            v.browse = Some((i + step) % models);
            v.reload = true;
            v.reframe = true;
        }
    }
}

fn reload_stage(v: &mut Viewer) {
    let name = v.stages[v.index].clone();
    match stage::load(&mut v.disc, &name) {
        Ok(d) => v.data = d,
        Err(e) => error!("loading {name}: {e:#}"),
    }
    v.cache = MaterialCache::default();
    v.reload = true;
    v.reframe = true;
}

#[allow(clippy::too_many_arguments)]
fn rebuild(
    mut commands: Commands,
    mut v: ResMut<Viewer>,
    roots: Query<Entity, With<SceneRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cam: Query<(&mut Transform, &mut FlyCam)>,
) {
    if !v.reload {
        return;
    }
    v.reload = false;
    for e in &roots {
        commands.entity(e).despawn();
    }
    let v = &mut *v;
    // Pick a motion archive that fits the browsed model.
    match v.browse.and_then(|i| v.data.models.get(i)) {
        Some((_, model)) => {
            let sets = v.data.motion_sets_for(model);
            if !v.anim.set.is_some_and(|s| sets.contains(&s)) {
                v.anim.set = sets.first().copied();
                v.anim.motion = v.anim.motion.min(
                    v.anim.set.map_or(0, |s| v.data.motions[s].1.motions.len().saturating_sub(1)),
                );
            }
        }
        None => v.anim.set = None,
    }
    // (model index, position in metres)
    let shown: Vec<(usize, Vec3)> = match v.browse {
        Some(i) => vec![(i.min(v.data.models.len().saturating_sub(1)), Vec3::ZERO)],
        None => v
            .data
            .pieces
            .iter()
            .filter_map(|p| {
                let i = v.data.models.iter().position(|(h, _)| *h == p.model)?;
                Some((i, Vec3::from(p.position) * SCALE))
            })
            .collect(),
    };
    info!(
        "{}: {} map pieces in script, {} found among {} models",
        v.data.name,
        v.data.pieces.len(),
        shown.len(),
        v.data.models.len()
    );
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for &(i, pos) in &shown {
        let Some((_, model)) = v.data.models.get(i) else { continue };
        let e = scene::spawn_model(
            &mut commands,
            model,
            &v.data,
            &mut v.cache,
            &mut meshes,
            &mut images,
            &mut materials,
            v.unlit,
            SceneRoot,
        );
        commands.entity(e).insert(Transform::from_translation(pos));
        let (c, r) = scene::bounds(model);
        lo = lo.min(c + pos - Vec3::splat(r));
        hi = hi.max(c + pos + Vec3::splat(r));
    }
    if v.reframe && !shown.is_empty() {
        v.reframe = false;
        if let Ok((mut tf, mut fc)) = cam.single_mut() {
            let (c, r) = ((lo + hi) / 2.0, (hi - lo).length() / 2.0 / 3f32.sqrt());
            let ground = std::mem::take(&mut v.ground_view);
            let (from, target, speed) = match (v.browse, ground.then(|| standing_spot(&v.data, c)).flatten()) {
                (Some(_), _) => (c + Vec3::new(0.0, r * 0.4, r * 2.2), c, r.max(1.0)),
                // Eye height on the main floor level, looking at the centre.
                (None, Some(p)) => {
                    let eye = p + Vec3::Y * 1.7;
                    let mut to = Vec3::new(c.x, eye.y, c.z);
                    if to.distance(eye) < 1.0 {
                        to = eye + Vec3::NEG_Z;
                    }
                    (eye, to, 4.0)
                }
                // Overview from outside the level's bounds.
                (None, None) => (c + Vec3::new(0.0, r * 0.5, r * 1.1), c, 10.0),
            };
            fc.speed = speed;
            fc.look_at(&mut tf, from, target);
        }
    }
}

/// Poses the browsed model with the current motion (CPU skinning).
fn animate(
    time: Res<Time>,
    mut v: ResMut<Viewer>,
    parts: Query<(&scene::Part, &Mesh3d)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let (Some(set), Some(model_i)) = (v.anim.set, v.browse) else { return };
    if v.anim.playing {
        v.anim.frame += time.delta_secs() * 60.0;
    }
    let Some((_, model)) = v.data.models.get(model_i) else { return };
    let motion = v.data.motions[set].1.motions.get(v.anim.motion);
    let mats = anim::pose(model, motion, v.anim.frame, v.anim.conjugate);
    for (part, mesh3d) in &parts {
        let Some(mut mesh) = meshes.get_mut(&mesh3d.0) else { continue };
        let (pos, nrm) = anim::skin(model, part.object, part.mesh, &mats);
        let pos: Vec<[f32; 3]> = pos.into_iter().map(|p| (Vec3::from(p) * SCALE).into()).collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    }
}

/// A place to stand (metres): on the floor height with the most walkable
/// area (usually the main deck), the polygon closest to `centre`
/// horizontally.
fn standing_spot(data: &StageData, centre: Vec3) -> Option<Vec3> {
    let h = data.collision.as_ref()?;
    // (centroid, area) of every upward-facing floor polygon.
    let floors: Vec<(Vec3, f32)> = h
        .areas
        .iter()
        .flat_map(|a| &a.groups)
        .flat_map(|g| &g.floors)
        .flat_map(|f| &f.polygons)
        .filter(|p| p.normal[1] > 0.7 && p.vertices.len() >= 3)
        .map(|p| {
            let v: Vec<Vec3> = p.vertices.iter().map(|v| Vec3::from(*v) * SCALE).collect();
            let area = (1..v.len() - 1).map(|i| (v[i] - v[0]).cross(v[i + 1] - v[0]).length() / 2.0).sum();
            (v.iter().sum::<Vec3>() / v.len() as f32, area)
        })
        .collect();
    let mut by_height: std::collections::HashMap<i32, f32> = Default::default();
    for (c, a) in &floors {
        *by_height.entry((c.y * 10.0).round() as i32).or_default() += a;
    }
    let (&level, _) = by_height.iter().max_by(|a, b| a.1.total_cmp(b.1))?;
    floors
        .iter()
        .filter(|(c, _)| (c.y * 10.0).round() as i32 == level)
        .map(|(c, _)| *c)
        .min_by(|a, b| {
            let d = |p: &Vec3| Vec2::new(p.x - centre.x, p.z - centre.z).length_squared();
            d(a).total_cmp(&d(b))
        })
}

fn draw_collision(v: Res<Viewer>, mut gizmos: Gizmos) {
    if v.browse.is_some() {
        return;
    }
    let Some(h) = &v.data.collision else { return };
    let p = |a: [f32; 3]| Vec3::from(a) * SCALE;
    let mut draw = |walls: &[hzx::Wall], floors: &[hzx::Floor], wall_c: Color, floor_c: Color| {
        for w in walls {
            for pair in w.points.windows(2) {
                let ((a, ha), (b, hb)) = (pair[0], pair[1]);
                let (a, b) = (p(a), p(b));
                let (ta, tb) = (a + Vec3::Y * ha * SCALE, b + Vec3::Y * hb * SCALE);
                gizmos.line(a, b, wall_c);
                gizmos.line(ta, tb, wall_c);
                gizmos.line(a, ta, wall_c);
                gizmos.line(b, tb, wall_c);
            }
        }
        for f in floors {
            for poly in &f.polygons {
                let n = poly.vertices.len();
                for i in 0..n {
                    gizmos.line(p(poly.vertices[i]), p(poly.vertices[(i + 1) % n]), floor_c);
                }
            }
        }
    };
    for g in h.areas.iter().flat_map(|a| &a.groups) {
        if v.show_collision {
            draw(&g.walls, &g.floors, Color::srgb(1.0, 0.2, 0.15), Color::srgb(0.2, 1.0, 0.3));
        }
        if v.show_collision2 {
            draw(&g.walls2, &g.floors2, Color::srgb(1.0, 0.8, 0.1), Color::srgb(0.2, 0.6, 1.0));
        }
    }
}

fn hud(v: Res<Viewer>, cam: Query<&Transform, With<FlyCam>>, mut text: Query<&mut Text, With<Hud>>) {
    let Ok(mut t) = text.single_mut() else { return };
    let pos = cam.single().map(|tf| tf.translation / SCALE).unwrap_or_default();
    let mode = match v.browse {
        Some(i) => {
            let (h, m) = &v.data.models[i.min(v.data.models.len().saturating_sub(1))];
            let anim = match v.anim.set {
                Some(s) => {
                    let (mh, mar) = &v.data.motions[s];
                    let mo = mar.motions.get(v.anim.motion);
                    format!(
                        "\nmotion {}/{} of {:06x}.mar  id {:#x}  frame {:.0}/{}  joints {}{}{}   M/Shift+M motion  J archive  Space pause  K conjugate",
                        v.anim.motion + 1,
                        mar.motions.len(),
                        mh,
                        mo.map_or(0, |m| m.id),
                        v.anim.frame % mo.map_or(1, |m| m.length.max(1)) as f32,
                        mo.map_or(0, |m| m.length),
                        mo.map_or(0, |m| m.joints.len()),
                        if v.anim.playing { "" } else { "  (paused)" },
                        if v.anim.conjugate { "  (conjugated)" } else { "" },
                    )
                }
                None => "\nno matching motion archive in this stage".into(),
            };
            format!(
                "model {}/{}  {:06x}.kms  objects {}  textures {:06x}.tri{anim}",
                i + 1,
                v.data.models.len(),
                h,
                m.objects.len(),
                m.texture_set
            )
        }
        None if v.data.pieces.is_empty() => "no level geometry (cutscene stage) - press B to browse models".into(),
        None => format!("level ({} pieces)", v.data.pieces.len()),
    };
    **t = format!(
        "{} ({}/{})  {}\n\
         camera {:.0} {:.0} {:.0} mm   textures {}  models {}  missing textures {}{}\n\
         RMB look  WASD/QE fly  Shift fast  [ ] stage  C collision{}  V collision 2{}  L {}  B browse  N/P model  F overview  G ground",
        v.data.name,
        v.index + 1,
        v.stages.len(),
        mode,
        pos.x,
        pos.y,
        pos.z,
        v.data.textures.len(),
        v.data.models.len(),
        v.cache.missing.len(),
        if v.data.warnings.is_empty() { String::new() } else { format!("  warnings {}", v.data.warnings.len()) },
        if v.show_collision { "*" } else { "" },
        if v.show_collision2 { "*" } else { "" },
        if v.unlit { "unlit" } else { "lit" },
    );
}
