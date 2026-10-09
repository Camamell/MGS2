//! The sound driver's music: stage sound packs and an offline port of the
//! IOP driver's sequencer (iop_main.irx in MODULE.IPK, the MGS1 `sd` driver
//! grown for the PS2).
//!
//! A stage's sound page (`0x7F000010`) holds one pack: a header of four
//! `u32 start sector, u32 id` pairs (sectors of 0x800 bytes), then
//!
//! 0. a wave bank (`FUN_00001928`): big-endian `u32 tone table offset,
//!    u32 table size`, padding to 0x10, the tone table (16-byte tones copied
//!    to tone `offset / 16` on), then big-endian `u32 SPU address, u32 size`,
//!    padding, and the SPU ADPCM data (loaded at 0x20000 + address);
//! 1. a second wave bank;
//! 2. sound effect data;
//! 3. song data (`FUN_000016cc`): `u32 count`, `u32` offsets of each song,
//!    a song being 32 `u32` track offsets (24 bits; 0 = unused), all from
//!    the section's start.
//!
//! A tone: `u32 address` (+ 0x20000), `u8 base note, u8 fine tune`, attack
//! mode, attack (0x7f - rate), decay (0xf - rate), sustain mode, sustain
//! (0x7f - rate), sustain level, release mode, release (0x1f - rate), pan
//! (0..20, 10 centre), volume decrement (`FUN_0000ad58`).
//!
//! Tracks are runs of 4-byte events read back to front (`FUN_000037c8`):
//! byte 3 the command, bytes 2, 1, 0 its parameters. Commands under 0x80
//! are notes (length in steps, gate %, velocity); 0x80 and up are
//! controls (table at 0x1a720). The driver ticks at 200 Hz (a 2.5 ms
//! hardware timer, every other wake); a track advances a step whenever its
//! tempo, added each tick, passes 0x100.

/// SPU ADPCM base for tone addresses (`DAT_0009ba9c`).
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

const SPU_BASE: usize = 0x20000;
const SPU_RAM: usize = 0x20_0000;
/// Output rate (the SPU2's).
pub const RATE: u32 = 48000;
const TICK_SAMPLES: usize = (RATE / 200) as usize;

/// Pitch per semitone (0x1000 = 48 kHz), index (note + base note) & 0x7f
/// (table at 0x1ab7c).
const PITCH: [u32; 130] = [
    267, 283, 300, 318, 337, 357, 378, 401, 425, 450, 477, 505, 535, 567, 601, 637, 675, 715, 757, 802, 850, 901, 954, 1011, 1071, 1135, 1202, 1274, 1350, 1430, 1515, 1605, 1701,
    1802, 1909, 2022, 2143, 2270, 2405, 2548, 2700, 2860, 3030, 3211, 3402, 3604, 3818, 4045, 4286, 4541, 4811, 5097, 5400, 5721, 6061, 6422, 6804, 7208, 7637, 8091, 8572, 9082,
    9622, 10194, 10800, 11442, 12122, 12844, 13608, 14416, 15276, 16182, 33, 35, 38, 40, 42, 45, 47, 50, 53, 56, 60, 63, 66, 70, 75, 79, 84, 89, 94, 100, 106, 112, 119, 126, 133,
    141, 150, 159, 168, 178, 189, 200, 212, 225, 238, 252, 0, 0, 0, 0, 0, 268451839, 0, 524, 0, 0, 0, 524, 0, 0, 0, 524, 0, 0, 0, 524, 0, 0,
];
/// Pan curve, 0..40 (0x1a9d4).
const PAN: [u32; 41] = [
    0, 321, 850, 1781, 2682, 3555, 3933, 4855, 5655, 6425, 6758, 7488, 8188, 8860, 9220, 9847, 10446, 11015, 11556, 11844, 12287, 12707, 12957, 13387, 13787, 14159, 14353, 14680,
    14978, 15248, 15489, 15587, 15786, 15957, 16113, 16165, 16263, 16331, 16371, 16383, 16383,
];
/// Effect pan curve, 0..64 (0x1aa78).
const PAN64: [u32; 65] = [
    0, 197, 589, 1231, 1672, 2291, 2657, 3255, 3665, 4012, 4579, 4969, 5513, 5887, 6357, 6716, 7068, 7559, 7848, 8318, 8639, 8909, 9349, 9648, 10065, 10309, 10705, 10973, 11236,
    11564, 11811, 12154, 12287, 12418, 12736, 12951, 13218, 13418, 13611, 13876, 14029, 14273, 14435, 14656, 14783, 14924, 15114, 15222, 15391, 15502, 15606, 15730, 15819, 15935,
    16008, 16101, 16151, 16202, 16265, 16296, 16338, 16358, 16377, 16383, 16383,
];
/// Vibrato shape (0x1a9a4).
const VIB: [u8; 32] = [0, 32, 56, 80, 104, 128, 144, 160, 176, 192, 208, 224, 232, 240, 240, 248, 255, 248, 244, 240, 232, 224, 208, 192, 176, 160, 144, 128, 104, 80, 56, 32];
/// Random pitch table (0x1a920).
const RND: [u8; 130] = [
    159, 60, 178, 82, 175, 69, 199, 137, 16, 127, 224, 157, 220, 31, 97, 22, 57, 201, 156, 235, 87, 8, 102, 248, 90, 36, 191, 14, 62, 21, 75, 219, 171, 245, 49, 12, 67, 2, 85, 222,
    65, 218, 189, 174, 25, 176, 72, 87, 186, 163, 54, 11, 249, 223, 23, 168, 4, 12, 224, 145, 24, 93, 221, 211, 40, 138, 242, 17, 89, 111, 6, 10, 52, 42, 121, 172, 94, 167, 131,
    198, 57, 193, 180, 58, 63, 254, 79, 239, 31, 0, 48, 153, 76, 40, 131, 237, 138, 47, 44, 102, 63, 214, 108, 183, 73, 34, 188, 101, 250, 207, 2, 177, 70, 240, 154, 215, 226, 15,
    17, 197, 116, 246, 122, 44, 143, 251, 25, 106, 229, 0,
];

#[derive(Clone, Copy, Debug, Default)]
pub struct Tone {
    pub addr: u32,
    pub note: u8,
    pub fine: u8,
    pub raw: [u8; 16],
}

/// Sound packs loaded the way the driver loads them, ready to play: tones,
/// sample memory, songs and sound effects.
pub struct Pack {
    pub tones: Vec<Tone>,
    /// SPU memory, then (from `IOP_BANK`) the IOP-side bank whose tones the
    /// driver streams into the SPU when they play.
    pub spu: Vec<u8>,
    /// Song data, then the resident and the stage sound effect data.
    code: Vec<u8>,
    /// Resident effect table in `code` (ids under 0x100; sequences 0x1000
    /// after it).
    se_resident: Option<usize>,
    /// Stage effects: id - 0x100 -> entry, and the table in `code`.
    se_map: Vec<u8>,
    se_stage: Option<usize>,
    /// Tones whose samples are in the IOP-side bank.
    iop_tones: Vec<bool>,
}

/// Where the IOP-side bank (wave tables at offset 0x1000 and up, loaded to
/// IOP 0x1ba70 by `FUN_00001928`) sits in `Pack::spu`.
const IOP_BANK: usize = SPU_RAM;
const IOP_BANK_SIZE: usize = 0x80000;

impl Pack {
    /// A stage's pack on its own.
    pub fn parse(data: &[u8]) -> Option<Pack> {
        Self::with_resident(None, data)
    }

    /// A stage's pack loaded over the chapter's resident pack (`r_tnk0`),
    /// which brings the common samples and sound effects.
    pub fn with_resident(resident: Option<&[u8]>, stage: &[u8]) -> Option<Pack> {
        let mut pack = Pack {
            tones: vec![Tone::default(); 0x240],
            spu: vec![0; SPU_RAM + IOP_BANK_SIZE],
            code: Vec::new(),
            se_resident: None,
            se_map: Vec::new(),
            se_stage: None,
            iop_tones: vec![false; 0x240],
        };
        // Songs first so their offsets are offsets in `code`.
        let s = sec(stage, 3)?;
        pack.code = stage.get(s..)?.to_vec();
        if let Some(r) = resident {
            pack.load(r);
        }
        pack.load(stage);
        Some(pack)
    }

    fn load(&mut self, data: &[u8]) {
        let id = |i: usize| le32(data, i * 8 + 4).unwrap_or(0xFF);
        for b in 0..2 {
            if id(b) != 0xFF
                && let Some(o) = sec(data, b)
            {
                self.load_bank(data, o);
            }
        }
        if id(2) != 0xFF
            && let (Some(o), Some(end)) = (sec(data, 2), sec(data, 3))
            && let (Some(map), Some(rest)) = (data.get(o..o + 0x800), data.get(o + 0x800..end.min(data.len())))
        {
            let base = self.code.len();
            self.code.extend_from_slice(rest);
            if map[..2] == [0xFE, 0xFE] {
                self.se_resident = Some(base);
            } else {
                self.se_map = map.to_vec();
                self.se_stage = Some(base);
            }
        }
    }

    /// `FUN_00001928` + `FUN_0000178c`.
    fn load_bank(&mut self, data: &[u8], o: usize) -> Option<()> {
        let (off, size) = (be32(data, o)? as usize, be32(data, o + 4)? as usize);
        let table = o + 0x10;
        let count = size / 16;
        let entry = |i: usize| -> Option<[u8; 16]> { data.get(table + i * 16..table + i * 16 + 16).map(|r| r.try_into().unwrap()) };
        let addr = |r: &[u8; 16]| u32::from_le_bytes(r[..4].try_into().unwrap());
        // A table loaded at tone 0 runs while its addresses rise; the rest
        // goes to tone 0x200 on (the drum tones).
        let mut split = count;
        if off == 0 {
            let mut prev = 0;
            for i in 0..count {
                let a = addr(&entry(i)?);
                if i > 0 && a < prev {
                    split = i;
                    break;
                }
                prev = a;
            }
        }
        let iop = off >= 0x1000;
        for i in 0..count {
            let raw = entry(i)?;
            let n = if i < split { off / 16 + i } else { 0x200 + i - split };
            if let Some(t) = self.tones.get_mut(n) {
                *t = Tone { addr: addr(&raw), note: raw[4], fine: raw[5], raw };
                self.iop_tones[n] = iop;
            }
        }
        let p = table + size;
        let (dst, n) = (be32(data, p)? as usize, be32(data, p + 4)? as usize);
        let src = data.get(p + 0x10..(p + 0x10 + n).min(data.len()))?;
        let (base, limit) = if iop { (IOP_BANK, IOP_BANK + IOP_BANK_SIZE) } else { (SPU_BASE, SPU_RAM) };
        let dst = base + dst;
        let len = src.len().min(limit.saturating_sub(dst));
        self.spu[dst..dst + len].copy_from_slice(&src[..len]);
        Some(())
    }

    /// Sample start of a tone in `spu`.
    fn tone_start(&self, n: usize) -> usize {
        let a = self.tones[n].addr as usize;
        if self.iop_tones[n] { IOP_BANK + a } else { SPU_BASE + a }
    }

    /// Number of songs in the pack.
    pub fn song_count(&self) -> u32 {
        le32(&self.code, 0).unwrap_or(0)
    }

    /// Renders song `n` (1-based) for `seconds` as interleaved stereo at
    /// 48 kHz. `level` is the interactive music level (0 = the song's own
    /// mix; 1..8 as the game sets with `0xff00010n`).
    pub fn render(self: &Arc<Self>, n: u32, seconds: f32, level: u8) -> Vec<i16> {
        let mut seq = Sequencer::new(self.clone(), level);
        seq.start_song(n);
        self.run(seq, seconds)
    }

    /// Renders sound effect `cmd` (`pan << 18 | volume << 12 | id`).
    pub fn render_effect(self: &Arc<Self>, cmd: u32, seconds: f32) -> Vec<i16> {
        let mut seq = Sequencer::new(self.clone(), 0);
        seq.play_effect(cmd);
        self.run(seq, seconds)
    }

    fn run(&self, mut seq: Sequencer, seconds: f32) -> Vec<i16> {
        let ticks = (seconds * 200.0) as usize;
        let mut out = Vec::with_capacity(ticks * TICK_SAMPLES * 2);
        for _ in 0..ticks {
            seq.tick();
            seq.spu.mix(&self.spu, TICK_SAMPLES, &mut out);
            if seq.finished() && seq.spu.silent() {
                break;
            }
        }
        out
    }
}

fn be32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()))
}

fn le32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn sec(data: &[u8], i: usize) -> Option<usize> {
    le32(data, i * 8).map(|s| s as usize * 0x800).filter(|&o| o != 0 && o < data.len())
}

// ---------------------------------------------------------------- SPU

#[derive(Clone, Copy, Default, PartialEq)]
enum Phase {
    #[default]
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Default)]
struct Voice {
    // registers
    pitch: u16,
    vol_l: i32,
    vol_r: i32,
    ssa: usize,
    adsr1: u16,
    adsr2: u16,
    // state
    phase: Phase,
    level: i32,
    counter: i32,
    addr: usize,
    loop_addr: usize,
    pos: u32,
    block: [i16; 28],
    prev: [i32; 2],
    hist: [i32; 2],
    stop_after: bool,
}

const COEF: [(i32, i32); 5] = [(0, 0), (60, 0), (115, -52), (98, -55), (122, -60)];

impl Voice {
    fn key_on(&mut self, ram: &[u8]) {
        self.addr = self.ssa;
        self.loop_addr = self.ssa;
        self.phase = Phase::Attack;
        self.level = 0;
        self.counter = 0;
        self.pos = 0;
        self.hist = [0, 0];
        self.prev = [0, 0];
        self.stop_after = false;
        self.decode(ram);
    }

    fn key_off(&mut self) {
        if self.phase != Phase::Off {
            self.phase = Phase::Release;
        }
    }

    fn decode(&mut self, ram: &[u8]) {
        let a = self.addr.min(ram.len() - 16);
        let b = &ram[a..a + 16];
        let shift = u32::from(b[0] & 0xF).min(12);
        let (f0, f1) = COEF[usize::from(b[0] >> 4).min(4)];
        let flags = b[1];
        if flags & 4 != 0 {
            self.loop_addr = a;
        }
        for i in 0..28 {
            let nib = (b[2 + i / 2] >> ((i & 1) * 4)) & 0xF;
            let nib = i32::from(nib as i8) << 28 >> 28;
            let s = ((nib << 12) >> shift) + ((self.hist[0] * f0 + self.hist[1] * f1) >> 6);
            let s = s.clamp(-32768, 32767);
            self.hist[1] = self.hist[0];
            self.hist[0] = s;
            self.block[i] = s as i16;
        }
        self.addr = a + 16;
        if flags & 1 != 0 {
            if flags & 2 != 0 {
                self.addr = self.loop_addr;
            } else {
                self.stop_after = true;
            }
        }
    }

    /// One envelope step (PS1/PS2 SPU rules).
    fn env(&mut self, rate: i32, exp: bool, dec: bool) {
        if rate >= 0x7F {
            return;
        }
        let shift = rate >> 2;
        let mut step = if dec { -8 + (rate & 3) } else { 7 - (rate & 3) };
        let mut inc = 0x8000;
        if shift < 11 {
            step <<= 11 - shift;
        } else {
            inc = (0x8000 >> (shift - 11)).max(1);
        }
        if exp && !dec && self.level > 0x6000 {
            inc = (inc >> 2).max(1);
        }
        if exp && dec {
            step = step * self.level >> 15;
        }
        self.counter += inc;
        if self.counter >= 0x8000 {
            self.counter = 0;
            self.level = (self.level + step).clamp(0, 0x7FFF);
        }
    }

    fn envelope(&mut self) {
        let (a1, a2) = (i32::from(self.adsr1), i32::from(self.adsr2));
        match self.phase {
            Phase::Off => {}
            Phase::Attack => {
                self.env((a1 >> 8) & 0x7F, a1 & 0x8000 != 0, false);
                if self.level >= 0x7FFF {
                    self.level = 0x7FFF;
                    self.phase = Phase::Decay;
                }
            }
            Phase::Decay => {
                self.env(((a1 >> 4) & 0xF) << 2, true, true);
                let sl = ((a1 & 0xF) + 1) * 0x800;
                if self.level <= sl {
                    self.phase = Phase::Sustain;
                }
            }
            Phase::Sustain => self.env((a2 >> 6) & 0x7F, a2 & 0x8000 != 0, a2 & 0x4000 != 0),
            Phase::Release => {
                self.env((a2 & 0x1F) << 2, a2 & 0x20 != 0, true);
                if self.level == 0 {
                    self.phase = Phase::Off;
                }
            }
        }
    }

    fn sample(&mut self, ram: &[u8]) -> (i32, i32) {
        if self.phase == Phase::Off {
            return (0, 0);
        }
        let i = (self.pos >> 12) as usize;
        let frac = (self.pos & 0xFFF) as i32;
        let s0 = i32::from(self.block[i.min(27)]);
        let s1 = if i + 1 < 28 { i32::from(self.block[i + 1]) } else { s0 };
        let s = s0 + ((s1 - s0) * frac >> 12);
        self.envelope();
        let v = s * self.level >> 15;
        self.pos += u32::from(self.pitch.min(0x3FFF));
        while self.pos >= 28 << 12 {
            self.pos -= 28 << 12;
            if self.stop_after {
                self.phase = Phase::Off;
                self.level = 0;
                break;
            }
            self.decode(ram);
        }
        (v * self.vol_l >> 14, v * self.vol_r >> 14)
    }
}

struct Spu {
    voices: Vec<Voice>,
}

impl Spu {
    fn mix(&mut self, ram: &[u8], n: usize, out: &mut Vec<i16>) {
        for _ in 0..n {
            let (mut l, mut r) = (0i32, 0i32);
            for v in &mut self.voices {
                let (a, b) = v.sample(ram);
                l += a;
                r += b;
            }
            out.push(l.clamp(-32768, 32767) as i16);
            out.push(r.clamp(-32768, 32767) as i16);
        }
    }

    fn silent(&self) -> bool {
        self.voices.iter().all(|v| v.phase == Phase::Off)
    }
}

// ---------------------------------------------------------------- sequencer

/// A track's state (the driver's 0xfc-byte work area; offsets in comments).
#[derive(Clone, Default)]
struct Track {
    ptr: usize, // 0x00, 0 = stopped
    ngc: u8,    // 0x04 steps left in the note
    ngo: u8,    // 0x05 steps left in the gate
    ngs: u8,    // 0x06 note length
    ngg: u8,    // 0x07 gate %
    lp1_cnt: u8,
    lp2_cnt: u8,
    lp1_vol: i32,
    lp2_vol: i32,
    lp1_freq: i32,
    lp2_freq: i32,
    lp1_addr: usize,
    lp2_addr: usize,
    l3_addr: usize,
    kakko_cnt: u8,
    kakko_start: usize,
    kakko_end: usize,
    pvoc: u8,   // 0x34 volume move steps
    pvod: i32,  // 0x38 volume << 8
    pvoad: i32, // 0x3c
    pvom: i32,  // 0x40
    vol: u8,    // 0x44 velocity
    panc: u8,   // 0x45
    panf: i32,  // 0x48 pan << 8
    panad: i32, // 0x4c
    panm: i32,  // 0x50
    pan: u8,    // 0x54
    panmode: u8, // 0x56
    swpc: u8,   // 0x57 portamento steps
    swphc: u8,  // 0x58 delay
    swpd: i32,  // 0x5c pitch (semitone << 8)
    swpad: i32, // 0x60
    swpm: i32,  // 0x64 target
    sws_cnt: u8, // 0x68
    sws_delay: u8, // 0x69
    por: u8,    // 0x6a
    sws_ofs: i32, // 0x6c
    vibhc: u8,  // 0x70
    vib_tmp: u32, // 0x74
    vib_idx: u8, // 0x78
    vib_ofs: u8, // 0x79
    vibcc: u8,  // 0x7a
    vibd: u32,  // 0x7c
    vibdm: u32, // 0x80
    vibhs: u8,  // 0x84
    vibcs: u8,  // 0x85
    vib_rate: u8, // 0x86
    vibad: u32, // 0x88
    rdm_cnt: u32, // 0x8c
    rdm_idx: u32, // 0x90
    rdms: u8,   // 0x94
    rdm_mask: u32, // 0x98
    tone: u32,  // 0xa4
    ptps: i32,  // 0xa8 transpose
    dec_vol: u32, // 0xac
    tund: i32,  // 0xb0 detune
    tmpd: u32,  // 0xb4
    tmp: u8,    // 0xb8 tempo
    tmpad: i32, // 0xbc
    tmpc: u8,   // 0xc0
    tmpw: i32,  // 0xc4
    tmpm: u8,   // 0xc8
    base_note: u8, // 0xd0
    fine: u8,   // 0xd1
    rr: u16,    // 0xd2
    sync_track: u8, // 0xd8
    sync_every: u32, // 0xd4
    sync_cnt: u8, // 0xd9
    sync_addr: usize, // 0xdc
    sync_go: bool, // 0xe0
    signal: bool, // 0xe1
    dyn_seen: i32, // 0xe4
    dyn_mode: u8, // 0xe8
    dyn_vol: [u8; 8], // 0xe9..0xf0
    dyn_time: [u8; 8], // 0xf1..0xf8
    // per-track mix level (0x9f8e0..)
    mix_cur: i32,
    mix_target: i32,
    mix_step: i32,
    /// Pan from the game (0x9f8ec, 0..64) for pan mode 2.
    ext_pan: u32,
}

impl Track {
    fn init(ptr: usize) -> Track {
        Track { ptr, ngc: 1, pvod: 0x7F, vol: 0x7F, panf: 0x1400, pan: 0x14, tmpd: 1, tmp: 0xFF, sync_cnt: 1, dyn_seen: -1, mix_cur: 0xFFFF, mix_target: 0xFFFF, ext_pan: 0x20, ..Default::default() }
    }
}

/// Song tracks (and their voices); the effect tracks follow.
const SONG_TRACKS: usize = 32;
const EFFECT_TRACKS: usize = 12;
const TRACKS: usize = SONG_TRACKS + EFFECT_TRACKS;

/// An effect track's request (`FUN_00010004` / `FUN_0000ecbc`).
#[derive(Clone, Copy, Default)]
struct Effect {
    cmd: u32,
    priority: u8,
    /// Nonzero: positional volume and pan from the command.
    kind: u8,
    group: u8,
}

struct Sequencer {
    pack: Arc<Pack>,
    tracks: Vec<Track>,
    effects: [Effect; EFFECT_TRACKS],
    spu: Spu,
    level: i32,
    key_on: u64,
    key_off: u64,
    note_on: bool,
}

impl Sequencer {
    fn new(pack: Arc<Pack>, level: u8) -> Self {
        Sequencer {
            pack,
            tracks: vec![Track::default(); TRACKS],
            effects: [Effect::default(); EFFECT_TRACKS],
            spu: Spu { voices: vec![Voice::default(); TRACKS] },
            level: i32::from(level),
            key_on: 0,
            key_off: 0,
            note_on: false,
        }
    }

    /// `FUN_0000e994`: song n's tracks (0 stops the song).
    fn start_song(&mut self, n: u32) {
        for i in 0..SONG_TRACKS {
            self.tracks[i] = Track::default();
            self.spu.voices[i].key_off();
        }
        let d = &self.pack.code;
        if n >= 1 && n <= self.pack.song_count()
            && let Some(song) = le32(d, n as usize * 4)
        {
            for i in 0..SONG_TRACKS {
                let off = le32(d, song as usize + i * 4).unwrap_or(0) & 0xFF_FFFF;
                if off != 0 {
                    self.tracks[i] = Track::init(off as usize);
                }
            }
        }
    }

    /// `FUN_00010004`: starts sound effect `cmd` (`pan << 18 | volume << 12
    /// | id`) on up to three effect tracks.
    fn play_effect(&mut self, cmd: u32) {
        let id = (cmd & 0x7FF) as usize;
        let pack = self.pack.clone();
        let (table, entry) = if id < 0x100 {
            let Some(t) = pack.se_resident else { return };
            (t, id)
        } else {
            let Some(t) = pack.se_stage else { return };
            match pack.se_map.get(id - 0x100) {
                Some(&e) if e != 0xFF => (t, usize::from(e)),
                _ => return,
            }
        };
        let e = table + entry * 16;
        let b = |k: usize| pack.code.get(e + k).copied().unwrap_or(0);
        let (priority, count, kind, group) = (b(0), b(1).min(3), b(2), b(3));
        let mut used = 0u16;
        for k in 0..usize::from(count) {
            let Some(off) = le32(&pack.code, e + 4 + k * 4) else { break };
            if off == 0xFFFF_FFFF {
                continue;
            }
            let busy = |s: &Self, c: usize| s.tracks[SONG_TRACKS + c].ptr != 0;
            let free = |c: &usize| used & (1 << *c) == 0;
            let pick = (0..EFFECT_TRACKS)
                .filter(free)
                .find(|&c| busy(self, c) && (self.effects[c].cmd & 0x7FF) as usize == id)
                .or_else(|| (0..EFFECT_TRACKS).filter(free).find(|&c| group != 0 && busy(self, c) && self.effects[c].group == group))
                .or_else(|| (0..EFFECT_TRACKS).filter(free).find(|&c| !busy(self, c)))
                .or_else(|| (0..EFFECT_TRACKS).filter(free).filter(|&c| self.effects[c].priority <= priority).min_by_key(|&c| self.effects[c].priority));
            let Some(c) = pick else { continue };
            used |= 1 << c;
            self.effects[c] = Effect { cmd, priority, kind, group };
            let i = SONG_TRACKS + c;
            self.tracks[i] = Track::init(table + 0x1000 + off as usize);
            self.spu.voices[i].key_off();
        }
    }

    fn finished(&self) -> bool {
        self.tracks.iter().all(|t| t.ptr == 0)
    }

    fn tick(&mut self) {
        self.key_on = 0;
        self.key_off = 0;
        for i in 0..TRACKS {
            let t = &mut self.tracks[i];
            if t.mix_cur != t.mix_target {
                t.mix_cur += t.mix_step;
                if (t.mix_step < 0 && t.mix_cur < t.mix_target) || (t.mix_step >= 0 && t.mix_cur > t.mix_target) {
                    t.mix_cur = t.mix_target;
                }
            }
            if self.tracks[i].ptr != 0 && self.track_tick(i) {
                self.tracks[i].ptr = 0;
                self.key_off |= 1 << i;
            }
        }
        // Key off first, then key on (the driver writes 0x1600 before 0x1500).
        for i in 0..TRACKS {
            if self.key_off & (1 << i) != 0 {
                self.spu.voices[i].key_off();
            }
        }
        for i in 0..TRACKS {
            if self.key_on & (1 << i) != 0 {
                self.spu.voices[i].key_on(&self.pack.spu);
            }
        }
    }

    fn byte(&self, p: usize) -> u8 {
        self.pack.code.get(p).copied().unwrap_or(0)
    }
    /// `FUN_000033a0`. Returns true when the track ends.
    fn track_tick(&mut self, i: usize) -> bool {
        {
            let t = &mut self.tracks[i];
            t.tmpd += u32::from(t.tmp);
            if t.tmpd > 0xFF && t.sync_every != 0 {
                t.sync_cnt = t.sync_cnt.wrapping_sub(1);
                if t.sync_cnt == 0 {
                    t.sync_cnt = t.sync_every as u8;
                    let other = usize::from(t.sync_track & 0x1F);
                    if t.sync_track != 0xFF && other != i && self.tracks[other].signal {
                        self.tracks[other].signal = false;
                        let t = &mut self.tracks[i];
                        t.sync_go = true;
                        t.ptr = t.sync_addr;
                        t.ngc = 1;
                    }
                }
            }
        }
        let t = &self.tracks[i];
        if t.sync_every != 0 && !t.sync_go {
            self.tracks[i].tmpd &= 0xFF;
            return false;
        }
        self.note_on = false;
        if self.tracks[i].tmpd < 0x100 {
            self.tick_effects(i);
        } else {
            self.tracks[i].tmpd &= 0xFF;
            self.tracks[i].ngc = self.tracks[i].ngc.wrapping_sub(1);
            if self.tracks[i].ngc == 0 {
                if self.read(i) {
                    return true;
                }
            } else {
                self.step_effects(i);
            }
            self.tempo_move(i);
            self.lookahead_sweep(i);
            self.vol_pan_move(i);
            self.dynamics(i);
        }
        // Effect tracks' IOP-side tones are streamed in and keyed on too.
        if self.note_on && (self.tracks[i].tone < 0x100 || i >= SONG_TRACKS) {
            self.key_on |= 1 << i;
        }
        false
    }

    /// `FUN_000037c8`: reads events until a note, rest or tie.
    fn read(&mut self, i: usize) -> bool {
        for _ in 0..0xFF {
            let p = self.tracks[i].ptr;
            let cmd = self.byte(p + 3);
            if cmd == 0 {
                return true;
            }
            let (a, b, c) = (self.byte(p + 2), self.byte(p + 1), self.byte(p));
            self.tracks[i].ptr = p + 4;
            if cmd < 0x80 {
                if self.tracks[i].ngg < 100 && c != 0 {
                    self.note_on = true;
                }
                self.note_set(i, cmd, a, b, c);
                return false;
            }
            self.control(i, cmd, a, b, c);
            match cmd {
                0xFF => return true,
                0xEC if self.tracks[i].ptr == 0 => return true,
                0xF2 | 0xF3 | 0xDA | 0xDB => return false,
                _ => {}
            }
        }
        true
    }

    fn note_set(&mut self, i: usize, note: u8, len: u8, gate: u8, vel: u8) {
        let t = &mut self.tracks[i];
        t.ngs = len;
        t.ngg = gate;
        t.vol = vel & 0x7F;
        self.freq_set(i, note);
        let t = &mut self.tracks[i];
        t.ngc = t.ngs;
        t.ngo = ((u32::from(t.ngg) * u32::from(t.ngc)) / 100).max(1) as u8;
    }

    /// `FUN_00003bfc`.
    fn freq_set(&mut self, i: usize, note: u8) {
        let mut n = i32::from(note);
        if note >= 0x48 {
            self.tone_set(i, u32::from(note) + 0x1B8);
            n = 0x24;
        }
        let t = &mut self.tracks[i];
        let mut pitch = (n + t.ptps) * 0x100 + t.tund + t.lp1_freq + t.lp2_freq;
        while pitch > 0x5FFF {
            pitch -= 0x6000;
        }
        let old = t.swpd;
        t.swpd = pitch;
        t.vibcc = 0;
        t.vibhc = 0;
        t.vib_idx = 0;
        t.vib_tmp = 0;
        t.vibd = 0;
        let rr = t.rr;
        self.set_rr(i, rr);
        let t = &mut self.tracks[i];
        t.swpc = t.sws_cnt;
        if t.swpc != 0 {
            t.swphc = t.sws_delay;
            if t.por == 0 {
                let target = t.swpd;
                if t.sws_ofs < 0x7F01 {
                    t.swpd -= t.sws_ofs;
                } else {
                    t.swpd = t.swpd + 0x10000 - (t.sws_ofs & 0xFFFF);
                }
                Self::sweep_to(t, target);
            } else {
                t.swpm = t.swpd;
                t.swpd = old;
            }
        }
        let p = self.tracks[i].swpd;
        self.pitch_set(i, p);
    }

    /// `FUN_00003f40`.
    fn sweep_to(t: &mut Track, target: i32) {
        if t.swpc == 0 {
            return;
        }
        let div = ((u32::from(t.swpc) << 8) / u32::from(t.tmp.max(1))).max(1) as i32;
        let target = target.clamp(0, 0x5FFF);
        t.swpm = target;
        t.swpad = (target - t.swpd) / div;
    }

    /// `FUN_0000bafc`: semitone << 8 to SPU pitch.
    fn pitch_set(&mut self, i: usize, p: i32) {
        let t = &self.tracks[i];
        let v = (p + i32::from(t.fine)) as u32;
        let idx = (((v >> 8) as u8).wrapping_add(t.base_note) & 0x7F) as usize;
        let lo = PITCH[idx];
        let mut d = PITCH[idx + 1].wrapping_sub(lo);
        if d & 0x8000 != 0 {
            d = 0xC9;
        }
        let f = v & 0xFF;
        let pitch = (((d & 0xFF) * f) >> 8) + ((d >> 8) & 0xFF) * f + lo;
        self.spu.voices[i].pitch = pitch as u16;
    }

    fn set_rr(&mut self, i: usize, rr: u16) {
        let v = &mut self.spu.voices[i];
        v.adsr2 = (v.adsr2 & !0x1F) | (rr & 0x1F);
    }

    /// `FUN_0000ad58`.
    fn tone_set(&mut self, i: usize, n: u32) {
        let Some(tone) = self.pack.tones.get(n as usize).copied() else { return };
        let r = tone.raw;
        let v = &mut self.spu.voices[i];
        v.ssa = self.pack.tone_start(n as usize);
        let am: u16 = if r[6] != 0 { 0x8000 } else { 0 };
        v.adsr1 = am | (u16::from(0x7F - (r[7] & 0x7F)) << 8) | (u16::from(0xF - (r[8] & 0xF)) << 4) | u16::from(r[11] & 0xF);
        let sm: u16 = match r[9] {
            0 => 0x4000,
            1 => 0xC000,
            2 => 0,
            _ => 0x8000,
        };
        let rm: u16 = if r[12] != 0 { 0x20 } else { 0 };
        let rr = u16::from(0x1F - (r[13] & 0x1F));
        v.adsr2 = sm | (u16::from(0x7F - (r[10] & 0x7F)) << 6) | rm | rr;
        let t = &mut self.tracks[i];
        t.base_note = r[4];
        t.fine = r[5];
        t.rr = rr;
        if t.panmode == 0 {
            t.pan = r[14] << 1;
            t.panf = i32::from(r[14]) << 9;
        }
        t.dec_vol = u32::from(r[15]);
    }

    /// `FUN_00004404` + `FUN_0000449c`: per-step note handling.
    fn step_effects(&mut self, i: usize) {
        let t = &mut self.tracks[i];
        if t.ngg < 100 && t.ngc == 1 && t.rr > 7 {
            let v = &mut self.spu.voices[i];
            v.adsr2 = (v.adsr2 & !0x1F) | 7;
        }
        let t = &mut self.tracks[i];
        if t.ngo != 0 {
            t.ngo -= 1;
            if t.ngo == 0 {
                self.key_off |= 1 << i;
            }
        }
        let t = &mut self.tracks[i];
        let mut changed = false;
        if t.swpc != 0 {
            if t.swphc == 0 {
                if t.por == 0 {
                    t.swpc -= 1;
                    if t.swpc == 0 {
                        t.swpd = t.swpm;
                    } else {
                        t.swpd += t.swpad;
                    }
                } else {
                    Self::glide(t);
                }
                changed = true;
            } else {
                t.swphc -= 1;
            }
        }
        let mut extra = 0;
        if t.vibdm != 0 {
            if t.vibhc == t.vibhs {
                if t.vibcc == t.vibcs {
                    t.vibd = t.vibdm;
                } else {
                    t.vibd = if t.vibcc == 0 { t.vibad } else { t.vibd + t.vibad };
                    t.vibcc += 1;
                }
                t.vib_tmp += u32::from(t.vib_rate);
                if t.vib_tmp > 0xFF {
                    t.vib_tmp &= 0xFF;
                    extra += Self::vib_calc(t);
                    changed = true;
                }
            } else {
                t.vibhc += 1;
            }
        }
        let r = Self::random(t);
        if r != 0 {
            extra += r;
            changed = true;
        }
        if changed {
            let p = self.tracks[i].swpd + extra;
            self.pitch_set(i, p);
        }
    }

    /// `FUN_00004fec`: per-tick pitch effects between steps.
    fn tick_effects(&mut self, i: usize) {
        let t = &mut self.tracks[i];
        let mut changed = false;
        if t.swpc != 0 && t.swphc == 0 {
            changed = true;
            if t.por == 0 {
                t.swpd += t.swpad;
            } else {
                Self::glide(t);
            }
        }
        let mut p = t.swpd;
        if t.vibd != 0 && t.vibhs == t.vibhc {
            t.vib_tmp += u32::from(t.vib_rate);
            if t.vib_tmp > 0xFF {
                t.vib_tmp &= 0xFF;
                p += Self::vib_calc(t);
                changed = true;
            }
        }
        let r = Self::random(t);
        if r != 0 {
            p += r;
            changed = true;
        }
        if changed {
            self.pitch_set(i, p);
        }
    }

    /// `FUN_000049b4`: portamento glide by a fraction of the distance.
    fn glide(t: &mut Track) {
        let d = t.swpm - t.swpd;
        let k = i32::from(t.sws_cnt);
        let mut s = if d < 0 {
            let u = -d;
            -((u >> 8) * k + ((k * (u & 0xFF)) >> 8)).max(1)
        } else if d == 0 {
            t.swpc = 0;
            0
        } else {
            ((d >> 8) * k + ((k * (d & 0xFF)) >> 8)).max(1)
        };
        if d == 0 {
            s = 0;
        }
        t.swpd += s;
    }

    /// `FUN_00004bb0`.
    fn vib_calc(t: &mut Track) -> i32 {
        t.vib_idx = t.vib_idx.wrapping_add(t.vib_ofs) & 0x3F;
        let w = u32::from(VIB[usize::from(t.vib_idx & 0x1F)]);
        let v = if t.vibd < 0x8000 { ((t.vibd >> 7) & 0xFE) * w >> 8 } else { (((t.vibd >> 8) & 0x7F) + 2) * w >> 1 };
        if t.vib_idx > 0x1F { -(v as i32) } else { v as i32 }
    }

    /// `FUN_0000544c`.
    fn random(t: &mut Track) -> i32 {
        if t.rdms == 0 {
            return 0;
        }
        t.rdm_cnt += u32::from(t.rdms);
        if t.rdm_cnt <= 0x100 {
            return 0;
        }
        t.rdm_cnt &= 0xFF;
        t.rdm_idx = (t.rdm_idx + 1) & 0x7F;
        let i = t.rdm_idx as usize;
        let v = (i32::from(RND[i + 1] as i8) * 0x100 + i32::from(RND[i])) as u32 & t.rdm_mask;
        v as i32
    }

    /// `FUN_000055e8`.
    fn tempo_move(&mut self, i: usize) {
        let t = &mut self.tracks[i];
        if t.tmpc != 0 {
            t.tmpc -= 1;
            if t.tmpc == 0 {
                t.tmpw = i32::from(t.tmpm) << 8;
            } else {
                t.tmpw += t.tmpad;
            }
            t.tmp = (t.tmpw >> 8) as u8;
        }
    }

    /// `FUN_00004eb4`: a sweep (0xe4) right after a note.
    fn lookahead_sweep(&mut self, i: usize) {
        let p = self.tracks[i].ptr;
        if self.tracks[i].swpc == 0 && self.byte(p + 3) == 0xE4 {
            let (a, b, c) = (self.byte(p + 2), self.byte(p + 1), self.byte(p));
            let t = &mut self.tracks[i];
            t.swphc = a;
            t.swpc = b;
            t.ptr = p + 4;
            let target = (i32::from(c) + t.ptps) * 0x100 + t.tund;
            Self::sweep_to(t, target);
        }
    }

    /// `FUN_00004094` + `FUN_00004310`.
    fn vol_pan_move(&mut self, i: usize) {
        let t = &mut self.tracks[i];
        if t.pvoc != 0 {
            t.pvoc -= 1;
            if t.pvoc == 0 {
                t.pvod = t.pvom << 8;
            } else {
                t.pvod += t.pvoad;
            }
        }
        if t.vol != 0 {
            self.volume(i);
        }
        let t = &mut self.tracks[i];
        if t.panc != 0 {
            t.panc -= 1;
            if t.panc == 0 {
                t.panf = t.panm;
            } else {
                t.panf += t.panad;
            }
            t.pan = (t.panf >> 8) as u8;
        }
    }

    /// `FUN_000056dc` + `FUN_0000b358`.
    fn volume(&mut self, i: usize) {
        let t = &self.tracks[i];
        let v = (i32::from(t.vol) + t.lp1_vol + t.lp2_vol).clamp(0, 0x7F);
        let v = ((((t.pvod >> 8) & 0xFF) * v + 0x80) >> 8) as u32;
        let v = v.saturating_sub(t.dec_vol) + 1;
        let (l, r) = match i.checked_sub(SONG_TRACKS).map(|c| self.effects[c]) {
            // Positional effects: the command's volume and 64-step pan.
            Some(e) if e.kind != 0 => {
                let vol = (e.cmd >> 12) & 0x3F;
                let pan = ((e.cmd >> 18) & 0x3F) as usize;
                let k = ((v * vol) >> 6) + 1;
                ((k * PAN64[0x40 - pan]) >> 7, (k * PAN64[pan]) >> 7)
            }
            // Pan mode 2 (`dd 2`): pan and level from the game (0xfb`ttppvv`,
            // `bgmfader.c`).
            None if t.panmode == 2 => {
                let pan = t.ext_pan.min(0x40) as usize;
                let mix = t.mix_cur.clamp(0, 0xFFFF) as u32;
                (((v * PAN64[0x40 - pan]) / 0x7F * mix) >> 16, ((v * PAN64[pan]) / 0x7F * mix) >> 16)
            }
            effect => {
                let pan = ((t.panf >> 8).clamp(0, 0x28)) as usize;
                let mix = if effect.is_some() { 0xFFFF } else { t.mix_cur.clamp(0, 0xFFFF) as u32 };
                (((v * PAN[0x28 - pan]) / 0x7F * mix) >> 16, ((v * PAN[pan]) / 0x7F * mix) >> 16)
            }
        };
        let voice = &mut self.spu.voices[i];
        voice.vol_l = l.min(0x3FFF) as i32;
        voice.vol_r = r.min(0x3FFF) as i32;
    }

    /// `FUN_000057f4`: interactive music levels.
    fn dynamics(&mut self, i: usize) {
        let level = self.level;
        let t = &mut self.tracks[i];
        if t.dyn_mode != 1 || t.dyn_seen == level {
            return;
        }
        t.dyn_seen = level;
        if !(1..=8).contains(&level) {
            return;
        }
        let k = (level - 1) as usize;
        let target = i32::from(t.dyn_vol[k]) * 0x101;
        let time = i32::from(t.dyn_time[k]);
        t.mix_target = target;
        if target == t.mix_cur {
            t.mix_step = 0;
        } else if time == 0 {
            t.mix_cur = target;
            t.mix_step = 0;
        } else {
            t.mix_step = (target - t.mix_cur) / (time * 10);
            if t.mix_step == 0 {
                t.mix_step = 1;
            }
        }
    }

    fn control(&mut self, i: usize, cmd: u8, a: u8, b: u8, c: u8) {
        let p = self.tracks[i].ptr;
        let t = &mut self.tracks[i];
        let fill = |t: &mut Track, from: usize, a: u8, b: u8, mode_one: bool| {
            // 0xf8..0xfc, 0xcd..0xcf: level volumes from `from` on.
            t.dyn_vol[from] = a;
            t.dyn_time[from] = b;
            for k in from + 1..8 {
                t.dyn_vol[k] = a;
                t.dyn_time[k] = if mode_one { 0 } else { 0xFF };
            }
        };
        match cmd {
            0xD0 => t.tmp = a,
            0xD1 => {
                t.tmpc = a;
                t.tmpm = b;
                t.tmpw = i32::from(t.tmp) << 8;
                let d = (i32::from(b) - i32::from(t.tmp)).clamp(-0x7F, 0x7F);
                t.tmpad = if a == 0 { 0 } else { ((d << 8) / i32::from(a)).clamp(-0x7F0, 0x7F0) };
            }
            0xD2 | 0xD3 | 0xD4 => {
                t.tone = u32::from(a);
                self.key_off |= 1 << i;
                self.tone_set(i, u32::from(a));
            }
            0xFD => {
                t.tone = u32::from(a) + 0x100;
                self.key_off |= 1 << i;
                self.tone_set(i, u32::from(a) + 0x100);
            }
            0xD5 => {
                t.pvod = i32::from(a) << 8;
                t.pvoc = 0;
            }
            0xD6 => {
                t.pvoc = a;
                t.pvom = i32::from(b);
                let d = i32::from(b) * 0x100 - t.pvod;
                t.pvoad = if a == 0 { 0 } else { (d / i32::from(a)).clamp(-0x7F0, 0x7F0) };
            }
            0xD7 => {
                let v = &mut self.spu.voices[i];
                v.adsr1 = (u16::from(0x7F - (a & 0x7F)) << 8) | (u16::from(0xF - (b & 0xF)) << 4) | u16::from(c & 0xF);
            }
            0xD8 => {
                let v = &mut self.spu.voices[i];
                v.adsr2 = 0x4000 | (u16::from(0x7F - (a & 0x7F)) << 6) | (v.adsr2 & 0x3F);
            }
            0xD9 => {
                let rr = u16::from(0x1F - (a & 0x1F));
                t.rr = rr;
                let v = &mut self.spu.voices[i];
                v.adsr2 = (v.adsr2 & !0x3F) | rr;
            }
            0xDA => {
                t.sync_track = a;
                t.sync_every = u32::from(b);
                t.sync_addr = p;
            }
            0xDB => {
                t.ptr = t.sync_addr;
                t.sync_go = false;
            }
            0xDC => {
                let v = &mut self.spu.voices[i];
                v.ssa = v.ssa.wrapping_add(usize::from(a) * 0x1000 + usize::from(b) * 0x10);
            }
            0xDD => {
                t.panmode = a;
                t.pan = (b as i8 as i32 + 0x14) as u8;
                t.panf = i32::from(t.pan as i8) << 8;
                t.panc = 0;
            }
            0xDE => {
                t.panc = a;
                let to = b.wrapping_add(0x14);
                t.panm = i32::from(to) << 8;
                let d = i32::from(to) - i32::from(t.pan as i8);
                t.panad = if a == 0 { 0 } else { ((d * 0x100) / i32::from(a)).clamp(-0x7F0, 0x7F0) };
            }
            0xDF => t.ptps = i32::from(a as i8),
            0xE0 => t.tund = i32::from(a as i8) << 2,
            0xE1 => {
                t.vibhs = a;
                let mut r = b;
                if r < 0x40 {
                    if r < 0x20 {
                        t.vib_ofs = 1;
                        r <<= 3;
                    } else {
                        t.vib_ofs = 2;
                        r <<= 2;
                    }
                } else if (r as i8) < 0 {
                    t.vib_ofs = if r == 0xFF { 0x10 } else { 8 };
                } else {
                    t.vib_ofs = 4;
                    r <<= 1;
                }
                t.vib_rate = r;
                t.vibd = u32::from(c) << 8;
                t.vibdm = u32::from(c) << 8;
            }
            0xE2 => {
                t.vibcs = a;
                t.vibad = if a == 0 { 0 } else { t.vibdm / u32::from(a) };
            }
            0xE3 => {
                t.rdms = a;
                t.rdm_mask = u32::from(b) * 0x100 + u32::from(c);
                t.rdm_cnt = 0;
                t.rdm_idx = 0;
            }
            0xE5 => {
                t.por = 0;
                t.sws_delay = a;
                t.sws_cnt = b;
                t.sws_ofs = i32::from(c) << 8;
            }
            0xE6 => {
                t.sws_delay = 0;
                t.sws_cnt = a;
                t.por = u8::from(a != 0);
            }
            0xE7 => {
                t.lp1_addr = p;
                t.lp1_cnt = 0;
                t.lp1_freq = 0;
                t.lp1_vol = 0;
            }
            0xE8 => {
                t.lp1_cnt = t.lp1_cnt.wrapping_add(1);
                if t.lp1_cnt == a && a != 0 {
                    t.lp1_vol = 0;
                    t.lp1_freq = 0;
                } else {
                    t.lp1_vol += i32::from(b as i8);
                    t.lp1_freq += i32::from(c as i8) * 8;
                    t.ptr = t.lp1_addr;
                }
            }
            0xE9 => {
                t.lp2_addr = p;
                t.lp2_cnt = 0;
                t.lp2_freq = 0;
                t.lp2_vol = 0;
            }
            0xEA => {
                t.lp2_cnt = t.lp2_cnt.wrapping_add(1);
                if t.lp2_cnt != a || a == 0 {
                    t.lp2_vol += i32::from(b as i8);
                    t.lp2_freq += i32::from(c as i8) * 8;
                    t.ptr = t.lp2_addr;
                }
            }
            0xEB => t.l3_addr = p,
            0xEC => {
                if t.l3_addr == 0 {
                    t.ptr = 0;
                } else {
                    t.ptr = t.l3_addr;
                }
            }
            0xED => {
                t.kakko_start = p;
                t.kakko_cnt = 0;
            }
            0xEE => match t.kakko_cnt {
                0 => t.kakko_cnt = 1,
                1 => {
                    t.kakko_cnt = 2;
                    t.kakko_end = p;
                    t.ptr = t.kakko_start;
                }
                2 => {
                    t.kakko_cnt = 1;
                    t.ptr = t.kakko_end;
                }
                _ => {}
            },
            0xEF => t.signal = true,
            0xF1 => {
                let v = &mut self.spu.voices[i];
                let am = if a == 0 { 0 } else { 0x8000 };
                let sm: u16 = match b {
                    1 => 0xC000,
                    0 => 0x4000,
                    2 => 0,
                    _ => 0x8000,
                };
                let rm = if c == 0 { 0 } else { 0x20 };
                v.adsr1 = (v.adsr1 & 0x7FFF) | am;
                v.adsr2 = (v.adsr2 & 0x1FDF) | sm | rm;
            }
            0xF2 => {
                self.key_off |= 1 << i;
                t.ngs = a;
                t.ngg = 0;
                t.vol = 0;
                t.ngc = a;
                t.ngo = 0;
            }
            0xF3 => {
                t.ngs = a;
                t.ngg = b;
                t.ngc = a;
                t.ngo = ((u32::from(b) * u32::from(a)) / 100).max(1) as u8;
            }
            0xF8 => {
                if t.dyn_seen != self.level {
                    t.mix_cur = i32::from(a) * 0x101;
                    t.mix_target = t.mix_cur;
                    t.mix_step = 0;
                }
                t.dyn_mode = c;
                t.dyn_vol = [a; 8];
                t.dyn_time = [if c == 1 { 0 } else { 0xFF }; 8];
                t.dyn_time[0] = b;
            }
            0xF9 => fill(t, 1, a, b, t.dyn_mode == 1),
            0xFA => fill(t, 2, a, b, t.dyn_mode != 0),
            0xFB => fill(t, 3, a, b, t.dyn_mode != 0),
            0xFC => fill(t, 4, a, b, t.dyn_mode == 1),
            0xCD => fill(t, 5, a, b, t.dyn_mode == 1),
            0xCE => fill(t, 6, a, b, t.dyn_mode == 1),
            0xCF => {
                t.dyn_vol[7] = a;
                t.dyn_time[7] = b;
            }
            0xFF => self.key_off |= 1 << i,
            _ => {}
        }
    }
}

/// What a [`Player`] should play: song (0 = silence), music level and
/// sound effects, set from another thread.
#[derive(Default)]
pub struct Control {
    song: AtomicU32,
    level: AtomicU32,
    effects: std::sync::Mutex<Vec<u32>>,
    /// Per song track: 0, or 0x8000_0000 | pan << 8 | volume (0xfb commands).
    tracks: [AtomicU32; 32],
}

impl Control {
    pub fn set(&self, song: u32, level: u8) {
        self.song.store(song, Ordering::Relaxed);
        self.level.store(u32::from(level), Ordering::Relaxed);
    }

    pub fn song(&self) -> u32 {
        self.song.load(Ordering::Relaxed)
    }

    /// Song track `t`'s pan (0..63) and volume (0..63), as the driver's
    /// `0xfb` command.
    pub fn track(&self, t: usize, pan: u8, vol: u8) {
        if let Some(a) = self.tracks.get(t) {
            a.store(0x8000_0000 | u32::from(pan & 0x3F) << 8 | u32::from(vol & 0x3F), Ordering::Relaxed);
        }
    }

    /// Queues sound effect `cmd` (`pan << 18 | volume << 12 | id`).
    pub fn effect(&self, cmd: u32) {
        if let Ok(mut q) = self.effects.lock() {
            q.push(cmd);
        }
    }
}

/// Streams a pack live (interleaved stereo at 48 kHz): a new song starts
/// from its beginning, a new level reaches the playing song's tracks as the
/// driver's level commands do, and queued effects start on the next tick.
pub struct Player {
    pack: Arc<Pack>,
    control: Arc<Control>,
    seq: Sequencer,
    song: u32,
    buf: Vec<i16>,
    pos: usize,
}

impl Player {
    pub fn new(pack: Arc<Pack>, control: Arc<Control>) -> Self {
        Player { seq: Sequencer::new(pack.clone(), 0), pack, control, song: 0, buf: Vec::new(), pos: 0 }
    }

    /// The next sample (left, right, left, ...).
    pub fn next_sample(&mut self) -> i16 {
        if self.pos >= self.buf.len() {
            self.refill();
        }
        let s = self.buf.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        s
    }

    fn refill(&mut self) {
        self.buf.clear();
        self.pos = 0;
        let song = self.control.song();
        self.seq.level = self.control.level.load(Ordering::Relaxed) as i32;
        if song != self.song {
            self.song = song;
            self.seq.start_song(song);
        }
        let effects = self.control.effects.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
        for cmd in effects {
            self.seq.play_effect(cmd);
        }
        for (t, a) in self.control.tracks.iter().enumerate() {
            let v = a.load(Ordering::Relaxed);
            if v & 0x8000_0000 != 0 {
                let track = &mut self.seq.tracks[t];
                let vol = (v & 0x3F) as i32;
                track.ext_pan = (v >> 8) & 0x3F;
                track.mix_cur = vol * 0x410 + (vol >> 2);
                track.mix_target = track.mix_cur;
                track.mix_step = 0;
            }
        }
        self.seq.tick();
        self.seq.spu.mix(&self.pack.spu, TICK_SAMPLES, &mut self.buf);
    }
}
/// Interleaved stereo samples as a WAV file.
pub fn wav(samples: &[i16]) -> Vec<u8> {
    crate::vox::Clip { channels: 2, rate: RATE, samples: samples.to_vec(), frames: 0 }.wav()
}
