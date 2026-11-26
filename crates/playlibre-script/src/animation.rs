//! Animations for scripts: `Animator:LoadAnimation(animation)` (and the
//! older `Humanoid:LoadAnimation`) gives an animation track to `Play`,
//! `Stop` and `AdjustSpeed`. The engine asks [`pose_for`] how each
//! character's limbs should be turned.
//!
//! A track plays a built-in clip (`playlibre://wave`), a keyframe sequence
//! a script registered (or the engine supplied for an asset id), or, for
//! an asset playlibre can't fetch, the built-in clip that the character's
//! animate script means by the animation's name (its wave, jump, climb and
//! so on), else nothing for a second.

use std::rc::Rc;

use mlua::{Lua, MetaMethod, MultiValue, UserData, UserDataFields, UserDataMethods, Value};
use playlibre_instance::animation::{Clip, Pose, clip};
use playlibre_instance::{ClassKind, InstanceId, Property, PropertyValue};

use crate::bindings::{Signal, SignalKind, as_instance, fire, host, push_instance, require_class};
use crate::keyframes::{CORE, Motion, enum_number, from_sequence, id_key, mix_moving};
use crate::names::animation as names;

fn runtime_err(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

/// One loaded animation on one humanoid.
struct Track {
    id: u64,
    humanoid: InstanceId,
    animation: InstanceId,
    clip: Rc<Motion>,
    looped: bool,
    /// Its `AnimationPriority` number: higher ones play over lower ones.
    priority: u32,
    /// How far in it was when its keyframes and markers were last looked
    /// at, unwrapped (a looped track counts on past its length).
    seen: f32,
    speed: f32,
    weight: f32,
    /// How far in it is, in seconds, as of `at`.
    position: f32,
    /// The game clock when `position` was last set.
    at: f64,
    playing: bool,
    /// How long fading in (when playing) or out (when stopping) takes.
    fade: f32,
    /// The clock when it started fading in or out.
    fade_from: f64,
    /// While stopping: it fades out and then is no longer drawn.
    stopping: bool,
}

impl Track {
    fn time(&self, now: f64) -> f32 {
        if self.playing { self.position + (now - self.at) as f32 * self.speed } else { self.position }
    }

    /// How much of it shows now, 0 to 1.
    fn strength(&self, now: f64) -> f32 {
        let faded =
            if self.fade > 0.0 { ((now - self.fade_from) as f32 / self.fade).clamp(0.0, 1.0) } else { 1.0 };
        let shown = if self.stopping {
            1.0 - faded
        } else if self.playing {
            faded
        } else {
            0.0
        };
        shown * self.weight.clamp(0.0, 1.0)
    }
}

/// Every loaded track.
#[derive(Default)]
pub(crate) struct Tracks {
    tracks: Vec<Track>,
    next: u64,
    /// How many script objects stand for each track.
    held: Held,
    /// Keyframe sequences by the id they were registered under (see
    /// [`id_key`]): the sequence, and the motion it was read as.
    sequences: std::collections::HashMap<String, (Option<InstanceId>, Rc<Motion>)>,
    /// How many sequences scripts have registered.
    temp: u64,
}

/// The order tracks are drawn in: `Core` under everything, then by number.
fn rank(priority: u32) -> i64 {
    if priority == CORE { -1 } else { i64::from(priority) }
}

type Held = std::rc::Rc<std::cell::RefCell<std::collections::HashMap<u64, u32>>>;

fn with_track<R>(lua: &Lua, id: u64, f: impl FnOnce(&mut Track, f64) -> R) -> mlua::Result<R> {
    let host = host(lua);
    let now = host.clock.get();
    let mut tracks = host.animations.borrow_mut();
    let track = tracks
        .tracks
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| runtime_err("that animation track is gone"))?;
    Ok(f(track, now))
}

/// A track, as scripts see it.
struct LuaTrack(u64, Held);

impl LuaTrack {
    fn new(id: u64, held: &Held) -> Self {
        *held.borrow_mut().entry(id).or_default() += 1;
        Self(id, held.clone())
    }
}

impl Drop for LuaTrack {
    fn drop(&mut self) {
        if let Ok(mut held) = self.1.try_borrow_mut()
            && let Some(n) = held.get_mut(&self.0)
        {
            *n -= 1;
            if *n == 0 {
                held.remove(&self.0);
            }
        }
    }
}

impl UserData for LuaTrack {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(names::IS_PLAYING, |lua, this| with_track(lua, this.0, |t, _| t.playing));
        fields.add_field_method_get(names::LENGTH, |lua, this| with_track(lua, this.0, |t, _| t.clip.length));
        fields.add_field_method_get(names::LOOPED, |lua, this| with_track(lua, this.0, |t, _| t.looped));
        fields.add_field_method_set(names::LOOPED, |lua, this, looped: bool| {
            with_track(lua, this.0, |t, _| t.looped = looped)
        });
        fields.add_field_method_get(names::SPEED, |lua, this| with_track(lua, this.0, |t, _| t.speed));
        fields.add_field_method_get(names::WEIGHT, |lua, this| with_track(lua, this.0, |t, _| t.weight));
        fields.add_field_method_get(names::TIME_POSITION, |lua, this| {
            with_track(lua, this.0, |t, now| {
                let time = t.time(now);
                if t.looped { time.rem_euclid(t.clip.length.max(1e-3)) } else { time.min(t.clip.length) }
            })
        });
        fields.add_field_method_set(names::TIME_POSITION, |lua, this, position: f32| {
            with_track(lua, this.0, |t, now| {
                t.position = position.max(0.0);
                t.at = now;
                t.seen = t.position;
            })
        });
        fields.add_field_method_get(names::NAME, |lua, this| {
            with_track(lua, this.0, |t, _| t.clip.name.clone())
        });
        fields.add_field_method_get(names::ANIMATION, |lua, this| {
            let animation = with_track(lua, this.0, |t, _| t.animation)?;
            push_instance(lua, animation)
        });
        fields
            .add_field_method_get(names::STOPPED, |_, this| Ok(Signal(SignalKind::AnimationStopped(this.0))));
        // Fires as it finishes; playlibre's tracks are done once stopped.
        fields.add_field_method_get("Ended", |_, this| Ok(Signal(SignalKind::AnimationStopped(this.0))));
        fields.add_field_method_get(names::PRIORITY, |lua, this| extra_member(lua, this.0, names::PRIORITY));
        fields.add_field_method_set(names::PRIORITY, |lua, this, priority: Value| {
            if let Some(n) = enum_number(&priority) {
                with_track(lua, this.0, |t, _| t.priority = n)?;
            }
            extra(lua, this.0)?.raw_set(names::PRIORITY, priority)
        });
        fields.add_field_method_get(names::KEYFRAME_REACHED, |lua, this| {
            extra_member(lua, this.0, names::KEYFRAME_REACHED)
        });
        fields.add_field_method_get(names::DID_LOOP, |lua, this| extra_member(lua, this.0, names::DID_LOOP));
        fields.add_meta_field(MetaMethod::Type, names::TRACK_TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_function(
            names::PLAY,
            |lua, (ud, fade, weight, speed): (mlua::AnyUserData, Option<f32>, Option<f32>, Option<f32>)| {
                let id = ud.borrow::<LuaTrack>()?.0;
                let (started, humanoid) = with_track(lua, id, |t, now| {
                    let started = !t.playing || t.stopping;
                    if started {
                        t.position = 0.0;
                        // So a keyframe or marker right at the start fires.
                        t.seen = -1e-4;
                    } else {
                        t.position = t.time(now);
                    }
                    t.at = now;
                    t.playing = true;
                    t.stopping = false;
                    t.fade = fade.unwrap_or(0.1).max(0.0);
                    t.fade_from = now;
                    t.weight = weight.unwrap_or(1.0);
                    if let Some(speed) = speed {
                        t.speed = speed;
                    }
                    (started, t.humanoid)
                })?;
                if started {
                    let animators: Vec<InstanceId> = {
                        let host = host(lua);
                        let tree = host.tree.borrow();
                        tree.children(humanoid)
                            .map(|c| {
                                c.iter()
                                    .copied()
                                    .filter(|c| tree.class(*c) == Ok(ClassKind::Animator))
                                    .collect()
                            })
                            .unwrap_or_default()
                    };
                    for owner in std::iter::once(humanoid).chain(animators) {
                        let args = MultiValue::from_iter([Value::UserData(ud.clone())]);
                        crate::lenient::fire_event(lua, owner, "AnimationPlayed", args)?;
                    }
                }
                Ok(())
            },
        );
        methods.add_method(names::STOP, |lua, this, fade: Option<f32>| {
            let was_playing = with_track(lua, this.0, |t, now| {
                let was = t.playing && !t.stopping;
                if was {
                    t.position = t.time(now);
                    t.at = now;
                    t.stopping = true;
                    t.fade = fade.unwrap_or(0.1).max(0.0);
                    t.fade_from = now;
                }
                was
            })?;
            if was_playing {
                fire(lua, SignalKind::AnimationStopped(this.0), || Ok(MultiValue::new()));
            }
            Ok(())
        });
        methods.add_method(names::ADJUST_SPEED, |lua, this, speed: Option<f32>| {
            with_track(lua, this.0, |t, now| {
                t.position = t.time(now);
                t.at = now;
                t.speed = speed.unwrap_or(1.0);
            })
        });
        methods.add_method(names::GET_MARKER_REACHED_SIGNAL, |lua, this, name: String| {
            let key = format!("marker:{name}");
            let t = extra(lua, this.0)?;
            if let Value::Nil = t.raw_get::<Value>(key.as_str())? {
                t.raw_set(key.as_str(), crate::lenient::new_signal(lua)?)?;
            }
            t.raw_get::<Value>(key.as_str())
        });
        methods.add_method(names::GET_TIME_OF_KEYFRAME, |lua, this, name: String| {
            with_track(lua, this.0, |t, _| {
                t.clip.keyframes.iter().find(|(_, n)| *n == name).map_or(0.0, |(time, _)| *time)
            })
        });
        methods.add_method(names::ADJUST_WEIGHT, |lua, this, weight: Option<f32>| {
            with_track(lua, this.0, |t, _| t.weight = weight.unwrap_or(1.0))
        });
    }
}

/// What a track plays when its animation is one playlibre can't fetch,
/// such as an `rbxassetid://` one kept online: nothing, for a second, so
/// scripts that play, wait for and stop it still run.
static SILENT: Clip = Clip { name: "Animation", length: 1.0, looped: false, legs: false, keys: &[] };

const EXTRA_KEY: &str = "playlibre.animation.extra";

/// A track's other members (its priority and events), kept in Lua.
fn extra(lua: &Lua, id: u64) -> mlua::Result<mlua::Table> {
    let all: mlua::Table = match lua.named_registry_value::<Value>(EXTRA_KEY)? {
        Value::Table(t) => t,
        _ => {
            let t = lua.create_table()?;
            lua.set_named_registry_value(EXTRA_KEY, t.clone())?;
            t
        }
    };
    if let Value::Table(t) = all.raw_get::<Value>(id)? {
        return Ok(t);
    }
    let t = lua.create_table()?;
    all.raw_set(id, t.clone())?;
    Ok(t)
}

/// A member of a track kept in Lua: its priority, or an event that fires
/// when markers or keyframes are reached (never, for playlibre's clips).
fn extra_member(lua: &Lua, id: u64, key: &str) -> mlua::Result<Value> {
    let t = extra(lua, id)?;
    let v: Value = t.raw_get(key)?;
    if !v.is_nil() {
        return Ok(v);
    }
    let made = match key {
        names::PRIORITY => {
            let priority = with_track(lua, id, |t, _| t.priority)?;
            let e: mlua::Table = lua.globals().get("Enum")?;
            let p: Value = e.get(names::PRIORITY_ENUM)?;
            match p {
                Value::Table(p) => mlua::ObjectLike::call_method(&p, names::FROM_VALUE, priority)?,
                Value::UserData(p) => mlua::ObjectLike::call_method(&p, names::FROM_VALUE, priority)?,
                _ => Value::Nil,
            }
        }
        names::KEYFRAME_REACHED | names::DID_LOOP => crate::lenient::new_signal(lua)?,
        _ => return Err(runtime_err(format!("{key} is not a valid member of AnimationTrack"))),
    };
    t.raw_set(key, made.clone())?;
    Ok(made)
}

const LOAD_KEY: &str = "playlibre.animation.load";
const PLAYING_KEY: &str = "playlibre.animation.playing";

/// The humanoid an animator or humanoid plays animations for.
fn humanoid_of(lua: &Lua, id: InstanceId) -> mlua::Result<InstanceId> {
    let host = host(lua);
    let tree = host.tree.borrow();
    match tree.class(id) {
        Ok(ClassKind::Humanoid) => Ok(id),
        Ok(ClassKind::Animator)
            if tree.parent(id).ok().flatten().is_some_and(|p| tree.class(p) == Ok(ClassKind::Humanoid)) =>
        {
            Ok(tree.parent(id).ok().flatten().unwrap_or(id))
        }
        // An animation controller animates a model without a humanoid.
        Ok(ClassKind::Other) if is_controller(&tree, id) => Ok(id),
        Ok(ClassKind::Animator) => tree
            .parent(id)
            .ok()
            .flatten()
            .filter(|p| is_controller(&tree, *p))
            .ok_or_else(|| runtime_err("an Animator plays animations when it's inside a Humanoid")),
        _ => Err(runtime_err("LoadAnimation is for an Animator or a Humanoid")),
    }
}

fn is_controller(tree: &playlibre_instance::InstanceTree, id: InstanceId) -> bool {
    matches!(tree.get(id, Property::OtherClass), Ok(PropertyValue::String(c)) if c == "AnimationController")
}

pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let load = lua.create_function(|lua, (this, animation): (Value, Value)| {
        let this = as_instance(&this)
            .ok_or_else(|| runtime_err("call it with a colon, like animator:LoadAnimation(a)"))?;
        let humanoid = humanoid_of(lua, this)?;
        let animation =
            as_instance(&animation).ok_or_else(|| runtime_err("LoadAnimation expects an Animation"))?;
        require_class(lua, animation, ClassKind::Animation, names::LOAD_ANIMATION)?;
        let clip = motion_for(lua, animation)?;
        let host = host(lua);
        let now = host.clock.get();
        let mut tracks = host.animations.borrow_mut();
        tracks.next += 1;
        let id = tracks.next;
        tracks.tracks.push(Track {
            id,
            humanoid,
            animation,
            looped: clip.looped,
            priority: clip.priority,
            seen: 0.0,
            clip,
            speed: 1.0,
            weight: 1.0,
            position: 0.0,
            at: now,
            playing: false,
            fade: 0.0,
            fade_from: now,
            stopping: false,
        });
        Ok(LuaTrack::new(id, &tracks.held))
    })?;
    lua.set_named_registry_value(LOAD_KEY, load)?;
    let playing = lua.create_function(|lua, this: Value| {
        let this = as_instance(&this).ok_or_else(|| runtime_err("call it with a colon"))?;
        let humanoid = humanoid_of(lua, this)?;
        let host = host(lua);
        let tracks = host.animations.borrow();
        let ids: Vec<u64> = tracks
            .tracks
            .iter()
            .filter(|t| t.humanoid == humanoid && t.playing && !t.stopping)
            .map(|t| t.id)
            .collect();
        Ok(ids.into_iter().map(|id| LuaTrack::new(id, &tracks.held)).collect::<Vec<_>>())
    })?;
    lua.set_named_registry_value(PLAYING_KEY, playing)?;
    Ok(())
}

/// The layer's hooks for `KeyframeSequenceProvider`: registering a
/// sequence gives an id an animation can play it by, and that id gives
/// the sequence back.
pub(crate) fn install_layer(lua: &Lua, layer: &mlua::Table) -> mlua::Result<()> {
    layer.set(
        names::REGISTER_SEQUENCE,
        lua.create_function(|lua, sequence: Value| {
            let sequence = as_instance(&sequence)
                .ok_or_else(|| runtime_err("RegisterKeyframeSequence expects a KeyframeSequence"))?;
            let motion = Rc::new(from_sequence(lua, sequence)?);
            let host = host(lua);
            let mut tracks = host.animations.borrow_mut();
            let id = format!("{}{}", names::TEMP_ID, tracks.temp);
            tracks.temp += 1;
            tracks.sequences.insert(id_key(&id), (Some(sequence), motion));
            Ok(id)
        })?,
    )?;
    layer.set(
        names::REGISTERED_SEQUENCE,
        lua.create_function(|lua, id: String| {
            let found = host(lua).animations.borrow().sequences.get(&id_key(&id)).and_then(|s| s.0);
            let Some(sequence) = found else { return Ok(Value::Nil) };
            if host(lua).tree.borrow().is_destroyed(sequence) != Ok(false) {
                return Ok(Value::Nil);
            }
            Ok(Value::UserData(push_instance(lua, sequence)?))
        })?,
    )?;
    Ok(())
}

/// Makes `id` (an asset id such as `rbxassetid://123`, or any other) play
/// the keyframe sequence `sequence`, as it is now.
pub(crate) fn register(lua: &Lua, id: &str, sequence: InstanceId) -> mlua::Result<()> {
    let motion = Rc::new(from_sequence(lua, sequence)?);
    host(lua).animations.borrow_mut().sequences.insert(id_key(id), (Some(sequence), motion));
    Ok(())
}

/// What an `Animation` plays: see the top of this file.
fn motion_for(lua: &Lua, animation: InstanceId) -> mlua::Result<Rc<Motion>> {
    let host = host(lua);
    let (id, own_name, holder_name) = {
        let tree = host.tree.borrow();
        let id = match tree.get(animation, Property::AnimationId) {
            Ok(PropertyValue::String(s)) => s,
            _ => String::new(),
        };
        let name = |i: InstanceId| match tree.get(i, Property::Name) {
            Ok(PropertyValue::String(s)) => s.to_ascii_lowercase(),
            _ => String::new(),
        };
        let holder = tree.parent(animation).ok().flatten().map(name).unwrap_or_default();
        (id, name(animation), holder)
    };
    if let Some(clip) = clip(&id) {
        return Ok(Rc::new(Motion::built_in(clip)));
    }
    // A built-in name that isn't one is a mistake worth saying.
    if playlibre_instance::animation::built_in_name(&id).is_some() {
        return Err(runtime_err(format!(
            "there is no animation {id:?}; built-in ones are playlibre://wave, cheer, point, nod, shake, jacks, dance, jump, fall, climb, hold and laugh"
        )));
    }
    if let Some((_, motion)) = host.animations.borrow().sequences.get(&id_key(&id)) {
        return Ok(motion.clone());
    }
    let by_name = names::BY_NAME
        .iter()
        .find(|(n, _)| *n == own_name || *n == holder_name)
        .and_then(|(_, built_in)| clip(&format!("{}{built_in}", playlibre_instance::animation::BUILT_IN)));
    Ok(Rc::new(Motion::built_in(by_name.unwrap_or(&SILENT))))
}

/// An animator's (or humanoid's) animation methods.
pub(crate) fn index(lua: &Lua, id: InstanceId, class: ClassKind, key: &str) -> mlua::Result<Option<Value>> {
    let animates = match class {
        ClassKind::Animator | ClassKind::Humanoid => true,
        ClassKind::Other => is_controller(&host(lua).tree.borrow(), id),
        _ => false,
    };
    if !animates {
        return Ok(None);
    }
    match key {
        names::LOAD_ANIMATION => Ok(Some(lua.named_registry_value(LOAD_KEY)?)),
        names::GET_PLAYING_ANIMATION_TRACKS => Ok(Some(lua.named_registry_value(PLAYING_KEY)?)),
        _ => Ok(None),
    }
}

/// Ends tracks that ran out, firing their `Stopped`, and forgets tracks
/// whose humanoid is gone or that no script holds and aren't playing.
pub(crate) fn step(lua: &Lua) {
    let mut forgotten = Vec::new();
    let mut reached = Vec::new();
    let ended: Vec<u64> = {
        let host = host(lua);
        let now = host.clock.get();
        let tree = host.tree.borrow();
        let mut tracks = host.animations.borrow_mut();
        let held = tracks.held.clone();
        let held = held.borrow();
        tracks.tracks.retain(|t| {
            let keep = tree.is_destroyed(t.humanoid) == Ok(false)
                && (t.playing || t.stopping || held.contains_key(&t.id));
            if !keep {
                forgotten.push(t.id);
            }
            keep
        });
        let mut ended = Vec::new();
        for t in &mut tracks.tracks {
            if t.playing && !t.stopping {
                reached_since(t, now, &mut reached);
            }
            if t.playing && !t.stopping && !t.looped && t.time(now) >= t.clip.length {
                t.position = t.clip.length;
                t.at = now;
                t.stopping = true;
                t.fade = 0.0;
                t.fade_from = now;
                ended.push(t.id);
            }
            // Fully faded out: stopped.
            if t.stopping && t.strength(now) <= 0.0 {
                t.playing = false;
                t.stopping = false;
            }
        }
        ended
    };
    for (id, what) in reached {
        let Ok(t) = extra(lua, id) else { continue };
        let (key, arg) = match what {
            Reached::Keyframe(name) => (names::KEYFRAME_REACHED.to_owned(), Some(name)),
            Reached::Marker(name, value) => (format!("marker:{name}"), Some(value)),
            Reached::Loop => (names::DID_LOOP.to_owned(), None),
        };
        // Only events a script asked for were made, so only they fire.
        if let Ok(Value::Table(signal)) = t.raw_get::<Value>(key.as_str()) {
            let _ = mlua::ObjectLike::call_method::<()>(&signal, names::FIRE, arg);
        }
    }
    for id in ended {
        fire(lua, SignalKind::AnimationStopped(id), || Ok(MultiValue::new()));
    }
    if !forgotten.is_empty() {
        if let Ok(Value::Table(all)) = lua.named_registry_value::<Value>(EXTRA_KEY) {
            for id in &forgotten {
                let _ = all.raw_set(*id, Value::Nil);
            }
        }
        let forgotten: std::collections::HashSet<u64> = forgotten.into_iter().collect();
        crate::bindings::disconnect_where(
            lua,
            |s| matches!(s, SignalKind::AnimationStopped(id) if forgotten.contains(&id)),
        );
    }
}

/// Something a track passed while playing.
enum Reached {
    Keyframe(String),
    Marker(String, String),
    Loop,
}

/// What `t` passed since it was last looked at, up to now.
fn reached_since(t: &mut Track, now: f64, out: &mut Vec<(u64, Reached)>) {
    let clip = t.clip.clone();
    let length = clip.length;
    let mut to = t.time(now);
    if !t.looped {
        to = to.min(length);
    }
    let from = t.seen;
    t.seen = to;
    if to <= from || (clip.keyframes.is_empty() && clip.markers.is_empty() && !t.looped) {
        return;
    }
    // Each time round a looped track, and at most a few, for a long frame.
    let rounds: Vec<f32> = if t.looped && length > 1e-3 {
        let first = (from / length).floor().max(0.0) as i64;
        let last = (to / length).floor() as i64;
        (first..=last.min(first + 4)).map(|r| r as f32 * length).collect()
    } else {
        vec![0.0]
    };
    for start in rounds {
        if start > 0.0 && start > from && start <= to {
            out.push((t.id, Reached::Loop));
        }
        let passed = |time: f32| {
            let at = start + time;
            at > from && at <= to
        };
        let mut found: Vec<(f32, Reached)> = Vec::new();
        for (time, name) in &clip.keyframes {
            if passed(*time) {
                found.push((*time, Reached::Keyframe(name.clone())));
            }
        }
        for (time, name, value) in &clip.markers {
            if passed(*time) {
                found.push((*time, Reached::Marker(name.clone(), value.clone())));
            }
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.extend(found.into_iter().map(|(_, r)| (t.id, r)));
    }
}

/// How the animations playing on `humanoid` turn its limbs now, and
/// whether they move the legs. `None` while none are playing.
pub(crate) fn pose_for(lua: &Lua, humanoid: InstanceId) -> Option<(Pose, bool)> {
    let host = host(lua);
    let now = host.clock.get();
    let tracks = host.animations.borrow();
    let mut pose: Option<(Pose, bool)> = None;
    let mut playing: Vec<&Track> = tracks.tracks.iter().filter(|t| t.humanoid == humanoid).collect();
    // Lower priorities first, so higher ones play over them where they move.
    playing.sort_by_key(|t| rank(t.priority));
    for t in playing {
        let strength = t.strength(now);
        if strength <= 0.0 || t.clip.is_still() {
            continue;
        }
        let sampled = t.clip.sample(if t.looped { t.time(now) } else { t.time(now).min(t.clip.length) });
        let (base, legs) = pose.unwrap_or_default();
        pose = Some((mix_moving(base, sampled, strength, &t.clip.moves), legs || t.clip.legs()));
    }
    pose
}
