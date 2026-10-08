//! `.evm` skinned models (file type 4, letter `e`): the cutscene meshes laid
//! over a kms skeleton (loader `FUN_00129bd8` -> `FUN_001299f8`).
//!
//! ```text
//! header (0x30)
//!   +00 u32 animated bones (the cutscene actor's joint count)
//!   +04 u32 bone records
//!   +08 f32 bbox[6]
//!   +20 u32 ?            +24 u32 texture set (.tri name)
//!   +28 u32 mesh count   +2C u32 offset of the meshes
//! bones (0x40 each, from +0x30)
//!   +04 i32 parent (-1 root)
//!   +08 f32 offset[3]    from the parent (bind pose)
//!   +14 f32 position[3]  in the model (bind pose)
//!   +20 f32 bbox min[4], +30 bbox max[4]
//! meshes (0x40 each)
//!   +00 u32 flags        +04 u32 texture hash[3]
//!   +10 u32 vertex count +14 u32 ?
//!   +18 u8 bones[8]      (0xff = unused)
//!   +20 u32 offsets: positions, normals, uvs, uvs2, ?, skin
//! ```
//!
//! Positions are `i16 x, y, z, w` in model space (bind pose), in 1/16 mm;
//! `w` bit 15 marks a strip restart. Normals are `i16 x, y, z, w` / 4096 and
//! UVs `i16 u, v, 4096, 0` / 4096 (8 bytes a vertex). The skin stream has 8
//! bytes per vertex:
//! four weights (u8, 128 = 1) and four slots (u8, 4 x index into the mesh's
//! bones).

use crate::{Result, format_err};

#[derive(Clone, Debug)]
pub struct Bone {
    pub parent: i32,
    pub offset: [f32; 3],
    pub position: [f32; 3],
}

#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub flags: u32,
    pub textures: [u32; 3],
    /// Model space, millimetres (bind pose).
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Per vertex: up to four (bone, weight).
    pub skin: Vec<[(u8, f32); 4]>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct Model {
    pub animated: usize,
    pub texture_set: u32,
    pub bones: Vec<Bone>,
    pub meshes: Vec<Mesh>,
}

fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or_else(|| crate::Error::Format(format!("evm: read past end at {o:#x}")))
}

fn f32_at(d: &[u8], o: usize) -> Result<f32> {
    u32_at(d, o).map(f32::from_bits)
}

fn i16_at(d: &[u8], o: usize) -> Result<i16> {
    d.get(o..o + 2).map(|b| i16::from_le_bytes([b[0], b[1]])).ok_or_else(|| crate::Error::Format(format!("evm: read past end at {o:#x}")))
}

pub fn parse(d: &[u8]) -> Result<Model> {
    let animated = u32_at(d, 0)? as usize;
    let nbones = u32_at(d, 4)? as usize;
    let nmesh = u32_at(d, 0x28)? as usize;
    let mt = u32_at(d, 0x2C)? as usize;
    if nbones == 0 || nbones > 512 || nmesh > 4096 || mt < 0x30 + nbones * 0x40 || mt > d.len() {
        return format_err(format!("evm: implausible header ({nbones} bones, {nmesh} meshes at {mt:#x})"));
    }
    let mut bones = Vec::with_capacity(nbones);
    for i in 0..nbones {
        let o = 0x30 + i * 0x40;
        bones.push(Bone {
            parent: u32_at(d, o + 4)? as i32,
            offset: [f32_at(d, o + 8)?, f32_at(d, o + 0xC)?, f32_at(d, o + 0x10)?],
            position: [f32_at(d, o + 0x14)?, f32_at(d, o + 0x18)?, f32_at(d, o + 0x1C)?],
        });
    }
    let mut meshes = Vec::with_capacity(nmesh);
    for i in 0..nmesh {
        let o = mt + i * 0x40;
        let n = u32_at(d, o + 0x10)? as usize;
        let list: Vec<u8> = d.get(o + 0x18..o + 0x20).map(|b| b.to_vec()).unwrap_or_default();
        let ptr = |k: usize| u32_at(d, o + 0x20 + k * 4).map(|p| p as usize);
        let mut m = Mesh { flags: u32_at(d, o)?, textures: [u32_at(d, o + 4)?, u32_at(d, o + 8)?, u32_at(d, o + 0xC)?], ..Default::default() };
        let mut restart = vec![false; n];
        let pos = ptr(0)?;
        for v in 0..n {
            let q = pos + v * 8;
            m.positions.push([f32::from(i16_at(d, q)?) / 16.0, f32::from(i16_at(d, q + 2)?) / 16.0, f32::from(i16_at(d, q + 4)?) / 16.0]);
            restart[v] = i16_at(d, q + 6)? < 0;
        }
        let s = 1.0 / 4096.0;
        if ptr(1)? != 0 {
            for v in 0..n {
                let q = ptr(1)? + v * 8;
                m.normals.push([f32::from(i16_at(d, q)?) * s, f32::from(i16_at(d, q + 2)?) * s, f32::from(i16_at(d, q + 4)?) * s]);
            }
        }
        if ptr(2)? != 0 {
            for v in 0..n {
                let q = ptr(2)? + v * 8;
                m.uvs.push([f32::from(i16_at(d, q)?) * s, f32::from(i16_at(d, q + 2)?) * s]);
            }
        }
        let skin = ptr(5)?;
        for v in 0..n {
            let q = skin + v * 8;
            let mut w = [(0u8, 0.0f32); 4];
            for (k, slot) in w.iter_mut().enumerate() {
                let (Some(&wt), Some(&sl)) = (d.get(q + k), d.get(q + 4 + k)) else { break };
                let bone = list.get(usize::from(sl / 4)).copied().unwrap_or(0xFF);
                *slot = (bone, f32::from(wt) / 128.0);
            }
            if skin == 0 || w.iter().all(|(_, x)| *x == 0.0) {
                w = [(list.first().copied().unwrap_or(0), 1.0), (0, 0.0), (0, 0.0), (0, 0.0)];
            }
            m.skin.push(w);
        }
        let mut run = 0usize;
        for v in 0..n {
            if restart[v] || v < 2 {
                run = if restart[v] { 0 } else { run + 1 };
                continue;
            }
            run += 1;
            let (a, b, c) = (v as u32 - 2, v as u32 - 1, v as u32);
            if run % 2 == 1 {
                m.indices.extend_from_slice(&[a, b, c]);
            } else {
                m.indices.extend_from_slice(&[b, a, c]);
            }
        }
        meshes.push(m);
    }
    Ok(Model { animated, texture_set: u32_at(d, 0x24)?, bones, meshes })
}
