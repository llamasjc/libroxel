//! What an API-style class has, from the open-source reflection
//! database (rbx-dom): every property a script can read or set. The
//! lenient layer uses it for members libroxel has no engine side for, so
//! a script that sets one remembers the value and a script that reads one
//! gets the class's usual default, instead of an error.

use mlua::{Lua, ObjectLike, Table, Value};
use rbx_reflection::{ClassDescriptor, ReflectionDatabase, Scriptability};
use rbx_types::Variant;

fn db() -> &'static ReflectionDatabase<'static> {
    rbx_reflection_database::get_bundled()
}

/// Whether `class` (or a class it comes from) is called `ancestor`.
pub(crate) fn class_is_a(class: &str, ancestor: &str) -> bool {
    let mut current = db().classes.get(class);
    while let Some(c) = current {
        if c.name == ancestor {
            return true;
        }
        current = c.superclass.and_then(|s| db().classes.get(s));
    }
    false
}

/// Whether instances of `name` can be made with `Instance.new`.
pub(crate) fn is_creatable(name: &str) -> bool {
    db().classes.get(name).is_some_and(|c| {
        !c.tags.contains(&rbx_reflection::ClassTag::NotCreatable)
            && !c.tags.contains(&rbx_reflection::ClassTag::Service)
    })
}

/// The class and the classes it comes from, nearest first.
pub(crate) fn lineage(name: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    let mut current = db().classes.get(name);
    while let Some(c) = current {
        out.push(c.name);
        current = c.superclass.and_then(|s| db().classes.get(s));
    }
    out
}

fn find(class: &str, key: &str) -> Option<&'static ClassDescriptor<'static>> {
    let mut current = db().classes.get(class);
    while let Some(c) = current {
        if let Some(p) = c.properties.get(key) {
            return (!matches!(p.scriptability, Scriptability::None)).then_some(c);
        }
        current = c.superclass.and_then(|s| db().classes.get(s));
    }
    None
}

/// Whether scripts can use property `key` on `class`.
pub(crate) fn has_property(class: &str, key: &str) -> bool {
    find(class, key).is_some()
}

/// The value `key` starts at on a new `class`, if scripts can use it:
/// `Some(nil)` when it has no simple default.
pub(crate) fn default_value(lua: &Lua, class: &str, key: &str) -> mlua::Result<Option<Value>> {
    let Some(descriptor) = find(class, key) else { return Ok(None) };
    let start = db().classes.get(class).unwrap_or(descriptor);
    let canonical = match &descriptor.properties[key].kind {
        rbx_reflection::PropertyKind::Alias { alias_for } => alias_for,
        _ => key,
    };
    let enum_name = match descriptor.properties[key].data_type {
        rbx_reflection::DataType::Enum(name) => Some(name),
        _ => None,
    };
    let value = db()
        .find_default_property(start, canonical)
        .or_else(|| db().find_default_property(start, key))
        .map(|v| to_lua(lua, v, enum_name))
        .transpose()?
        .unwrap_or(Value::Nil);
    Ok(Some(value))
}

/// How a value kept by name (see `InstanceTree::set_extra`) reads in a
/// script: numbers stand for enumeration items and brick colours where the
/// property holds one.
pub(crate) fn extra_to_lua(
    lua: &Lua,
    class: &str,
    key: &str,
    value: libroxel_instance::PropertyValue,
) -> mlua::Result<Value> {
    use libroxel_instance::PropertyValue as P;
    let data_type = find(class, key).map(|c| &c.properties[key].data_type);
    match (data_type, &value) {
        (Some(rbx_reflection::DataType::Enum(name)), P::Number(n)) => {
            let e = rbx_types::Enum::from_u32(*n as u32);
            to_lua(lua, &Variant::Enum(e), Some(name))
        }
        (Some(rbx_reflection::DataType::Value(rbx_types::VariantType::BrickColor)), P::Number(n)) => {
            call(lua, "BrickColor", "new", *n)
        }
        // A value with no kind of its own in the tree, kept as text.
        (_, P::String(text)) if let Some(v) = crate::items::rich_to_lua(lua, text)? => Ok(v),
        _ => crate::bindings::property_to_lua(lua, value),
    }
}

/// A script's value for a property libroxel has no setting for, as one
/// the instance can keep (`Some(None)` for nil). None when it can't be kept.
pub(crate) fn extra_from_lua(
    class: &str,
    key: &str,
    value: &Value,
) -> Option<Option<libroxel_instance::PropertyValue>> {
    use libroxel_instance::PropertyValue as P;
    let data_type = &find(class, key)?.properties[key].data_type;
    if let rbx_reflection::DataType::Enum(name) = data_type {
        let item = match value {
            Value::Nil => return Some(None),
            Value::String(s) => s.to_str().ok()?.to_owned(),
            Value::Integer(n) => return Some(Some(P::Number(*n as f64))),
            Value::Number(n) => return Some(Some(P::Number(*n))),
            Value::Table(t) => t.raw_get::<String>("Name").ok()?,
            Value::UserData(u) => {
                let e = u.borrow::<crate::bindings::LuaEnumItem>().ok()?;
                crate::names::enum_item_name(e.0).to_owned()
            }
            _ => return None,
        };
        let number = db().enums.get(name)?.items.get(item.as_str())?;
        return Some(Some(P::Number(f64::from(*number))));
    }
    if matches!(data_type, rbx_reflection::DataType::Value(rbx_types::VariantType::BrickColor)) {
        let Value::UserData(u) = value else { return None };
        let number: f64 = u.get("Number").ok()?;
        return Some(Some(P::Number(number)));
    }
    Some(Some(match value {
        Value::Nil => return Some(None),
        Value::Boolean(b) => P::Bool(*b),
        Value::Integer(n) => P::Number(*n as f64),
        Value::Number(n) => P::Number(*n),
        Value::String(s) => P::String(s.to_str().ok()?.to_owned()),
        Value::Vector(v) => P::Vec3(libroxel_instance::Vec3::new(v.x(), v.y(), v.z())),
        Value::UserData(u) => {
            if let Ok(c) = u.borrow::<crate::bindings::LuaColor3>() {
                P::Color3(c.0)
            } else if let Ok(v) = u.borrow::<crate::types::LuaVector2>() {
                P::Vec2(v.0)
            } else if let Ok(v) = u.borrow::<crate::types::LuaUDim2>() {
                P::UDim2(v.0)
            } else if let Ok(v) = u.borrow::<crate::types::LuaUDim>() {
                P::UDim(v.0)
            } else {
                return None;
            }
        }
        _ => return None,
    }))
}

/// Every enumeration in the class database: name to a list of
/// `{item name, number}`, numbers in order.
pub(crate) fn enums(lua: &Lua) -> mlua::Result<Table> {
    let out = lua.create_table()?;
    for (name, e) in &db().enums {
        let mut items: Vec<(&str, u32)> = e.items.iter().map(|(k, v)| (*k, *v)).collect();
        items.sort_by_key(|(k, v)| (*v, *k));
        let list = lua.create_table()?;
        for (k, v) in items {
            list.raw_push(lua.create_sequence_from([
                Value::String(lua.create_string(k)?),
                Value::Integer(v as mlua::Integer),
            ])?)?;
        }
        out.raw_set(*name, list)?;
    }
    Ok(out)
}

fn call(lua: &Lua, library: &str, function: &str, args: impl mlua::IntoLuaMulti) -> mlua::Result<Value> {
    let lib: Table = lua.globals().get(library)?;
    let f: mlua::Function = lib.get(function)?;
    f.call(args)
}

fn to_lua(lua: &Lua, value: &Variant, enum_name: Option<&str>) -> mlua::Result<Value> {
    Ok(match value {
        Variant::Bool(b) => Value::Boolean(*b),
        Variant::Int32(n) => Value::Integer(*n as mlua::Integer),
        Variant::Int64(n) => Value::Integer(*n as mlua::Integer),
        Variant::Float32(n) => Value::Number(f64::from(*n)),
        Variant::Float64(n) => Value::Number(*n),
        Variant::String(s) => Value::String(lua.create_string(s)?),
        Variant::ContentId(c) => Value::String(lua.create_string(c.as_str())?),
        Variant::Content(c) => Value::String(lua.create_string(c.as_uri().unwrap_or_default())?),
        Variant::Vector3(v) => call(lua, "Vector3", "new", (v.x, v.y, v.z))?,
        Variant::Vector2(v) => call(lua, "Vector2", "new", (v.x, v.y))?,
        Variant::Color3(c) => call(lua, "Color3", "new", (c.r, c.g, c.b))?,
        Variant::Color3uint8(c) => call(lua, "Color3", "fromRGB", (c.r, c.g, c.b))?,
        Variant::UDim(u) => call(lua, "UDim", "new", (u.scale, u.offset))?,
        Variant::UDim2(u) => call(lua, "UDim2", "new", (u.x.scale, u.x.offset, u.y.scale, u.y.offset))?,
        Variant::CFrame(cf) => {
            let (p, m) = (cf.position, cf.orientation);
            let args = (p.x, p.y, p.z, m.x.x, m.x.y, m.x.z, m.y.x, m.y.y, m.y.z, m.z.x, m.z.y, m.z.z);
            call(lua, "CFrame", "new", args)?
        }
        Variant::BrickColor(b) => call(lua, "BrickColor", "new", *b as u16)?,
        Variant::NumberRange(r) => call(lua, "NumberRange", "new", (r.min, r.max))?,
        Variant::NumberSequence(s) => {
            call(lua, "NumberSequence", "new", s.keypoints.first().map_or(0.0, |k| k.value))?
        }
        Variant::ColorSequence(s) => {
            let c = s.keypoints.first().map_or(rbx_types::Color3::new(1.0, 1.0, 1.0), |k| k.color);
            let colour = call(lua, "Color3", "new", (c.r, c.g, c.b))?;
            call(lua, "ColorSequence", "new", colour)?
        }
        Variant::Rect(r) => call(lua, "Rect", "new", (r.min.x, r.min.y, r.max.x, r.max.y))?,
        Variant::Font(f) => call(lua, "Font", "new", f.family.as_str())?,
        Variant::Enum(e) => {
            let Some(name) = enum_name else { return Ok(Value::Nil) };
            let Some(item) = db()
                .enums
                .get(name)
                .and_then(|en| en.items.iter().find(|(_, v)| **v == e.to_u32()).map(|(k, _)| *k))
            else {
                return Ok(Value::Nil);
            };
            let enums: Table = lua.globals().get("Enum")?;
            match enums.get::<Value>(name)? {
                Value::Nil => Value::Nil,
                list => match list {
                    Value::Table(t) => t.get(item)?,
                    Value::UserData(u) => u.get::<Value>(item).unwrap_or(Value::Nil),
                    _ => Value::Nil,
                },
            }
        }
        _ => Value::Nil,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_database_knows_api_classes() {
        assert!(class_is_a("Part", "BasePart"));
        assert!(has_property("Lighting", "TimeOfDay"));
        assert!(has_property("Part", "Position"));
        assert!(!has_property("Part", "NoSuchThing"));
        assert!(is_creatable("AnimationController"));
        assert!(!is_creatable("Workspace"));
        assert_eq!(lineage("Part")[..2], ["Part", "FormFactorPart"]);
    }
}
