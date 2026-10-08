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
}

impl Body {
    /// Switches motion, restarting it only when it changes.
    pub fn play(&mut self, motion: usize) {
        if self.motion != motion {
            self.motion = motion;
            self.frame = 0.0;
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
        b.frame += dt * 60.0;
        t.translation = b.pos * SCALE;
        t.rotation = Quat::from_rotation_y(b.yaw);
        let model = &game.data.models[b.model].1;
        let motion = b.motions.and_then(|m| game.data.motions[m].1.motions.get(b.motion));
        let mats = anim::pose(model, motion, b.frame, false);
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
