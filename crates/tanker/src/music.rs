//! Stage music from the sound driver (`mgs2_formats::sd`), streamed live.
//!
//! Stages that start `bgmanage.c` (`chara #70a68a`) pick the song from the
//! alert phase (`FUN_00230330` in the stage resident): sneaking plays song 5
//! at level 1, alert song 2 at level 2, evasion song 3 at level 3 and caution
//! song 4. Cutscenes bring their own audio, so the music stops while one
//! plays.

use std::num::NonZero;
use std::sync::Arc;
use std::time::Duration;

use bevy::audio::{AddAudioSource, Decodable, Source};
use bevy::prelude::*;
use mgs2_formats::gcx::{Gcx, Stmt, Value};
use mgs2_formats::sd::{Control, Pack, Player};
use mgs2_formats::str_code;

use crate::Game;
use mgs2_bevy::scene::SCALE;
use crate::guard::{Phase, PhaseKind};

/// `bgmanage.c`'s actor name.
const BGM_MANAGER: u32 = 0x70A68A;

/// A stage's music as an audio source: the decoder runs the sequencer.
#[derive(Asset, TypePath)]
pub struct SdMusic {
    pack: Arc<Pack>,
    control: Arc<Control>,
}

pub struct SdDecoder(Player);

impl Iterator for SdDecoder {
    type Item = bevy::audio::Sample;
    fn next(&mut self) -> Option<Self::Item> {
        Some(f32::from(self.0.next_sample()) / 32768.0)
    }
}

impl Source for SdDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> bevy::audio::ChannelCount {
        NonZero::new(2).unwrap()
    }
    fn sample_rate(&self) -> bevy::audio::SampleRate {
        NonZero::new(mgs2_formats::sd::RATE).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Decodable for SdMusic {
    type Decoder = SdDecoder;
    fn decoder(&self) -> SdDecoder {
        SdDecoder(Player::new(self.pack.clone(), self.control.clone()))
    }
}

pub fn plugin(app: &mut App) {
    app.add_audio_source::<SdMusic>().init_resource::<Sound>().add_systems(Update, update);
}

/// Whether a stage script starts the music manager.
fn has_manager(script: &Gcx) -> bool {
    fn walk(stmts: &[Stmt]) -> bool {
        stmts.iter().any(|s| match s {
            Stmt::Command { name, args, options } => {
                (*name == str_code(b"chara") && matches!(args.first(), Some(Value::Name(h)) if *h == BGM_MANAGER))
                    || args.iter().chain(options.iter().flat_map(|(_, v)| v)).any(|v| matches!(v, Value::Block(b) if walk(b)))
            }
            _ => false,
        })
    }
    walk(&script.main) || script.procs.iter().any(|p| walk(&p.body))
}

#[derive(Component)]
struct MusicPlayer;

/// The sound driver of the stage being played: its music and effects.
#[derive(Resource, Default)]
pub struct Sound {
    stage: String,
    managed: bool,
    control: Option<Arc<Control>>,
}

impl Sound {
    /// Plays sound effect `id` at full volume, centred.
    pub fn effect(&self, id: u32) {
        self.command(0x20 << 18 | 0x3F << 12 | id);
    }

    /// A raw driver command (`#f2eedc -c`): effects (high byte 0) play; the
    /// rest (fades, song control) is not modelled.
    pub fn command(&self, cmd: u32) {
        if cmd >> 24 == 0
            && cmd & 0x7FF != 0
            && let Some(c) = &self.control
        {
            debug!("sound effect {:#x} vol {} pan {}", cmd & 0x7FF, (cmd >> 12) & 0x3F, (cmd >> 18) & 0x3F);
            c.effect(cmd);
        }
    }
}

/// Starts a stage's sound driver when it loads; the music follows the alert
/// phase.
fn update(
    mut commands: Commands,
    mut game: ResMut<Game>,
    phase: Option<Res<Phase>>,
    cutscene: Res<crate::demo::Cutscene>,
    mut assets: ResMut<Assets<SdMusic>>,
    players: Query<Entity, With<MusicPlayer>>,
    mut sound: ResMut<Sound>,
    mut last: Local<Option<PhaseKind>>,
    cams: Query<&GlobalTransform, With<crate::player::FollowCam>>,
) {
    if sound.stage != game.data.name {
        for e in &players {
            commands.entity(e).despawn();
        }
        sound.stage = game.data.name.clone();
        sound.control = None;
        sound.managed = game.data.script.as_ref().is_some_and(has_manager);
        if let Some(bytes) = game.data.sound.as_ref() {
            match Pack::with_resident(game.data.resident_sound.as_deref(), bytes) {
                Some(pack) => {
                    info!("sound: {} has {} songs (music {})", game.data.name, pack.song_count(), if sound.managed { "on" } else { "off" });
                    let control = Arc::new(Control::default());
                    let handle = assets.add(SdMusic { pack: Arc::new(pack), control: control.clone() });
                    commands.spawn((AudioPlayer(handle), PlaybackSettings::LOOP.with_volume(bevy::audio::Volume::Linear(0.7)), MusicPlayer));
                    sound.control = Some(control);
                }
                None => warn!("sound: {}: unreadable sound pack", game.data.name),
            }
        }
    }
    for c in std::mem::take(&mut game.sound_cmds) {
        sound.command(c);
    }
    if let Ok(cam) = cams.single() {
        let (listener, right) = (cam.translation() / SCALE, cam.right().as_vec3());
        for c in crate::sfx::drain(listener, right) {
            sound.command(c);
        }
    }
    let kind = phase.map_or(PhaseKind::Infiltration, |p| p.kind);
    // bgmanage plays the alert sting (effect 0x2a) as the alert starts.
    if kind == PhaseKind::Alert && last.is_some_and(|k| k != PhaseKind::Alert) {
        sound.effect(0x2A);
    }
    *last = Some(kind);
    let Some(control) = &sound.control else { return };
    let (song, level) = if cutscene.active() || !sound.managed {
        (0, 0)
    } else {
        match kind {
            PhaseKind::Infiltration => (5, 1),
            PhaseKind::Alert => (2, 2),
            PhaseKind::Evasion => (3, 3),
            PhaseKind::Caution => (4, 3),
        }
    };
    if control.song() != song {
        info!("music: song {song} level {level}");
    }
    control.set(song, level);
}