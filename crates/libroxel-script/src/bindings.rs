//! Lua-side types, globals and the thread scheduler. Every script-visible
//! name comes from [`crate::names`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use libroxel_instance::{ClassKind, Color3, InstanceId, Property, PropertyValue, TreeError, Vec3};
use mlua::{
    AnyUserData, Function, Lua, MetaMethod, MultiValue, RegistryKey, Table, Thread, UserData, UserDataFields,
    UserDataMethods, Value, Variadic,
};

use crate::names::{self, constructors, globals};
use crate::{OutputLevel, OutputLine, ScriptError, SharedTree};

const INSTANCE_CACHE: &str = "libroxel.instance_cache";
const WAIT_FOR_CHILD_FN: &str = "libroxel.wait_for_child";
/// How long a script may run without yielding before it is stopped.
pub(crate) const RUN_LIMIT: Duration = Duration::from_secs(5);

/// The signals a script can connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SignalKind {
    Heartbeat,
    Touched(InstanceId),
}

struct Connection {
    id: u64,
    signal: SignalKind,
    script: String,
    handler: RegistryKey,
}

/// A thread paused by `task.wait` or scheduled by `task.delay`.
struct Waiting {
    thread: Thread,
    resume_at: f64,
    since: f64,
    /// `None`: resume with the elapsed time (a wait). `Some`: resume with these (a delay).
    args: Option<Vec<Value>>,
    script: String,
}

/// Host state stored in the VM's app data.
struct Host {
    tree: SharedTree,
    connections: RefCell<Vec<Connection>>,
    next_connection: Cell<u64>,
    output: RefCell<Vec<OutputLine>>,
    errors: RefCell<Vec<ScriptError>>,
    current_script: RefCell<String>,
    clock: Cell<f64>,
    waiting: RefCell<Vec<Waiting>>,
    /// When the running thread must have yielded by. Checked by the VM's interrupt.
    deadline: Rc<Cell<Option<Instant>>>,
}

fn host(lua: &Lua) -> mlua::AppDataRef<'_, Host> {
    lua.app_data_ref::<Host>().expect("libroxel host state is installed")
}

fn tree_err(err: TreeError) -> mlua::Error {
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
fn resume(lua: &Lua, thread: Thread, args: MultiValue, script: &str) -> Option<ScriptError> {
    let previous = host(lua).current_script.replace(script.to_owned());
    let deadline = host(lua).deadline.clone();
    let previous_deadline = deadline.replace(Some(Instant::now() + RUN_LIMIT));
    let result = thread.resume::<MultiValue>(args);
    deadline.set(previous_deadline);
    *host(lua).current_script.borrow_mut() = previous;
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
                });
            }
            None
        }
        Err(err) => Some(ScriptError { script: script.to_owned(), message: err.to_string() }),
    }
}

fn spawn(lua: &Lua, func: Function, args: MultiValue, script: &str) -> mlua::Result<()> {
    let thread = lua.create_thread(func)?;
    if let Some(err) = resume(lua, thread, args, script) {
        host(lua).errors.borrow_mut().push(err);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Value types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct LuaVector3(Vec3);

#[derive(Debug, Clone, Copy)]
struct LuaColor3(Color3);

fn as_vector3(value: &Value) -> Option<Vec3> {
    match value {
        Value::UserData(ud) => ud.borrow::<LuaVector3>().ok().map(|v| v.0),
        _ => None,
    }
}

fn as_number(value: &Value) -> Option<f32> {
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
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (Value, Value)| {
            let colour = |v: &Value| match v {
                Value::UserData(ud) => ud.borrow::<LuaColor3>().ok().map(|c| c.0),
                _ => None,
            };
            Ok(matches!((colour(&a), colour(&b)), (Some(a), Some(b)) if a == b))
        });
    }
}

// ---------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct LuaInstance(InstanceId);

/// Returns the one userdata for `id`, so the same instance is always the
/// same Lua value (it works as a table key and with `rawequal`).
fn push_instance(lua: &Lua, id: InstanceId) -> mlua::Result<AnyUserData> {
    let cache: Table = lua.named_registry_value(INSTANCE_CACHE)?;
    let key = id.to_string();
    if let Some(existing) = cache.raw_get::<Option<AnyUserData>>(key.as_str())? {
        return Ok(existing);
    }
    let ud = lua.create_userdata(LuaInstance(id))?;
    cache.raw_set(key, &ud)?;
    Ok(ud)
}

fn as_instance(value: &Value) -> Option<InstanceId> {
    match value {
        Value::UserData(ud) => ud.borrow::<LuaInstance>().ok().map(|i| i.0),
        _ => None,
    }
}

fn property_to_lua(lua: &Lua, value: PropertyValue) -> mlua::Result<Value> {
    Ok(match value {
        PropertyValue::Bool(b) => Value::Boolean(b),
        PropertyValue::Number(n) => Value::Number(n),
        PropertyValue::String(s) => Value::String(lua.create_string(&s)?),
        PropertyValue::Vec3(v) => Value::UserData(lua.create_userdata(LuaVector3(v))?),
        PropertyValue::Color3(c) => Value::UserData(lua.create_userdata(LuaColor3(c))?),
    })
}

fn property_from_lua(value: &Value) -> Option<PropertyValue> {
    Some(match value {
        Value::Boolean(b) => PropertyValue::Bool(*b),
        Value::Number(n) => PropertyValue::Number(*n),
        Value::Integer(n) => PropertyValue::Number(*n as f64),
        Value::String(s) => PropertyValue::String(s.to_string_lossy()),
        Value::UserData(ud) => {
            if let Ok(v) = ud.borrow::<LuaVector3>() {
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

fn not_a_member(lua: &Lua, id: InstanceId, key: &str) -> mlua::Error {
    let host = host(lua);
    let tree = host.tree.borrow();
    let name = tree.name(id).unwrap_or("?").to_owned();
    mlua::Error::runtime(format!("{key} is not a valid member of {name}"))
}

fn class_arg(name: &str) -> mlua::Result<ClassKind> {
    names::class_from_name(name)
        .ok_or_else(|| mlua::Error::runtime(format!("{name} is not a valid class name")))
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
            let (root, workspace) = {
                let host = host(lua);
                let tree = host.tree.borrow();
                (tree.root(), tree.workspace())
            };
            if this.0 != root {
                return Err(not_a_member(lua, this.0, GET_SERVICE));
            }
            match name.as_str() {
                names::services::RUN_SERVICE => Ok(Value::UserData(lua.create_userdata(RunService)?)),
                n if n == names::class_name(ClassKind::Workspace) => {
                    Ok(Value::UserData(push_instance(lua, workspace)?))
                }
                _ => Err(mlua::Error::runtime(format!("{name} is not a valid service name"))),
            }
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
            if key == names::events::TOUCHED {
                let class = host(lua).tree.borrow().class(id).map_err(tree_err)?;
                if class.is_physical() {
                    return Ok(Value::UserData(lua.create_userdata(Signal(SignalKind::Touched(id)))?));
                }
            }
            if let Some(property) = names::property_from_name(&key) {
                let value = host(lua).tree.borrow().get(id, property);
                match value {
                    Ok(value) => return property_to_lua(lua, value),
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
            let Some(property) = names::property_from_name(&key) else {
                return Err(not_a_member(lua, id, &key));
            };
            let value = property_from_lua(&value).ok_or_else(|| {
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
    }
}

#[derive(Clone, Copy)]
struct Signal(SignalKind);

impl UserData for Signal {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(names::signal::CONNECT, |lua, this, handler: Function| {
            let handler = lua.create_registry_value(handler)?;
            let host = host(lua);
            let id = host.next_connection.get();
            host.next_connection.set(id + 1);
            let script = host.current_script.borrow().clone();
            host.connections.borrow_mut().push(Connection { id, signal: this.0, script, handler });
            Ok(ConnectionHandle(id))
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

fn fire(lua: &Lua, signal: SignalKind, args: impl Fn() -> mlua::Result<MultiValue>) {
    // Snapshot handlers first: a handler may connect or disconnect others.
    let handlers: Vec<(String, mlua::Result<Function>)> = host(lua)
        .connections
        .borrow()
        .iter()
        .filter(|c| c.signal == signal)
        .map(|c| (c.script.clone(), lua.registry_value::<Function>(&c.handler)))
        .collect();
    for (script, handler) in handlers {
        let result = handler.and_then(|f| spawn(lua, f, args()?, &script));
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
        local lib = ...
        lib["{wait}"] = function(seconds) return yield(seconds or 0) end
        local wait = lib["{wait}"]
        return wait, function(self, name, timeout)
            local waited = 0
            while true do
                local child = self:{find}(name)
                if child then return child end
                if timeout and waited >= timeout then return nil end
                waited += wait()
            end
        end
        "#,
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
        clock: Cell::new(0.0),
        waiting: RefCell::new(Vec::new()),
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
            spawn(lua, func, args.into_iter().collect(), &script)
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
            });
            Ok(())
        })?,
    )?;
    let (wait, wait_for_child): (Function, Function) =
        lua.load(prelude()).set_name("=libroxel").call(task_lib.clone())?;
    lua.set_named_registry_value(WAIT_FOR_CHILD_FN, wait_for_child)?;
    g.set(globals::TASK, task_lib)?;
    g.set(globals::WAIT, wait)?;
    g.set(globals::TIME, lua.create_function(|lua, ()| Ok(host(lua).clock.get()))?)?;

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

/// Runs a chunk as a new thread in its own environment. Errors raised before
/// the script first yields are returned; later ones surface from `step`.
pub(crate) fn run_chunk(
    lua: &Lua,
    script_name: &str,
    source: &str,
    script_instance: Option<InstanceId>,
) -> Result<(), ScriptError> {
    let fail = |err: mlua::Error| ScriptError { script: script_name.to_owned(), message: err.to_string() };
    // Each script gets its own global environment that falls back to the
    // shared, read-only globals, so scripts can't leak globals into each other.
    let env = lua.create_table().map_err(fail)?;
    let meta = lua.create_table().map_err(fail)?;
    meta.set("__index", lua.globals()).map_err(fail)?;
    env.set_metatable(Some(meta)).map_err(fail)?;
    if let Some(id) = script_instance {
        env.raw_set(globals::SCRIPT, push_instance(lua, id).map_err(fail)?).map_err(fail)?;
    }
    let func = lua
        .load(source)
        .set_name(format!("={script_name}"))
        .set_environment(env)
        .into_function()
        .map_err(fail)?;
    let thread = lua.create_thread(func).map_err(fail)?;
    match resume(lua, thread, MultiValue::new(), script_name) {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Runs every enabled script instance in the tree, in tree order.
pub(crate) fn run_tree_scripts(lua: &Lua) -> Vec<ScriptError> {
    let scripts: Vec<(InstanceId, String, String)> = {
        let host = host(lua);
        let tree = host.tree.borrow();
        tree.descendants_of_class(tree.root(), ClassKind::Script)
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
    for w in due {
        let args: MultiValue = match w.args {
            Some(args) => args.into_iter().collect(),
            None => std::iter::once(Value::Number(now - w.since)).collect(),
        };
        if let Some(err) = resume(lua, w.thread, args, &w.script) {
            host(lua).errors.borrow_mut().push(err);
        }
    }
    fire(lua, SignalKind::Heartbeat, || Ok(std::iter::once(Value::Number(dt)).collect()));
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
