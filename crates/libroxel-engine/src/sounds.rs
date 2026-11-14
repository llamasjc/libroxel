//! Sounds: playing `Sound` instances while a game runs.
//!
//! A sound name is a WAV or OGG file, looked for in the same folders as
//! pictures (see [`crate::pictures::set_folders`]). Names starting with
//! `libroxel://` are sounds made by Libroxel itself: `coin`, `jump`, `boom`,
//! `click`, `splash`, `chime` and `hum` (which loops smoothly).
//!
//! A sound inside a part sounds quieter the further the camera is from it:
//! full volume within 10 studs, then falling off until it can't be heard at
//! [`HEARING_DISTANCE`]. Sounds anywhere else play at their own volume.

use std::collections::HashMap;
use std::f32::consts::TAU;

use bevy::audio::{
    AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackMode, PlaybackSettings, Volume,
};
use bevy::prelude::*;
use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property};

use crate::pictures::BUILT_IN;
use crate::view::{EngineState, to_bevy};

/// Built-in sounds, by name after [`BUILT_IN`].
pub const SOUNDS: &[&str] = &["coin", "jump", "boom", "click", "splash", "chime", "hum"];

/// Samples per second of the built-in sounds.
const RATE: u32 = 22_050;

/// Within this many studs a sound in a part plays at its full volume.
const NEAR: f32 = 10.0;

/// Past this many studs a sound in a part can't be heard.
pub const HEARING_DISTANCE: f32 = 200.0;

/// The file a sound name means, if it can be found.
pub fn bytes_for(name: &str) -> Option<Vec<u8>> {
    let name = name.trim();
    if let Some(built_in) = name.strip_prefix(BUILT_IN) {
        return synth(built_in).map(|samples| wav(&samples));
    }
    crate::pictures::bytes_for_kind(name, &["ogg", "wav", "mp3"])
}

/// How loud a sound in a part is, from 0 to 1, `distance` studs away.
pub fn falloff(distance: f32) -> f32 {
    if distance <= NEAR {
        1.0
    } else if distance >= HEARING_DISTANCE {
        0.0
    } else {
        // Inverse distance, eased to nothing at the edge of hearing.
        let edge = 1.0 - (distance - NEAR) / (HEARING_DISTANCE - NEAR);
        (NEAR / distance) * edge.sqrt()
    }
}

/// A small, repeatable noise source.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// The samples of a built-in sound, from -1 to 1.
pub fn synth(name: &str) -> Option<Vec<f32>> {
    let rate = RATE as f32;
    let len = |seconds: f32| (seconds * rate) as usize;
    let t = |i: usize| i as f32 / rate;
    let mut noise = Noise(0x9e37_79b9);
    let samples: Vec<f32> = match name {
        // Two bright notes, the second ringing out.
        "coin" => (0..len(0.4))
            .map(|i| {
                let t = t(i);
                let (freq, start) = if t < 0.07 { (988.0, 0.0) } else { (1319.0, 0.07) };
                let tone = (TAU * freq * t).sin().signum() * 0.35 + (TAU * freq * t).sin() * 0.25;
                tone * (-(t - start) * 9.0).exp()
            })
            .collect(),
        // A quick rising sweep.
        "jump" => {
            let mut phase = 0.0f32;
            (0..len(0.22))
                .map(|i| {
                    let t = t(i);
                    phase += TAU * (260.0 + 2200.0 * t) / rate;
                    phase.sin() * 0.5 * (1.0 - t / 0.22)
                })
                .collect()
        }
        // A rumble that dies away.
        "boom" => {
            let mut low = 0.0f32;
            (0..len(1.2))
                .map(|i| {
                    let t = t(i);
                    low += (noise.next() - low) * 0.04;
                    let thump = (TAU * 48.0 * t).sin() * (-t * 5.0).exp();
                    ((low * 5.0 + thump * 0.6) * (-t * 3.0).exp()).clamp(-0.9, 0.9)
                })
                .collect()
        }
        // A very short tick.
        "click" => (0..len(0.03))
            .map(|i| {
                let t = t(i);
                ((TAU * 1800.0 * t).sin() * 0.5 + noise.next() * 0.2) * (-t * 160.0).exp()
            })
            .collect(),
        // Hissing water that swells and fades.
        "splash" => {
            let mut soft = 0.0f32;
            (0..len(0.7))
                .map(|i| {
                    let t = t(i);
                    soft += (noise.next() - soft) * 0.3;
                    soft * 0.9 * (t * 40.0).min(1.0) * (-t * 5.0).exp()
                })
                .collect()
        }
        // A bell: a note and its overtones, each fading at its own pace.
        "chime" => (0..len(1.5))
            .map(|i| {
                let t = t(i);
                [(1.0, 0.4, 3.0), (2.0, 0.2, 5.0), (3.01, 0.1, 8.0)]
                    .iter()
                    .map(|(n, amp, decay)| (TAU * 880.0 * n * t).sin() * amp * (-t * decay).exp())
                    .sum()
            })
            .collect(),
        // One second of a soft low chord; every note fits a whole number of
        // times, so it loops without a click.
        "hum" => (0..len(1.0))
            .map(|i| {
                let t = t(i);
                (TAU * 110.0 * t).sin() * 0.25
                    + (TAU * 165.0 * t).sin() * 0.15
                    + (TAU * 220.0 * t).sin() * 0.08
            })
            .collect(),
        _ => return None,
    };
    Some(samples)
}

/// A mono 16-bit WAV file of `samples`.
pub fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // plain samples
    out.extend_from_slice(&1u16.to_le_bytes()); // one channel
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}

/// Plays `Sound`s during a game. Needs Bevy's audio plugin (part of
/// `DefaultPlugins`); without it, it does nothing.
pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Playing>().add_systems(Update, play_sounds.after(crate::view::EngineSet::Sync));
    }
}

/// One sound being played.
struct Voice {
    entity: Entity,
    name: String,
    looped: bool,
}

#[derive(Resource, Default)]
struct Playing {
    voices: HashMap<InstanceId, Voice>,
    /// Loaded sounds by name (`None` when the file couldn't be found).
    loaded: HashMap<String, Option<Handle<AudioSource>>>,
    generation: u64,
}

/// How one sound should be playing right now.
pub struct SoundState {
    pub name: String,
    pub playing: bool,
    pub looped: bool,
    /// Volume after distance, 0 to 10.
    pub volume: f32,
    pub speed: f32,
}

/// The sounds that can be heard: those in the workspace or under players.
pub fn audible_sounds(tree: &InstanceTree, listener: Option<Vec3>) -> Vec<(InstanceId, SoundState)> {
    let mut roots = vec![tree.workspace()];
    roots.extend(tree.service(ClassKind::Players));
    let mut out = Vec::new();
    for root in roots {
        for id in tree.descendants_of_class(root, ClassKind::Sound) {
            let name = tree.get_string(id, Property::SoundId).unwrap_or_default();
            let mut volume = tree.get_number(id, Property::Volume).unwrap_or(0.5).clamp(0.0, 10.0) as f32;
            let parent = tree.parent(id).ok().flatten();
            let at = parent
                .filter(|p| tree.class(*p).is_ok_and(|c| c.is_physical()))
                .and_then(|p| tree.get_vec3(p, Property::Position));
            if let (Some(at), Some(listener)) = (at, listener) {
                volume *= falloff(to_bevy(at).distance(listener));
            }
            out.push((
                id,
                SoundState {
                    name,
                    playing: tree.get_bool(id, Property::Playing) == Some(true),
                    looped: tree.get_bool(id, Property::Looped) == Some(true),
                    volume,
                    speed: tree.get_number(id, Property::PlaybackSpeed).unwrap_or(1.0).clamp(0.05, 20.0)
                        as f32,
                },
            ));
        }
    }
    out
}

fn play_sounds(
    mut engine: NonSendMut<EngineState>,
    mut playing: ResMut<Playing>,
    sources: Option<ResMut<Assets<AudioSource>>>,
    mut sinks: Query<&mut AudioSink>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut commands: Commands,
) {
    let Some(mut sources) = sources else { return };
    let playing = &mut *playing;
    let engine = &mut *engine;
    let Some(session) = engine.session.as_mut() else {
        // Stopped: everything goes quiet.
        for (_, voice) in playing.voices.drain() {
            commands.entity(voice.entity).despawn();
        }
        return;
    };
    if playing.generation != crate::pictures::generation() {
        playing.generation = crate::pictures::generation();
        playing.loaded.clear();
    }
    // Play and Stop start a sound over.
    for id in session.take_sound_restarts() {
        if let Some(voice) = playing.voices.remove(&id) {
            commands.entity(voice.entity).despawn();
        }
    }
    let listener = camera.iter().next().map(|t| t.translation());
    let sounds = audible_sounds(&engine.tree.borrow(), listener);

    let live: std::collections::HashSet<InstanceId> = sounds.iter().map(|(id, _)| *id).collect();
    playing.voices.retain(|id, voice| {
        let keep = live.contains(id);
        if !keep {
            commands.entity(voice.entity).despawn();
        }
        keep
    });

    let mut ended = Vec::new();
    for (id, sound) in sounds {
        // A different file or looping now: start again from scratch.
        if let Some(voice) = playing.voices.get(&id)
            && (voice.name != sound.name || voice.looped != sound.looped)
        {
            commands.entity(voice.entity).despawn();
            playing.voices.remove(&id);
        }
        match playing.voices.get(&id) {
            Some(voice) => {
                let Ok(mut sink) = sinks.get_mut(voice.entity) else { continue };
                sink.set_volume(Volume::Linear(sound.volume));
                sink.set_speed(sound.speed);
                if sink.empty() {
                    if sound.playing && !sound.looped {
                        ended.push(id);
                    }
                    commands.entity(voice.entity).despawn();
                    playing.voices.remove(&id);
                } else if sound.playing && sink.is_paused() {
                    sink.play();
                } else if !sound.playing && !sink.is_paused() {
                    sink.pause();
                }
            }
            None if sound.playing => {
                let handle = playing
                    .loaded
                    .entry(sound.name.clone())
                    .or_insert_with(|| {
                        let bytes = bytes_for(&sound.name);
                        if bytes.is_none() && !sound.name.trim().is_empty() {
                            warn!("couldn't find the sound {:?}", sound.name);
                        }
                        bytes.map(|b| sources.add(AudioSource { bytes: b.into() }))
                    })
                    .clone();
                let Some(handle) = handle else { continue };
                let settings = PlaybackSettings {
                    mode: if sound.looped { PlaybackMode::Loop } else { PlaybackMode::Once },
                    volume: Volume::Linear(sound.volume),
                    speed: sound.speed,
                    ..default()
                };
                let entity = commands.spawn((AudioPlayer(handle), settings)).id();
                playing.voices.insert(id, Voice { entity, name: sound.name, looped: sound.looped });
            }
            None => {}
        }
    }
    for id in ended {
        session.sound_ended(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_sounds_are_wav_files() {
        for name in SOUNDS {
            let bytes = bytes_for(&format!("{BUILT_IN}{name}")).expect("built in");
            assert_eq!(&bytes[0..4], b"RIFF", "{name}");
            assert_eq!(&bytes[8..12], b"WAVE");
            let samples = (bytes.len() - 44) / 2;
            assert!(samples > 500, "{name} is {samples} samples");
            // Not silent, and not clipping everywhere.
            let s = synth(name).unwrap();
            let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.1 && peak <= 1.0, "{name} peaks at {peak}");
        }
        assert!(bytes_for("libroxel://nothing").is_none());
        // The hum loops: its ends meet.
        let hum = synth("hum").unwrap();
        assert!((hum[0] - hum[hum.len() - 1]).abs() < 0.1);
    }

    #[test]
    fn far_sounds_are_quieter() {
        assert_eq!(falloff(0.0), 1.0);
        assert_eq!(falloff(NEAR), 1.0);
        assert!(falloff(40.0) < 0.3 && falloff(40.0) > 0.05);
        assert!(falloff(150.0) < falloff(40.0));
        assert_eq!(falloff(HEARING_DISTANCE), 0.0);
    }
}
