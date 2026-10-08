//! Snake: movement, collision and the follow camera.

use std::f32::consts::TAU;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use mgs2_bevy::scene::SCALE;

use crate::Game;
use crate::body::{Body, turn_toward};

/// Motions in `snake.mar` identified by their root motion (see
/// `re/scripts/mar_root.py`): hip height and travel speed.
pub mod motion {
    pub const IDLE: usize = 0;
    pub const CROUCH_IDLE: usize = 5;
    pub const WALK: usize = 17;
    pub const RUN: usize = 18;
    pub const CROUCH_WALK: usize = 53;
}

/// Travel speeds measured from those motions' root tracks (mm/s).
const WALK_SPEED: f32 = 1250.0;
const RUN_SPEED: f32 = 3050.0;
const CROUCH_SPEED: f32 = 1250.0;
const TURN_RATE: f32 = TAU * 1.5;
pub const RADIUS: f32 = 300.0;
pub const HEIGHT: f32 = 1700.0;
pub const STEP_UP: f32 = 450.0;
const GRAVITY: f32 = 9800.0;

#[derive(Component, Default)]
pub struct Player {
    pub fall_speed: f32,
    pub crouch: bool,
    /// Current horizontal speed (mm/s); guards hear running.
    pub speed: f32,
}

#[derive(Component)]
pub struct FollowCam {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    /// Follow the n-th guard instead of Snake (debugging).
    pub guard: Option<usize>,
}

/// Synthetic input for automated runs (`--auto-walk`).
#[derive(Resource)]
pub struct AutoWalk {
    pub from: f32,
    pub seconds: f32,
}

pub fn control(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    auto: Option<Res<AutoWalk>>,
    game: Res<Game>,
    cam: Query<&FollowCam>,
    mut q: Query<(&mut Player, &mut Body)>,
) {
    let Ok((mut p, mut body)) = q.single_mut() else { return };
    let Ok(cam) = cam.single() else { return };
    let dt = time.delta_secs().min(0.05);

    let mut input = Vec2::ZERO;
    for (k, v) in [
        (KeyCode::KeyW, Vec2::Y),
        (KeyCode::KeyS, Vec2::NEG_Y),
        (KeyCode::KeyD, Vec2::X),
        (KeyCode::KeyA, Vec2::NEG_X),
    ] {
        if keys.pressed(k) {
            input += v;
        }
    }
    if auto.is_some_and(|a| (a.from..a.from + a.seconds).contains(&time.elapsed_secs())) {
        input = Vec2::Y;
    }
    if keys.just_pressed(KeyCode::KeyC) {
        p.crouch = !p.crouch;
    }
    let walking = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    // Camera-relative direction on the ground plane.
    let fwd = Vec2::new(-cam.yaw.sin(), -cam.yaw.cos());
    let right = Vec2::new(-fwd.y, fwd.x);
    let dir = (fwd * input.y + right * input.x).normalize_or_zero();

    let moving = dir != Vec2::ZERO;
    let speed = match (moving, p.crouch, walking) {
        (false, _, _) => 0.0,
        (true, true, _) => CROUCH_SPEED,
        (true, false, true) => WALK_SPEED,
        (true, false, false) => RUN_SPEED,
    };
    p.speed = speed;
    if moving {
        body.yaw = turn_toward(body.yaw, dir.x.atan2(dir.y), TURN_RATE * dt);
    }

    // Horizontal move, then walls, then floor.
    let old = body.pos;
    let mut next = body.pos + Vec3::new(dir.x, 0.0, dir.y) * speed * dt;
    if let Some(col) = &game.collision {
        next = col.push_out(next, RADIUS, HEIGHT, STEP_UP);
        match col.floor_below(next.x, next.z, next.y + STEP_UP) {
            Some(floor) => {
                if next.y - floor > 30.0 {
                    p.fall_speed += GRAVITY * dt;
                    next.y = (next.y - p.fall_speed * dt).max(floor);
                } else {
                    next.y = floor;
                }
                if next.y <= floor {
                    p.fall_speed = 0.0;
                }
            }
            // No ground: refuse the move (the edge of the walkable area).
            None => next = old,
        }
    }
    body.pos = next;

    body.play(match (p.crouch, speed) {
        (true, s) if s > 0.0 => motion::CROUCH_WALK,
        (true, _) => motion::CROUCH_IDLE,
        (false, s) if s >= RUN_SPEED => motion::RUN,
        (false, s) if s > 0.0 => motion::WALK,
        _ => motion::IDLE,
    });
}

/// Orbiting follow camera: right mouse drag or Q/E to turn.
pub fn follow_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    player: Query<&Body, With<Player>>,
    guards: Query<&Body, (With<crate::guard::Guard>, Without<Player>)>,
    mut cam: Query<(&mut Transform, &mut FollowCam)>,
) {
    let Ok((mut tf, mut c)) = cam.single_mut() else { return };
    let b = match c.guard {
        Some(i) => guards.iter().nth(i),
        None => player.single().ok(),
    };
    let Some(b) = b else { return };
    let dt = time.delta_secs();
    if buttons.pressed(MouseButton::Right) {
        c.yaw -= motion.delta.x * 0.004;
        c.pitch = (c.pitch + motion.delta.y * 0.004).clamp(0.1, 1.55);
    }
    if keys.pressed(KeyCode::KeyQ) {
        c.yaw += 2.0 * dt;
    }
    if keys.pressed(KeyCode::KeyE) {
        c.yaw -= 2.0 * dt;
    }
    let target = b.pos * SCALE + Vec3::Y * 0.9;
    let back = Vec3::new(c.yaw.sin(), 0.0, c.yaw.cos()) * c.pitch.cos() + Vec3::Y * c.pitch.sin();
    tf.translation = target + back * c.distance;
    tf.look_at(target, Vec3::Y);
}


