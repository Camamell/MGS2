//! The Codec's faces (FACE.DAT, see `mgs2_formats::face`).
//!
//! Each stage has a face pack (`f00a` for w00a...) with the speakers' 3D
//! heads: Snake's `sna_radio_mh_mt.evm`, Otacon's `otc_radio_mh_mt.evm`,
//! `otc_hair_mh.evm` and `otc_glasses_mt.kms`, with their texture sets. The
//! heads are drawn on their own render layer, each by a camera into an image
//! shown on the Codec screen (Snake left, Otacon right), the speaker of the
//! line on screen lit and the other dimmed. Their motions (`cdc_*.mar`) and
//! lip data (`lip_*.rmt`) are not used yet: the heads hold their bind pose.

use std::collections::HashMap;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, RenderTarget};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use mgs2_bevy::scene::{self, SCALE};
use mgs2_bevy::stage::Texture;
use mgs2_formats::face::{self, FaceFile};
use mgs2_formats::str_code;

use crate::codec::{CodecData, CodecState};

/// The render layer the heads live on.
const LAYER: usize = 5;
/// (speaker, the face pack's files of his head).
const HEADS: [(u32, &[&str]); 2] = [(0x28DCE6, &["sna_radio_mh_mt"]), (0x4AB569, &["otc_radio_mh_mt", "otc_hair_mh", "otc_glasses_mt"])];
/// Where the heads are put, far from any stage (m).
const PLACE: [Vec3; 2] = [Vec3::new(-2000.0, -2000.0, 0.0), Vec3::new(2000.0, -2000.0, 0.0)];
const SIZE: u32 = 256;

#[derive(Component)]
pub struct FacePart;

/// A portrait on the Codec screen (index into `HEADS`).
#[derive(Component)]
pub struct Portrait(usize);

#[derive(Component)]
pub struct FaceCamera;

pub fn setup_portraits(mut commands: Commands) {
    for (i, left) in [percent(10), percent(72)].into_iter().enumerate() {
        commands.spawn((
            Portrait(i),
            ImageNode::default(),
            Node { position_type: PositionType::Absolute, left, bottom: percent(38), width: px(SIZE as f32), height: px(SIZE as f32), ..default() },
            BorderColor::all(Color::srgb(0.3, 0.9, 0.5)),
            Visibility::Hidden,
            GlobalZIndex(6),
        ));
    }
}

/// The face pack for a stage (`w01c` -> `f01c`).
fn pack_name(stage: &str) -> Option<String> {
    stage.strip_prefix('w').map(|rest| format!("f{rest}"))
}

/// Loads the stage's face pack and builds the heads and their cameras.
#[allow(clippy::too_many_arguments)]
pub fn load(
    mut commands: Commands,
    game: Res<crate::Game>,
    data: Option<Res<CodecData>>,
    parts: Query<Entity, Or<(With<FacePart>, With<FaceCamera>)>>,
    mut portraits: Query<(&Portrait, &mut ImageNode)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut stage: Local<String>,
) {
    if *stage == game.data.name {
        return;
    }
    *stage = game.data.name.clone();
    let Some(data) = data else { return };
    for e in &parts {
        commands.entity(e).despawn();
    }
    let Some(name) = pack_name(&game.data.name) else { return };
    let files = match mgs2_formats::iso::Iso::open(&data.iso).and_then(|iso| iso.open_file("MGS2/FACE.DAT")).map_err(anyhow::Error::from).and_then(|mut f| Ok(face::read(&mut f, &name)?)) {
        Ok(f) if !f.is_empty() => f,
        Ok(_) => return,
        Err(e) => {
            warn!("codec faces {name}: {e:#}");
            return;
        }
    };
    let textures: HashMap<u32, Texture> = files
        .iter()
        .filter(|f| f.type_id == 19)
        .filter_map(|f| mgs2_formats::tri::parse(&f.data).ok())
        .flatten()
        .map(|t| (t.hash, Texture { width: t.width, height: t.height, rgba: t.rgba }))
        .collect();
    let mut mats: HashMap<u32, Handle<StandardMaterial>> = HashMap::new();
    let mut material = |hash: u32, images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>| {
        mats.entry(hash)
            .or_insert_with(|| {
                let tex = textures.get(&hash);
                materials.add(StandardMaterial {
                    base_color_texture: tex.map(|t| images.add(scene::image(t))),
                    base_color: if tex.is_some() { Color::WHITE } else { Color::srgb(0.6, 0.6, 0.6) },
                    alpha_mode: tex.map_or(AlphaMode::Opaque, scene::alpha_mode),
                    unlit: true,
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                })
            })
            .clone()
    };
    let find = |t: u8, n: &str| -> Option<&FaceFile> { files.iter().find(|f| f.type_id == t && f.hash == str_code(n.as_bytes())) };
    let mut built = 0;
    for (i, (who, names)) in HEADS.iter().enumerate() {
        let root = commands.spawn((FacePart, Transform::from_translation(PLACE[i]), Visibility::Visible, RenderLayers::layer(LAYER))).id();
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        // The head bone (12 in the standard skeleton) of the first evm: kms
        // parts (glasses) hang from it, and the camera frames it.
        let mut head: Option<Vec3> = None;
        for n in *names {
            let mut add = |mesh: Mesh, tex: u32, at: Vec3, commands: &mut Commands| {
                if let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
                    for v in p {
                        lo = lo.min(Vec3::from(*v));
                        hi = hi.max(Vec3::from(*v));
                    }
                }
                let mat = material(tex, &mut images, &mut materials);
                let child = commands.spawn((FacePart, Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat), Transform::from_translation(at), RenderLayers::layer(LAYER))).id();
                commands.entity(root).add_child(child);
            };
            if let Some(f) = find(4, n) {
                match mgs2_formats::evm::parse(&f.data) {
                    Ok(m) => {
                        if head.is_none() {
                            head = m.bones.get(12).map(|b| Vec3::from(b.position) * SCALE);
                        }
                        for (tex, mesh, _) in crate::demo::evm_meshes(&m) {
                            add(mesh, tex, Vec3::ZERO, &mut commands);
                        }
                    }
                    Err(e) => warn!("codec face {n}: {e}"),
                }
            } else if let Some(f) = find(10, n) {
                match mgs2_formats::kms::parse(&f.data) {
                    Ok(m) => {
                        for (tex, mesh, _) in scene::model_meshes(&m) {
                            add(mesh, tex, head.unwrap_or(Vec3::ZERO), &mut commands);
                        }
                    }
                    Err(e) => warn!("codec face {n}: {e}"),
                }
            }
        }
        if lo.x > hi.x {
            continue;
        }
        built += 1;
        // Frame the head from the front (+z).
        let head = head.unwrap_or(Vec3::new(0.0, hi.y - 0.15, 0.0));
        let target = PLACE[i] + head + Vec3::new(0.0, 0.0, 0.03);
        let eye = target + Vec3::new(0.0, 0.02, 0.85);
        let image = images.add(Image::new_target_texture(SIZE, SIZE, TextureFormat::Rgba8UnormSrgb, None));
        commands.spawn((
            FaceCamera,
            Camera3d::default(),
            Camera { order: -2 - i as isize, clear_color: ClearColorConfig::Custom(Color::srgb(0.02, 0.12, 0.06)), is_active: false, ..default() },
            RenderTarget::from(image.clone()),
            Projection::Perspective(PerspectiveProjection { fov: 30f32.to_radians(), ..default() }),
            Transform::from_translation(eye).looking_at(target, Vec3::Y),
            RenderLayers::layer(LAYER),
        ));
        for (p, mut node) in &mut portraits {
            if p.0 == i {
                node.image = image.clone();
            }
        }
        debug!("codec face {:06x}: {:?}..{:?}", who, lo / SCALE, hi / SCALE);
    }
    info!("codec faces {name}: {built} heads");
}

/// Shows the portraits (and runs their cameras) while a call is open, the
/// line's speaker lit.
pub fn show(
    state: Res<CodecState>,
    mut portraits: Query<(&Portrait, &mut Visibility, &mut ImageNode)>,
    mut cams: Query<&mut Camera, With<FaceCamera>>,
) {
    let speaker = state.call.as_ref().map(|c| c.lines[c.index.min(c.lines.len() - 1)].0 & 0x00FF_FFFF);
    let open = state.call.is_some();
    for mut c in &mut cams {
        if c.is_active != open {
            c.is_active = open;
        }
    }
    for (p, mut v, mut node) in &mut portraits {
        let want = if open && node.image != Handle::default() { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
        let lit = speaker == Some(HEADS[p.0].0);
        node.color = if lit { Color::WHITE } else { Color::srgb(0.45, 0.5, 0.45) };
    }
}
