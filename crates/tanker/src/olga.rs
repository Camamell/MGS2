//! Olga Gurlukovich, the boss on the navigational deck (w00b):
//! `chara 0x7798f6 <name> -m <model> -a <motions> -p x y z -d rx ry rz -v
//! <voice ids> -h <procs>...`.
//!
//! The game's handler is `cmd_7798f6` in w00b's resident image (0x25EE10):
//! act `FUN_0025EC50` runs the step function at actor+0xF84, a pointer into
//! step tables (0x1F6280.., e.g. 0x1F6378 start, 0x1F62F0 after it); each
//! step advances it by 4, and `FUN_00267DE0` picks the next sequence. Those
//! are not ported yet: the behaviour below is a simplified stand-in (take
//! cover at her spawn point, step out to the side, aim and fire bursts,
//! duck back), with darts draining her stamina until she goes down.

use bevy::prelude::*;

use crate::body::{Body, turn_toward};
use crate::bullet::{self, Rng};
use crate::gcl::Spawn;
use crate::player::Player;

pub const OLGA_ACTOR: u32 = 0x7798F6;

/// orga.mar motions: aim 14 / fire 15 / run 4 / down 46 as her steps use them
/// (`FUN_002617E8`, `FUN_00261878`, `FUN_00260A10`, `FUN_002661E0`); the
/// crouch behind cover (24) is picked from a contact sheet.
mod motion {
    pub const COVER: usize = 24;
    pub const RUN: usize = 4;
    pub const AIM: usize = 14;
    pub const FIRE: usize = 15;
    pub const DOWN: usize = 46;
}

const STEP_OUT: f32 = 1200.0;
const RUN_SPEED: f32 = 3000.0;
const AIM_FRAMES: f32 = 25.0;
const SHOT_FRAMES: f32 = 6.0;
/// Seconds behind cover between attacks.
const COVER_TIME: f32 = 2.5;
/// Stamina lost to a dart in the body / head.
const DART_BODY: i32 = 15;
const DART_HEAD: i32 = 30;

#[derive(Clone, Copy, Debug)]
pub enum OlgaState {
    Cover(f32),
    StepOut,
    Aim(f32),
    Fire { shots: u32, cool: f32 },
    StepIn,
    Down,
}

#[derive(Component, Debug)]
pub struct Olga {
    pub stamina: i32,
    pub state: OlgaState,
    cover: Vec3,
    side: f32,
}

impl Olga {
    pub fn new(cover: Vec3) -> Self {
        Self { stamina: 100, state: OlgaState::Cover(COVER_TIME), cover, side: 1.0 }
    }

    pub fn hit(&mut self, head: bool) {
        if matches!(self.state, OlgaState::Down) {
            return;
        }
        self.stamina -= if head { DART_HEAD } else { DART_BODY };
        if self.stamina <= 0 {
            self.stamina = 0;
            self.state = OlgaState::Down;
        }
    }

    pub fn down(&self) -> bool {
        matches!(self.state, OlgaState::Down)
    }
}

/// (model hash, motion archive hash, position, facing in radians).
pub fn from_spawn(s: &Spawn) -> Option<(u32, u32, Vec3, f32)> {
    let model = s.opt(b'm')?.first()?.int()? as u32 & 0x00FF_FFFF;
    let motions = s.opt(b'a')?.first()?.int()? as u32 & 0x00FF_FFFF;
    let p = |i| s.opt_int(b'p', i).unwrap_or(0) as f32;
    let yaw = s.opt_int(b'd', 1).unwrap_or(0) as f32 / 4096.0 * std::f32::consts::TAU;
    Some((model, motions, Vec3::new(p(0), p(1), p(2)), yaw))
}

pub fn think(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: ResMut<Rng>,
    snake: Query<(&Body, &Player), Without<Olga>>,
    mut olgas: Query<(&mut Olga, &mut Body)>,
) {
    let dt = time.delta_secs().min(0.05);
    let frames = dt * 60.0;
    let Ok((sb, player)) = snake.single() else { return };
    for (mut o, mut b) in &mut olgas {
        let to_snake = sb.pos - b.pos;
        let face = to_snake.x.atan2(to_snake.z);
        // Where she stands to shoot: beside her cover, across Snake's line.
        let across = Vec3::new(to_snake.z, 0.0, -to_snake.x).normalize_or_zero();
        let out_at = o.cover + across * STEP_OUT * o.side;
        o.state = match o.state {
            OlgaState::Down => {
                b.play_once(motion::DOWN);
                OlgaState::Down
            }
            OlgaState::Cover(t) => {
                b.play(motion::COVER);
                b.yaw = turn_toward(b.yaw, face, 3.0 * dt);
                if t - dt <= 0.0 && !player.dead() { OlgaState::StepOut } else { OlgaState::Cover(t - dt) }
            }
            OlgaState::StepOut => {
                b.play(motion::RUN);
                if step(&mut b, out_at, dt) { OlgaState::Aim(0.0) } else { OlgaState::StepOut }
            }
            OlgaState::Aim(f) => {
                b.play(motion::AIM);
                b.yaw = turn_toward(b.yaw, face, 6.0 * dt);
                if f + frames >= AIM_FRAMES {
                    OlgaState::Fire { shots: 3 + rng.next() % 4, cool: 0.0 }
                } else {
                    OlgaState::Aim(f + frames)
                }
            }
            OlgaState::Fire { shots, cool } => {
                b.play(motion::FIRE);
                b.yaw = turn_toward(b.yaw, face, 6.0 * dt);
                if cool - frames > 0.0 {
                    OlgaState::Fire { shots, cool: cool - frames }
                } else if shots == 0 || player.dead() {
                    OlgaState::StepIn
                } else {
                    let muzzle = b.pos + Vec3::Y * 1400.0 + b.forward() * 400.0;
                    bullet::fire(&mut commands, &mut rng, muzzle, sb.pos + Vec3::Y * (player.height() * 0.6));
                    OlgaState::Fire { shots: shots - 1, cool: SHOT_FRAMES }
                }
            }
            OlgaState::StepIn => {
                b.play(motion::RUN);
                let cover = o.cover;
                if step(&mut b, cover, dt) {
                    o.side = -o.side;
                    OlgaState::Cover(COVER_TIME)
                } else {
                    OlgaState::StepIn
                }
            }
        };
    }
}

/// Runs toward `to`; true on arrival.
fn step(b: &mut Body, to: Vec3, dt: f32) -> bool {
    let d = Vec2::new(to.x - b.pos.x, to.z - b.pos.z);
    if d.length() < 80.0 {
        return true;
    }
    b.yaw = turn_toward(b.yaw, d.x.atan2(d.y), 8.0 * dt);
    let s = (RUN_SPEED * dt).min(d.length());
    let v = d.normalize() * s;
    b.pos += Vec3::new(v.x, 0.0, v.y);
    false
}

