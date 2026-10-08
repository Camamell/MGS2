//! Metal Gear Solid 2: the Tanker chapter, rebuilt on Bevy from the data on
//! your own disc.
//!
//! Controls: WASD move (run), Shift walk, C crouch, Q/E or right mouse drag to
//! turn the camera, F1 collision overlay, F2 guard vision cones.

mod body;
mod bullet;
mod collision;
mod gcl;
mod guard;
mod nav;
mod player;

use std::path::PathBuf;

use anyhow::{Context, Result};
use bevy::prelude::*;
use clap::Parser;
use mgs2_bevy::scene::{self, MaterialCache, SCALE};
use mgs2_bevy::stage::{self, StageData};
use mgs2_formats::str_code;

use body::Body;
use collision::Collision;
use guard::{Guard, Phase};
use player::{AutoWalk, FollowCam, Player};

#[derive(Parser)]
#[command(about = "Metal Gear Solid 2 Tanker chapter (needs your SLUS-20144 disc image)")]
struct Cli {
    /// The game's .iso (or set MGS2_ISO).
    #[arg(long)]
    iso: Option<PathBuf>,
    #[arg(default_value = "w00a")]
    stage: String,
    /// Arrive as if coming from this stage (its script supplies Snake's start
    /// and the story marker). Defaults to the opening cutscene `d01t` for
    /// w00a; `none` uses `--entry` instead.
    #[arg(long)]
    from: Option<String>,
    /// Which arrival from `--from` to use (some stages have several).
    #[arg(long, default_value_t = 0)]
    arrival: usize,
    /// Which of the stage's own entry points to start at (without `--from`).
    #[arg(long, default_value_t = 0)]
    entry: usize,
    /// 10 very easy, 20 easy, 30 normal, 40 hard, 50 extreme (the game's
    /// values of script variable Bs16:0x8).
    #[arg(long, default_value_t = 30)]
    difficulty: i16,
    /// Initial camera yaw in degrees.
    #[arg(long, default_value_t = 0.0)]
    cam_yaw: f32,
    /// Camera distance (m) and pitch (degrees above horizontal).
    #[arg(long, default_value_t = 6.5)]
    cam_dist: f32,
    #[arg(long, default_value_t = 60.0)]
    cam_pitch: f32,
    /// Hold "forward" for this many seconds (for automated testing).
    #[arg(long)]
    auto_walk: Option<f32>,
    /// Start the auto-walk this many seconds in.
    #[arg(long, default_value_t = 0.0)]
    walk_from: f32,
    /// Put Snake here instead (mm, `--snake-at=x,y,z`; for testing).
    #[arg(long, value_delimiter = ',', allow_hyphen_values = true)]
    snake_at: Option<Vec<f32>>,
    /// Start with guard vision cones shown.
    #[arg(long)]
    vision: bool,
    /// Point the camera at the n-th guard instead of Snake.
    #[arg(long)]
    follow_guard: Option<usize>,
    /// Log guard positions and states every second.
    #[arg(long)]
    trace: bool,
    /// Print every actor the stage script creates.
    #[arg(long)]
    spawns: bool,
    /// Save a screenshot after `--shot-at` seconds, then exit.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    #[arg(long, default_value_t = 8.0)]
    shot_at: f32,
}

#[derive(Resource)]
pub struct Game {
    pub data: StageData,
    pub collision: Option<Collision>,
    pub nav: Option<nav::Nav>,
    pub show_collision: bool,
    pub show_vision: bool,
}

/// Characters to spawn at startup: (component bundle kind, model, motions,
/// position, facing).
struct Cast {
    snake: Option<(usize, Option<usize>, Vec3, f32)>,
    guards: Vec<(Guard, usize, Option<usize>, Vec3)>,
}

#[derive(Resource)]
struct Start {
    cast: std::sync::Mutex<Option<Cast>>,
    difficulty: i16,
    /// Snake's start, for continuing after a game over.
    snake_start: Option<(Vec3, f32)>,
    caution_time: i32,
    cam_yaw: f32,
    cam_dist: f32,
    cam_pitch: f32,
    follow_guard: Option<usize>,
}

#[derive(Resource)]
struct Capture {
    path: PathBuf,
    at: f32,
    taken: bool,
}

#[derive(Component)]
struct Hud;

fn model_index(data: &StageData, name: &str) -> Option<usize> {
    data.models.iter().position(|(h, _)| *h == str_code(name.as_bytes()))
}

fn motions_index(data: &StageData, name: &str) -> Option<usize> {
    data.motions.iter().position(|(h, _)| *h == str_code(name.as_bytes()))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let iso = cli.iso.or_else(stage::default_iso).context("pass --iso <path to MGS2 .iso> or set MGS2_ISO")?;
    let mut disc = stage::open_disc(&iso)?;
    let data = stage::load(&mut disc, &cli.stage)?;
    let collision = data.collision.as_ref().map(Collision::new);
    let nav = data.collision.as_ref().map(nav::Nav::new);

    // Snake's start: how the previous stage sends him here, else one of this
    // stage's own entry points; dropped onto the floor.
    let from = cli.from.clone().or_else(|| (cli.stage == "w00a").then(|| "d01t".to_string()));
    let arrival = match from.as_deref() {
        Some("none") | None => None,
        Some(f) => stage::arrivals(&mut disc, f, &cli.stage)?.get(cli.arrival).copied(),
    };
    let progress = arrival.and_then(|a| a.progress);
    let entry = arrival.map(|a| a.entry).or_else(|| data.entries.get(cli.entry).copied());
    let (mut pos, direction) = entry.map_or((Vec3::ZERO, 0), |e| (Vec3::from(e.position), e.direction));
    if let Some(at) = cli.snake_at.as_ref().filter(|a| a.len() == 3) {
        pos = Vec3::new(at[0], at[1], at[2]);
    }
    if let Some(col) = &collision {
        if let Some(y) = col.floor_below(pos.x, pos.z, pos.y + 2000.0) {
            pos.y = y;
        }
    }
    let snake = model_index(&data, "sna_def")
        .map(|m| (m, motions_index(&data, "snake"), pos, direction as f32 / 4096.0 * std::f32::consts::TAU));

    // Run the stage script to find out which guards it creates.
    let mut guards = Vec::new();
    let mut phase = Phase::default();
    if let Some(script) = &data.script {
        let mut vm = gcl::Vm::new(script);
        vm.set_b16(gcl::DIFFICULTY, cli.difficulty);
        if let Some(p) = progress {
            vm.set_a32(gcl::PROGRESS, p);
        }
        vm.run(&script.main);
        let (gm, gmar) = (model_index(&data, "gbs_def"), motions_index(&data, "gbs"));
        for s in vm.spawns.iter().filter(|s| s.kind == guard::GUARD_ACTOR) {
            if let (Some((g, mut p)), Some(m)) = (guard::from_spawn(s, 6000.0), gm) {
                if let Some(col) = &collision {
                    if let Some(y) = col.floor_below(p.x, p.z, p.y + 1000.0) {
                        p.y = y;
                    }
                }
                guards.push((g, m, gmar, p));
            }
        }
        phase = Phase::from_manager(vm.spawns.iter().find(|s| s.kind == guard::MANAGER_ACTOR));
        eprintln!("script created {} actors, {} guards", vm.spawns.len(), guards.len());
        if cli.spawns {
            for s in &vm.spawns {
                eprintln!("  {:06x} {:?} {:?}", s.kind, s.args, s.options);
            }
        }
    }

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "MGS2 Tanker".into(), ..default() }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.02, 0.03, 0.05)))
    .insert_resource(Game { data, collision, nav, show_collision: false, show_vision: cli.vision })
    .insert_resource(bullet::Rng(0x2001_1113))
    .insert_resource(Start {
        difficulty: cli.difficulty,
        snake_start: snake.map(|s| (s.2, s.3)),
        caution_time: phase.caution_time,
        cast: std::sync::Mutex::new(Some(Cast { snake, guards })),
        cam_yaw: cli.cam_yaw.to_radians(), cam_dist: cli.cam_dist, cam_pitch: cli.cam_pitch.to_radians(), follow_guard: cli.follow_guard,
    })
    .insert_resource(phase)
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            player::control,
            guard::think,
            bullet::fly,
            game_over,
            body::skin,
            player::follow_camera,
            toggles,
            draw_collision,
            guard::draw,
            bullet::draw,
            hud,
        )
            .chain(),
    );
    if cli.trace {
        app.insert_resource(guard::Trace::default()).add_systems(Update, guard::trace);
    }
    if let Some(s) = cli.auto_walk {
        app.insert_resource(AutoWalk { from: cli.walk_from, seconds: s });
    }
    if let Some(path) = cli.screenshot {
        app.insert_resource(Capture { path, at: cli.shot_at, taken: false }).add_systems(Last, capture);
    }
    app.run();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    game: Res<Game>,
    start: Res<Start>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut cache = MaterialCache::default();
    let unlit = true;
    let data = &game.data;
    scene::spawn_level(&mut commands, data, &mut cache, &mut meshes, &mut images, &mut materials, unlit, ());
    let Some(cast) = start.cast.lock().unwrap().take() else { return };
    let mut spawn = |commands: &mut Commands, model: usize, bundle: Body| {
        scene::spawn_model(
            commands,
            &data.models[model].1,
            data,
            &mut cache,
            &mut meshes,
            &mut images,
            &mut materials,
            unlit,
            bundle,
        )
    };
    match cast.snake {
        Some((model, motions, pos, yaw)) => {
            let e = spawn(&mut commands, model, Body { pos, yaw, model, motions, motion: 0, frame: 0.0 });
            commands.entity(e).insert(Player::new(start.difficulty));
        }
        None => error!("Snake's model (sna_def) not found"),
    }
    for (g, model, motions, pos) in cast.guards {
        let e = spawn(&mut commands, model, Body { pos, yaw: 0.0, model, motions, motion: 0, frame: 0.0 });
        commands.entity(e).insert(g);
    }
    commands.spawn((
        Camera3d::default(),
        Transform::default(),
        FollowCam { yaw: start.cam_yaw, pitch: start.cam_pitch, distance: start.cam_dist, current: start.cam_dist, guard: start.follow_guard },
    ));
    commands.insert_resource(GlobalAmbientLight { brightness: 400.0, ..default() });
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(14.0), ..default() },
        Node { position_type: PositionType::Absolute, top: px(8), left: px(8), ..default() },
    ));
}

fn toggles(keys: Res<ButtonInput<KeyCode>>, mut game: ResMut<Game>) {
    if keys.just_pressed(KeyCode::F1) {
        game.show_collision = !game.show_collision;
    }
    if keys.just_pressed(KeyCode::F2) {
        game.show_vision = !game.show_vision;
    }
}

fn draw_collision(game: Res<Game>, mut gizmos: Gizmos) {
    if !game.show_collision {
        return;
    }
    let Some(h) = &game.data.collision else { return };
    let p = |a: [f32; 3]| Vec3::from(a) * SCALE;
    for g in h.areas.iter().flat_map(|a| &a.groups) {
        for w in &g.walls {
            for pair in w.points.windows(2) {
                let ((a, ha), (b, hb)) = (pair[0], pair[1]);
                let (a, b) = (p(a), p(b));
                gizmos.line(a, b, Color::srgb(1.0, 0.2, 0.15));
                gizmos.line(a + Vec3::Y * ha * SCALE, b + Vec3::Y * hb * SCALE, Color::srgb(1.0, 0.2, 0.15));
            }
        }
        for poly in g.floors.iter().flat_map(|f| &f.polygons) {
            let n = poly.vertices.len();
            for i in 0..n {
                gizmos.line(p(poly.vertices[i]), p(poly.vertices[(i + 1) % n]), Color::srgb(0.2, 1.0, 0.3));
            }
        }
    }
    for r in &h.routes {
        for pair in r.points.windows(2) {
            gizmos.line(p(pair[0].position), p(pair[1].position), Color::srgb(0.9, 0.5, 1.0));
        }
    }
}

/// After a game over, Enter continues from the stage start with full life
/// and the guards back on patrol.
#[allow(clippy::type_complexity)]
fn game_over(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    start: Res<Start>,
    mut phase: ResMut<Phase>,
    mut player: Query<(&mut Player, &mut Body), Without<Guard>>,
    mut guards: Query<&mut Guard>,
    bullets: Query<Entity, With<bullet::Bullet>>,
) {
    let Ok((mut p, mut body)) = player.single_mut() else { return };
    if !p.dead() || !keys.just_pressed(KeyCode::Enter) {
        return;
    }
    *p = Player::new(start.difficulty);
    if let Some((pos, yaw)) = start.snake_start {
        body.pos = pos;
        body.yaw = yaw;
    }
    *phase = Phase::default();
    phase.caution_time = start.caution_time;
    for mut g in &mut guards {
        g.reset();
    }
    for e in &bullets {
        commands.entity(e).despawn();
    }
}

fn hud(
    game: Res<Game>,
    phase: Res<Phase>,
    player: Query<(&Body, &Player)>,
    guards: Query<&Guard>,
    mut text: Query<&mut Text, With<Hud>>,
) {
    let (Ok((b, p)), Ok(mut t)) = (player.single(), text.single_mut()) else { return };
    let status = match phase.kind {
        guard::PhaseKind::Alert => format!("ALERT  (meter {})", phase.meter),
        guard::PhaseKind::Evasion => format!("EVASION {:.1}", phase.evasion as f32 / 60.0),
        guard::PhaseKind::Caution => format!("CAUTION {:.1}", phase.caution as f32 / 60.0),
        guard::PhaseKind::Infiltration => "INFILTRATION".into(),
    };
    let bar = |n: i32, max: i32| {
        let full = (n.max(0) * 20 + max - 1) / max.max(1);
        format!("[{}{}]", "#".repeat(full as usize), "-".repeat(20 - full as usize))
    };
    let life = if p.dead() {
        "LIFE [--------------------]  GAME OVER - press Enter to continue".to_string()
    } else {
        format!("LIFE {} {}/{}{}", bar(p.life, p.max_life), p.life, p.max_life, if p.hurt > 0.0 { "  HIT!" } else { "" })
    };
    **t = format!(
        "{}  {}  guards {}\n{}\npos {:.0} {:.0} {:.0} mm  motion {}{}\n\
         WASD move  Shift walk  C crouch  Q/E or RMB camera  F1 collision+routes  F2 vision cones",
        game.data.name,
        status,
        guards.iter().count(),
        life,
        b.pos.x,
        b.pos.y,
        b.pos.z,
        b.motion,
        if p.crouch { "  (crouching)" } else { "" },
    );
}

fn capture(mut commands: Commands, time: Res<Time>, mut c: ResMut<Capture>, mut exit: MessageWriter<AppExit>) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let t = time.elapsed_secs();
    if !c.taken && t > c.at {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(c.path.clone()));
        c.taken = true;
    }
    if t > c.at + 2.0 {
        exit.write(AppExit::Success);
    }
}





