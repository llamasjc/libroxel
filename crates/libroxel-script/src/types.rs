//! Value types that don't need the host: CFrame, Vector2 and UDim2.
//! Every script-visible name comes from [`crate::names`].

use libroxel_instance::{CFrame, Rotation, UDim2, Vec2, Vec3};
use mlua::{Lua, MetaMethod, Table, UserData, UserDataFields, UserDataMethods, Value};

use crate::bindings::{LuaVector3, as_number, as_vector3};
use crate::names::{self, cframe, constructors, gui_values};

fn err(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

// ---------------------------------------------------------------------------
// CFrame
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub(crate) struct LuaCFrame(pub CFrame);

pub(crate) fn as_cframe(value: &Value) -> Option<CFrame> {
    match value {
        Value::UserData(ud) => ud.borrow::<LuaCFrame>().ok().map(|c| c.0),
        _ => None,
    }
}

impl UserData for LuaCFrame {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        use cframe::*;
        fields.add_field_method_get(POSITION, |_, this| Ok(LuaVector3(this.0.pos)));
        fields.add_field_method_get(X, |_, this| Ok(this.0.pos.x));
        fields.add_field_method_get(Y, |_, this| Ok(this.0.pos.y));
        fields.add_field_method_get(Z, |_, this| Ok(this.0.pos.z));
        fields.add_field_method_get(LOOK_VECTOR, |_, this| Ok(LuaVector3(-this.0.rot.z)));
        fields.add_field_method_get(RIGHT_VECTOR, |_, this| Ok(LuaVector3(this.0.rot.x)));
        fields.add_field_method_get(UP_VECTOR, |_, this| Ok(LuaVector3(this.0.rot.y)));
        fields.add_field_method_get(ROTATION, |_, this| Ok(LuaCFrame(CFrame::new(Vec3::ZERO, this.0.rot))));
        fields.add_meta_field(MetaMethod::Type, TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        use cframe::*;
        let cf_arg =
            |v: &Value, what: &str| as_cframe(v).ok_or_else(|| err(format!("{what} expects a CFrame")));
        methods.add_method(INVERSE, |_, this, ()| Ok(LuaCFrame(this.0.inverse())));
        methods.add_method(LERP, move |_, this, (other, t): (Value, f32)| {
            Ok(LuaCFrame(this.0.lerp(cf_arg(&other, LERP)?, t)))
        });
        methods.add_method(TO_WORLD_SPACE, move |_, this, other: Value| {
            Ok(LuaCFrame(this.0 * cf_arg(&other, TO_WORLD_SPACE)?))
        });
        methods.add_method(TO_OBJECT_SPACE, move |_, this, other: Value| {
            Ok(LuaCFrame(this.0.inverse() * cf_arg(&other, TO_OBJECT_SPACE)?))
        });
        methods.add_method(POINT_TO_WORLD_SPACE, |_, this, p: Value| {
            let p = as_vector3(&p).ok_or_else(|| err("PointToWorldSpace expects a Vector3"))?;
            Ok(LuaVector3(this.0.point_to_world(p)))
        });
        methods.add_method(POINT_TO_OBJECT_SPACE, |_, this, p: Value| {
            let p = as_vector3(&p).ok_or_else(|| err("PointToObjectSpace expects a Vector3"))?;
            Ok(LuaVector3(this.0.inverse().point_to_world(p)))
        });
        methods.add_method(TO_ORIENTATION, |_, this, ()| {
            let d = this.0.rot.to_orientation();
            Ok((d.x.to_radians(), d.y.to_radians(), d.z.to_radians()))
        });

        methods.add_meta_function(MetaMethod::Mul, |lua, (a, b): (Value, Value)| {
            let a = as_cframe(&a).ok_or_else(|| err("attempt to multiply a CFrame"))?;
            if let Some(b) = as_cframe(&b) {
                return Ok(Value::UserData(lua.create_userdata(LuaCFrame(a * b))?));
            }
            if let Some(v) = as_vector3(&b) {
                return Ok(Value::UserData(lua.create_userdata(LuaVector3(a.point_to_world(v)))?));
            }
            Err(err("a CFrame can only be multiplied by a CFrame or a Vector3"))
        });
        methods.add_meta_function(MetaMethod::Add, |_, (a, b): (Value, Value)| {
            match (as_cframe(&a), as_vector3(&b)) {
                (Some(cf), Some(v)) => Ok(LuaCFrame(CFrame::new(cf.pos + v, cf.rot))),
                _ => Err(err("a CFrame can only have a Vector3 added to it")),
            }
        });
        methods.add_meta_function(MetaMethod::Sub, |_, (a, b): (Value, Value)| {
            match (as_cframe(&a), as_vector3(&b)) {
                (Some(cf), Some(v)) => Ok(LuaCFrame(CFrame::new(cf.pos - v, cf.rot))),
                _ => Err(err("a CFrame can only have a Vector3 subtracted from it")),
            }
        });
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            Ok(matches!((as_cframe(&a), as_cframe(&b)), (Some(a), Some(b)) if a == b))
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            let (p, r) = (this.0.pos, this.0.rot);
            Ok(format!(
                "{}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}",
                p.x, p.y, p.z, r.x.x, r.y.x, r.z.x, r.x.y, r.y.y, r.z.y, r.x.z, r.y.z, r.z.z
            ))
        });
    }
}

fn cframe_lib(lua: &Lua) -> mlua::Result<Table> {
    let lib = lua.create_table()?;
    lib.set(
        constructors::NEW,
        lua.create_function(|_, args: mlua::Variadic<Value>| {
            let cf = match args.as_slice() {
                [] => CFrame::IDENTITY,
                [p] => CFrame::new(
                    as_vector3(p).ok_or_else(|| err("CFrame.new expects a Vector3"))?,
                    Rotation::IDENTITY,
                ),
                [p, look] if as_vector3(p).is_some() => {
                    let target = as_vector3(look).ok_or_else(|| err("CFrame.new expects two Vector3s"))?;
                    CFrame::look_at(as_vector3(p).unwrap_or_default(), target)
                }
                [x, y, z, ..] => {
                    let n = |v: &Value| as_number(v).ok_or_else(|| err("CFrame.new expects numbers"));
                    CFrame::new(Vec3::new(n(x)?, n(y)?, n(z)?), Rotation::IDENTITY)
                }
                _ => return Err(err("CFrame.new expects a position")),
            };
            Ok(LuaCFrame(cf))
        })?,
    )?;
    lib.set(
        cframe::LOOK_AT,
        lua.create_function(|_, (at, target): (Value, Value)| {
            match (as_vector3(&at), as_vector3(&target)) {
                (Some(at), Some(target)) => Ok(LuaCFrame(CFrame::look_at(at, target))),
                _ => Err(err("CFrame.lookAt expects two Vector3s")),
            }
        })?,
    )?;
    lib.set(
        cframe::ANGLES,
        lua.create_function(|_, (x, y, z): (Option<f32>, Option<f32>, Option<f32>)| {
            let rot = Rotation::about_x(x.unwrap_or(0.0))
                * Rotation::about_y(y.unwrap_or(0.0))
                * Rotation::about_z(z.unwrap_or(0.0));
            Ok(LuaCFrame(CFrame::new(Vec3::ZERO, rot)))
        })?,
    )?;
    lib.set(
        cframe::FROM_ORIENTATION,
        lua.create_function(|_, (x, y, z): (Option<f32>, Option<f32>, Option<f32>)| {
            let degrees = Vec3::new(x.unwrap_or(0.0), y.unwrap_or(0.0), z.unwrap_or(0.0))
                * std::f32::consts::FRAC_1_PI
                * 180.0;
            Ok(LuaCFrame(CFrame::new(Vec3::ZERO, Rotation::from_orientation(degrees))))
        })?,
    )?;
    lib.set(
        cframe::FROM_AXIS_ANGLE,
        lua.create_function(|_, (axis, angle): (Value, f32)| {
            let axis = as_vector3(&axis).ok_or_else(|| err("CFrame.fromAxisAngle expects a Vector3"))?;
            Ok(LuaCFrame(CFrame::new(Vec3::ZERO, Rotation::about_axis(axis, angle))))
        })?,
    )?;
    lib.set(cframe::IDENTITY, LuaCFrame(CFrame::IDENTITY))?;
    Ok(lib)
}

// ---------------------------------------------------------------------------
// Vector2
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub(crate) struct LuaVector2(pub Vec2);

pub(crate) fn as_vector2(value: &Value) -> Option<Vec2> {
    match value {
        Value::UserData(ud) => ud.borrow::<LuaVector2>().ok().map(|v| v.0),
        _ => None,
    }
}

fn vector2_operands(a: &Value, b: &Value) -> mlua::Result<(Vec2, Vec2)> {
    let lift = |v: &Value| as_vector2(v).or_else(|| as_number(v).map(|n| Vec2::new(n, n)));
    match (lift(a), lift(b)) {
        (Some(a), Some(b)) => Ok((a, b)),
        _ => Err(err(format!("attempt to do arithmetic on a {} and a {}", a.type_name(), b.type_name()))),
    }
}

impl UserData for LuaVector2 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(gui_values::X, |_, this| Ok(this.0.x));
        fields.add_field_method_get(gui_values::Y, |_, this| Ok(this.0.y));
        fields.add_field_method_get(names::vector3::MAGNITUDE, |_, this| Ok(this.0.x.hypot(this.0.y)));
        fields.add_meta_field(MetaMethod::Type, gui_values::VECTOR2_TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_function(MetaMethod::Add, |_, (a, b): (Value, Value)| {
            let (a, b) = vector2_operands(&a, &b)?;
            Ok(LuaVector2(Vec2::new(a.x + b.x, a.y + b.y)))
        });
        methods.add_meta_function(MetaMethod::Sub, |_, (a, b): (Value, Value)| {
            let (a, b) = vector2_operands(&a, &b)?;
            Ok(LuaVector2(Vec2::new(a.x - b.x, a.y - b.y)))
        });
        methods.add_meta_function(MetaMethod::Mul, |_, (a, b): (Value, Value)| {
            let (a, b) = vector2_operands(&a, &b)?;
            Ok(LuaVector2(Vec2::new(a.x * b.x, a.y * b.y)))
        });
        methods.add_meta_function(MetaMethod::Div, |_, (a, b): (Value, Value)| {
            let (a, b) = vector2_operands(&a, &b)?;
            Ok(LuaVector2(Vec2::new(a.x / b.x, a.y / b.y)))
        });
        methods
            .add_meta_method(MetaMethod::Unm, |_, this, ()| Ok(LuaVector2(Vec2::new(-this.0.x, -this.0.y))));
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            Ok(matches!((as_vector2(&a), as_vector2(&b)), (Some(a), Some(b)) if a == b))
        });
        methods
            .add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(format!("{}, {}", this.0.x, this.0.y)));
    }
}

// ---------------------------------------------------------------------------
// UDim2
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub(crate) struct LuaUDim2(pub UDim2);

/// One axis of a UDim2.
#[derive(Debug, Clone, Copy)]
struct LuaUDim {
    scale: f32,
    offset: f32,
}

impl UserData for LuaUDim {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(gui_values::SCALE, |_, this| Ok(this.scale));
        fields.add_field_method_get(gui_values::OFFSET, |_, this| Ok(this.offset));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{}, {}", this.scale, this.offset))
        });
    }
}

pub(crate) fn as_udim2(value: &Value) -> Option<UDim2> {
    match value {
        Value::UserData(ud) => ud.borrow::<LuaUDim2>().ok().map(|v| v.0),
        _ => None,
    }
}

impl UserData for LuaUDim2 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        use gui_values::*;
        let x = |u: UDim2| LuaUDim { scale: u.x_scale, offset: u.x_offset };
        let y = |u: UDim2| LuaUDim { scale: u.y_scale, offset: u.y_offset };
        fields.add_field_method_get(X, move |_, this| Ok(x(this.0)));
        fields.add_field_method_get(WIDTH, move |_, this| Ok(x(this.0)));
        fields.add_field_method_get(Y, move |_, this| Ok(y(this.0)));
        fields.add_field_method_get(HEIGHT, move |_, this| Ok(y(this.0)));
        fields.add_meta_field(MetaMethod::Type, UDIM2_TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(names::vector3::LERP, |_, this, (other, t): (Value, f32)| {
            let other = as_udim2(&other).ok_or_else(|| err("Lerp expects a UDim2"))?;
            Ok(LuaUDim2(this.0.lerp(other, t)))
        });
        let both = |a: &Value, b: &Value| match (as_udim2(a), as_udim2(b)) {
            (Some(a), Some(b)) => Ok((a, b)),
            _ => Err(err("UDim2 arithmetic needs two UDim2s")),
        };
        methods.add_meta_function(MetaMethod::Add, move |_, (a, b): (Value, Value)| {
            let (a, b) = both(&a, &b)?;
            Ok(LuaUDim2(UDim2::new(
                a.x_scale + b.x_scale,
                a.x_offset + b.x_offset,
                a.y_scale + b.y_scale,
                a.y_offset + b.y_offset,
            )))
        });
        methods.add_meta_function(MetaMethod::Sub, move |_, (a, b): (Value, Value)| {
            let (a, b) = both(&a, &b)?;
            Ok(LuaUDim2(UDim2::new(
                a.x_scale - b.x_scale,
                a.x_offset - b.x_offset,
                a.y_scale - b.y_scale,
                a.y_offset - b.y_offset,
            )))
        });
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            Ok(matches!((as_udim2(&a), as_udim2(&b)), (Some(a), Some(b)) if a == b))
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            let u = this.0;
            Ok(format!("{{{}, {}}}, {{{}, {}}}", u.x_scale, u.x_offset, u.y_scale, u.y_offset))
        });
    }
}

/// Installs the `CFrame`, `Vector2` and `UDim2` libraries.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let g = lua.globals();
    g.set(names::globals::CFRAME, cframe_lib(lua)?)?;

    let vector2 = lua.create_table()?;
    vector2.set(
        constructors::NEW,
        lua.create_function(|_, (x, y): (Option<f32>, Option<f32>)| {
            Ok(LuaVector2(Vec2::new(x.unwrap_or(0.0), y.unwrap_or(0.0))))
        })?,
    )?;
    vector2.set(constructors::ZERO, LuaVector2(Vec2::ZERO))?;
    vector2.set(constructors::ONE, LuaVector2(Vec2::new(1.0, 1.0)))?;
    g.set(names::globals::VECTOR2, vector2)?;

    let udim2 = lua.create_table()?;
    udim2.set(
        constructors::NEW,
        lua.create_function(|_, (xs, xo, ys, yo): (Option<f32>, Option<f32>, Option<f32>, Option<f32>)| {
            Ok(LuaUDim2(UDim2::new(
                xs.unwrap_or(0.0),
                xo.unwrap_or(0.0),
                ys.unwrap_or(0.0),
                yo.unwrap_or(0.0),
            )))
        })?,
    )?;
    udim2.set(
        gui_values::FROM_SCALE,
        lua.create_function(|_, (x, y): (Option<f32>, Option<f32>)| {
            Ok(LuaUDim2(UDim2::new(x.unwrap_or(0.0), 0.0, y.unwrap_or(0.0), 0.0)))
        })?,
    )?;
    udim2.set(
        gui_values::FROM_OFFSET,
        lua.create_function(|_, (x, y): (Option<f32>, Option<f32>)| {
            Ok(LuaUDim2(UDim2::new(0.0, x.unwrap_or(0.0), 0.0, y.unwrap_or(0.0))))
        })?,
    )?;
    g.set(names::globals::UDIM2, udim2)?;
    Ok(())
}
