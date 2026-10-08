//! Metal Gear Solid 2: the Tanker chapter, rebuilt on Bevy from the data on
//! your own disc.
//!
//! Controls: WASD move (run), Shift walk, C crouch, Space action (doors),
//! Q/E or right mouse drag to turn the camera, F1 collision overlay, F2
//! guard vision cones.

mod body;
mod bullet;
mod collision;
mod gcl;
mod guard;
mod nav;
mod player;

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use bevy::prelude::*;
use clap::Parser;
use mgs2_bevy::scene::{self, MaterialCache, SCALE};
use mgs2_bevy::stage::{self, Disc, StageData};
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
    /// Open the n-th door of the stage after `--door-at` seconds (testing).
    #[arg(long)]
    use_door: Option<usize>,
    #[arg(long, default_value_t = 3.0)]
    door_at: f32,
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
    /// Script variables as they stood after the stage script ran.
    pub banks: gcl::Banks,
    pub doors: Vec<Door>,
    /// Snake's start, for continuing after a game over.
    pub snake_start: (Vec3, f32),
}

/// A door actor (`chara 0xebec24`): `-h` gives the two handle points and
/// `-O` the proc run once it is opened (stage exits load the next stage
/// from there).
#[derive(Clone, Debug)]
pub struct Door {
    pub a: Vec3,
    pub b: Vec3,
    pub on_open: u32,
}

const DOOR_ACTOR: u32 = 0xEBEC24;
const PLAYER_ACTOR: u32 = 0x128946;
/// How close (mm) Snake must be to a door's handle line to open it.
const DOOR_REACH: f32 = 900.0;

/// Characters to spawn for a freshly loaded stage.
struct Cast {
    snake: Option<(usize, Option<usize>, Vec3, f32)>,
    guards: Vec<(Guard, usize, Option<usize>, Vec3)>,
}

/// A stage ready to be put into the world.
struct Level {
    game: Game,
    phase: Phase,
    cast: Cast,
}

/// Everything that belongs to the current stage; despawned on a change.
#[derive(Component, Clone)]
struct LevelEntity;

/// Set while a newly loaded stage waits to be spawned.
#[derive(Resource)]
struct NewCast(Mutex<Option<Cast>>);

#[derive(Resource)]
struct Session {
    disc: Mutex<Disc>,
    difficulty: i16,
    print_spawns: bool,
    show_vision: bool,
    use_door: Option<(usize, f32)>,
}

#[derive(Resource)]
struct Start {
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

fn set_snake_start(banks: &mut gcl::Banks, pos: [f32; 3], dir: i32) {
    for (off, v) in [(gcl::SNAKE_X, pos[0] as i32), (gcl::SNAKE_Y, pos[1] as i32), (gcl::SNAKE_Z, pos[2] as i32), (gcl::SNAKE_DIR, dir)] {
        banks.set_a32(off, v);
    }
}

/// Loads a stage and runs its script with `banks` (which carry Snake's
/// start position and the story state), then works out the cast.
fn prepare(disc: &mut Disc, name: &str, banks: gcl::Banks, print_spawns: bool, show_vision: bool) -> Result<Level> {
    let data = stage::load(disc, name)?;
    let collision = data.collision.as_ref().map(Collision::new);
    let nav = data.collision.as_ref().map(nav::Nav::new);

    let mut guards = Vec::new();
    let mut phase = Phase::default();
    let mut doors = Vec::new();
    let mut banks_after = banks.clone();
    // Snake's start: the player actor's `-p x y z -d dir`, which read the
    // start variables; else the start variables themselves.
    let a32 = |o: u16| banks.a32(o);
    let mut start = (
        Vec3::new(a32(gcl::SNAKE_X) as f32, a32(gcl::SNAKE_Y) as f32, a32(gcl::SNAKE_Z) as f32),
        a32(gcl::SNAKE_DIR),
    );
    if let Some(script) = &data.script {
        let mut vm = gcl::Vm::with_banks(script, banks);
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
        for s in vm.spawns.iter().filter(|s| s.kind == DOOR_ACTOR) {
            let h = |i| s.opt_int(b'h', i).unwrap_or(0) as f32;
            if let Some(gcl::Val::Proc(p)) = s.opt(b'O').and_then(|v| v.first()) {
                doors.push(Door { a: Vec3::new(h(0), h(1), h(2)), b: Vec3::new(h(3), h(4), h(5)), on_open: *p });
            }
        }
        if let Some(p) = vm.spawns.iter().find(|s| s.kind == PLAYER_ACTOR) {
            let (x, y, z) = (p.opt_int(b'p', 0), p.opt_int(b'p', 1), p.opt_int(b'p', 2));
            if let (Some(x), Some(y), Some(z)) = (x, y, z) {
                start = (Vec3::new(x as f32, y as f32, z as f32), p.opt_int(b'd', 1).unwrap_or(start.1));
            }
        }
        phase = Phase::from_manager(vm.spawns.iter().find(|s| s.kind == guard::MANAGER_ACTOR));
        eprintln!("{name}: script created {} actors, {} guards, {} doors", vm.spawns.len(), guards.len(), doors.len());
        if print_spawns {
            for s in &vm.spawns {
                eprintln!("  {:06x} {:?} {:?}", s.kind, s.args, s.options);
            }
        }
        banks_after = vm.banks();
    }

    let mut pos = start.0;
    if let Some(col) = &collision {
        if let Some(y) = col.floor_below(pos.x, pos.z, pos.y + 2000.0) {
            pos.y = y;
        }
    }
    let yaw = start.1 as f32 / 4096.0 * std::f32::consts::TAU;
    let snake = model_index(&data, "sna_def").map(|m| (m, motions_index(&data, "snake"), pos, yaw));
    Ok(Level {
        game: Game {
            data,
            collision,
            nav,
            show_collision: false,
            show_vision,
            banks: banks_after,
            doors,
            snake_start: (pos, yaw),
        },
        phase,
        cast: Cast { snake, guards },
    })
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let iso = cli.iso.or_else(stage::default_iso).context("pass --iso <path to MGS2 .iso> or set MGS2_ISO")?;
    let mut disc = stage::open_disc(&iso)?;

    // Snake's start: how the previous stage sends him here, else one of this
    // stage's own entry points.
    let mut banks = gcl::Banks::default();
    banks.set_b16(gcl::DIFFICULTY, cli.difficulty);
    // The title screen's script opens with the per-difficulty settings
    // (guard vision ranges As16:0x1bc.., timers); run just that statement.
    if let Some(title) = stage::script(&mut disc, "n_title")? {
        if let Some(first) = title.main.first() {
            let mut vm = gcl::Vm::with_banks(&title, banks);
            vm.run(std::slice::from_ref(first));
            banks = vm.banks();
        }
    }
    let from = cli.from.clone().or_else(|| (cli.stage == "w00a").then(|| "d01t".to_string()));
    let arrival = match from.as_deref() {
        Some("none") | None => None,
        Some(f) => stage::arrivals(&mut disc, f, &cli.stage)?.get(cli.arrival).copied(),
    };
    if let Some(p) = arrival.and_then(|a| a.progress) {
        banks.set_a32(gcl::PROGRESS, p as i32);
    }
    let entry = match arrival {
        Some(a) => Some(a.entry),
        // The stage's own entries need its data; peek at them first.
        None => stage::load(&mut disc, &cli.stage)?.entries.get(cli.entry).copied(),
    };
    if let Some(e) = entry {
        set_snake_start(&mut banks, e.position, e.direction);
    }
    if let Some(at) = cli.snake_at.as_ref().filter(|a| a.len() == 3) {
        set_snake_start(&mut banks, [at[0], at[1], at[2]], entry.map_or(0, |e| e.direction));
    }
    let level = prepare(&mut disc, &cli.stage, banks, cli.spawns, cli.vision)?;

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "MGS2 Tanker".into(), ..default() }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.02, 0.03, 0.05)))
    .insert_resource(level.game)
    .insert_resource(level.phase)
    .insert_resource(NewCast(Mutex::new(Some(level.cast))))
    .insert_resource(bullet::Rng(0x2001_1113))
    .insert_resource(Session {
        disc: Mutex::new(disc),
        difficulty: cli.difficulty,
        print_spawns: cli.spawns,
        show_vision: cli.vision,
        use_door: cli.use_door.map(|d| (d, cli.door_at)),
    })
    .insert_resource(Start {
        cam_yaw: cli.cam_yaw.to_radians(),
        cam_dist: cli.cam_dist,
        cam_pitch: cli.cam_pitch.to_radians(),
        follow_guard: cli.follow_guard,
    })
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            spawn_level.run_if(resource_exists::<NewCast>),
            player::control,
            guard::think,
            bullet::fly,
            game_over,
            body::skin,
            player::follow_camera,
            toggles,
            doors,
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

/// Camera, light and HUD: these live across stages.
fn setup(mut commands: Commands, start: Res<Start>) {
    commands.spawn((
        Camera3d::default(),
        Transform::default(),
        FollowCam {
            yaw: start.cam_yaw,
            pitch: start.cam_pitch,
            distance: start.cam_dist,
            current: start.cam_dist,
            guard: start.follow_guard,
        },
    ));
    commands.insert_resource(GlobalAmbientLight { brightness: 400.0, ..default() });
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(14.0), ..default() },
        Node { position_type: PositionType::Absolute, top: px(8), left: px(8), ..default() },
    ));
}

/// Puts the current stage's level geometry and cast into the world.
#[allow(clippy::too_many_arguments)]
fn spawn_level(
    mut commands: Commands,
    game: Res<Game>,
    session: Res<Session>,
    new: Res<NewCast>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cams: Query<&mut FollowCam>,
    mut first: Local<bool>,
) {
    let cast = new.0.lock().unwrap().take();
    // After a stage change, look over Snake's shoulder the way he faces.
    if std::mem::replace(&mut *first, true) {
        if let (Some((_, _, _, yaw)), Ok(mut cam)) = (cast.as_ref().and_then(|c| c.snake), cams.single_mut()) {
            cam.yaw = yaw + std::f32::consts::PI;
        }
    }
    commands.remove_resource::<NewCast>();
    let Some(cast) = cast else { return };
    let mut cache = MaterialCache::default();
    let unlit = true;
    let data = &game.data;
    scene::spawn_level(&mut commands, data, &mut cache, &mut meshes, &mut images, &mut materials, unlit, LevelEntity);
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
            (bundle, LevelEntity),
        )
    };
    match cast.snake {
        Some((model, motions, pos, yaw)) => {
            let e = spawn(&mut commands, model, Body { pos, yaw, model, motions, motion: 0, frame: 0.0 });
            commands.entity(e).insert(Player::new(session.difficulty));
        }
        None => error!("Snake's model (sna_def) not found"),
    }
    for (g, model, motions, pos) in cast.guards {
        let e = spawn(&mut commands, model, Body { pos, yaw: 0.0, model, motions, motion: 0, frame: 0.0 });
        commands.entity(e).insert(g);
    }
}

/// The door Snake can open from where he stands, if any.
fn door_in_reach(game: &Game, snake: Vec3) -> Option<usize> {
    game.doors.iter().position(|d| {
        let (a, b, p) = (d.a.xz(), d.b.xz(), snake.xz());
        let ab = b - a;
        let t = if ab.length_squared() > 1.0 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
        (a + ab * t).distance(p) < DOOR_REACH && (snake.y - d.a.y).abs() < 1000.0
    })
}

/// Space at a door runs its `-O` proc like the game does once the door is
/// open; a stage exit there sets Snake's next start and `load`s the next
/// stage, which replaces the current one.
#[allow(clippy::too_many_arguments)]
fn doors(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<Session>,
    mut game: ResMut<Game>,
    player: Query<(&Body, &Player)>,
    level: Query<Entity, (With<LevelEntity>, Without<ChildOf>)>,
    bullets: Query<Entity, With<bullet::Bullet>>,
) {
    let Ok((body, p)) = player.single() else { return };
    if p.dead() {
        return;
    }
    let forced = match session.use_door {
        Some((d, at)) if time.elapsed_secs() >= at => {
            session.use_door = None;
            Some(d)
        }
        _ => None,
    };
    let door = match forced {
        Some(d) => Some(d).filter(|d| *d < game.doors.len()),
        None if keys.just_pressed(KeyCode::Space) => door_in_reach(&game, body.pos),
        None => None,
    };
    let Some(door) = door else { return };
    let Some(script) = &game.data.script else { return };
    let mut vm = gcl::Vm::with_banks(script, game.banks.clone());
    vm.eager = true;
    vm.call_proc(game.doors[door].on_open, &[0]);
    let banks = vm.banks();
    let Some(next) = vm.load.clone() else {
        info!("door {door}: opened (no stage change)");
        game.banks = banks;
        return;
    };
    let next = match next.strip_prefix('#') {
        // A hashed name: find the stage it stands for.
        Some(h) => {
            let h = u32::from_str_radix(h, 16).unwrap_or(0);
            let disc = session.disc.lock().unwrap();
            stage::tanker_stages(&disc).into_iter().find(|s| str_code(s.as_bytes()) == h).unwrap_or(next)
        }
        None => next,
    };
    info!("door {door}: loading {next}");
    let level_result = {
        let mut disc = session.disc.lock().unwrap();
        prepare(&mut disc, &next, banks, session.print_spawns, session.show_vision)
    };
    let new = match level_result {
        Ok(l) => l,
        Err(e) => {
            error!("loading {next}: {e:#}");
            return;
        }
    };
    for e in level.iter().chain(bullets.iter()) {
        commands.entity(e).despawn();
    }
    *game = new.game;
    commands.insert_resource(new.phase);
    commands.insert_resource(NewCast(Mutex::new(Some(new.cast))));
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
    for d in &game.doors {
        gizmos.line(d.a * SCALE + Vec3::Y, d.b * SCALE + Vec3::Y, Color::srgb(0.2, 1.0, 1.0));
    }
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
    session: Res<Session>,
    game: Res<Game>,
    mut phase: ResMut<Phase>,
    mut player: Query<(&mut Player, &mut Body), Without<Guard>>,
    mut guards: Query<&mut Guard>,
    bullets: Query<Entity, With<bullet::Bullet>>,
) {
    let Ok((mut p, mut body)) = player.single_mut() else { return };
    if !p.dead() || !keys.just_pressed(KeyCode::Enter) {
        return;
    }
    *p = Player::new(session.difficulty);
    (body.pos, body.yaw) = game.snake_start;
    let caution_time = phase.caution_time;
    *phase = Phase::default();
    phase.caution_time = caution_time;
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
    let prompt = if door_in_reach(&game, b.pos).is_some() { "  [Space] open door" } else { "" };
    **t = format!(
        "{}  {}  guards {}\n{}{}\npos {:.0} {:.0} {:.0} mm  motion {}{}\n\
         WASD move  Shift walk  C crouch  Space action  Q/E or RMB camera  F1 collision+routes  F2 vision cones",
        game.data.name,
        status,
        guards.iter().count(),
        life,
        prompt,
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
