//! Sound methods: `Play`, `Stop`, `Pause` and `Resume`.
//!
//! A sound plays while its `Playing` property is true; `Pause` and `Resume`
//! only change that. `Play` and `Stop` also go back to the beginning, which
//! the host learns from [`take_sound_restarts`].

use mlua::{Lua, MultiValue, Value};
use playlibre_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue};

use crate::bindings::{
    SignalKind, as_instance, fire, host, push_instance, require_class, take_errors, tree_err,
};
use crate::names::sound;

fn set_playing(lua: &Lua, id: InstanceId, playing: bool) -> mlua::Result<()> {
    host(lua).tree.borrow_mut().set(id, Property::Playing, PropertyValue::Bool(playing)).map_err(tree_err)
}

/// The registry names of the sound methods, by method name, with the
/// event each fires.
const METHODS: [(&str, &str, bool, bool, &str); 4] = [
    (sound::PLAY, "libroxel.sound.play", true, true, "Played"),
    (sound::STOP, "libroxel.sound.stop", false, true, "Stopped"),
    (sound::PAUSE, "libroxel.sound.pause", false, false, "Paused"),
    (sound::RESUME, "libroxel.sound.resume", true, false, "Resumed"),
];

fn is_playing(lua: &Lua, id: InstanceId) -> bool {
    host(lua).tree.borrow().get_bool(id, Property::Playing) == Some(true)
}

/// Makes the sound methods. They're found through [`index`] rather than
/// being methods of every instance, so a child called "Play" still works.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    for (name, key, playing, restart, event) in METHODS {
        let f = lua.create_function(move |lua, this: Value| {
            let id = as_instance(&this)
                .ok_or_else(|| mlua::Error::runtime(format!("call it with a colon, like sound:{name}()")))?;
            require_class(lua, id, ClassKind::Sound, name)?;
            let was = is_playing(lua, id);
            set_playing(lua, id, playing)?;
            if restart {
                host(lua).world.borrow_mut().sound_restarts.push(id);
                // Stop (and Play) go back to the start.
                let _ = push_instance(lua, id).and_then(|s| mlua::ObjectLike::set(&s, "TimePosition", 0.0));
            }
            // Pausing a sound that isn't playing, or resuming one that is,
            // changes nothing and fires nothing.
            if restart || was != playing {
                let args = MultiValue::from_iter([Value::String(lua.create_string(sound_id(lua, id))?)]);
                crate::lenient::fire_event(lua, id, event, args)?;
            }
            Ok(())
        })?;
        lua.set_named_registry_value(key, f)?;
    }
    Ok(())
}

/// A sound method called `key`, if `class` is a sound.
pub(crate) fn index(lua: &Lua, class: ClassKind, key: &str) -> mlua::Result<Option<Value>> {
    if class != ClassKind::Sound {
        return Ok(None);
    }
    match METHODS.iter().find(|(name, ..)| *name == key) {
        Some((_, registry, ..)) => Ok(Some(lua.named_registry_value(registry)?)),
        None => Ok(None),
    }
}

/// Takes the sounds that `Play` or `Stop` sent back to the beginning.
pub(crate) fn take_sound_restarts(lua: &Lua) -> Vec<InstanceId> {
    std::mem::take(&mut host(lua).world.borrow_mut().sound_restarts)
}

/// A sound played to its end: it stops playing and fires `Ended`.
pub(crate) fn sound_ended(lua: &Lua, id: InstanceId) -> Vec<crate::ScriptError> {
    // The sound may be gone already; then nothing is listening either.
    let _ = set_playing(lua, id, false);
    fire(lua, SignalKind::SoundEnded(id), || {
        let mut args = MultiValue::new();
        args.push_back(Value::String(lua.create_string(sound_id(lua, id))?));
        Ok(args)
    });
    // A copy playing for a destroyed sound goes once it's done.
    {
        let host = host(lua);
        let mut tree = host.tree.borrow_mut();
        if marked(&tree, id) {
            let holder = tree.parent(id).ok().flatten().filter(|p| marked(&tree, *p)).unwrap_or(id);
            let _ = tree.destroy(holder);
        }
    }
    take_errors(lua)
}

fn marked(tree: &InstanceTree, id: InstanceId) -> bool {
    matches!(tree.get_extra(id, sound::PLAYED_ON_REMOVE), Ok(Some(PropertyValue::Bool(true))))
}

/// The longest a copy playing for a destroyed sound stays, in seconds, in
/// case its file never plays to the end (or can't be found).
const PLAYED_ON_REMOVE_FOR: f64 = 60.0;

/// Call before destroying `id`: each sound in it (or it) that is set to
/// `PlayOnRemove` and could be heard plays on as a copy where it was,
/// held by a hidden part when it was in one, and goes once it's done.
pub(crate) fn play_on_remove(lua: &Lua, id: InstanceId) {
    let host = host(lua);
    let mut tree = host.tree.borrow_mut();
    let workspace = tree.workspace();
    let heard = |tree: &InstanceTree, id| {
        tree.is_descendant_of(id, workspace).unwrap_or(false)
            || tree.service(ClassKind::Players).is_some_and(|p| tree.is_descendant_of(id, p).unwrap_or(false))
    };
    if !heard(&tree, id) {
        return;
    }
    let mut sounds = tree.descendants_of_class(id, ClassKind::Sound);
    if tree.class(id) == Ok(ClassKind::Sound) && !sounds.contains(&id) {
        sounds.push(id);
    }
    let set = |v: bool| PropertyValue::Bool(v);
    let now = host.clock.get();
    for s in sounds {
        if !matches!(tree.get_extra(s, sound::PLAY_ON_REMOVE), Ok(Some(PropertyValue::Bool(true)))) {
            continue;
        }
        let at = tree
            .parent(s)
            .ok()
            .flatten()
            .filter(|p| tree.class(*p).is_ok_and(|c| c.is_physical()))
            .and_then(|p| tree.get_vec3(p, Property::Position));
        let Ok(copy) = tree.clone_subtree(s) else { continue };
        let _ = tree.set_extra(copy, sound::PLAY_ON_REMOVE, None);
        let _ = tree.set_extra(copy, sound::PLAYED_ON_REMOVE, Some(set(true)));
        let _ = tree.set(copy, Property::Playing, set(true));
        // The holder part is only made when the sound has a place to play from.
        let made = at.and_then(|_| tree.create(ClassKind::Part, sound::PLAY_ON_REMOVE).ok());
        let holder = match made.zip(at) {
            Some((holder, at)) => {
                let _ = tree.set(holder, Property::Position, PropertyValue::Vec3(at));
                let _ = tree.set(
                    holder,
                    Property::Size,
                    PropertyValue::Vec3(playlibre_instance::Vec3::new(0.2, 0.2, 0.2)),
                );
                let _ = tree.set(holder, Property::Transparency, PropertyValue::Number(1.0));
                for p in [Property::CanCollide, Property::CanQuery, Property::CanTouch] {
                    let _ = tree.set(holder, p, set(false));
                }
                let _ = tree.set(holder, Property::Anchored, set(true));
                let _ = tree.set_extra(holder, sound::PLAYED_ON_REMOVE, Some(set(true)));
                let _ = tree.set_parent(holder, Some(workspace));
                let _ = tree.set_parent(copy, Some(holder));
                holder
            }
            None => {
                let _ = tree.set_parent(copy, Some(workspace));
                copy
            }
        };
        host.debris.borrow_mut().push((holder, now + PLAYED_ON_REMOVE_FOR));
        host.world.borrow_mut().sound_restarts.push(copy);
    }
}

fn sound_id(lua: &Lua, id: InstanceId) -> String {
    match host(lua).tree.borrow().get(id, Property::SoundId) {
        Ok(PropertyValue::String(s)) => s,
        _ => String::new(),
    }
}
