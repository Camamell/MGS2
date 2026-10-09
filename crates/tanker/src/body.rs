//! Skinned, animated characters (Snake, guards).

use bevy::prelude::*;
use mgs2_bevy::anim;
use mgs2_bevy::scene::{Part, SCALE};

use crate::Game;

/// Placement and animation state of a character. Units are mm.
#[derive(Component)]
pub struct Body {
    /// Feet position.
    pub pos: Vec3,
    /// Facing; the models face +Z at 0.
    pub yaw: f32,
    /// Index into `StageData::models`.
    pub model: usize,
    /// Index into `StageData::motions`.
    pub motions: Option<usize>,
    pub motion: usize,
    pub frame: f32,
    /// Stop on the last frame instead of looping.
    pub once: bool,
}

impl Body {
    pub fn new(pos: Vec3, yaw: f32, model: usize, motions: Option<usize>) -> Self {
        Self { pos, yaw, model, motions, motion: 0, frame: 0.0, once: false }
    }

    /// Switches to a looping motion, restarting it only when it changes.
    pub fn play(&mut self, motion: usize) {
        if self.motion != motion || self.once {
            self.motion = motion;
            self.frame = 0.0;
            self.once = false;
        }
    }

    /// Plays a motion once and holds its last frame.
    pub fn play_once(&mut self, motion: usize) {
        if self.motion != motion || !self.once {
            self.motion = motion;
            self.frame = 0.0;
            self.once = true;
        }
    }

    pub fn forward(&self) -> Vec3 {
        Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos())
    }
}

/// Turns `yaw` toward `target` by at most `max_step` radians.
pub fn turn_toward(yaw: f32, target: f32, max_step: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let mut diff = (target - yaw).rem_euclid(TAU);
    if diff > PI {
        diff -= TAU;
    }
    yaw + diff.clamp(-max_step, max_step)
}

/// A model carried in a character's hand (weapons), placed on that joint
/// every frame. Assumes the model is built in the hand's space.
#[derive(Component)]
pub struct Held {
    pub owner: Entity,
    /// Joint (object index) it is attached to.
    pub object: usize,
    /// Only shown while the owner is aiming (Snake's M9).
    pub when_aiming: bool,
    /// Weapon orientation in the hand's space (found by eye, not from the
    /// game: the models are built barrel along -y).
    pub grip: Quat,
}

/// Grip for the AKS (barrel along -y in the model): points it forward (+z)
/// from the guard's hand.
pub fn rifle_grip() -> Quat {
    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
}

/// Grip for the M9: as modelled (it is built in the hand's space; the AKS's
/// turn above may mean the game holds the rifle from another joint).
pub fn pistol_grip() -> Quat {
    Quat::IDENTITY
}

/// Right hand joint of both Snake (`sna_def`) and the soldiers (`gbs_def`):
/// the end of the arm chain on the -x side (models face +z).
pub const RIGHT_HAND: usize = 6;

pub fn hold(
    mut commands: Commands,
    game: Res<Game>,
    bodies: Query<(&Body, Option<&crate::player::Player>), Without<Held>>,
    mut held: Query<(Entity, &Held, &mut Transform, &mut Visibility)>,
) {
    for (e, h, mut t, mut vis) in &mut held {
        let Ok((b, player)) = bodies.get(h.owner) else {
            // Its owner has gone (a trooper who left).
            commands.entity(e).despawn();
            continue;
        };
        let model = &game.data.models[b.model].1;
        let motion = b.motions.and_then(|m| game.data.motions[m].1.motions.get(b.motion));
        let Some(hand) = anim::pose(model, motion, b.frame, false).get(h.object).copied() else { continue };
        let owner = Transform::from_translation(b.pos * SCALE).with_rotation(Quat::from_rotation_y(b.yaw));
        let (_, rot, pos) = hand.to_scale_rotation_translation();
        *t = owner * Transform::from_translation(pos * SCALE).with_rotation(rot * h.grip);
        let shown = !h.when_aiming || player.is_some_and(|p| p.aiming);
        *vis = if shown { Visibility::Inherited } else { Visibility::Hidden };
    }
}

/// Places every body and CPU-skins its meshes.
pub fn skin(
    time: Res<Time>,
    game: Res<Game>,
    mut bodies: Query<(Entity, &mut Body, &mut Transform)>,
    children: Query<&Children>,
    parts: Query<(&Part, &Mesh3d)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let dt = time.delta_secs().min(0.05);
    for (e, mut b, mut t) in &mut bodies {
        t.translation = b.pos * SCALE;
        t.rotation = Quat::from_rotation_y(b.yaw);
        let model = &game.data.models[b.model].1;
        let motion = b.motions.and_then(|m| game.data.motions[m].1.motions.get(b.motion));
        b.frame += dt * 60.0;
        if let (true, Some(m)) = (b.once, motion) {
            b.frame = b.frame.min(m.length.saturating_sub(1) as f32);
        }
        let mats = anim::pose(model, motion, b.frame, false);
        // Lit by the stage's lt2 like the map (assumed: the same per-vertex
        // lights at the body's current place).
        let lt2 = game.data.lights.first().map(|(_, l)| l).filter(|_| std::env::var_os("MGS2_NO_LT2").is_none());
        let reach = Vec3::splat(2000.0);
        let lights = lt2.map(|l| l.gather((b.pos - reach).into(), (b.pos + reach).into())).unwrap_or_default();
        for child in children.iter_descendants(e) {
            let Ok((part, mesh3d)) = parts.get(child) else { continue };
            let Some(mut mesh) = meshes.get_mut(&mesh3d.0) else { continue };
            let (pos, nrm) = anim::skin(model, part.object, part.mesh, &mats);
            if let Some(l) = lt2 {
                let col: Vec<[f32; 4]> = pos
                    .iter()
                    .zip(&nrm)
                    .map(|(p, n)| {
                        let c = l.shade((t.rotation * Vec3::from(*p) + b.pos).into(), (t.rotation * Vec3::from(*n)).into(), &lights);
                        [c[0] / 128.0, c[1] / 128.0, c[2] / 128.0, 1.0]
                    })
                    .collect();
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
            }
            let pos: Vec<[f32; 3]> = pos.into_iter().map(|v| (Vec3::from(v) * SCALE).into()).collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
        }
    }
}
