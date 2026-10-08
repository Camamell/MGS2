//! Snake: movement, collision and the follow camera.

use std::f32::consts::TAU;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings, RayCastVisibility};
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
    /// Life (script Bs16:0xf2, player+0x782) and its maximum (Bs16:0xf4).
    pub life: i32,
    pub max_life: i32,
    pub rations: u32,
    /// Seconds of hit flash left.
    pub hurt: f32,
    pub very_easy: bool,
    /// Holding the M9 up.
    pub aiming: bool,
}

/// Maximum life by difficulty, from `n_title`'s difficulty select
/// (Very Easy 200, Easy 150, Normal 100, Hard 75, Extreme 50).
pub fn max_life(difficulty: i16) -> i32 {
    match difficulty {
        ..=10 => 200,
        11..=20 => 150,
        21..=30 => 100,
        31..=40 => 75,
        _ => 50,
    }
}

impl Player {
    pub fn new(difficulty: i16) -> Self {
        let max = max_life(difficulty);
        Self { life: max, max_life: max, rations: 0, very_easy: difficulty <= 10, ..default() }
    }

    pub fn dead(&self) -> bool {
        self.life <= 0
    }

    /// Damage with the ration revive of `FUN_00289320`: an equipped ration
    /// restores full life on Very Easy, half otherwise.
    pub fn damage(&mut self, amount: i32) {
        self.life -= amount;
        self.hurt = 0.25;
        if self.life <= 0 && self.rations > 0 {
            self.rations -= 1;
            self.life = if self.very_easy { self.max_life } else { self.max_life / 2 };
        }
    }
}

#[derive(Component)]
pub struct FollowCam {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    /// Distance after pulling in for obstacles.
    pub current: f32,
    /// Follow the n-th guard instead of Snake (debugging).
    pub guard: Option<usize>,
}

/// Synthetic input for automated runs (`--auto-walk`).
#[derive(Resource)]
pub struct AutoWalk {
    pub from: f32,
    pub seconds: f32,
}

/// `--fire-at`: aim for half a second from this time, then fire.
#[derive(Resource)]
pub struct AutoFire(pub f32);

/// Muzzle height when aiming (mm above the feet, approximate).
const MUZZLE: f32 = 1350.0;

#[allow(clippy::too_many_arguments)]
pub fn control(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    auto: Option<Res<AutoWalk>>,
    auto_fire: Option<Res<AutoFire>>,
    mut inv: ResMut<crate::items::Inventory>,
    mut game: ResMut<Game>,
    cam: Query<&FollowCam>,
    mut q: Query<(&mut Player, &mut Body)>,
) {
    let Ok((mut p, mut body)) = q.single_mut() else { return };
    let Ok(cam) = cam.single() else { return };
    let dt = time.delta_secs().min(0.05);
    p.hurt = (p.hurt - dt).max(0.0);
    if p.dead() {
        p.speed = 0.0;
        body.play(motion::IDLE);
        return;
    }

    // M9: hold F to aim along the camera's view, release to fire a dart.
    let t = time.elapsed_secs();
    let aim = (keys.pressed(KeyCode::KeyF) || auto_fire.as_ref().is_some_and(|a| (a.0..a.0 + 0.5).contains(&t)))
        && game.entrance <= 0.0;
    if aim {
        p.aiming = true;
        p.speed = 0.0;
        body.yaw = turn_toward(body.yaw, cam.yaw + std::f32::consts::PI, TURN_RATE * 2.0 * dt);
        body.play(motion::IDLE);
        return;
    }
    if std::mem::take(&mut p.aiming) {
        let ammo = inv.counts.entry(crate::items::M9_AMMO).or_default();
        if *ammo > 0 {
            *ammo -= 1;
            let f = body.forward();
            crate::bullet::fire_dart(&mut commands, body.pos + Vec3::Y * MUZZLE + f * 400.0, f);
        }
    }

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
    let mut dir = (fwd * input.y + right * input.x).normalize_or_zero();
    // Walking in through a door: straight ahead, no control yet.
    let entering = game.entrance > 0.0;
    let walking = walking || entering;
    if entering {
        let f = body.forward();
        dir = Vec2::new(f.x, f.z).normalize_or_zero();
    }

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
    if entering {
        let moved = Vec2::new(next.x - old.x, next.z - old.z).length();
        game.entrance -= moved;
        // Done, or walked into something.
        if game.entrance <= 0.0 || moved < speed * dt * 0.25 {
            game.entrance_done();
        }
    }

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
    game: Res<Game>,
    mut cam: Query<(&mut Transform, &mut FollowCam)>,
    mut rays: MeshRayCast,
    parents: Query<&ChildOf>,
    bodies: Query<(), With<Body>>,
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
    let manual = buttons.pressed(MouseButton::Right) || keys.pressed(KeyCode::KeyQ) || keys.pressed(KeyCode::KeyE);
    let dir = |yaw: f32| Vec3::new(yaw.sin(), 0.0, yaw.cos()) * c.pitch.cos() + Vec3::Y * c.pitch.sin();
    let mut back = dir(c.yaw);
    let mut want = c.distance;
    if let Some(col) = &game.collision {
        // First obstacle between two points (metres) as a fraction: level
        // meshes (front faces: ceilings from below, walls facing Snake) and
        // the collision walls and floors.
        let not_character = |e: Entity| parents.get(e).map_or(true, |p| !bodies.contains(p.parent()));
        let settings = MeshRayCastSettings::default()
            .with_filter(&not_character)
            .with_visibility(RayCastVisibility::Any);
        let mut obstacle = |a: Vec3, b: Vec3| -> Option<f32> {
            let len = a.distance(b);
            let mesh = Dir3::new(b - a)
                .ok()
                .and_then(|d| rays.cast_ray(Ray3d::new(a, d), &settings).first().map(|(_, h)| h.distance / len))
                .filter(|t| *t <= 1.0);
            let wall = col.ray(a / SCALE, b / SCALE);
            mesh.into_iter().chain(wall).min_by(f32::total_cmp)
        };
        // Where the camera can be along `back`: under the ceiling, flattening
        // the view rather than closing in, then pulled in front of walls.
        let mut place = |back: Vec3| -> (Vec3, f32) {
            let mut cam = target + back * c.distance;
            if let Some(t) = obstacle(target, target + Vec3::Y * 20.0) {
                let ceiling = target.y + t * 20.0 - 0.3;
                cam.y = cam.y.min(ceiling.max(target.y));
            }
            let offset = cam - target;
            let mut d = offset.length();
            if let Some(t) = obstacle(target, cam) {
                d = (d * t - 0.3).max(0.8);
            }
            // Outside the playable area there are no walls to stop the
            // camera, so also require floor under it.
            let at = (target + offset.normalize_or(back) * d) / SCALE;
            if !col.floor_below(at.x, at.z, at.y).is_some_and(|y| y > target.y / SCALE - 3000.0) {
                d = d.min(0.8);
            }
            (offset.normalize_or(back), d)
        };
        (back, want) = place(back);
        // Boxed in (just through a door, in a corridor): swing round to the
        // nearest clear side.
        if !manual && want < c.distance * 0.5 {
            let best = [0.5f32, -0.5, 1.0, -1.0, 1.5, -1.5, 2.0, -2.0, 2.6, -2.6, std::f32::consts::PI]
                .into_iter()
                .find(|o| place(dir(c.yaw + o)).1 >= c.distance * 0.8);
            if let Some(o) = best {
                c.yaw += o.signum() * o.abs().min(2.5 * dt);
            }
        }
    }
    c.current = if want < c.current { want } else { c.current + (want - c.current) * (dt * 3.0).min(1.0) };
    tf.translation = target + back * c.current;
    tf.look_at(target, Vec3::Y);
}


