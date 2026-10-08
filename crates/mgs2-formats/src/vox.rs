//! VOX.DAT voice clips and SPU ADPCM.
//!
//! A clip starts on a sector (`id - 0x20000000`) with the DEMO.DAT chunk
//! layout: kind-0x10 headers, a kind-1 audio header (0x20 bytes: big-endian
//! u32 total ADPCM bytes, `00 7f`, a u16 rate field, u8 channels), kind-1
//! data chunks (SPU ADPCM; stereo interleaves 0x800-byte blocks per
//! channel), kind 5 lip sync, kind 4 subtitles and the end chunk 0xf0.
//!
//! The rate field (0xac44) is not the playback rate: the speech in clips
//! with subtitles lines up with them at about 9375 Hz per channel (SPU pitch
//! 0x320), which is used here.

pub const RATE: u32 = 9375;
const INTERLEAVE: usize = 0x800;

/// SPU ADPCM (16-byte blocks of 28 samples) to PCM.
pub fn decode_adpcm(data: &[u8]) -> Vec<i16> {
    const COEF: [(i32, i32); 5] = [(0, 0), (60, 0), (115, -52), (98, -55), (122, -60)];
    let mut out = Vec::with_capacity(data.len() / 16 * 28);
    let (mut s1, mut s2) = (0i32, 0i32);
    for block in data.chunks_exact(16) {
        let shift = u32::from(block[0] & 0xF);
        let filter = usize::from(block[0] >> 4).min(4);
        let (f0, f1) = COEF[filter];
        for i in 0..28 {
            let byte = block[2 + i / 2];
            let nib = if i % 2 == 1 { byte >> 4 } else { byte & 0xF };
            let nib = i32::from(nib as i8) << 28 >> 28;
            let s = ((nib << 12) >> shift.min(12)) + ((s1 * f0 + s2 * f1) >> 6);
            let s = s.clamp(-32768, 32767);
            out.push(s as i16);
            s2 = s1;
            s1 = s;
        }
    }
    out
}

/// A decoded clip.
pub struct Clip {
    pub channels: u16,
    pub rate: u32,
    /// Interleaved samples.
    pub samples: Vec<i16>,
    /// Length in frames (60 per second), from the end chunk.
    pub frames: u32,
}

fn u32_le(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// Decodes a clip from its chunks.
pub fn decode_clip(data: &[u8]) -> Option<Clip> {
    let mut pos = 0;
    let mut audio = Vec::new();
    let mut channels = 1u16;
    let mut frames = 0;
    let mut n = 0;
    while let (Some(kind), Some(size), Some(frame)) = (u32_le(data, pos), u32_le(data, pos + 4), u32_le(data, pos + 8)) {
        let size = size as usize;
        if !(16..=0x10_0000).contains(&size) || pos + size > data.len() || (kind == 0x10 && n > 2) {
            break;
        }
        let body = &data[pos + 16..pos + size];
        match kind {
            1 if size == 0x20 => channels = u16::from(body.get(8).copied().unwrap_or(1).clamp(1, 2)),
            1 => audio.extend_from_slice(body),
            0xF0 => {
                frames = frame;
                break;
            }
            _ => {}
        }
        pos += size;
        n += 1;
    }
    if audio.is_empty() {
        return None;
    }
    let samples = if channels == 2 {
        let (mut l, mut r) = (Vec::new(), Vec::new());
        for (i, block) in audio.chunks(INTERLEAVE).enumerate() {
            if i % 2 == 0 { l.extend_from_slice(block) } else { r.extend_from_slice(block) }
        }
        let (l, r) = (decode_adpcm(&l), decode_adpcm(&r));
        l.iter().zip(r.iter()).flat_map(|(a, b)| [*a, *b]).collect()
    } else {
        decode_adpcm(&audio)
    };
    Some(Clip { channels, rate: RATE, samples, frames })
}

impl Clip {
    /// A RIFF WAVE file of the samples.
    pub fn wav(&self) -> Vec<u8> {
        let bytes = (self.samples.len() * 2) as u32;
        let mut w = Vec::with_capacity(44 + bytes as usize);
        let block = self.channels * 2;
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + bytes).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&self.channels.to_le_bytes());
        w.extend_from_slice(&self.rate.to_le_bytes());
        w.extend_from_slice(&(self.rate * u32::from(block)).to_le_bytes());
        w.extend_from_slice(&block.to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&bytes.to_le_bytes());
        for s in &self.samples {
            w.extend_from_slice(&s.to_le_bytes());
        }
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_decodes_to_zero() {
        let mut block = [0u8; 16];
        block[0] = 0x0C;
        block[1] = 0x02;
        assert!(decode_adpcm(&block).iter().all(|s| *s == 0));
    }
}
