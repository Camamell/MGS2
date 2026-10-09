//! Lightning over the open decks: `chara #89af5e [-p chance] [-t]` (w00a-c,
//! w01e; `cmd_89af5e`, act `FUN_0029aa08` in their residents).
//!
//! Each frame it strikes with chance 1 / `-p` (default 0x1000). A strike
//! (unless `-t`) draws a bolt (`d_thunder.c`, not ported) and lights five
//! copies of a 64 m sky quad (model 0x3177fc, texture 0x985073), each turned
//! at random and spread over the sky (`FUN_0029a3c0`), tinted (25, 50, 50)
//! +-5 (its other tint, (18, 30, 30), is never chosen), for 10..29 frames: they
//! flicker (each frame shown or not at random) and fade by 10 a frame over
//! the last 10. Its thunder follows 60..123 frames later (effect 0x1d5 /
//! 0x1d6), or 30..61 (0x1d8 / 0x1d9) for a near one (`FUN_0029a9c0`). The
//! game places the quads around the bolt's point in the sky, which it picks
//! on screen; here they go up ahead of the camera.

use bevy::prelude::*;

use crate::{Game, SCALE};

pub const THUNDER_ACTOR: u32 = 0x89AF5E;
const FLASH_MODEL: u32 = 0x3177FC;

#[derive(Clone, Copy, Debug)]
pub struct ThunderSpec {
    /// One strike in this many frames on average.
    pub chance: u32,
}

pub fn from_spawn(s: &crate::gcl::Spawn) -> ThunderSpec {
    // MGS2_THUNDER=n: one strike in n frames instead (testing).
    let test = std::env::var("MGS2_THUNDER").ok().and_then(|v| v.parse().ok());
    ThunderSpec { chance: test.unwrap_or(s.opt_int(b'p', 0).filter(|p| *p > 0).unwrap_or(0x1000) as u32).max(1) }
}

/// A lit sky quad.
#[derive(Component)]
struct Flash {
    material: Handle<StandardMaterial>,
}

#[derive(Default)]
struct State {
    /// Frames the flash still lasts (-1: none).
    left: i32,
    colour: Vec3,
    /// (frames until the thunder, effect).
    sound: Option<(i32, u32)>,
    /// 60 Hz frame accumulator.
    acc: f32,
    rng: u32,
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, run);
}

fn rand(s: &mut State) -> u32 {
    // The game's generator (`DAT_0019227c`).
    s.rng = s.rng.wrapping_mul(0x5D58_8B65).wrapping_add(1);
    s.rng
}

#[allow(clippy::too_many_arguments)]
fn run(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    cutscene: Res<crate::demo::Cutscene>,
    cams: Query<&Transform, With<crate::player::FollowCam>>,
    mut flashes: Query<(Entity, &Flash, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut st: Local<State>,
) {
    let Some(spec) = game.thunder else {
        for (e, _, _) in &flashes {
            commands.entity(e).despawn();
        }
        return;
    };
    if st.rng == 0 {
        st.rng = 0x1234_5678;
        st.left = -1;
    }
    st.acc += time.delta_secs() * 60.0;
    while st.acc >= 1.0 {
        st.acc -= 1.0;
        // The thunder clap's countdown runs regardless.
        if let Some((n, id)) = st.sound.as_mut() {
            *n -= 1;
            if *n <= 0 {
                crate::sfx::play(*id, None);
                st.sound = None;
            }
        }
        if cutscene.active() {
            continue;
        }
        if st.left < 0 {
            let r = rand(&mut st);
            if (r % spec.chance) != 0 {
                continue;
            }
            // A strike.
            st.left = ((rand(&mut st) >> 8) % 20 + 10) as i32;
            let jitter = (rand(&mut st) as f32 * 2.328_306_4e-10 * 10.0 - 5.0).trunc();
            st.colour = Vec3::new(jitter + 25.0, jitter + 50.0, jitter + 50.0);
            let near = rand(&mut st) >> 8 & 1 != 0;
            let (frames, id) = if near {
                ((rand(&mut st) & 0x1F) as i32 + 0x1E, if rand(&mut st) >> 8 & 1 == 0 { 0x1D9 } else { 0x1D8 })
            } else {
                ((rand(&mut st) & 0x3F) as i32 + 0x3C, if rand(&mut st) >> 8 & 1 == 0 { 0x1D6 } else { 0x1D5 })
            };
            st.sound = Some((frames, id));
            spawn_flashes(&mut commands, &game, &cams, &mut st, &mut meshes, &mut images, &mut materials);
        } else {
            st.left -= 1;
            if st.left < 10 {
                st.colour = (st.colour - Vec3::splat(10.0)).max(Vec3::ZERO);
            }
        }
    }
    let on = st.left >= 0;
    for (e, f, mut vis) in &mut flashes {
        if !on {
            commands.entity(e).despawn();
            continue;
        }
        let shown = rand(&mut st) >> 8 & 1 != 0;
        *vis = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if let Some(mut m) = materials.get_mut(&f.material) {
            let c = st.colour / 128.0;
            m.base_color = Color::linear_rgb(c.x, c.y, c.z);
        }
    }
    if st.left == 0 {
        st.left = -1;
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_flashes(
    commands: &mut Commands,
    game: &Game,
    cams: &Query<&Transform, With<crate::player::FollowCam>>,
    st: &mut State,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) {
    let data = &game.data;
    let Some((_, model)) = data.models.iter().find(|(h, _)| *h & 0x00FF_FFFF == FLASH_MODEL) else { return };
    let Ok(cam) = cams.single() else { return };
    let ahead = (cam.forward().as_vec3() * Vec3::new(1.0, 0.0, 1.0)).normalize_or(Vec3::Z);
    // Up in the sky ahead of the camera (mm).
    let base = cam.translation / SCALE + ahead * 120_000.0 + Vec3::Y * 50_000.0;
    let parts = mgs2_bevy::scene::model_meshes(model);
    for i in 0..5 {
        let mut at = base;
        if i > 0 {
            let dx = rand(st) as f32 * 4.656_613e-10 - 1.0;
            let dz = rand(st) as f32 * 4.656_613e-10 - 1.0;
            at += Vec3::new(dx, 0.0, dz) * 50_000.0;
        }
        let turn = (rand(st) & 0xFFF) as f32 / 4096.0 * std::f32::consts::TAU;
        let place = Transform::from_translation(at * SCALE).with_rotation(Quat::from_rotation_y(turn));
        let Some((tex, mesh, _)) = parts.first() else { return };
        let material = materials.add(StandardMaterial {
            base_color_texture: data.textures.get(tex).map(|t| images.add(mgs2_bevy::scene::image(t))),
            base_color: Color::BLACK,
            unlit: true,
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            double_sided: true,
            fog_enabled: false,
            ..default()
        });
        commands.spawn((crate::LevelEntity, Mesh3d(meshes.add(mesh.clone())), MeshMaterial3d(material.clone()), Flash { material }, place, Visibility::Hidden));
    }
}
