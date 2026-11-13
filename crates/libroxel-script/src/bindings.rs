//! Lua-side types, globals and the thread scheduler. Every script-visible
//! name comes from [`crate::names`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use libroxel_instance::{
    CFrame, Change, ClassKind, Color3, EasingDirection, EasingStyle, EnumKind, EnumValue, InstanceId,
    Property, PropertyValue, Rotation, TreeError, UDim2, Vec2, Vec3,
};

use crate::types::{LuaCFrame, LuaUDim2, LuaVector2, as_cframe, as_udim2, as_vector2};
use mlua::{
    AnyUserData, Function, Lua, MetaMethod, MultiValue, RegistryKey, Table, Thread, UserData, UserDataFields,
    UserDataMethods, Value, Variadic,
};

use crate::names::{self, constructors, globals};
use crate::{OutputLevel, OutputLine, ScriptError, SharedTree};

const INSTANCE_CACHE: &str = "libroxel.instance_cache";
const WAIT_FOR_CHILD_FN: &str = "libroxel.wait_for_child";
const SIGNAL_WAIT_FN: &str = "libroxel.signal_wait";
/// How long a script may run without yielding before it is stopped.
pub(crate) const RUN_LIMIT: Duration = Duration::from_secs(5);

/// The signals a script can connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SignalKind {
    Heartbeat,
    Touched(InstanceId),
    PlayerAdded,
    PlayerRemoving,
    CharacterAdded(InstanceId),
    Died(InstanceId),
    Changed(InstanceId),
    PropertyChanged(InstanceId, Property),
    ChildAdded(InstanceId),
    ButtonClicked(InstanceId),
    ButtonActivated(InstanceId),
    MouseClick(InstanceId),
    TweenCompleted(u64),
    PromptTriggered(InstanceId),
    PromptTriggerEnded(InstanceId),
    ServerEvent(InstanceId),
    ClientEvent(InstanceId),
    BindableEvent(InstanceId),
    InputBegan,
    InputEnded,
    AttributeChanged(InstanceId),
    /// One attribute, by its interned name (see [`intern`]).
    AttributeNamedChanged(InstanceId, u32),
    /// A tag, by its interned name, was added to or removed from an instance.
    TagAdded(u32),
    TagRemoved(u32),
    ToolEquipped(InstanceId),
    ToolUnequipped(InstanceId),
    ToolActivated(InstanceId),
    ToolDeactivated(InstanceId),
    MouseButton1Down,
    MouseButton1Up,
    TextBoxFocused(InstanceId),
    MoveToFinished(InstanceId),
    Stepped,
    RenderStepped,
    TextBoxFocusLost(InstanceId),
    ExplosionHit(InstanceId),
    TouchEnded(InstanceId),
    ChildRemoved(InstanceId),
    DescendantAdded(InstanceId),
    DescendantRemoving(InstanceId),
    Destroying(InstanceId),
    HealthChanged(InstanceId),
    CharacterRemoving(InstanceId),
    Seated(InstanceId),
    MouseEnter(InstanceId),
    MouseLeave(InstanceId),
    SoundEnded(InstanceId),
    /// An animation track (by its number) stopped.
    AnimationStopped(u64),
}

impl SignalKind {
    /// The instance whose changes this signal follows, if any.
    fn watched(self) -> Option<InstanceId> {
        match self {
            SignalKind::Changed(id)
            | SignalKind::PropertyChanged(id, _)
            | SignalKind::ChildAdded(id)
            | SignalKind::ChildRemoved(id)
            | SignalKind::DescendantAdded(id)
            | SignalKind::DescendantRemoving(id)
            | SignalKind::Destroying(id)
            | SignalKind::HealthChanged(id)
            | SignalKind::AttributeChanged(id)
            | SignalKind::AttributeNamedChanged(id, _) => Some(id),
            _ => None,
        }
    }
}

struct Connection {
    id: u64,
    signal: SignalKind,
    script: String,
    /// The script instance that connected it; destroying that script disconnects it.
    owner: Option<InstanceId>,
    handler: RegistryKey,
    /// Disconnect after the first time it fires.
    once: bool,
}

/// A thread paused by `task.wait` or scheduled by `task.delay`.
struct Waiting {
    thread: Thread,
    resume_at: f64,
    since: f64,
    /// `None`: resume with the elapsed time (a wait). `Some`: resume with these (a delay).
    args: Option<Vec<Value>>,
    script: String,
    owner: Option<InstanceId>,
}

/// Host state stored in the VM's app data.
pub(crate) struct Host {
    pub(crate) tree: SharedTree,
    connections: RefCell<Vec<Connection>>,
    next_connection: Cell<u64>,
    pub(crate) output: RefCell<Vec<OutputLine>>,
    pub(crate) errors: RefCell<Vec<ScriptError>>,
    current_script: RefCell<String>,
    /// The script instance whose code is running, if it came from one.
    current_owner: Cell<Option<InstanceId>>,
    pub(crate) clock: Cell<f64>,
    waiting: RefCell<Vec<Waiting>>,
    /// When the running thread must have yielded by. Checked by the VM's interrupt.
    deadline: Rc<Cell<Option<Instant>>>,
    tweens: RefCell<Vec<TweenState>>,
    next_tween: Cell<u64>,
    /// Keys and mouse buttons held down right now.
    pub(crate) inputs_down: RefCell<std::collections::HashSet<crate::UserInput>>,
    /// Saved data: store name, then key, then value.
    pub(crate) data: RefCell<crate::services::DataStores>,
    pub(crate) data_changed: Cell<bool>,
    /// Instances to destroy, and when.
    pub(crate) debris: RefCell<Vec<(InstanceId, f64)>>,
    /// Attribute and tag names that signals refer to by number.
    interned: RefCell<Vec<String>>,
    /// Where the player's mouse points, set by the host each frame.
    pub(crate) mouse: Cell<Option<crate::MouseRay>>,
    /// Velocities, the focused text box and the view size, shared with the engine.
    pub(crate) world: RefCell<crate::world::WorldState>,
    /// Which side of a networked game this is, and what it is sending.
    pub(crate) net: RefCell<crate::net::NetState>,
    /// Animations loaded on humanoids.
    pub(crate) animations: RefCell<crate::animation::Tracks>,
    /// Collision groups scripts registered, and the pairs that pass
    /// through each other (each pair in name order).
    pub(crate) groups: RefCell<crate::compat::CollisionGroups>,
}

/// The number for `name`, giving it one if it has none yet.
pub(crate) fn intern(lua: &Lua, name: &str) -> u32 {
    let host = host(lua);
    let mut names = host.interned.borrow_mut();
    if let Some(i) = names.iter().position(|n| n == name) {
        return i as u32;
    }
    names.push(name.to_owned());
    (names.len() - 1) as u32
}

/// The number for `name`, if anything has asked for one (so something may be listening).
fn interned(lua: &Lua, name: &str) -> Option<u32> {
    host(lua).interned.borrow().iter().position(|n| n == name).map(|i| i as u32)
}

/// Whether `id` is a character: a model with a humanoid in it.
pub(crate) fn is_character(tree: &libroxel_instance::InstanceTree, id: InstanceId) -> bool {
    tree.class(id) == Ok(ClassKind::Model)
        && tree.find_first_child_of_class(id, ClassKind::Humanoid).ok().flatten().is_some()
}

pub(crate) fn host(lua: &Lua) -> mlua::AppDataRef<'_, Host> {
    lua.app_data_ref::<Host>().expect("libroxel host state is installed")
}

pub(crate) fn tree_err(err: TreeError) -> mlua::Error {
    mlua::Error::runtime(describe_tree_error(err))
}

/// Phrases tree errors using script-facing names.
fn describe_tree_error(err: TreeError) -> String {
    match err {
        TreeError::Dead(_) => "the instance no longer exists".into(),
        TreeError::ParentLocked(_) => "the Parent property is locked".into(),
        TreeError::WouldCycle { .. } => "cannot set Parent: it would create a cycle".into(),
        TreeError::NotCreatable(class) => format!("{} cannot be created", names::class_name(class)),
        TreeError::NoSuchProperty { class, property } => format!(
            "{} is not a valid member of {}",
            names::property_name(property),
            names::class_name(class)
        ),
        TreeError::BadAttribute(kind) => format!("attributes can't hold a {}", names::value_kind_name(kind)),
        TreeError::WrongType { property, expected, got } => format!(
            "{} expects a {}, got a {}",
            names::property_name(property),
            names::value_kind_name(expected),
            names::value_kind_name(got)
        ),
    }
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

/// Resumes `thread`; if it yields, it is parked until the time it asked for.
/// Errors are recorded against `script`. Returns the error, if any, so the
/// first resume of a script can report it directly.
fn resume(
    lua: &Lua,
    thread: Thread,
    args: MultiValue,
    script: &str,
    owner: Option<InstanceId>,
) -> Option<ScriptError> {
    let previous = host(lua).current_script.replace(script.to_owned());
    let previous_owner = host(lua).current_owner.replace(owner);
    let deadline = host(lua).deadline.clone();
    let previous_deadline = deadline.replace(Some(Instant::now() + RUN_LIMIT));
    let result = thread.resume::<MultiValue>(args);
    deadline.set(previous_deadline);
    *host(lua).current_script.borrow_mut() = previous;
    host(lua).current_owner.set(previous_owner);
    match result {
        Ok(yielded) => {
            if thread.is_resumable() {
                let seconds = yielded.front().and_then(|v| match v {
                    Value::Number(n) => Some(*n),
                    Value::Integer(n) => Some(*n as f64),
                    _ => None,
                });
                let host = host(lua);
                let now = host.clock.get();
                host.waiting.borrow_mut().push(Waiting {
                    thread,
                    resume_at: now + seconds.unwrap_or(0.0).max(0.0),
                    since: now,
                    args: None,
                    script: script.to_owned(),
                    owner,
                });
            }
            None
        }
        Err(err) => Some(ScriptError { script: script.to_owned(), message: err.to_string() }),
    }
}

pub(crate) fn spawn(
    lua: &Lua,
    func: Function,
    args: MultiValue,
    script: &str,
    owner: Option<InstanceId>,
) -> mlua::Result<()> {
    let thread = lua.create_thread(func)?;
    if let Some(err) = resume(lua, thread, args, script, owner) {
        host(lua).errors.borrow_mut().push(err);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Value types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub(crate) struct LuaVector3(pub Vec3);

#[derive(Debug, Clone, Copy)]
pub(crate) struct LuaColor3(pub(crate) Color3);

pub(crate) fn as_vector3(value: &Value) -> Option<Vec3> {
    match value {
        Value::UserData(ud) => ud.borrow::<LuaVector3>().ok().map(|v| v.0),
        _ => None,
    }
}

pub(crate) fn as_number(value: &Value) -> Option<f32> {
    match value {
        Value::Number(n) => Some(*n as f32),
        Value::Integer(n) => Some(*n as f32),
        _ => None,
    }
}

fn vector_operands(op: &str, a: &Value, b: &Value) -> mlua::Result<(Vec3, Vec3)> {
    let lift = |v: &Value| as_vector3(v).or_else(|| as_number(v).map(|n| Vec3::new(n, n, n)));
    match (lift(a), lift(b)) {
        (Some(a), Some(b)) => Ok((a, b)),
        _ => {
            Err(mlua::Error::runtime(format!("attempt to {op} a {} with a {}", a.type_name(), b.type_name())))
        }
    }
}

impl UserData for LuaVector3 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        use names::vector3::*;
        fields.add_field_method_get(X, |_, this| Ok(this.0.x));
        fields.add_field_method_get(Y, |_, this| Ok(this.0.y));
        fields.add_field_method_get(Z, |_, this| Ok(this.0.z));
        fields.add_field_method_get(MAGNITUDE, |_, this| Ok(this.0.length()));
        fields.add_field_method_get(UNIT, |_, this| Ok(LuaVector3(this.0.normalize_or_zero())));
        fields.add_meta_field(MetaMethod::Type, TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        use names::vector3::*;
        methods.add_method(DOT, |_, this, other: Value| {
            let other = as_vector3(&other).ok_or_else(|| mlua::Error::runtime("Dot expects a Vector3"))?;
            Ok(this.0.dot(other))
        });
        methods.add_method(CROSS, |_, this, other: Value| {
            let other = as_vector3(&other).ok_or_else(|| mlua::Error::runtime("Cross expects a Vector3"))?;
            Ok(LuaVector3(this.0.cross(other)))
        });
        methods.add_method(LERP, |_, this, (other, t): (Value, f32)| {
            let other = as_vector3(&other).ok_or_else(|| mlua::Error::runtime("Lerp expects a Vector3"))?;
            Ok(LuaVector3(this.0.lerp(other, t)))
        });
        for (name, pick) in [(MIN, f32::min as fn(f32, f32) -> f32), (MAX, f32::max)] {
            methods.add_method(name, move |_, this, others: Variadic<Value>| {
                let mut out = this.0;
                for other in others.iter() {
                    let other = as_vector3(other)
                        .ok_or_else(|| mlua::Error::runtime(format!("{name} expects Vector3s")))?;
                    out = Vec3::new(pick(out.x, other.x), pick(out.y, other.y), pick(out.z, other.z));
                }
                Ok(LuaVector3(out))
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
                Ok(LuaVector3(Vec3::new(each(this.0.x), each(this.0.y), each(this.0.z))))
            });
        }
        methods.add_method(FUZZY_EQ, |_, this, (other, epsilon): (Value, Option<f32>)| {
            let other =
                as_vector3(&other).ok_or_else(|| mlua::Error::runtime("FuzzyEq expects a Vector3"))?;
            let eps = epsilon.unwrap_or(1e-5);
            let d = this.0 - other;
            Ok(d.x.abs() <= eps && d.y.abs() <= eps && d.z.abs() <= eps)
        });
        methods.add_method(ANGLE, |_, this, (other, axis): (Value, Option<Value>)| {
            let other = as_vector3(&other).ok_or_else(|| mlua::Error::runtime("Angle expects a Vector3"))?;
            let (a, b) = (this.0.normalize_or_zero(), other.normalize_or_zero());
            let angle = a.dot(b).clamp(-1.0, 1.0).acos();
            // With an axis, the angle is signed: positive turning one way round it.
            let sign = match axis.as_ref().and_then(as_vector3) {
                Some(axis) if a.cross(b).dot(axis) < 0.0 => -1.0,
                _ => 1.0,
            };
            Ok(angle * sign)
        });

        methods.add_meta_function(MetaMethod::Add, |_, (a, b): (Value, Value)| {
            let (a, b) = vector_operands("add", &a, &b)?;
            Ok(LuaVector3(a + b))
        });
        methods.add_meta_function(MetaMethod::Sub, |_, (a, b): (Value, Value)| {
            let (a, b) = vector_operands("subtract", &a, &b)?;
            Ok(LuaVector3(a - b))
        });
        methods.add_meta_function(MetaMethod::Mul, |_, (a, b): (Value, Value)| {
            let (a, b) = vector_operands("multiply", &a, &b)?;
            Ok(LuaVector3(a * b))
        });
        methods.add_meta_function(MetaMethod::Div, |_, (a, b): (Value, Value)| {
            let (a, b) = vector_operands("divide", &a, &b)?;
            Ok(LuaVector3(a / b))
        });
        methods.add_meta_method(MetaMethod::Unm, |_, this, ()| Ok(LuaVector3(-this.0)));
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            Ok(matches!((as_vector3(&a), as_vector3(&b)), (Some(a), Some(b)) if a == b))
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{}, {}, {}", this.0.x, this.0.y, this.0.z))
        });
    }
}

impl UserData for LuaColor3 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        use names::color3::*;
        fields.add_field_method_get(R, |_, this| Ok(this.0.r));
        fields.add_field_method_get(G, |_, this| Ok(this.0.g));
        fields.add_field_method_get(B, |_, this| Ok(this.0.b));
        fields.add_meta_field(MetaMethod::Type, TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{}, {}, {}", this.0.r, this.0.g, this.0.b))
        });
        methods.add_method(names::color3::LERP, |_, this, (other, t): (Value, f32)| {
            let Value::UserData(ud) = &other else {
                return Err(mlua::Error::runtime("Lerp expects a Color3"));
            };
            let other =
                ud.borrow::<LuaColor3>().map_err(|_| mlua::Error::runtime("Lerp expects a Color3"))?.0;
            let mix = |a: f32, b: f32| a + (b - a) * t;
            Ok(LuaColor3(Color3::new(mix(this.0.r, other.r), mix(this.0.g, other.g), mix(this.0.b, other.b))))
        });
        methods.add_method(names::color3::TO_HSV, |_, this, ()| Ok(crate::calls::to_hsv(this.0)));
        methods.add_method(names::color3::TO_HEX, |_, this, ()| Ok(crate::calls::to_hex(this.0)));
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            let colour = |v: &Value| match v {
                Value::UserData(ud) => ud.borrow::<LuaColor3>().ok().map(|c| c.0),
                _ => None,
            };
            Ok(matches!((colour(&a), colour(&b)), (Some(a), Some(b)) if a == b))
        });
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LuaEnumItem(pub(crate) EnumValue);

impl UserData for LuaEnumItem {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        use names::enum_item::*;
        fields.add_field_method_get(NAME, |_, this| Ok(names::enum_item_name(this.0)));
        fields.add_field_method_get(VALUE, |_, this| Ok(this.0.index() as i64));
        fields.add_field_method_get(ENUM_TYPE, |_, this| Ok(names::enum_name(this.0.kind())));
        fields.add_meta_field(MetaMethod::Type, TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!(
                "{}.{}.{}",
                globals::ENUM,
                names::enum_name(this.0.kind()),
                names::enum_item_name(this.0)
            ))
        });
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            let item = |v: &Value| match v {
                Value::UserData(ud) => ud.borrow::<LuaEnumItem>().ok().map(|e| e.0),
                _ => None,
            };
            Ok(matches!((item(&a), item(&b)), (Some(a), Some(b)) if a == b))
        });
    }
}

/// Reads an enum value of `kind` from a script value: an item, its name, or its number.
pub(crate) fn as_enum(kind: EnumKind, value: &Value) -> Option<EnumValue> {
    match value {
        Value::UserData(ud) => ud.borrow::<LuaEnumItem>().ok().map(|e| e.0).filter(|e| e.kind() == kind),
        Value::String(s) => names::enum_item_from_name(kind, &s.to_string_lossy()),
        Value::Integer(n) => usize::try_from(*n).ok().and_then(|i| kind.items().get(i).copied()),
        Value::Number(n) if n.fract() == 0.0 && *n >= 0.0 => kind.items().get(*n as usize).copied(),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub(crate) struct LuaInstance(pub(crate) InstanceId);

/// Returns the one userdata for `id`, so the same instance is always the
/// same Lua value (it works as a table key and with `rawequal`).
pub(crate) fn push_instance(lua: &Lua, id: InstanceId) -> mlua::Result<AnyUserData> {
    let cache: Table = lua.named_registry_value(INSTANCE_CACHE)?;
    let key = id.to_string();
    if let Some(existing) = cache.raw_get::<Option<AnyUserData>>(key.as_str())? {
        return Ok(existing);
    }
    let ud = lua.create_userdata(LuaInstance(id))?;
    cache.raw_set(key, &ud)?;
    Ok(ud)
}

pub(crate) fn as_instance(value: &Value) -> Option<InstanceId> {
    match value {
        Value::UserData(ud) => ud.borrow::<LuaInstance>().ok().map(|i| i.0),
        _ => None,
    }
}

pub(crate) fn property_to_lua(lua: &Lua, value: PropertyValue) -> mlua::Result<Value> {
    Ok(match value {
        PropertyValue::Bool(b) => Value::Boolean(b),
        PropertyValue::Number(n) => Value::Number(n),
        PropertyValue::String(s) => Value::String(lua.create_string(&s)?),
        PropertyValue::Vec3(v) => Value::UserData(lua.create_userdata(LuaVector3(v))?),
        PropertyValue::Color3(c) => Value::UserData(lua.create_userdata(LuaColor3(c))?),
        PropertyValue::Enum(e) => Value::UserData(lua.create_userdata(LuaEnumItem(e))?),
        PropertyValue::Ref(Some(id)) => Value::UserData(push_instance(lua, id)?),
        PropertyValue::Ref(None) => Value::Nil,
        PropertyValue::Vec2(v) => Value::UserData(lua.create_userdata(LuaVector2(v))?),
        PropertyValue::UDim2(u) => Value::UserData(lua.create_userdata(LuaUDim2(u))?),
        PropertyValue::UDim(u) => Value::UserData(lua.create_userdata(crate::types::LuaUDim(u))?),
    })
}

/// Converts a script value into a value for `property`, if it fits.
fn property_from_lua(property: Property, value: &Value) -> Option<PropertyValue> {
    if let Some(special) = crate::effects::from_lua(property, value) {
        return Some(special);
    }
    if let Some(kind) = property.enum_kind() {
        return as_enum(kind, value).map(PropertyValue::Enum);
    }
    if property.expected_kind() == "ref" {
        return match value {
            Value::Nil => Some(PropertyValue::Ref(None)),
            other => as_instance(other).map(|id| PropertyValue::Ref(Some(id))),
        };
    }
    Some(match value {
        Value::Boolean(b) => PropertyValue::Bool(*b),
        Value::Number(n) => PropertyValue::Number(*n),
        Value::Integer(n) => PropertyValue::Number(*n as f64),
        Value::String(s) => PropertyValue::String(s.to_string_lossy()),
        Value::UserData(ud) => {
            if let Some(v) = as_vector2(value) {
                PropertyValue::Vec2(v)
            } else if let Some(u) = as_udim2(value) {
                PropertyValue::UDim2(u)
            } else if let Some(u) = crate::types::as_udim(value) {
                PropertyValue::UDim(u)
            } else if let Ok(v) = ud.borrow::<LuaVector3>() {
                PropertyValue::Vec3(v.0)
            } else if let Ok(c) = ud.borrow::<LuaColor3>() {
                PropertyValue::Color3(c.0)
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

pub(crate) fn not_a_member(lua: &Lua, id: InstanceId, key: &str) -> mlua::Error {
    let host = host(lua);
    let tree = host.tree.borrow();
    let name = tree.name(id).unwrap_or("?").to_owned();
    mlua::Error::runtime(format!("{key} is not a valid member of {name}"))
}

fn class_arg(name: &str) -> mlua::Result<ClassKind> {
    names::class_from_name(name)
        .ok_or_else(|| mlua::Error::runtime(format!("{name} is not a valid class name")))
}

pub(crate) fn class_of(lua: &Lua, id: InstanceId) -> mlua::Result<ClassKind> {
    host(lua).tree.borrow().class(id).map_err(tree_err)
}

/// Fails with "not a valid member" unless `id` is a `class`.
pub(crate) fn require_class(lua: &Lua, id: InstanceId, class: ClassKind, member: &str) -> mlua::Result<()> {
    if class_of(lua, id)?.is_a(class) { Ok(()) } else { Err(not_a_member(lua, id, member)) }
}

/// The event called `key` on an instance of `class`, if it has one.
fn instance_event(class: ClassKind, id: InstanceId, key: &str) -> Option<SignalKind> {
    use names::events::*;
    Some(match key {
        TOUCHED if class.is_physical() => SignalKind::Touched(id),
        PLAYER_ADDED if class == ClassKind::Players => SignalKind::PlayerAdded,
        PLAYER_REMOVING if class == ClassKind::Players => SignalKind::PlayerRemoving,
        CHARACTER_ADDED if class == ClassKind::Player => SignalKind::CharacterAdded(id),
        DIED if class == ClassKind::Humanoid => SignalKind::Died(id),
        CHANGED => SignalKind::Changed(id),
        CHILD_ADDED => SignalKind::ChildAdded(id),
        MOUSE_BUTTON1_CLICK if class.is_button() => SignalKind::ButtonClicked(id),
        MOUSE_ENTER if class.is_gui_object() => SignalKind::MouseEnter(id),
        MOUSE_LEAVE if class.is_gui_object() => SignalKind::MouseLeave(id),
        ACTIVATED if class.is_button() => SignalKind::ButtonActivated(id),
        ACTIVATED if class == ClassKind::Tool => SignalKind::ToolActivated(id),
        DEACTIVATED if class == ClassKind::Tool => SignalKind::ToolDeactivated(id),
        EQUIPPED if class == ClassKind::Tool => SignalKind::ToolEquipped(id),
        UNEQUIPPED if class == ClassKind::Tool => SignalKind::ToolUnequipped(id),
        ATTRIBUTE_CHANGED => SignalKind::AttributeChanged(id),
        MOUSE_CLICK if class == ClassKind::ClickDetector => SignalKind::MouseClick(id),
        TRIGGERED if class == ClassKind::ProximityPrompt => SignalKind::PromptTriggered(id),
        TRIGGER_ENDED if class == ClassKind::ProximityPrompt => SignalKind::PromptTriggerEnded(id),
        ON_SERVER_EVENT if class == ClassKind::RemoteEvent => SignalKind::ServerEvent(id),
        ON_CLIENT_EVENT if class == ClassKind::RemoteEvent => SignalKind::ClientEvent(id),
        EVENT if class == ClassKind::BindableEvent => SignalKind::BindableEvent(id),
        FOCUSED if class == ClassKind::TextBox => SignalKind::TextBoxFocused(id),
        FOCUS_LOST if class == ClassKind::TextBox => SignalKind::TextBoxFocusLost(id),
        HIT if class == ClassKind::Explosion => SignalKind::ExplosionHit(id),
        names::sound::ENDED if class == ClassKind::Sound => SignalKind::SoundEnded(id),
        MOVE_TO_FINISHED if class == ClassKind::Humanoid => SignalKind::MoveToFinished(id),
        TOUCH_ENDED if class.is_physical() => SignalKind::TouchEnded(id),
        CHILD_REMOVED => SignalKind::ChildRemoved(id),
        DESCENDANT_ADDED => SignalKind::DescendantAdded(id),
        DESCENDANT_REMOVING => SignalKind::DescendantRemoving(id),
        DESTROYING => SignalKind::Destroying(id),
        HEALTH_CHANGED if class == ClassKind::Humanoid => SignalKind::HealthChanged(id),
        CHARACTER_REMOVING if class == ClassKind::Player => SignalKind::CharacterRemoving(id),
        SEATED if class == ClassKind::Humanoid => SignalKind::Seated(id),
        _ => return None,
    })
}

/// Whether instances of `class` have a `CFrame` (built from Position and Orientation).
pub(crate) fn has_cframe(class: ClassKind) -> bool {
    class.is_physical() || class == ClassKind::Camera
}

/// A part's position and orientation as one CFrame.
pub(crate) fn part_cframe(lua: &Lua, id: InstanceId) -> mlua::Result<CFrame> {
    let host = host(lua);
    let tree = host.tree.borrow();
    let pos = tree.get_vec3(id, Property::Position).ok_or_else(|| tree_err(TreeError::Dead(id)))?;
    let orientation = tree.get_vec3(id, Property::Orientation).unwrap_or_default();
    Ok(CFrame::new(pos, Rotation::from_orientation(orientation)))
}

pub(crate) fn set_part_cframe(lua: &Lua, id: InstanceId, cf: CFrame) -> mlua::Result<()> {
    let host = host(lua);
    let mut tree = host.tree.borrow_mut();
    tree.set(id, Property::Position, PropertyValue::Vec3(cf.pos)).map_err(tree_err)?;
    tree.set(id, Property::Orientation, PropertyValue::Vec3(cf.rot.to_orientation())).map_err(tree_err)
}

/// The part a model pivots around: its root part if it has one, else its first part.
pub(crate) fn model_pivot_part(lua: &Lua, model: InstanceId) -> mlua::Result<Option<InstanceId>> {
    let host = host(lua);
    let tree = host.tree.borrow();
    let parts = tree.descendants_of_class(model, ClassKind::Part);
    let root = parts.iter().copied().find(|p| tree.name(*p) == Ok(names::character::ROOT_PART));
    Ok(root.or_else(|| parts.first().copied()))
}

impl UserData for LuaInstance {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, names::INSTANCE_TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        use names::instance::*;

        methods.add_method(FIND_FIRST_CHILD, |lua, this, name: String| {
            let found = host(lua).tree.borrow().find_first_child(this.0, &name).map_err(tree_err)?;
            found.map(|id| push_instance(lua, id)).transpose()
        });
        methods.add_method(FIND_FIRST_CHILD_OF_CLASS, |lua, this, class: String| {
            let class = class_arg(&class)?;
            let found = host(lua).tree.borrow().find_first_child_of_class(this.0, class).map_err(tree_err)?;
            found.map(|id| push_instance(lua, id)).transpose()
        });
        methods.add_method(GET_CHILDREN, |lua, this, ()| {
            let children = host(lua).tree.borrow().children(this.0).map_err(tree_err)?.to_vec();
            children.into_iter().map(|id| push_instance(lua, id)).collect::<mlua::Result<Vec<_>>>()
        });
        methods.add_method(GET_DESCENDANTS, |lua, this, ()| {
            let all = host(lua).tree.borrow().descendants(this.0).map_err(tree_err)?;
            all.into_iter().map(|id| push_instance(lua, id)).collect::<mlua::Result<Vec<_>>>()
        });
        methods.add_method(IS_A, |lua, this, class: String| {
            let Some(class) = names::class_from_name(&class) else { return Ok(false) };
            Ok(host(lua).tree.borrow().class(this.0).map_err(tree_err)?.is_a(class))
        });
        methods.add_method(IS_DESCENDANT_OF, |lua, this, other: Value| {
            let other = as_instance(&other)
                .ok_or_else(|| mlua::Error::runtime("IsDescendantOf expects an Instance"))?;
            host(lua).tree.borrow().is_descendant_of(this.0, other).map_err(tree_err)
        });
        methods.add_method(GET_FULL_NAME, |lua, this, ()| {
            let host = host(lua);
            let tree = host.tree.borrow();
            let mut parts = Vec::new();
            let mut current = Some(this.0);
            while let Some(id) = current {
                // The root is not part of a full name.
                if id == tree.root() {
                    break;
                }
                parts.push(tree.name(id).map_err(tree_err)?.to_owned());
                current = tree.parent(id).map_err(tree_err)?;
            }
            parts.reverse();
            Ok(parts.join("."))
        });
        methods.add_method(DESTROY, |lua, this, ()| {
            host(lua).tree.borrow_mut().destroy(this.0).map_err(tree_err)
        });
        methods.add_method(CLONE, |lua, this, ()| {
            let copy = host(lua).tree.borrow_mut().clone_subtree(this.0).map_err(tree_err)?;
            push_instance(lua, copy)
        });
        methods.add_method(GET_SERVICE, |lua, this, name: String| {
            require_class(lua, this.0, ClassKind::DataModel, GET_SERVICE)?;
            if name == names::services::RUN_SERVICE {
                return Ok(Value::UserData(lua.create_userdata(RunService)?));
            }
            if name == names::services::TWEEN_SERVICE {
                return Ok(Value::UserData(lua.create_userdata(TweenService)?));
            }
            if let Some(service) = crate::services::get_service(lua, &name)? {
                return Ok(service);
            }
            if let Some(service) = crate::calls::get_service(lua, &name)? {
                return Ok(service);
            }
            if let Some(service) = crate::pathfinding::get_service(lua, &name)? {
                return Ok(service);
            }
            if let Some(service) = crate::compat::get_service(lua, &name)? {
                return Ok(service);
            }
            let service = names::class_from_name(&name).and_then(|c| host(lua).tree.borrow().service(c));
            match service {
                Some(id) => Ok(Value::UserData(push_instance(lua, id)?)),
                None => Err(mlua::Error::runtime(format!("{name} is not a valid service name"))),
            }
        });
        methods.add_method(names::methods::GET_PLAYERS, |lua, this, ()| {
            let class = class_of(lua, this.0)?;
            if class != ClassKind::Players && class != ClassKind::Team {
                return Err(not_a_member(lua, this.0, names::methods::GET_PLAYERS));
            }
            let players: Vec<InstanceId> = {
                let host = host(lua);
                let tree = host.tree.borrow();
                let Some(service) = tree.service(ClassKind::Players) else { return Ok(Vec::new()) };
                let children = tree.children(service).map_err(tree_err)?;
                children
                    .iter()
                    .copied()
                    .filter(|c| tree.class(*c) == Ok(ClassKind::Player))
                    // On a team: only the players on it.
                    .filter(|c| {
                        class == ClassKind::Players || tree.get_ref(*c, Property::Team) == Some(this.0)
                    })
                    .collect()
            };
            players.into_iter().map(|id| push_instance(lua, id)).collect::<mlua::Result<Vec<_>>>()
        });
        methods.add_method(names::methods::GET_PLAYER_FROM_CHARACTER, |lua, this, model: Value| {
            require_class(lua, this.0, ClassKind::Players, names::methods::GET_PLAYER_FROM_CHARACTER)?;
            let Some(model) = as_instance(&model) else { return Ok(None) };
            let found = {
                let host = host(lua);
                let tree = host.tree.borrow();
                let children = tree.children(this.0).map_err(tree_err)?;
                children.iter().copied().find(|p| tree.get_ref(*p, Property::Character) == Some(model))
            };
            found.map(|id| push_instance(lua, id)).transpose()
        });
        methods.add_method(names::methods::GET_PROPERTY_CHANGED_SIGNAL, |lua, this, name: String| {
            let class = class_of(lua, this.0)?;
            let property =
                names::property_from_name(class, &name).ok_or_else(|| not_a_member(lua, this.0, &name))?;
            Ok(Signal(SignalKind::PropertyChanged(this.0, property)))
        });
        methods.add_method(names::methods::CLEAR_ALL_CHILDREN, |lua, this, ()| {
            let host = host(lua);
            let mut tree = host.tree.borrow_mut();
            let children = tree.children(this.0).map_err(tree_err)?.to_vec();
            for child in children {
                let _ = tree.destroy(child);
            }
            Ok(())
        });
        methods.add_method(names::methods::FIND_FIRST_ANCESTOR, |lua, this, name: String| {
            let found = {
                let host = host(lua);
                let tree = host.tree.borrow();
                let mut current = tree.parent(this.0).map_err(tree_err)?;
                while let Some(id) = current {
                    if tree.name(id) == Ok(name.as_str()) {
                        break;
                    }
                    current = tree.parent(id).map_err(tree_err)?;
                }
                current
            };
            found.map(|id| push_instance(lua, id)).transpose()
        });
        methods.add_method(names::methods::FIND_FIRST_DESCENDANT, |lua, this, name: String| {
            let found = {
                let host = host(lua);
                let tree = host.tree.borrow();
                tree.descendants(this.0)
                    .map_err(tree_err)?
                    .into_iter()
                    .find(|d| tree.name(*d) == Ok(name.as_str()))
            };
            found.map(|id| push_instance(lua, id)).transpose()
        });
        methods.add_method(names::methods::GET_PIVOT, |lua, this, ()| {
            require_class(lua, this.0, ClassKind::Model, names::methods::GET_PIVOT)?;
            match model_pivot_part(lua, this.0)? {
                Some(part) => Ok(LuaCFrame(part_cframe(lua, part)?)),
                None => Ok(LuaCFrame(CFrame::IDENTITY)),
            }
        });
        methods.add_method(names::methods::PIVOT_TO, |lua, this, target: Value| {
            require_class(lua, this.0, ClassKind::Model, names::methods::PIVOT_TO)?;
            let target =
                as_cframe(&target).ok_or_else(|| mlua::Error::runtime("PivotTo expects a CFrame"))?;
            let Some(pivot) = model_pivot_part(lua, this.0)? else { return Ok(()) };
            // Move every part by the same rigid motion that takes the pivot to the target.
            let delta = target * part_cframe(lua, pivot)?.inverse();
            let parts = host(lua).tree.borrow().descendants_of_class(this.0, ClassKind::Part);
            for part in parts {
                let cf = part_cframe(lua, part)?;
                set_part_cframe(lua, part, delta * cf)?;
            }
            Ok(())
        });
        crate::services::add_instance_methods(methods);
        crate::items::add_instance_methods(methods);
        crate::world::add_instance_methods(methods);
        crate::compat::add_instance_methods(methods);
        crate::calls::add_instance_methods(methods);
        crate::effects::add_instance_methods(methods);
        crate::terrain::add_instance_methods(methods);
        methods.add_method(names::methods::TAKE_DAMAGE, |lua, this, amount: f64| {
            require_class(lua, this.0, ClassKind::Humanoid, names::methods::TAKE_DAMAGE)?;
            let host = host(lua);
            let mut tree = host.tree.borrow_mut();
            let health = tree.get_number(this.0, Property::Health).unwrap_or(0.0);
            let health = (health - amount.max(0.0)).max(0.0);
            tree.set(this.0, Property::Health, PropertyValue::Number(health)).map_err(tree_err)
        });

        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            let id = this.0;
            if key == CLASS_NAME {
                let class = host(lua).tree.borrow().class(id).map_err(tree_err)?;
                return Ok(Value::String(lua.create_string(names::class_name(class))?));
            }
            if key == PARENT {
                let parent = host(lua).tree.borrow().parent(id).map_err(tree_err)?;
                return match parent {
                    Some(p) => Ok(Value::UserData(push_instance(lua, p)?)),
                    None => Ok(Value::Nil),
                };
            }
            if key == WAIT_FOR_CHILD {
                return lua.named_registry_value::<Value>(WAIT_FOR_CHILD_FN);
            }
            let class = class_of(lua, id)?;
            if key == names::PART_CFRAME && has_cframe(class) {
                return Ok(Value::UserData(lua.create_userdata(LuaCFrame(part_cframe(lua, id)?))?));
            }
            if let Some(value) = crate::world::index(lua, id, class, &key)? {
                return Ok(value);
            }
            if let Some(value) = crate::compat::index(lua, id, class, &key)? {
                return Ok(value);
            }
            if let Some(value) = crate::calls::index(lua, id, class, &key)? {
                return Ok(value);
            }
            if let Some(value) = crate::sound::index(lua, class, &key)? {
                return Ok(value);
            }
            if let Some(value) = crate::animation::index(lua, class, &key)? {
                return Ok(value);
            }
            if key == names::methods::LOCAL_PLAYER && class == ClassKind::Players {
                return match crate::services::local_player(lua) {
                    Some(p) => Ok(Value::UserData(push_instance(lua, p)?)),
                    None => Ok(Value::Nil),
                };
            }
            if let Some(signal) = instance_event(class, id, &key) {
                return Ok(Value::UserData(lua.create_userdata(Signal(signal))?));
            }
            if let Some(property) = names::property_from_name(class, &key) {
                let value = host(lua).tree.borrow().get(id, property);
                match value {
                    Ok(value) => {
                        if let Some(special) = crate::effects::to_lua(lua, property, &value)? {
                            return Ok(special);
                        }
                        return property_to_lua(lua, value);
                    }
                    Err(TreeError::NoSuchProperty { .. }) => {} // fall through to children
                    Err(err) => return Err(tree_err(err)),
                }
            }
            // `instance.ChildName` finds a child by name.
            let child = host(lua).tree.borrow().find_first_child(id, &key).map_err(tree_err)?;
            match child {
                Some(child) => Ok(Value::UserData(push_instance(lua, child)?)),
                None => Err(not_a_member(lua, id, &key)),
            }
        });

        methods.add_meta_method(MetaMethod::NewIndex, |lua, this, (key, value): (String, Value)| {
            let id = this.0;
            if key == PARENT {
                let parent = match &value {
                    Value::Nil => None,
                    other => Some(
                        as_instance(other)
                            .ok_or_else(|| mlua::Error::runtime("Parent must be an Instance or nil"))?,
                    ),
                };
                return host(lua).tree.borrow_mut().set_parent(id, parent).map_err(tree_err);
            }
            let class = class_of(lua, id)?;
            if key == names::PART_CFRAME && has_cframe(class) {
                let cf = as_cframe(&value).ok_or_else(|| mlua::Error::runtime("CFrame expects a CFrame"))?;
                return set_part_cframe(lua, id, cf);
            }
            if crate::world::new_index(lua, id, class, &key, &value)? {
                return Ok(());
            }
            if crate::compat::new_index(lua, id, class, &key, &value)? {
                return Ok(());
            }
            if crate::calls::new_index(lua, id, class, &key, &value)? {
                return Ok(());
            }
            let Some(property) = names::property_from_name(class, &key) else {
                return Err(not_a_member(lua, id, &key));
            };
            let value = property_from_lua(property, &value).ok_or_else(|| {
                mlua::Error::runtime(format!(
                    "{key} expects a {}, got a {}",
                    names::value_kind_name(property.expected_kind()),
                    value.type_name()
                ))
            })?;
            host(lua).tree.borrow_mut().set(id, property, value).map_err(tree_err)
        });

        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            Ok(matches!((as_instance(&a), as_instance(&b)), (Some(a), Some(b)) if a == b))
        });
        methods.add_meta_method(MetaMethod::ToString, |lua, this, ()| {
            Ok(host(lua).tree.borrow().name(this.0).map_err(tree_err)?.to_owned())
        });
    }
}

// ---------------------------------------------------------------------------
// Services and signals
// ---------------------------------------------------------------------------

struct RunService;

impl UserData for RunService {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(names::services::HEARTBEAT, |_, _| Ok(Signal(SignalKind::Heartbeat)));
        fields.add_field_method_get(names::services::STEPPED, |_, _| Ok(Signal(SignalKind::Stepped)));
        fields.add_field_method_get(names::services::RENDER_STEPPED, |_, _| {
            Ok(Signal(SignalKind::RenderStepped))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        use crate::net::NetRole;
        use names::services::*;
        methods.add_method(IS_CLIENT, |lua, _, ()| Ok(crate::net::role(lua) != NetRole::Server));
        methods.add_method(IS_SERVER, |lua, _, ()| Ok(crate::net::role(lua) != NetRole::Client));
        methods.add_method(IS_STUDIO, |lua, _, ()| Ok(crate::net::role(lua) == NetRole::Solo));
    }
}

// ---------------------------------------------------------------------------
// Tweens
// ---------------------------------------------------------------------------

/// How a tween plays: from `TweenInfo.new`.
#[derive(Debug, Clone, Copy)]
struct LuaTweenInfo {
    time: f64,
    style: EasingStyle,
    direction: EasingDirection,
    /// Extra plays after the first; negative repeats forever.
    repeat_count: i64,
    reverses: bool,
    delay: f64,
}

impl UserData for LuaTweenInfo {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        use names::tween::*;
        fields.add_field_method_get(TIME, |_, this| Ok(this.time));
        fields.add_field_method_get(EASING_STYLE, |_, this| {
            Ok(LuaEnumItem(EnumValue::EasingStyle(this.style)))
        });
        fields.add_field_method_get(EASING_DIRECTION, |_, this| {
            Ok(LuaEnumItem(EnumValue::EasingDirection(this.direction)))
        });
        fields.add_field_method_get(REPEAT_COUNT, |_, this| Ok(this.repeat_count));
        fields.add_field_method_get(REVERSES, |_, this| Ok(this.reverses));
        fields.add_field_method_get(DELAY_TIME, |_, this| Ok(this.delay));
        fields.add_meta_field(MetaMethod::Type, INFO_TYPE_NAME);
    }
}

/// time, style, direction, repeat count, reverses, delay.
type TweenInfoArgs = (Option<f64>, Option<Value>, Option<Value>, Option<i64>, Option<bool>, Option<f64>);

fn install_tween_info(lua: &Lua) -> mlua::Result<()> {
    let lib = lua.create_table()?;
    lib.set(
        constructors::NEW,
        lua.create_function(|_, (time, style, direction, repeat_count, reverses, delay): TweenInfoArgs| {
            let style = match style.as_ref().map(|v| as_enum(EnumKind::EasingStyle, v)) {
                None => EasingStyle::Quad,
                Some(Some(EnumValue::EasingStyle(s))) => s,
                Some(_) => return Err(mlua::Error::runtime("TweenInfo.new expects an EasingStyle")),
            };
            let direction = match direction.as_ref().map(|v| as_enum(EnumKind::EasingDirection, v)) {
                None => EasingDirection::Out,
                Some(Some(EnumValue::EasingDirection(d))) => d,
                Some(_) => return Err(mlua::Error::runtime("TweenInfo.new expects an EasingDirection")),
            };
            Ok(LuaTweenInfo {
                time: time.unwrap_or(1.0).max(0.0),
                style,
                direction,
                repeat_count: repeat_count.unwrap_or(0),
                reverses: reverses.unwrap_or(false),
                delay: delay.unwrap_or(0.0).max(0.0),
            })
        })?,
    )?;
    lua.globals().set(globals::TWEEN_INFO, lib)
}

/// A value a tween can move between.
#[derive(Debug, Clone, Copy)]
enum TweenValue {
    Number(f64),
    Vec3(Vec3),
    Color(Color3),
    Vec2(Vec2),
    UDim2(UDim2),
    UDim(libroxel_instance::UDim),
    CFrame(CFrame),
}

impl TweenValue {
    fn from_property(value: &PropertyValue) -> Option<TweenValue> {
        Some(match value {
            PropertyValue::Number(n) => TweenValue::Number(*n),
            PropertyValue::Vec3(v) => TweenValue::Vec3(*v),
            PropertyValue::Color3(c) => TweenValue::Color(*c),
            PropertyValue::Vec2(v) => TweenValue::Vec2(*v),
            PropertyValue::UDim2(u) => TweenValue::UDim2(*u),
            PropertyValue::UDim(u) => TweenValue::UDim(*u),
            _ => return None,
        })
    }

    fn lerp(self, to: TweenValue, t: f32) -> TweenValue {
        match (self, to) {
            (TweenValue::Number(a), TweenValue::Number(b)) => TweenValue::Number(a + (b - a) * t as f64),
            (TweenValue::Vec3(a), TweenValue::Vec3(b)) => TweenValue::Vec3(a.lerp(b, t)),
            (TweenValue::Color(a), TweenValue::Color(b)) => TweenValue::Color(a.lerp(b, t)),
            (TweenValue::Vec2(a), TweenValue::Vec2(b)) => {
                TweenValue::Vec2(Vec2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t))
            }
            (TweenValue::UDim2(a), TweenValue::UDim2(b)) => TweenValue::UDim2(a.lerp(b, t)),
            (TweenValue::UDim(a), TweenValue::UDim(b)) => TweenValue::UDim(a.lerp(b, t)),
            (TweenValue::CFrame(a), TweenValue::CFrame(b)) => TweenValue::CFrame(a.lerp(b, t)),
            (_, to) => to,
        }
    }
}

/// What a tween goal changes: a property, or a part's CFrame.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TweenTarget {
    Property(Property),
    PartCFrame,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TweenPhase {
    Idle,
    Playing,
    Paused,
    Done,
}

struct TweenState {
    id: u64,
    instance: InstanceId,
    info: LuaTweenInfo,
    /// Target, value at the start (filled in when played), goal value.
    goals: Vec<(TweenTarget, Option<TweenValue>, TweenValue)>,
    elapsed: f64,
    phase: TweenPhase,
}

struct TweenService;

impl UserData for TweenService {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(names::methods::CREATE, |lua, _, (target, info, goals): (Value, Value, Table)| {
            create_tween(lua, target, info, goals)
        });
        methods.add_method(
            names::compat::GET_VALUE,
            |_, _, (alpha, style, direction): (f32, Value, Value)| {
                crate::compat::tween_value(alpha, &style, &direction)
            },
        );
    }
}

fn read_tween_value(lua: &Lua, id: InstanceId, target: TweenTarget) -> mlua::Result<Option<TweenValue>> {
    Ok(match target {
        TweenTarget::PartCFrame => Some(TweenValue::CFrame(part_cframe(lua, id)?)),
        TweenTarget::Property(p) => {
            host(lua).tree.borrow().get(id, p).ok().as_ref().and_then(TweenValue::from_property)
        }
    })
}

fn write_tween_value(lua: &Lua, id: InstanceId, target: TweenTarget, value: TweenValue) {
    let _ = match (target, value) {
        (TweenTarget::PartCFrame, TweenValue::CFrame(cf)) => set_part_cframe(lua, id, cf),
        (TweenTarget::Property(p), v) => {
            let value = match v {
                TweenValue::Number(n) => PropertyValue::Number(n),
                TweenValue::Vec3(v) => PropertyValue::Vec3(v),
                TweenValue::Color(c) => PropertyValue::Color3(c),
                TweenValue::Vec2(v) => PropertyValue::Vec2(v),
                TweenValue::UDim2(u) => PropertyValue::UDim2(u),
                TweenValue::UDim(u) => PropertyValue::UDim(u),
                TweenValue::CFrame(_) => return,
            };
            host(lua).tree.borrow_mut().set(id, p, value).map_err(tree_err)
        }
        _ => Ok(()),
    };
}

fn create_tween(lua: &Lua, target: Value, info: Value, goals: Table) -> mlua::Result<LuaTween> {
    let id = as_instance(&target).ok_or_else(|| mlua::Error::runtime("Create expects an Instance"))?;
    let info = match &info {
        Value::UserData(ud) => {
            *ud.borrow::<LuaTweenInfo>().map_err(|_| mlua::Error::runtime("Create expects a TweenInfo"))?
        }
        _ => return Err(mlua::Error::runtime("Create expects a TweenInfo")),
    };
    let class = class_of(lua, id)?;
    let mut list = Vec::new();
    for pair in goals.pairs::<String, Value>() {
        let (key, value) = pair?;
        if key == names::PART_CFRAME && has_cframe(class) {
            let cf = as_cframe(&value).ok_or_else(|| mlua::Error::runtime("CFrame goals must be CFrames"))?;
            list.push((TweenTarget::PartCFrame, None, TweenValue::CFrame(cf)));
            continue;
        }
        let property = names::property_from_name(class, &key).ok_or_else(|| not_a_member(lua, id, &key))?;
        let goal = property_from_lua(property, &value)
            .as_ref()
            .and_then(TweenValue::from_property)
            .ok_or_else(|| mlua::Error::runtime(format!("{key} can't be tweened to that value")))?;
        list.push((TweenTarget::Property(property), None, goal));
    }
    let host = host(lua);
    let tween = host.next_tween.get();
    host.next_tween.set(tween + 1);
    host.tweens.borrow_mut().push(TweenState {
        id: tween,
        instance: id,
        info,
        goals: list,
        elapsed: 0.0,
        phase: TweenPhase::Idle,
    });
    Ok(LuaTween(tween))
}

struct LuaTween(u64);

impl LuaTween {
    fn with<R>(&self, lua: &Lua, f: impl FnOnce(&mut TweenState) -> R) -> Option<R> {
        let host = host(lua);
        let mut tweens = host.tweens.borrow_mut();
        tweens.iter_mut().find(|t| t.id == self.0).map(f)
    }
}

impl UserData for LuaTween {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(names::events::COMPLETED, |_, this| {
            Ok(Signal(SignalKind::TweenCompleted(this.0)))
        });
        fields.add_meta_field(MetaMethod::Type, names::tween::TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(names::tween::PLAY, |lua, this, ()| {
            let Some((instance, phase, targets)) =
                this.with(lua, |t| (t.instance, t.phase, t.goals.iter().map(|g| g.0).collect::<Vec<_>>()))
            else {
                return Ok(());
            };
            if phase == TweenPhase::Playing {
                return Ok(());
            }
            if phase != TweenPhase::Paused {
                // Starting fresh: remember where everything starts.
                let mut starts = Vec::new();
                for target in &targets {
                    starts.push(read_tween_value(lua, instance, *target)?);
                }
                this.with(lua, |t| {
                    for (goal, start) in t.goals.iter_mut().zip(starts) {
                        goal.1 = start;
                    }
                    t.elapsed = 0.0;
                });
            }
            // A newer tween takes over any property an older one is still moving.
            {
                let host = host(lua);
                let mut tweens = host.tweens.borrow_mut();
                for other in tweens.iter_mut().filter(|t| t.id != this.0 && t.instance == instance) {
                    if other.phase == TweenPhase::Playing {
                        other.goals.retain(|g| !targets.contains(&g.0));
                    }
                }
            }
            this.with(lua, |t| t.phase = TweenPhase::Playing);
            Ok(())
        });
        methods.add_method(names::tween::PAUSE, |lua, this, ()| {
            this.with(lua, |t| {
                if t.phase == TweenPhase::Playing {
                    t.phase = TweenPhase::Paused;
                }
            });
            Ok(())
        });
        methods.add_method(names::tween::CANCEL, |lua, this, ()| {
            this.with(lua, |t| {
                t.phase = TweenPhase::Idle;
                t.elapsed = 0.0;
            });
            Ok(())
        });
    }
}

/// Moves every playing tween forward by `dt` and fires the ones that finished.
fn step_tweens(lua: &Lua, dt: f64) {
    let mut writes: Vec<(InstanceId, TweenTarget, TweenValue)> = Vec::new();
    let mut finished = Vec::new();
    {
        let host = host(lua);
        let mut tweens = host.tweens.borrow_mut();
        for t in tweens.iter_mut().filter(|t| t.phase == TweenPhase::Playing) {
            t.elapsed += dt;
            let info = t.info;
            let active = t.elapsed - info.delay;
            if active < 0.0 {
                continue;
            }
            let cycle = info.time * if info.reverses { 2.0 } else { 1.0 };
            let plays = if info.repeat_count < 0 { f64::INFINITY } else { (info.repeat_count + 1) as f64 };
            let done = cycle <= 0.0 || active >= cycle * plays;
            let alpha = if done {
                if info.reverses { 0.0 } else { 1.0 }
            } else {
                let within = active % cycle;
                let forward = within.min(info.time) / info.time.max(1e-9);
                if info.reverses && within > info.time {
                    1.0 - (within - info.time) / info.time.max(1e-9)
                } else {
                    forward
                }
            };
            let eased = info.style.ease(info.direction, alpha as f32);
            for (target, start, goal) in &t.goals {
                let Some(start) = start else { continue };
                writes.push((t.instance, *target, start.lerp(*goal, eased)));
            }
            if done {
                t.phase = TweenPhase::Done;
                finished.push(t.id);
            }
        }
    }
    for (id, target, value) in writes {
        write_tween_value(lua, id, target, value);
    }
    for tween in finished {
        fire(lua, SignalKind::TweenCompleted(tween), || Ok(MultiValue::new()));
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Signal(pub(crate) SignalKind);

impl UserData for Signal {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(names::signal::CONNECT, |lua, this, handler: Function| {
            let handler = lua.create_registry_value(handler)?;
            let host = host(lua);
            let id = host.next_connection.get();
            host.next_connection.set(id + 1);
            let script = host.current_script.borrow().clone();
            host.connections.borrow_mut().push(Connection {
                id,
                signal: this.0,
                script,
                owner: host.current_owner.get(),
                handler,
                once: false,
            });
            Ok(ConnectionHandle(id))
        });
        methods.add_method(names::signal::ONCE, |lua, this, handler: Function| {
            let handler = lua.create_registry_value(handler)?;
            let host = host(lua);
            let id = host.next_connection.get();
            host.next_connection.set(id + 1);
            let script = host.current_script.borrow().clone();
            host.connections.borrow_mut().push(Connection {
                id,
                signal: this.0,
                script,
                owner: host.current_owner.get(),
                handler,
                once: true,
            });
            Ok(ConnectionHandle(id))
        });
        // `Wait` has to yield, so it is written in Luau (see the prelude).
        methods.add_meta_method(MetaMethod::Index, |lua, _, key: String| {
            if key == names::signal::WAIT {
                return lua.named_registry_value::<Value>(SIGNAL_WAIT_FN);
            }
            Err(mlua::Error::runtime(format!("{key} is not a valid member of a signal")))
        });
    }
}

struct ConnectionHandle(u64);

impl UserData for ConnectionHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get(names::signal::CONNECTED, |lua, this| {
            Ok(host(lua).connections.borrow().iter().any(|c| c.id == this.0))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(names::signal::DISCONNECT, |lua, this, ()| {
            let removed: Vec<Connection> = {
                let host = host(lua);
                let mut conns = host.connections.borrow_mut();
                let (gone, kept) = std::mem::take(&mut *conns).into_iter().partition(|c| c.id == this.0);
                *conns = kept;
                gone
            };
            for conn in removed {
                lua.remove_registry_value(conn.handler)?;
            }
            Ok(())
        });
    }
}

pub(crate) fn fire(lua: &Lua, signal: SignalKind, args: impl Fn() -> mlua::Result<MultiValue>) {
    // Snapshot handlers first: a handler may connect or disconnect others.
    let handlers: Vec<(String, Option<InstanceId>, mlua::Result<Function>)> = {
        let host = host(lua);
        let tree = host.tree.borrow();
        host.connections
            .borrow()
            .iter()
            .filter(|c| c.signal == signal && owner_alive(&tree, c.owner))
            .map(|c| (c.script.clone(), c.owner, lua.registry_value::<Function>(&c.handler)))
            .collect()
    };
    // One-shot connections are gone once they have fired.
    let spent: Vec<Connection> = {
        let host = host(lua);
        let mut conns = host.connections.borrow_mut();
        let (spent, kept) =
            std::mem::take(&mut *conns).into_iter().partition(|c| c.once && c.signal == signal);
        *conns = kept;
        spent
    };
    for conn in spent {
        let _ = lua.remove_registry_value(conn.handler);
    }
    for (script, owner, handler) in handlers {
        let result = handler.and_then(|f| spawn(lua, f, args()?, &script, owner));
        if let Err(err) = result {
            host(lua).errors.borrow_mut().push(ScriptError { script, message: err.to_string() });
        }
    }
}

// ---------------------------------------------------------------------------
// Installation and host entry points
// ---------------------------------------------------------------------------

fn join_args(args: Variadic<Value>) -> mlua::Result<String> {
    let parts: mlua::Result<Vec<String>> = args.iter().map(|v| v.to_string()).collect();
    Ok(parts?.join(" "))
}

/// Luau-side helpers that must be able to yield, so they can't be Rust functions.
fn prelude() -> String {
    use names::{instance, task};
    format!(
        r#"
        local yield = coroutine.yield
        local lib, load_module = ...
        lib["{wait}"] = function(seconds) return yield(seconds or 0) end
        local wait = lib["{wait}"]
        local wait_for_child = function(self, name, timeout)
            local waited = 0
            while true do
                local child = self:{find}(name)
                if child then return child end
                if timeout and waited >= timeout then return nil end
                waited += wait()
            end
        end
        -- Each module runs once; later requires get the same value back.
        local results, loading = {{}}, {{}}
        local require = function(module)
            local done = results[module]
            if done then return done.value end
            if loading[module] then
                error("require loop: " .. tostring(module) .. " requires itself", 2)
            end
            local run = load_module(module)
            loading[module] = true
            local out = table.pack(pcall(run))
            loading[module] = nil
            if not out[1] then error(out[2], 0) end
            if out.n ~= 2 then
                error(tostring(module) .. " must return exactly one value", 2)
            end
            results[module] = {{ value = out[2] }}
            return out[2]
        end
        local signal_wait = function(self)
            local result
            self:{once}(function(...) result = table.pack(...) end)
            while not result do wait() end
            return table.unpack(result, 1, result.n)
        end
        return wait, wait_for_child, require, signal_wait
        "#,
        once = names::signal::ONCE,
        wait = task::WAIT,
        find = instance::FIND_FIRST_CHILD,
    )
}

pub(crate) fn install(lua: &Lua, tree: SharedTree) -> mlua::Result<()> {
    let (root, workspace) = {
        let tree = tree.borrow();
        (tree.root(), tree.workspace())
    };
    let deadline: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    let check = deadline.clone();
    lua.set_interrupt(move |_| {
        if check.get().is_some_and(|d| Instant::now() > d) {
            check.set(None);
            return Err(mlua::Error::runtime(format!(
                "script ran for more than {} seconds without waiting; add task.wait() inside long loops",
                RUN_LIMIT.as_secs()
            )));
        }
        Ok(mlua::VmState::Continue)
    });
    lua.set_app_data(Host {
        deadline,
        tree,
        connections: RefCell::new(Vec::new()),
        next_connection: Cell::new(1),
        output: RefCell::new(Vec::new()),
        errors: RefCell::new(Vec::new()),
        current_script: RefCell::new(String::new()),
        current_owner: Cell::new(None),
        clock: Cell::new(0.0),
        waiting: RefCell::new(Vec::new()),
        tweens: RefCell::new(Vec::new()),
        next_tween: Cell::new(1),
        inputs_down: RefCell::new(Default::default()),
        data: RefCell::new(Default::default()),
        animations: RefCell::new(Default::default()),
        data_changed: Cell::new(false),
        debris: RefCell::new(Vec::new()),
        interned: RefCell::new(Vec::new()),
        mouse: Cell::new(None),
        world: RefCell::new(Default::default()),
        net: RefCell::new(Default::default()),
        groups: RefCell::new(Default::default()),
    });

    // Weak-valued cache so each instance has exactly one userdata.
    let cache = lua.create_table()?;
    let cache_meta = lua.create_table()?;
    cache_meta.set("__mode", "v")?;
    cache.set_metatable(Some(cache_meta))?;
    lua.set_named_registry_value(INSTANCE_CACHE, cache)?;

    let g = lua.globals();
    g.set(globals::GAME, push_instance(lua, root)?)?;
    g.set(globals::WORKSPACE, push_instance(lua, workspace)?)?;

    let instance_lib = lua.create_table()?;
    instance_lib.set(
        constructors::NEW,
        lua.create_function(|lua, (class, parent): (String, Option<Value>)| {
            let class = class_arg(&class)?;
            if !class.is_user_creatable() {
                return Err(tree_err(TreeError::NotCreatable(class)));
            }
            let parent = parent.as_ref().and_then(as_instance);
            let id = {
                let host = host(lua);
                let mut tree = host.tree.borrow_mut();
                let id = tree.create(class, names::class_name(class)).map_err(tree_err)?;
                if parent.is_some() {
                    tree.set_parent(id, parent).map_err(tree_err)?;
                }
                id
            };
            push_instance(lua, id)
        })?,
    )?;
    g.set(globals::INSTANCE, instance_lib)?;

    let vector3_lib = lua.create_table()?;
    vector3_lib.set(
        constructors::NEW,
        lua.create_function(|_, (x, y, z): (Option<f32>, Option<f32>, Option<f32>)| {
            Ok(LuaVector3(Vec3::new(x.unwrap_or(0.0), y.unwrap_or(0.0), z.unwrap_or(0.0))))
        })?,
    )?;
    vector3_lib.set(constructors::ZERO, LuaVector3(Vec3::ZERO))?;
    vector3_lib.set(constructors::ONE, LuaVector3(Vec3::ONE))?;
    g.set(globals::VECTOR3, vector3_lib)?;

    let color3_lib = lua.create_table()?;
    color3_lib.set(
        constructors::NEW,
        lua.create_function(|_, (r, g, b): (Option<f32>, Option<f32>, Option<f32>)| {
            Ok(LuaColor3(Color3::new(r.unwrap_or(0.0), g.unwrap_or(0.0), b.unwrap_or(0.0))))
        })?,
    )?;
    color3_lib.set(
        constructors::FROM_RGB,
        lua.create_function(|_, (r, g, b): (Option<f32>, Option<f32>, Option<f32>)| {
            Ok(LuaColor3(Color3::new(
                r.unwrap_or(0.0) / 255.0,
                g.unwrap_or(0.0) / 255.0,
                b.unwrap_or(0.0) / 255.0,
            )))
        })?,
    )?;
    g.set(globals::COLOR3, color3_lib)?;

    let task_lib = lua.create_table()?;
    task_lib.set(
        names::task::SPAWN,
        lua.create_function(|lua, (func, args): (Function, Variadic<Value>)| {
            let script = host(lua).current_script.borrow().clone();
            let owner = host(lua).current_owner.get();
            spawn(lua, func, args.into_iter().collect(), &script, owner)
        })?,
    )?;
    task_lib.set(
        names::task::DELAY,
        lua.create_function(|lua, (seconds, func, args): (f64, Function, Variadic<Value>)| {
            let thread = lua.create_thread(func)?;
            let host = host(lua);
            let now = host.clock.get();
            let script = host.current_script.borrow().clone();
            host.waiting.borrow_mut().push(Waiting {
                thread,
                resume_at: now + seconds.max(0.0),
                since: now,
                args: Some(args.into_iter().collect()),
                script,
                owner: host.current_owner.get(),
            });
            Ok(())
        })?,
    )?;
    let load_module = lua.create_function(|lua, module: Value| {
        let id =
            as_instance(&module).ok_or_else(|| mlua::Error::runtime("require expects a ModuleScript"))?;
        let (class, name, source) = {
            let host = host(lua);
            let tree = host.tree.borrow();
            let class = tree.class(id).map_err(tree_err)?;
            (class, tree.name(id).map_err(tree_err)?.to_owned(), tree.get_string(id, Property::Source))
        };
        if class != ClassKind::ModuleScript {
            return Err(mlua::Error::runtime(format!(
                "require expects a {}, got a {}",
                names::class_name(ClassKind::ModuleScript),
                names::class_name(class)
            )));
        }
        let env = script_env(lua, Some(id))?;
        lua.load(source.unwrap_or_default()).set_name(format!("={name}")).set_environment(env).into_function()
    })?;
    let (wait, wait_for_child, require, signal_wait): (Function, Function, Function, Function) =
        lua.load(prelude()).set_name("=libroxel").call((task_lib.clone(), load_module))?;
    lua.set_named_registry_value(SIGNAL_WAIT_FN, signal_wait)?;
    crate::types::install(lua)?;
    install_tween_info(lua)?;
    crate::services::install(lua)?;
    crate::world::install(lua)?;
    crate::compat::install(lua)?;
    lua.set_named_registry_value(WAIT_FOR_CHILD_FN, wait_for_child)?;
    g.set(globals::REQUIRE, require)?;

    let enum_lib = lua.create_table()?;
    for &kind in EnumKind::ALL {
        let items = lua.create_table()?;
        for &item in kind.items() {
            items.set(names::enum_item_name(item), LuaEnumItem(item))?;
        }
        crate::compat::add_enum_items(lua, &items, kind)?;
        items.set_readonly(true);
        enum_lib.set(names::enum_name(kind), items)?;
    }
    enum_lib.set_readonly(true);
    g.set(globals::ENUM, enum_lib)?;
    g.set(globals::TASK, task_lib)?;
    g.set(globals::WAIT, wait)?;
    g.set(globals::TIME, lua.create_function(|lua, ()| Ok(host(lua).clock.get()))?)?;
    crate::calls::install(lua)?;
    crate::sound::install(lua)?;
    crate::animation::install(lua)?;
    crate::effects::install(lua)?;

    g.set(
        globals::PRINT,
        lua.create_function(|lua, args: Variadic<Value>| {
            let text = join_args(args)?;
            host(lua).output.borrow_mut().push(OutputLine { level: OutputLevel::Print, text });
            Ok(())
        })?,
    )?;
    g.set(
        globals::WARN,
        lua.create_function(|lua, args: Variadic<Value>| {
            let text = join_args(args)?;
            host(lua).output.borrow_mut().push(OutputLine { level: OutputLevel::Warn, text });
            Ok(())
        })?,
    )?;
    Ok(())
}

/// Each script gets its own global environment that falls back to the
/// shared, read-only globals, so scripts can't leak globals into each other.
fn script_env(lua: &Lua, script_instance: Option<InstanceId>) -> mlua::Result<Table> {
    let env = lua.create_table()?;
    let meta = lua.create_table()?;
    meta.set("__index", lua.globals())?;
    env.set_metatable(Some(meta))?;
    if let Some(id) = script_instance {
        env.raw_set(globals::SCRIPT, push_instance(lua, id)?)?;
    }
    Ok(env)
}

/// Runs a chunk as a new thread in its own environment. Errors raised before
/// the script first yields are returned; later ones surface from `step`.
pub(crate) fn run_chunk(
    lua: &Lua,
    script_name: &str,
    source: &str,
    script_instance: Option<InstanceId>,
) -> Result<(), ScriptError> {
    let fail = |err: mlua::Error| ScriptError { script: script_name.to_owned(), message: err.to_string() };
    let env = script_env(lua, script_instance).map_err(fail)?;
    let func = lua
        .load(source)
        .set_name(format!("={script_name}"))
        .set_environment(env)
        .into_function()
        .map_err(fail)?;
    let thread = lua.create_thread(func).map_err(fail)?;
    match resume(lua, thread, MultiValue::new(), script_name, script_instance) {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Runs every enabled `Script` in the workspace and the server script
/// service, in tree order. Scripts elsewhere (in storage, or in GUIs
/// waiting to be copied to players) don't run.
pub(crate) fn run_tree_scripts(lua: &Lua) -> Vec<ScriptError> {
    let roots: Vec<InstanceId> = {
        let host = host(lua);
        let tree = host.tree.borrow();
        [ClassKind::Workspace, ClassKind::ServerScriptService]
            .iter()
            .filter_map(|c| tree.service(*c))
            .collect()
    };
    roots.into_iter().flat_map(|root| run_scripts_under(lua, root, ClassKind::Script)).collect()
}

/// Runs every enabled script of `class` under `ancestor`, in tree order.
pub(crate) fn run_scripts_under(lua: &Lua, ancestor: InstanceId, class: ClassKind) -> Vec<ScriptError> {
    let scripts: Vec<(InstanceId, String, String)> = {
        let host = host(lua);
        let tree = host.tree.borrow();
        tree.descendants_of_class(ancestor, class)
            .into_iter()
            .filter(|id| tree.get_bool(*id, Property::Disabled) != Some(true))
            .filter_map(|id| {
                let source = match tree.get(id, Property::Source).ok()? {
                    PropertyValue::String(s) => s,
                    _ => return None,
                };
                Some((id, tree.name(id).ok()?.to_owned(), source))
            })
            .collect()
    };
    scripts
        .into_iter()
        .filter_map(|(id, name, source)| run_chunk(lua, &name, &source, Some(id)).err())
        .collect()
}

/// Whether the script that owns a thread or connection still exists.
fn owner_alive(tree: &libroxel_instance::InstanceTree, owner: Option<InstanceId>) -> bool {
    owner.is_none_or(|id| tree.exists(id) && !tree.is_destroyed(id).unwrap_or(true))
}

/// Disconnects everything connected by scripts that have been destroyed
/// (like the old copy of a GUI after a respawn), and drops their waiting threads.
fn stop_destroyed_scripts(lua: &Lua) {
    let gone: Vec<Connection> = {
        let host = host(lua);
        let tree = host.tree.borrow();
        let mut conns = host.connections.borrow_mut();
        let (gone, kept) =
            std::mem::take(&mut *conns).into_iter().partition(|c| !owner_alive(&tree, c.owner));
        *conns = kept;
        host.waiting.borrow_mut().retain(|w| owner_alive(&tree, w.owner));
        gone
    };
    for conn in gone {
        let _ = lua.remove_registry_value(conn.handler);
    }
}

/// Advances the clock, resumes due threads, then fires the frame signal.
pub(crate) fn step(lua: &Lua, dt: f64) -> Vec<ScriptError> {
    let now = {
        let host = host(lua);
        host.clock.set(host.clock.get() + dt);
        host.clock.get()
    };
    // Take only threads due now; ones that wait again are parked for a later frame.
    let mut due: Vec<Waiting> = {
        let host = host(lua);
        let mut waiting = host.waiting.borrow_mut();
        let (due, later) = std::mem::take(&mut *waiting).into_iter().partition(|w| w.resume_at <= now);
        *waiting = later;
        due
    };
    due.sort_by(|a, b| a.resume_at.total_cmp(&b.resume_at));
    stop_destroyed_scripts(lua);
    for w in due {
        // A destroyed script's threads stop where they are.
        if !owner_alive(&host(lua).tree.borrow(), w.owner) {
            continue;
        }
        let args: MultiValue = match w.args {
            Some(args) => args.into_iter().collect(),
            None => std::iter::once(Value::Number(now - w.since)).collect(),
        };
        if let Some(err) = resume(lua, w.thread, args, &w.script, w.owner) {
            host(lua).errors.borrow_mut().push(err);
        }
    }
    step_tweens(lua, dt);
    crate::animation::step(lua);
    crate::services::step_debris(lua, now);
    fire(lua, SignalKind::Stepped, || Ok([Value::Number(now), Value::Number(dt)].into_iter().collect()));
    fire(lua, SignalKind::Heartbeat, || Ok(std::iter::once(Value::Number(dt)).collect()));
    fire(lua, SignalKind::RenderStepped, || Ok(std::iter::once(Value::Number(dt)).collect()));
    take_errors(lua)
}

/// Fires an instance event with one instance argument.
pub(crate) fn fire_with_instance(lua: &Lua, signal: SignalKind, arg: InstanceId) -> Vec<ScriptError> {
    fire(lua, signal, || Ok(std::iter::once(Value::UserData(push_instance(lua, arg)?)).collect()));
    take_errors(lua)
}

/// Fires an instance event with no arguments.
pub(crate) fn fire_plain(lua: &Lua, signal: SignalKind) -> Vec<ScriptError> {
    fire(lua, signal, || Ok(MultiValue::new()));
    take_errors(lua)
}

/// Fires change events for everything in `changes` that a script is listening to.
pub(crate) fn fire_changes(lua: &Lua, changes: &[Change]) -> Vec<ScriptError> {
    let (watched, tags_heard, tools_heard) = {
        let host = host(lua);
        let conns = host.connections.borrow();
        let watched: std::collections::HashSet<InstanceId> =
            conns.iter().filter_map(|c| c.signal.watched()).collect();
        let tags =
            conns.iter().any(|c| matches!(c.signal, SignalKind::TagAdded(_) | SignalKind::TagRemoved(_)));
        let tools = conns
            .iter()
            .any(|c| matches!(c.signal, SignalKind::ToolEquipped(_) | SignalKind::ToolUnequipped(_)));
        (watched, tags, tools)
    };
    if watched.is_empty() && !tags_heard && !tools_heard {
        return Vec::new();
    }
    let with_instance = |signal: SignalKind, id: InstanceId| {
        fire(lua, signal, || Ok(std::iter::once(Value::UserData(push_instance(lua, id)?)).collect()));
    };
    for change in changes {
        match change {
            Change::PropertyChanged { id, property } if watched.contains(id) => {
                let name = names::property_name(*property);
                fire(lua, SignalKind::Changed(*id), || {
                    Ok(std::iter::once(Value::String(lua.create_string(name)?)).collect())
                });
                fire(lua, SignalKind::PropertyChanged(*id, *property), || Ok(MultiValue::new()));
                if *property == Property::Health {
                    let health = host(lua).tree.borrow().get_number(*id, Property::Health);
                    if let Some(health) = health {
                        fire(lua, SignalKind::HealthChanged(*id), || {
                            Ok(std::iter::once(Value::Number(health)).collect())
                        });
                    }
                }
            }
            Change::Reparented { id, old_parent, new_parent } => {
                if let Some(parent) = old_parent
                    && watched.contains(parent)
                {
                    with_instance(SignalKind::ChildRemoved(*parent), *id);
                }
                if let Some(parent) = new_parent
                    && watched.contains(parent)
                {
                    with_instance(SignalKind::ChildAdded(*parent), *id);
                }
                // Everything moved (the instance and what's inside it) left the
                // old parent's ancestors and arrived in the new parent's.
                let (moved, left, arrived) = {
                    let host = host(lua);
                    let tree = host.tree.borrow();
                    let up = |from: &Option<InstanceId>| {
                        let mut chain = Vec::new();
                        let mut at = *from;
                        while let Some(a) = at {
                            if watched.contains(&a) {
                                chain.push(a);
                            }
                            at = tree.parent(a).ok().flatten();
                        }
                        chain
                    };
                    let (left, arrived) = (up(old_parent), up(new_parent));
                    let mut moved = Vec::new();
                    if !left.is_empty() || !arrived.is_empty() {
                        moved.push(*id);
                        moved.extend(tree.descendants(*id).unwrap_or_default());
                    }
                    (moved, left, arrived)
                };
                for &ancestor in &left {
                    for &item in &moved {
                        with_instance(SignalKind::DescendantRemoving(ancestor), item);
                    }
                }
                for &ancestor in &arrived {
                    for &item in &moved {
                        with_instance(SignalKind::DescendantAdded(ancestor), item);
                    }
                }
                if tools_heard {
                    let (is_tool, from, to) = {
                        let host = host(lua);
                        let tree = host.tree.borrow();
                        let held = |p: &Option<InstanceId>| p.is_some_and(|p| is_character(&tree, p));
                        (tree.class(*id) == Ok(ClassKind::Tool), held(old_parent), held(new_parent))
                    };
                    if is_tool && from && !to {
                        fire(lua, SignalKind::ToolUnequipped(*id), || Ok(MultiValue::new()));
                    }
                    if is_tool && to && !from {
                        fire(lua, SignalKind::ToolEquipped(*id), || {
                            Ok(std::iter::once(Value::UserData(lua.create_userdata(crate::items::Mouse)?))
                                .collect())
                        });
                    }
                }
            }
            Change::AttributeChanged { id, name } if watched.contains(id) => {
                fire(lua, SignalKind::AttributeChanged(*id), || {
                    Ok(std::iter::once(Value::String(lua.create_string(name)?)).collect())
                });
                if let Some(n) = interned(lua, name) {
                    fire(lua, SignalKind::AttributeNamedChanged(*id, n), || Ok(MultiValue::new()));
                }
            }
            Change::TagAdded { id, tag } if tags_heard => {
                if let Some(n) = interned(lua, tag) {
                    with_instance(SignalKind::TagAdded(n), *id);
                }
            }
            Change::TagRemoved { id, tag } if tags_heard => {
                if let Some(n) = interned(lua, tag) {
                    with_instance(SignalKind::TagRemoved(n), *id);
                }
            }
            Change::Destroyed(id) if watched.contains(id) || tags_heard => {
                if watched.contains(id) {
                    fire(lua, SignalKind::Destroying(*id), || Ok(MultiValue::new()));
                }
                if !tags_heard {
                    continue;
                }
                let tags = host(lua).tree.borrow().tags(*id).map(|t| t.to_vec()).unwrap_or_default();
                for n in tags.iter().filter_map(|t| interned(lua, t)) {
                    with_instance(SignalKind::TagRemoved(n), *id);
                }
            }
            _ => {}
        }
    }
    take_errors(lua)
}

/// Fires `MouseEnter` (or, when `entered` is false, `MouseLeave`) on a GUI object.
pub(crate) fn mouse_over(lua: &Lua, object: InstanceId, entered: bool) -> Vec<ScriptError> {
    let signal = if entered { SignalKind::MouseEnter(object) } else { SignalKind::MouseLeave(object) };
    fire(lua, signal, || Ok(MultiValue::new()));
    take_errors(lua)
}

/// Fires a text button's click events.
pub(crate) fn click_button(lua: &Lua, button: InstanceId) -> Vec<ScriptError> {
    fire(lua, SignalKind::ButtonClicked(button), || Ok(MultiValue::new()));
    fire(lua, SignalKind::ButtonActivated(button), || Ok(MultiValue::new()));
    take_errors(lua)
}

/// Fires `part.TouchEnded(other)`.
pub(crate) fn fire_touch_ended(lua: &Lua, part: InstanceId, other: InstanceId) -> Vec<ScriptError> {
    fire(lua, SignalKind::TouchEnded(part), || {
        Ok(std::iter::once(Value::UserData(push_instance(lua, other)?)).collect())
    });
    take_errors(lua)
}

/// Fires `part.Touched(other)`.
pub(crate) fn fire_touched(lua: &Lua, part: InstanceId, other: InstanceId) -> Vec<ScriptError> {
    fire(lua, SignalKind::Touched(part), || {
        Ok(std::iter::once(Value::UserData(push_instance(lua, other)?)).collect())
    });
    take_errors(lua)
}

pub(crate) fn take_errors(lua: &Lua) -> Vec<ScriptError> {
    std::mem::take(&mut *host(lua).errors.borrow_mut())
}

pub(crate) fn take_output(lua: &Lua) -> Vec<OutputLine> {
    std::mem::take(&mut *host(lua).output.borrow_mut())
}

pub(crate) fn connection_count(lua: &Lua) -> usize {
    host(lua).connections.borrow().len()
}

pub(crate) fn waiting_count(lua: &Lua) -> usize {
    host(lua).waiting.borrow().len()
}

pub(crate) fn clock(lua: &Lua) -> f64 {
    host(lua).clock.get()
}
