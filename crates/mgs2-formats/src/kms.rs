//! `.kms` models (file type 10, letter `k`).
//!
//! Loader: `FUN_00129ac0` (accepts magics 0xC81371, 0xB253A3, 0x4DD2C1,
//! 0x4E9AE2) and pointer fix-up `FUN_001295d8`.
//!
//! ```text
//! header (0x40)
//!   +00 u32 magic
//!   +04 u32 ?            (1..5)
//!   +08 u32 object count
//!   +0C u32 texture set  (str_code of the .tri this model uses)
//!   +10 u32 ?, +14 u32 ?
//!   +18 ..  bounding box (min xyz, max xyz) at +1C
//!   +34 f32 origin[3]    translation of the whole model (level geometry is
//!                        stored centred; verified against .hzx collision)
//! objects (0x40 each, from +0x40)
//!   +00 u32 flags
//!   +04 u32 mesh count
//!   +08 f32 bbox[6]
//!   +20 f32 offset[3]    translation from the parent object (bind pose)
//!   +2C i32 parent object (-1 = root)
//!   +30 u32 offset of the object's meshes
//! meshes (0x30 each)
//!   +00 u32 flags        (0x48 common; 0x2F8/0x2F9 carry a second UV set)
//!   +04 u32 vertex count
//!   +08 u32 texture hash[3]  (0 = none)
//!   +14 u32 offsets: positions, normals, uvs, uvs2, ?, ?
//! ```
//!
//! Vertices form triangle strips. Positions are `i16 x,y,z,w` in world units
//! (millimetres, Y up). Normals are `i16 x,y,z,w` scaled by 4096; bit 15 of `w`
//! means "no triangle ends at this vertex" (strip restart). UVs are `i16 u,v`
//! scaled by 4096.
//!
//! Objects form a bone hierarchy and vertices are in their object's local
//! space. Position `w` / 4096 is the vertex's weight on its own object; the
//! rest goes to the parent, i.e.
//! `world = w * M_self * v + (1 - w) * M_parent * (v + offset_self)`.
//! In bind pose both terms equal `bind_offset(self) + v`.

use crate::{Result, format_err};

pub const MAGICS: [u32; 4] = [0x00C8_1371, 0x00B2_53A3, 0x004D_D2C1, 0x004E_9AE2];

#[derive(Clone, Debug)]
pub struct Model {
    pub magic: u32,
    pub texture_set: u32,
    /// Model-space bounds (before `origin` is applied).
    pub bbox: [f32; 6],
    /// Translation applied to root objects.
    pub origin: [f32; 3],
    pub objects: Vec<Object>,
}

#[derive(Clone, Debug)]
pub struct Object {
    pub flags: u32,
    pub parent: i32,
    /// Translation from the parent (bind pose).
    pub offset: [f32; 3],
    pub bbox: [f32; 6],
    pub meshes: Vec<Mesh>,
}

#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub flags: u32,
    pub textures: [u32; 3],
    pub positions: Vec<[f32; 3]>,
    /// Raw `w` of each position; meaning not yet known (often 0x1000).
    pub position_w: Vec<i16>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub uvs2: Vec<[f32; 2]>,
    /// Triangle list built from the strips (indices into the vertex arrays).
    pub indices: Vec<u32>,
}

impl Model {
    /// Position of each object's origin in bind pose, including the model
    /// origin.
    pub fn bind_offsets(&self) -> Vec<[f32; 3]> {
        let mut out: Vec<[f32; 3]> = Vec::with_capacity(self.objects.len());
        for o in &self.objects {
            let base = usize::try_from(o.parent).ok().and_then(|p| out.get(p).copied()).unwrap_or(self.origin);
            out.push([base[0] + o.offset[0], base[1] + o.offset[1], base[2] + o.offset[2]]);
        }
        out
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn u32(&self, o: usize) -> Result<u32> {
        self.0
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| crate::Error::Format(format!("kms: read past end at {o:#x}")))
    }
    fn f32(&self, o: usize) -> Result<f32> {
        self.u32(o).map(f32::from_bits)
    }
    fn i16x(&self, o: usize, n: usize) -> Result<Vec<i16>> {
        self.0
            .get(o..o + n * 2)
            .map(|b| b.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect())
            .ok_or_else(|| crate::Error::Format(format!("kms: array past end at {o:#x}")))
    }
}

pub fn parse(data: &[u8]) -> Result<Model> {
    let r = Reader(data);
    let magic = r.u32(0)?;
    if !MAGICS.contains(&magic) {
        return format_err(format!("kms: bad magic {magic:#x}"));
    }
    let count = r.u32(8)? as usize;
    let bbox = std::array::from_fn(|i| r.f32(0x1C + i * 4).unwrap_or(0.0));
    let mut objects = Vec::with_capacity(count);
    for k in 0..count {
        let o = 0x40 + k * 0x40;
        let nmesh = r.u32(o + 4)? as usize;
        let moff = r.u32(o + 0x30)? as usize;
        let mut meshes = Vec::with_capacity(nmesh);
        for m in 0..nmesh {
            meshes.push(parse_mesh(&r, moff + m * 0x30)?);
        }
        objects.push(Object {
            flags: r.u32(o)?,
            parent: r.u32(o + 0x2C)? as i32,
            offset: std::array::from_fn(|i| r.f32(o + 0x20 + i * 4).unwrap_or(0.0)),
            bbox: std::array::from_fn(|i| r.f32(o + 8 + i * 4).unwrap_or(0.0)),
            meshes,
        });
    }
    let origin = std::array::from_fn(|i| r.f32(0x34 + i * 4).unwrap_or(0.0));
    Ok(Model { magic, texture_set: r.u32(0xC)?, bbox, origin, objects })
}

fn parse_mesh(r: &Reader, mo: usize) -> Result<Mesh> {
    let n = r.u32(mo + 4)? as usize;
    let ptr = |i: usize| r.u32(mo + 0x14 + i * 4).map(|p| p as usize);
    let mut mesh = Mesh {
        flags: r.u32(mo)?,
        textures: [r.u32(mo + 8)?, r.u32(mo + 0xC)?, r.u32(mo + 0x10)?],
        ..Default::default()
    };
    let pos = r.i16x(ptr(0)?, n * 4)?;
    let s = 1.0 / 4096.0;
    for v in pos.chunks_exact(4) {
        mesh.positions.push([v[0] as f32, v[1] as f32, v[2] as f32]);
        mesh.position_w.push(v[3]);
    }
    let mut restart = vec![false; n];
    if ptr(1)? != 0 {
        let nrm = r.i16x(ptr(1)?, n * 4)?;
        for (i, v) in nrm.chunks_exact(4).enumerate() {
            mesh.normals.push([v[0] as f32 * s, v[1] as f32 * s, v[2] as f32 * s]);
            restart[i] = v[3] < 0;
        }
    }
    for (slot, dst) in [(2, &mut mesh.uvs), (3, &mut mesh.uvs2)] {
        if ptr(slot)? != 0 {
            let uv = r.i16x(ptr(slot)?, n * 2)?;
            dst.extend(uv.chunks_exact(2).map(|c| [c[0] as f32 * s, c[1] as f32 * s]));
        }
    }
    // Strips: a triangle ends at every vertex not flagged as a restart; the
    // winding alternates with the vertex's position in its strip.
    let mut run = 0usize;
    for i in 0..n {
        if restart[i] || i < 2 {
            run = if restart[i] { 0 } else { run + 1 };
            continue;
        }
        run += 1;
        let (a, b, c) = (i as u32 - 2, i as u32 - 1, i as u32);
        if run % 2 == 1 {
            mesh.indices.extend_from_slice(&[a, b, c]);
        } else {
            mesh.indices.extend_from_slice(&[b, a, c]);
        }
    }
    Ok(mesh)
}
