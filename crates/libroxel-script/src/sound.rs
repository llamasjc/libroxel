//! Sound methods: `Play`, `Stop`, `Pause` and `Resume`.
//!
//! A sound plays while its `Playing` property is true; `Pause` and `Resume`
//! only change that. `Play` and `Stop` also go back to the beginning, which
//! the host learns from [`take_sound_restarts`].

use libroxel_instance::{ClassKind, InstanceId, Property, PropertyValue};
use mlua::{Lua, MultiValue, Value};

use crate::bindings::{SignalKind, as_instance, fire, host, require_class, take_errors, tree_err};
use crate::names::sound;

fn set_playing(lua: &Lua, id: InstanceId, playing: bool) -> mlua::Result<()> {
    host(lua).tree.borrow_mut().set(id, Property::Playing, PropertyValue::Bool(playing)).map_err(tree_err)
}

/// The registry names of the sound methods, by method name.
const METHODS: [(&str, &str, bool, bool); 4] = [
    (sound::PLAY, "libroxel.sound.play", true, true),
    (sound::STOP, "libroxel.sound.stop", false, true),
    (sound::PAUSE, "libroxel.sound.pause", false, false),
    (sound::RESUME, "libroxel.sound.resume", true, false),
];

/// Makes the sound methods. They're found through [`index`] rather than
/// being methods of every instance, so a child called "Play" still works.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    for (name, key, playing, restart) in METHODS {
        let f = lua.create_function(move |lua, this: Value| {
            let id = as_instance(&this)
                .ok_or_else(|| mlua::Error::runtime(format!("call it with a colon, like sound:{name}()")))?;
            require_class(lua, id, ClassKind::Sound, name)?;
            set_playing(lua, id, playing)?;
            if restart {
                host(lua).world.borrow_mut().sound_restarts.push(id);
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
    take_errors(lua)
}

fn sound_id(lua: &Lua, id: InstanceId) -> String {
    match host(lua).tree.borrow().get(id, Property::SoundId) {
        Ok(PropertyValue::String(s)) => s,
        _ => String::new(),
    }
}
