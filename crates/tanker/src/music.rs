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
    app.add_audio_source::<SdMusic>().add_systems(Update, update);
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

/// The music of the stage being played.
#[derive(Default)]
struct Playing {
    stage: String,
    control: Option<Arc<Control>>,
}

/// Starts a stage's music when it loads and follows the alert phase.
fn update(
    mut commands: Commands,
    game: Res<Game>,
    phase: Option<Res<Phase>>,
    cutscene: Res<crate::demo::Cutscene>,
    mut assets: ResMut<Assets<SdMusic>>,
    players: Query<Entity, With<MusicPlayer>>,
    mut playing: Local<Playing>,
) {
    if playing.stage != game.data.name {
        for e in &players {
            commands.entity(e).despawn();
        }
        playing.stage = game.data.name.clone();
        playing.control = None;
        let managed = game.data.script.as_ref().is_some_and(has_manager);
        if let (true, Some(bytes)) = (managed, game.data.sound.as_ref()) {
            match Pack::parse(bytes) {
                Some(pack) => {
                    info!("music: {} has {} songs", game.data.name, pack.song_count());
                    let control = Arc::new(Control::default());
                    let handle = assets.add(SdMusic { pack: Arc::new(pack), control: control.clone() });
                    commands.spawn((AudioPlayer(handle), PlaybackSettings::LOOP.with_volume(bevy::audio::Volume::Linear(0.7)), MusicPlayer));
                    playing.control = Some(control);
                }
                None => warn!("music: {}: unreadable sound pack", game.data.name),
            }
        }
    }
    let Some(control) = &playing.control else { return };
    let (song, level) = if cutscene.active() {
        (0, 0)
    } else {
        match phase.map_or(PhaseKind::Infiltration, |p| p.kind) {
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
