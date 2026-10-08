//! Bullets, from the guard shot `FUN_002550C0` -> `FUN_0036D5B8(matrix,
//! flags, 1, 0x19, 10, 20000, 1000, ...)`: hit box 25 mm, damage 10,
//! range 20000 mm, speed 1000 mm per frame; aim jitter ±0x80 (4096 units)
//! on two axes.

use bevy::prelude::*;
use mgs2_bevy::scene::SCALE;

use crate::Game;
use crate::body::Body;
use crate::player::{HEIGHT, Player, RADIUS};

pub const SPEED: f32 = 1000.0 * 60.0;
pub const RANGE: f32 = 20000.0;
pub const DAMAGE: i32 = 10;
/// Aim jitter, radians (0x80 of 4096 per axis).
pub const SPREAD: f32 = 0x80 as f32 / 4096.0 * std::f32::consts::TAU;

#[derive(Component)]
pub struct Bullet {
    pub pos: Vec3,
    pub dir: Vec3,
    pub travelled: f32,
}

/// Small deterministic RNG (the game uses `FUN_0016F600`).
#[derive(Resource)]
pub struct Rng(pub u32);

impl Rng {
    pub fn next(&mut self) -> u32 {
        // xorshift32
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform in [-1, 1).
    pub fn signed(&mut self) -> f32 {
        (self.next() % 65536) as f32 / 32768.0 - 1.0
    }
}

pub fn fire(commands: &mut Commands, rng: &mut Rng, from: Vec3, at: Vec3) {
    let d = (at - from).normalize_or_zero();
    let yaw = d.x.atan2(d.z) + rng.signed() * SPREAD;
    let pitch = d.y.asin() + rng.signed() * SPREAD;
    let dir = Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos());
    commands.spawn(Bullet { pos: from, dir, travelled: 0.0 });
}

/// Moves bullets, stops them at walls and applies hits to Snake.
pub fn fly(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    mut bullets: Query<(Entity, &mut Bullet)>,
    mut snake: Query<(&Body, &mut Player)>,
) {
    let dt = time.delta_secs().min(0.05);
    let Ok((sb, mut player)) = snake.single_mut() else { return };
    for (e, mut b) in &mut bullets {
        let step = SPEED * dt;
        let next = b.pos + b.dir * step;
        // Snake: distance from his body axis to this frame's segment.
        let base = sb.pos;
        let hit = (0..=8).any(|i| {
            let p = b.pos.lerp(next, i as f32 / 8.0);
            let flat = Vec2::new(p.x - base.x, p.z - base.z).length();
            flat < RADIUS && p.y > base.y && p.y < base.y + HEIGHT
        });
        let wall = game.collision.as_ref().is_some_and(|c| c.line_blocked(b.pos, next));
        if hit && !player.dead() {
            player.damage(DAMAGE);
            commands.entity(e).despawn();
            continue;
        }
        b.pos = next;
        b.travelled += step;
        if wall || b.travelled > RANGE {
            commands.entity(e).despawn();
        }
    }
}

pub fn draw(bullets: Query<&Bullet>, mut gizmos: Gizmos) {
    for b in &bullets {
        let p = b.pos * SCALE;
        gizmos.line(p, p - b.dir * 0.6, Color::srgb(1.0, 0.9, 0.5));
    }
}
