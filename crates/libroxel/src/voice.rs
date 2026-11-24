use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use bevy_egui::egui;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use libroxel_engine::PlaySession;
use libroxel_engine::voice::{FRAME, RATE};
use libroxel_hub::{Chat, HubClient};
use libroxel_instance::{InstanceId, InstanceTree};

use crate::i18n::t;
use crate::theme::{self, icon};

const MOST_BUFFERED: usize = RATE as usize / 2;
const PRIMED: usize = FRAME * 2;
const GATE: f32 = 400.0;
const HANG: Duration = Duration::from_millis(400);
const SPEAKING: Duration = Duration::from_millis(300);

static ON: LazyLock<AtomicBool> =
    LazyLock::new(|| AtomicBool::new(std::fs::read_to_string(file()).is_ok_and(|t| t.trim() == "on")));

fn file() -> std::path::PathBuf {
    crate::server::data_dir().join("voice")
}

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

pub fn toggle(ui: &mut egui::Ui) {
    let mut on = on();
    if ui.checkbox(&mut on, t("Voice chat")).changed() {
        ON.store(on, Ordering::Relaxed);
        let _ = std::fs::create_dir_all(crate::server::data_dir());
        let _ = std::fs::write(file(), if on { "on" } else { "off" });
        #[cfg(target_os = "android")]
        if on {
            ask_for_mic();
        }
    }
}

#[derive(Default)]
struct Mix {
    voices: HashMap<InstanceId, (VecDeque<i16>, bool)>,
}

impl Mix {
    fn next(&mut self) -> f32 {
        let mut sum = 0.0;
        for (queue, playing) in self.voices.values_mut() {
            if !*playing && queue.len() >= PRIMED {
                *playing = true;
            }
            if *playing {
                match queue.pop_front() {
                    Some(s) => sum += f32::from(s) / 32768.0,
                    None => *playing = false,
                }
            }
        }
        sum.clamp(-1.0, 1.0)
    }
}

#[derive(Default)]
struct Shared {
    mix: Mutex<Mix>,
    mic: Mutex<VecDeque<i16>>,
    talking: AtomicBool,
    no_mic: AtomicBool,
    stop: AtomicBool,
}

#[derive(Default)]
pub struct Voice {
    mic_on: bool,
    muted: HashSet<InstanceId>,
    speaking: HashMap<InstanceId, Instant>,
    shared: Option<Arc<Shared>>,
    allowed: Arc<Mutex<Option<bool>>>,
    asked: bool,
    open_until: Option<Instant>,
}

impl Drop for Voice {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Voice {
    fn stop(&mut self) {
        if let Some(shared) = self.shared.take() {
            shared.stop.store(true, Ordering::Relaxed);
        }
    }

    fn allowed(&self) -> bool {
        *self.allowed.lock().unwrap() == Some(true)
    }

    pub fn tick(&mut self, session: Option<&mut PlaySession>, client: impl FnOnce() -> HubClient) {
        let Some(session) = session.filter(|s| s.online()) else {
            if self.asked {
                *self = Self::default();
            }
            return;
        };
        if !on() {
            session.take_heard();
            self.stop();
            self.mic_on = false;
            return;
        }
        if !self.asked {
            self.asked = true;
            let (out, client) = (self.allowed.clone(), client());
            std::thread::spawn(move || {
                let rule = crate::identity().and_then(|me| client.chat_rule(&me.player_id()));
                *out.lock().unwrap() = Some(rule.is_ok_and(|r| r.chat == Chat::Standard));
            });
        }
        let heard = session.take_heard();
        if !self.allowed() {
            return;
        }
        let shared = self
            .shared
            .get_or_insert_with(|| {
                let shared = Arc::new(Shared::default());
                let run = shared.clone();
                std::thread::spawn(move || audio(run));
                shared
            })
            .clone();
        shared.talking.store(self.mic_on, Ordering::Relaxed);
        let now = Instant::now();
        if !heard.is_empty() {
            let mut mix = shared.mix.lock().unwrap();
            for (from, samples) in heard {
                self.speaking.insert(from, now);
                if self.muted.contains(&from) {
                    continue;
                }
                let queue = &mut mix.voices.entry(from).or_default().0;
                queue.extend(samples);
                let over = queue.len().saturating_sub(MOST_BUFFERED);
                queue.drain(..over);
            }
        }
        let mut mic = shared.mic.lock().unwrap();
        if !self.mic_on {
            mic.clear();
            return;
        }
        while mic.len() >= FRAME {
            let frame: Vec<i16> = mic.drain(..FRAME).collect();
            let loud = (frame.iter().map(|s| f32::from(*s).powi(2)).sum::<f32>() / FRAME as f32).sqrt();
            if loud > GATE {
                self.open_until = Some(now + HANG);
            }
            if self.open_until.is_some_and(|until| now < until) {
                session.speak(&frame);
            }
        }
    }

    pub fn ui(&mut self, ui: &egui::Ui, tree: &InstanceTree) {
        let Some(shared) = self.shared.clone().filter(|_| self.allowed()) else { return };
        let now = Instant::now();
        self.speaking.retain(|_, at| now.duration_since(*at) < SPEAKING);
        let no_mic = shared.no_mic.load(Ordering::Relaxed);
        let rect = ui.max_rect();
        egui::Area::new("voice".into()).fixed_pos(rect.min + egui::vec2(10.0, 54.0)).show(ui.ctx(), |ui| {
            let (kind, glyph) = match (self.mic_on, no_mic) {
                (true, false) => (theme::Kind::Primary, icon::MICROPHONE),
                _ => (theme::Kind::Plain, icon::MICROPHONE_SLASH),
            };
            if theme::block_button(ui, kind, glyph, "", 34.0).clicked() {
                self.mic_on = !self.mic_on || no_mic;
                shared.no_mic.store(false, Ordering::Relaxed);
                #[cfg(target_os = "android")]
                if self.mic_on {
                    ask_for_mic();
                }
            }
            let mut who: Vec<(InstanceId, &str)> =
                self.speaking.keys().map(|id| (*id, tree.name(*id).unwrap_or("?"))).collect();
            who.sort_by_key(|(_, name)| *name);
            for (id, name) in who {
                let muted = self.muted.contains(&id);
                let glyph = if muted { icon::SPEAKER_SLASH } else { icon::SPEAKER_HIGH };
                if ui.button(format!("{glyph} {name}")).clicked() && !self.muted.remove(&id) {
                    self.muted.insert(id);
                    shared.mix.lock().unwrap().voices.remove(&id);
                }
            }
        });
        if !self.speaking.is_empty() {
            ui.ctx().request_repaint_after(SPEAKING);
        }
    }
}

#[cfg(target_os = "android")]
#[allow(unsafe_code)]
fn ask_for_mic() {
    use jni::JavaVM;
    use jni::objects::{JObject, JValue};
    let Some(app) = bevy::android::ANDROID_APP.get() else { return };
    let Ok(vm) = (unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }) else { return };
    let Ok(mut env) = vm.attach_current_thread() else { return };
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let asked = (|| -> jni::errors::Result<()> {
        let name = env.new_string("android.permission.RECORD_AUDIO")?;
        let list = env.new_object_array(1, "java/lang/String", &name)?;
        env.call_method(
            &activity,
            "requestPermissions",
            "([Ljava/lang/String;I)V",
            &[JValue::Object(&list), JValue::Int(1)],
        )?;
        Ok(())
    })();
    if asked.is_err() && env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}

fn audio(shared: Arc<Shared>) {
    let host = cpal::default_host();
    let _out = host.default_output_device().and_then(|d| play(&d, shared.clone()));
    let mut input = None;
    while !shared.stop.load(Ordering::Relaxed) {
        let want = shared.talking.load(Ordering::Relaxed);
        if !want {
            input = None;
        } else if input.is_none() && !shared.no_mic.load(Ordering::Relaxed) {
            input = host.default_input_device().and_then(|d| listen(&d, shared.clone()));
            if input.is_none() {
                shared.no_mic.store(true, Ordering::Relaxed);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn listen(device: &cpal::Device, shared: Arc<Shared>) -> Option<cpal::Stream> {
    let supported = device.default_input_config().ok()?;
    let config = supported.config();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => capture::<f32>(device, &config, shared),
        SampleFormat::I16 => capture::<i16>(device, &config, shared),
        SampleFormat::U16 => capture::<u16>(device, &config, shared),
        SampleFormat::I32 => capture::<i32>(device, &config, shared),
        _ => return None,
    }?;
    stream.play().ok()?;
    Some(stream)
}

fn capture<T>(device: &cpal::Device, config: &cpal::StreamConfig, shared: Arc<Shared>) -> Option<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels.max(1));
    let step = RATE as f32 / config.sample_rate.max(1) as f32;
    let (mut phase, mut sum, mut count, mut last) = (0.0f32, 0.0f32, 0u32, 0.0f32);
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mut got = Vec::with_capacity(data.len() / channels + 1);
                for frame in data.chunks(channels) {
                    sum += frame.iter().map(|s| f32::from_sample_(*s)).sum::<f32>() / frame.len() as f32;
                    count += 1;
                    phase += step;
                    while phase >= 1.0 {
                        phase -= 1.0;
                        if count > 0 {
                            last = sum / count as f32;
                        }
                        (sum, count) = (0.0, 0);
                        got.push((last * 32767.0).clamp(-32768.0, 32767.0) as i16);
                    }
                }
                let mut mic = shared.mic.lock().unwrap();
                mic.extend(got);
                let over = mic.len().saturating_sub(MOST_BUFFERED);
                mic.drain(..over);
            },
            |_| {},
            None,
        )
        .ok()
}

fn play(device: &cpal::Device, shared: Arc<Shared>) -> Option<cpal::Stream> {
    let supported = device.default_output_config().ok()?;
    let config = supported.config();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => render::<f32>(device, &config, shared),
        SampleFormat::I16 => render::<i16>(device, &config, shared),
        SampleFormat::U16 => render::<u16>(device, &config, shared),
        SampleFormat::I32 => render::<i32>(device, &config, shared),
        _ => return None,
    }?;
    stream.play().ok()?;
    Some(stream)
}

fn render<T>(device: &cpal::Device, config: &cpal::StreamConfig, shared: Arc<Shared>) -> Option<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels.max(1));
    let step = RATE as f32 / config.sample_rate.max(1) as f32;
    let (mut frac, mut prev, mut cur) = (0.0f32, 0.0f32, 0.0f32);
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let mut mix = shared.mix.lock().unwrap();
                for frame in data.chunks_mut(channels) {
                    frac += step;
                    while frac >= 1.0 {
                        frac -= 1.0;
                        prev = cur;
                        cur = mix.next();
                    }
                    frame.fill(T::from_sample_(prev + (cur - prev) * frac));
                }
            },
            |_| {},
            None,
        )
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_voice_waits_for_a_little_before_playing_and_stops_when_done() {
        let mut mix = Mix::default();
        let id = InstanceId::from_bits(1);
        mix.voices.entry(id).or_default().0.extend(std::iter::repeat_n(16384i16, FRAME));
        assert_eq!(mix.next(), 0.0);
        mix.voices.entry(id).or_default().0.extend(std::iter::repeat_n(16384i16, FRAME));
        assert_eq!(mix.next(), 0.5);
        for _ in 1..PRIMED {
            mix.next();
        }
        assert_eq!(mix.next(), 0.0);
        assert!(!mix.voices[&id].1);
    }
}
