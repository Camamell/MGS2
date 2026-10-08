//! Codec calls from CODEC.DAT.
//!
//! CODEC.DAT is a run of GCL script records (see `re/notes/formats.md`).
//! Stage scripts name a record by id: its low 24 bits times 16 is the
//! record's offset. A record's `main` picks the conversation from the story
//! variables; its `#3b91eb <speaker> .. -m str[n]` commands are the lines.
//!
//! K calls the frequency the area has set (141.12, Otacon); incoming calls
//! (`command #4a243a -c`) open by themselves. Space/Enter next line, Esc
//! hang up. Play pauses while the Codec is open.

use std::io::Read;

use bevy::prelude::*;
use mgs2_formats::gcx::{self, Gcx};

use crate::Game;
use crate::gcl::{Banks, Vm};

#[derive(Resource)]
pub struct CodecData {
    data: Vec<u8>,
}

impl CodecData {
    pub fn load(iso: &std::path::Path) -> anyhow::Result<Self> {
        let mut data = Vec::new();
        mgs2_formats::iso::Iso::open(iso)?.open_file("MGS2/CODEC.DAT")?.read_to_end(&mut data)?;
        Ok(Self { data })
    }

    /// The record a stage script's codec id points at.
    pub fn record(&self, id: u32) -> Option<Gcx> {
        let start = (id & 0x00FF_FFFF) as usize * 16;
        let stamp = self.data.get(..4)?;
        if self.data.get(start..start + 4)? != stamp {
            return None;
        }
        let end = (start + 16..self.data.len().saturating_sub(4))
            .step_by(16)
            .find(|&i| &self.data[i..i + 4] == stamp)
            .unwrap_or(self.data.len());
        gcx::parse(&self.data[start..end]).ok()
    }
}

/// Runs a record's `main` with the current variables and returns its lines
/// and the variables afterwards (calls set "heard" flags).
pub fn play(record: &Gcx, banks: Banks) -> (Vec<(u32, String)>, Banks) {
    let mut vm = Vm::with_banks(record, banks);
    vm.codec_mode = true;
    vm.run(&record.main);
    let lines = std::mem::take(&mut vm.lines);
    let (_, _, _, banks) = vm.finish();
    (lines, banks)
}

pub fn speaker(h: u32) -> &'static str {
    match h {
        0x4AB569 => "OTACON",
        0x28DCE6 => "SNAKE",
        _ => "???",
    }
}

/// An open call.
pub struct Call {
    pub freq: i32,
    pub lines: Vec<(u32, String)>,
    pub index: usize,
}

#[derive(Resource, Default)]
pub struct CodecState {
    pub call: Option<Call>,
}

pub fn idle(state: Res<CodecState>) -> bool {
    state.call.is_none()
}

#[derive(Component)]
pub struct CodecScreen;

pub fn setup_screen(mut commands: Commands) {
    commands.spawn((
        CodecScreen,
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(22.0), ..default() },
        TextColor(Color::srgb(0.55, 1.0, 0.65)),
        BackgroundColor(Color::srgba(0.0, 0.05, 0.02, 0.88)),
        Node {
            position_type: PositionType::Absolute,
            left: percent(10),
            right: percent(10),
            bottom: percent(8),
            padding: UiRect::all(px(18)),
            ..default()
        },
        Visibility::Hidden,
    ));
}

fn open(game: &mut Game, data: &CodecData, freq: i32, id: u32) -> Option<Call> {
    let record = data.record(id)?;
    let banks = std::mem::replace(&mut game.banks, Banks { a: Vec::new(), b: Vec::new() });
    let (lines, banks) = play(&record, banks);
    game.banks = banks;
    for (who, l) in &lines {
        info!("codec {}: {}", speaker(*who), l.replace('\n', " "));
    }
    (!lines.is_empty()).then_some(Call { freq, lines, index: 0 })
}

/// `--codec-at`: press K at this time (seconds) for automated tests.
#[derive(Resource)]
pub struct AutoCall(pub f32);

pub fn input(
    time: Res<Time<Real>>,
    auto: Option<ResMut<AutoCall>>,
    keys: Res<ButtonInput<KeyCode>>,
    data: Option<Res<CodecData>>,
    mut state: ResMut<CodecState>,
    mut game: ResMut<Game>,
) {
    let Some(data) = data else { return };
    if let Some(call) = state.call.as_mut() {
        if keys.just_pressed(KeyCode::Escape) {
            state.call = None;
        } else if keys.just_pressed(KeyCode::Space) || keys.just_pressed(KeyCode::Enter) {
            call.index += 1;
            if call.index >= call.lines.len() {
                state.call = None;
            }
        }
        return;
    }
    if let Some((freq, id)) = game.codec_calls.pop() {
        state.call = open(&mut game, &data, freq, id);
    } else if keys.just_pressed(KeyCode::KeyK) || auto.is_some_and(|mut a| a.0 <= time.elapsed_secs() && { a.0 = f32::MAX; true }) {
        let call = match game.codec_area {
            Some((freq, id)) => open(&mut game, &data, freq, id),
            None => None,
        };
        state.call = Some(call.unwrap_or(Call { freq: 14112, lines: vec![(0, "(no answer)".into())], index: 0 }));
    }
}

pub fn screen(state: Res<CodecState>, mut q: Query<(&mut Text, &mut Visibility), With<CodecScreen>>) {
    let Ok((mut t, mut v)) = q.single_mut() else { return };
    match &state.call {
        Some(call) => {
            *v = Visibility::Inherited;
            let (who, line) = &call.lines[call.index.min(call.lines.len() - 1)];
            **t = format!(
                "CODEC  {}.{:02}   [{}/{}]\n\n{}:\n{}\n\nSpace: next   Esc: end",
                call.freq / 100,
                call.freq % 100,
                call.index + 1,
                call.lines.len(),
                speaker(*who),
                line.trim_end()
            );
        }
        None => *v = Visibility::Hidden,
    }
}
