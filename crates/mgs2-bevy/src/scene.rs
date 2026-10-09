//! Converts parsed stage data into Bevy meshes, images and materials.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use mgs2_formats::kms;

use crate::stage::{StageData, Texture};

/// Game units are millimetres.
pub const SCALE: f32 = 0.001;

/// Materials for one stage, keyed by texture hash (0 = untextured).
#[derive(Default)]
pub struct MaterialCache {
    pub by_texture: HashMap<u32, Handle<StandardMaterial>>,
    pub missing: Vec<u32>,
    /// Face to cull (`None`: draw both sides).
    pub cull: Option<bevy::render::render_resource::Face>,
}

pub fn alpha_mode(t: &Texture) -> AlphaMode {
    let (mut cut, mut partial) = (false, false);
    for a in t.rgba.as_chunks::<4>().0.iter().map(|p| p[3]) {
        match a {
            0..=8 => cut = true,
            247..=255 => {}
            _ => partial = true,
        }
    }
    if partial {
        AlphaMode::Blend
    } else if cut {
        AlphaMode::Mask(0.5)
    } else {
        AlphaMode::Opaque
    }
}

pub fn image(t: &Texture) -> Image {
    let mut img = Image::new(
        Extent3d { width: t.width, height: t.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        t.rgba.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

impl MaterialCache {
    pub fn get(
        &mut self,
        hash: u32,
        data: &StageData,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
        unlit: bool,
    ) -> Handle<StandardMaterial> {
        if let Some(h) = self.by_texture.get(&hash) {
            return h.clone();
        }
        let mat = match data.textures.get(&hash) {
            Some(t) => StandardMaterial {
                base_color_texture: Some(images.add(image(t))),
                alpha_mode: alpha_mode(t),
                unlit,
                cull_mode: self.cull,
                double_sided: self.cull.is_none(),
                perceptual_roughness: 0.9,
                ..default()
            },
            None => {
                if hash != 0 {
                    self.missing.push(hash);
                }
                StandardMaterial {
                    base_color: if hash == 0 { Color::srgb(0.6, 0.6, 0.6) } else { Color::srgb(1.0, 0.0, 1.0) },
                    unlit,
                    cull_mode: self.cull,
                    double_sided: self.cull.is_none(),
                    ..default()
                }
            }
        };
        let h = materials.add(mat);
        self.by_texture.insert(hash, h.clone());
        h
    }
}

/// Which kms object/mesh a spawned Bevy mesh came from (for skinning).
#[derive(Component, Clone, Copy)]
pub struct Part {
    pub object: usize,
    pub mesh: usize,
}

/// One Bevy mesh per kms mesh, in bind pose, in model space (metres).
pub fn model_meshes(model: &kms::Model) -> Vec<(u32, Mesh, Part)> {
    let offsets = model.bind_offsets();
    let mut out = Vec::new();
    for (o, obj) in model.objects.iter().enumerate() {
        let d = Vec3::from(offsets[o]);
        for (mi, m) in obj.meshes.iter().enumerate() {
            if m.indices.is_empty() {
                continue;
            }
            let pos: Vec<[f32; 3]> = m.positions.iter().map(|p| ((Vec3::from(*p) + d) * SCALE).into()).collect();
            let n = pos.len();
            // Kept in the main world too so skinning can rewrite positions.
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            let normals = if m.normals.len() == n { m.normals.clone() } else { vec![[0.0, 1.0, 0.0]; n] };
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
            let uvs = if m.uvs.len() == n { m.uvs.clone() } else { vec![[0.0, 0.0]; n] };
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
            mesh.insert_indices(Indices::U32(m.indices.clone()));
            out.push((m.textures[0], mesh, Part { object: o, mesh: mi }));
        }
    }
    out
}

/// Gives a map piece's meshes the vertex colours its lt2 lighting gives them
/// (`mgs2_formats::lt2`; the models have none of their own). `at` is the
/// piece's placement (translation in metres, as on its entity). Colours are
/// scaled so 128 = 1.0.
pub fn light_meshes(model: &kms::Model, parts: &mut [(u32, Mesh, Part)], lt2: &mgs2_formats::lt2::Lt2, place: Transform) {
    let (rot, at) = (place.rotation, place.translation / SCALE);
    use bevy::mesh::VertexAttributeValues as V;
    let objects = parts.iter().map(|p| p.2.object).max().map_or(0, |m| m + 1);
    for o in 0..objects.min(model.objects.len()) {
        let world = |m: &Mesh| -> Vec<Vec3> {
            match m.attribute(Mesh::ATTRIBUTE_POSITION) {
                Some(V::Float32x3(p)) => p.iter().map(|v| rot * Vec3::from(*v) / SCALE + at).collect(),
                _ => Vec::new(),
            }
        };
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for (_, m, _) in parts.iter().filter(|p| p.2.object == o) {
            for p in world(m) {
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        if lo.x > hi.x {
            continue;
        }
        let lights = lt2.gather(lo.into(), hi.into());
        for (_, m, _) in parts.iter_mut().filter(|p| p.2.object == o) {
            let pos = world(m);
            let nrm: Vec<[f32; 3]> = match m.attribute(Mesh::ATTRIBUTE_NORMAL) {
                Some(V::Float32x3(n)) => n.clone(),
                _ => vec![[0.0, 1.0, 0.0]; pos.len()],
            };
            let col: Vec<[f32; 4]> = pos
                .iter()
                .zip(&nrm)
                .map(|(p, n)| {
                    let c = lt2.shade((*p).into(), (rot * Vec3::from(*n)).into(), &lights);
                    [c[0] / 128.0, c[1] / 128.0, c[2] / 128.0, 1.0]
                })
                .collect();
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
        }
    }
}

/// Spawns a model as children of a new entity and returns that entity.
#[allow(clippy::too_many_arguments)]
pub fn spawn_model(
    commands: &mut Commands,
    model: &kms::Model,
    data: &StageData,
    cache: &mut MaterialCache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    unlit: bool,
    root: impl Bundle,
) -> Entity {
    let parent = commands.spawn((root, Transform::default(), Visibility::default())).id();
    for (tex, mesh, part) in model_meshes(model) {
        let mat = cache.get(tex, data, images, materials, unlit);
        let child = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat), part)).id();
        commands.entity(parent).add_child(child);
    }
    parent
}

/// [`spawn_model`] for a model placed at `place`, lit by the stage's lighting
/// (its first lt2; gameplay stages have one) the way map pieces are.
#[allow(clippy::too_many_arguments)]
pub fn spawn_model_lit(
    commands: &mut Commands,
    model: &kms::Model,
    data: &StageData,
    cache: &mut MaterialCache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    unlit: bool,
    root: impl Bundle,
    place: Transform,
) -> Entity {
    let mut parts = model_meshes(model);
    if let Some((_, l)) = data.lights.first().filter(|_| std::env::var_os("MGS2_NO_LT2").is_none()) {
        light_meshes(model, &mut parts, l, place);
    }
    let parent = commands.spawn((root, place, Visibility::default())).id();
    for (tex, mesh, part) in parts {
        let mat = cache.get(tex, data, images, materials, unlit);
        let child = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat), part)).id();
        commands.entity(parent).add_child(child);
    }
    parent
}

/// Spawns every script-placed level piece of the stage; each root gets
/// `marker` and the piece's translation. Returns the spawned roots.
#[allow(clippy::too_many_arguments)]
pub fn spawn_level(
    commands: &mut Commands,
    data: &StageData,
    cache: &mut MaterialCache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    unlit: bool,
    marker: impl Bundle + Clone,
) -> Vec<Entity> {
    let mut out = Vec::new();
    for piece in &data.pieces {
        let Some((_, model)) = data.models.iter().find(|(h, _)| *h == piece.model) else { continue };
        let place = Transform::from_translation(Vec3::from(piece.position) * SCALE);
        let mut parts = model_meshes(model);
        let lt2 = piece.lt2.and_then(|h| data.lights.iter().find(|(x, _)| *x == h)).map(|(_, l)| l);
        if let Some(l) = lt2.filter(|_| std::env::var_os("MGS2_NO_LT2").is_none()) {
            light_meshes(model, &mut parts, l, place);
        }
        let e = commands.spawn((marker.clone(), place, Visibility::default())).id();
        for (tex, mesh, part) in parts {
            let mat = cache.get(tex, data, images, materials, unlit);
            let child = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat), part)).id();
            commands.entity(e).add_child(child);
        }
        out.push(e);
    }
    out
}

/// Centre and radius (metres) of a model's bounding box.
pub fn bounds(model: &kms::Model) -> (Vec3, f32) {
    let (b, o) = (model.bbox, Vec3::from(model.origin));
    let (min, max) = ((Vec3::new(b[0], b[1], b[2]) + o) * SCALE, (Vec3::new(b[3], b[4], b[5]) + o) * SCALE);
    ((min + max) / 2.0, ((max - min).length() / 2.0).max(0.1))
}
