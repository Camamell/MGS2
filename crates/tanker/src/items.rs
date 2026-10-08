//! Item boxes (`chara 0x9aff54 <name> -p x y z -k model -f kind -i id -n
//! count -x { on pickup }`) and Snake's inventory.
//!
//! `-f` is the category: 0 item, 1 weapon ammunition, 2 grenade-type
//! weapon. Names follow MGS2's item and weapon order.

use std::collections::{BTreeMap, HashSet};

use bevy::prelude::*;

use crate::body::Body;
use crate::gcl::Spawn;
use crate::player::Player;

pub const ITEM_ACTOR: u32 = 0x9AFF54;
/// Inventory key of the M9's darts (weapon ammunition, weapon 1).
pub const M9_AMMO: (i32, i32) = (1, 1);
/// Darts Snake starts with (an assumption, not read from the game).
pub const M9_START: i32 = 15;
/// Pickup reach (mm, horizontal) and height tolerance.
const REACH: f32 = 700.0;

#[derive(Component, Clone, Debug)]
pub struct Item {
    /// The actor's name (first argument), remembered once taken.
    pub name: u32,
    pub kind: i32,
    pub id: i32,
    pub count: i32,
    pub pos: Vec3,
}

impl Item {
    pub fn from_spawn(s: &Spawn) -> Option<(Item, u32)> {
        let name = s.args.first()?.int()? as u32;
        let p = |i| s.opt_int(b'p', i).unwrap_or(0) as f32;
        let model = s.opt(b'k')?.first()?.int()? as u32 & 0x00FF_FFFF;
        let item = Item {
            name,
            kind: s.opt_int(b'f', 0).unwrap_or(0),
            id: s.opt_int(b'i', 0).unwrap_or(0),
            count: s.opt_int(b'n', 0).unwrap_or(1),
            pos: Vec3::new(p(0), p(1), p(2)),
        };
        Some((item, model))
    }
}

pub fn name(kind: i32, id: i32) -> String {
    let known = match (kind, id) {
        (0, 1) => "Ration",
        (0, 4) => "Cold Medicine",
        (0, 5) => "Bandage",
        (0, 13) => "Thermal Goggles",
        (0, 16) | (0, 24) => "Box",
        (0, 33) => "Dog Tags",
        (1, 1) => "M9 darts",
        (1, 2) => "USP ammo",
        (2, 10) => "Chaff G.",
        (2, 11) => "Stun G.",
        (2, 17) => "Grenade",
        _ => "",
    };
    if known.is_empty() { format!("item {kind}/{id}") } else { known.to_string() }
}

/// What Snake carries, kept across stages.
#[derive(Resource)]
pub struct Inventory {
    pub counts: BTreeMap<(i32, i32), i32>,
    /// Item actors already taken, per stage.
    pub taken: HashSet<(String, u32)>,
    /// Last pickup message and seconds to show it.
    pub message: Option<(String, f32)>,
    pub ration_equipped: bool,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            counts: BTreeMap::from([(M9_AMMO, M9_START)]),
            taken: HashSet::new(),
            message: None,
            ration_equipped: false,
        }
    }
}

pub fn pickup(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    game: Res<crate::Game>,
    mut inv: ResMut<Inventory>,
    mut snake: Query<(&Body, &mut Player)>,
    items: Query<(Entity, &Item)>,
) {
    if let Some((_, t)) = inv.message.as_mut() {
        *t -= time.delta_secs();
        if *t <= 0.0 {
            inv.message = None;
        }
    }
    let Ok((body, mut player)) = snake.single_mut() else { return };
    if keys.just_pressed(KeyCode::KeyR) {
        inv.ration_equipped = !inv.ration_equipped;
    }
    for (e, item) in &items {
        let d = Vec2::new(item.pos.x - body.pos.x, item.pos.z - body.pos.z).length();
        if d > REACH || (item.pos.y - body.pos.y).abs() > 1000.0 || player.dead() {
            continue;
        }
        *inv.counts.entry((item.kind, item.id)).or_default() += item.count;
        inv.taken.insert((game.data.name.clone(), item.name));
        inv.message = Some((format!("{} x{}", name(item.kind, item.id), item.count), 2.5));
        commands.entity(e).despawn();
    }
    // Rations live in the inventory; the revive uses an equipped one.
    player.rations = if inv.ration_equipped { inv.counts.get(&(0, 1)).copied().unwrap_or(0).max(0) as u32 } else { 0 };
}

/// After a revive (`Player::damage` used one), take it from the inventory.
pub fn sync_rations(mut inv: ResMut<Inventory>, snake: Query<&Player>) {
    let Ok(p) = snake.single() else { return };
    if inv.ration_equipped {
        let have = inv.counts.get(&(0, 1)).copied().unwrap_or(0);
        if (p.rations as i32) < have {
            inv.counts.insert((0, 1), p.rations as i32);
            inv.message = Some(("Ration used".into(), 2.5));
        }
    }
}
