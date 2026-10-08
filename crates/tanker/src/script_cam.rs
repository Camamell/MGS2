//! The stage scripts' cameras (`chara 0x43f718 <name> -c <type> ... -s
//! <on/off>`), usually switched by traps as Snake moves between areas.
//!
//! - `-c 0 -b x y z x y z -l x y z x y z -r pitch yaw roll -f dist`: follow
//!   Snake from `dist` mm at the given angles (4096 = full turn), with the
//!   camera position clamped into box `-b` and the point looked at into box
//!   `-l`. A flat box makes a rail.
//! - `-c 1 -p x y z -t x y z`: a fixed camera at `-p` (aimed toward `-t`;
//!   here it keeps Snake in view).
//! - `-s`: 1 or an enter/stay trap event switches it on, -1 or the leave
//!   event off.
//!
//! Read from the scripts; the camera actor's own code is not ported yet
//! (`-a` blend time, `-i`, `-E`, `-z`, `-N` and type 2 are ignored).

use bevy::prelude::*;

use crate::gcl::{Spawn, trap_event};

pub const CAMERA_ACTOR: u32 = 0x43F718;

#[derive(Clone, Debug)]
pub enum ScriptCam {
    Follow { bmin: Vec3, bmax: Vec3, lmin: Vec3, lmax: Vec3, pitch: f32, yaw: f32, dist: f32 },
    Fixed { pos: Vec3, target: Vec3 },
}

/// The camera currently switched on, by actor name.
#[derive(Default)]
pub struct Cameras {
    pub active: Option<(u32, ScriptCam)>,
}

fn angle(v: i32) -> f32 {
    v as f32 / 4096.0 * std::f32::consts::TAU
}

impl Cameras {
    /// Applies camera actors created by a script run, in order.
    pub fn apply(&mut self, spawns: &[Spawn]) {
        for s in spawns.iter().filter(|s| s.kind == CAMERA_ACTOR) {
            let Some(name) = s.args.first().and_then(|v| v.int()) else { continue };
            let name = name as u32;
            let on = match s.opt_int(b's', 0) {
                Some(1) => true,
                Some(e) if e as u32 == trap_event::ENTER || e as u32 == trap_event::STAY => true,
                Some(_) => false,
                None => true,
            };
            if !on {
                if self.active.as_ref().is_some_and(|(n, _)| *n == name) {
                    self.active = None;
                }
                continue;
            }
            let v3 = |l: u8, i: usize| -> Option<Vec3> {
                Some(Vec3::new(s.opt_int(l, i)? as f32, s.opt_int(l, i + 1)? as f32, s.opt_int(l, i + 2)? as f32))
            };
            let cam = match s.opt_int(b'c', 0) {
                Some(0) => (|| {
                    let (b0, b1, l0, l1) = (v3(b'b', 0)?, v3(b'b', 3)?, v3(b'l', 0)?, v3(b'l', 3)?);
                    Some(ScriptCam::Follow {
                        bmin: b0.min(b1),
                        bmax: b0.max(b1),
                        lmin: l0.min(l1),
                        lmax: l0.max(l1),
                        pitch: angle(s.opt_int(b'r', 0)?),
                        yaw: angle(s.opt_int(b'r', 1)?),
                        dist: s.opt_int(b'f', 0)? as f32,
                    })
                })(),
                Some(1) => (|| Some(ScriptCam::Fixed { pos: v3(b'p', 0)?, target: v3(b't', 0)? }))(),
                _ => None,
            };
            if let Some(cam) = cam {
                self.active = Some((name, cam));
            }
        }
    }
}

impl ScriptCam {
    /// Eye and look-at point (mm) for Snake's chest at `snake`.
    pub fn place(&self, snake: Vec3) -> (Vec3, Vec3) {
        match self {
            ScriptCam::Follow { bmin, bmax, lmin, lmax, pitch, yaw, dist } => {
                // The camera faces along (yaw, pitch down); it sits `dist`
                // behind the look-at point.
                let facing = Vec3::new(yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos());
                let look = snake.clamp(*lmin, *lmax);
                let eye = (snake - facing * *dist).clamp(*bmin, *bmax);
                (eye, look)
            }
            ScriptCam::Fixed { pos, target } => (*pos, snake.lerp(*target, 0.25)),
        }
    }
}
