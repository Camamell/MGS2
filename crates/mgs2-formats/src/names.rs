//! Reverse lookup of `str_code` hashes from candidate names.

use std::collections::HashMap;

use crate::cipher::str_code;

/// Script command and common identifier names. GCL command names are known
/// from Metal Gear Solid (1998) and confirmed here only when their hash
/// appears in MGS2 scripts.
pub const BUILTIN: &[&str] = &[
    "if", "eval", "return", "foreach", "mesg", "trap", "chara", "map", "hzd", "camera", "light",
    "start", "load", "radio", "strstatus", "demo", "ntrap", "delay", "pad", "varsave", "system",
    "sound", "menu", "rand", "func", "print", "jimaku", "unlock", "restart", "sleep", "wait",
    "set", "unset", "loop", "switch", "message", "bgm", "se", "voice", "weather", "rain", "sight",
    "door", "item", "weapon", "group", "area", "event", "route", "enemy", "player", "snake",
    "raiden", "olga", "otacon", "vamp", "ocelot", "guard", "game", "stage", "scenerio", "scenario",
    "init", "main", "end", "exit", "kill", "die", "dead", "alert", "evasion", "caution", "clear",
    "save", "continue", "gameover", "title", "option", "camera2", "target", "motion", "effect",
    "particle", "fog", "sun", "wind", "lamp", "ship", "tanker", "plant", "deck", "bridge", "hold",
    "engine", "mesh", "model", "texture", "anime", "anim", "pos", "dir", "rot", "x", "y", "z",
    "time", "timer", "count", "flag", "mode", "type", "on", "off", "true", "false", "add",
    "sub", "call", "goto", "label", "break", "while", "for", "do", "else", "elseif", "select",
    "case", "default", "proc", "obj", "object", "actor", "actors", "obj_set", "regist", "remove",
    "create", "delete", "kill_all", "watcher", "soldier", "sentry", "gurlukovich", "solid",
    "liquid", "codec", "call_codec", "jaw", "face", "mouth", "eye", "hand", "body", "head",
    "life", "o2", "stamina", "ration", "rations", "items", "weapons", "equip", "use", "get",
    "put", "drop", "throw", "shoot", "fire", "reload", "aim", "hit", "damage", "attack",
    "punch", "kick", "hold_up", "choke", "drag", "lock", "open", "close", "push", "pull",
];

/// Collects identifier-like ASCII runs (length 2..=32) from binary data, plus
/// `foo` for every `foo.c` source name.
pub fn harvest(data: &[u8], out: &mut Vec<String>) {
    let mut start = None;
    for (i, &c) in data.iter().chain(std::iter::once(&0)).enumerate() {
        let ident = c.is_ascii_alphanumeric() || c == b'_' || c == b'.';
        match (ident, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                let len = i - s;
                if (2..=32).contains(&len) {
                    let w = String::from_utf8_lossy(&data[s..i]).into_owned();
                    for part in w.split('.') {
                        if part.len() >= 2 {
                            out.push(part.to_string());
                        }
                    }
                }
                start = None;
            }
            _ => {}
        }
    }
}

#[derive(Clone, Default)]
pub struct Names {
    /// hash -> (name, is_dictionary_guess)
    map: HashMap<u32, (String, bool)>,
}

impl Names {
    pub fn with_builtin() -> Self {
        let mut n = Self::default();
        n.extend(BUILTIN.iter().copied());
        n
    }

    /// Adds names believed to occur in the data (built-ins, strings harvested
    /// from the same stage). These replace dictionary guesses.
    pub fn extend<S: AsRef<str>>(&mut self, words: impl IntoIterator<Item = S>) {
        self.insert(words, false);
    }

    fn insert<S: AsRef<str>>(&mut self, words: impl IntoIterator<Item = S>, guess: bool) {
        for w in words {
            let w = w.as_ref();
            for v in [w.to_string(), w.to_ascii_lowercase()] {
                let h = str_code(v.as_bytes());
                match self.map.get(&h) {
                    Some((_, false)) => {}
                    Some((_, true)) if guess => {}
                    _ => {
                        self.map.insert(h, (v, guess));
                    }
                }
            }
        }
    }

    /// Adds names from every `.txt` file under `dir` (JoyDict layout: one
    /// name per line, optionally `hash.x|name.ext`); extensions are dropped.
    /// With 24-bit hashes a large list produces false matches, so these are
    /// kept as guesses and shown with a `~` prefix. Returns the number of
    /// distinct names read.
    pub fn load_dir(&mut self, dir: &std::path::Path) -> std::io::Result<usize> {
        let mut words = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d)?.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "txt") {
                    for line in std::fs::read_to_string(&p).unwrap_or_default().lines() {
                        let name = line.rsplit('|').next().unwrap_or(line).trim();
                        let stem = name.split('.').next().unwrap_or(name);
                        if !stem.is_empty() {
                            words.push(stem.to_string());
                        }
                    }
                }
            }
        }
        words.sort();
        words.dedup();
        let n = words.len();
        self.insert(words, true);
        Ok(n)
    }

    pub fn get(&self, hash: u32) -> Option<&str> {
        self.map.get(&(hash & 0x00FF_FFFF)).map(|(s, _)| s.as_str())
    }

    /// Name if known (`~name` for dictionary guesses), otherwise `#xxxxxx`.
    pub fn show(&self, hash: u32) -> String {
        match self.map.get(&(hash & 0x00FF_FFFF)) {
            Some((s, false)) => s.clone(),
            Some((s, true)) => format!("~{s}"),
            None => format!("#{:06x}", hash & 0x00FF_FFFF),
        }
    }
}
