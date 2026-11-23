//! Sounds: playing `Sound` instances while a game runs.
//!
//! A sound name is a WAV or OGG file, looked for in the same folders as
//! pictures (see [`crate::pictures::set_folders`]). Names starting with
//! `libroxel://` are sounds made by Libroxel itself: `coin`, `jump`, `boom`,
//! `click`, `splash`, `chime` and `hum` (which loops smoothly), the `wood/`
//! chess sounds (`move`, `capture`, `notify`, `lowtime`, `confirm` and
//! `social`), and the
//! files in [`FILES`] (`lichess/sfx/move`, `aisleriot/click` and so on).
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
pub const SOUNDS: &[&str] = &[
    "coin",
    "jump",
    "boom",
    "click",
    "splash",
    "chime",
    "hum",
    "wood/move",
    "wood/capture",
    "wood/notify",
    "wood/lowtime",
    "wood/confirm",
    "wood/social",
];

/// Sound files built in, by name after [`BUILT_IN`]. See `assets/SOUNDS.md`.
pub const FILES: &[(&str, &[u8])] = &[
    ("aisleriot/click", include_bytes!("../assets/sounds/aisleriot/click.ogg")),
    ("aisleriot/slide", include_bytes!("../assets/sounds/aisleriot/slide.ogg")),
    ("aisleriot/splat", include_bytes!("../assets/sounds/aisleriot/splat.ogg")),
    ("aisleriot/victory", include_bytes!("../assets/sounds/aisleriot/victory.ogg")),
    ("four-in-a-row/bad", include_bytes!("../assets/sounds/four-in-a-row/bad.ogg")),
    ("four-in-a-row/bonus", include_bytes!("../assets/sounds/four-in-a-row/bonus.ogg")),
    ("four-in-a-row/reverse", include_bytes!("../assets/sounds/four-in-a-row/reverse.ogg")),
    ("four-in-a-row/slide", include_bytes!("../assets/sounds/four-in-a-row/slide.ogg")),
    ("lichess/futuristic/capture", include_bytes!("../assets/sounds/lichess/futuristic/capture.ogg")),
    ("lichess/futuristic/challenge", include_bytes!("../assets/sounds/lichess/futuristic/challenge.ogg")),
    ("lichess/futuristic/check", include_bytes!("../assets/sounds/lichess/futuristic/check.ogg")),
    ("lichess/futuristic/confirm", include_bytes!("../assets/sounds/lichess/futuristic/confirm.ogg")),
    ("lichess/futuristic/defeat", include_bytes!("../assets/sounds/lichess/futuristic/defeat.ogg")),
    ("lichess/futuristic/draw", include_bytes!("../assets/sounds/lichess/futuristic/draw.ogg")),
    ("lichess/futuristic/lowtime", include_bytes!("../assets/sounds/lichess/futuristic/lowtime.ogg")),
    ("lichess/futuristic/message", include_bytes!("../assets/sounds/lichess/futuristic/message.ogg")),
    ("lichess/futuristic/move", include_bytes!("../assets/sounds/lichess/futuristic/move.ogg")),
    ("lichess/futuristic/notify", include_bytes!("../assets/sounds/lichess/futuristic/notify.ogg")),
    ("lichess/futuristic/victory", include_bytes!("../assets/sounds/lichess/futuristic/victory.ogg")),
    ("lichess/nes/capture", include_bytes!("../assets/sounds/lichess/nes/capture.ogg")),
    ("lichess/nes/challenge", include_bytes!("../assets/sounds/lichess/nes/challenge.ogg")),
    ("lichess/nes/check", include_bytes!("../assets/sounds/lichess/nes/check.ogg")),
    ("lichess/nes/confirm", include_bytes!("../assets/sounds/lichess/nes/confirm.ogg")),
    ("lichess/nes/defeat", include_bytes!("../assets/sounds/lichess/nes/defeat.ogg")),
    ("lichess/nes/draw", include_bytes!("../assets/sounds/lichess/nes/draw.ogg")),
    ("lichess/nes/lowtime", include_bytes!("../assets/sounds/lichess/nes/lowtime.ogg")),
    ("lichess/nes/message", include_bytes!("../assets/sounds/lichess/nes/message.ogg")),
    ("lichess/nes/move", include_bytes!("../assets/sounds/lichess/nes/move.ogg")),
    ("lichess/nes/notify", include_bytes!("../assets/sounds/lichess/nes/notify.ogg")),
    ("lichess/nes/victory", include_bytes!("../assets/sounds/lichess/nes/victory.ogg")),
    ("lichess/piano/capture", include_bytes!("../assets/sounds/lichess/piano/capture.ogg")),
    ("lichess/piano/challenge", include_bytes!("../assets/sounds/lichess/piano/challenge.ogg")),
    ("lichess/piano/check", include_bytes!("../assets/sounds/lichess/piano/check.ogg")),
    ("lichess/piano/confirm", include_bytes!("../assets/sounds/lichess/piano/confirm.ogg")),
    ("lichess/piano/defeat", include_bytes!("../assets/sounds/lichess/piano/defeat.ogg")),
    ("lichess/piano/draw", include_bytes!("../assets/sounds/lichess/piano/draw.ogg")),
    ("lichess/piano/lowtime", include_bytes!("../assets/sounds/lichess/piano/lowtime.ogg")),
    ("lichess/piano/message", include_bytes!("../assets/sounds/lichess/piano/message.ogg")),
    ("lichess/piano/move", include_bytes!("../assets/sounds/lichess/piano/move.ogg")),
    ("lichess/piano/notify", include_bytes!("../assets/sounds/lichess/piano/notify.ogg")),
    ("lichess/piano/victory", include_bytes!("../assets/sounds/lichess/piano/victory.ogg")),
    ("lichess/sfx/capture", include_bytes!("../assets/sounds/lichess/sfx/capture.ogg")),
    ("lichess/sfx/challenge", include_bytes!("../assets/sounds/lichess/sfx/challenge.ogg")),
    ("lichess/sfx/check", include_bytes!("../assets/sounds/lichess/sfx/check.ogg")),
    ("lichess/sfx/confirm", include_bytes!("../assets/sounds/lichess/sfx/confirm.ogg")),
    ("lichess/sfx/defeat", include_bytes!("../assets/sounds/lichess/sfx/defeat.ogg")),
    ("lichess/sfx/draw", include_bytes!("../assets/sounds/lichess/sfx/draw.ogg")),
    ("lichess/sfx/lowtime", include_bytes!("../assets/sounds/lichess/sfx/lowtime.ogg")),
    ("lichess/sfx/message", include_bytes!("../assets/sounds/lichess/sfx/message.ogg")),
    ("lichess/sfx/move", include_bytes!("../assets/sounds/lichess/sfx/move.ogg")),
    ("lichess/sfx/notify", include_bytes!("../assets/sounds/lichess/sfx/notify.ogg")),
    ("lichess/sfx/victory", include_bytes!("../assets/sounds/lichess/sfx/victory.ogg")),
    ("lidraughts/capture", include_bytes!("../assets/sounds/lidraughts/capture.ogg")),
    ("lidraughts/challenge", include_bytes!("../assets/sounds/lidraughts/challenge.ogg")),
    ("lidraughts/confirm", include_bytes!("../assets/sounds/lidraughts/confirm.ogg")),
    ("lidraughts/defeat", include_bytes!("../assets/sounds/lidraughts/defeat.ogg")),
    ("lidraughts/draw", include_bytes!("../assets/sounds/lidraughts/draw.ogg")),
    ("lidraughts/message", include_bytes!("../assets/sounds/lidraughts/message.ogg")),
    ("lidraughts/move", include_bytes!("../assets/sounds/lidraughts/move.ogg")),
    ("lidraughts/notify", include_bytes!("../assets/sounds/lidraughts/notify.ogg")),
    ("lidraughts/victory", include_bytes!("../assets/sounds/lidraughts/victory.ogg")),
];

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
        if let Some((_, bytes)) = FILES.iter().find(|(n, _)| *n == built_in) {
            return Some(bytes.to_vec());
        }
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
        "wood/move" => (0..len(0.1)).map(|i| knock(t(i), 240.0, 0.0, &mut noise)).collect(),
        "wood/capture" => (0..len(0.16))
            .map(|i| {
                let t = t(i);
                knock(t, 300.0, 0.0, &mut noise) * 0.9 + knock(t, 190.0, 0.022, &mut noise) * 0.8
            })
            .collect(),
        // A soft two-note bell.
        "wood/notify" => (0..len(1.0)).map(|i| bell(t(i), 784.0, 0.0) + bell(t(i), 587.0, 0.17)).collect(),
        // Three quiet beeps.
        "wood/lowtime" => (0..len(0.42))
            .map(|i| {
                let t = t(i);
                let k = (t / 0.14).floor();
                let local = t - k * 0.14;
                if local < 0.05 {
                    (TAU * 988.0 * t).sin() * 0.35 * (1.0 - local / 0.05) * (1.0 - k * 0.25)
                } else {
                    0.0
                }
            })
            .collect(),
        // A small pop.
        "wood/confirm" => {
            let mut phase = 0.0f32;
            (0..len(0.09))
                .map(|i| {
                    let t = t(i);
                    phase += TAU * (520.0 + 4000.0 * t) / rate;
                    phase.sin() * 0.5 * (-t * 45.0).exp()
                })
                .collect()
        }
        // Two plucked notes.
        "wood/social" => (0..len(0.5))
            .map(|i| {
                let t = t(i);
                let pluck = |f: f32, at: f32| {
                    if t < at {
                        0.0
                    } else {
                        let u = t - at;
                        ((TAU * f * u).sin() + 0.4 * (TAU * 2.0 * f * u).sin()) * 0.3 * (-u * 13.0).exp()
                    }
                };
                pluck(1047.0, 0.0) + pluck(1319.0, 0.09)
            })
            .collect(),
        _ => return None,
    };
    Some(samples)
}

/// A piece set down on a wooden board: a short thud with a click on top,
/// starting `at` seconds in.
fn knock(t: f32, freq: f32, at: f32, noise: &mut Noise) -> f32 {
    let grit = noise.next();
    if t < at {
        return 0.0;
    }
    let u = t - at;
    let body = (TAU * (freq - 60.0 * u / 0.1) * u).sin() * (-u * 60.0).exp();
    let click = grit * (-u * 900.0).exp();
    body * 0.75 + click * 0.45
}

/// A bell note starting `at` seconds in.
fn bell(t: f32, freq: f32, at: f32) -> f32 {
    if t < at {
        return 0.0;
    }
    let u = t - at;
    ((TAU * freq * u).sin() + 0.25 * (TAU * 2.76 * freq * u).sin()) * 0.32 * (-u * 5.0).exp()
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
    fn sound_files_are_ogg() {
        for (name, bytes) in FILES {
            assert_eq!(&bytes[0..4], b"OggS", "{name}");
            assert_eq!(bytes_for(&format!("{BUILT_IN}{name}")).as_deref(), Some(*bytes));
        }
        for set in ["sfx", "piano", "nes", "futuristic"] {
            assert!(bytes_for(&format!("{BUILT_IN}lichess/{set}/move")).is_some());
        }
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
