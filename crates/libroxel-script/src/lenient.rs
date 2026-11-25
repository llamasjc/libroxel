//! The bridge to the lenient layer (`lenient.luau`): members that scripts
//! written for other engines use and that libroxel answers with sensible
//! stand-ins. The bindings ask it only after every real member, property
//! and child has said no.

use libroxel_instance::{ClassKind, InstanceId, Property, PropertyValue, Vec3};
use mlua::{AnyUserData, Function, Lua, MultiValue, Table, Value};

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
    // The layer is most of the runtime's Luau and most of it runs rarely:
    // turning all of it into machine code took about two thirds of the time
    // a game took to start. It runs interpreted; game scripts still get the JIT.
    #[cfg(not(target_arch = "arm"))]
    lua.enable_jit(false);
    let layer = lua.load(include_str!("lenient.luau")).set_name("=libroxel.lenient").into_function();
    #[cfg(not(target_arch = "arm"))]
    lua.enable_jit(!cfg!(target_os = "ios"));
    let layer: Table = layer?.call((enum_lib.clone(), lua.globals()))?;
    layer.get::<Function>("add_enums")?.call::<()>((enum_lib.clone(), crate::reflection::enums(lua)?))?;
    layer.get::<Function>("globals")?.call::<()>(())?;
    crate::animation::install_layer(lua, &layer)?;
    // What the engine says a humanoid's character is doing (climbing,
    // swimming), or nil.
    layer.set(
        "moving",
        lua.create_function(|lua, h: Value| {
            let Some(id) = crate::bindings::as_instance(&h) else { return Ok(None) };
            let host = crate::bindings::host(lua);
            let tree = host.tree.borrow();
            Ok(match tree.get_extra(id, crate::names::character::MOVING) {
                Ok(Some(libroxel_instance::PropertyValue::String(s))) => Some(s),
                _ => None,
            })
        })?,
    )?;
    // Where the GUI object was really drawn, for AbsolutePosition and AbsoluteSize.
    layer.set(
        "drawn_rect",
        lua.create_function(|lua, obj: Value| {
            let rect = crate::bindings::as_instance(&obj).and_then(|id| crate::world::gui_rect(lua, id));
            Ok((rect.map(|r| r[0]), rect.map(|r| r[1]), rect.map(|r| r[2]), rect.map(|r| r[3])))
        })?,
    )?;
    // A touch gesture on the GUI objects under a point: fires their event
    // `key` (TouchTap, TouchLongPress) with the rest of the arguments.
    layer.set(
        "gui_touch",
        lua.create_function(|lua, (x, y, key, args): (f32, f32, String, MultiValue)| {
            let under: Vec<InstanceId> = crate::world::gui_under(lua, x, y);
            for id in under {
                fire_event(lua, id, &key, args.clone())?;
            }
            Ok(())
        })?,
    )?;
    // A part's CustomPhysicalProperties, kept in the part (so the physics
    // uses it, and it saves and replicates).
    layer.set(
        "get_physical",
        lua.create_function(|lua, part: Value| crate::compat::custom_physical(lua, &part))?,
    )?;
    layer.set(
        "set_physical",
        lua.create_function(|lua, (part, value): (Value, Value)| {
            crate::compat::set_custom_physical(lua, &part, &value)
        })?,
    )?;
    // The phone's vibration motor, for HapticService.
    layer.set(
        "can_vibrate",
        lua.create_function(|lua, ()| Ok(crate::bindings::host(lua).world.borrow().devices.vibration))?,
    )?;
    layer.set(
        "set_vibration",
        lua.create_function(|lua, strength: f32| {
            crate::bindings::host(lua).world.borrow_mut().vibration = Some(strength.clamp(0.0, 1.0));
            Ok(())
        })?,
    )?;
    // Touches' InputBegan and InputEnded, which are the input service's own.
    layer.set(
        "fire_input",
        lua.create_function(|lua, (kind, obj, processed): (String, Value, bool)| {
            let signal = if kind == "began" {
                crate::bindings::SignalKind::InputBegan
            } else {
                crate::bindings::SignalKind::InputEnded
            };
            crate::bindings::fire(lua, signal, || {
                Ok([obj.clone(), Value::Boolean(processed)].into_iter().collect())
            });
            Ok(())
        })?,
    )?;
    // A Motor6D's Transform, for the engine to pose its parts with.
    layer.set(
        "set_joint_transform",
        lua.create_function(|lua, (obj, x, y, z, rx, ry, rz): (Value, f32, f32, f32, f32, f32, f32)| {
            if let Some(id) = crate::bindings::as_instance(&obj) {
                let host = crate::bindings::host(lua);
                let mut tree = host.tree.borrow_mut();
                let _ =
                    tree.set_extra(id, "TransformPosition", Some(PropertyValue::Vec3(Vec3::new(x, y, z))));
                let _ = tree.set_extra(
                    id,
                    "TransformOrientation",
                    Some(PropertyValue::Vec3(Vec3::new(rx, ry, rz))),
                );
            }
            Ok(())
        })?,
    )?;
    layer.set(
        "set_3d_rendering",
        lua.create_function(|_, on: bool| {
            crate::RENDERING_3D.store(on, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        })?,
    )?;
    // A name for a new drawn picture, unique across every script runtime.
    layer.set("editable_key", lua.create_function(|_, ()| Ok(crate::drawn::new_key()))?)?;
    // A script's drawn picture changed: hand its pixels to the renderer.
    layer.set(
        "editable_store",
        lua.create_function(|_, (key, w, h, pixels): (String, usize, usize, Option<mlua::Buffer>)| {
            match pixels {
                Some(pixels) => crate::drawn::store(&key, [w, h], pixels.to_vec()),
                None => crate::drawn::forget(&key),
            }
            Ok(())
        })?,
    )?;
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

/// Drops what scripts kept on destroyed instances.
pub(crate) fn forget(lua: &Lua, ids: &[InstanceId]) {
    let Ok(layer) = layer(lua) else { return };
    let Ok(forget) = layer.get::<Function>("forget") else { return };
    let keys: Vec<String> = ids.iter().map(|id| instance_key(*id)).collect();
    let _ = forget.call::<()>(keys);
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

/// The API-style class name scripts see for an instance.
pub(crate) fn api_class(lua: &Lua, id: InstanceId, class: ClassKind) -> String {
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
    let api_name = api_class(lua, id, class);
    let kept = crate::bindings::host(lua).tree.borrow().get_extra(id, key).ok().flatten();
    if let Some(kept) = kept {
        return crate::reflection::extra_to_lua(lua, &api_name, key, kept).map(Some);
    }
    let other = (api_name != names::class_name(class)).then_some(api_name.as_str());
    if let Some(found) = lookup(lua, instance_key(id), Value::UserData(obj), key, groups(lua, class, other)?)?
    {
        return Ok(Some(found));
    }
    crate::reflection::default_value(lua, &api_name, key)
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
    let api_name = api_class(lua, id, class);
    let other = (api_name != names::class_name(class)).then_some(api_name.as_str());
    let groups = groups(lua, class, other)?;
    let host = crate::bindings::host(lua);
    let kept = host.tree.borrow().get_extra(id, key).ok().flatten().is_some();
    let in_database = crate::reflection::has_property(&api_name, key);
    if kept || in_database {
        // Plain values are kept on the instance, so saving keeps them.
        if let Some(plain) = crate::reflection::extra_from_lua(&api_name, key, &value) {
            let has_setter: bool = layer(lua)?.get::<Function>("has_setter")?.call((groups.clone(), key))?;
            if has_setter {
                // The layer has its own handling: it owns the value.
                store(lua, instance_key(id), Value::UserData(obj), key, value, groups, false)?;
                let _ = host.tree.borrow_mut().set_extra(id, key, None);
            } else {
                let _ = host.tree.borrow_mut().set_extra(id, key, plain);
                fire_changed(lua, id, key)?;
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
    let key_id = format!("service:{service}");
    match lookup(lua, key_id.clone(), Value::UserData(obj), key, groups)? {
        Some(value) => Ok(value),
        None if let Some(value) = common(lua, &key_id, service, key)? => Ok(value),
        None if crate::reflection::has_property(service, key) => {
            Ok(crate::reflection::default_value(lua, service, key)?.unwrap_or(Value::Nil))
        }
        None => Err(mlua::Error::runtime(format!("{key} is not a valid member of {service}"))),
    }
}

/// Members every instance has (GetPropertyChangedSignal, attributes,
/// IsA, FindFirstChild...), for service objects that aren't instances.
fn common(lua: &Lua, key_id: &str, service: &str, key: &str) -> mlua::Result<Option<Value>> {
    let (found, value): (bool, Value) =
        layer(lua)?.get::<Function>("common")?.call((key_id, service, key))?;
    Ok(found.then_some(value))
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

/// The event that fires when member `key` of an instance changes, for
/// members that aren't saved properties.
pub(crate) fn changed_signal(lua: &Lua, id: InstanceId, key: &str) -> mlua::Result<Value> {
    layer(lua)?.get::<Function>("changed_signal")?.call((instance_key(id), key))
}

/// Fires [`changed_signal`] for `key`, if a script asked for it.
pub(crate) fn fire_changed(lua: &Lua, id: InstanceId, key: &str) -> mlua::Result<()> {
    layer(lua)?.get::<Function>("fire_changed")?.call((instance_key(id), key))
}

/// Hands the pictures scripts drew since the last call to the renderer.
/// Passes a finger's touch to the layer (see `L.touch_event`).
pub(crate) fn touch_event(
    lua: &Lua,
    id: u64,
    phase: &str,
    x: f32,
    y: f32,
    processed: bool,
    time: f64,
) -> mlua::Result<()> {
    // Lua numbers are doubles: keep the finger's number small enough to be exact.
    let id = (id % (1 << 40)) as f64;
    layer(lua)?.get::<Function>("touch_event")?.call((id, phase, x, y, processed, time))
}

pub(crate) fn touch_tick(lua: &Lua, time: f64) -> mlua::Result<()> {
    layer(lua)?.get::<Function>("touch_tick")?.call(time)
}

pub(crate) fn touch_controls_shown(lua: &Lua) -> bool {
    layer(lua).and_then(|l| l.get::<Function>("touch_controls_shown")?.call(())).unwrap_or(true)
}

pub(crate) fn flush_drawn(lua: &Lua) -> mlua::Result<()> {
    layer(lua)?.get::<Function>("flush_editables")?.call(())
}

/// Fires an object's mouse event `key` (such as `MouseButton1Down`) with
/// the mouse position, if a script asked for it.
/// Fires `signal` of the layer's service `name` with `args`.
pub(crate) fn fire_service_signal(
    lua: &Lua,
    name: &str,
    signal: &str,
    args: impl mlua::IntoLuaMulti,
) -> mlua::Result<()> {
    let args = args.into_lua_multi(lua)?;
    let mut all = MultiValue::new();
    all.push_back(Value::String(lua.create_string(name)?));
    all.push_back(Value::String(lua.create_string(signal)?));
    all.extend(args);
    layer(lua)?.get::<Function>("fire_service_signal")?.call::<()>(all)
}

/// Fires event `key` of a service made in Rust (such as the input
/// service), whose events the layer keeps, if a script asked for it.
pub(crate) fn fire_service_event(
    lua: &Lua,
    service: &str,
    key: &str,
    args: impl mlua::IntoLuaMulti,
) -> mlua::Result<()> {
    let mut all = MultiValue::new();
    all.push_back(Value::String(lua.create_string(format!("service:{service}"))?));
    all.push_back(Value::String(lua.create_string(key)?));
    all.extend(args.into_lua_multi(lua)?);
    layer(lua)?.get::<Function>("fire_event")?.call::<()>(all)
}

pub(crate) fn fire_mouse(lua: &Lua, id: InstanceId, key: &str) -> mlua::Result<()> {
    layer(lua)?.get::<Function>("fire_mouse")?.call((instance_key(id), key))
}

pub(crate) fn fire_event(lua: &Lua, id: InstanceId, key: &str, args: MultiValue) -> mlua::Result<()> {
    let mut all = MultiValue::new();
    all.push_back(Value::String(lua.create_string(instance_key(id))?));
    all.push_back(Value::String(lua.create_string(key)?));
    all.extend(args);
    layer(lua)?.get::<Function>("fire_event")?.call::<()>(all)
}

/// Gives a service made as a plain table (see compat.rs) the rest of its
/// members: the layer's extras for it, what every instance has, and the
/// class database's properties. Anything else is "not a valid member".
pub(crate) fn dress_service(lua: &Lua, t: &Table, name: &'static str) -> mlua::Result<()> {
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(move |lua, (_t, key): (Value, String)| {
            let (found, value): (bool, Value) =
                layer(lua)?.get::<Function>("service_extra")?.call((name, key.as_str()))?;
            if found {
                return Ok(value);
            }
            if crate::reflection::has_property(name, &key) {
                return Ok(crate::reflection::default_value(lua, name, &key)?.unwrap_or(Value::Nil));
            }
            Err(mlua::Error::runtime(format!("{key} is not a valid member of {name}")))
        })?,
    )?;
    meta.set(
        "__newindex",
        lua.create_function(move |lua, (t, key, value): (Table, String, Value)| {
            let known: bool = layer(lua)?.get::<Function>("service_extra_set")?.call((
                name,
                key.as_str(),
                value.clone(),
            ))?;
            if !known {
                t.raw_set(key, value)?;
            }
            Ok(())
        })?,
    )?;
    meta.set("__tostring", lua.create_function(move |_, _: Value| Ok(name))?)?;
    t.set_metatable(Some(meta))?;
    Ok(())
}

/// A new event made in Lua, which fires when a script fires it.
pub(crate) fn new_signal(lua: &Lua) -> mlua::Result<Value> {
    layer(lua)?.get::<Function>("signal")?.call(())
}

/// Pages of `rows` (a list), `size` to a page, like the documented `Pages` objects.
pub(crate) fn list_pages(lua: &Lua, rows: Table, size: usize) -> mlua::Result<Value> {
    layer(lua)?.get::<Function>("list_pages")?.call((rows, size.max(1)))
}
