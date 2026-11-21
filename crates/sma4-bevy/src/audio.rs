//! Sound effects: the ROM's own renderings (`sma4_assets::sound`, extracted by running its
//! sound driver), started by the core's `Game::sounds`. As in the driver (0x080E7D54), a
//! sound started in a slot stops the one playing there.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bevy::audio::{AddAudioSource, ChannelCount, Decodable, Sample, SampleRate, Source};
use bevy::prelude::*;
use sma4_assets::sound::{SOUND_IDS, SOUND_RATE, SoundAsset};

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<Pcm>()
            .init_resource::<SoundQueue>()
            .add_systems(Startup, load_sounds)
            .add_systems(Update, play_sounds);
    }
}

/// Sounds the core started since the last rendered frame (`input::step_game` may run
/// several core frames per rendered one).
#[derive(Resource, Default)]
pub struct SoundQueue(pub Vec<u8>);

/// One effect: interleaved stereo samples at `SOUND_RATE`.
#[derive(Asset, TypePath, Clone)]
pub struct Pcm {
    samples: Arc<[Sample]>,
}

pub struct PcmDecoder {
    samples: Arc<[Sample]>,
    at: usize,
}

impl Iterator for PcmDecoder {
    type Item = Sample;
    fn next(&mut self) -> Option<Sample> {
        let s = self.samples.get(self.at).copied();
        self.at += 1;
        s
    }
}

impl Source for PcmDecoder {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.samples.len().saturating_sub(self.at))
    }
    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).unwrap()
    }
    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(SOUND_RATE).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(self.samples.len() as f64 / 2.0 / SOUND_RATE as f64))
    }
}

impl Decodable for Pcm {
    type Decoder = PcmDecoder;
    fn decoder(&self) -> PcmDecoder {
        PcmDecoder { samples: self.samples.clone(), at: 0 }
    }
}

#[derive(Resource, Default)]
struct Sounds(HashMap<u8, (u8, Handle<Pcm>)>);

/// The sound playing in a driver slot.
#[derive(Component)]
struct Playing(u8);

fn load_sounds(mut commands: Commands, dir: Res<crate::AssetDir>, mut pcm: ResMut<Assets<Pcm>>) {
    let mut sounds = HashMap::new();
    for &id in SOUND_IDS {
        match SoundAsset::read(&dir.0, id) {
            Ok(s) => {
                // The GBA's 8-bit DAC: a sample of 0x7F is the top.
                let samples: Arc<[Sample]> = s.samples.iter().map(|&v| v as Sample / 128.0).collect();
                sounds.insert(id, (s.slot, pcm.add(Pcm { samples })));
            }
            Err(e) => warn!("{e}; re-run sma4-extract for the sound effects"),
        }
    }
    commands.insert_resource(Sounds(sounds));
}

fn play_sounds(
    mut commands: Commands,
    mut queue: ResMut<SoundQueue>,
    sounds: Res<Sounds>,
    playing: Query<(Entity, &Playing)>,
) {
    for id in queue.0.drain(..) {
        let Some((slot, handle)) = sounds.0.get(&id) else { continue };
        for (e, p) in &playing {
            if p.0 == *slot {
                commands.entity(e).despawn();
            }
        }
        commands.spawn((AudioPlayer(handle.clone()), PlaybackSettings::DESPAWN, Playing(*slot)));
    }
}
