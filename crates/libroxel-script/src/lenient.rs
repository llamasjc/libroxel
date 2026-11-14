//! The bridge to the lenient layer (`lenient.luau`): members that scripts
//! written for other engines use and that libroxel answers with sensible
//! stand-ins. The bindings ask it only after every real member, property
//! and child has said no.

use libroxel_instance::{ClassKind, InstanceId, Property, PropertyValue};
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
    layer.get::<Function>("add_enums")?.call::<()>((enum_lib.clone(), crate::reflection::enums(lua)?))?;
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
/// `other` is the class an [`ClassKind::Other`] instance stands for.
fn groups(lua: &Lua, class: ClassKind, other: Option<&str>) -> mlua::Result<Table> {
    let cache: Table = lua.named_registry_value(GROUPS_KEY)?;
    let name = other.unwrap_or(names::class_name(class));
    if let Value::Table(t) = cache.get::<Value>(name)? {
        return Ok(t);
    }
    if let Some(other) = other
        && class == ClassKind::Other
    {
        // The class and everything it comes from, by the class database.
        let mut list = crate::reflection::lineage(other);
        if list.is_empty() {
            list.push("Instance");
        }
        let t = lua.create_sequence_from(list)?;
        cache.set(name, t.clone())?;
        return Ok(t);
    }
    // A stand-in has its own class's members first.
    let mut list: Vec<&str> = other.into_iter().collect();
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
    known: bool,
) -> mlua::Result<bool> {
    let new_index: Function = layer(lua)?.get("new_index")?;
    new_index.call((key_id, obj, key, value, groups, known))
}

/// The Roblox-style class name scripts see for an instance.
pub(crate) fn roblox_class(lua: &Lua, id: InstanceId, class: ClassKind) -> String {
    let host = crate::bindings::host(lua);
    let tree = host.tree.borrow();
    if class == ClassKind::Other
        && let Ok(PropertyValue::String(name)) = tree.get(id, Property::OtherClass)
    {
        return name;
    }
    stand_in_class(&tree, id).unwrap_or_else(|| names::class_name(class).to_owned())
}

/// The class a stand-in (such as a ViewportFrame made as a Frame) was
/// made as, kept under [`STAND_IN`].
pub(crate) fn stand_in_class(tree: &libroxel_instance::InstanceTree, id: InstanceId) -> Option<String> {
    match tree.get_extra(id, names::STAND_IN) {
        Ok(Some(PropertyValue::String(name))) => Some(name),
        _ => None,
    }
}

/// The lenient value of `key` on an instance, if the layer has one: a
/// value kept from a file, one a script set, the layer's own stand-in, or
/// the class's usual starting value.
pub(crate) fn index(
    lua: &Lua,
    id: InstanceId,
    class: ClassKind,
    obj: AnyUserData,
    key: &str,
) -> mlua::Result<Option<Value>> {
    let roblox = roblox_class(lua, id, class);
    let kept = crate::bindings::host(lua).tree.borrow().get_extra(id, key).ok().flatten();
    if let Some(kept) = kept {
        return crate::reflection::extra_to_lua(lua, &roblox, key, kept).map(Some);
    }
    let other = (roblox != names::class_name(class)).then_some(roblox.as_str());
    if let Some(found) = lookup(lua, instance_key(id), Value::UserData(obj), key, groups(lua, class, other)?)?
    {
        return Ok(Some(found));
    }
    crate::reflection::default_value(lua, &roblox, key)
}

/// Sets `key` on an instance through the layer; false when neither it nor
/// the class database knows `key`.
pub(crate) fn new_index(
    lua: &Lua,
    id: InstanceId,
    class: ClassKind,
    obj: AnyUserData,
    key: &str,
    value: Value,
) -> mlua::Result<bool> {
    let roblox = roblox_class(lua, id, class);
    let other = (roblox != names::class_name(class)).then_some(roblox.as_str());
    let groups = groups(lua, class, other)?;
    let host = crate::bindings::host(lua);
    let kept = host.tree.borrow().get_extra(id, key).ok().flatten().is_some();
    let in_database = crate::reflection::has_property(&roblox, key);
    if kept || in_database {
        // Plain values are kept on the instance, so saving keeps them.
        if let Some(plain) = crate::reflection::extra_from_lua(&roblox, key, &value) {
            if store(lua, instance_key(id), Value::UserData(obj), key, value, groups, false)? {
                // The layer has its own handling: it owns the value.
                let _ = host.tree.borrow_mut().set_extra(id, key, None);
            } else {
                let _ = host.tree.borrow_mut().set_extra(id, key, plain);
            }
            return Ok(true);
        }
        let _ = host.tree.borrow_mut().set_extra(id, key, None);
    }
    store(lua, instance_key(id), Value::UserData(obj), key, value, groups, in_database)
}

/// The lenient value of `key` on a service object (one that isn't an
/// instance), or a "not a valid member" error.
pub(crate) fn service_index(lua: &Lua, service: &str, obj: AnyUserData, key: &str) -> mlua::Result<Value> {
    let groups = lua.create_sequence_from([service])?;
    match lookup(lua, format!("service:{service}"), Value::UserData(obj), key, groups)? {
        Some(value) => Ok(value),
        None if crate::reflection::has_property(service, key) => {
            Ok(crate::reflection::default_value(lua, service, key)?.unwrap_or(Value::Nil))
        }
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
    let known = crate::reflection::has_property(service, key);
    if store(lua, format!("service:{service}"), Value::UserData(obj), key, value, groups, known)? {
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

/// A new event made in Lua, which fires when a script fires it.
pub(crate) fn new_signal(lua: &Lua) -> mlua::Result<Value> {
    layer(lua)?.get::<Function>("signal")?.call(())
}

/// Pages of data store keys (each a table with a `KeyName`), all on one page.
pub(crate) fn pages(lua: &Lua, keys: Vec<String>) -> mlua::Result<Value> {
    layer(lua)?.get::<Function>("key_pages")?.call(keys)
}
