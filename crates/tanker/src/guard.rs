//! Gurlukovich soldiers: the `chara 0xf7f777` actor, ported from the game's
//! AI (w00a resident-image addresses; see `re/notes/guard-ai.md`).
//!
//! Faithful to the decompiled code:
//! - perception `FUN_002559C0`: near range `d` (`-i`), far range 1.5d,
//!   ±0x200 (45°) horizontally around the head direction and ±0x200
//!   vertically, line of sight; level 3 "seen" inside d, 2 "glimpse"
//!   between d and 1.5d, 1 "blocked"; a 250 x 1500 x 250 mm box around the
//!   guard counts as seen ("bump");
//! - suspicion meter `FUN_00256488` (0..0x400);
//! - normal-phase transitions `FUN_00227520`/`FUN_00227430`: meter > 0
//!   investigates, meter >= 2 or seen starts discovery, investigation
//!   escalates at meter > 8; investigation looks around for up to 300 frames;
//! - discovery `FUN_00238EB8`/`FUN_00237128`/`FUN_002381E8`: turn to face
//!   Snake during the "!" action, then set the meter to 0x400;
//! - alert brain `FUN_00229D30`: seen -> meter 0x400; the tracking flag
//!   expires 60 frames after the last sighting;
//! - phases (`Phase`) from the enemy manager `FUN_00220818`/`FUN_00220430`.
//!
//! Simplified for now: one "!" duration for every guard (the action
//! animation length is not read yet), close-quarters sub-states, shooting,
//! radio reports, noise/body/footprint stimuli, and the evasion search
//! pattern (guards visit nav nodes around the last known position).

use std::f32::consts::TAU;

use bevy::prelude::*;
use mgs2_formats::hzx::Waypoint;

use crate::Game;
use crate::body::{Body, turn_toward};
use crate::gcl::Spawn;
use crate::player::{HEIGHT, Player, RADIUS, STEP_UP};

pub const GUARD_ACTOR: u32 = 0xF7F777;
/// The enemy manager (`cmd_ddf5ca`): phases and their timers.
pub const MANAGER_ACTOR: u32 = 0xDDF5CA;

mod motion {
    pub const IDLE: usize = 0;
    pub const WALK: usize = 1;
    pub const RUN: usize = 2;
}

const WALK_SPEED: f32 = 1330.0;
const RUN_SPEED: f32 = 4050.0;
const TURN_RATE: f32 = TAU;
/// Eye height above the feet (head joint position, approximate).
const EYE: f32 = 1600.0;
/// 4096 units per turn.
const ANGLE: f32 = TAU / 4096.0;
/// Frames the "!" reaction lasts (stand-in for the action's animation).
const EXCLAIM_FRAMES: u32 = 45;
const METER_MAX: i16 = 0x400;

/// Global phase, `Bs16:0x112`, as run by the enemy manager.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PhaseKind {
    #[default]
    Infiltration = 0,
    Alert = 1,
    Evasion = 2,
    Caution = 3,
}

#[derive(Resource, Debug)]
pub struct Phase {
    pub kind: PhaseKind,
    /// Manager +0x1C: follows the highest guard meter, decays 16/frame.
    pub meter: i32,
    /// Evasion countdown (manager +0x38), frames.
    pub evasion: i32,
    /// Caution countdown (manager +0x24), frames.
    pub caution: i32,
    /// `-h` of the manager actor (default 0xE10 = 60 s).
    pub caution_time: i32,
    pub last_seen: Option<Vec3>,
    changed: bool,
}

impl Default for Phase {
    fn default() -> Self {
        Self {
            kind: PhaseKind::Infiltration,
            meter: 0,
            evasion: 0,
            caution: 0,
            caution_time: 0xE10,
            last_seen: None,
            changed: false,
        }
    }
}

impl Phase {
    pub fn from_manager(s: Option<&Spawn>) -> Self {
        let mut p = Self::default();
        if let Some(s) = s {
            p.caution_time = s.opt_int(b'h', 0).unwrap_or(0xE10);
        }
        p
    }

    fn set(&mut self, k: PhaseKind) {
        if self.kind != k {
            self.kind = k;
            self.changed = true;
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Vision {
    /// Near range `d` (controller +0x248).
    pub distance: f32,
    /// Half angle in 4096-units (controller +0x244).
    pub half_angle: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Normal 0: walking the route.
    Patrol,
    /// Waiting at a route point (frames left; infinite = -1 wait).
    Wait(f32),
    /// Normal 6: go and look (target, sub-state, frame counter).
    Investigate { target: Vec3, sub: u8, timer: u32 },
    /// Normal 8: discovery ("!"), frame counter.
    Discovery { timer: u32 },
    /// Alert brain: chase.
    Chase,
    /// Evasion brain: search near the last known position (frames to wait).
    Search { wait: u32 },
    /// Going back to the route after alert/evasion/investigation.
    Return,
}

#[derive(Component)]
pub struct Guard {
    pub route: usize,
    pub node: usize,
    pub state: State,
    pub vision: Vision,
    /// Perception result (controller +0x18E).
    pub level: u8,
    /// Suspicion meter (controller +0x274).
    pub meter: i16,
    /// Tracking flag (+0x348 & 0x40) and its timer (+0x33C).
    tracking: Option<u32>,
    /// Last position Snake was seen at (+0x200).
    pub last_seen: Option<Vec3>,
    path: Vec<Vec3>,
    repath: u32,
}

/// Builds a guard from its script spawn.
pub fn from_spawn(s: &Spawn, default_vision: f32) -> Option<(Guard, Vec3)> {
    let route = s.opt_int(b'r', 0)? as usize;
    let node = s.opt_int(b'n', 0).unwrap_or(0).max(0) as usize;
    let pos = Vec3::new(
        s.opt_int(b'd', 0).unwrap_or(0) as f32,
        s.opt_int(b'd', 1).unwrap_or(0) as f32,
        s.opt_int(b'd', 2).unwrap_or(0) as f32,
    );
    // `-i` d0 [d1 d2 d3]; FUN_00222F60 defaults 6000; vision uses d0.
    let distance = s.opt_int(b'i', 0).filter(|d| *d > 0).map_or(default_vision, |d| d as f32);
    let guard = Guard {
        route,
        node,
        state: State::Patrol,
        vision: Vision { distance, half_angle: 0x200 },
        level: 0,
        meter: 0,
        tracking: None,
        last_seen: None,
        path: Vec::new(),
        repath: 0,
    };
    Some((guard, pos))
}

/// Signed difference of two 4096-unit angles (`FUN_001413B8`).
fn angle_diff(a: i32, b: i32) -> i32 {
    let d = (b - a) & 0xFFF;
    if d > 0x800 { d - 0x1000 } else { d }
}

fn to_units(rad: f32) -> i32 {
    ((rad / ANGLE).round() as i32) & 0xFFF
}

/// `FUN_002559C0` for the target Snake. Returns the perception level.
fn perceive(g: &Guard, body: &Body, snake: Vec3, game: &Game) -> u8 {
    let to = snake - body.pos;
    let dist = to.length();
    let (near, far) = (g.vision.distance, g.vision.distance * 1.5);
    if far < dist {
        return 0;
    }
    let eye = body.pos + Vec3::Y * EYE;
    let target = snake + Vec3::Y * (HEIGHT * 0.6);
    let blocked = || game.collision.as_ref().is_some_and(|c| c.line_blocked(eye, target));
    // FUN_00256C08(self, target, 250, 1500, 250): bumping into the guard.
    if to.x.abs() <= 250.0 && to.z.abs() <= 250.0 && to.y.abs() <= 1500.0 {
        return if blocked() { 1 } else { 3 };
    }
    // Horizontal: head direction vs direction to the target.
    let dir = to_units(to.x.atan2(to.z));
    let head = to_units(body.yaw);
    if angle_diff(head, dir).abs() > g.vision.half_angle {
        return 0;
    }
    // Vertical (FUN_0022DD78 minus 0x400, head pitch 0): within ±0x200.
    let rel = target - eye;
    let pitch = to_units(Vec2::new(rel.x, rel.z).length().atan2(rel.y)) - 0x400;
    if pitch.abs() > 0x200 {
        return 0;
    }
    if blocked() {
        1
    } else if dist <= near {
        3
    } else {
        2
    }
}

/// `FUN_00256488`.
fn update_meter(g: &mut Guard, phase: PhaseKind) {
    let m = g.meter;
    g.meter = match g.level {
        2 if phase == PhaseKind::Alert => m - 1,
        2 => m.max(1),
        3 => m + 1,
        _ if g.tracking.is_some() => m,
        _ => m - 16,
    }
    .clamp(0, METER_MAX);
}

/// Moves along `g.path` toward its end; returns true on arrival.
fn follow(g: &mut Guard, body: &mut Body, speed: f32, reach: f32, dt: f32, game: &Game) -> bool {
    while let Some(&next) = g.path.first() {
        let last = g.path.len() == 1;
        let flat = Vec2::new(next.x - body.pos.x, next.z - body.pos.z);
        if flat.length() <= if last { reach } else { 400.0 } {
            g.path.remove(0);
            continue;
        }
        body.yaw = turn_toward(body.yaw, flat.x.atan2(flat.y), TURN_RATE * dt);
        let step = (speed * dt).min(flat.length());
        let mut p = body.pos + body.forward() * step;
        if let Some(col) = &game.collision {
            p = col.push_out(p, RADIUS, HEIGHT, STEP_UP);
            match col.floor_below(p.x, p.z, p.y + STEP_UP) {
                Some(y) => p.y = y,
                None => {
                    g.path.remove(0);
                    continue;
                }
            }
        }
        body.pos = p;
        return false;
    }
    true
}

fn plan(g: &mut Guard, from: Vec3, to: Vec3, game: &Game) {
    g.path = match &game.nav {
        Some(nav) if !nav.is_empty() => nav.path(from, to),
        _ => vec![to],
    };
    g.repath = 30;
}

fn waypoint(game: &Game, route: usize, node: usize) -> Option<Waypoint> {
    let r = game.data.collision.as_ref()?.routes.get(route)?;
    r.points.get(node % r.points.len().max(1)).copied()
}

fn face(body: &mut Body, target: Vec3, dt: f32) {
    let to = target - body.pos;
    body.yaw = turn_toward(body.yaw, to.x.atan2(to.z), TURN_RATE * dt);
}

pub fn think(
    time: Res<Time>,
    game: Res<Game>,
    mut phase: ResMut<Phase>,
    snake: Query<&Body, (With<Player>, Without<Guard>)>,
    mut guards: Query<(&mut Guard, &mut Body)>,
) {
    let dt = time.delta_secs().min(0.05);
    let Ok(snake_body) = snake.single() else { return };
    let snake_pos = snake_body.pos;
    let changed = std::mem::take(&mut phase.changed);
    let mut highest = 0i32;

    for (mut g, mut body) in &mut guards {
        // Sensing (FUN_00222978): perception, then the meter.
        g.level = perceive(&g, &body, snake_pos, &game);
        if g.level == 3 {
            g.last_seen = Some(snake_pos);
            phase.last_seen = Some(snake_pos);
            g.tracking = Some(0);
        }
        update_meter(&mut g, phase.kind);

        // Brain switches with the phase (inits FUN_00227E00/229F98/22C978).
        if changed {
            match phase.kind {
                PhaseKind::Alert => g.state = State::Chase,
                PhaseKind::Evasion => {
                    let to = phase.last_seen.unwrap_or(snake_pos);
                    plan(&mut g, body.pos, to, &game);
                    g.state = State::Search { wait: 0 };
                }
                PhaseKind::Caution | PhaseKind::Infiltration => {
                    if matches!(g.state, State::Chase | State::Search { .. }) {
                        if let Some(wp) = waypoint(&game, g.route, g.node) {
                            plan(&mut g, body.pos, Vec3::from(wp.position), &game);
                        }
                        g.state = State::Return;
                    }
                }
            }
        }

        match phase.kind {
            PhaseKind::Alert => alert_brain(&mut g, &mut body, snake_pos, dt, &game),
            PhaseKind::Evasion => evasion_brain(&mut g, &mut body, &phase, dt, &game),
            _ => normal_brain(&mut g, &mut body, snake_pos, dt, &game),
        }
        highest = highest.max(i32::from(g.meter));
    }

    manager(&mut phase, highest);
}

/// `FUN_00227BC8` + transitions `FUN_00227520` (states the game can
/// currently produce).
fn normal_brain(g: &mut Guard, body: &mut Body, snake: Vec3, dt: f32, game: &Game) {
    // Transitions out of patrol/wait/return, in the game's priority order
    // (later checks override earlier ones).
    if matches!(g.state, State::Patrol | State::Wait(_) | State::Return) {
        if g.meter > 0 {
            let target = g.last_seen.unwrap_or(snake);
            g.state = State::Investigate { target, sub: 0, timer: 0 };
        }
        if g.meter >= 2 || g.level == 3 {
            g.state = State::Discovery { timer: 0 };
        }
    }
    // Investigation escalates (FUN_00227430).
    if matches!(g.state, State::Investigate { .. }) && g.meter > 8 {
        g.state = State::Discovery { timer: 0 };
    }

    match g.state.clone() {
        State::Patrol => {
            let Some(wp) = waypoint(game, g.route, g.node) else {
                body.play(motion::IDLE);
                return;
            };
            body.play(motion::WALK);
            if g.path.is_empty() {
                g.path = vec![Vec3::from(wp.position)];
            }
            if follow(g, body, WALK_SPEED, 150.0, dt, game) {
                g.state = match wp.wait {
                    0 => {
                        g.node += 1;
                        State::Patrol
                    }
                    w if w < 0 => State::Wait(f32::INFINITY),
                    w => State::Wait(f32::from(w) / 60.0),
                };
            }
        }
        State::Wait(left) => {
            body.play(motion::IDLE);
            if let Some(wp) = waypoint(game, g.route, g.node) {
                let f = match wp.look_at {
                    Some(l) => (l[0] - body.pos.x).atan2(l[2] - body.pos.z),
                    None => f32::from(wp.direction) * ANGLE,
                };
                body.yaw = turn_toward(body.yaw, f, TURN_RATE * dt);
            }
            let left = left - dt;
            g.state = if left <= 0.0 {
                g.node += 1;
                State::Patrol
            } else {
                State::Wait(left)
            };
        }
        State::Investigate { target, sub, timer } => {
            // Sub-states: 0 glance toward the target, 1 walk there,
            // 2 look around (sub-state 10: up to 300 frames), then return.
            let timer = timer + 1;
            g.state = match sub {
                0 => {
                    body.play(motion::IDLE);
                    face(body, target, dt);
                    if timer > 30 {
                        plan(g, body.pos, target, game);
                        State::Investigate { target, sub: 1, timer: 0 }
                    } else {
                        State::Investigate { target, sub, timer }
                    }
                }
                1 => {
                    body.play(motion::WALK);
                    if follow(g, body, WALK_SPEED, 600.0, dt, game) || timer > 600 {
                        State::Investigate { target, sub: 2, timer: 0 }
                    } else {
                        State::Investigate { target, sub, timer }
                    }
                }
                _ => {
                    body.play(motion::IDLE);
                    body.yaw += (timer as f32 / 60.0).sin() * dt;
                    if timer > 300 {
                        g.last_seen = None;
                        if let Some(wp) = waypoint(game, g.route, g.node) {
                            plan(g, body.pos, Vec3::from(wp.position), game);
                        }
                        State::Return
                    } else {
                        State::Investigate { target, sub, timer }
                    }
                }
            };
        }
        State::Discovery { timer } => {
            // "!": face Snake while the reaction plays, then the meter goes
            // to 0x400, which raises the alert through the manager.
            body.play(motion::IDLE);
            face(body, snake, dt);
            let timer = timer + 1;
            if timer >= EXCLAIM_FRAMES {
                g.meter = METER_MAX;
            }
            g.state = State::Discovery { timer };
        }
        State::Return => {
            body.play(motion::WALK);
            if follow(g, body, WALK_SPEED, 300.0, dt, game) {
                g.path.clear();
                g.state = State::Patrol;
            }
        }
        State::Chase | State::Search { .. } => g.state = State::Return,
    }
}

/// `FUN_00229D30` (chase part).
fn alert_brain(g: &mut Guard, body: &mut Body, snake: Vec3, dt: f32, game: &Game) {
    if g.level == 3 {
        g.meter = METER_MAX;
    }
    // Tracking flag times out 60 frames after the last sighting.
    if let Some(t) = g.tracking.as_mut() {
        *t += 1;
        if *t > 60 {
            g.tracking = None;
        }
    }
    let target = if g.level == 3 { snake } else { g.last_seen.unwrap_or(snake) };
    g.repath = g.repath.saturating_sub(1);
    if g.path.is_empty() || g.repath == 0 {
        plan(g, body.pos, target, game);
    }
    let reach = if g.level == 3 { 2500.0 } else { 600.0 };
    if follow(g, body, RUN_SPEED, reach, dt, game) {
        body.play(motion::IDLE);
        face(body, target, dt);
    } else {
        body.play(motion::RUN);
    }
}

/// `FUN_0022C670` stand-in: walk to the last known position, then visit
/// nav nodes nearby, pausing to look around.
fn evasion_brain(g: &mut Guard, body: &mut Body, phase: &Phase, dt: f32, game: &Game) {
    let State::Search { wait } = g.state else {
        g.state = State::Search { wait: 0 };
        return;
    };
    if wait > 0 {
        body.play(motion::IDLE);
        body.yaw += (wait as f32 / 40.0).sin() * dt;
        g.state = State::Search { wait: wait - 1 };
        if wait == 1 {
            let around = phase.last_seen.unwrap_or(body.pos);
            if let Some(nav) = &game.nav {
                let near = nav.nodes_near(around, 8000.0);
                if !near.is_empty() {
                    let pick = near[(body.pos.x.abs() as usize + wait as usize * 7) % near.len()];
                    plan(g, body.pos, nav.node_pos(pick), game);
                }
            }
        }
        return;
    }
    body.play(motion::WALK);
    if follow(g, body, WALK_SPEED, 400.0, dt, game) {
        g.state = State::Search { wait: 120 };
    }
}

/// Enemy manager per frame: `FUN_00220430` (meter) and `FUN_00220818`
/// (phase changes).
fn manager(p: &mut Phase, highest_guard_meter: i32) {
    p.meter = (p.meter - 16).max(0).max(highest_guard_meter);
    match p.kind {
        PhaseKind::Infiltration | PhaseKind::Caution => {
            if p.kind == PhaseKind::Caution {
                p.caution -= 1;
                if p.caution < 1 {
                    p.set(PhaseKind::Infiltration);
                }
            }
            if p.meter >= 0x400 {
                p.meter = 0x400;
                p.set(PhaseKind::Alert);
            }
        }
        PhaseKind::Alert => {
            if p.meter < 1 {
                p.evasion = 0x4B0;
                p.set(PhaseKind::Evasion);
            }
        }
        PhaseKind::Evasion => {
            if p.evasion < 1 {
                p.caution = p.caution_time;
                p.set(PhaseKind::Caution);
            }
            if p.meter >= 0x400 {
                p.meter = 0x400;
                p.set(PhaseKind::Alert);
            }
            p.evasion -= 1;
        }
    }
}

/// `--trace`: log guard positions and states once a second.
#[derive(Resource, Default)]
pub struct Trace(pub f32);

pub fn trace(
    time: Res<Time>,
    mut t: ResMut<Trace>,
    phase: Res<Phase>,
    guards: Query<(&Guard, &Body)>,
    snake: Query<&Body, With<Player>>,
) {
    t.0 += time.delta_secs();
    if t.0 < 1.0 {
        return;
    }
    t.0 = 0.0;
    let s = snake.single().map(|b| b.pos).unwrap_or_default();
    eprint!(
        "t={:5.1} snake=({:.0},{:.0},{:.0}) {:?} meter={} ev={} ca={} |",
        time.elapsed_secs(),
        s.x,
        s.y,
        s.z,
        phase.kind,
        phase.meter,
        phase.evasion,
        phase.caution
    );
    for (g, b) in &guards {
        let st = match &g.state {
            State::Investigate { sub, timer, .. } => format!("Investigate{sub}/{timer}"),
            State::Discovery { timer } => format!("Discovery/{timer}"),
            State::Wait(w) => format!("Wait{w:.1}"),
            s => format!("{s:?}"),
        };
        eprint!(" r{} ({:.0},{:.0},{:.0}) {} L{} m{} |", g.route, b.pos.x, b.pos.y, b.pos.z, st, g.level, g.meter);
    }
    eprintln!();
}

/// Debug view: vision cones (near and far range) and state markers.
pub fn draw(game: Res<Game>, guards: Query<(&Guard, &Body)>, mut gizmos: Gizmos) {
    use mgs2_bevy::scene::SCALE;
    for (g, b) in &guards {
        let eye = (b.pos + Vec3::Y * EYE) * SCALE;
        let colour = match (&g.state, g.level) {
            (State::Chase | State::Discovery { .. }, _) | (_, 3) => Color::srgb(1.0, 0.1, 0.1),
            (State::Investigate { .. } | State::Search { .. }, _) | (_, 2) => Color::srgb(1.0, 0.8, 0.1),
            _ => Color::srgba(0.3, 0.8, 1.0, 0.7),
        };
        if game.show_vision {
            let half = g.vision.half_angle as f32 * ANGLE;
            for (range, alpha) in [(g.vision.distance, 1.0), (g.vision.distance * 1.5, 0.4)] {
                for side in [-1.0, 1.0] {
                    let yaw = b.yaw + side * half;
                    let end = eye + Vec3::new(yaw.sin(), 0.0, yaw.cos()) * range * SCALE;
                    gizmos.line(eye, end, colour.with_alpha(alpha));
                }
            }
        }
        if !matches!(g.state, State::Patrol | State::Wait(_) | State::Return) {
            gizmos.sphere(Isometry3d::from_translation(eye + Vec3::Y * 0.6), 0.12, colour);
        }
        if game.show_vision {
            for pair in g.path.windows(2) {
                gizmos.line(pair[0] * SCALE + Vec3::Y * 0.1, pair[1] * SCALE + Vec3::Y * 0.1, colour);
            }
        }
    }
}


