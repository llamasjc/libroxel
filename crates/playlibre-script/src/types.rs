//! Value types that don't need the host: CFrame, Vector2 and UDim2.
//! Every script-visible name comes from [`crate::names`].

use mlua::{Lua, MetaMethod, Table, UserData, UserDataFields, UserDataMethods, Value};
use playlibre_instance::{CFrame, Rotation, UDim, UDim2, Vec2, Vec3};

use crate::bindings::{LuaVector3, as_number, as_vector3};
use crate::names::{self, cframe, constructors, gui_values};

fn err(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

// ---------------------------------------------------------------------------
// CFrame
// ---------------------------------------------------------------------------

type Entry = fn(&Rotation) -> f32;

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
        for name in [POSITION, OLD_POSITION] {
            fields.add_field_method_get(name, |_, this| Ok(LuaVector3(this.0.pos)));
        }
        fields.add_field_method_get(X, |_, this| Ok(this.0.pos.x));
        fields.add_field_method_get(Y, |_, this| Ok(this.0.pos.y));
        fields.add_field_method_get(Z, |_, this| Ok(this.0.pos.z));
        for name in [LOOK_VECTOR, OLD_LOOK_VECTOR] {
            fields.add_field_method_get(name, |_, this| Ok(LuaVector3(-this.0.rot.z)));
        }
        for name in [RIGHT_VECTOR, OLD_RIGHT_VECTOR] {
            fields.add_field_method_get(name, |_, this| Ok(LuaVector3(this.0.rot.x)));
        }
        for name in [UP_VECTOR, OLD_UP_VECTOR] {
            fields.add_field_method_get(name, |_, this| Ok(LuaVector3(this.0.rot.y)));
        }
        fields.add_field_method_get(X_VECTOR, |_, this| Ok(LuaVector3(this.0.rot.x)));
        fields.add_field_method_get(Y_VECTOR, |_, this| Ok(LuaVector3(this.0.rot.y)));
        fields.add_field_method_get(Z_VECTOR, |_, this| Ok(LuaVector3(this.0.rot.z)));
        fields.add_field_method_get(ROTATION, |_, this| Ok(LuaCFrame(CFrame::new(Vec3::ZERO, this.0.rot))));
        let entries: [(&'static str, Entry); 9] = [
            ("R00", |r| r.x.x),
            ("R01", |r| r.y.x),
            ("R02", |r| r.z.x),
            ("R10", |r| r.x.y),
            ("R11", |r| r.y.y),
            ("R12", |r| r.z.y),
            ("R20", |r| r.x.z),
            ("R21", |r| r.y.z),
            ("R22", |r| r.z.z),
        ];
        for (name, get) in entries {
            fields.add_field_method_get(name, move |_, this| Ok(get(&this.0.rot)));
        }
        fields.add_meta_field(MetaMethod::Type, TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        use cframe::*;
        let cf_arg =
            |v: &Value, what: &str| as_cframe(v).ok_or_else(|| err(format!("{what} expects a CFrame")));
        for name in [INVERSE, "inverse"] {
            methods.add_method(name, |_, this, ()| Ok(LuaCFrame(this.0.inverse())));
        }
        for name in [LERP, "lerp"] {
            methods.add_method(name, move |_, this, (other, t): (Value, f32)| {
                Ok(LuaCFrame(this.0.lerp(cf_arg(&other, LERP)?, t)))
            });
        }
        for name in [TO_WORLD_SPACE, "toWorldSpace"] {
            methods.add_method(name, move |_, this, other: Value| {
                Ok(LuaCFrame(this.0 * cf_arg(&other, TO_WORLD_SPACE)?))
            });
        }
        for name in [TO_OBJECT_SPACE, "toObjectSpace"] {
            methods.add_method(name, move |_, this, other: Value| {
                Ok(LuaCFrame(this.0.inverse() * cf_arg(&other, TO_OBJECT_SPACE)?))
            });
        }
        for name in [POINT_TO_WORLD_SPACE, "pointToWorldSpace"] {
            methods.add_method(name, |_, this, p: Value| {
                let p = as_vector3(&p).ok_or_else(|| err("PointToWorldSpace expects a Vector3"))?;
                Ok(LuaVector3(this.0.point_to_world(p)))
            });
        }
        for name in [POINT_TO_OBJECT_SPACE, "pointToObjectSpace"] {
            methods.add_method(name, |_, this, p: Value| {
                let p = as_vector3(&p).ok_or_else(|| err("PointToObjectSpace expects a Vector3"))?;
                Ok(LuaVector3(this.0.inverse().point_to_world(p)))
            });
        }
        methods.add_method(TO_ORIENTATION, |_, this, ()| {
            let d = this.0.rot.to_orientation();
            Ok((d.x.to_radians(), d.y.to_radians(), d.z.to_radians()))
        });
        methods.add_method(TO_EULER_ANGLES_YXZ, |_, this, ()| {
            let d = this.0.rot.to_orientation();
            Ok((d.x.to_radians(), d.y.to_radians(), d.z.to_radians()))
        });
        for name in [TO_EULER_ANGLES_XYZ, TO_EULER_ANGLES] {
            methods.add_method(name, |_, this, ()| Ok(euler_xyz(this.0.rot)));
        }
        for name in [TO_AXIS_ANGLE, "toAxisAngle"] {
            methods.add_method(name, |_, this, ()| {
                let [x, y, z, w] = this.0.rot.to_quat();
                let w = w.clamp(-1.0, 1.0);
                let angle = 2.0 * w.acos();
                let s = (1.0 - w * w).sqrt();
                let axis = if s > 1e-6 { Vec3::new(x / s, y / s, z / s) } else { Vec3::new(1.0, 0.0, 0.0) };
                Ok((LuaVector3(axis), angle))
            });
        }
        for name in ["VectorToWorldSpace", "vectorToWorldSpace"] {
            methods.add_method(name, |_, this, v: Value| {
                let v = as_vector3(&v).ok_or_else(|| err("VectorToWorldSpace expects a Vector3"))?;
                Ok(LuaVector3(this.0.rot.apply(v)))
            });
        }
        for name in ["VectorToObjectSpace", "vectorToObjectSpace"] {
            methods.add_method(name, |_, this, v: Value| {
                let v = as_vector3(&v).ok_or_else(|| err("VectorToObjectSpace expects a Vector3"))?;
                Ok(LuaVector3(this.0.rot.transpose().apply(v)))
            });
        }
        methods.add_method("Orthonormalize", |_, this, ()| {
            Ok(LuaCFrame(CFrame::new(this.0.pos, Rotation::from_quat(this.0.rot.to_quat()))))
        });
        methods.add_method("FuzzyEq", |_, this, (other, epsilon): (Value, Option<f32>)| {
            let o = as_cframe(&other).ok_or_else(|| err("FuzzyEq expects a CFrame"))?;
            let e = epsilon.unwrap_or(1e-5);
            let (a, b) = (this.0, o);
            let close = |p: Vec3, q: Vec3| (p - q).length() <= e.max(1e-5) * 10.0;
            Ok(close(a.pos, b.pos)
                && close(a.rot.x, b.rot.x)
                && close(a.rot.y, b.rot.y)
                && close(a.rot.z, b.rot.z))
        });
        methods.add_method("AngleBetween", |_, this, other: Value| {
            let o = as_cframe(&other).ok_or_else(|| err("AngleBetween expects a CFrame"))?;
            let [_, _, _, w] = (this.0.rot.transpose() * o.rot).to_quat();
            Ok(2.0 * w.abs().clamp(0.0, 1.0).acos())
        });
        for name in [GET_COMPONENTS, "components"] {
            methods.add_method(name, |_, this, ()| {
                let (p, r) = (this.0.pos, this.0.rot);
                Ok(mlua::Variadic::from_iter([
                    p.x, p.y, p.z, r.x.x, r.y.x, r.z.x, r.x.y, r.y.y, r.z.y, r.x.z, r.y.z, r.z.z,
                ]))
            });
        }

        // The CFrame is borrowed straight from the Luau stack: taking it as
        // a plain value made a registry reference for it, a good part of
        // what `a * b` cost. (`b` stays a value: a Vector3 needs no
        // reference, and trying the CFrame first would cost a Vector3 an
        // error built and thrown away.)
        methods.add_meta_function(
            MetaMethod::Mul,
            |lua, (a, b): (mlua::Either<mlua::UserDataRef<LuaCFrame>, Value>, Value)| {
                let a = match a {
                    mlua::Either::Left(a) => a.0,
                    mlua::Either::Right(other) => {
                        as_cframe(&other).ok_or_else(|| err("attempt to multiply a CFrame"))?
                    }
                };
                if let Some(v) = as_vector3(&b) {
                    return Ok(crate::bindings::vector_value(a.point_to_world(v)));
                }
                if let Some(b) = as_cframe(&b) {
                    return Ok(Value::UserData(lua.create_userdata(LuaCFrame(a * b))?));
                }
                Err(err("a CFrame can only be multiplied by a CFrame or a Vector3"))
            },
        );
        // (The CFrame borrowed from the stack, as for `*`.)
        let left = |a: mlua::Either<mlua::UserDataRef<LuaCFrame>, Value>| match a {
            mlua::Either::Left(a) => Some(a.0),
            mlua::Either::Right(other) => as_cframe(&other),
        };
        methods.add_meta_function(
            MetaMethod::Add,
            move |_, (a, b): (mlua::Either<mlua::UserDataRef<LuaCFrame>, Value>, Value)| match (
                left(a),
                as_vector3(&b),
            ) {
                (Some(cf), Some(v)) => Ok(LuaCFrame(CFrame::new(cf.pos + v, cf.rot))),
                _ => Err(err("a CFrame can only have a Vector3 added to it")),
            },
        );
        methods.add_meta_function(
            MetaMethod::Sub,
            move |_, (a, b): (mlua::Either<mlua::UserDataRef<LuaCFrame>, Value>, Value)| match (
                left(a),
                as_vector3(&b),
            ) {
                (Some(cf), Some(v)) => Ok(LuaCFrame(CFrame::new(cf.pos - v, cf.rot))),
                _ => Err(err("a CFrame can only have a Vector3 subtracted from it")),
            },
        );
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

/// The angles (radians) that `Angles` turns by to make `r`: about X, then
/// Y, then Z.
fn euler_xyz(r: Rotation) -> (f32, f32, f32) {
    let b = r.z.x.clamp(-1.0, 1.0).asin();
    if b.cos().abs() > 1e-5 {
        ((-r.z.y).atan2(r.z.z), b, (-r.y.x).atan2(r.x.x))
    } else {
        // Straight up or down: put all the turn about X.
        (r.y.z.atan2(r.y.y), b, 0.0)
    }
}

fn rotation_xyz(x: f32, y: f32, z: f32) -> Rotation {
    Rotation::about_x(x) * Rotation::about_y(y) * Rotation::about_z(z)
}

/// The shortest rotation that turns `from` to point along `to`.
fn rotation_between(from: Vec3, to: Vec3) -> Rotation {
    let (a, b) = (from.normalize_or_zero(), to.normalize_or_zero());
    if a.length() < 0.5 || b.length() < 0.5 {
        return Rotation::IDENTITY;
    }
    let axis = a.cross(b);
    let cos = a.dot(b).clamp(-1.0, 1.0);
    if axis.length() < 1e-6 {
        if cos > 0.0 {
            return Rotation::IDENTITY;
        }
        // Opposite: half a turn about any axis square to them.
        let side = if a.x.abs() < 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) };
        return Rotation::about_axis(a.cross(side), std::f32::consts::PI);
    }
    Rotation::about_axis(axis, axis.length().atan2(cos))
}

fn rotation_yxz(x: f32, y: f32, z: f32) -> Rotation {
    Rotation::about_y(y) * Rotation::about_x(x) * Rotation::about_z(z)
}

/// A rotation from its rows, tidied into a proper one.
fn rotation_rows(m: [f32; 9]) -> Rotation {
    let x = Vec3::new(m[0], m[3], m[6]).normalize_or_zero();
    let y0 = Vec3::new(m[1], m[4], m[7]);
    let z = x.cross(y0).normalize_or_zero();
    if x.length() < 0.5 || z.length() < 0.5 {
        return Rotation::IDENTITY;
    }
    Rotation { x, y: z.cross(x), z }
}

fn cframe_lib(lua: &Lua) -> mlua::Result<Table> {
    let lib = lua.create_table()?;
    lib.set(
        "lookAlong",
        lua.create_function(|_, (at, direction, _up): (Value, Value, Option<Value>)| {
            let at = as_vector3(&at).ok_or_else(|| err("CFrame.lookAlong expects a Vector3"))?;
            let d = as_vector3(&direction).ok_or_else(|| err("CFrame.lookAlong expects a direction"))?;
            Ok(LuaCFrame(CFrame::look_at(at, at + d)))
        })?,
    )?;
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
                [x, y, z, rest @ ..] => {
                    let n = |v: &Value| as_number(v).ok_or_else(|| err("CFrame.new expects numbers"));
                    let pos = Vec3::new(n(x)?, n(y)?, n(z)?);
                    let rot = match rest {
                        [] => Rotation::IDENTITY,
                        // A quaternion.
                        [qx, qy, qz, qw] => Rotation::from_quat([n(qx)?, n(qy)?, n(qz)?, n(qw)?]),
                        // The rotation's rows.
                        m if m.len() == 9 => {
                            let mut rows = [0.0; 9];
                            for (r, v) in rows.iter_mut().zip(m) {
                                *r = n(v)?;
                            }
                            rotation_rows(rows)
                        }
                        _ => return Err(err("CFrame.new expects 3, 7 or 12 numbers")),
                    };
                    CFrame::new(pos, rot)
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
            let rot = rotation_xyz(x.unwrap_or(0.0), y.unwrap_or(0.0), z.unwrap_or(0.0));
            Ok(LuaCFrame(CFrame::new(Vec3::ZERO, rot)))
        })?,
    )?;
    lib.set(
        cframe::FROM_EULER_ANGLES_XYZ,
        lua.create_function(|_, (x, y, z): (Option<f32>, Option<f32>, Option<f32>)| {
            let rot = rotation_xyz(x.unwrap_or(0.0), y.unwrap_or(0.0), z.unwrap_or(0.0));
            Ok(LuaCFrame(CFrame::new(Vec3::ZERO, rot)))
        })?,
    )?;
    lib.set(
        cframe::FROM_EULER_ANGLES_YXZ,
        lua.create_function(|_, (x, y, z): (Option<f32>, Option<f32>, Option<f32>)| {
            let rot = rotation_yxz(x.unwrap_or(0.0), y.unwrap_or(0.0), z.unwrap_or(0.0));
            Ok(LuaCFrame(CFrame::new(Vec3::ZERO, rot)))
        })?,
    )?;
    lib.set(
        cframe::FROM_MATRIX,
        lua.create_function(|_, (pos, vx, vy, vz): (Value, Value, Value, Option<Value>)| {
            let v = |v: &Value| as_vector3(v).ok_or_else(|| err("CFrame.fromMatrix expects Vector3s"));
            let (x, y) = (v(&vx)?, v(&vy)?);
            let z = match vz {
                Some(z) => v(&z)?,
                None => x.cross(y),
            };
            let rot = rotation_rows([x.x, y.x, z.x, x.y, y.y, z.y, x.z, y.z, z.z]);
            Ok(LuaCFrame(CFrame::new(v(&pos)?, rot)))
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
    lib.set(
        "fromRotationBetweenVectors",
        lua.create_function(|_, (from, to): (Value, Value)| {
            let v = |v: &Value| {
                as_vector3(v).ok_or_else(|| err("CFrame.fromRotationBetweenVectors expects two Vector3s"))
            };
            Ok(LuaCFrame(CFrame::new(Vec3::ZERO, rotation_between(v(&from)?, v(&to)?))))
        })?,
    )?;
    lib.set(
        "fromEulerAngles",
        lua.create_function(
            |lua, (x, y, z, order): (Option<f32>, Option<f32>, Option<f32>, Option<Value>)| {
                // The order names the axes outermost first (XYZ is CFrame.Angles).
                let order = match order {
                    Some(Value::Nil) | None => "XYZ".to_owned(),
                    Some(v) => lua.globals().get::<mlua::Function>("tostring")?.call::<String>(v)?,
                };
                let order = order.rsplit('.').next().unwrap_or("XYZ");
                let order = if order.len() == 3 { order } else { "XYZ" };
                let (x, y, z) = (x.unwrap_or(0.0), y.unwrap_or(0.0), z.unwrap_or(0.0));
                let mut rot = Rotation::IDENTITY;
                for axis in order.chars() {
                    rot = rot
                        * match axis {
                            'X' => Rotation::about_x(x),
                            'Y' => Rotation::about_y(y),
                            _ => Rotation::about_z(z),
                        };
                }
                Ok(LuaCFrame(CFrame::new(Vec3::ZERO, rot)))
            },
        )?,
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
        fields.add_field_method_get(names::vector3::UNIT, |_, this| {
            let length = this.0.x.hypot(this.0.y);
            Ok(LuaVector2(if length > 0.0 {
                Vec2::new(this.0.x / length, this.0.y / length)
            } else {
                Vec2::ZERO
            }))
        });
        fields.add_meta_field(MetaMethod::Type, gui_values::VECTOR2_TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        use names::vector3::{ABS, ANGLE, CEIL, CROSS, DOT, FLOOR, FUZZY_EQ, LERP, MAX, MIN, SIGN};
        fn other(name: &str, value: &Value) -> mlua::Result<Vec2> {
            as_vector2(value).ok_or_else(|| mlua::Error::runtime(format!("{name} expects a Vector2")))
        }
        methods.add_method(DOT, |_, this, v: Value| {
            let v = other(DOT, &v)?;
            Ok(this.0.x * v.x + this.0.y * v.y)
        });
        methods.add_method(CROSS, |_, this, v: Value| {
            let v = other(CROSS, &v)?;
            Ok(this.0.x * v.y - this.0.y * v.x)
        });
        for name in [LERP, "lerp"] {
            methods.add_method(name, |_, this, (v, t): (Value, f32)| {
                let v = other(LERP, &v)?;
                Ok(LuaVector2(Vec2::new(this.0.x + (v.x - this.0.x) * t, this.0.y + (v.y - this.0.y) * t)))
            });
        }
        for (name, pick) in [(MIN, f32::min as fn(f32, f32) -> f32), (MAX, f32::max)] {
            methods.add_method(name, move |_, this, others: mlua::Variadic<Value>| {
                let mut out = this.0;
                for v in others.iter() {
                    let v = other(name, v)?;
                    out = Vec2::new(pick(out.x, v.x), pick(out.y, v.y));
                }
                Ok(LuaVector2(out))
            });
        }
        for (name, each) in [
            (ABS, f32::abs as fn(f32) -> f32),
            (FLOOR, f32::floor),
            (CEIL, f32::ceil),
            (SIGN, |v: f32| {
                if v > 0.0 {
                    1.0
                } else if v < 0.0 {
                    -1.0
                } else {
                    0.0
                }
            }),
        ] {
            methods.add_method(name, move |_, this, ()| {
                Ok(LuaVector2(Vec2::new(each(this.0.x), each(this.0.y))))
            });
        }
        methods.add_method(FUZZY_EQ, |_, this, (v, epsilon): (Value, Option<f32>)| {
            let v = other(FUZZY_EQ, &v)?;
            let eps = epsilon.unwrap_or(1e-5);
            Ok((this.0.x - v.x).abs() <= eps && (this.0.y - v.y).abs() <= eps)
        });
        methods.add_method(ANGLE, |_, this, (v, signed): (Value, Option<bool>)| {
            let v = other(ANGLE, &v)?;
            let cross = this.0.x * v.y - this.0.y * v.x;
            let dot = this.0.x * v.x + this.0.y * v.y;
            let angle = cross.atan2(dot);
            Ok(if signed.unwrap_or(false) { angle } else { angle.abs() })
        });
        methods.add_meta_function(MetaMethod::IDiv, |_, (a, b): (Value, Value)| {
            let (a, b) = vector2_operands(&a, &b)?;
            Ok(LuaVector2(Vec2::new((a.x / b.x).floor(), (a.y / b.y).floor())))
        });
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

/// One screen dimension, also each axis of a UDim2.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LuaUDim(pub UDim);

pub(crate) fn as_udim(value: &Value) -> Option<UDim> {
    match value {
        Value::UserData(ud) => ud.borrow::<LuaUDim>().ok().map(|v| v.0),
        _ => None,
    }
}

impl UserData for LuaUDim {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(gui_values::SCALE, |_, this| Ok(this.0.scale));
        fields.add_field_method_get(gui_values::OFFSET, |_, this| Ok(this.0.offset));
        fields.add_meta_field(MetaMethod::Type, gui_values::UDIM_TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{}, {}", this.0.scale, this.0.offset))
        });
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            Ok(matches!((as_udim(&a), as_udim(&b)), (Some(a), Some(b)) if a == b))
        });
        methods.add_meta_function(MetaMethod::Add, |_, (a, b): (Value, Value)| {
            match (as_udim(&a), as_udim(&b)) {
                (Some(a), Some(b)) => Ok(LuaUDim(UDim::new(a.scale + b.scale, a.offset + b.offset))),
                _ => Err(err("UDim arithmetic needs two UDims")),
            }
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
        let x = |u: UDim2| LuaUDim(UDim::new(u.x_scale, u.x_offset));
        let y = |u: UDim2| LuaUDim(UDim::new(u.y_scale, u.y_offset));
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
    vector2.set(names::vector3::X_AXIS, LuaVector2(Vec2::new(1.0, 0.0)))?;
    vector2.set(names::vector3::Y_AXIS, LuaVector2(Vec2::new(0.0, 1.0)))?;
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

    let udim = lua.create_table()?;
    udim.set(
        constructors::NEW,
        lua.create_function(|_, (scale, offset): (Option<f32>, Option<f32>)| {
            Ok(LuaUDim(UDim::new(scale.unwrap_or(0.0), offset.unwrap_or(0.0))))
        })?,
    )?;
    g.set(names::globals::UDIM, udim)?;
    Ok(())
}
