//! `.mar` motion archives (file type 12, letter `m`, magic `MARa`).
//!
//! Loader `FUN_0011C8F8` (pointer fix-up). The player lives in each stage's
//! resident image (w00a addresses): start `FUN_003B0BA0`, per-motion init
//! `FUN_003B1110`, per-tick update `FUN_003B13F8`.
//!
//! ```text
//! archive: "MARa", u32 version (0x15), u32 count, u32 base
//!          count x { u32 offset (from base), u32 ? }
//! motion (offsets below are in u16 units from the data pointer):
//!   +00 u32 id            +04 u32 flags       +08 u32 step
//!   +0C u32 length (frames)                    +10 u32 data offset (bytes)
//!   +14 u32 ?             +18 u32 joint count
//!   +1C u32 track 0x10    +20 u32 root track (0x01 / 0x20)
//!   +24 u32 ?             +28 u32 track 0x04
//!   +2C u32 joint rotation tracks [joint count]
//!       u32 joint translation tracks [joint count] (flag 0x40)
//! ```
//!
//! Flags: 0x01 root deltas, 0x02 joint rotations, 0x04 / 0x10 unknown
//! single tracks, 0x20 root Hermite curve, 0x40 joint Hermite curves;
//! bits 16..19 pick the left/right mirror table (joint counts 16/21/33).
//!
//! Joint rotation keys (`FUN_003B2BE0`, `FUN_003B2D40`), read from a stream of
//! u16 words 32 bits at a time:
//! `u8 duration` (frames; bit 7 = no second quaternion), quaternion `a`
//! (4 x signed 12-bit / 2047, x y z w), quaternion `b` unless bit 7 (then
//! `b = a`). Playback (`FUN_003B2918`) blends consecutive keys with
//! `squad`: `nlerp(2t(1-t), nlerp(a0, a1, t), nlerp(b0, b1, t))`.
//! Only joint rotations are decoded so far.

use crate::{Result, format_err};

pub type Quat = [f32; 4];

#[derive(Clone, Debug)]
pub struct Mar {
    pub motions: Vec<Motion>,
}

#[derive(Clone, Debug)]
pub struct Motion {
    pub id: u32,
    pub flags: u32,
    pub step: u32,
    /// Length in frames (60 per second).
    pub length: u32,
    /// Per-joint rotation keys (empty when flag 0x02 is clear).
    pub joints: Vec<Vec<RotKey>>,
    /// Root position keys (flag 0x20), in millimetres. Empty when absent.
    pub root: Vec<RootKey>,
}

/// Root (hips) position. Values in the file are in units of 100 mm, which
/// gives standing / crouching / prone hip heights of 0.8 / 0.42 / 0.14 m.
#[derive(Clone, Copy, Debug)]
pub struct RootKey {
    pub frame: f32,
    pub position: [f32; 3],
}

/// Scale from root-track units to millimetres (inferred from hip heights).
pub const ROOT_UNIT_MM: f32 = 100.0;

#[derive(Clone, Copy, Debug)]
pub struct RotKey {
    /// Frame at which the track reaches this key.
    pub frame: f32,
    pub a: Quat,
    pub b: Quat,
}

struct Bits<'a> {
    d: &'a [u8],
    word: usize,
    bit: u32,
}

impl Bits<'_> {
    fn read(&mut self, n: u32) -> Result<u32> {
        let at = self.word * 2;
        let Some(b) = self.d.get(at..at + 4) else {
            return format_err("mar: bitstream past end");
        };
        let v = u32::from_le_bytes(b.try_into().unwrap()) >> self.bit & ((1 << n) - 1);
        self.bit += n;
        self.word += (self.bit >> 4) as usize;
        self.bit &= 15;
        Ok(v)
    }

    fn quat(&mut self) -> Result<Quat> {
        let mut q = [0.0; 4];
        for c in &mut q {
            let v = self.read(12)? as i32;
            *c = ((v << 20) >> 20) as f32 / 2047.0;
        }
        Ok(q)
    }
}

pub fn parse(data: &[u8]) -> Result<Mar> {
    let u32_at = |o: usize| {
        data.get(o..o + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| crate::Error::Format(format!("mar: read past end at {o:#x}")))
    };
    if data.get(..4) != Some(b"MARa") {
        return format_err("mar: bad magic");
    }
    let (count, base) = (u32_at(8)? as usize, u32_at(0xC)? as usize);
    let mut motions = Vec::with_capacity(count);
    for i in 0..count {
        let m = base + u32_at(0x10 + i * 8)? as usize;
        let flags = u32_at(m + 4)?;
        let (step, length) = (u32_at(m + 8)?, u32_at(m + 0xC)?);
        let data_at = m + u32_at(m + 0x10)? as usize;
        let njoints = u32_at(m + 0x18)? as usize;
        let mut joints = Vec::new();
        if flags & 2 != 0 {
            for j in 0..njoints {
                let off = u32_at(m + 0x2C + j * 4)? as usize;
                joints.push(rotation_keys(data, data_at + off * 2, length)?);
            }
        }
        let root = if flags & 0x20 != 0 {
            root_keys(data, data_at + u32_at(m + 0x20)? as usize * 2, length)?
        } else {
            Vec::new()
        };
        motions.push(Motion { id: u32_at(m)?, flags, step, length, joints, root });
    }
    Ok(Mar { motions })
}

fn rotation_keys(data: &[u8], at: usize, length: u32) -> Result<Vec<RotKey>> {
    if at % 2 != 0 {
        return format_err("mar: misaligned track");
    }
    let mut bits = Bits { d: data, word: at / 2, bit: 0 };
    let mut keys = Vec::new();
    let mut frame = 0u32;
    // A key's duration is the length of the segment that ends at it; the
    // track ends with the key reaching the motion length (the player then
    // rewinds to the first key).
    while frame < length.max(1) && keys.len() < 4096 {
        let d = bits.read(8)?;
        let a = bits.quat()?;
        let (dur, b) = if d & 0x80 != 0 { (d & 0x7F, a) } else { (d, bits.quat()?) };
        frame += dur;
        keys.push(RotKey { frame: frame as f32, a, b });
        if dur == 0 {
            break;
        }
    }
    Ok(keys)
}

/// IEEE half-precision to f32 (`FUN_003B2F18`, which flushes denormals).
pub fn f16(v: u16) -> f32 {
    let e = u32::from(v >> 10 & 0x1F);
    let bits = if e == 0 {
        u32::from(v & 0x8000) << 16
    } else {
        u32::from(v & 0x8000) << 16 | (e + 112) << 23 | u32::from(v & 0x3FF) << 13
    };
    f32::from_bits(bits)
}

/// Root curve (`FUN_003B1650`, `FUN_003B1E50`): 3 x fp16 start position, then
/// keys of `u8 duration, 3 x fp16 position, 3 x fp16 tangent`. Tangents are
/// ignored; positions are interpolated linearly.
fn root_keys(data: &[u8], at: usize, length: u32) -> Result<Vec<RootKey>> {
    let mut bits = Bits { d: data, word: at / 2, bit: 0 };
    let read3 = |bits: &mut Bits| -> Result<[f32; 3]> {
        let mut p = [0.0; 3];
        for c in &mut p {
            *c = f16(bits.read(16)? as u16) * ROOT_UNIT_MM;
        }
        Ok(p)
    };
    let mut keys = vec![RootKey { frame: 0.0, position: read3(&mut bits)? }];
    let mut frame = 0;
    while frame < length.max(1) && keys.len() < 4096 {
        let d = bits.read(8)?;
        let position = read3(&mut bits)?;
        read3(&mut bits)?; // tangent
        frame += d;
        keys.push(RootKey { frame: frame as f32, position });
        if d == 0 {
            break;
        }
    }
    Ok(keys)
}

fn nlerp(t: f32, p: Quat, mut q: Quat) -> Quat {
    let dot: f32 = (0..4).map(|i| p[i] * q[i]).sum();
    if dot < 0.0 {
        q = q.map(|c| -c);
    }
    let mut r: Quat = std::array::from_fn(|i| p[i] * (1.0 - t) + q[i] * t);
    let len = r.iter().map(|c| c * c).sum::<f32>().sqrt().max(1e-6);
    r.iter_mut().for_each(|c| *c /= len);
    r
}

impl Motion {
    /// Root position (mm) at `frame`, looping; `None` without a root track.
    pub fn root_position(&self, frame: f32) -> Option<[f32; 3]> {
        let keys = &self.root;
        let f = if self.length > 0 { frame.rem_euclid(self.length as f32) } else { 0.0 };
        let n = keys.partition_point(|k| k.frame <= f);
        let k0 = keys.get(n.checked_sub(1)?)?;
        let Some(k1) = keys.get(n).filter(|k1| k1.frame > k0.frame) else {
            return Some(k0.position);
        };
        let t = (f - k0.frame) / (k1.frame - k0.frame);
        Some(std::array::from_fn(|i| k0.position[i] + (k1.position[i] - k0.position[i]) * t))
    }

    /// Joint rotation (x, y, z, w) at `frame`, looping over the motion length.
    pub fn rotation(&self, joint: usize, frame: f32) -> Option<Quat> {
        let keys = self.joints.get(joint)?;
        let first = keys.first()?;
        let f = if self.length > 0 { frame.rem_euclid(self.length as f32) } else { 0.0 };
        // Segment k0 -> k1 with k0.frame <= f < k1.frame.
        let n = keys.partition_point(|k| k.frame <= f);
        if n == 0 {
            return Some(first.a);
        }
        let (k0, k1) = (&keys[n - 1], keys.get(n).unwrap_or(&keys[n - 1]));
        if k1.frame <= k0.frame {
            return Some(k0.a);
        }
        let t = ((f - k0.frame) / (k1.frame - k0.frame)).clamp(0.0, 1.0);
        let qa = nlerp(t, k0.a, k1.a);
        let qb = nlerp(t, k0.b, k1.b);
        Some(nlerp(2.0 * t * (1.0 - t), qa, qb))
    }
}
