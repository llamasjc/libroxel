//! Value types for visual effects (NumberRange, NumberSequence,
//! ColorSequence and their keypoints) and `ParticleEmitter:Emit`.
//!
//! Particle properties are stored simply: a range is a pair of numbers,
//! and a sequence given to a colour or number property keeps its first
//! value.

use libroxel_instance::{ClassKind, Color3, InstanceId, Property, PropertyValue, Vec2};
use mlua::{Lua, MetaMethod, Table, UserData, UserDataFields, UserDataMethods, Value};

use crate::bindings::{LuaColor3, as_number, host};
use crate::names::{self, constructors, effects};

fn err(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LuaNumberRange(pub(crate) Vec2);

impl UserData for LuaNumberRange {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(effects::MIN, |_, this| Ok(this.0.x));
        fields.add_field_method_get(effects::MAX, |_, this| Ok(this.0.y));
        fields.add_meta_field(MetaMethod::Type, effects::NUMBER_RANGE);
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(format!("{} {}", this.0.x, this.0.y)));
    }
}

#[derive(Debug, Clone, Copy)]
struct NumberKeypoint(f32, f32);

impl UserData for NumberKeypoint {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(effects::TIME, |_, this| Ok(this.0));
        fields.add_field_method_get(effects::VALUE, |_, this| Ok(this.1));
        fields.add_meta_field(MetaMethod::Type, effects::NUMBER_KEYPOINT);
    }
}

#[derive(Debug, Clone, Copy)]
struct ColorKeypoint(f32, Color3);

impl UserData for ColorKeypoint {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(effects::TIME, |_, this| Ok(this.0));
        fields.add_field_method_get(effects::VALUE, |_, this| Ok(LuaColor3(this.1)));
        fields.add_meta_field(MetaMethod::Type, effects::COLOR_KEYPOINT);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LuaNumberSequence(Vec<(f32, f32)>);

impl UserData for LuaNumberSequence {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(effects::KEYPOINTS, |_, this| {
            Ok(this.0.iter().map(|&(t, v)| NumberKeypoint(t, v)).collect::<Vec<_>>())
        });
        fields.add_meta_field(MetaMethod::Type, effects::NUMBER_SEQUENCE);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LuaColorSequence(Vec<(f32, Color3)>);

impl UserData for LuaColorSequence {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(effects::KEYPOINTS, |_, this| {
            Ok(this.0.iter().map(|&(t, c)| ColorKeypoint(t, c)).collect::<Vec<_>>())
        });
        fields.add_meta_field(MetaMethod::Type, effects::COLOR_SEQUENCE);
    }
}

fn as_color(value: &Value) -> Option<Color3> {
    match value {
        Value::UserData(ud) => ud.borrow::<LuaColor3>().ok().map(|c| c.0),
        _ => None,
    }
}

/// Properties that scripts see as a `NumberRange`.
pub(crate) fn is_range(property: Property) -> bool {
    matches!(property, Property::Lifetime | Property::Speed)
}

/// How a stored value looks to scripts, when it isn't the usual way.
pub(crate) fn to_lua(lua: &Lua, property: Property, value: &PropertyValue) -> mlua::Result<Option<Value>> {
    match value {
        PropertyValue::Vec2(v) if is_range(property) => {
            Ok(Some(Value::UserData(lua.create_userdata(LuaNumberRange(*v))?)))
        }
        _ => Ok(None),
    }
}

/// A script value for `property` that only effects accept: a range, or a
/// sequence standing in for its first value.
pub(crate) fn from_lua(property: Property, value: &Value) -> Option<PropertyValue> {
    if is_range(property) {
        if let Some(n) = as_number(value) {
            return Some(PropertyValue::Vec2(Vec2::new(n, n)));
        }
        if let Value::UserData(ud) = value
            && let Ok(r) = ud.borrow::<LuaNumberRange>()
        {
            return Some(PropertyValue::Vec2(r.0));
        }
        return None;
    }
    let Value::UserData(ud) = value else { return None };
    match property.expected_kind() {
        "Color3" => {
            ud.borrow::<LuaColorSequence>().ok().and_then(|s| s.0.first().map(|k| PropertyValue::Color3(k.1)))
        }
        "number" => ud
            .borrow::<LuaNumberSequence>()
            .ok()
            .and_then(|s| s.0.first().map(|k| PropertyValue::Number(k.1 as f64))),
        _ => None,
    }
}

/// Reads `new(a)`, `new(a, b)` or `new({keypoints})` for a sequence.
fn sequence<T: Copy>(
    args: (Value, Option<Value>),
    single: impl Fn(&Value) -> Option<T>,
    keypoint: impl Fn(&Value) -> Option<(f32, T)>,
    what: &str,
) -> mlua::Result<Vec<(f32, T)>> {
    match args {
        (Value::Table(t), None) => {
            let mut points = Vec::new();
            for v in t.sequence_values::<Value>() {
                points.push(keypoint(&v?).ok_or_else(|| err(format!("{what}.new expects keypoints")))?);
            }
            if points.len() < 2 {
                return Err(err(format!("{what}.new needs at least two keypoints")));
            }
            Ok(points)
        }
        (a, b) => {
            let a = single(&a).ok_or_else(|| err(format!("{what}.new expects values or keypoints")))?;
            let b = match &b {
                Some(b) => single(b).ok_or_else(|| err(format!("{what}.new expects values or keypoints")))?,
                None => a,
            };
            Ok(vec![(0.0, a), (1.0, b)])
        }
    }
}

/// Fails unless `id` is a particle emitter.
fn emitter(lua: &Lua, id: InstanceId) -> mlua::Result<()> {
    crate::bindings::require_class(lua, id, ClassKind::ParticleEmitter, effects::EMIT)
}

pub(crate) fn add_instance_methods<M: UserDataMethods<crate::bindings::LuaInstance>>(m: &mut M) {
    m.add_method(effects::EMIT, |lua, this, count: Option<f64>| {
        emitter(lua, this.0)?;
        let count = count.unwrap_or(16.0).clamp(0.0, 1000.0) as u32;
        host(lua).world.borrow_mut().emits.push((this.0, count));
        Ok(())
    });
}

pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let g = lua.globals();
    let lib = |f: &dyn Fn(&Table) -> mlua::Result<()>| -> mlua::Result<Table> {
        let t = lua.create_table()?;
        f(&t)?;
        t.set_readonly(true);
        Ok(t)
    };
    g.set(
        effects::NUMBER_RANGE,
        lib(&|t| {
            t.set(
                constructors::NEW,
                lua.create_function(|_, (min, max): (f32, Option<f32>)| {
                    let max = max.unwrap_or(min);
                    if max < min {
                        return Err(err("NumberRange.new: max is less than min"));
                    }
                    Ok(LuaNumberRange(Vec2::new(min, max)))
                })?,
            )
        })?,
    )?;
    g.set(
        effects::NUMBER_KEYPOINT,
        lib(&|t| {
            t.set(
                constructors::NEW,
                lua.create_function(|_, (time, v): (f32, f32)| Ok(NumberKeypoint(time, v)))?,
            )
        })?,
    )?;
    g.set(
        effects::COLOR_KEYPOINT,
        lib(&|t| {
            t.set(
                constructors::NEW,
                lua.create_function(|_, (time, c): (f32, Value)| {
                    let c = as_color(&c).ok_or_else(|| err("ColorSequenceKeypoint.new expects a Color3"))?;
                    Ok(ColorKeypoint(time, c))
                })?,
            )
        })?,
    )?;
    g.set(
        effects::NUMBER_SEQUENCE,
        lib(&|t| {
            t.set(
                constructors::NEW,
                lua.create_function(|_, args: (Value, Option<Value>)| {
                    let points = sequence(
                        args,
                        as_number,
                        |v| match v {
                            Value::UserData(ud) => ud.borrow::<NumberKeypoint>().ok().map(|k| (k.0, k.1)),
                            _ => None,
                        },
                        effects::NUMBER_SEQUENCE,
                    )?;
                    Ok(LuaNumberSequence(points))
                })?,
            )
        })?,
    )?;
    g.set(
        effects::COLOR_SEQUENCE,
        lib(&|t| {
            t.set(
                constructors::NEW,
                lua.create_function(|_, args: (Value, Option<Value>)| {
                    let points = sequence(
                        args,
                        as_color,
                        |v| match v {
                            Value::UserData(ud) => ud.borrow::<ColorKeypoint>().ok().map(|k| (k.0, k.1)),
                            _ => None,
                        },
                        effects::COLOR_SEQUENCE,
                    )?;
                    Ok(LuaColorSequence(points))
                })?,
            )
        })?,
    )?;
    let _ = names::globals::GAME;
    Ok(())
}
