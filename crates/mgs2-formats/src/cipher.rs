//! The XOR stream cipher and name hash used by the game's data loader.

/// Word-wise XOR stream cipher (`FUN_00100688` in SLUS_201.44).
///
/// Each little-endian 32-bit word is XORed with `key`, then
/// `key = key * 0x02E90EDD + step`. A trailing partial word is processed as a
/// whole word, matching the game.
#[derive(Clone, Copy, Debug)]
pub struct Cipher {
    pub key: u32,
    pub step: u32,
}

impl Cipher {
    pub fn new(key: u32, step: u32) -> Self {
        Self { key, step }
    }

    /// Cipher for the STAGE.DAT table of contents, seeded by its first word
    /// (`FUN_00100d80`).
    pub fn for_toc(seed: u32) -> Self {
        Self::new(seed, seed ^ 0xF0F0)
    }

    /// Cipher for one stage's file table (`FUN_001011c0`).
    pub fn for_stage(seed: u32, stage: &str) -> Self {
        let h = str_code(stage.as_bytes()).wrapping_mul(0x81);
        Self::new(
            h.wrapping_add(seed).wrapping_sub(0x5876_DA27),
            h.wrapping_add(0x7A88_FB59),
        )
    }

    /// Cipher for a zlib-compressed page, derived from its first halfword
    /// (`FUN_00100658`). After decryption that halfword reads `0x9385`; the
    /// caller must add `0x08F3` to restore the zlib header (`78 9C`).
    pub fn for_page(first_halfword: u16) -> Self {
        let lo = u32::from(first_halfword ^ 0x9385);
        let hi = u32::from(first_halfword ^ 0xF6F3);
        Self::new(lo | (hi << 16), lo.wrapping_mul(0x116))
    }

    pub fn apply(&mut self, data: &mut [u8]) {
        for chunk in data.chunks_mut(4) {
            let mut word = [0u8; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            let x = (u32::from_le_bytes(word) ^ self.key).to_le_bytes();
            chunk.copy_from_slice(&x[..chunk.len()]);
            self.key = self.key.wrapping_mul(0x02E9_0EDD).wrapping_add(self.step);
        }
    }
}

/// 24-bit name hash used for stage names and file ids (`FUN_001408b8`).
pub fn str_code(name: &[u8]) -> u32 {
    let h = name.iter().fold(0u32, |h, &c| {
        ((h << 5) | (h >> 19)).wrapping_add(u32::from(c)) & 0x00FF_FFFF
    });
    if h == 0 { 1 } else { h }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_name_hash_matches_file_ids() {
        // w00a's lighting file id is 0x0b3c4661: type 0x0b, hash of "w00a".
        assert_eq!(str_code(b"w00a"), 0x3C4661);
    }

    #[test]
    fn cipher_round_trips() {
        let mut data = *b"metal gear solid 2!";
        Cipher::new(1, 2).apply(&mut data);
        Cipher::new(1, 2).apply(&mut data);
        assert_eq!(&data, b"metal gear solid 2!");
    }

    #[test]
    fn page_cipher_restores_zlib_header() {
        let v = 0x1234u16;
        let mut first = v.to_le_bytes().to_vec();
        first.extend_from_slice(&[0, 0]);
        Cipher::for_page(v).apply(&mut first);
        let h = u16::from_le_bytes([first[0], first[1]]).wrapping_add(0x08F3);
        assert_eq!(h.to_le_bytes(), [0x78, 0x9C]);
    }
}
