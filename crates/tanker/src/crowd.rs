//! The Marines standing in formation in the holds (`chara #b3e388`, w04c
//! resident `cmd_b3e388` / `FUN_0022a5d8`).
//!
//! `-a` lists the members, eleven values each: name, x y z, rotation x y z
//! (4096ths), variant (`% 22`), an unused value, idle variation (`% 6`) and
//! flags (0x10 / 0x20 / 0x40: rifle `m4a_nm`; 0x1 cap, 0x4 helmet, 0x80
//! hat, 0x100 headphones; 0x2 glasses). Variants below 18 are `us_def_1`
//! with some parts swapped for `w04a_us_def` parts (table at 0x1f73e0, 0x54
//! bytes per variant, one entry per part, -1 = keep); 19 is the commandant
//! (`sco_def`), 20-21 `us_def_pants`. Members placed at the origin are
//! moved by the actor later (the commandant's walk) and are left out here.
//!
//! Drawn standing with the first `ushold` motion; the part swaps,
//! accessories and the crowd's reactions are not done yet.

use bevy::prelude::*;

use crate::gcl::{Spawn, Val};

pub const CROWD_ACTOR: u32 = 0xB3E388;
const RECORD: usize = 11;

#[derive(Component)]
pub struct Marine;

/// One member: feet position (mm), facing, variant, idle variation.
pub struct Member {
    pub pos: Vec3,
    pub yaw: f32,
    pub variant: i32,
    pub idle: i32,
}

pub fn members(s: &Spawn) -> Vec<Member> {
    let Some(list) = s.opt(b'a') else { return Vec::new() };
    list.chunks_exact(RECORD)
        .filter_map(|r| {
            let v = |i: usize| r[i].int().unwrap_or(0);
            if !matches!(r[0], Val::Name(_) | Val::Int(_)) {
                return None;
            }
            let pos = Vec3::new(v(1) as f32, v(2) as f32, v(3) as f32);
            (pos.y != 0.0).then(|| Member {
                pos,
                yaw: v(5) as f32 / 4096.0 * std::f32::consts::TAU,
                variant: v(7).rem_euclid(22),
                idle: v(9).rem_euclid(6),
            })
        })
        .collect()
}

/// Model for a variant: the base body for the marines, the commandant's
/// own model, the trousers-only variant.
pub fn model_name(variant: i32) -> &'static str {
    match variant {
        19 => "sco_def",
        20 | 21 => "us_def_pants",
        _ => "us_def_1",
    }
}
