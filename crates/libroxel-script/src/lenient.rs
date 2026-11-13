//! The bridge to the lenient layer (`lenient.luau`): members that scripts
//! written for other engines use and that libroxel answers with sensible
//! stand-ins. The bindings ask it only after every real member, property
//! and child has said no.

use libroxel_instance::{ClassKind, InstanceId};
use mlua::{AnyUserData, Function, Lua, Table, Value};

use crate::names;

const LAYER_KEY: &str = "libroxel.lenient";
const GROUPS_KEY: &str = "libroxel.lenient.groups";

/// Families a class can belong to, most specific first.
const FAMILIES: &[&str] = &[
    "BasePart",
    "GuiButton",
    "GuiObject",
    "LayerCollector",
    "ValueBase",
    "Light",
    "PostEffect",
    "Constraint",
    "JointInstance",
    "BodyMover",
];

/// Loads the layer: adds the enumerations libroxel lacks to `enum_lib`
/// (which must still be writable) and the extra globals.
pub(crate) fn install(lua: &Lua, enum_lib: &Table) -> mlua::Result<()> {
    let layer: Table = lua
        .load(include_str!("lenient.luau"))
        .set_name("=libroxel.lenient")
        .call((enum_lib.clone(), lua.globals()))?;
    layer.get::<Function>("add_enums")?.call::<()>(enum_lib.clone())?;
    layer.get::<Function>("globals")?.call::<()>(())?;
    lua.set_named_registry_value(LAYER_KEY, layer)?;
    lua.set_named_registry_value(GROUPS_KEY, lua.create_table()?)
}

/// Once the globals are frozen: `shared` (and `_G`) stay writable, since
/// scripts use them to share values.
pub(crate) fn after_sandbox(lua: &Lua) -> mlua::Result<()> {
    if let Value::Table(shared) = lua.globals().get::<Value>("shared")? {
        shared.set_readonly(false);
    }
    Ok(())
}

fn layer(lua: &Lua) -> mlua::Result<Table> {
    lua.named_registry_value(LAYER_KEY)
}

/// The class's name, the families it belongs to, and then "Instance".
fn groups(lua: &Lua, class: ClassKind) -> mlua::Result<Table> {
    let cache: Table = lua.named_registry_value(GROUPS_KEY)?;
    let name = names::class_name(class);
    if let Value::Table(t) = cache.get::<Value>(name)? {
        return Ok(t);
    }
    let mut list = Vec::new();
    let mut current = Some(class);
    while let Some(c) = current {
        list.push(names::class_name(c));
        current = c.superclass();
    }
    for family in FAMILIES {
        let belongs = names::is_a_family(class, family)
            .unwrap_or_else(|| names::class_from_name(family).is_some_and(|f| class.is_a(f)));
        if belongs && !list.contains(family) {
            list.push(family);
        }
    }
    list.push("Instance");
    let t = lua.create_sequence_from(list)?;
    cache.set(name, t.clone())?;
    Ok(t)
}

fn instance_key(id: InstanceId) -> String {
    id.to_bits().to_string()
}

fn lookup(lua: &Lua, key_id: String, obj: Value, key: &str, groups: Table) -> mlua::Result<Option<Value>> {
    let index: Function = layer(lua)?.get("index")?;
    let (found, value): (bool, Value) = index.call((key_id, obj, key, groups))?;
    Ok(found.then_some(value))
}

fn store(
    lua: &Lua,
    key_id: String,
    obj: Value,
    key: &str,
    value: Value,
    groups: Table,
) -> mlua::Result<bool> {
    let new_index: Function = layer(lua)?.get("new_index")?;
    new_index.call((key_id, obj, key, value, groups))
}

/// The lenient value of `key` on an instance, if the layer has one.
pub(crate) fn index(
    lua: &Lua,
    id: InstanceId,
    class: ClassKind,
    obj: AnyUserData,
    key: &str,
) -> mlua::Result<Option<Value>> {
    lookup(lua, instance_key(id), Value::UserData(obj), key, groups(lua, class)?)
}

/// Sets `key` on an instance through the layer; false when it doesn't know `key`.
pub(crate) fn new_index(
    lua: &Lua,
    id: InstanceId,
    class: ClassKind,
    obj: AnyUserData,
    key: &str,
    value: Value,
) -> mlua::Result<bool> {
    store(lua, instance_key(id), Value::UserData(obj), key, value, groups(lua, class)?)
}

/// The lenient value of `key` on a service object (one that isn't an
/// instance), or a "not a valid member" error.
pub(crate) fn service_index(lua: &Lua, service: &str, obj: AnyUserData, key: &str) -> mlua::Result<Value> {
    let groups = lua.create_sequence_from([service])?;
    match lookup(lua, format!("service:{service}"), Value::UserData(obj), key, groups)? {
        Some(value) => Ok(value),
        None => Err(mlua::Error::runtime(format!("{key} is not a valid member of {service}"))),
    }
}

/// Sets `key` on a service object through the layer.
pub(crate) fn service_new_index(
    lua: &Lua,
    service: &str,
    obj: AnyUserData,
    key: &str,
    value: Value,
) -> mlua::Result<()> {
    let groups = lua.create_sequence_from([service])?;
    if store(lua, format!("service:{service}"), Value::UserData(obj), key, value, groups)? {
        Ok(())
    } else {
        Err(mlua::Error::runtime(format!("{key} is not a valid member of {service}")))
    }
}

/// Gives a service object's type the lenient layer's members for `service`.
pub(crate) fn add_service_members<T: mlua::UserData, M: mlua::UserDataMethods<T>>(
    methods: &mut M,
    service: &'static str,
) {
    methods.add_meta_function(mlua::MetaMethod::Index, move |lua, (obj, key): (AnyUserData, String)| {
        service_index(lua, service, obj, &key)
    });
    methods.add_meta_function(
        mlua::MetaMethod::NewIndex,
        move |lua, (obj, key, value): (AnyUserData, String, Value)| {
            service_new_index(lua, service, obj, &key, value)
        },
    );
}

/// A service libroxel only stands in for, if `name` is one.
pub(crate) fn service(lua: &Lua, name: &str) -> mlua::Result<Option<Value>> {
    let make: Function = layer(lua)?.get("service")?;
    let made: Value = make.call(name)?;
    Ok(if made.is_nil() { None } else { Some(made) })
}
