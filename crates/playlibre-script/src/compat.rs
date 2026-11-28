//! Older and less-used parts of the scripting API, so that scripts written
//! for other engines run: named brick colours, rays and the older ray
//! searches, dates, model helpers, enumeration lists, key bindings through
//! the action service, and services with little or nothing behind them yet
//! (purchases, teleports) that answer harmlessly instead of failing.

use mlua::{
    Function, Lua, MetaMethod, MultiValue, ObjectLike, Table, UserData, UserDataFields, UserDataMethods,
    Value,
};
use playlibre_instance::{
    CFrame, ClassKind, Color3, EnumKind, EnumValue, InstanceId, Property, PropertyValue, Rotation,
    UserInputState, Vec2, Vec3, geometry,
};

use crate::UserInput;
use crate::bindings::{
    LuaColor3, LuaEnumItem, LuaInstance, LuaVector3, as_enum, as_instance, as_number, as_vector3, class_of,
    clock, host, model_pivot_part, part_cframe, push_instance, require_class, set_part_cframe, tree_err,
};
use crate::names::{self, compat::*};
use crate::types::{LuaCFrame, LuaVector2, as_cframe};

fn runtime_err(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

// ---------------------------------------------------------------------------
// BrickColor
// ---------------------------------------------------------------------------

/// A named colour, by its place in [`BRICK_COLORS`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaBrickColor(pub(crate) usize);

/// The named colour at `i`, if there is one.
pub(crate) fn brick_color(i: usize) -> Option<LuaBrickColor> {
    (i < BRICK_COLORS.len()).then_some(LuaBrickColor(i))
}

fn brick_rgb(i: usize) -> Color3 {
    let [r, g, b] = BRICK_COLORS[i].2;
    Color3::from_rgb8(r, g, b)
}

fn default_brick() -> usize {
    BRICK_COLORS.iter().position(|b| b.0 == DEFAULT_BRICK_COLOR).unwrap_or(0)
}

/// The named colour nearest to `c`.
pub(crate) fn nearest_brick(c: Color3) -> usize {
    let distance = |i: usize| {
        let b = brick_rgb(i);
        let (dr, dg, db) = (b.r - c.r, b.g - c.g, b.b - c.b);
        dr * dr + dg * dg + db * db
    };
    (0..BRICK_COLORS.len()).min_by(|a, b| distance(*a).total_cmp(&distance(*b))).unwrap_or(0)
}

/// The colour of a script's BrickColor value.
pub(crate) fn brick_colour(value: &Value) -> Option<Color3> {
    as_brick(value).map(brick_rgb)
}

/// The named colour nearest `c`, as a script value.
pub(crate) fn brick_for(lua: &Lua, c: Color3) -> mlua::Result<Value> {
    lua.create_userdata(LuaBrickColor(nearest_brick(c))).map(Value::UserData)
}

fn as_brick(value: &Value) -> Option<usize> {
    match value {
        Value::UserData(ud) => ud.borrow::<LuaBrickColor>().ok().map(|b| b.0),
        _ => None,
    }
}

impl UserData for LuaBrickColor {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(NAME, |_, this| Ok(BRICK_COLORS[this.0].0));
        fields.add_field_method_get(NUMBER, |_, this| Ok(BRICK_COLORS[this.0].1));
        fields.add_field_method_get(COLOR, |_, this| Ok(LuaColor3(brick_rgb(this.0))));
        fields.add_field_method_get(R, |_, this| Ok(brick_rgb(this.0).r));
        fields.add_field_method_get(G, |_, this| Ok(brick_rgb(this.0).g));
        fields.add_field_method_get(B, |_, this| Ok(brick_rgb(this.0).b));
        fields.add_meta_field(MetaMethod::Type, BRICK_COLOR);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(BRICK_COLORS[this.0].0));
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            Ok(matches!((as_brick(&a), as_brick(&b)), (Some(a), Some(b)) if a == b))
        });
    }
}

fn brick_lib(lua: &Lua) -> mlua::Result<Table> {
    let lib = lua.create_table()?;
    lib.set(
        names::constructors::NEW,
        lua.create_function(|_, args: mlua::Variadic<Value>| {
            let found = match args.as_slice() {
                [] => default_brick(),
                [Value::String(s)] => {
                    let name = s.to_str()?;
                    BRICK_COLORS
                        .iter()
                        .position(|b| b.0.eq_ignore_ascii_case(&name))
                        .unwrap_or(default_brick())
                }
                [Value::UserData(ud)] => match ud.borrow::<LuaColor3>() {
                    Ok(c) => nearest_brick(c.0),
                    Err(_) => return Err(runtime_err("BrickColor.new expects a name, a number or a Color3")),
                },
                [n] if as_number(n).is_some() => {
                    let n = as_number(n).unwrap_or(0.0) as u16;
                    BRICK_COLORS.iter().position(|b| b.1 == n).unwrap_or(default_brick())
                }
                [r, g, b] => {
                    let n =
                        |v: &Value| as_number(v).ok_or_else(|| runtime_err("BrickColor.new expects numbers"));
                    nearest_brick(Color3::new(n(r)?, n(g)?, n(b)?))
                }
                _ => return Err(runtime_err("BrickColor.new expects a name, a number or a Color3")),
            };
            Ok(LuaBrickColor(found))
        })?,
    )?;
    let random = lua.create_function(|_, ()| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos() as usize);
        Ok(LuaBrickColor(nanos / 7 % BRICK_COLORS.len()))
    })?;
    lib.set(RANDOM, random.clone())?;
    // Both spellings are in use.
    lib.set("Random", random)?;
    lib.set(PALETTE, lua.create_function(|_, i: usize| Ok(LuaBrickColor(i.min(BRICK_COLORS.len() - 1))))?)?;
    for &(shortcut, name) in SHORTCUTS {
        let i = BRICK_COLORS.iter().position(|b| b.0 == name).unwrap_or(0);
        lib.set(shortcut, lua.create_function(move |_, ()| Ok(LuaBrickColor(i)))?)?;
    }
    lib.set_readonly(true);
    Ok(lib)
}

// ---------------------------------------------------------------------------
// Ray
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct LuaRay {
    origin: Vec3,
    direction: Vec3,
}

fn as_ray(value: &Value) -> Option<LuaRay> {
    match value {
        Value::UserData(ud) => ud.borrow::<LuaRay>().ok().map(|r| *r),
        _ => None,
    }
}

impl LuaRay {
    fn closest(&self, p: Vec3) -> Vec3 {
        let d = self.direction.normalize_or_zero();
        let t = (p - self.origin).dot(d).max(0.0);
        self.origin + d * t
    }
}

impl UserData for LuaRay {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(ORIGIN, |_, this| Ok(LuaVector3(this.origin)));
        fields.add_field_method_get(DIRECTION, |_, this| Ok(LuaVector3(this.direction)));
        fields.add_field_method_get(UNIT, |_, this| {
            Ok(LuaRay { origin: this.origin, direction: this.direction.normalize_or_zero() })
        });
        fields.add_meta_field(MetaMethod::Type, RAY);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(CLOSEST_POINT, |_, this, p: Value| {
            let p = as_vector3(&p).ok_or_else(|| runtime_err("ClosestPoint expects a Vector3"))?;
            Ok(LuaVector3(this.closest(p)))
        });
        methods.add_method(DISTANCE, |_, this, p: Value| {
            let p = as_vector3(&p).ok_or_else(|| runtime_err("Distance expects a Vector3"))?;
            Ok((p - this.closest(p)).length())
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            let (o, d) = (this.origin, this.direction);
            Ok(format!("{{{}, {}, {}}}, {{{}, {}, {}}}", o.x, o.y, o.z, d.x, d.y, d.z))
        });
    }
}

/// The older ray searches: the first part along `ray`, skipping (or, with
/// `only`, keeping just) the listed instances and what's inside them.
fn find_part_on_ray(lua: &Lua, ray: &Value, listed: Vec<InstanceId>, only: bool) -> mlua::Result<MultiValue> {
    let ray = as_ray(ray).ok_or_else(|| runtime_err("expected a Ray"))?;
    let hit = {
        let host = host(lua);
        let tree = host.tree.borrow();
        let inside = |part: InstanceId| {
            listed.iter().any(|f| *f == part || tree.is_descendant_of(part, *f).unwrap_or(false))
        };
        geometry::raycast_indexed(
            &tree,
            &crate::bindings::part_index(&host, &tree),
            ray.origin,
            ray.direction,
            |part| inside(part) == only && tree.get_bool(part, Property::CanQuery) != Some(false),
        )
        .map(|hit| {
            let material = match tree.voxels(hit.part) {
                Ok(v) if !v.is_empty() => {
                    let (x, y, z) = playlibre_instance::voxels::cell_at(hit.position - hit.normal * 0.5);
                    v.get(x, y, z).map(EnumValue::Material)
                }
                _ => tree.get_enum(hit.part, Property::Material),
            };
            (hit, material)
        })
    };
    let mut out = MultiValue::new();
    match hit {
        Some((hit, material)) => {
            out.push_back(Value::UserData(push_instance(lua, hit.part)?));
            out.push_back(crate::bindings::vector_value(hit.position));
            out.push_back(crate::bindings::vector_value(hit.normal));
            let material = material.unwrap_or(EnumValue::Material(playlibre_instance::Material::Plastic));
            out.push_back(Value::UserData(lua.create_userdata(LuaEnumItem(material))?));
        }
        None => {
            out.push_back(Value::Nil);
            out.push_back(crate::bindings::vector_value(ray.origin + ray.direction));
            out.push_back(crate::bindings::vector_value(Vec3::ZERO));
            let air = EnumValue::Material(playlibre_instance::Material::Air);
            out.push_back(Value::UserData(lua.create_userdata(LuaEnumItem(air))?));
        }
    }
    Ok(out)
}

fn instance_list(value: &Value) -> Vec<InstanceId> {
    match value {
        Value::Table(t) => {
            t.sequence_values::<Value>().filter_map(|v| v.ok().and_then(|v| as_instance(&v))).collect()
        }
        other => as_instance(other).into_iter().collect(),
    }
}

// ---------------------------------------------------------------------------
// DateTime
// ---------------------------------------------------------------------------

/// A moment, in milliseconds since 1970 (UTC).
#[derive(Debug, Clone, Copy)]
struct LuaDateTime(i64);

/// Year, month and day of a count of days since 1970.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

impl LuaDateTime {
    /// Year, month, day, hour, minute, second, millisecond.
    fn parts(self) -> (i64, u32, u32, i64, i64, i64, i64) {
        let ms = self.0.rem_euclid(1000);
        let secs = self.0.div_euclid(1000);
        let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
        let (y, mo, d) = civil(days);
        (y, mo, d, rest / 3600, rest / 60 % 60, rest % 60, ms)
    }
}

impl UserData for LuaDateTime {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(UNIX_TIMESTAMP, |_, this| Ok(this.0.div_euclid(1000)));
        fields.add_field_method_get(UNIX_TIMESTAMP_MILLIS, |_, this| Ok(this.0));
        fields.add_meta_field(MetaMethod::Type, DATE_TIME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(TO_ISO_DATE, |_, this, ()| {
            let (y, mo, d, h, mi, s, _) = this.parts();
            Ok(format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z"))
        });
        for name in [TO_UNIVERSAL_TIME, TO_LOCAL_TIME] {
            methods.add_method(name, |lua, this, ()| {
                let (y, mo, d, h, mi, s, ms) = this.parts();
                let t = lua.create_table()?;
                t.set(YEAR, y)?;
                t.set(MONTH, mo)?;
                t.set(DAY, d)?;
                t.set(HOUR, h)?;
                t.set(MINUTE, mi)?;
                t.set(SECOND, s)?;
                t.set(MILLISECOND, ms)?;
                Ok(t)
            });
        }
        // Local time is taken as universal time.
        for name in ["FormatUniversalTime", "FormatLocalTime"] {
            methods.add_method(name, |_, this, (format, _locale): (String, Option<String>)| {
                Ok(format_date(*this, &format))
            });
        }
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(this.0.to_string()));
    }
}

/// Days from 1970-01-01 to a date (the inverse of [`civil`]).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Formats a date with the usual tokens (YYYY, MMMM, DD, dddd, HH, hh, mm,
/// ss, A...) and the short forms LT, LTS, L, LL, LLL and LLLL, in English.
fn format_date(date: LuaDateTime, format: &str) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    const DAYS: [&str; 7] = ["Thursday", "Friday", "Saturday", "Sunday", "Monday", "Tuesday", "Wednesday"];
    let (y, mo, d, h, mi, s, ms) = date.parts();
    let weekday = DAYS[date.0.div_euclid(86_400_000).rem_euclid(7) as usize];
    let month = MONTHS[(mo as usize).saturating_sub(1).min(11)];
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    let ampm = if h < 12 { "AM" } else { "PM" };
    let expanded = |token: &str| -> Option<String> {
        Some(match token {
            "LTS" => format!("{h12}:{mi:02}:{s:02} {ampm}"),
            "LT" => format!("{h12}:{mi:02} {ampm}"),
            "LLLL" => format!("{weekday}, {month} {d}, {y} {h12}:{mi:02} {ampm}"),
            "LLL" => format!("{month} {d}, {y} {h12}:{mi:02} {ampm}"),
            "LL" => format!("{month} {d}, {y}"),
            "L" => format!("{mo:02}/{d:02}/{y:04}"),
            "YYYY" => format!("{y:04}"),
            "YY" => format!("{:02}", y.rem_euclid(100)),
            "MMMM" => month.to_owned(),
            "MMM" => month[..3].to_owned(),
            "MM" => format!("{mo:02}"),
            "M" => mo.to_string(),
            "dddd" => weekday.to_owned(),
            "ddd" => weekday[..3].to_owned(),
            "DD" => format!("{d:02}"),
            "D" => d.to_string(),
            "HH" => format!("{h:02}"),
            "H" => h.to_string(),
            "hh" => format!("{h12:02}"),
            "h" => h12.to_string(),
            "mm" => format!("{mi:02}"),
            "m" => mi.to_string(),
            "ss" => format!("{s:02}"),
            "s" => s.to_string(),
            "SSS" => format!("{ms:03}"),
            "A" => ampm.to_owned(),
            "a" => ampm.to_lowercase(),
            _ => return None,
        })
    };
    const TOKENS: [&str; 28] = [
        "LTS", "LT", "LLLL", "LLL", "LL", "L", "YYYY", "YY", "MMMM", "MMM", "MM", "M", "dddd", "ddd", "DD",
        "D", "HH", "H", "hh", "h", "mm", "m", "ss", "s", "SSS", "A", "a", "",
    ];
    let mut out = String::new();
    let mut rest = format;
    while let Some(c) = rest.chars().next() {
        // [text] is kept as it is.
        if c == '['
            && let Some(end) = rest.find(']')
        {
            out.push_str(&rest[1..end]);
            rest = &rest[end + 1..];
            continue;
        }
        match TOKENS.iter().filter(|t| !t.is_empty()).find(|t| rest.starts_with(**t)) {
            Some(token) => {
                out.push_str(&expanded(token).unwrap_or_default());
                rest = &rest[token.len()..];
            }
            None => {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

fn now_millis() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn date_lib(lua: &Lua) -> mlua::Result<Table> {
    let lib = lua.create_table()?;
    lib.set(NOW, lua.create_function(|_, ()| Ok(LuaDateTime(now_millis())))?)?;
    lib.set(FROM_UNIX_TIMESTAMP, lua.create_function(|_, s: f64| Ok(LuaDateTime((s * 1000.0) as i64)))?)?;
    lib.set(FROM_UNIX_TIMESTAMP_MILLIS, lua.create_function(|_, ms: f64| Ok(LuaDateTime(ms as i64)))?)?;
    for name in ["fromUniversalTime", "fromLocalTime"] {
        lib.set(
            name,
            lua.create_function(|_, parts: mlua::Variadic<Option<i64>>| {
                let part = |i: usize, default: i64| parts.get(i).copied().flatten().unwrap_or(default);
                let days = days_from_civil(part(0, 1970), part(1, 1), part(2, 1));
                let secs = days * 86_400 + part(3, 0) * 3600 + part(4, 0) * 60 + part(5, 0);
                Ok(LuaDateTime(secs * 1000 + part(6, 0)))
            })?,
        )?;
    }
    lib.set(
        "fromIsoDate",
        lua.create_function(|_, text: String| {
            let digits: Vec<i64> = text
                .split(|c: char| !c.is_ascii_digit())
                .filter(|p| !p.is_empty())
                .filter_map(|p| p.parse().ok())
                .collect();
            if digits.len() < 3 {
                return Ok(None);
            }
            let part = |i: usize| digits.get(i).copied().unwrap_or(0);
            let days = days_from_civil(part(0), part(1), part(2));
            Ok(Some(LuaDateTime((days * 86_400 + part(3) * 3600 + part(4) * 60 + part(5)) * 1000)))
        })?,
    )?;
    lib.set_readonly(true);
    Ok(lib)
}

// ---------------------------------------------------------------------------
// PhysicalProperties
// ---------------------------------------------------------------------------

/// What a part is made of (see [`playlibre_instance::physical`]).
#[derive(Debug, Clone, Copy)]
pub(crate) struct LuaPhysicalProperties([f32; 5]);

/// A part's `CustomPhysicalProperties`, or nil.
pub(crate) fn custom_physical(lua: &Lua, part: &Value) -> mlua::Result<Option<LuaPhysicalProperties>> {
    let Some(id) = crate::bindings::as_instance(part) else { return Ok(None) };
    let host = host(lua);
    let custom = playlibre_instance::physical::custom(&host.tree.borrow(), id);
    Ok(custom.map(|p| LuaPhysicalProperties(p.to_array())))
}

/// Sets a part's `CustomPhysicalProperties` (nil goes back to its material's).
pub(crate) fn set_custom_physical(lua: &Lua, part: &Value, value: &Value) -> mlua::Result<()> {
    let Some(id) = crate::bindings::as_instance(part) else { return Ok(()) };
    let physical = match value {
        Value::Nil => None,
        Value::UserData(ud) => match ud.borrow::<LuaPhysicalProperties>() {
            Ok(p) => Some(playlibre_instance::physical::Physical::from_array(p.0)),
            Err(_) => return Err(runtime_err("CustomPhysicalProperties expects PhysicalProperties or nil")),
        },
        _ => return Err(runtime_err("CustomPhysicalProperties expects PhysicalProperties or nil")),
    };
    playlibre_instance::physical::set_custom(&mut host(lua).tree.borrow_mut(), id, physical)
        .map_err(|e| runtime_err(e.to_string()))
}

impl UserData for LuaPhysicalProperties {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        for (i, name) in
            [DENSITY, FRICTION, ELASTICITY, FRICTION_WEIGHT, ELASTICITY_WEIGHT].into_iter().enumerate()
        {
            fields.add_field_method_get(name, move |_, this| Ok(this.0[i]));
        }
        fields.add_meta_field(MetaMethod::Type, PHYSICAL_PROPERTIES);
    }
}

fn physical_lib(lua: &Lua) -> mlua::Result<Table> {
    let lib = lua.create_table()?;
    lib.set(
        names::constructors::NEW,
        lua.create_function(|_, args: mlua::Variadic<Value>| {
            // A material's own, as PhysicalProperties.new(Enum.Material.Ice) asks.
            if let Some(EnumValue::Material(m)) = args
                .first()
                .filter(|v| matches!(v, Value::UserData(_)))
                .and_then(|v| as_enum(EnumKind::Material, v))
            {
                return Ok(LuaPhysicalProperties(
                    playlibre_instance::physical::Physical::of_material(m).to_array(),
                ));
            }
            // Plastic, unless given.
            let mut values = [0.7, 0.3, 0.5, 1.0, 1.0];
            for (slot, v) in values.iter_mut().zip(args.iter()) {
                if let Some(n) = as_number(v) {
                    *slot = n;
                }
            }
            Ok(LuaPhysicalProperties(values))
        })?,
    )?;
    lib.set_readonly(true);
    Ok(lib)
}

pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let g = lua.globals();
    g.set(BRICK_COLOR, brick_lib(lua)?)?;
    let ray = lua.create_table()?;
    ray.set(
        names::constructors::NEW,
        lua.create_function(|_, (origin, direction): (Value, Value)| {
            let v = |v: &Value| as_vector3(v).ok_or_else(|| runtime_err("Ray.new expects two Vector3s"));
            Ok(LuaRay { origin: v(&origin)?, direction: v(&direction)? })
        })?,
    )?;
    ray.set_readonly(true);
    g.set(RAY, ray)?;
    g.set(DATE_TIME, date_lib(lua)?)?;
    g.set(PHYSICAL_PROPERTIES, physical_lib(lua)?)?;
    Ok(())
}

/// Adds `GetEnumItems` to an enumeration's table.
pub(crate) fn add_enum_items(lua: &Lua, table: &Table, kind: EnumKind) -> mlua::Result<()> {
    table.set(
        GET_ENUM_ITEMS,
        lua.create_function(move |_, _: Value| {
            Ok(kind.items().iter().map(|i| LuaEnumItem(*i)).collect::<Vec<_>>())
        })?,
    )
}

/// `TweenService:GetValue(alpha, style, direction)`: how far along an eased
/// tween is when `alpha` of its time has passed.
pub(crate) fn tween_value(alpha: f32, style: &Value, direction: &Value) -> mlua::Result<f32> {
    let Some(EnumValue::EasingStyle(style)) = as_enum(EnumKind::EasingStyle, style) else {
        return Err(runtime_err("GetValue expects an EasingStyle"));
    };
    let Some(EnumValue::EasingDirection(direction)) = as_enum(EnumKind::EasingDirection, direction) else {
        return Err(runtime_err("GetValue expects an EasingDirection"));
    };
    Ok(style.ease(direction, alpha.clamp(0.0, 1.0)))
}

/// The state an input object reports.
pub(crate) fn input_state(began: bool) -> LuaEnumItem {
    LuaEnumItem(EnumValue::UserInputState(if began { UserInputState::Begin } else { UserInputState::End }))
}

// ---------------------------------------------------------------------------
// Members on instances
// ---------------------------------------------------------------------------

fn part_mass(lua: &Lua, id: InstanceId) -> f32 {
    playlibre_instance::physical::mass(&host(lua).tree.borrow(), id)
}

/// The parts welded to `part` (and to them), with `part` itself.
fn connected(lua: &Lua, part: InstanceId) -> Vec<InstanceId> {
    let host = host(lua);
    let tree = host.tree.borrow();
    let ws = tree.workspace();
    let mut links = Vec::new();
    let welds = [ClassKind::WeldConstraint, ClassKind::Weld, ClassKind::Motor6D]
        .into_iter()
        .flat_map(|c| tree.descendants_of_class(ws, c));
    for weld in welds {
        if tree.get_bool(weld, Property::Enabled) == Some(false) {
            continue;
        }
        if let (Some(a), Some(b)) = (tree.get_ref(weld, Property::Part0), tree.get_ref(weld, Property::Part1))
        {
            links.push((a, b));
        }
    }
    let mut found = vec![part];
    let mut i = 0;
    while i < found.len() {
        let at = found[i];
        for &(a, b) in &links {
            let other = if a == at {
                b
            } else if b == at {
                a
            } else {
                continue;
            };
            if !found.contains(&other) {
                found.push(other);
            }
        }
        i += 1;
    }
    found
}

/// The welds that hold `part`.
fn joints_of(lua: &Lua, part: InstanceId) -> Vec<InstanceId> {
    let host = host(lua);
    let tree = host.tree.borrow();
    [ClassKind::WeldConstraint, ClassKind::Weld, ClassKind::Motor6D]
        .into_iter()
        .flat_map(|c| tree.descendants_of_class(tree.workspace(), c))
        .filter(|w| {
            tree.get_ref(*w, Property::Part0) == Some(part) || tree.get_ref(*w, Property::Part1) == Some(part)
        })
        .collect()
}

/// The world box round everything in a model: its middle and size.
fn model_box(lua: &Lua, model: InstanceId) -> mlua::Result<Option<(Vec3, Vec3)>> {
    let parts = {
        let host = host(lua);
        let tree = host.tree.borrow();
        let all = tree.descendants(model).map_err(tree_err)?;
        all.into_iter().filter(|p| tree.class(*p).is_ok_and(|c| c.is_physical())).collect::<Vec<_>>()
    };
    let mut lo = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut hi = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
    for part in &parts {
        let cf = part_cframe(lua, *part)?;
        let half = host(lua).tree.borrow().get_vec3(*part, Property::Size).unwrap_or_default() / 2.0;
        for i in 0..8 {
            let corner = Vec3::new(
                if i & 1 == 0 { -half.x } else { half.x },
                if i & 2 == 0 { -half.y } else { half.y },
                if i & 4 == 0 { -half.z } else { half.z },
            );
            let p = cf.point_to_world(corner);
            lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
    }
    Ok((!parts.is_empty()).then(|| ((lo + hi) / 2.0, hi - lo)))
}

/// Moves every part in a model by `delta`.
fn move_model(lua: &Lua, model: InstanceId, delta: CFrame) -> mlua::Result<()> {
    let parts = {
        let host = host(lua);
        let tree = host.tree.borrow();
        let all = tree.descendants(model).map_err(tree_err)?;
        all.into_iter().filter(|p| tree.class(*p).is_ok_and(|c| c.is_physical())).collect::<Vec<_>>()
    };
    for part in parts {
        let cf = part_cframe(lua, part)?;
        set_part_cframe(lua, part, delta * cf)?;
    }
    Ok(())
}

/// `Model:MoveTo(position)`: moves the model so its pivot is there.
pub(crate) fn model_move_to(lua: &Lua, model: InstanceId, target: Value) -> mlua::Result<()> {
    let target = as_vector3(&target).ok_or_else(|| runtime_err("MoveTo expects a Vector3"))?;
    let pivot = model_pivot(lua, model)?;
    move_model(lua, model, CFrame::new(target - pivot.pos, Rotation::IDENTITY))
}

fn model_pivot(lua: &Lua, model: InstanceId) -> mlua::Result<CFrame> {
    match model_pivot_part(lua, model)? {
        Some(part) => part_cframe(lua, part),
        None => Ok(CFrame::IDENTITY),
    }
}

/// The two saved halves (position, orientation) of a joint's C0 or C1.
fn joint_halves(class: ClassKind, key: &str) -> Option<(Property, Property)> {
    if !class.has_property(Property::C0Position) {
        return None;
    }
    match key {
        names::joint::C0 => Some((Property::C0Position, Property::C0Orientation)),
        names::joint::C1 => Some((Property::C1Position, Property::C1Orientation)),
        _ => None,
    }
}

/// Members beyond the saved properties: `Some` when `key` is one of them.
/// An align orientation's or body gyro's `CFrame`: only its turn is kept.
fn is_target_turn(class: ClassKind, key: &str) -> bool {
    key == "CFrame" && class.has_property(Property::TargetOrientation)
}

pub(crate) fn index(lua: &Lua, id: InstanceId, class: ClassKind, key: &str) -> mlua::Result<Option<Value>> {
    if let Some(v) = crate::effects::gradient_index(lua, id, class, key)? {
        return Ok(Some(v));
    }
    if is_target_turn(class, key) {
        let turn = host(lua).tree.borrow().get_vec3(id, Property::TargetOrientation).unwrap_or_default();
        let cf = CFrame::new(Vec3::ZERO, Rotation::from_orientation(turn));
        return Ok(Some(Value::UserData(lua.create_userdata(LuaCFrame(cf))?)));
    }
    if let Some(halves) = joint_halves(class, key) {
        let host = host(lua);
        let tree = host.tree.borrow();
        let pos = tree.get_vec3(id, halves.0).unwrap_or_default();
        let rot = Rotation::from_orientation(tree.get_vec3(id, halves.1).unwrap_or_default());
        return Ok(Some(Value::UserData(lua.create_userdata(LuaCFrame(CFrame::new(pos, rot)))?)));
    }
    Ok(Some(match key {
        "Enabled" if crate::bindings::is_script_class(class) || class == ClassKind::ModuleScript => {
            Value::Boolean(host(lua).tree.borrow().get_bool(id, Property::Disabled) != Some(true))
        }
        BRICK_COLOR_PROPERTY if class.is_physical() => {
            let color = host(lua).tree.borrow().get_color(id, Property::Color).unwrap_or(Color3::WHITE);
            Value::UserData(lua.create_userdata(LuaBrickColor(nearest_brick(color)))?)
        }
        ASSEMBLY_MASS if class.is_physical() => {
            Value::Number(connected(lua, id).into_iter().map(|p| part_mass(lua, p)).sum::<f32>() as f64)
        }
        DISTRIBUTED_GAME_TIME if class == ClassKind::Workspace => Value::Number(clock(lua)),
        MOVE_DIRECTION if class == ClassKind::Humanoid => {
            let v = humanoid_velocity(lua, id);
            let flat = Vec3::new(v.x, 0.0, v.z);
            let dir = if flat.length() > 0.1 { flat.normalize_or_zero() } else { Vec3::ZERO };
            crate::bindings::vector_value(dir)
        }
        DISPLAY_NAME if class == ClassKind::Humanoid => {
            let host = host(lua);
            let tree = host.tree.borrow();
            let name = match tree.parent(id).map_err(tree_err)? {
                Some(model) => tree.name(model).map_err(tree_err)?.to_owned(),
                None => String::new(),
            };
            Value::String(lua.create_string(name)?)
        }
        _ => return Ok(None),
    }))
}

fn humanoid_velocity(lua: &Lua, humanoid: InstanceId) -> Vec3 {
    let root = {
        let host = host(lua);
        let tree = host.tree.borrow();
        tree.parent(humanoid)
            .ok()
            .flatten()
            .and_then(|m| tree.find_first_child(m, names::character::ROOT_PART).ok().flatten())
    };
    root.map_or(Vec3::ZERO, |r| crate::world::velocity_of(lua, r))
}

/// Sets members beyond the saved properties; true when `key` was one.
pub(crate) fn new_index(
    lua: &Lua,
    id: InstanceId,
    class: ClassKind,
    key: &str,
    value: &Value,
) -> mlua::Result<bool> {
    if crate::effects::gradient_new_index(lua, id, class, key, value)? {
        return Ok(true);
    }
    if is_target_turn(class, key) {
        let cf = as_cframe(value).ok_or_else(|| runtime_err(format!("{key} expects a CFrame")))?;
        let turn = PropertyValue::Vec3(cf.rot.to_orientation());
        host(lua).tree.borrow_mut().set(id, Property::TargetOrientation, turn).map_err(tree_err)?;
        return Ok(true);
    }
    if let Some(halves) = joint_halves(class, key) {
        let cf = as_cframe(value).ok_or_else(|| runtime_err(format!("{key} expects a CFrame")))?;
        let host = host(lua);
        let mut tree = host.tree.borrow_mut();
        tree.set(id, halves.0, PropertyValue::Vec3(cf.pos)).map_err(tree_err)?;
        tree.set(id, halves.1, PropertyValue::Vec3(cf.rot.to_orientation())).map_err(tree_err)?;
        return Ok(true);
    }
    match key {
        "Enabled" if crate::bindings::is_script_class(class) || class == ClassKind::ModuleScript => {
            let Value::Boolean(on) = value else {
                return Err(runtime_err(format!("Enabled expects a boolean, got a {}", value.type_name())));
            };
            if class != ClassKind::ModuleScript {
                host(lua)
                    .tree
                    .borrow_mut()
                    .set(id, Property::Disabled, PropertyValue::Bool(!on))
                    .map_err(tree_err)?;
                crate::bindings::script_toggled(lua, id);
            }
            Ok(true)
        }
        BRICK_COLOR_PROPERTY if class.is_physical() => {
            let brick = as_brick(value).ok_or_else(|| runtime_err("BrickColor expects a BrickColor"))?;
            let color = PropertyValue::Color3(brick_rgb(brick));
            host(lua).tree.borrow_mut().set(id, Property::Color, color).map_err(tree_err)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub(crate) fn add_instance_methods<M: UserDataMethods<LuaInstance>>(m: &mut M) {
    m.add_method(APPLY_ANGULAR_IMPULSE, |lua, this, impulse: Value| {
        if !class_of(lua, this.0)?.is_physical() {
            return Err(crate::bindings::not_a_member(lua, this.0, APPLY_ANGULAR_IMPULSE));
        }
        let impulse =
            as_vector3(&impulse).ok_or_else(|| runtime_err("ApplyAngularImpulse expects a Vector3"))?;
        // Roughly how hard it is to spin: a box's own turning weight.
        let size = host(lua).tree.borrow().get_vec3(this.0, Property::Size).unwrap_or(Vec3::ONE);
        let inertia = (part_mass(lua, this.0) * size.dot(size) / 18.0).max(0.01);
        let spin = crate::world::spin_of(lua, this.0) + impulse / inertia;
        crate::world::request_spin(lua, this.0, spin);
        Ok(())
    });
    // Ownership is always the one machine running the physics.
    m.add_method(SET_NETWORK_OWNER, |_, _, _: Value| Ok(()));
    m.add_method(SET_NETWORK_OWNERSHIP_AUTO, |_, _, ()| Ok(()));
    m.add_method(GET_NETWORK_OWNER, |_, _, ()| Ok(Value::Nil));
    m.add_method(CAN_SET_NETWORK_OWNERSHIP, |_, _, ()| Ok(true));
    m.add_method(GET_CONNECTED_PARTS, |lua, this, _recursive: Option<bool>| {
        let parts = connected(lua, this.0);
        parts
            .into_iter()
            .filter(|p| *p != this.0)
            .map(|p| push_instance(lua, p))
            .collect::<mlua::Result<Vec<_>>>()
    });
    m.add_method(GET_JOINTS, |lua, this, ()| {
        joints_of(lua, this.0).into_iter().map(|p| push_instance(lua, p)).collect::<mlua::Result<Vec<_>>>()
    });

    m.add_method(TRANSLATE_BY, |lua, this, by: Value| {
        require_class(lua, this.0, ClassKind::Model, TRANSLATE_BY)?;
        let by = as_vector3(&by).ok_or_else(|| runtime_err("TranslateBy expects a Vector3"))?;
        move_model(lua, this.0, CFrame::new(by, Rotation::IDENTITY))
    });
    m.add_method(SET_PRIMARY_PART_CFRAME, |lua, this, target: Value| {
        require_class(lua, this.0, ClassKind::Model, SET_PRIMARY_PART_CFRAME)?;
        let target =
            as_cframe(&target).ok_or_else(|| runtime_err("SetPrimaryPartCFrame expects a CFrame"))?;
        let pivot = model_pivot(lua, this.0)?;
        move_model(lua, this.0, target * pivot.inverse())
    });
    m.add_method(GET_PRIMARY_PART_CFRAME, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::Model, GET_PRIMARY_PART_CFRAME)?;
        Ok(LuaCFrame(model_pivot(lua, this.0)?))
    });
    m.add_method(GET_BOUNDING_BOX, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::Model, GET_BOUNDING_BOX)?;
        let (middle, size) = model_box(lua, this.0)?.unwrap_or((Vec3::ZERO, Vec3::ZERO));
        Ok((LuaCFrame(CFrame::new(middle, Rotation::IDENTITY)), LuaVector3(size)))
    });
    m.add_method(GET_EXTENTS_SIZE, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::Model, GET_EXTENTS_SIZE)?;
        Ok(LuaVector3(model_box(lua, this.0)?.map_or(Vec3::ZERO, |b| b.1)))
    });

    m.add_method(FIND_PART_ON_RAY, |lua, this, (ray, ignore): (Value, Option<Value>)| {
        require_class(lua, this.0, ClassKind::Workspace, FIND_PART_ON_RAY)?;
        find_part_on_ray(lua, &ray, ignore.as_ref().map(instance_list).unwrap_or_default(), false)
    });
    m.add_method(FIND_PART_ON_RAY_WITH_IGNORE_LIST, |lua, this, (ray, ignore): (Value, Option<Value>)| {
        require_class(lua, this.0, ClassKind::Workspace, FIND_PART_ON_RAY_WITH_IGNORE_LIST)?;
        find_part_on_ray(lua, &ray, ignore.as_ref().map(instance_list).unwrap_or_default(), false)
    });
    m.add_method(FIND_PART_ON_RAY_WITH_WHITELIST, |lua, this, (ray, keep): (Value, Option<Value>)| {
        require_class(lua, this.0, ClassKind::Workspace, FIND_PART_ON_RAY_WITH_WHITELIST)?;
        find_part_on_ray(lua, &ray, keep.as_ref().map(instance_list).unwrap_or_default(), true)
    });
    m.add_method(GET_SERVER_TIME_NOW, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::Workspace, GET_SERVER_TIME_NOW)?;
        Ok(now_millis() as f64 / 1000.0)
    });

    m.add_method(GET_MINUTES_AFTER_MIDNIGHT, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::Lighting, GET_MINUTES_AFTER_MIDNIGHT)?;
        Ok(host(lua).tree.borrow().get_number(this.0, Property::ClockTime).unwrap_or(14.0) * 60.0)
    });
    m.add_method(SET_MINUTES_AFTER_MIDNIGHT, |lua, this, minutes: f64| {
        require_class(lua, this.0, ClassKind::Lighting, SET_MINUTES_AFTER_MIDNIGHT)?;
        let hours = (minutes / 60.0).rem_euclid(24.0);
        host(lua)
            .tree
            .borrow_mut()
            .set(this.0, Property::ClockTime, PropertyValue::Number(hours))
            .map_err(tree_err)
    });

    m.add_method(BIND_TO_CLOSE, |lua, this, f: Function| {
        require_class(lua, this.0, ClassKind::DataModel, BIND_TO_CLOSE)?;
        // Run when the game shuts down (when you stop playing), so games can save.
        let list = match lua.named_registry_value::<Value>(crate::bindings::CLOSE_KEY)? {
            Value::Table(t) => t,
            _ => {
                let t = lua.create_table()?;
                lua.set_named_registry_value(crate::bindings::CLOSE_KEY, t.clone())?;
                t
            }
        };
        list.push(f)
    });
    m.add_method(IS_LOADED, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::DataModel, IS_LOADED)?;
        Ok(true)
    });
}

// ---------------------------------------------------------------------------
// Services
// ---------------------------------------------------------------------------

const SERVICES_KEY: &str = "playlibre.compat.services";
const ACTIONS_KEY: &str = "playlibre.compat.actions";
const ACTIONS_ORDER_KEY: &str = "playlibre.compat.actions.order";
// Fields of a bound action besides its handler (1) and keys (2).
const ACTION_TOUCH_BUTTON: &str = "touch_button";
const ACTION_ORDER: &str = "order";
const ACTION_TITLE: &str = "title";
const ACTION_IMAGE: &str = "image";
const ACTION_DESCRIPTION: &str = "description";
const ACTION_POSITION: &str = "position";
const ACTION_PRIORITY: &str = "priority";
/// `Enum.ContextActionPriority.Default`.
const ACTION_DEFAULT_PRIORITY: f64 = 2000.0;

/// A service table (made once, so every `GetService` gets the same one).
pub(crate) fn get_service(lua: &Lua, name: &str) -> mlua::Result<Option<Value>> {
    let known = [
        MARKETPLACE_SERVICE,
        BADGE_SERVICE,
        TELEPORT_SERVICE,
        SOUND_SERVICE,
        PHYSICS_SERVICE,
        GUI_SERVICE,
        TEXT_SERVICE,
        CONTENT_PROVIDER,
        VR_SERVICE,
        HAPTIC_SERVICE,
        POLICY_SERVICE,
        MESSAGING_SERVICE,
        CONTEXT_ACTION_SERVICE,
    ];
    let Some(&name) = known.iter().find(|k| **k == name) else { return Ok(None) };
    let cache: Table = match lua.named_registry_value::<Value>(SERVICES_KEY)? {
        Value::Table(t) => t,
        _ => {
            let t = lua.create_table()?;
            lua.set_named_registry_value(SERVICES_KEY, t.clone())?;
            t
        }
    };
    if let Value::Table(t) = cache.get::<Value>(name)? {
        return Ok(Some(Value::Table(t)));
    }
    let service = make_service(lua, name)?;
    cache.set(name, service.clone())?;
    Ok(Some(Value::Table(service)))
}

fn method<A, R>(
    lua: &Lua,
    t: &Table,
    name: &str,
    f: impl Fn(&Lua, A) -> mlua::Result<R> + 'static,
) -> mlua::Result<()>
where
    A: mlua::FromLuaMulti,
    R: mlua::IntoLuaMulti,
{
    t.set(name, lua.create_function(move |lua, (_service, args): (Value, A)| f(lua, args))?)
}

fn warn(lua: &Lua, text: String) {
    host(lua).output.borrow_mut().push(crate::OutputLine { level: crate::OutputLevel::Warn, text });
}

fn badge_id(value: &Value) -> Option<String> {
    match value {
        Value::Integer(n) => Some(n.to_string()),
        Value::Number(n) if n.fract() == 0.0 => Some((*n as i64).to_string()),
        Value::String(s) => s.to_str().ok().map(|s| s.to_owned()).filter(|s| !s.is_empty() && s.len() <= 64),
        _ => None,
    }
}

// A Luau integer is an i32 on 32-bit processors and an i64 elsewhere.
#[allow(clippy::useless_conversion)]
fn whole_id(value: &Value) -> Option<i64> {
    match value {
        Value::Integer(n) => Some(i64::from(*n)),
        Value::Number(n) if n.fract() == 0.0 => Some(*n as i64),
        _ => None,
    }
    .filter(|n| *n > 0)
}

fn badge_service(lua: &Lua, t: &Table) -> mlua::Result<()> {
    for award in [AWARD_BADGE, "AwardBadgeAsync"] {
        method(lua, t, award, |lua, (user, badge): (Value, Value)| {
            let (Some(user), Some(badge)) = (whole_id(&user), badge_id(&badge)) else { return Ok(false) };
            Ok(award_badge(lua, user, &badge))
        })?;
    }
    for has in [USER_HAS_BADGE_ASYNC, "UserHasBadge"] {
        method(lua, t, has, |lua, (user, badge): (Value, Value)| {
            let (Some(user), Some(badge)) = (whole_id(&user), badge_id(&badge)) else { return Ok(false) };
            Ok(has_badge(lua, user, &badge))
        })?;
    }
    method(lua, t, CHECK_USER_BADGES_ASYNC, |lua, (user, badges): (Value, Option<Table>)| {
        let out = lua.create_table()?;
        let (Some(user), Some(badges)) = (whole_id(&user), badges) else { return Ok(out) };
        for badge in badges.sequence_values::<Value>() {
            let badge = badge?;
            if badge_id(&badge).is_some_and(|id| has_badge(lua, user, &id)) {
                out.raw_push(badge)?;
            }
        }
        Ok(out)
    })?;
    method(lua, t, GET_BADGE_INFO_ASYNC, |lua, badge: Value| {
        let id = badge_id(&badge).unwrap_or_default();
        let host = host(lua);
        let world = host.world.borrow();
        let found = world.badges.info.iter().find(|b| b.id == id);
        let info = lua.create_table()?;
        match found {
            Some(b) => {
                info.set(NAME, b.name.as_str())?;
                info.set("Description", b.description.as_str())?;
            }
            None => {
                info.set(NAME, format!("Badge {id}"))?;
                info.set("Description", "")?;
            }
        }
        info.set("IconImageId", 0)?;
        info.set("IsEnabled", true)?;
        info.set("Id", badge)?;
        Ok(info)
    })
}

/// TeleportService: teleports wait for the host (or, on a player's own
/// computer, for the app), which sends each player on through the Hub.
fn teleport_service(lua: &Lua, t: &Table) -> mlua::Result<()> {
    let queue =
        lua.create_function(|lua, (user, place, code, data): (f64, f64, Option<String>, Value)| {
            let data = match data {
                Value::Nil => None,
                other => Some(crate::services::to_json(&other, 0)?),
            };
            if data.as_ref().is_some_and(|d| d.to_string().len() > 64 * 1024) {
                return Err(mlua::Error::runtime("teleport data can be at most 64 KB"));
            }
            let host = host(lua);
            host.world.borrow_mut().teleports.queued.push(crate::world::Teleport {
                user_id: user as i64,
                place_id: place as i64,
                access_code: code.filter(|c| !c.is_empty()),
                data,
            });
            Ok(())
        })?;
    let reserve = lua.create_function(|_, ()| {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| mlua::Error::runtime(e.to_string()))?;
        Ok(bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
    })?;
    let local = lua.create_function(|lua, ()| {
        let data = host(lua).world.borrow().teleports.local.data.clone();
        data.map_or(Ok(Value::Nil), |d| crate::services::from_json(lua, &d))
    })?;
    let methods: Table = lua
        .load(
            r#"local queue, reserve, localData = ...
            local function each(players)
                if typeof(players) == "Instance" then
                    return { players }
                end
                return players or {}
            end
            local function send(name, placeId, players, code, data)
                if type(placeId) ~= "number" then
                    error(name .. " needs a place id", 3)
                end
                for _, player in each(players) do
                    queue(player.UserId, placeId, code, data)
                end
            end
            local function field(options, name)
                local ok, value = pcall(function()
                    return options[name]
                end)
                return ok and value or nil
            end
            return {
                Teleport = function(_, placeId, player, data)
                    send("Teleport", placeId, player or game:GetService("Players").LocalPlayer, nil, data)
                end,
                TeleportAsync = function(_, placeId, players, options)
                    local code, data
                    if options ~= nil then
                        code = field(options, "ReservedServerAccessCode")
                        if code == "" then
                            code = nil
                        end
                        if field(options, "ShouldReserveServer") then
                            code = reserve()
                        end
                        local ok, d = pcall(function()
                            return options:GetTeleportData()
                        end)
                        if ok then
                            data = d
                        end
                    end
                    send("TeleportAsync", placeId, players, code, data)
                    return { PrivateServerId = code or "", ReservedServerAccessCode = code or "" }
                end,
                TeleportToPrivateServer = function(_, placeId, code, players, _spawnName, data)
                    send("TeleportToPrivateServer", placeId, players, code, data)
                end,
                TeleportToPlaceInstance = function(_, placeId, _jobId, player, _spawnName, data)
                    send("TeleportToPlaceInstance", placeId, player or game:GetService("Players").LocalPlayer, nil, data)
                end,
                TeleportPartyAsync = function(_, placeId, players, data)
                    send("TeleportPartyAsync", placeId, players, nil, data)
                    return ""
                end,
                ReserveServer = function()
                    local code = reserve()
                    return code, code
                end,
                ReserveServerAsync = function()
                    local code = reserve()
                    return code, code
                end,
                GetLocalPlayerTeleportData = function()
                    return localData()
                end,
            }"#,
        )
        .set_name("=TeleportService")
        .call((queue, reserve, local))?;
    for pair in methods.pairs::<String, Function>() {
        let (name, f) = pair?;
        t.set(name, f)?;
    }
    Ok(())
}

fn make_service(lua: &Lua, name: &'static str) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set(NAME, name)?;
    t.set(names::instance::CLASS_NAME, name)?;
    match name {
        MARKETPLACE_SERVICE => {
            for event in [
                "PromptGamePassPurchaseFinished",
                "PromptProductPurchaseFinished",
                "PromptPurchaseFinished",
                "PromptBundlePurchaseFinished",
                "PromptSubscriptionPurchaseFinished",
            ] {
                t.set(event, crate::lenient::new_signal(lua)?)?;
            }
            // Nothing can be bought in PlayLibre. As when a player
            // closes the prompt, its Finished event says it wasn't bought,
            // so shops that wait for the answer don't get stuck.
            for (prompt, event, by_user_id) in [
                (PROMPT_PURCHASE, "PromptPurchaseFinished", false),
                (PROMPT_GAME_PASS_PURCHASE, "PromptGamePassPurchaseFinished", false),
                (PROMPT_PRODUCT_PURCHASE, "PromptProductPurchaseFinished", true),
                ("PromptBundlePurchase", "PromptBundlePurchaseFinished", false),
                ("PromptSubscriptionPurchase", "PromptSubscriptionPurchaseFinished", false),
            ] {
                let signal: Value = t.get(event)?;
                method(lua, &t, prompt, move |lua, (player, id): (Value, Value)| {
                    // Passes of a game on the Hub: the host asks the player.
                    if prompt == PROMPT_GAME_PASS_PURCHASE && host(lua).world.borrow().passes.on {
                        let user = match &player {
                            Value::UserData(ud) => {
                                mlua::ObjectLike::get::<Option<f64>>(ud, "UserId").ok().flatten()
                            }
                            _ => None,
                        };
                        let pass = whole_id(&id);
                        let (Some(user), Some(pass)) = (user, pass) else {
                            return Err(mlua::Error::runtime(
                                "PromptGamePassPurchase needs a player and a game pass id",
                            ));
                        };
                        let user = user as i64;
                        let host = host(lua);
                        let mut world = host.world.borrow_mut();
                        let p = &mut world.passes;
                        let sold = p.info.iter().any(|i| i.id == pass);
                        if sold && !p.owned.contains(&(user, pass)) {
                            if !p.prompts.contains(&(user, pass)) {
                                p.prompts.push((user, pass));
                            }
                            return Ok(());
                        }
                    } else if prompt == PROMPT_PRODUCT_PURCHASE && host(lua).world.borrow().products.on {
                        let user = match &player {
                            Value::UserData(ud) => {
                                mlua::ObjectLike::get::<Option<f64>>(ud, "UserId").ok().flatten()
                            }
                            _ => None,
                        };
                        let product = whole_id(&id);
                        let (Some(user), Some(product)) = (user, product) else {
                            return Err(mlua::Error::runtime(
                                "PromptProductPurchase needs a player and a product id",
                            ));
                        };
                        let user = user as i64;
                        let host = host(lua);
                        let mut world = host.world.borrow_mut();
                        let p = &mut world.products;
                        if p.info.iter().any(|i| i.id == product) {
                            if !p.prompts.contains(&(user, product)) {
                                p.prompts.push((user, product));
                            }
                            return Ok(());
                        }
                    } else {
                        warn(lua, format!("{MARKETPLACE_SERVICE}.{prompt} {NOT_AVAILABLE}"));
                    }
                    let who = if by_user_id {
                        match &player {
                            Value::UserData(ud) => {
                                mlua::ObjectLike::get::<Value>(ud, "UserId").unwrap_or(Value::Nil)
                            }
                            other => other.clone(),
                        }
                    } else {
                        player
                    };
                    let defer: Function = lua
                        .load("local signal, who, id = ... task.defer(function() signal:Fire(who, id, false) end)")
                        .set_name("=MarketplaceService")
                        .into_function()?;
                    defer.call::<()>((signal.clone(), who, id))
                })?;
            }
            // Waits (as the real call does) until the host has looked up
            // the player's passes, for at most ten seconds.
            let known = lua.create_function(|lua, user: f64| {
                let host = host(lua);
                let world = host.world.borrow();
                Ok(!world.passes.on || world.passes.known.contains(&(user as i64)))
            })?;
            let has = lua.create_function(|lua, (user, pass): (f64, f64)| {
                Ok(host(lua).world.borrow().passes.owned.contains(&(user as i64, pass as i64)))
            })?;
            let owns_pass: Function = lua
                .load(
                    "local known, has = ...
                     return function(_, userId, passId)
                         if type(userId) ~= \"number\" or type(passId) ~= \"number\" then
                             error(\"UserOwnsGamePassAsync needs a user id and a game pass id\", 2)
                         end
                         local waited = 0
                         while not known(userId) and waited < 10 do
                             waited += task.wait(0.1)
                         end
                         return has(userId, passId)
                     end",
                )
                .set_name("=MarketplaceService")
                .call((known, has))?;
            t.set(USER_OWNS_GAME_PASS_ASYNC, owns_pass.clone())?;
            t.set("UserOwnsGamePass", owns_pass)?;
            for owns in
                [PLAYER_OWNS_ASSET, "PlayerOwnsAssetAsync", "PlayerOwnsBundle", "PlayerOwnsBundleAsync"]
            {
                method(lua, &t, owns, |_, _: MultiValue| Ok(false))?;
            }
            for info in [GET_PRODUCT_INFO, "GetProductInfoAsync"] {
                method(lua, &t, info, |lua, (id, kind): (Option<f64>, Value)| {
                    product_info(lua, id.unwrap_or(0.0), &kind)
                })?;
            }
        }
        BADGE_SERVICE => badge_service(lua, &t)?,
        TELEPORT_SERVICE => teleport_service(lua, &t)?,
        SOUND_SERVICE => {
            method(lua, &t, PLAY_LOCAL_SOUND, |_, sound: Value| {
                if let Value::UserData(ud) = sound {
                    ud.call_method::<()>(names::sound::PLAY, ())?;
                }
                Ok(())
            })?;
        }
        PHYSICS_SERVICE => physics_service(lua, &t)?,
        GUI_SERVICE => {
            method(lua, &t, IS_TEN_FOOT_INTERFACE, |_, ()| Ok(false))?;
            method(lua, &t, GET_GUI_INSET, |_, ()| {
                Ok((LuaVector2(Vec2::new(0.0, crate::GUI_INSET)), LuaVector2(Vec2::new(0.0, 0.0))))
            })?;
        }
        TEXT_SERVICE => {
            // GetTextSize(text, size, font, frameSize): wrapped at the frame's width.
            method(
                lua,
                &t,
                GET_TEXT_SIZE,
                |_, (text, size, _font, frame): (String, Option<f32>, Value, Value)| {
                    let size = size.unwrap_or(14.0).max(1.0);
                    let width = crate::types::as_vector2(&frame).map(|f| f.x).unwrap_or(f32::INFINITY);
                    let (w, h) = crate::measure_text(&text, size, width);
                    Ok(LuaVector2(Vec2::new(w.ceil(), h.ceil())))
                },
            )?;
            // The text, filtered once; every way of asking for it gives the same.
            method(lua, &t, FILTER_STRING_ASYNC, |lua, (text, _from): (String, MultiValue)| {
                let result = lua.create_table()?;
                result.set("ClassName", "TextFilterResult")?;
                let clean = crate::filter::filter(&text);
                for name in [
                    GET_NON_CHAT_STRING_FOR_BROADCAST_ASYNC,
                    GET_NON_CHAT_STRING_FOR_USER_ASYNC,
                    GET_CHAT_FOR_USER_ASYNC,
                ] {
                    let clean = clean.clone();
                    result.set(name, lua.create_function(move |_, _: MultiValue| Ok(clean.clone()))?)?;
                }
                Ok(result)
            })?;
        }
        CONTENT_PROVIDER => {
            method(lua, &t, PRELOAD_ASYNC, |_, _: MultiValue| Ok(()))?;
        }
        VR_SERVICE => {
            t.set(VR_ENABLED, false)?;
        }
        HAPTIC_SERVICE => {
            // Phones vibrate for the Touch input type; see the lenient layer's HapticService.
            method(lua, &t, IS_VIBRATION_SUPPORTED, |lua, kind: Value| {
                let touch = matches!(
                    crate::bindings::as_enum(EnumKind::UserInputType, &kind),
                    Some(EnumValue::UserInputType(playlibre_instance::UserInputType::Touch))
                );
                Ok(touch && host(lua).world.borrow().devices.vibration)
            })?;
        }
        POLICY_SERVICE => {
            method(lua, &t, GET_POLICY_INFO_FOR_PLAYER_ASYNC, |lua, _: MultiValue| {
                let info = lua.create_table()?;
                info.set(ARE_ADS_ALLOWED, false)?;
                // PlayLibre shows no links out of the game, trading or paid
                // random items, so every such policy is the strict one.
                info.set("AllowedExternalLinkReferences", lua.create_table()?)?;
                info.set("ArePaidRandomItemsRestricted", true)?;
                info.set("IsPaidItemTradingAllowed", false)?;
                info.set("IsSubjectToChinaPolicies", false)?;
                info.set("IsContentSharingAllowed", false)?;
                info.set("IsEligibleToPurchaseSubscription", false)?;
                Ok(info)
            })?;
        }
        MESSAGING_SERVICE => messaging(lua, &t)?,
        CONTEXT_ACTION_SERVICE => actions(lua, &t)?,
        _ => {}
    }
    crate::lenient::dress_service(lua, &t, name)?;
    Ok(t)
}

/// The collision groups scripts made, and which pairs pass through each other.
#[derive(Debug, Default)]
pub(crate) struct CollisionGroups {
    registered: Vec<String>,
    apart: std::collections::BTreeSet<(String, String)>,
    /// The workspace's `CollisionGroups` text these were last read from or
    /// written to.
    seen: Option<String>,
}

const GROUPS_EXTRA: &str = "CollisionGroups";

/// Keeps the groups in step with the workspace's `CollisionGroups` text:
/// entries of `name^number^mask` separated by `\` (bit `n` of the mask
/// set: the group collides with group `n`). The file reader puts the
/// place's own groups there, and the text is sent to players' computers
/// with the workspace, so theirs match the server's.
fn seed_collision_groups(lua: &Lua) {
    let host = host(lua);
    // Read again later if the tree is busy right now.
    let Ok(tree) = host.tree.try_borrow() else { return };
    let text = match tree.get_extra(tree.workspace(), GROUPS_EXTRA) {
        Ok(Some(PropertyValue::String(text))) => text,
        _ => String::new(),
    };
    drop(tree);
    let mut groups = host.groups.borrow_mut();
    if groups.seen.as_deref() == Some(text.as_str()) {
        return;
    }
    groups.registered.clear();
    groups.apart.clear();
    let mut list: Vec<(String, u32, u32)> = Vec::new();
    for entry in text.split('\\').filter(|e| !e.is_empty()) {
        let mut parts = entry.rsplitn(3, '^');
        let (Some(mask), Some(id), Some(name)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let (Ok(mask), Ok(id)) = (mask.trim().parse::<i64>(), id.trim().parse::<u32>()) else { continue };
        list.push((name.to_owned(), id, mask as u32));
    }
    let collides = |mask: u32, id: u32| id < 32 && mask & (1 << id) != 0;
    for (name, _, _) in &list {
        if name != playlibre_instance::DEFAULT_COLLISION_GROUP && !groups.registered.contains(name) {
            groups.registered.push(name.clone());
        }
    }
    for (a, a_id, a_mask) in &list {
        for (b, b_id, b_mask) in &list {
            if !collides(*a_mask, *b_id) || !collides(*b_mask, *a_id) {
                groups.apart.insert(pair(a.clone(), b.clone()));
            }
        }
    }
    groups.seen = Some(text);
}

/// Writes the groups back as the workspace's `CollisionGroups` text.
fn store_collision_groups(lua: &Lua) {
    let host = host(lua);
    let text = {
        let groups = host.groups.borrow();
        let names: Vec<&str> = std::iter::once(playlibre_instance::DEFAULT_COLLISION_GROUP)
            .chain(groups.registered.iter().map(String::as_str))
            .take(32)
            .collect();
        let mut text = String::new();
        for (i, a) in names.iter().enumerate() {
            let mut mask: u32 = 0;
            for (j, b) in names.iter().enumerate() {
                if !groups.apart.contains(&pair((*a).to_owned(), (*b).to_owned())) {
                    mask |= 1 << j;
                }
            }
            if i > 0 {
                text.push('\\');
            }
            text.push_str(&format!("{a}^{i}^{}", mask as i32));
        }
        text
    };
    let Ok(mut tree) = host.tree.try_borrow_mut() else { return };
    let ws = tree.workspace();
    let _ = tree.set_extra(ws, GROUPS_EXTRA, Some(PropertyValue::String(text.clone())));
    host.groups.borrow_mut().seen = Some(text);
}

fn pair(a: String, b: String) -> (String, String) {
    if a <= b { (a, b) } else { (b, a) }
}

pub(crate) fn collision_rules(lua: &Lua) -> Vec<(String, String)> {
    seed_collision_groups(lua);
    host(lua).groups.borrow().apart.iter().cloned().collect()
}

fn physics_service(lua: &Lua, t: &Table) -> mlua::Result<()> {
    let register = |lua: &Lua, name: String| {
        seed_collision_groups(lua);
        let host = host(lua);
        let mut groups = host.groups.borrow_mut();
        if name != playlibre_instance::DEFAULT_COLLISION_GROUP && !groups.registered.contains(&name) {
            groups.registered.push(name);
            drop(groups);
            store_collision_groups(lua);
        }
        Ok(())
    };
    method(lua, t, REGISTER_COLLISION_GROUP, register)?;
    method(lua, t, CREATE_COLLISION_GROUP, move |lua, name: String| register(lua, name).map(|()| 0))?;
    method(lua, t, UNREGISTER_COLLISION_GROUP, |lua, name: String| {
        seed_collision_groups(lua);
        let host = host(lua);
        let mut groups = host.groups.borrow_mut();
        groups.registered.retain(|g| *g != name);
        groups.apart.retain(|(a, b)| *a != name && *b != name);
        drop(groups);
        store_collision_groups(lua);
        Ok(())
    })?;
    method(lua, t, IS_COLLISION_GROUP_REGISTERED, |lua, name: String| {
        seed_collision_groups(lua);
        Ok(name == playlibre_instance::DEFAULT_COLLISION_GROUP
            || host(lua).groups.borrow().registered.contains(&name))
    })?;
    method(lua, t, COLLISION_GROUP_SET_COLLIDABLE, |lua, (a, b, collide): (String, String, bool)| {
        seed_collision_groups(lua);
        let host = host(lua);
        let mut groups = host.groups.borrow_mut();
        if collide {
            groups.apart.remove(&pair(a, b));
        } else {
            groups.apart.insert(pair(a, b));
        }
        drop(groups);
        store_collision_groups(lua);
        Ok(())
    })?;
    method(lua, t, COLLISION_GROUPS_ARE_COLLIDABLE, |lua, (a, b): (String, String)| {
        seed_collision_groups(lua);
        Ok(!host(lua).groups.borrow().apart.contains(&pair(a, b)))
    })?;
    method(lua, t, GET_REGISTERED_COLLISION_GROUPS, |lua, ()| {
        seed_collision_groups(lua);
        let names: Vec<String> = std::iter::once(playlibre_instance::DEFAULT_COLLISION_GROUP.to_owned())
            .chain(host(lua).groups.borrow().registered.iter().cloned())
            .collect();
        let list = lua.create_table()?;
        for name in names {
            let entry = lua.create_table()?;
            entry.set("name", name.as_str())?;
            entry.set(NAME, name)?;
            list.raw_push(entry)?;
        }
        Ok(list)
    })?;
    method(lua, t, SET_PART_COLLISION_GROUP, |lua, (part, group): (Value, String)| {
        seed_collision_groups(lua);
        let part = as_instance(&part).ok_or_else(|| runtime_err("SetPartCollisionGroup expects a part"))?;
        let value = PropertyValue::String(group);
        host(lua).tree.borrow_mut().set(part, Property::CollisionGroup, value).map_err(tree_err)
    })
}

/// Messages go to this game's own subscribers (there's one server).
fn messaging(lua: &Lua, t: &Table) -> mlua::Result<()> {
    let topics = lua.create_table()?;
    let subscribe_topics = topics.clone();
    method(lua, t, SUBSCRIBE_ASYNC, move |lua, (topic, f): (String, Function)| {
        let list: Table = match subscribe_topics.get::<Value>(topic.as_str())? {
            Value::Table(l) => l,
            _ => {
                let l = lua.create_table()?;
                subscribe_topics.set(topic.as_str(), l.clone())?;
                l
            }
        };
        list.raw_push(f.clone())?;
        let connection = lua.create_table()?;
        let (list2, f2) = (list.clone(), f);
        connection.set(
            DISCONNECT,
            lua.create_function(move |_, _: Value| {
                let len = list2.raw_len();
                for i in 1..=len {
                    if list2.raw_get::<Value>(i)? == Value::Function(f2.clone()) {
                        list2.raw_remove(i)?;
                        break;
                    }
                }
                Ok(())
            })?,
        )?;
        Ok(connection)
    })?;
    method(lua, t, PUBLISH_ASYNC, move |lua, (topic, data): (String, Value)| {
        let Value::Table(list) = topics.get::<Value>(topic.as_str())? else { return Ok(()) };
        for f in list.sequence_values::<Function>() {
            let message = lua.create_table()?;
            message.set(DATA, data.clone())?;
            message.set(SENT, now_millis() as f64 / 1000.0)?;
            crate::bindings::spawn(
                lua,
                f?,
                MultiValue::from_iter([Value::Table(message)]),
                MESSAGING_SERVICE,
                None,
            )?;
        }
        Ok(())
    })
}

/// What `GetProductInfo` tells about a product nobody can buy here: the
/// documented fields, with a price of 0 and not for sale.
fn product_info(lua: &Lua, id: f64, kind: &Value) -> mlua::Result<Table> {
    let kind = if kind.is_nil() { String::new() } else { kind.to_string().unwrap_or_default() };
    let info = lua.create_table()?;
    info.set("Name", "")?;
    info.set("Description", "")?;
    info.set("PriceInRobux", 0)?;
    info.set("IsForSale", false)?;
    info.set("IsPublicDomain", false)?;
    info.set("IsLimited", false)?;
    info.set("IsLimitedUnique", false)?;
    info.set("IsNew", false)?;
    info.set("Remaining", 0)?;
    info.set("Sales", 0)?;
    info.set("IconImageAssetId", 0)?;
    info.set("MinimumMembershipLevel", 0)?;
    info.set("ContentRatingTypeId", 0)?;
    info.set("Created", "1970-01-01T00:00:00Z")?;
    info.set("Updated", "1970-01-01T00:00:00Z")?;
    let creator = lua.create_table()?;
    creator.set("Id", 0)?;
    creator.set("Name", "")?;
    creator.set("CreatorType", "User")?;
    creator.set("CreatorTargetId", 0)?;
    creator.set("HasVerifiedBadge", false)?;
    info.set("Creator", creator)?;
    if kind.ends_with(".GamePass") {
        info.set("TargetId", id)?;
        info.set("ProductType", "Game Pass")?;
        let host = host(lua);
        let world = host.world.borrow();
        if let Some(pass) = world.passes.info.iter().find(|p| p.id as f64 == id) {
            info.set("Name", pass.name.as_str())?;
            info.set("Description", pass.description.as_str())?;
            info.set("PriceInRobux", pass.price)?;
            info.set("IsForSale", true)?;
        }
    } else if kind.ends_with(".Product") {
        info.set("ProductId", id)?;
        info.set("ProductType", "Developer Product")?;
        let host = host(lua);
        let world = host.world.borrow();
        if let Some(product) = world.products.info.iter().find(|p| p.id as f64 == id) {
            info.set("Name", product.name.as_str())?;
            info.set("Description", product.description.as_str())?;
            info.set("PriceInRobux", product.price)?;
            info.set("IsForSale", true)?;
        }
    } else {
        info.set("AssetId", id)?;
        info.set("AssetTypeId", 0)?;
        info.set("ProductId", 0)?;
    }
    Ok(info)
}

/// Badges are kept with the game's saved data, so an award lasts like
/// a real one: in a store scripts can't name (it holds a control
/// character), user id to the badges they have. Named before the rename
/// to PlayLibre, so badges already given stay.
const BADGE_STORE: &str = "libroxel\u{1f}badges";

fn has_badge(lua: &Lua, user: i64, badge: &str) -> bool {
    let host = host(lua);
    if host.world.borrow().badges.owned.contains(&(user, badge.to_owned())) {
        return true;
    }
    let data = host.data.borrow();
    let Some(list) = data.get(BADGE_STORE).and_then(|s| s.get(&user.to_string())) else { return false };
    list.as_array().is_some_and(|l| {
        l.iter().any(|b| match b {
            serde_json::Value::String(s) => s == badge,
            other => other.as_i64().is_some_and(|n| n.to_string() == badge),
        })
    })
}

fn award_badge(lua: &Lua, user: i64, badge: &str) -> bool {
    if has_badge(lua, user, badge) {
        return false;
    }
    let host = host(lua);
    {
        let mut world = host.world.borrow_mut();
        world.badges.owned.insert((user, badge.to_owned()));
        world.badges.awards.push((user, badge.to_owned()));
    }
    host.data_changed.set(true);
    let mut data = host.data.borrow_mut();
    let list = data
        .entry(BADGE_STORE.to_owned())
        .or_default()
        .entry(user.to_string())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    if let serde_json::Value::Array(l) = list {
        l.push(serde_json::Value::from(badge));
    }
    true
}

/// Key bindings: `BindAction(name, handler, button, keys...)` calls the
/// handler with the action's name, the press's state and the input when
/// one of the keys goes down or up.
fn actions(lua: &Lua, t: &Table) -> mlua::Result<()> {
    let bound = lua.create_table()?;
    lua.set_named_registry_value(ACTIONS_KEY, bound)?;
    let bind = |lua: &Lua, name: String, f: Function, button: Value, priority: f64, keys: Vec<Value>| {
        // Binding a name again replaces the old binding, which is told to
        // cancel if one of its inputs is held.
        cancel_action(lua, &name)?;
        let bound: Table = lua.named_registry_value(ACTIONS_KEY)?;
        let entry = lua.create_table()?;
        entry.set(1, f)?;
        let list = lua.create_table()?;
        for key in keys {
            list.raw_push(key)?;
        }
        entry.set(2, list)?;
        // A button on the touch screen, drawn by the app (see `touch_buttons`).
        entry.set(ACTION_TOUCH_BUTTON, button.as_boolean().unwrap_or(false))?;
        let order: u64 = lua.named_registry_value::<Option<u64>>(ACTIONS_ORDER_KEY)?.unwrap_or(0) + 1;
        lua.set_named_registry_value(ACTIONS_ORDER_KEY, order)?;
        entry.set(ACTION_ORDER, order)?;
        entry.set(ACTION_PRIORITY, priority)?;
        bound.set(name, entry)
    };
    method(
        lua,
        t,
        BIND_ACTION,
        move |lua, (name, f, button, keys): (String, Function, Value, mlua::Variadic<Value>)| {
            bind(lua, name, f, button, ACTION_DEFAULT_PRIORITY, keys.to_vec())
        },
    )?;
    method(
        lua,
        t,
        BIND_ACTION_AT_PRIORITY,
        move |lua, (name, f, button, priority, keys): (String, Function, Value, Value, mlua::Variadic<Value>)| {
            bind(lua, name, f, button, priority_number(&priority)?, keys.to_vec())
        },
    )?;
    method(lua, t, UNBIND_ACTION, |lua, name: String| {
        cancel_action(lua, &name)?;
        let bound: Table = lua.named_registry_value(ACTIONS_KEY)?;
        bound.set(name, Value::Nil)
    })?;
    method(lua, t, names::compat::GET_BOUND_ACTION_INFO, |lua, name: String| {
        let bound: Table = lua.named_registry_value(ACTIONS_KEY)?;
        match bound.get::<Value>(name)? {
            Value::Table(entry) => action_info(lua, &entry),
            _ => lua.create_table(),
        }
    })?;
    method(lua, t, names::compat::GET_ALL_BOUND_ACTION_INFO, |lua, ()| {
        let bound: Table = lua.named_registry_value(ACTIONS_KEY)?;
        let all = lua.create_table()?;
        for pair in bound.pairs::<String, Table>() {
            let (name, entry) = pair?;
            all.set(name, action_info(lua, &entry)?)?;
        }
        Ok(all)
    })?;
    method(lua, t, UNBIND_ALL_ACTIONS, |lua, ()| {
        let bound: Table = lua.named_registry_value(ACTIONS_KEY)?;
        let names: Vec<String> =
            bound.pairs::<String, Value>().filter_map(|p| p.ok().map(|(k, _)| k)).collect();
        for name in names {
            cancel_action(lua, &name)?;
        }
        lua.set_named_registry_value(ACTIONS_KEY, lua.create_table()?)
    })?;
    // How an action's touch button looks and where it sits.
    for (member, field) in [
        (names::compat::SET_TITLE, ACTION_TITLE),
        (names::compat::SET_IMAGE, ACTION_IMAGE),
        (names::compat::SET_DESCRIPTION, ACTION_DESCRIPTION),
        (names::compat::SET_POSITION, ACTION_POSITION),
    ] {
        method(lua, t, member, move |lua, (name, value): (String, Value)| {
            let bound: Table = lua.named_registry_value(ACTIONS_KEY)?;
            if let Value::Table(entry) = bound.get::<Value>(name)? {
                entry.set(field, value)?;
            }
            Ok(())
        })?;
    }
    // The reference platform hands back the button as a GUI object; scripts get nil, as
    // they do on the reference platform where no button was made (on a computer).
    method(lua, t, names::compat::GET_BUTTON, |_, _name: String| Ok(Value::Nil))
}

/// The actions bound with a touch button, in the order they were bound.
/// Whether bound actions take over the character's moves and its jump
/// (`Enum.PlayerActions.CharacterForward` and the like), so the player's own
/// controls don't move it.
pub(crate) fn bound_character_actions(lua: &Lua) -> mlua::Result<(bool, bool)> {
    let Value::Table(bound) = lua.named_registry_value::<Value>(ACTIONS_KEY)? else {
        return Ok((false, false));
    };
    let (mut moves, mut jumps) = (false, false);
    let tostring: Function = lua.globals().get("tostring")?;
    for pair in bound.pairs::<Value, Table>() {
        let (_, entry) = pair?;
        let Value::Table(keys) = entry.get::<Value>(2)? else { continue };
        for key in keys.sequence_values::<Value>() {
            let key = key?;
            if !matches!(key, Value::Table(_) | Value::UserData(_)) {
                continue;
            }
            let name: String = tostring.call(key)?;
            match name.strip_prefix("Enum.PlayerActions.") {
                Some("CharacterJump") => jumps = true,
                Some("CharacterForward" | "CharacterBackward" | "CharacterLeft" | "CharacterRight") => {
                    moves = true
                }
                _ => {}
            }
        }
    }
    Ok((moves, jumps))
}

pub(crate) fn touch_buttons(lua: &Lua) -> mlua::Result<Vec<crate::TouchButton>> {
    let Value::Table(bound) = lua.named_registry_value::<Value>(ACTIONS_KEY)? else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for pair in bound.pairs::<String, Table>() {
        let (name, entry) = pair?;
        if !entry.get::<Option<bool>>(ACTION_TOUCH_BUTTON)?.unwrap_or(false) {
            continue;
        }
        let text = |field: &str| -> mlua::Result<Option<String>> {
            Ok(match entry.get::<Value>(field)? {
                Value::String(s) => Some(s.to_string_lossy()).filter(|s| !s.is_empty()),
                _ => None,
            })
        };
        let position = crate::types::as_udim2(&entry.get::<Value>(ACTION_POSITION)?)
            .map(|p| [p.x_scale, p.x_offset, p.y_scale, p.y_offset]);
        out.push((
            entry.get::<Option<u64>>(ACTION_ORDER)?.unwrap_or(0),
            crate::TouchButton { title: text(ACTION_TITLE)?, image: text(ACTION_IMAGE)?, position, name },
        ));
    }
    out.sort_by_key(|(order, _)| *order);
    Ok(out.into_iter().map(|(_, b)| b).collect())
}

/// A touch button went down or up: runs its action as the reference platform does, with
/// the action's name, Begin or End, and a touch input.
pub(crate) fn press_touch_button(lua: &Lua, name: &str, pressed: bool) -> mlua::Result<()> {
    let Value::Table(bound) = lua.named_registry_value::<Value>(ACTIONS_KEY)? else { return Ok(()) };
    let Value::Table(entry) = bound.get::<Value>(name)? else { return Ok(()) };
    let f: Function = entry.get(1)?;
    let input = lua.create_table()?;
    input.set(
        "UserInputType",
        LuaEnumItem(EnumValue::UserInputType(playlibre_instance::UserInputType::Touch)),
    )?;
    input.set("UserInputState", input_state(pressed))?;
    input.set("KeyCode", LuaEnumItem(EnumValue::KeyCode(playlibre_instance::KeyCode::Unknown)))?;
    input.set("Position", crate::bindings::LuaVector3(playlibre_instance::Vec3::ZERO))?;
    input.set("Delta", crate::bindings::LuaVector3(playlibre_instance::Vec3::ZERO))?;
    let args = MultiValue::from_iter([
        Value::String(lua.create_string(name)?),
        Value::UserData(lua.create_userdata(input_state(pressed))?),
        Value::Table(input),
    ]);
    crate::bindings::spawn(lua, f, args, CONTEXT_ACTION_SERVICE, None)?;
    Ok(())
}

/// A priority given as a number or a `ContextActionPriority` item.
fn priority_number(value: &Value) -> mlua::Result<f64> {
    match value {
        Value::Integer(n) => Ok(*n as f64),
        Value::Number(n) => Ok(*n),
        Value::Nil => Ok(ACTION_DEFAULT_PRIORITY),
        Value::UserData(ud) => mlua::ObjectLike::get::<f64>(ud, "Value"),
        Value::Table(t) => t.get::<f64>("Value"),
        _ => Err(mlua::Error::runtime("BindActionAtPriority expects a number for the priority")),
    }
}

/// What `GetBoundActionInfo` tells about an action.
fn action_info(lua: &Lua, entry: &Table) -> mlua::Result<Table> {
    let info = lua.create_table()?;
    let inputs: Table = entry.get(2)?;
    info.set("inputTypes", inputs.clone())?;
    info.set("priorityLevel", entry.get::<Option<f64>>(ACTION_PRIORITY)?.unwrap_or(ACTION_DEFAULT_PRIORITY))?;
    info.set("stackOrder", entry.get::<Option<u64>>(ACTION_ORDER)?.unwrap_or(0))?;
    info.set("createTouchButton", entry.get::<Option<bool>>(ACTION_TOUCH_BUTTON)?.unwrap_or(false))?;
    for (field, key) in
        [(ACTION_TITLE, "title"), (ACTION_DESCRIPTION, "description"), (ACTION_IMAGE, "image")]
    {
        let value: Value = entry.get(field)?;
        if !value.is_nil() {
            info.set(key, value)?;
        }
    }
    Ok(info)
}

/// The input each of `entry`'s bindings is for.
fn input_matches(entry: &Table, wanted: EnumValue, is_key: bool) -> mlua::Result<bool> {
    let keys: Table = entry.get(2)?;
    for key in keys.sequence_values::<Value>() {
        let Value::UserData(ud) = key? else { continue };
        let Ok(item) = ud.borrow::<LuaEnumItem>().map(|i| i.0) else { continue };
        // Binding the keyboard as a whole takes every key.
        if item == wanted
            || (is_key && item == EnumValue::UserInputType(playlibre_instance::UserInputType::Keyboard))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn wanted_input(input: UserInput) -> (EnumValue, bool) {
    match input {
        UserInput::Key(k) => (EnumValue::KeyCode(k), true),
        UserInput::MouseButton1 => {
            (EnumValue::UserInputType(playlibre_instance::UserInputType::MouseButton1), false)
        }
        UserInput::MouseButton2 => {
            (EnumValue::UserInputType(playlibre_instance::UserInputType::MouseButton2), false)
        }
    }
}

/// Tells an action being unbound (or bound again) that the input it was
/// handling is cancelled, if one of its inputs is held down.
fn cancel_action(lua: &Lua, name: &str) -> mlua::Result<()> {
    let Value::Table(bound) = lua.named_registry_value::<Value>(ACTIONS_KEY)? else { return Ok(()) };
    let Value::Table(entry) = bound.get::<Value>(name)? else { return Ok(()) };
    let held: Vec<UserInput> = crate::bindings::host(lua).inputs_down.borrow().iter().copied().collect();
    for input in held {
        let (wanted, is_key) = wanted_input(input);
        if input_matches(&entry, wanted, is_key)? {
            let object = lua.create_userdata(crate::services::InputObject(input, false))?;
            let args = MultiValue::from_iter([
                Value::String(lua.create_string(name)?),
                Value::UserData(
                    lua.create_userdata(LuaEnumItem(EnumValue::UserInputState(UserInputState::Cancel)))?,
                ),
                Value::UserData(object),
            ]);
            crate::bindings::spawn(lua, entry.get::<Function>(1)?, args, CONTEXT_ACTION_SERVICE, None)?;
            break;
        }
    }
    Ok(())
}

/// Runs the bound actions for a key or button going down or up. As
/// documented, the highest priority goes first (the latest bound among equals),
/// and each handler sinks the input unless it returns
/// `Enum.ContextActionResult.Pass`.
pub(crate) fn action_input(lua: &Lua, input: UserInput, began: bool, object: &Value) -> mlua::Result<()> {
    let Value::Table(bound) = lua.named_registry_value::<Value>(ACTIONS_KEY)? else { return Ok(()) };
    let (wanted, is_key) = wanted_input(input);
    let mut calls = Vec::new();
    for pair in bound.pairs::<String, Table>() {
        let (name, entry) = pair?;
        if input_matches(&entry, wanted, is_key)? {
            let priority = entry.get::<Option<f64>>(ACTION_PRIORITY)?.unwrap_or(ACTION_DEFAULT_PRIORITY);
            let order = entry.get::<Option<u64>>(ACTION_ORDER)?.unwrap_or(0);
            calls.push((priority, order, name, entry.get::<Function>(1)?));
        }
    }
    calls.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    for (_, _, name, f) in calls {
        let args = MultiValue::from_iter([
            Value::String(lua.create_string(&name)?),
            Value::UserData(lua.create_userdata(input_state(began))?),
            object.clone(),
        ]);
        let returned = crate::bindings::spawn_returning(lua, f, args, CONTEXT_ACTION_SERVICE)?;
        let passed = returned.and_then(|r| r.into_iter().next()).is_some_and(|v| is_pass(&v));
        if !passed {
            break;
        }
    }
    Ok(())
}

/// The handler returned `Enum.ContextActionResult.Pass`.
fn is_pass(value: &Value) -> bool {
    match value {
        Value::UserData(_) | Value::Table(_) => value.to_string().is_ok_and(|s| s.ends_with(".Pass")),
        _ => false,
    }
}
