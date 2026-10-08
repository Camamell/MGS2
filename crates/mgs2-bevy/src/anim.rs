//! CPU skinning of kms models posed by mar motions.

use bevy::math::{Affine3A, Quat, Vec3};
use mgs2_formats::kms;
use mgs2_formats::mar::Motion;

/// Model-space matrix (millimetres) of every object. Joints without a track
/// keep their bind orientation. The motion's root track supplies the hips
/// height (horizontal root motion is left to the caller).
pub fn pose(model: &kms::Model, motion: Option<&Motion>, frame: f32, conjugate: bool) -> Vec<Affine3A> {
    let root_y = motion.and_then(|m| m.root_position(frame)).map_or(0.0, |p| p[1]);
    let mut out: Vec<Affine3A> = Vec::with_capacity(model.objects.len());
    for (i, o) in model.objects.iter().enumerate() {
        let rot = motion
            .and_then(|m| m.rotation(i, frame))
            .map(|q| {
                let q = Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize();
                if conjugate { q.conjugate() } else { q }
            })
            .unwrap_or(Quat::IDENTITY);
        let mut offset = Vec3::from(o.offset);
        if o.parent < 0 {
            offset += Vec3::from(model.origin);
            offset.y += root_y;
        }
        let local = Affine3A::from_rotation_translation(rot, offset);
        let parent = usize::try_from(o.parent).ok().and_then(|p| out.get(p).copied()).unwrap_or(Affine3A::IDENTITY);
        out.push(parent * local);
    }
    out
}

/// Skinned positions (millimetres) and normals of one mesh:
/// `w * M_self * v + (1 - w) * M_parent * (v + offset_self)`.
pub fn skin(model: &kms::Model, obj: usize, mesh: usize, mats: &[Affine3A]) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let o = &model.objects[obj];
    let m = &o.meshes[mesh];
    let me = mats[obj];
    let parent = usize::try_from(o.parent).ok().and_then(|p| mats.get(p).copied());
    let off = Vec3::from(o.offset);
    let mut pos = Vec::with_capacity(m.positions.len());
    let mut nrm = Vec::with_capacity(m.positions.len());
    for (i, p) in m.positions.iter().enumerate() {
        let v = Vec3::from(*p);
        let w = (f32::from(m.position_w[i]) / 4096.0).clamp(0.0, 1.0);
        let a = me.transform_point3(v);
        let (world, rot) = match parent {
            Some(pm) if w < 1.0 => {
                let b = pm.transform_point3(v + off);
                (a * w + b * (1.0 - w), if w >= 0.5 { me } else { pm })
            }
            _ => (a, me),
        };
        pos.push(world.into());
        let n = m.normals.get(i).map_or(Vec3::Y, |n| Vec3::from(*n));
        nrm.push(rot.transform_vector3(n).normalize_or(Vec3::Y).into());
    }
    (pos, nrm)
}
