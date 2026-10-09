//! The holds' time limit (`chara #9b65f0`, w04 residents `FUN_0028fe00` /
//! `FUN_0028f890`).
//!
//! `-t frames` (the scripts pass `Au32:0x70`, 25200 = 7 minutes from the
//! title script), `-p proc` run when it reaches zero, `-s flags` (bit 0:
//! start paused). `mesg #9b65f0 #000000` pauses it, `#000001` runs it;
//! `command #450c64` reads the frames left (the scripts compare it with the
//! speech and save it back to `Au32:0x70` between the holds). It counts
//! 60 a second on screen as minutes, seconds and hundredths, ticking each
//! second (effect 0x585, 0xcf in the last ten seconds).

use crate::gcl::{Spawn, Val};

pub const TIMER_ACTOR: u32 = 0x9B65F0;
const TICK: u32 = 0x585;
const TICK_LAST: u32 = 0xCF;

#[derive(Clone, Debug)]
pub struct Timer {
    /// Frames left (60 a second).
    pub left: f32,
    pub paused: bool,
    pub proc: Option<u32>,
}

impl Timer {
    pub fn from_spawn(s: &Spawn) -> Option<Timer> {
        let left = s.opt_int(b't', 0)? as f32;
        let flags = s.opt_int(b's', 0).unwrap_or(0);
        let proc = match s.opt(b'p').and_then(|v| v.first()) {
            Some(Val::Proc(p)) => Some(*p),
            _ => None,
        };
        Some(Timer { left, paused: flags & 1 != 0, proc })
    }

    pub fn frames(&self) -> i32 {
        self.left.max(0.0) as i32
    }

    /// Advances by `dt` seconds; returns the proc to run when it runs out.
    pub fn tick(&mut self, dt: f32) -> Option<u32> {
        if self.paused || self.left <= 0.0 {
            return None;
        }
        let before = self.frames();
        self.left = (self.left - dt * 60.0).max(0.0);
        let after = self.frames();
        if after > 0 && before / 60 != after / 60 {
            crate::sfx::play(if after < 600 { TICK_LAST } else { TICK }, None);
        }
        if after == 0 { self.proc.take() } else { None }
    }

    /// `mm:ss:cc` as the game shows it.
    pub fn text(&self) -> String {
        let f = self.frames();
        format!("{:02}:{:02}:{:02}", f / 3600, (f / 60) % 60, (f % 60) * 100 / 60)
    }
}
