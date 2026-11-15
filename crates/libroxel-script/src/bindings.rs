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
    AnyUserData, Function, Lua, MetaMethod, MultiValue, ObjectLike, RegistryKey, Table, Thread, UserData,
    UserDataFields, UserDataMethods, Value, Variadic,
};

use crate::names::{self, constructors, globals};
use crate::{OutputLevel, OutputLine, ScriptError, SharedTree};

const INSTANCE_CACHE: &str = "libroxel.instance_cache";
const WAIT_FOR_CHILD_FN: &str = "libroxel.wait_for_child";
const SIGNAL_WAIT_FN: &str = "libroxel.signal_wait";
/// How long a script may run without yielding before it is stopped.
pub(crate) const RUN_LIMIT: Duration = Duration::from_secs(5);

/// The signals a script can connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    AncestryChanged(InstanceId),
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
            | SignalKind::AncestryChanged(id)
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

/// Every connection, kept by signal so firing one doesn't look at the rest.
#[derive(Default)]
pub(crate) struct Connections {
    by_signal: std::collections::HashMap<SignalKind, Vec<Connection>>,
    signal_of: std::collections::HashMap<u64, SignalKind>,
    /// How many connections follow each instance's changes.
    watched: std::collections::HashMap<InstanceId, usize>,
    tags: usize,
    tools: usize,
}

impl Connections {
    fn count(&mut self, signal: SignalKind, by: isize) {
        let bump = |n: &mut usize| *n = n.saturating_add_signed(by);
        if let Some(id) = signal.watched() {
            let n = self.watched.entry(id).or_default();
            bump(n);
            if *n == 0 {
                self.watched.remove(&id);
            }
        }
        match signal {
            SignalKind::TagAdded(_) | SignalKind::TagRemoved(_) => bump(&mut self.tags),
            SignalKind::ToolEquipped(_) | SignalKind::ToolUnequipped(_) => bump(&mut self.tools),
            _ => {}
        }
    }

    fn push(&mut self, conn: Connection) {
        self.count(conn.signal, 1);
        self.signal_of.insert(conn.id, conn.signal);
        self.by_signal.entry(conn.signal).or_default().push(conn);
    }

    fn contains(&self, id: u64) -> bool {
        self.signal_of.contains_key(&id)
    }

    fn remove(&mut self, id: u64) -> Option<Connection> {
        let signal = self.signal_of.remove(&id)?;
        let list = self.by_signal.get_mut(&signal)?;
        let at = list.iter().position(|c| c.id == id)?;
        let conn = list.remove(at);
        if list.is_empty() {
            self.by_signal.remove(&signal);
        }
        self.count(signal, -1);
        Some(conn)
    }

    fn of(&self, signal: SignalKind) -> &[Connection] {
        self.by_signal.get(&signal).map_or(&[], Vec::as_slice)
    }

    /// Takes out the connections `keep` says no to.
    fn take_where(&mut self, mut gone: impl FnMut(&Connection) -> bool) -> Vec<Connection> {
        let mut out = Vec::new();
        for list in self.by_signal.values_mut() {
            if list.iter().any(&mut gone) {
                let (g, kept): (Vec<_>, Vec<_>) = std::mem::take(list).into_iter().partition(&mut gone);
                *list = kept;
                out.extend(g);
            }
        }
        self.by_signal.retain(|_, l| !l.is_empty());
        for c in &out {
            self.signal_of.remove(&c.id);
            self.count(c.signal, -1);
        }
        out
    }

    /// Takes out a signal's one-shot connections.
    fn take_once(&mut self, signal: SignalKind) -> Vec<Connection> {
        let Some(list) = self.by_signal.get_mut(&signal) else { return Vec::new() };
        if !list.iter().any(|c| c.once) {
            return Vec::new();
        }
        let (once, kept): (Vec<_>, Vec<_>) = std::mem::take(list).into_iter().partition(|c| c.once);
        *list = kept;
        if list.is_empty() {
            self.by_signal.remove(&signal);
        }
        for c in &once {
            self.signal_of.remove(&c.id);
            self.count(c.signal, -1);
        }
        once
    }

    fn len(&self) -> usize {
        self.signal_of.len()
    }

    fn is_watched(&self, id: &InstanceId) -> bool {
        self.watched.contains_key(id)
    }
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
    /// An Actor's message handler: on Roblox it runs on another core, so
    /// it is never held back for the frame's sake.
    parallel: bool,
}

/// Watches the running script's deadline from another thread, and raises a
/// flag once it has passed so that Luau's interrupt asks whether to stop it.
pub(crate) struct Watchdog {
    start: Instant,
    /// Milliseconds after `start` the running script must stop by; 0: none.
    deadline_ms: std::sync::atomic::AtomicU64,
    overdue: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Watchdog {
    /// A watchdog with its own thread, which ends when the watchdog is dropped.
    fn start() -> std::sync::Arc<Self> {
        let dog = std::sync::Arc::new(Self {
            start: Instant::now(),
            deadline_ms: std::sync::atomic::AtomicU64::new(0),
            overdue: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });
        let weak = std::sync::Arc::downgrade(&dog);
        let _ = std::thread::Builder::new().name("script watchdog".into()).spawn(move || {
            while let Some(dog) = weak.upgrade() {
                let due = dog.deadline_ms.load(std::sync::atomic::Ordering::Relaxed);
                if due != 0 && dog.start.elapsed().as_millis() as u64 > due {
                    dog.overdue.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                drop(dog);
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        dog
    }

    fn overdue_flag(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.overdue.clone()
    }

    /// The running script must stop by `due` (or `None`: nothing is running).
    fn set_deadline(&self, due: Option<Instant>) {
        let ms = due.map_or(0, |d| d.saturating_duration_since(self.start).as_millis().max(1) as u64);
        self.deadline_ms.store(ms, std::sync::atomic::Ordering::Relaxed);
        self.overdue.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Host state stored in the VM's app data.
pub(crate) struct Host {
    /// The weak-valued table of each instance's one userdata, by its id's
    /// bits (kept here so finding it doesn't go through the registry).
    instance_cache: RefCell<Option<Table>>,
    /// A Luau function that turns a list of instance numbers into their
    /// objects in place (see [`instance_list`]).
    fill_instances: RefCell<Option<Function>>,
    pub(crate) tree: SharedTree,
    connections: RefCell<Connections>,
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
    watchdog: std::sync::Arc<Watchdog>,
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
    /// Scripts that have been started, so none starts twice.
    running_scripts: RefCell<std::collections::HashSet<InstanceId>>,
    /// Scripts enabled while the game runs, started on the next step.
    to_start: RefCell<Vec<InstanceId>>,
    /// How long each script has run, and how many times it was resumed.
    pub(crate) script_time: RefCell<std::collections::HashMap<String, (f64, u64)>>,
    /// Finished threads kept to run the next handler in.
    pub(crate) spare_threads: RefCell<Vec<Thread>>,
    /// How long each waiting thread ran the last time it was resumed from a
    /// wait (by its pointer), to tell heavy background work from the rest.
    thread_cost: RefCell<std::collections::HashMap<usize, Duration>>,
    /// How long each script's last resume from a wait took, for its threads
    /// not seen yet (a `pcall` or `task.spawn` inside a long loop makes a new
    /// thread for every piece of the work).
    script_cost: RefCell<std::collections::HashMap<String, Duration>>,
    /// Heavy background work run beyond its share of recent frames: each
    /// frame pays [`RESUME_BUDGET`] of it off, and heavy work waits while
    /// any is owed.
    heavy_debt: Cell<Duration>,
    /// Where the workspace's parts are, for raycasts and part queries.
    pub(crate) part_index: RefCell<libroxel_instance::geometry::PartIndex>,
}

/// The part index, caught up with the tree.
pub(crate) fn part_index<'a>(
    host: &'a Host,
    tree: &libroxel_instance::InstanceTree,
) -> std::cell::RefMut<'a, libroxel_instance::geometry::PartIndex> {
    let mut index = host.part_index.borrow_mut();
    index.refresh(tree);
    index
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
    let due = Instant::now() + RUN_LIMIT;
    let previous_deadline = deadline.replace(Some(due));
    host(lua).watchdog.set_deadline(Some(due));
    let began = Instant::now();
    let result = thread.resume::<MultiValue>(args);
    let took = began.elapsed().as_secs_f64();
    deadline.set(previous_deadline);
    host(lua).watchdog.set_deadline(previous_deadline);
    {
        let host = host(lua);
        let mut times = host.script_time.borrow_mut();
        // A nested resume's time is counted in its caller's too.
        match times.get_mut(script) {
            Some(entry) => {
                entry.0 += took;
                entry.1 += 1;
            }
            None => {
                times.insert(script.to_owned(), (took, 1));
            }
        }
    }
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
                    parallel: false,
                });
            }
            None
        }
        Err(err) => Some(ScriptError { script: script.to_owned(), message: err.to_string() }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Server,
    Client,
}

/// Which side the running code belongs to, by the script that started it:
/// a local script's code (and modules it requires) is the client's, a
/// script's is the server's. None for code no script started.
fn running_side(lua: &Lua) -> Option<Side> {
    let owner = host(lua).current_owner.get()?;
    match host(lua).tree.borrow().class(owner).ok()? {
        ClassKind::LocalScript => Some(Side::Client),
        ClassKind::Script => Some(Side::Server),
        _ => None,
    }
}

pub(crate) fn spawn(
    lua: &Lua,
    func: Function,
    args: MultiValue,
    script: &str,
    owner: Option<InstanceId>,
) -> mlua::Result<()> {
    // Threads that ran to the end are reused: making a new one for every
    // event handler was most of what firing an event cost.
    let reused = host(lua).spare_threads.borrow_mut().pop();
    let thread = match reused {
        Some(thread) if thread.reset(func.clone()).is_ok() => thread,
        _ => lua.create_thread(func)?,
    };
    if let Some(err) = resume(lua, thread.clone(), args, script, owner) {
        host(lua).errors.borrow_mut().push(err);
    } else if !thread.is_resumable() {
        let host = host(lua);
        let mut spare = host.spare_threads.borrow_mut();
        if spare.len() < 64 {
            spare.push(thread);
        }
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
        Value::Vector(v) => Some(Vec3::new(v.x(), v.y(), v.z())),
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

/// Like Roblox, a Vector3 is Luau's built-in vector: made, read and
/// added up by the VM itself, with no allocation. Its members (Magnitude,
/// Unit, Dot...) come from the vector type's metatable, set up in
/// [`install_vector3`].
impl mlua::IntoLua for LuaVector3 {
    fn into_lua(self, _: &Lua) -> mlua::Result<Value> {
        Ok(vector_value(self.0))
    }
}

impl mlua::FromLua for LuaVector3 {
    fn from_lua(value: Value, _: &Lua) -> mlua::Result<Self> {
        as_vector3(&value)
            .map(LuaVector3)
            .ok_or_else(|| mlua::Error::runtime(format!("expected a Vector3, got a {}", value.type_name())))
    }
}

/// How a Vector3 prints: "1, 2.5, 3".
pub(crate) fn vector_text(v: mlua::Vector) -> String {
    format!("{}, {}, {}", v.x(), v.y(), v.z())
}

/// A Vector3 as a script value.
pub(crate) fn vector_value(v: Vec3) -> Value {
    Value::Vector(mlua::Vector::new(v.x, v.y, v.z))
}

/// Vector3's members, on Luau's vector type (so every vector has them), and
/// `typeof` saying "Vector3" for one, as in Roblox.
fn install_vector3(lua: &Lua) -> mlua::Result<()> {
    let mt: Table = lua.load(VECTOR3_MEMBERS).set_name("=Vector3").eval()?;
    mt.set("__tostring", lua.create_function(|_, v: mlua::Vector| Ok(vector_text(v)))?)?;
    lua.set_type_metatable::<mlua::Vector>(Some(mt));
    Ok(())
}

const VECTOR3_MEMBERS: &str = r##"
local vector, error, type, select, math = vector, error, type, select, math
local abs = math.abs
local zero = vector.create(0, 0, 0)
local function check(v, what)
	if type(v) ~= "vector" then
		error(what .. " expects a Vector3", 3)
	end
end
local methods = {}
function methods.Dot(a, b) check(b, "Dot"); return vector.dot(a, b) end
function methods.Cross(a, b) check(b, "Cross"); return vector.cross(a, b) end
function methods.Lerp(a, b, t) check(b, "Lerp"); return a + (b - a) * t end
function methods.Min(a, ...)
	for i = 1, select("#", ...) do local b = select(i, ...); check(b, "Min"); a = vector.min(a, b) end
	return a
end
function methods.Max(a, ...)
	for i = 1, select("#", ...) do local b = select(i, ...); check(b, "Max"); a = vector.max(a, b) end
	return a
end
function methods.Abs(a) return vector.abs(a) end
function methods.Floor(a) return vector.floor(a) end
function methods.Ceil(a) return vector.ceil(a) end
function methods.Sign(a) return vector.sign(a) end
function methods.FuzzyEq(a, b, eps)
	check(b, "FuzzyEq")
	eps = eps or 1e-5
	local d = a - b
	return abs(d.x) <= eps and abs(d.y) <= eps and abs(d.z) <= eps
end
function methods.Angle(a, b, axis)
	check(b, "Angle")
	local m = vector.magnitude(a) * vector.magnitude(b)
	if m == 0 then return 0 end
	local angle = math.acos(math.clamp(vector.dot(a, b) / m, -1, 1))
	if type(axis) == "vector" and vector.dot(vector.cross(a, b), axis) < 0 then
		return -angle
	end
	return angle
end
return {
	__index = function(v, key)
		if key == "Magnitude" then
			return vector.magnitude(v)
		elseif key == "Unit" then
			if v == zero then return v end
			return vector.normalize(v)
		end
		local m = methods[key]
		if m then return m end
		error(tostring(key) .. " is not a valid member of Vector3", 2)
	end,
	__type = "Vector3",
}
"##;

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
        // An item of this enumeration that libroxel doesn't draw differently
        // (such as ScaleType.Slice): the closest it has, its first.
        Value::Table(t) => {
            let of_kind = t
                .raw_get::<Value>("EnumType")
                .ok()
                .and_then(|e| e.to_string().ok())
                .is_some_and(|e| e == names::enum_name(kind));
            if !of_kind {
                return None;
            }
            let name: String = t.raw_get("Name").ok()?;
            names::enum_item_from_name(kind, &name).or_else(|| kind.items().first().copied())
        }
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
    let cache = match host(lua).instance_cache.borrow().clone() {
        Some(cache) => cache,
        None => lua.named_registry_value(INSTANCE_CACHE)?,
    };
    // A number key: no string to build for each lookup (ids fit in a double).
    let key = id.to_bits() as f64;
    if let Some(existing) = cache.raw_get::<Option<AnyUserData>>(key)? {
        return Ok(existing);
    }
    let ud = lua.create_userdata(LuaInstance(id))?;
    cache.raw_set(key, &ud)?;
    Ok(ud)
}

/// A script table holding the objects for `ids`, in order.
pub(crate) fn instance_list(lua: &Lua, ids: Vec<InstanceId>) -> mlua::Result<Table> {
    let out = lua.create_sequence_from(ids.into_iter().map(|id| id.to_bits() as f64))?;
    let fill = host(lua).fill_instances.borrow().clone();
    match fill {
        Some(fill) => fill.call::<()>(&out)?,
        None => {
            for i in 1..=out.raw_len() {
                let key: f64 = out.raw_get(i)?;
                out.raw_set(i, push_instance(lua, InstanceId::from_bits(key as u64))?)?;
            }
        }
    }
    Ok(out)
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
        PropertyValue::Vec3(v) => vector_value(v),
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
        Value::Vector(_) => PropertyValue::Vec3(as_vector3(value)?),
        Value::UserData(ud) => {
            if let Some(v) = as_vector2(value) {
                PropertyValue::Vec2(v)
            } else if let Some(u) = as_udim2(value) {
                PropertyValue::UDim2(u)
            } else if let Some(u) = crate::types::as_udim(value) {
                PropertyValue::UDim(u)
            } else if let Ok(c) = ud.borrow::<LuaColor3>() {
                PropertyValue::Color3(c.0)
            } else {
                // A named colour where a colour goes, such as a team's.
                PropertyValue::Color3(crate::compat::brick_colour(value)?)
            }
        }
        _ => return None,
    })
}

/// Roblox's error for a member an instance doesn't have, naming its class
/// and where it is: `Foo is not a valid member of Part "Workspace.Crate"`.
/// Where a part's pivot is, from its centre (its PivotOffset).
fn pivot_offset(lua: &Lua, id: InstanceId) -> mlua::Result<CFrame> {
    let offset = push_instance(lua, id)?.get::<Value>("PivotOffset").ok();
    Ok(offset.as_ref().and_then(as_cframe).unwrap_or(CFrame::IDENTITY))
}

/// Instances parented to an object libroxel only stands in for (one made
/// in Lua), by instance.
fn stand_in_parents(lua: &Lua) -> mlua::Result<Table> {
    const KEY: &str = "libroxel.stand_in_parents";
    if let Ok(t) = lua.named_registry_value::<Table>(KEY) {
        return Ok(t);
    }
    let t = lua.create_table()?;
    lua.set_named_registry_value(KEY, &t)?;
    Ok(t)
}

/// See [`crate::ScriptRuntime::waiting_report`].
pub(crate) fn waiting_report(lua: &Lua) -> Vec<crate::WaitingThread> {
    let host = host(lua);
    let now = host.clock.get();
    let traceback: Option<Function> =
        lua.globals().get::<Table>("debug").ok().and_then(|d| d.get::<Function>("traceback").ok());
    let mut out: Vec<crate::WaitingThread> = Vec::new();
    for w in host.waiting.borrow().iter() {
        let trace =
            traceback.as_ref().and_then(|tb| tb.call::<String>((w.thread.clone(),)).ok()).unwrap_or_default();
        let trace = trace
            .lines()
            .map(str::trim)
            .filter(|l| {
                !l.is_empty()
                    && *l != "[C]: in ?"
                    && !l.starts_with("__mlua")
                    && !l.starts_with("stack traceback")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let seconds = now - w.since;
        match out.iter_mut().find(|o| o.script == w.script && o.trace == trace) {
            Some(o) => {
                o.count += 1;
                o.seconds = o.seconds.max(seconds);
            }
            None => out.push(crate::WaitingThread { script: w.script.clone(), trace, count: 1, seconds }),
        }
    }
    out.sort_by(|a, b| b.seconds.total_cmp(&a.seconds));
    out
}

pub(crate) fn not_a_member(lua: &Lua, id: InstanceId, key: &str) -> mlua::Error {
    let class = class_of(lua, id).map(|c| crate::lenient::roblox_class(lua, id, c)).unwrap_or_default();
    let host = host(lua);
    let tree = host.tree.borrow();
    let mut parts = Vec::new();
    let mut current = Some(id);
    while let Some(at) = current {
        if at == tree.root() {
            break;
        }
        parts.push(tree.name(at).unwrap_or("?").to_owned());
        current = tree.parent(at).ok().flatten();
    }
    parts.reverse();
    mlua::Error::runtime(format!("{key} is not a valid member of {class} \"{}\"", parts.join(".")))
}

/// Whether instance `id` is a `name`, a class or family name, as
/// `IsA` answers.
pub(crate) fn is_a(lua: &Lua, id: InstanceId, name: &str) -> mlua::Result<bool> {
    let class = class_of(lua, id)?;
    // The usual case first, without building the class name.
    let plain = class != ClassKind::Other
        && host(lua).tree.borrow().extras(id).map_or(true, |e| !e.contains_key(names::STAND_IN));
    if plain {
        if let Some(answer) = names::is_a_family(class, name) {
            return Ok(answer);
        }
        return Ok(match names::class_from_name(name) {
            Some(other) => class.is_a(other),
            None => crate::reflection::class_is_a(names::class_name(class), name),
        });
    }
    let roblox = crate::lenient::roblox_class(lua, id, class);
    if roblox != names::class_name(class) {
        return Ok(name == "Instance" || crate::reflection::class_is_a(&roblox, name));
    }
    if let Some(answer) = names::is_a_family(class, name) {
        return Ok(answer);
    }
    match names::class_from_name(name) {
        Some(other) => Ok(class.is_a(other)),
        // Families libroxel doesn't name, such as PVInstance's relatives.
        None => Ok(crate::reflection::class_is_a(names::class_name(class), name)),
    }
}

/// Whether instance `id`'s class is exactly `name`, as `FindFirstChildOfClass` asks.
pub(crate) fn is_class(lua: &Lua, id: InstanceId, name: &str) -> mlua::Result<bool> {
    let class = class_of(lua, id)?;
    Ok(crate::lenient::roblox_class(lua, id, class) == name)
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
        ANCESTRY_CHANGED => SignalKind::AncestryChanged(id),
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
    if let Some(primary) = tree.get_ref(model, Property::PrimaryPart)
        && tree.is_descendant_of(primary, model).unwrap_or(false)
    {
        return Ok(Some(primary));
    }
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
            let children = host(lua).tree.borrow().children(this.0).map_err(tree_err)?.to_vec();
            for child in children {
                if is_class(lua, child, &class)? {
                    return push_instance(lua, child).map(Some);
                }
            }
            Ok(None)
        });
        methods.add_method(GET_CHILDREN, |lua, this, ()| {
            let children = host(lua).tree.borrow().children(this.0).map_err(tree_err)?.to_vec();
            instance_list(lua, children)
        });
        methods.add_method(GET_DESCENDANTS, |lua, this, ()| {
            let all = {
                let host = host(lua);
                let tree = host.tree.borrow();
                // The workspace's list is kept by the tree between changes.
                if this.0 == tree.workspace() {
                    tree.workspace_descendants()
                } else {
                    tree.descendants(this.0).map_err(tree_err)?
                }
            };
            instance_list(lua, all)
        });
        methods.add_method(IS_A, |lua, this, name: String| is_a(lua, this.0, &name));
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
            if let Some(service) = crate::lenient::service(lua, &name)? {
                return Ok(service);
            }
            let service = names::class_from_name(names::service_alias(&name))
                .and_then(|c| host(lua).tree.borrow().service(c));
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
            if let Some(property) = names::property_from_name(class, &name) {
                return Ok(Value::UserData(
                    lua.create_userdata(Signal(SignalKind::PropertyChanged(this.0, property)))?,
                ));
            }
            // Members kept beyond the saved properties (CurrentCamera,
            // ViewportSize, AbsoluteSize, values the class database knows):
            // an event that fires when a script or libroxel changes them.
            let roblox = crate::lenient::roblox_class(lua, this.0, class);
            let known = crate::reflection::has_property(&roblox, &name)
                || push_instance(lua, this.0)?.get::<Value>(name.as_str()).is_ok();
            if !known {
                return Err(not_a_member(lua, this.0, &name));
            }
            crate::lenient::changed_signal(lua, this.0, &name)
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
            if class_of(lua, this.0)?.is_physical() {
                return Ok(LuaCFrame(part_cframe(lua, this.0)? * pivot_offset(lua, this.0)?));
            }
            require_class(lua, this.0, ClassKind::Model, names::methods::GET_PIVOT)?;
            match model_pivot_part(lua, this.0)? {
                Some(part) => Ok(LuaCFrame(part_cframe(lua, part)?)),
                None => Ok(LuaCFrame(CFrame::IDENTITY)),
            }
        });
        methods.add_method(names::methods::PIVOT_TO, |lua, this, target: Value| {
            let target =
                as_cframe(&target).ok_or_else(|| mlua::Error::runtime("PivotTo expects a CFrame"))?;
            if class_of(lua, this.0)?.is_physical() {
                return set_part_cframe(lua, this.0, target * pivot_offset(lua, this.0)?.inverse());
            }
            require_class(lua, this.0, ClassKind::Model, names::methods::PIVOT_TO)?;
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
            // A force field on the character takes the hit instead.
            let model = tree.parent(this.0).ok().flatten();
            if model.is_some_and(|m| {
                tree.find_first_child_of_class(m, ClassKind::ForceField).ok().flatten().is_some()
            }) {
                return Ok(());
            }
            let health = tree.get_number(this.0, Property::Health).unwrap_or(0.0);
            let health = (health - amount.max(0.0)).max(0.0);
            tree.set(this.0, Property::Health, PropertyValue::Number(health)).map_err(tree_err)
        });

        methods.add_meta_method(MetaMethod::Index, |lua, this, key: mlua::LuaString| {
            let id = this.0;
            let key = key.to_str()?;
            let key: &str = &key;
            // The most read member of all, answered first.
            if key == "Name" {
                let host = host(lua);
                let tree = host.tree.borrow();
                return Ok(Value::String(lua.create_string(tree.name(id).map_err(tree_err)?)?));
            }
            if key == CLASS_NAME {
                let class = host(lua).tree.borrow().class(id).map_err(tree_err)?;
                let name = crate::lenient::roblox_class(lua, id, class);
                return Ok(Value::String(lua.create_string(name)?));
            }
            if key == PARENT {
                let parent = host(lua).tree.borrow().parent(id).map_err(tree_err)?;
                return match parent {
                    Some(p) => Ok(Value::UserData(push_instance(lua, p)?)),
                    None => stand_in_parents(lua)?.raw_get(id.to_bits().to_string()),
                };
            }
            if key == WAIT_FOR_CHILD {
                return lua.named_registry_value::<Value>(WAIT_FOR_CHILD_FN);
            }
            let class = class_of(lua, id)?;
            if key == names::PART_CFRAME && has_cframe(class) {
                return Ok(Value::UserData(lua.create_userdata(LuaCFrame(part_cframe(lua, id)?))?));
            }
            if let Some(value) = crate::world::index(lua, id, class, key)? {
                return Ok(value);
            }
            if let Some(value) = crate::compat::index(lua, id, class, key)? {
                return Ok(value);
            }
            if let Some(value) = crate::calls::index(lua, id, class, key)? {
                return Ok(value);
            }
            if let Some(value) = crate::sound::index(lua, class, key)? {
                return Ok(value);
            }
            if let Some(value) = crate::animation::index(lua, id, class, key)? {
                return Ok(value);
            }
            if key == names::methods::LOCAL_PLAYER && class == ClassKind::Players {
                return match crate::services::local_player(lua) {
                    Some(p) => Ok(Value::UserData(push_instance(lua, p)?)),
                    None => Ok(Value::Nil),
                };
            }
            if let Some(signal) = instance_event(class, id, key) {
                return Ok(Value::UserData(lua.create_userdata(Signal(signal))?));
            }
            if let Some(property) = names::property_from_name(class, key) {
                let value = host(lua).tree.borrow().get(id, property);
                match value {
                    Ok(value) => {
                        if let Some(special) = crate::effects::to_lua(lua, property, &value)? {
                            return Ok(special);
                        }
                        // Team colours are named colours.
                        if property == Property::TeamColor
                            && let PropertyValue::Color3(c) = value
                        {
                            return crate::compat::brick_for(lua, c);
                        }
                        return property_to_lua(lua, value);
                    }
                    Err(TreeError::NoSuchProperty { .. }) => {} // fall through to children
                    Err(err) => return Err(tree_err(err)),
                }
            }
            // `instance.ChildName` finds a child by name.
            let child = host(lua).tree.borrow().find_first_child(id, key).map_err(tree_err)?;
            if let Some(child) = child {
                return Ok(Value::UserData(push_instance(lua, child)?));
            }
            match crate::lenient::index(lua, id, class, push_instance(lua, id)?, key)? {
                Some(value) => Ok(value),
                None => Err(not_a_member(lua, id, key)),
            }
        });

        methods.add_meta_method(MetaMethod::NewIndex, |lua, this, (key, value): (String, Value)| {
            let id = this.0;
            if key == CLASS_NAME {
                return Err(mlua::Error::runtime("ClassName can't be changed"));
            }
            if key == PARENT {
                let parent = match &value {
                    Value::Nil => None,
                    // A service or settings object libroxel only stands in
                    // for: the instance goes out of the tree, but reads
                    // back as being in it.
                    Value::Table(_) => {
                        host(lua).tree.borrow_mut().set_parent(id, None).map_err(tree_err)?;
                        return stand_in_parents(lua)?.raw_set(id.to_bits().to_string(), value);
                    }
                    other => Some(
                        as_instance(other)
                            .ok_or_else(|| mlua::Error::runtime("Parent must be an Instance or nil"))?,
                    ),
                };
                stand_in_parents(lua)?.raw_set(id.to_bits().to_string(), Value::Nil)?;
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
                if crate::lenient::new_index(lua, id, class, push_instance(lua, id)?, &key, value)? {
                    return Ok(());
                }
                return Err(not_a_member(lua, id, &key));
            };
            let value = property_from_lua(property, &value).ok_or_else(|| {
                mlua::Error::runtime(format!(
                    "{key} expects a {}, got a {}",
                    names::value_kind_name(property.expected_kind()),
                    value.type_name()
                ))
            })?;
            host(lua).tree.borrow_mut().set(id, property, value).map_err(tree_err)?;
            if property == Property::Disabled && is_script_class(class) {
                script_toggled(lua, id);
            }
            Ok(())
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
        methods.add_method(IS_CLIENT, |lua, _, ()| {
            Ok(crate::net::role(lua) != NetRole::Server && running_side(lua) != Some(Side::Server))
        });
        methods.add_method(IS_SERVER, |lua, _, ()| {
            Ok(crate::net::role(lua) != NetRole::Client && running_side(lua) != Some(Side::Client))
        });
        methods.add_method(IS_STUDIO, |lua, _, ()| Ok(crate::net::role(lua) == NetRole::Solo));
        crate::lenient::add_service_members(methods, names::services::RUN_SERVICE);
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
    /// A member libroxel keeps by name (such as a frame's Rotation), read
    /// and set the way a script would.
    Named(&'static str),
}

/// A member name kept for as long as the game runs, so tween targets stay
/// small. The same name is kept once.
fn member_name(name: &str) -> &'static str {
    thread_local! {
        static NAMES: RefCell<std::collections::HashSet<&'static str>> = RefCell::default();
    }
    NAMES.with(|names| {
        let mut names = names.borrow_mut();
        if let Some(n) = names.get(name) {
            return *n;
        }
        let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
        names.insert(leaked);
        leaked
    })
}

impl TweenValue {
    fn from_lua(value: &Value) -> Option<TweenValue> {
        Some(match value {
            Value::Integer(n) => TweenValue::Number(*n as f64),
            Value::Number(n) => TweenValue::Number(*n),
            Value::Vector(_) => TweenValue::Vec3(as_vector3(value)?),
            Value::UserData(ud) => {
                if let Ok(c) = ud.borrow::<LuaColor3>() {
                    TweenValue::Color(c.0)
                } else if let Ok(c) = ud.borrow::<LuaCFrame>() {
                    TweenValue::CFrame(c.0)
                } else if let Some(v) = as_vector2(value) {
                    TweenValue::Vec2(v)
                } else if let Some(u) = as_udim2(value) {
                    TweenValue::UDim2(u)
                } else {
                    TweenValue::UDim(crate::types::as_udim(value)?)
                }
            }
            _ => return None,
        })
    }

    fn to_lua(self, lua: &Lua) -> mlua::Result<Value> {
        Ok(match self {
            TweenValue::Number(n) => Value::Number(n),
            TweenValue::Vec3(v) => vector_value(v),
            TweenValue::Color(c) => Value::UserData(lua.create_userdata(LuaColor3(c))?),
            TweenValue::CFrame(c) => Value::UserData(lua.create_userdata(LuaCFrame(c))?),
            TweenValue::Vec2(v) => Value::UserData(lua.create_userdata(LuaVector2(v))?),
            TweenValue::UDim2(u) => Value::UserData(lua.create_userdata(LuaUDim2(u))?),
            TweenValue::UDim(u) => Value::UserData(lua.create_userdata(crate::types::LuaUDim(u))?),
        })
    }
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
        crate::lenient::add_service_members(methods, "TweenService");
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
        TweenTarget::Named(name) => {
            let obj = push_instance(lua, id)?;
            TweenValue::from_lua(&obj.get::<Value>(name)?)
        }
    })
}

fn write_tween_value(lua: &Lua, id: InstanceId, target: TweenTarget, value: TweenValue) {
    let _ = match (target, value) {
        (TweenTarget::PartCFrame, TweenValue::CFrame(cf)) => set_part_cframe(lua, id, cf),
        (TweenTarget::Named(name), v) => {
            v.to_lua(lua).and_then(|v| push_instance(lua, id).and_then(|obj| obj.set(name, v)))
        }
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
        let Some(property) = names::property_from_name(class, &key) else {
            // Anything else a script can read and set as a number, vector or colour.
            let current: Value = push_instance(lua, id)?.get(key.as_str())?;
            let goal = TweenValue::from_lua(&value)
                .filter(|_| TweenValue::from_lua(&current).is_some())
                .ok_or_else(|| mlua::Error::runtime(format!("{key} can't be tweened to that value")))?;
            list.push((TweenTarget::Named(member_name(&key)), None, goal));
            continue;
        };
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
            Ok(host(lua).connections.borrow().contains(this.0))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(names::signal::DISCONNECT, |lua, this, ()| {
            let removed = host(lua).connections.borrow_mut().remove(this.0);
            if let Some(conn) = removed {
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
            .of(signal)
            .iter()
            .filter(|c| owner_alive(&tree, c.owner))
            .map(|c| (c.script.clone(), c.owner, lua.registry_value::<Function>(&c.handler)))
            .collect()
    };
    // One-shot connections are gone once they have fired.
    let spent = host(lua).connections.borrow_mut().take_once(signal);
    for conn in spent {
        let _ = lua.remove_registry_value(conn.handler);
    }
    if handlers.is_empty() {
        return;
    }
    // The same arguments for every handler (Roblox passes the same values).
    let args = match args() {
        Ok(a) => a,
        Err(err) => {
            host(lua)
                .errors
                .borrow_mut()
                .push(ScriptError { script: String::new(), message: err.to_string() });
            return;
        }
    };
    for (script, owner, handler) in handlers {
        let result = handler.and_then(|f| spawn(lua, f, args.clone(), &script, owner));
        if let Err(err) = result {
            host(lua).errors.borrow_mut().push(ScriptError { script, message: err.to_string() });
        }
    }
}

// ---------------------------------------------------------------------------
// Installation and host entry points
// ---------------------------------------------------------------------------

fn join_args(args: Variadic<Value>) -> mlua::Result<String> {
    let parts: mlua::Result<Vec<String>> = args
        .iter()
        .map(|v| match v {
            Value::Vector(v) => Ok(vector_text(*v)),
            v => v.to_string(),
        })
        .collect();
    Ok(parts?.join(" "))
}

/// Luau-side helpers that must be able to yield, so they can't be Rust functions.
fn prelude() -> String {
    use names::{instance, task};
    format!(
        r#"
        local yield = coroutine.yield
        local lib, load_module, side = ...
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
        -- Each module runs once per side (the server's scripts and the
        -- player's local scripts each get their own copy, as on Roblox, where
        -- they run on different computers); later requires get the same
        -- value back.
        local by_side = {{}}
        local require = function(module)
            local key = side()
            local mine = by_side[key]
            if not mine then
                mine = {{ results = {{}}, loading = {{}} }}
                by_side[key] = mine
            end
            local results, loading = mine.results, mine.loading
            local done = results[module]
            if done then return done.value end
            local loader = loading[module]
            if loader then
                -- Only a module that (through others) requires itself is a
                -- loop. Another script asking while it's still loading (it
                -- waited) waits for it, as Roblox does.
                if loader == coroutine.running() then
                    error("require loop: " .. tostring(module) .. " requires itself", 2)
                end
                while loading[module] do wait() end
                done = results[module]
                if done then return done.value end
                error("Requested module experienced an error while loading: " .. tostring(module), 2)
            end
            local run = load_module(module)
            loading[module] = coroutine.running()
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
    // Luau asks at every loop turn. Asking Rust each time would cost more
    // than most loops, so the question only reaches here once a watchdog
    // thread has seen a script run past its deadline.
    let watchdog = Watchdog::start();
    lua.set_interrupt_flag(Some(watchdog.overdue_flag()));
    let watched = watchdog.clone();
    lua.set_interrupt(move |_| {
        if check.get().is_some_and(|d| Instant::now() > d) {
            check.set(None);
            watched.set_deadline(None);
            return Err(mlua::Error::runtime(format!(
                "script ran for more than {} seconds without waiting; add task.wait() inside long loops",
                RUN_LIMIT.as_secs()
            )));
        }
        watched.set_deadline(check.get());
        Ok(mlua::VmState::Continue)
    });
    lua.set_app_data(Host {
        watchdog,
        deadline,
        tree,
        connections: RefCell::new(Connections::default()),
        next_connection: Cell::new(1),
        output: RefCell::new(Vec::new()),
        errors: RefCell::new(Vec::new()),
        current_script: RefCell::new(String::new()),
        current_owner: Cell::new(None),
        clock: Cell::new(0.0),
        waiting: RefCell::new(Vec::new()),
        running_scripts: RefCell::new(std::collections::HashSet::new()),
        to_start: RefCell::new(Vec::new()),
        script_time: RefCell::new(std::collections::HashMap::new()),
        spare_threads: RefCell::new(Vec::new()),
        thread_cost: RefCell::new(std::collections::HashMap::new()),
        script_cost: RefCell::new(std::collections::HashMap::new()),
        heavy_debt: Cell::new(Duration::ZERO),
        part_index: RefCell::new(libroxel_instance::geometry::PartIndex::new()),
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
        instance_cache: RefCell::new(None),
        fill_instances: RefCell::new(None),
    });

    // Weak-valued cache so each instance has exactly one userdata.
    let cache = lua.create_table()?;
    let cache_meta = lua.create_table()?;
    cache_meta.set("__mode", "v")?;
    cache.set_metatable(Some(cache_meta))?;
    *host(lua).instance_cache.borrow_mut() = Some(cache.clone());
    // Lists of instances are filled in Luau: a cache lookup there is a
    // fraction of what each one costs from Rust; only new objects come back.
    let make = lua.create_function(|lua, key: f64| push_instance(lua, InstanceId::from_bits(key as u64)))?;
    let fill: Function = lua
        .load(
            "local cache, make = ...\n\
             return function(t)\n\
                 for i = 1, #t do\n\
                     local key = t[i]\n\
                     local u = cache[key]\n\
                     if u == nil then u = make(key) end\n\
                     t[i] = u\n\
                 end\n\
             end",
        )
        .set_name("=instances")
        .call((cache.clone(), make))?;
    *host(lua).fill_instances.borrow_mut() = Some(fill);
    lua.set_named_registry_value(INSTANCE_CACHE, cache)?;

    let g = lua.globals();
    g.set(globals::GAME, push_instance(lua, root)?)?;
    g.set(globals::WORKSPACE, push_instance(lua, workspace)?)?;

    let instance_lib = lua.create_table()?;
    instance_lib.set(
        constructors::NEW,
        lua.create_function(|lua, (class, parent): (String, Option<Value>)| {
            let other = names::stand_in(&class).is_none()
                && names::class_from_name(&class).is_none()
                && crate::reflection::is_creatable(&class);
            let (kind, shape) = match names::stand_in(&class) {
                Some(stand_in) => stand_in,
                None if other => (ClassKind::Other, None),
                None => (class_arg(&class)?, None),
            };
            let roblox = class;
            let class = kind;
            if !class.is_user_creatable() {
                return Err(tree_err(TreeError::NotCreatable(class)));
            }
            let parent = parent.as_ref().and_then(as_instance);
            let id = {
                let host = host(lua);
                let mut tree = host.tree.borrow_mut();
                let name = if other || shape.is_none() && names::stand_in(&roblox).is_some() {
                    roblox.as_str()
                } else {
                    names::class_name(class)
                };
                let id = tree.create(class, name).map_err(tree_err)?;
                if other {
                    let _ = tree.set(id, Property::OtherClass, PropertyValue::String(roblox.clone()));
                } else if names::stand_in(&roblox).is_some() {
                    let _ = tree.set_extra(id, names::STAND_IN, Some(PropertyValue::String(roblox.clone())));
                }
                if let Some(shape) = shape {
                    let _ = tree.set(id, Property::Shape, PropertyValue::Enum(EnumValue::Shape(shape)));
                }
                if parent.is_some() {
                    tree.set_parent(id, parent).map_err(tree_err)?;
                }
                id
            };
            push_instance(lua, id)
        })?,
    )?;
    g.set(globals::INSTANCE, instance_lib)?;

    install_vector3(lua)?;
    let vector3_lib = lua.create_table()?;
    let new: mlua::Function = lua
        .load("local create = vector.create\nreturn function(x, y, z) return create(x or 0, y or 0, z or 0) end")
        .set_name("=Vector3.new")
        .eval()?;
    vector3_lib.set(constructors::NEW, new)?;
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
        lua.create_function(|lua, (func, args): (Value, Variadic<Value>)| {
            let script = host(lua).current_script.borrow().clone();
            let owner = host(lua).current_owner.get();
            // A function runs in a new thread; a thread carries on where it stopped.
            let thread = match func {
                Value::Function(f) => lua.create_thread(f)?,
                Value::Thread(t) => t,
                other => {
                    return Err(mlua::Error::runtime(format!(
                        "task.spawn expects a function or a thread, got a {}",
                        other.type_name()
                    )));
                }
            };
            if let Some(err) = resume(lua, thread.clone(), args.into_iter().collect(), &script, owner) {
                host(lua).errors.borrow_mut().push(err);
            }
            Ok(thread)
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
                thread: thread.clone(),
                resume_at: now + seconds.max(0.0),
                since: now,
                args: Some(args.into_iter().collect()),
                script,
                owner: host.current_owner.get(),
                parallel: false,
            });
            Ok(thread)
        })?,
    )?;
    // What an Actor's SendMessage starts (see `Waiting::parallel`).
    task_lib.set(
        "__parallel_defer",
        lua.create_function(|lua, (func, args): (Function, Variadic<Value>)| {
            let thread = lua.create_thread(func)?;
            let host = host(lua);
            let now = host.clock.get();
            let script = host.current_script.borrow().clone();
            host.waiting.borrow_mut().push(Waiting {
                thread: thread.clone(),
                resume_at: now,
                since: now,
                args: Some(args.into_iter().collect()),
                script,
                owner: host.current_owner.get(),
                parallel: true,
            });
            Ok(thread)
        })?,
    )?;
    task_lib.set(
        names::task::CANCEL,
        lua.create_function(|lua, thread: Thread| {
            host(lua).waiting.borrow_mut().retain(|w| w.thread != thread);
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
        lua.load(prelude()).set_name("=libroxel").call((
            task_lib.clone(),
            load_module,
            lua.create_function(|lua, ()| {
                Ok(match running_side(lua) {
                    Some(Side::Client) => "client",
                    _ => "server",
                })
            })?,
        ))?;
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
        enum_lib.set(names::enum_name(kind), items)?;
    }
    g.set(globals::ENUM, enum_lib.clone())?;
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
    crate::lenient::install(lua, &enum_lib)?;
    // Every enumeration's items are fixed once the layer has added its own.
    for pair in enum_lib.pairs::<Value, Value>() {
        if let (_, Value::Table(items)) = pair? {
            items.set_readonly(true);
        }
    }
    enum_lib.set_readonly(true);
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
    // Marked safe, as Roblox does for each script: Luau then looks built-in
    // globals (math.floor, Vector3.new, ...) up once when the script loads
    // instead of on every use, and the JIT can inline them. Without it every
    // global read went through the fallback metatable, several times slower.
    // (getfenv/setfenv in a script turn this back off by themselves.)
    env.set_safeenv(true);
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
    if let Some(id) = script_instance {
        host(lua).running_scripts.borrow_mut().insert(id);
    }
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
    let mut errors: Vec<ScriptError> =
        roots.into_iter().flat_map(|root| run_scripts_under(lua, root, ClassKind::Script)).collect();
    // Talking to characters through their Dialogs.
    if let Err(err) = run_chunk(lua, "Dialogs", include_str!("dialogs.luau"), None) {
        errors.push(err);
    }
    errors
}

/// Runs every enabled script of `class` under `ancestor` (and `ancestor`
/// itself, if it is one), in tree order.
pub(crate) fn run_scripts_under(lua: &Lua, ancestor: InstanceId, class: ClassKind) -> Vec<ScriptError> {
    let scripts: Vec<(InstanceId, String, String)> = {
        let host = host(lua);
        let tree = host.tree.borrow();
        let itself = (tree.class(ancestor) == Ok(class)).then_some(ancestor);
        itself
            .into_iter()
            .chain(tree.descendants_of_class(ancestor, class))
            .filter(|id| tree.get_bool(*id, Property::Disabled) != Some(true))
            .filter(|id| !host.running_scripts.borrow().contains(id))
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

/// A script (not a module), which `Enabled` starts and stops.
pub(crate) fn is_script_class(class: ClassKind) -> bool {
    matches!(class, ClassKind::Script | ClassKind::LocalScript)
}

/// Starts or stops a script whose `Enabled` (or `Disabled`) a script set,
/// as Roblox does: an enabled script where scripts of its kind run starts
/// on the next step, and a disabled one stops where it is.
pub(crate) fn script_toggled(lua: &Lua, id: InstanceId) {
    let host = host(lua);
    let disabled = host.tree.borrow().get_bool(id, Property::Disabled) == Some(true);
    if disabled {
        if host.running_scripts.borrow_mut().remove(&id) {
            stop_script(lua, id);
        }
        host.to_start.borrow_mut().retain(|s| *s != id);
    } else if !host.running_scripts.borrow().contains(&id) {
        let mut queue = host.to_start.borrow_mut();
        if !queue.contains(&id) {
            queue.push(id);
        }
    }
}

/// Disconnects a script's connections and drops its waiting threads.
fn stop_script(lua: &Lua, id: InstanceId) {
    let gone: Vec<Connection> = {
        let host = host(lua);
        let gone = host.connections.borrow_mut().take_where(|c| c.owner == Some(id));
        host.waiting.borrow_mut().retain(|w| w.owner != Some(id));
        gone
    };
    for conn in gone {
        let _ = lua.remove_registry_value(conn.handler);
    }
}

/// Whether a script sits where scripts of its kind run: a `Script` in the
/// workspace or the server script service, a `LocalScript` in a player's
/// scripts, GUI, backpack or character.
fn runs_where_it_is(tree: &libroxel_instance::InstanceTree, id: InstanceId) -> bool {
    let Ok(class) = tree.class(id) else { return false };
    let mut at = tree.parent(id).ok().flatten();
    while let Some(a) = at {
        let Ok(c) = tree.class(a) else { return false };
        match class {
            ClassKind::Script if matches!(c, ClassKind::Workspace | ClassKind::ServerScriptService) => {
                return true;
            }
            ClassKind::LocalScript
                if matches!(c, ClassKind::PlayerScripts | ClassKind::PlayerGui | ClassKind::Backpack) =>
            {
                return true;
            }
            ClassKind::LocalScript if c == ClassKind::Model => {
                let players = tree.service(ClassKind::Players);
                let is_character = players.is_some_and(|ps| {
                    tree.children(ps)
                        .map(|cs| cs.iter().any(|p| tree.get_ref(*p, Property::Character) == Some(a)))
                        .unwrap_or(false)
                });
                if is_character {
                    return true;
                }
            }
            _ => {}
        }
        at = tree.parent(a).ok().flatten();
    }
    false
}

/// Starts the scripts enabled since the last step.
fn start_enabled_scripts(lua: &Lua) -> Vec<ScriptError> {
    let queued = std::mem::take(&mut *host(lua).to_start.borrow_mut());
    let mut errors = Vec::new();
    for id in queued {
        let source = {
            let host = host(lua);
            let tree = host.tree.borrow();
            let alive = tree.exists(id) && !tree.is_destroyed(id).unwrap_or(true);
            if !alive
                || tree.get_bool(id, Property::Disabled) == Some(true)
                || host.running_scripts.borrow().contains(&id)
                || !runs_where_it_is(&tree, id)
            {
                continue;
            }
            match (tree.get(id, Property::Source), tree.name(id)) {
                (Ok(PropertyValue::String(s)), Ok(name)) => (s, name.to_owned()),
                _ => continue,
            }
        };
        if let Err(err) = run_chunk(lua, &source.1, &source.0, Some(id)) {
            errors.push(err);
        }
    }
    errors
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
        let gone = host.connections.borrow_mut().take_where(|c| !owner_alive(&tree, c.owner));
        host.waiting.borrow_mut().retain(|w| owner_alive(&tree, w.owner));
        gone
    };
    for conn in gone {
        let _ = lua.remove_registry_value(conn.handler);
    }
}

/// Advances the clock, resumes due threads, then fires the frame signal.
/// How long threads resumed from waits may run in one step before the rest
/// are left for the next one.
const RESUME_BUDGET: Duration = Duration::from_millis(4);

/// A resume this long marks its thread as heavy background work.
const HEAVY_RESUME: Duration = Duration::from_micros(400);

/// The share of a frame waiting threads get while the 3D view is turned off
/// (`Set3dRenderingEnabled(false)`, a loading screen): nobody plays yet, so
/// a long frame costs nothing, and the loading itself (often heavy
/// background work, like building the world) finishes sooner.
const LOADING_BUDGET: Duration = Duration::from_millis(30);

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
    let started = start_enabled_scripts(lua);
    host(lua).errors.borrow_mut().extend(started);
    // Waiting threads get a share of each frame, so that long background work
    // can't stall it (on Roblox such work runs on the server, another
    // computer, and never holds up the player's frames). Each frame runs at
    // least one piece of heavy work, then only as much more as fits in the
    // share; the rest waits one more frame (first in line then). Quick
    // threads always run.
    let budget_start = Instant::now();
    if host(lua).thread_cost.borrow().len() > 20_000 {
        host(lua).thread_cost.borrow_mut().clear();
    }
    // What a script's last heavy piece cost fades each step, so that a
    // script whose new threads keep being held back (and so never show
    // they are quick) isn't marked heavy for good.
    for cost in host(lua).script_cost.borrow_mut().values_mut() {
        *cost = cost.mul_f32(0.97);
    }
    let mut ran_heavy = false;
    let loading = !crate::RENDERING_3D.load(std::sync::atomic::Ordering::Relaxed);
    let share = if loading { LOADING_BUDGET } else { RESUME_BUDGET };
    // Like Roblox's scheduler under load: heavy background work gets a
    // share of each frame on average. One long piece (a 12 ms scan, say)
    // still runs whole, but then the next ones wait the frames it
    // borrowed, instead of a long piece running every single frame.
    let mut debt = host(lua).heavy_debt.get().saturating_sub(share);
    for w in due {
        let key = w.thread.to_pointer() as usize;
        let expected = {
            let host = host(lua);
            let known = host.thread_cost.borrow().get(&key).copied();
            known.or_else(|| host.script_cost.borrow().get(&w.script).copied()).unwrap_or_default()
        };
        let heavy = expected > HEAVY_RESUME;
        let over = ran_heavy && budget_start.elapsed() + expected > share;
        if heavy && !w.parallel && (over || !debt.is_zero()) {
            host(lua).waiting.borrow_mut().push(w);
            continue;
        }
        ran_heavy |= heavy;
        // A destroyed script's threads stop where they are.
        if !owner_alive(&host(lua).tree.borrow(), w.owner) {
            continue;
        }
        let resumed = Instant::now();
        let thread = w.thread.clone();
        let args: MultiValue = match w.args {
            Some(args) => args.into_iter().collect(),
            None => std::iter::once(Value::Number(now - w.since)).collect(),
        };
        if let Some(err) = resume(lua, w.thread, args, &w.script, w.owner) {
            host(lua).errors.borrow_mut().push(err);
        }
        let took = resumed.elapsed();
        if thread.is_resumable() {
            host(lua).thread_cost.borrow_mut().insert(key, took);
        } else {
            host(lua).thread_cost.borrow_mut().remove(&key);
        }
        ran_heavy |= took > HEAVY_RESUME;
        if heavy || took > HEAVY_RESUME {
            debt += took.saturating_sub(share);
        }
        {
            let host = host(lua);
            let mut costs = host.script_cost.borrow_mut();
            // A script's heavy pieces keep it marked heavy for a while,
            // even when quick waits of the same script run in between.
            match costs.get_mut(&w.script) {
                Some(c) => *c = took.max(c.mul_f32(0.9)),
                None => {
                    costs.insert(w.script.clone(), took);
                }
            }
        }
    }
    host(lua).heavy_debt.set(debt.min(Duration::from_millis(250)));
    step_tweens(lua, dt);
    crate::animation::step(lua);
    crate::services::step_debris(lua, now);
    fire(lua, SignalKind::Stepped, || Ok([Value::Number(now), Value::Number(dt)].into_iter().collect()));
    fire(lua, SignalKind::Heartbeat, || Ok(std::iter::once(Value::Number(dt)).collect()));
    fire(lua, SignalKind::RenderStepped, || Ok(std::iter::once(Value::Number(dt)).collect()));
    // Last, so pictures drawn in this frame's Heartbeat or RenderStepped
    // show this frame, together with the moves and turns of the labels
    // around them made in the same handlers. (Sent before those events,
    // a minimap showed last frame's map under this frame's markers and
    // scroll, and jumped back and forth: it flickered.)
    let _ = crate::lenient::flush_drawn(lua);
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
    let (any_watched, tags_heard, tools_heard) = {
        let host = host(lua);
        let conns = host.connections.borrow();
        (!conns.watched.is_empty(), conns.tags > 0, conns.tools > 0)
    };
    if !any_watched && !tags_heard && !tools_heard {
        return Vec::new();
    }
    let watched = |id: &InstanceId| host(lua).connections.borrow().is_watched(id);
    let with_instance = |signal: SignalKind, id: InstanceId| {
        fire(lua, signal, || Ok(std::iter::once(Value::UserData(push_instance(lua, id)?)).collect()));
    };
    // Changes are heard after the fact, against the tree as it is now: a
    // folder parented and then filled in the same frame already holds its
    // parts when its move is heard. Parts that are moved themselves later in
    // the batch are announced by their own move, not the folder's.
    // (And each instance is announced to an ancestor once a batch: a clone's
    // parts are put in it before it's parented, so both moves would.)
    let mut announced: std::collections::HashSet<(bool, InstanceId, InstanceId)> =
        std::collections::HashSet::new();
    let mut moves_left: std::collections::HashMap<InstanceId, usize> = std::collections::HashMap::new();
    for change in changes {
        if let Change::Reparented { id, .. } = change {
            *moves_left.entry(*id).or_default() += 1;
        }
    }
    for change in changes {
        match change {
            Change::PropertyChanged { id, property } if watched(id) => {
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
                if let Some(n) = moves_left.get_mut(id) {
                    *n = n.saturating_sub(1);
                }
                if let Some(parent) = old_parent
                    && watched(parent)
                {
                    with_instance(SignalKind::ChildRemoved(*parent), *id);
                }
                if let Some(parent) = new_parent
                    && watched(parent)
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
                            if watched(&a) {
                                chain.push(a);
                            }
                            at = tree.parent(a).ok().flatten();
                        }
                        chain
                    };
                    let (left, mut arrived) = (up(old_parent), up(new_parent));
                    // Put into something that is itself moved later in the
                    // batch (a clone's parts): that move announces it, after
                    // the clone itself.
                    let mut at = *new_parent;
                    while let Some(a) = at {
                        if moves_left.get(&a).is_some_and(|n| *n > 0) {
                            arrived.clear();
                            break;
                        }
                        at = tree.parent(a).ok().flatten();
                    }
                    let mut moved = Vec::new();
                    if !left.is_empty() || !arrived.is_empty() {
                        moved.push(*id);
                        moved.extend(tree.descendants(*id).unwrap_or_default());
                    }
                    (moved, left, arrived)
                };
                // The instance and everything inside it that listens hears
                // that its ancestry changed.
                let listening: Vec<InstanceId> = {
                    let host = host(lua);
                    let tree = host.tree.borrow();
                    std::iter::once(*id)
                        .chain(tree.descendants(*id).unwrap_or_default())
                        .filter(|i| watched(i))
                        .collect()
                };
                for item in listening {
                    fire(lua, SignalKind::AncestryChanged(item), || {
                        let parent = match new_parent {
                            Some(p) => Value::UserData(push_instance(lua, *p)?),
                            None => Value::Nil,
                        };
                        Ok([Value::UserData(push_instance(lua, *id)?), parent].into_iter().collect())
                    });
                }
                let moved: Vec<InstanceId> = moved
                    .into_iter()
                    .filter(|item| item == id || moves_left.get(item).is_none_or(|n| *n == 0))
                    .collect();
                for &ancestor in &left {
                    for &item in &moved {
                        if announced.insert((false, ancestor, item)) {
                            with_instance(SignalKind::DescendantRemoving(ancestor), item);
                        }
                    }
                }
                for &ancestor in &arrived {
                    for &item in &moved {
                        if announced.insert((true, ancestor, item)) {
                            with_instance(SignalKind::DescendantAdded(ancestor), item);
                        }
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
            Change::AttributeChanged { id, name } if watched(id) => {
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
            Change::Destroyed(id) if watched(id) || tags_heard => {
                if watched(id) {
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

/// The left mouse button went down (or up) on a GUI button.
pub(crate) fn press_button(lua: &Lua, button: InstanceId, down: bool) -> Vec<ScriptError> {
    let key = if down { "MouseButton1Down" } else { "MouseButton1Up" };
    let _ = crate::lenient::fire_mouse(lua, button, key);
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

/// Where the functions given to `game:BindToClose` are kept.
pub(crate) const CLOSE_KEY: &str = "libroxel.bind_to_close";
const CLOSING: &str = "BindToClose";

/// Shuts the game down as Roblox does: every function given to
/// `game:BindToClose` runs, and the game keeps going (up to 30 seconds of
/// game time, as on Roblox) until they have all finished.
pub(crate) fn close(lua: &Lua) -> Vec<ScriptError> {
    let funcs: Vec<Function> = match lua.named_registry_value::<Value>(CLOSE_KEY) {
        Ok(Value::Table(t)) => t.sequence_values::<Function>().filter_map(Result::ok).collect(),
        _ => Vec::new(),
    };
    if funcs.is_empty() {
        return Vec::new();
    }
    for f in funcs {
        if let Err(err) = spawn(lua, f, MultiValue::new(), CLOSING, None) {
            host(lua)
                .errors
                .borrow_mut()
                .push(ScriptError { script: CLOSING.into(), message: err.to_string() });
        }
    }
    let mut errors = take_errors(lua);
    for _ in 0..300 {
        if !host(lua).waiting.borrow().iter().any(|w| w.script == CLOSING) {
            break;
        }
        errors.extend(step(lua, 0.1));
    }
    errors
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
