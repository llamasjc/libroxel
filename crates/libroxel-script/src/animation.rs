//! Animations for scripts: `Animator:LoadAnimation(animation)` (and the
//! older `Humanoid:LoadAnimation`) gives an animation track to `Play`,
//! `Stop` and `AdjustSpeed`. The engine asks [`pose_for`] how each
//! character's limbs should be turned.

use libroxel_instance::animation::{Clip, Pose, clip, mix_pose};
use libroxel_instance::{ClassKind, InstanceId, Property, PropertyValue};
use mlua::{Lua, MetaMethod, MultiValue, UserData, UserDataFields, UserDataMethods, Value};

use crate::bindings::{Signal, SignalKind, as_instance, fire, host, push_instance, require_class};
use crate::names::animation as names;

fn runtime_err(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

/// One loaded animation on one humanoid.
struct Track {
    id: u64,
    humanoid: InstanceId,
    animation: InstanceId,
    clip: &'static Clip,
    looped: bool,
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
}

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
#[derive(Clone, Copy)]
struct LuaTrack(u64);

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
            })
        });
        fields.add_field_method_get(names::NAME, |lua, this| {
            with_track(lua, this.0, |t, _| t.clip.name.to_owned())
        });
        fields.add_field_method_get(names::ANIMATION, |lua, this| {
            let animation = with_track(lua, this.0, |t, _| t.animation)?;
            push_instance(lua, animation)
        });
        fields
            .add_field_method_get(names::STOPPED, |_, this| Ok(Signal(SignalKind::AnimationStopped(this.0))));
        // Fires as it finishes; libroxel's tracks are done once stopped.
        fields.add_field_method_get("Ended", |_, this| Ok(Signal(SignalKind::AnimationStopped(this.0))));
        fields.add_field_method_get("Priority", |lua, this| extra_member(lua, this.0, "Priority"));
        fields.add_field_method_set("Priority", |lua, this, priority: Value| {
            extra(lua, this.0)?.raw_set("Priority", priority)
        });
        fields.add_field_method_get("KeyframeReached", |lua, this| {
            extra_member(lua, this.0, "KeyframeReached")
        });
        fields.add_field_method_get("DidLoop", |lua, this| extra_member(lua, this.0, "DidLoop"));
        fields.add_meta_field(MetaMethod::Type, names::TRACK_TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(
            names::PLAY,
            |lua, this, (fade, weight, speed): (Option<f32>, Option<f32>, Option<f32>)| {
                with_track(lua, this.0, |t, now| {
                    if !t.playing || t.stopping {
                        t.position = 0.0;
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
                })
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
        methods.add_method("GetMarkerReachedSignal", |lua, this, name: String| {
            let key = format!("marker:{name}");
            let t = extra(lua, this.0)?;
            if let Value::Nil = t.raw_get::<Value>(key.as_str())? {
                t.raw_set(key.as_str(), crate::lenient::new_signal(lua)?)?;
            }
            t.raw_get::<Value>(key.as_str())
        });
        methods.add_method("GetTimeOfKeyframe", |_, _, _: Value| Ok(0.0));
        methods.add_method(names::ADJUST_WEIGHT, |lua, this, weight: Option<f32>| {
            with_track(lua, this.0, |t, _| t.weight = weight.unwrap_or(1.0))
        });
    }
}

/// What a track plays when its animation is one libroxel can't fetch,
/// such as an `rbxassetid://` one kept online: nothing, for a second, so
/// scripts that play, wait for and stop it still run.
static SILENT: Clip = Clip { name: "Animation", length: 1.0, looped: false, legs: false, keys: &[] };

const EXTRA_KEY: &str = "libroxel.animation.extra";

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
/// when markers or keyframes are reached (never, for libroxel's clips).
fn extra_member(lua: &Lua, id: u64, key: &str) -> mlua::Result<Value> {
    let t = extra(lua, id)?;
    let v: Value = t.raw_get(key)?;
    if !v.is_nil() {
        return Ok(v);
    }
    let made = match key {
        "Priority" => {
            let e: mlua::Table = lua.globals().get("Enum")?;
            let p: Value = e.get("AnimationPriority")?;
            match p {
                Value::Table(p) => p.get("Core")?,
                Value::UserData(p) => mlua::ObjectLike::get(&p, "Core")?,
                _ => Value::Nil,
            }
        }
        "KeyframeReached" | "DidLoop" => crate::lenient::new_signal(lua)?,
        _ => return Err(runtime_err(format!("{key} is not a valid member of AnimationTrack"))),
    };
    t.raw_set(key, made.clone())?;
    Ok(made)
}

const LOAD_KEY: &str = "libroxel.animation.load";
const PLAYING_KEY: &str = "libroxel.animation.playing";

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

fn is_controller(tree: &libroxel_instance::InstanceTree, id: InstanceId) -> bool {
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
        let id = match host(lua).tree.borrow().get(animation, Property::AnimationId) {
            Ok(PropertyValue::String(s)) => s,
            _ => String::new(),
        };
        let clip = match clip(&id) {
            Some(clip) => clip,
            // A built-in name that isn't one is a mistake worth saying.
            None if id.starts_with(libroxel_instance::animation::BUILT_IN) => {
                return Err(runtime_err(format!(
                    "there is no animation {id:?}; built-in ones are libroxel://wave, cheer, point, nod, shake, jacks and dance"
                )));
            }
            None => &SILENT,
        };
        let host = host(lua);
        let now = host.clock.get();
        let mut tracks = host.animations.borrow_mut();
        tracks.next += 1;
        let id = tracks.next;
        tracks.tracks.push(Track {
            id,
            humanoid,
            animation,
            clip,
            looped: clip.looped,
            speed: 1.0,
            weight: 1.0,
            position: 0.0,
            at: now,
            playing: false,
            fade: 0.0,
            fade_from: now,
            stopping: false,
        });
        Ok(LuaTrack(id))
    })?;
    lua.set_named_registry_value(LOAD_KEY, load)?;
    let playing = lua.create_function(|lua, this: Value| {
        let this = as_instance(&this).ok_or_else(|| runtime_err("call it with a colon"))?;
        let humanoid = humanoid_of(lua, this)?;
        let ids: Vec<u64> = host(lua)
            .animations
            .borrow()
            .tracks
            .iter()
            .filter(|t| t.humanoid == humanoid && t.playing && !t.stopping)
            .map(|t| t.id)
            .collect();
        Ok(ids.into_iter().map(LuaTrack).collect::<Vec<_>>())
    })?;
    lua.set_named_registry_value(PLAYING_KEY, playing)?;
    Ok(())
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
/// whose humanoid is gone.
pub(crate) fn step(lua: &Lua) {
    let ended: Vec<u64> = {
        let host = host(lua);
        let now = host.clock.get();
        let tree = host.tree.borrow();
        let mut tracks = host.animations.borrow_mut();
        tracks.tracks.retain(|t| tree.exists(t.humanoid) && !tree.is_destroyed(t.humanoid).unwrap_or(true));
        let mut ended = Vec::new();
        for t in &mut tracks.tracks {
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
    for id in ended {
        fire(lua, SignalKind::AnimationStopped(id), || Ok(MultiValue::new()));
    }
}

/// How the animations playing on `humanoid` turn its limbs now, and
/// whether they move the legs. `None` while none are playing.
pub(crate) fn pose_for(lua: &Lua, humanoid: InstanceId) -> Option<(Pose, bool)> {
    let host = host(lua);
    let now = host.clock.get();
    let tracks = host.animations.borrow();
    let mut pose: Option<(Pose, bool)> = None;
    for t in tracks.tracks.iter().filter(|t| t.humanoid == humanoid) {
        let strength = t.strength(now);
        if strength <= 0.0 || t.clip.keys.is_empty() {
            continue;
        }
        let sampled = t.clip.sample(if t.looped { t.time(now) } else { t.time(now).min(t.clip.length) });
        let (base, legs) = pose.unwrap_or_default();
        pose = Some((mix_pose(base, sampled, strength), legs || t.clip.legs));
    }
    pose
}
