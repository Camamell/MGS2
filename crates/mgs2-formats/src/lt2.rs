//! `.lt2` stage lighting (file type 11, letter `l`), named by map pieces
//! (`chara 0x35a2cf ... -l <lt2>`).
//!
//! The engine registers each lt2 (`FUN_00125fd0`, 16 slots at `0x1b9a50`)
//! and lights the vertices of the map pieces that use it on VU0
//! (`FUN_0012c440` -> `FUN_0012b6d8` gathers the lights whose boxes touch an
//! object, `FUN_0012b1d0` runs the micro programs per vertex; the microcode
//! is the MPG in the VIF0 packet at `0x196020`). Map models carry no colours
//! of their own: this is all of their shading.
//!
//! ```text
//! header (0x20)
//!   +00 f32 dir[3], f32 ?   the directional light (light matrix 0x190a80)
//!   +10 u8  colour[3], pad  (0x190ac0)
//!   +14 u8  ambient[3], pad (0x190af0)
//!   +18 u32 box count
//! boxes (0x30 each, from +0x20)
//!   +00 f32 max[3], pad; +10 f32 min[3], pad
//!   +20 u32 light count; +24 u32 kind (1 point, 2 spot, 4 segment,
//!       0x10 shadow; 0x4000 off); +28 u32 offset of the lights
//! kind 1 (0x20): f32 pos[3], f32 luminance, u8 rgb[4], f32 half range,
//!   f32 cube (|vertex - pos| per axis), u32 flags (0x200 on, 0x8000 off)
//! kind 2 (0x50): f32 max[3],pad, min[3],pad, pos[3],pad, axis[3], half
//!   range, u8 rgb[4], f32 cos inner, f32 cos outer, u32 flags (+0x4c)
//! kind 4 (0x50): bounds as kind 2, start[3],pad, dir[3], length, u8 rgb[4],
//!   f32 half range, pad, u32 flags (+0x4c)
//! kind 0x10 (0x40): bounds, centre[3],pad, f32 half range, u32 flags (+0x34)
//! ```
//!
//! Per vertex (colours 0..255, 128 = 1.0 under the GS's texture modulate):
//! `c = ambient + colour * max(0, n . dir)`; each light within range R
//! (twice its half range) at distance d adds `rgb * 1.5 * max(0, n . v) *
//! (1 - d / R)` where `v = (vertex - light) / d` (spots also times their
//! cone ramp, segments measure from the nearest point); the first shadow
//! sphere then scales `c` by `d / R`; `c` is clamped to 255. Model normals
//! point inwards, hence `n . (vertex - light)`.

#[derive(Clone, Debug)]
pub struct Lt2 {
    pub dir: [f32; 3],
    pub color: [f32; 3],
    pub ambient: [f32; 3],
    pub boxes: Vec<LightBox>,
}

#[derive(Clone, Debug)]
pub struct LightBox {
    pub max: [f32; 3],
    pub min: [f32; 3],
    pub kind: u32,
    pub lights: Vec<Light>,
}

#[derive(Clone, Copy, Debug)]
pub enum Shape {
    Point { pos: [f32; 3], cube: f32 },
    Spot { pos: [f32; 3], axis: [f32; 3], inner: f32, outer: f32 },
    Segment { start: [f32; 3], dir: [f32; 3], length: f32 },
    Shadow { centre: [f32; 3] },
}

#[derive(Clone, Copy, Debug)]
pub struct Light {
    pub shape: Shape,
    /// Bounds the vertex must lie in (spot, segment, shadow).
    pub max: [f32; 3],
    pub min: [f32; 3],
    pub rgb: [f32; 3],
    /// Range: twice the stored half range.
    pub range: f32,
    pub flags: u32,
}

impl Light {
    pub fn on(&self) -> bool {
        self.flags & 0x8000 == 0 && self.flags & 0x200 != 0
    }

    /// Bounds of the space it can reach.
    pub fn reach(&self) -> ([f32; 3], [f32; 3]) {
        match self.shape {
            Shape::Point { pos, cube } => (pos.map(|p| p - cube), pos.map(|p| p + cube)),
            _ => (self.min, self.max),
        }
    }
}

fn f(d: &[u8], o: usize) -> f32 {
    d.get(o..o + 4).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()))
}
fn u(d: &[u8], o: usize) -> u32 {
    d.get(o..o + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
}
fn v3(d: &[u8], o: usize) -> [f32; 3] {
    [f(d, o), f(d, o + 4), f(d, o + 8)]
}
fn rgb(d: &[u8], o: usize) -> [f32; 3] {
    [0, 1, 2].map(|i| f32::from(d.get(o + i).copied().unwrap_or(0)))
}

pub fn parse(d: &[u8]) -> crate::Result<Lt2> {
    if d.len() < 0x20 {
        return crate::format_err("lt2: too short");
    }
    let count = (u(d, 0x18) as usize).min(4096);
    let mut boxes = Vec::with_capacity(count);
    for i in 0..count {
        let b = 0x20 + i * 0x30;
        if b + 0x30 > d.len() {
            return crate::format_err("lt2: box past end");
        }
        let n = (u(d, b + 0x20) as usize).min(4096);
        let kind = u(d, b + 0x24);
        let at = u(d, b + 0x28) as usize;
        let size = match kind & 0x1F {
            1 => 0x20,
            2 | 4 => 0x50,
            0x10 => 0x40,
            _ => 0,
        };
        let mut lights = Vec::new();
        if size != 0 {
            for k in 0..n {
                let o = at + k * size;
                if o + size > d.len() {
                    break;
                }
                let (max, min) = (v3(d, o), v3(d, o + 0x10));
                lights.push(match kind & 0x1F {
                    1 => Light {
                        shape: Shape::Point { pos: v3(d, o), cube: f(d, o + 0x18) },
                        max: [0.0; 3],
                        min: [0.0; 3],
                        rgb: rgb(d, o + 0x10),
                        range: 2.0 * f(d, o + 0x14),
                        flags: u(d, o + 0x1C),
                    },
                    2 => Light {
                        shape: Shape::Spot { pos: v3(d, o + 0x20), axis: v3(d, o + 0x30), inner: f(d, o + 0x44), outer: f(d, o + 0x48) },
                        max,
                        min,
                        rgb: rgb(d, o + 0x40),
                        range: 2.0 * f(d, o + 0x3C),
                        flags: u(d, o + 0x4C),
                    },
                    4 => Light {
                        shape: Shape::Segment { start: v3(d, o + 0x20), dir: v3(d, o + 0x30), length: f(d, o + 0x3C) },
                        max,
                        min,
                        rgb: rgb(d, o + 0x40),
                        range: 2.0 * f(d, o + 0x44),
                        flags: u(d, o + 0x4C),
                    },
                    _ => Light {
                        shape: Shape::Shadow { centre: v3(d, o + 0x20) },
                        max,
                        min,
                        rgb: [0.0; 3],
                        range: 2.0 * f(d, o + 0x30),
                        flags: u(d, o + 0x34),
                    },
                });
            }
        }
        boxes.push(LightBox { max: v3(d, b), min: v3(d, b + 0x10), kind, lights });
    }
    Ok(Lt2 { dir: v3(d, 0), color: rgb(d, 0x10), ambient: rgb(d, 0x14), boxes })
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn inside(p: [f32; 3], min: [f32; 3], max: [f32; 3]) -> bool {
    (0..3).all(|i| min[i] <= p[i] && p[i] <= max[i])
}
fn overlaps(amin: [f32; 3], amax: [f32; 3], bmin: [f32; 3], bmax: [f32; 3]) -> bool {
    (0..3).all(|i| amin[i] <= bmax[i] && bmin[i] <= amax[i])
}

/// Attenuated diffuse factor of a light at offset `d` (vertex - light):
/// 1.5 * max(0, n . d) / |d| * (1 - |d| / range), or None out of range.
fn falloff(n: [f32; 3], d: [f32; 3], range: f32) -> Option<f32> {
    let dist2 = dot(d, d);
    if range * range - dist2 < 0.0 {
        return None;
    }
    let dist = dist2.sqrt();
    let k = 1.5 * (range - dist) / (range * dist);
    Some(dot(n, d).max(0.0) * if k.is_finite() { k } else { 0.0 })
}

impl Lt2 {
    /// The lights that can reach an object with these world bounds (mm), in
    /// the order the engine applies them.
    pub fn gather(&self, min: [f32; 3], max: [f32; 3]) -> Vec<(u32, Light)> {
        let mut out = Vec::new();
        for kind in [1, 2, 4, 0x10] {
            for b in &self.boxes {
                if b.kind & 0x4000 != 0 || b.kind & kind == 0 || !overlaps(min, max, b.min, b.max) {
                    continue;
                }
                for l in &b.lights {
                    let (lo, hi) = l.reach();
                    if l.on() && overlaps(min, max, lo, hi) {
                        out.push((kind, *l));
                    }
                }
            }
        }
        out
    }

    /// Colour (0..255 per channel) of a vertex at `p` (mm) with the model's
    /// (inward) normal `n`, lit by `lights` from [`Lt2::gather`].
    pub fn shade(&self, p: [f32; 3], n: [f32; 3], lights: &[(u32, Light)]) -> [f32; 3] {
        let k = dot(n, self.dir).max(0.0);
        let mut c = [0, 1, 2].map(|i| self.ambient[i] + self.color[i] * k);
        let mut shadowed = false;
        for (kind, l) in lights {
            let add = match (kind, l.shape) {
                (1, Shape::Point { pos, cube }) => {
                    let d = sub(p, pos);
                    if d.iter().any(|x| x.abs() > cube) {
                        continue;
                    }
                    falloff(n, d, l.range)
                }
                (2, Shape::Spot { pos, axis, inner, outer }) => {
                    if !inside(p, l.min, l.max) {
                        continue;
                    }
                    let d = sub(p, pos);
                    let dist = dot(d, d).sqrt();
                    let along = dot(d, axis);
                    if along - outer * dist < 0.0 || dist == 0.0 {
                        continue;
                    }
                    let cone = (outer * dist - along.min(inner * dist)) / ((outer - inner) * dist);
                    falloff(n, d, l.range).map(|f| f * if cone.is_finite() { cone } else { 1.0 })
                }
                (4, Shape::Segment { start, dir, length }) => {
                    if !inside(p, l.min, l.max) {
                        continue;
                    }
                    let a = dot(dir, start);
                    let t = dot(dir, p);
                    let d = if t < a {
                        sub(p, start)
                    } else if a + length < t {
                        sub(p, [0, 1, 2].map(|i| start[i] + dir[i] * length))
                    } else {
                        sub(p, [0, 1, 2].map(|i| start[i] + dir[i] * (t - a)))
                    };
                    falloff(n, d, l.range)
                }
                (_, Shape::Shadow { centre }) => {
                    if shadowed || !(0..3).all(|i| l.min[i] < p[i] && p[i] < l.max[i]) {
                        continue;
                    }
                    // Only the first sphere whose bounds hold the vertex.
                    shadowed = true;
                    let d = sub(p, centre);
                    let dist2 = dot(d, d);
                    if l.range * l.range - dist2 < 0.0 {
                        continue;
                    }
                    let s = dist2.sqrt() / l.range;
                    c = c.map(|x| x * s);
                    continue;
                }
                _ => None,
            };
            if let Some(f) = add {
                for i in 0..3 {
                    c[i] += l.rgb[i] * f;
                }
            }
        }
        c.map(|x| x.min(255.0))
    }
}
