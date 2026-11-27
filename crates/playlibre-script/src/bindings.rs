//! Lua-side types, globals and the thread scheduler. Every script-visible
//! name comes from [`crate::names`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use playlibre_instance::{
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

const INSTANCE_CACHE: &str = "playlibre.instance_cache";
const WAIT_FOR_CHILD_FN: &str = "playlibre.wait_for_child";
const SIGNAL_WAIT_FN: &str = "playlibre.signal_wait";
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
    /// The mouse's older key events, with the key as a letter.
    MouseKeyDown,
    MouseKeyUp,
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

    /// The instance the signal belongs to, if any.
    fn target(self) -> Option<InstanceId> {
        match self {
            SignalKind::Touched(id)
            | SignalKind::CharacterAdded(id)
            | SignalKind::Died(id)
            | SignalKind::ButtonClicked(id)
            | SignalKind::ButtonActivated(id)
            | SignalKind::MouseClick(id)
            | SignalKind::PromptTriggered(id)
            | SignalKind::PromptTriggerEnded(id)
            | SignalKind::ServerEvent(id)
            | SignalKind::ClientEvent(id)
            | SignalKind::BindableEvent(id)
            | SignalKind::ToolEquipped(id)
            | SignalKind::ToolUnequipped(id)
            | SignalKind::ToolActivated(id)
            | SignalKind::ToolDeactivated(id)
            | SignalKind::TextBoxFocused(id)
            | SignalKind::MoveToFinished(id)
            | SignalKind::TextBoxFocusLost(id)
            | SignalKind::ExplosionHit(id)
            | SignalKind::TouchEnded(id)
            | SignalKind::CharacterRemoving(id)
            | SignalKind::Seated(id)
            | SignalKind::MouseEnter(id)
            | SignalKind::MouseLeave(id)
            | SignalKind::SoundEnded(id) => Some(id),
            _ => self.watched(),
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
    /// How many connections each script instance made.
    owners: std::collections::HashMap<InstanceId, usize>,
    /// How many connections each instance's signals have.
    targets: std::collections::HashMap<InstanceId, usize>,
    tags: usize,
    tools: usize,
}

impl Connections {
    fn count(&mut self, signal: SignalKind, owner: Option<InstanceId>, by: isize) {
        let bump = |n: &mut usize| *n = n.saturating_add_signed(by);
        if let Some(owner) = owner {
            let n = self.owners.entry(owner).or_default();
            bump(n);
            if *n == 0 {
                self.owners.remove(&owner);
            }
        }
        if let Some(id) = signal.watched() {
            let n = self.watched.entry(id).or_default();
            bump(n);
            if *n == 0 {
                self.watched.remove(&id);
            }
        }
        if let Some(id) = signal.target() {
            let n = self.targets.entry(id).or_default();
            bump(n);
            if *n == 0 {
                self.targets.remove(&id);
            }
        }
        match signal {
            SignalKind::TagAdded(_) | SignalKind::TagRemoved(_) => bump(&mut self.tags),
            SignalKind::ToolEquipped(_) | SignalKind::ToolUnequipped(_) => bump(&mut self.tools),
            _ => {}
        }
    }

    fn push(&mut self, conn: Connection) {
        self.count(conn.signal, conn.owner, 1);
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
        self.count(signal, conn.owner, -1);
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
            self.count(c.signal, c.owner, -1);
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
            self.count(c.signal, c.owner, -1);
        }
        once
    }

    fn len(&self) -> usize {
        self.signal_of.len()
    }

    /// The script instances that made connections.
    fn owners(&self) -> impl Iterator<Item = InstanceId> + '_ {
        self.owners.keys().copied()
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
    /// An Actor's message handler: on the reference platform it runs on another core, so
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
    /// Switches Luau's interrupt on. It stays off otherwise: a call at
    /// every loop turn would make loops about twice as slow.
    arm: mlua::InterruptArm,
}

impl Watchdog {
    /// A watchdog with its own thread, which ends when the watchdog is dropped.
    fn start(arm: mlua::InterruptArm) -> std::sync::Arc<Self> {
        let dog = std::sync::Arc::new(Self {
            start: Instant::now(),
            deadline_ms: std::sync::atomic::AtomicU64::new(0),
            overdue: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            arm,
        });
        let weak = std::sync::Arc::downgrade(&dog);
        let _ = std::thread::Builder::new().name("script watchdog".into()).spawn(move || {
            while let Some(dog) = weak.upgrade() {
                let due = dog.deadline_ms.load(std::sync::atomic::Ordering::Relaxed);
                if due != 0 && dog.start.elapsed().as_millis() as u64 > due {
                    dog.overdue.store(true, std::sync::atomic::Ordering::Relaxed);
                    // (Again each time: the interrupt may have switched
                    // itself off just as the flag went up.)
                    dog.arm.arm();
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
    /// How much memory the game's scripts may use (see [`crate::MEMORY_LIMIT`]).
    pub(crate) memory_limit: Cell<usize>,
    /// The weak-valued table of each instance's one userdata, by its id's
    /// bits (kept here so finding it doesn't go through the registry).
    instance_cache: RefCell<Option<Table>>,
    /// A Luau function that turns a list of instance numbers into their
    /// objects in place (see [`instance_list`]).
    fill_instances: RefCell<Option<Function>>,
    /// Value objects whose Changed already fired when the value was set,
    /// with how many times: their next batch of changes skips that many.
    values_heard: RefCell<std::collections::HashMap<InstanceId, u32>>,
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
    /// Tweens whose script object was collected.
    tween_drops: Rc<RefCell<Vec<u64>>>,
    next_tween: Cell<u64>,
    /// Keys and mouse buttons held down right now.
    pub(crate) inputs_down: RefCell<std::collections::HashSet<crate::UserInput>>,
    /// Saved data: store name, then key, then value.
    pub(crate) data: RefCell<crate::services::DataStores>,
    pub(crate) data_changed: Cell<bool>,
    /// Versions, times, user ids and metadata of saved keys, for this run.
    pub(crate) data_info: RefCell<crate::services::KeyInfos>,
    /// A script reached `CollectionService.TagAdded` or `TagRemoved`.
    pub(crate) tag_sets_heard: Cell<bool>,
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
    /// Scripts already compiled, by a hash of their name and source (with
    /// both kept to check), so copies of a script start without compiling.
    compiled: RefCell<std::collections::HashMap<u64, (String, String, Function)>>,
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
    /// When the last frame's waiting threads were resumed, to size the share
    /// of the next frame by how long frames really take.
    last_step: Cell<Option<Instant>>,
    /// Every waiting thread runs each step, however long it takes.
    pub(crate) steady: Cell<bool>,
    /// The tree's destroy count when destroyed scripts were last looked for.
    destroys_seen: Cell<Option<u64>>,
    /// Where the workspace's parts are, for raycasts and part queries.
    pub(crate) part_index: RefCell<playlibre_instance::geometry::PartIndex>,
}

/// The part index, caught up with the tree.
pub(crate) fn part_index<'a>(
    host: &'a Host,
    tree: &playlibre_instance::InstanceTree,
) -> std::cell::RefMut<'a, playlibre_instance::geometry::PartIndex> {
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
pub(crate) fn is_character(tree: &playlibre_instance::InstanceTree, id: InstanceId) -> bool {
    tree.class(id) == Ok(ClassKind::Model)
        && tree.find_first_child_of_class(id, ClassKind::Humanoid).ok().flatten().is_some()
}

pub(crate) fn host(lua: &Lua) -> mlua::AppDataRef<'_, Host> {
    lua.app_data_ref::<Host>().expect("playlibre host state is installed")
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
        TreeError::TooMany => {
            format!(
                "the game already has {} instances, the most there can be",
                playlibre_instance::MOST_INSTANCES
            )
        }
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

/// What the engine's own waits yield first, ahead of the seconds. A thread
/// that yields anything else (a bare `coroutine.yield()`) is left alone
/// until something resumes it, such as `task.spawn(thread)`.
static WAIT_MARK: u8 = 0;

pub(crate) fn wait_mark() -> Value {
    Value::LightUserData(mlua::LightUserData(std::ptr::addr_of!(WAIT_MARK) as *mut std::ffi::c_void))
}

fn is_wait_mark(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::LightUserData(p)) if std::ptr::eq(p.0 as *const u8, std::ptr::addr_of!(WAIT_MARK)))
}

/// Resumes `thread`; if it waits, it is parked until the time it asked for.
/// Errors are recorded against `script`. Returns the error, if any, so the
/// first resume of a script can report it directly.
fn resume(
    lua: &Lua,
    thread: Thread,
    args: MultiValue,
    script: &str,
    owner: Option<InstanceId>,
) -> Option<ScriptError> {
    resume_returning(lua, thread, args, script, owner).err()
}

/// [`resume`], also giving back what the thread returned if it ran to its
/// end (`None` if it is waiting).
fn resume_returning(
    lua: &Lua,
    thread: Thread,
    args: MultiValue,
    script: &str,
    owner: Option<InstanceId>,
) -> Result<Option<MultiValue>, ScriptError> {
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
    // A thread of a script destroyed before the last look (one it didn't
    // find waiting) may have waited or connected again: look again.
    if owner.is_some() && host(lua).tree.try_borrow().is_ok_and(|tree| !owner_alive(&tree, owner)) {
        host(lua).destroys_seen.set(None);
    }
    match result {
        Ok(yielded) => {
            if thread.is_resumable() {
                // Yielded by the script itself: whoever resumes it next decides.
                if !is_wait_mark(yielded.front()) {
                    return Ok(None);
                }
                let seconds = yielded.get(1).and_then(|v| match v {
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
                return Ok(None);
            }
            Ok(Some(yielded))
        }
        Err(err) => Err(ScriptError { script: script.to_owned(), message: err.to_string() }),
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

/// Runs `func` in a new thread like [`spawn`], and gives back what it
/// returned if it finished without waiting. An error is reported and gives
/// `None`, as does a thread that is now waiting.
pub(crate) fn spawn_returning(
    lua: &Lua,
    func: Function,
    args: MultiValue,
    script: &str,
) -> mlua::Result<Option<MultiValue>> {
    let thread = lua.create_thread(func)?;
    match resume_returning(lua, thread, args, script, None) {
        Ok(values) => Ok(values),
        Err(err) => {
            host(lua).errors.borrow_mut().push(err);
            Ok(None)
        }
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

/// Like the reference platform, a Vector3 is Luau's built-in vector: made, read and
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
/// `typeof` saying "Vector3" for one, as on the reference platform.
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
		-- The VM answers `v.X` written out in the source itself; a key worked
		-- out while running (`v[axis]`) comes here.
		if key == "X" or key == "x" then
			return v.x
		elseif key == "Y" or key == "y" then
			return v.y
		elseif key == "Z" or key == "z" then
			return v.z
		elseif key == "Magnitude" or key == "magnitude" then
			return vector.magnitude(v)
		elseif key == "Unit" or key == "unit" then
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
        fields.add_field_method_get(VALUE, |_, this| Ok(enum_number(this.0)));
        // The enumeration itself, so `item.EnumType == Enum.Material` holds.
        fields.add_field_method_get(ENUM_TYPE, |lua, this| {
            let name = names::enum_name(this.0.kind());
            let lib: Option<Table> = lua.globals().get(globals::ENUM).ok();
            match lib.and_then(|lib| lib.raw_get::<Value>(name).ok()).filter(|v| !v.is_nil()) {
                Some(enum_table) => Ok(enum_table),
                None => Ok(Value::String(lua.create_string(name)?)),
            }
        });
        fields.add_meta_field(MetaMethod::Type, TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(names::enum_item::IS_A, |_, this, name: String| {
            Ok(names::enum_name(this.0.kind()) == name)
        });
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

/// An item's number: the reference platform's, or its place in the list
/// for items only playlibre has.
pub(crate) fn enum_number(item: EnumValue) -> i64 {
    crate::reflection::enum_number(names::enum_name(item.kind()), names::enum_item_name(item))
        .map_or(item.index() as i64, i64::from)
}

/// Reads an enum value of `kind` from a script value: an item, its name, or its number.
#[allow(clippy::useless_conversion)]
pub(crate) fn as_enum(kind: EnumKind, value: &Value) -> Option<EnumValue> {
    match value {
        Value::UserData(ud) => ud.borrow::<LuaEnumItem>().ok().map(|e| e.0).filter(|e| e.kind() == kind),
        Value::String(s) => names::enum_item_from_name(kind, &s.to_string_lossy()),
        Value::Integer(n) => kind.items().iter().copied().find(|i| enum_number(*i) == i64::from(*n)),
        Value::Number(n) if n.fract() == 0.0 => {
            kind.items().iter().copied().find(|i| enum_number(*i) as f64 == *n)
        }
        // An item of this enumeration that playlibre doesn't draw differently
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

/// Those of `ids` that scripts may still reach: a script's object for it
/// is alive, something is connected to its events or watches it, it is a
/// running script, or Debris or a tween will still touch it. The rest
/// can be freed.
pub(crate) fn held(lua: &Lua, ids: &[InstanceId]) -> std::collections::HashSet<InstanceId> {
    let host = host(lua);
    let cache = host.instance_cache.borrow().clone();
    let connections = host.connections.borrow();
    let running = host.running_scripts.borrow();
    let soon: std::collections::HashSet<InstanceId> = host
        .debris
        .borrow()
        .iter()
        .map(|(d, _)| *d)
        .chain(host.tweens.borrow().iter().map(|t| t.instance))
        .collect();
    ids.iter()
        .copied()
        .filter(|id| {
            let key = id.to_bits() as f64;
            cache.as_ref().is_some_and(|c| c.raw_get::<Value>(key).is_ok_and(|v| !v.is_nil()))
                || connections.targets.contains_key(id)
                || connections.watched.contains_key(id)
                || connections.owners.contains_key(id)
                || running.contains(id)
                || soon.contains(id)
        })
        .collect()
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
pub(crate) fn property_from_lua(property: Property, value: &Value) -> Option<PropertyValue> {
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

/// The reference platform's error for a member an instance doesn't have, naming its class
/// and where it is: `Foo is not a valid member of Part "Workspace.Crate"`.
/// Where a part's pivot is, from its centre (its PivotOffset).
fn pivot_offset(lua: &Lua, id: InstanceId) -> mlua::Result<CFrame> {
    let offset = push_instance(lua, id)?.get::<Value>("PivotOffset").ok();
    Ok(offset.as_ref().and_then(as_cframe).unwrap_or(CFrame::IDENTITY))
}

/// Instances parented to an object playlibre only stands in for (one made
/// in Lua), by instance.
fn stand_in_parents(lua: &Lua) -> mlua::Result<Table> {
    const KEY: &str = "playlibre.stand_in_parents";
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
    let class = class_of(lua, id).map(|c| crate::lenient::api_class(lua, id, c)).unwrap_or_default();
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
    let api_name = crate::lenient::api_class(lua, id, class);
    if api_name != names::class_name(class) {
        return Ok(name == "Instance" || crate::reflection::class_is_a(&api_name, name));
    }
    if let Some(answer) = names::is_a_family(class, name) {
        return Ok(answer);
    }
    match names::class_from_name(name) {
        Some(other) => Ok(class.is_a(other)),
        // Families playlibre doesn't name, such as PVInstance's relatives.
        None => Ok(crate::reflection::class_is_a(names::class_name(class), name)),
    }
}

/// Whether instance `id`'s class is exactly `name`, as `FindFirstChildOfClass` asks.
pub(crate) fn is_class(lua: &Lua, id: InstanceId, name: &str) -> mlua::Result<bool> {
    let class = class_of(lua, id)?;
    Ok(crate::lenient::api_class(lua, id, class) == name)
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

        methods.add_method(FIND_FIRST_CHILD, |lua, this, (name, recursive): (String, Option<bool>)| {
            let found = {
                let host = host(lua);
                let tree = host.tree.borrow();
                match tree.find_first_child(this.0, &name).map_err(tree_err)? {
                    Some(id) => Some(id),
                    None if recursive == Some(true) => tree
                        .descendants(this.0)
                        .map_err(tree_err)?
                        .into_iter()
                        .find(|d| tree.name(*d) == Ok(name.as_str())),
                    None => None,
                }
            };
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
            crate::sound::play_on_remove(lua, this.0);
            host(lua).tree.borrow_mut().destroy(this.0).map_err(tree_err)
        });
        methods.add_method(CLONE, |lua, this, ()| {
            let host = host(lua);
            let mut tree = host.tree.borrow_mut();
            if !tree.archivable(this.0) {
                return Ok(None);
            }
            if tree.held() >= playlibre_instance::MOST_INSTANCES {
                return Err(tree_err(TreeError::TooMany));
            }
            let copy = tree.clone_subtree(this.0).map_err(tree_err)?;
            drop(tree);
            push_instance(lua, copy).map(Some)
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
            // an event that fires when a script or playlibre changes them.
            let api_name = crate::lenient::api_class(lua, this.0, class);
            let known = crate::reflection::has_property(&api_name, &name)
                || push_instance(lua, this.0)?.get::<Value>(name.as_str()).is_ok();
            if !known {
                return Err(not_a_member(lua, this.0, &name));
            }
            crate::lenient::changed_signal(lua, this.0, &name)
        });
        methods.add_method(names::methods::CLEAR_ALL_CHILDREN, |lua, this, ()| {
            let host = host(lua);
            let children = host.tree.borrow().children(this.0).map_err(tree_err)?.to_vec();
            for child in children {
                crate::sound::play_on_remove(lua, child);
                let _ = host.tree.borrow_mut().destroy(child);
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
                let name = crate::lenient::api_class(lua, id, class);
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
                // A game's scripts can't read code, as on the reference
                // platform: a careless RemoteFunction that hands back
                // `thing.Source` would give any player the server's code.
                if property == Property::Source {
                    return Err(mlua::Error::runtime(format!(
                        "{} can't be read by a game's scripts",
                        names::property_name(property)
                    )));
                }
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
                    // A service or settings object playlibre only stands in
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
            set_heard_now(lua, id, class, property, value)?;
            crate::world::after_set(lua, id, class, property)?;
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
    UDim(playlibre_instance::UDim),
    CFrame(CFrame),
    /// Jumps to the goal when the tween reaches its end.
    Bool(bool),
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
            PropertyValue::Bool(b) => TweenValue::Bool(*b),
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
            (TweenValue::Bool(a), TweenValue::Bool(b)) => TweenValue::Bool(if t >= 1.0 { b } else { a }),
            (_, to) => to,
        }
    }
}

/// What a tween goal changes: a property, or a part's CFrame.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TweenTarget {
    Property(Property),
    PartCFrame,
    /// A member playlibre keeps by name (such as a frame's Rotation), read
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
            Value::Boolean(b) => TweenValue::Bool(*b),
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
            TweenValue::Bool(b) => Value::Boolean(b),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TweenPhase {
    Idle,
    Playing,
    Paused,
    Done,
    /// Stopped with Cancel.
    Cancelled,
}

/// `Enum.PlaybackState[name]`, from the enumerations the lenient layer adds.
fn playback_state(lua: &Lua, name: &str) -> mlua::Result<Value> {
    let lookup: Function = lua
        .load("local name = ... return Enum.PlaybackState[name]")
        .set_name("=playback_state")
        .into_function()?;
    lookup.call(name)
}

struct TweenState {
    id: u64,
    instance: InstanceId,
    info: LuaTweenInfo,
    /// Target, value at the start (filled in when played), goal value.
    goals: Vec<(TweenTarget, Option<TweenValue>, TweenValue)>,
    /// Every goal it was made with: a newer tween can take some of `goals`
    /// over, and playing again from the start gets them back.
    all_goals: Vec<(TweenTarget, TweenValue)>,
    elapsed: f64,
    phase: TweenPhase,
    /// No script holds it any more: it goes once it isn't playing.
    orphan: bool,
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
                TweenValue::Bool(b) => PropertyValue::Bool(b),
                TweenValue::CFrame(_) => return,
            };
            match class_of(lua, id) {
                Ok(class) => set_heard_now(lua, id, class, p, value),
                Err(err) => Err(err),
            }
        }
        _ => Ok(()),
    };
}

/// Whether `class` is a value object (IntValue, NumberValue and the rest).
fn is_value_object(class: ClassKind) -> bool {
    names::is_a_family(class, "ValueBase") == Some(true)
}

/// Sets `property` and, when it is a value object's Value and it changed,
/// fires the object's Changed with the new value right away, as the
/// reference platform does: the handlers have run before the setter's next
/// line (or a tween's Completed).
fn set_heard_now(
    lua: &Lua,
    id: InstanceId,
    class: ClassKind,
    property: Property,
    value: PropertyValue,
) -> mlua::Result<()> {
    let heard_now = is_value_object(class) && names::property_name(property) == "Value";
    let before = heard_now.then(|| host(lua).tree.borrow().get(id, property).ok()).flatten();
    host(lua).tree.borrow_mut().set(id, property, value).map_err(tree_err)?;
    if !heard_now || host(lua).tree.borrow().get(id, property).ok() == before {
        return Ok(());
    }
    *host(lua).values_heard.borrow_mut().entry(id).or_default() += 1;
    fire(lua, SignalKind::Changed(id), || {
        let object = push_instance(lua, id)?;
        Ok(std::iter::once(object.get::<Value>("Value")?).collect())
    });
    Ok(())
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
        all_goals: list.iter().map(|g| (g.0, g.2)).collect(),
        goals: list,
        elapsed: 0.0,
        phase: TweenPhase::Idle,
        orphan: false,
    });
    Ok(LuaTween(tween, host.tween_drops.clone()))
}

struct LuaTween(u64, Rc<RefCell<Vec<u64>>>);

impl Drop for LuaTween {
    fn drop(&mut self) {
        if let Ok(mut drops) = self.1.try_borrow_mut() {
            drops.push(self.0);
        }
    }
}

/// Lets go of tweens no script can reach: unheld ones that stopped, and
/// ones whose instance was destroyed.
fn forget_tweens(lua: &Lua) {
    let gone: Vec<Connection> = {
        let host = host(lua);
        let drops = std::mem::take(&mut *host.tween_drops.borrow_mut());
        let mut tweens = host.tweens.borrow_mut();
        if !drops.is_empty() {
            let drops: std::collections::HashSet<u64> = drops.into_iter().collect();
            for t in tweens.iter_mut().filter(|t| drops.contains(&t.id)) {
                t.orphan = true;
            }
        }
        let tree = host.tree.borrow();
        let mut removed = std::collections::HashSet::new();
        tweens.retain(|t| {
            let keep =
                !(t.orphan && t.phase != TweenPhase::Playing) && tree.is_destroyed(t.instance) == Ok(false);
            if !keep {
                removed.insert(t.id);
            }
            keep
        });
        if removed.is_empty() {
            return;
        }
        host.connections
            .borrow_mut()
            .take_where(|c| matches!(c.signal, SignalKind::TweenCompleted(id) if removed.contains(&id)))
    };
    for conn in gone {
        let _ = lua.remove_registry_value(conn.handler);
    }
}

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
        // Where it is: Begin, Delayed, Playing, Paused, Completed or Cancelled.
        fields.add_field_method_get("PlaybackState", |lua, this| {
            let state = this
                .with(lua, |t| match t.phase {
                    TweenPhase::Idle => "Begin",
                    TweenPhase::Playing if t.elapsed < t.info.delay => "Delayed",
                    TweenPhase::Playing => "Playing",
                    TweenPhase::Paused => "Paused",
                    TweenPhase::Done => "Completed",
                    TweenPhase::Cancelled => "Cancelled",
                })
                .unwrap_or("Completed");
            playback_state(lua, state)
        });
        fields.add_field_method_get("Instance", |lua, this| {
            Ok(this.with(lua, |t| t.instance).map(LuaInstance))
        });
        fields.add_field_method_get("TweenInfo", |lua, this| Ok(this.with(lua, |t| t.info)));
        fields.add_field_method_get("ClassName", |_, _| Ok(names::tween::TYPE_NAME));
        fields.add_field_method_get("Name", |_, _| Ok(names::tween::TYPE_NAME));
        fields.add_meta_field(MetaMethod::Type, names::tween::TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(names::tween::PLAY, |lua, this, ()| {
            let Some((instance, phase, targets)) = this.with(lua, |t| {
                if t.phase != TweenPhase::Paused && t.phase != TweenPhase::Playing {
                    t.goals = t.all_goals.iter().map(|(target, goal)| (*target, None, *goal)).collect();
                }
                (t.instance, t.phase, t.goals.iter().map(|g| g.0).collect::<Vec<_>>())
            }) else {
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
            // A newer tween takes over any property an older one is still
            // moving. One left with nothing to move is cancelled, as documented.
            let overridden: Vec<u64> = {
                let host = host(lua);
                let mut tweens = host.tweens.borrow_mut();
                let mut overridden = Vec::new();
                for other in tweens.iter_mut().filter(|t| t.id != this.0 && t.instance == instance) {
                    if matches!(other.phase, TweenPhase::Playing | TweenPhase::Paused) {
                        let before = other.goals.len();
                        other.goals.retain(|g| !targets.contains(&g.0));
                        if other.goals.is_empty() && before > 0 {
                            other.phase = TweenPhase::Cancelled;
                            other.elapsed = 0.0;
                            overridden.push(other.id);
                        }
                    }
                }
                overridden
            };
            this.with(lua, |t| t.phase = TweenPhase::Playing);
            if !overridden.is_empty() {
                let state = playback_state(lua, "Cancelled")?;
                for id in overridden {
                    fire(lua, SignalKind::TweenCompleted(id), || Ok(MultiValue::from_iter([state.clone()])));
                }
            }
            Ok(())
        });
        methods.add_method("IsA", |_, _, class: String| {
            Ok(matches!(class.as_str(), "Tween" | "TweenBase" | "Instance"))
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
            let was = this.with(lua, |t| {
                let was = t.phase;
                t.phase = TweenPhase::Cancelled;
                t.elapsed = 0.0;
                was
            });
            // As on the reference platform, Completed fires with Cancelled for a tween that was going.
            if matches!(was, Some(TweenPhase::Playing | TweenPhase::Paused)) {
                let state = playback_state(lua, "Cancelled")?;
                fire(lua, SignalKind::TweenCompleted(this.0), || Ok(MultiValue::from_iter([state.clone()])));
            }
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
        fire(lua, SignalKind::TweenCompleted(tween), || {
            Ok(MultiValue::from_iter([playback_state(lua, "Completed")?]))
        });
    }
    if !host(lua).tween_drops.borrow().is_empty() {
        forget_tweens(lua);
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Signal(pub(crate) SignalKind);

impl UserData for Signal {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "RBXScriptSignal");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        for name in [names::signal::CONNECT, names::signal::CONNECT_PARALLEL, names::signal::OLD_CONNECT] {
            methods.add_method(name, |lua, this, handler: Function| {
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
        }
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
            if key == names::signal::WAIT || key == names::signal::OLD_WAIT {
                return lua.named_registry_value::<Value>(SIGNAL_WAIT_FN);
            }
            Err(mlua::Error::runtime(format!("{key} is not a valid member of a signal")))
        });
    }
}

struct ConnectionHandle(u64);

impl UserData for ConnectionHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "RBXScriptConnection");
        for name in [names::signal::CONNECTED, names::signal::OLD_CONNECTED] {
            fields
                .add_field_method_get(name, |lua, this| Ok(host(lua).connections.borrow().contains(this.0)));
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        for name in [names::signal::DISCONNECT, names::signal::OLD_DISCONNECT] {
            methods.add_method(name, |lua, this, ()| {
                let removed = host(lua).connections.borrow_mut().remove(this.0);
                if let Some(conn) = removed {
                    lua.remove_registry_value(conn.handler)?;
                }
                Ok(())
            });
        }
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
    // The same arguments for every handler (the reference platform passes the same values).
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
        local lib, load_module, side, mark = ...
        lib["{wait}"] = function(seconds) return yield(mark, seconds or 0) end
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
        -- player's local scripts each get their own copy, as on the reference platform, where
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
                -- waited) waits for it, as the reference platform does.
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
    // Luau would ask at every loop turn, which costs about as much as a
    // simple loop itself, so the interrupt stays switched off until a
    // watchdog thread sees a script run past its deadline.
    let watchdog = Watchdog::start(lua.interrupt_arm());
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
        compiled: RefCell::new(std::collections::HashMap::new()),
        thread_cost: RefCell::new(std::collections::HashMap::new()),
        script_cost: RefCell::new(std::collections::HashMap::new()),
        heavy_debt: Cell::new(Duration::ZERO),
        last_step: Cell::new(None),
        steady: Cell::new(false),
        destroys_seen: Cell::new(None),
        part_index: RefCell::new(playlibre_instance::geometry::PartIndex::new()),
        tweens: RefCell::new(Vec::new()),
        memory_limit: Cell::new(crate::device_memory_limit()),
        tween_drops: Rc::new(RefCell::new(Vec::new())),
        next_tween: Cell::new(1),
        inputs_down: RefCell::new(Default::default()),
        data: RefCell::new(Default::default()),
        animations: RefCell::new(Default::default()),
        data_changed: Cell::new(false),
        data_info: RefCell::new(Default::default()),
        tag_sets_heard: Cell::new(false),
        debris: RefCell::new(Vec::new()),
        interned: RefCell::new(Vec::new()),
        mouse: Cell::new(None),
        world: RefCell::new(Default::default()),
        net: RefCell::new(Default::default()),
        groups: RefCell::new(Default::default()),
        instance_cache: RefCell::new(None),
        fill_instances: RefCell::new(None),
        values_heard: RefCell::new(Default::default()),
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
            let api_name = class;
            let class = kind;
            if !class.is_user_creatable() {
                return Err(tree_err(TreeError::NotCreatable(class)));
            }
            let parent = parent.as_ref().and_then(as_instance);
            let id = {
                let host = host(lua);
                let mut tree = host.tree.borrow_mut();
                let name = if other || shape.is_none() && names::stand_in(&api_name).is_some() {
                    api_name.as_str()
                } else {
                    names::class_name(class)
                };
                if tree.held() >= playlibre_instance::MOST_INSTANCES {
                    return Err(tree_err(TreeError::TooMany));
                }
                let id = tree.create(class, name).map_err(tree_err)?;
                if other {
                    let _ = tree.set(id, Property::OtherClass, PropertyValue::String(api_name.clone()));
                } else if names::stand_in(&api_name).is_some() {
                    let _ =
                        tree.set_extra(id, names::STAND_IN, Some(PropertyValue::String(api_name.clone())));
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
        load_script(lua, &name, &source.unwrap_or_default(), env)
    })?;
    let (wait, wait_for_child, require, signal_wait): (Function, Function, Function, Function) =
        lua.load(prelude()).set_name("=playlibre").call((
            task_lib.clone(),
            load_module,
            lua.create_function(|lua, ()| {
                Ok(match running_side(lua) {
                    Some(Side::Client) => "client",
                    _ => "server",
                })
            })?,
            wait_mark(),
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
    // Marked safe, as the reference platform does for each script: Luau then looks built-in
    // globals (math.floor, Vector3.new, ...) up once when the script loads
    // instead of on every use, and the JIT can inline them. Without it every
    // global read went through the fallback metatable, several times slower.
    // (getfenv/setfenv in a script turn this back off by themselves.)
    env.set_safeenv(true);
    Ok(env)
}

/// Compiles `source` (named `name` in errors) into a function that runs in
/// `env`. A script compiled before is copied instead: games often hold
/// hundreds of copies of one script (coins, enemies, doors), and compiling
/// each one, and turning it into machine code, was most of their start.
/// The copy shares the compiled code but has its own environment, so it
/// runs exactly as a freshly compiled one would.
fn load_script(lua: &Lua, name: &str, source: &str, env: Table) -> mlua::Result<Function> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (name, source).hash(&mut hasher);
    let key = hasher.finish();
    let cached = host(lua)
        .compiled
        .borrow()
        .get(&key)
        .filter(|(n, s, _)| n == name && s == source)
        .map(|(.., f)| f.clone());
    if let Some(template) = cached {
        let func = template.deep_clone()?;
        func.set_environment(env)?;
        return Ok(func);
    }
    let func = lua.load(source).set_name(format!("={name}")).set_environment(env).into_function()?;
    // The kept copy never runs, so its environment doesn't matter.
    let template = func.deep_clone()?;
    let host = host(lua);
    let mut compiled = host.compiled.borrow_mut();
    if compiled.len() >= 512 {
        compiled.clear();
    }
    compiled.insert(key, (name.to_owned(), source.to_owned(), template));
    Ok(func)
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
    let func = load_script(lua, script_name, source, env).map_err(fail)?;
    let thread = lua.create_thread(func).map_err(fail)?;
    if let Some(id) = script_instance {
        host(lua).running_scripts.borrow_mut().insert(id);
    }
    match resume(lua, thread, MultiValue::new(), script_name, script_instance) {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

pub(crate) fn run_plugin(
    lua: &Lua,
    name: &str,
    source: &str,
    selection: Rc<RefCell<Vec<InstanceId>>>,
) -> Result<(), ScriptError> {
    let fail = |err: mlua::Error| ScriptError { script: name.to_owned(), message: err.to_string() };
    let env = script_env(lua, None).map_err(fail)?;
    let plugin = lua.create_table().map_err(fail)?;
    let get = selection.clone();
    let get_selection = lua
        .create_function(move |lua, _: Value| {
            let list = lua.create_table()?;
            for (i, id) in get.borrow().iter().enumerate() {
                list.raw_set(i + 1, push_instance(lua, *id)?)?;
            }
            Ok(list)
        })
        .map_err(fail)?;
    let set_selection = lua
        .create_function(move |_, (_, list): (Value, Table)| {
            *selection.borrow_mut() = list
                .sequence_values::<Value>()
                .filter_map(Result::ok)
                .filter_map(|v| as_instance(&v))
                .collect();
            Ok(())
        })
        .map_err(fail)?;
    plugin.raw_set("GetSelection", get_selection).map_err(fail)?;
    plugin.raw_set("SetSelection", set_selection).map_err(fail)?;
    plugin.set_readonly(true);
    env.raw_set("plugin", plugin).map_err(fail)?;
    let func =
        lua.load(source).set_name(format!("={name}")).set_environment(env).into_function().map_err(fail)?;
    let thread = lua.create_thread(func).map_err(fail)?;
    match resume(lua, thread, MultiValue::new(), name, None) {
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
/// as the reference platform does: an enabled script where scripts of its kind run starts
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
fn runs_where_it_is(tree: &playlibre_instance::InstanceTree, id: InstanceId) -> bool {
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

/// Queues the scripts that were put where they run (a cloned script
/// parented into the workspace or a player's scripts), so they start on
/// the next step, as they would on the reference platform.
fn queue_moved_scripts(lua: &Lua, changes: &[Change]) {
    let moved: Vec<InstanceId> = changes
        .iter()
        .filter_map(|c| match c {
            Change::Reparented { id, new_parent: Some(_), .. } => Some(*id),
            _ => None,
        })
        .collect();
    if moved.is_empty() {
        return;
    }
    let role = crate::net::role(lua);
    let me = crate::services::local_player(lua);
    let host = host(lua);
    let tree = host.tree.borrow();
    let running = host.running_scripts.borrow();
    let mut queue = host.to_start.borrow_mut();
    for root in moved {
        for class in script_homes(&tree, root, role, me) {
            let itself = (tree.class(root) == Ok(class)).then_some(root);
            for id in itself.into_iter().chain(tree.descendants_of_class(root, class)) {
                if !running.contains(&id)
                    && !queue.contains(&id)
                    && tree.get_bool(id, Property::Disabled) != Some(true)
                    && runs_where_it_is(&tree, id)
                {
                    queue.push(id);
                }
            }
        }
    }
}

/// Which kinds of script run at `id` on this computer: `Script`s in the
/// workspace or the server script service (not on a joined computer),
/// `LocalScript`s in this computer's player's scripts, GUI, backpack or
/// character.
fn script_homes(
    tree: &playlibre_instance::InstanceTree,
    id: InstanceId,
    role: crate::net::NetRole,
    me: Option<InstanceId>,
) -> Vec<ClassKind> {
    let mut homes = Vec::new();
    let mut at = tree.parent(id).ok().flatten();
    while let Some(a) = at {
        let Ok(class) = tree.class(a) else { break };
        match class {
            ClassKind::Workspace | ClassKind::ServerScriptService => {
                if role != crate::net::NetRole::Client {
                    homes.push(ClassKind::Script);
                }
                break;
            }
            ClassKind::PlayerScripts | ClassKind::PlayerGui | ClassKind::Backpack => {
                if me.is_some() && tree.parent(a).ok().flatten() == me {
                    homes.push(ClassKind::LocalScript);
                }
                break;
            }
            ClassKind::Model if me.is_some_and(|p| tree.get_ref(p, Property::Character) == Some(a)) => {
                homes.push(ClassKind::LocalScript);
            }
            _ => {}
        }
        at = tree.parent(a).ok().flatten();
    }
    homes
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
fn owner_alive(tree: &playlibre_instance::InstanceTree, owner: Option<InstanceId>) -> bool {
    owner.is_none_or(|id| tree.exists(id) && !tree.is_destroyed(id).unwrap_or(true))
}

/// Disconnects everything connected by scripts that have been destroyed
/// (like the old copy of a GUI after a respawn), and drops their waiting threads.
fn stop_destroyed_scripts(lua: &Lua) {
    let gone: Vec<Connection> = {
        let host = host(lua);
        let tree = host.tree.borrow();
        // Nothing destroyed since the last look: no script can have gone.
        if host.destroys_seen.replace(Some(tree.destroy_count())) == Some(tree.destroy_count()) {
            return;
        }
        // Each script looked at once, not each of its (maybe thousands of)
        // connections and waiting threads.
        let mut connections = host.connections.borrow_mut();
        let dead: std::collections::HashSet<InstanceId> =
            connections.owners().filter(|o| !owner_alive(&tree, Some(*o))).collect();
        let gone = if dead.is_empty() {
            Vec::new()
        } else {
            connections.take_where(|c| c.owner.is_some_and(|o| dead.contains(&o)))
        };
        let mut alive = std::collections::HashMap::new();
        host.waiting.borrow_mut().retain(|w| {
            w.owner.is_none_or(|o| *alive.entry(o).or_insert_with(|| owner_alive(&tree, Some(o))))
        });
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

/// Heavy work held back this long (seconds) past when it was due runs
/// anyway: game logic waiting on it (a drop onto a belt, the ground near
/// the player) mustn't lag behind by seconds.
const MAX_LATE: f64 = 0.1;

/// A resume this long marks its thread as heavy background work.
const HEAVY_RESUME: Duration = Duration::from_micros(400);

/// The share of a frame waiting threads get while the 3D view is turned off
/// (`Set3dRenderingEnabled(false)`, a loading screen): nobody plays yet, so
/// a long frame costs nothing, and the loading itself (often heavy
/// background work, like building the world) finishes sooner.
const LOADING_BUDGET: Duration = Duration::from_millis(30);

/// The part of a frame's length waiting threads may use while playing, and
/// while a loading screen is up.
const PLAYING_SHARE: f32 = 0.2;
const LOADING_SHARE: f32 = 0.6;

pub(crate) fn step(lua: &Lua, dt: f64) -> Vec<ScriptError> {
    // The collector lets garbage pile up to about twice what is in use
    // before it catches up: past half the limit, collect it all now.
    if lua.used_memory() > host(lua).memory_limit.get() / 2 {
        let _ = lua.gc_collect();
    }
    let now = {
        let host = host(lua);
        host.clock.set(host.clock.get() + dt);
        host.clock.get()
    };
    // Take only threads due now; ones that wait again are parked for a later frame.
    let mut due: Vec<Waiting> = {
        let host = host(lua);
        // Taken out in place: the (often thousands of) threads still
        // asleep stay where they are instead of all moving to a new list
        // every frame.
        host.waiting.borrow_mut().extract_if(.., |w| w.resume_at <= now).collect()
    };
    due.sort_by(|a, b| a.resume_at.total_cmp(&b.resume_at));
    stop_destroyed_scripts(lua);
    let started = start_enabled_scripts(lua);
    host(lua).errors.borrow_mut().extend(started);
    // Waiting threads get a share of each frame, so that long background work
    // can't stall it (on the reference platform such work runs on the server, another
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
    // The share is a part of the frame's real length, not a fixed time: on a
    // slower computer frames are longer and fewer, and a fixed few ms a frame
    // starved background work (building the world took minutes there, while
    // on the reference platform it runs on the server at full speed).
    let frame = host(lua)
        .last_step
        .replace(Some(budget_start))
        .map_or(Duration::ZERO, |last| budget_start.duration_since(last).min(Duration::from_millis(100)));
    let share = if loading {
        LOADING_BUDGET.max(frame.mul_f32(LOADING_SHARE))
    } else {
        RESUME_BUDGET.max(frame.mul_f32(PLAYING_SHARE))
    };
    // Like the reference platform's scheduler under load: heavy background work gets a
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
        let late = now - w.resume_at > MAX_LATE;
        if heavy && !w.parallel && !late && !host(lua).steady.get() && (over || !debt.is_zero()) {
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
    queue_moved_scripts(lua, changes);
    let errors = fire_heard_changes(lua, changes);
    forget_destroyed(lua, changes);
    errors
}

/// Disconnects every connection to the signals `gone` picks.
pub(crate) fn disconnect_where(lua: &Lua, gone: impl Fn(SignalKind) -> bool) {
    let gone = host(lua).connections.borrow_mut().take_where(|c| gone(c.signal));
    for conn in gone {
        let _ = lua.remove_registry_value(conn.handler);
    }
}

/// Like the reference platform, a destroyed instance's events are disconnected (after its
/// `Destroying` fired), and what scripts kept on it is let go.
fn forget_destroyed(lua: &Lua, changes: &[Change]) {
    let mut dead: Vec<InstanceId> = Vec::new();
    for change in changes {
        if let Change::Destroyed(id) = change {
            dead.push(*id);
        }
    }
    if dead.is_empty() {
        return;
    }
    let gone: Vec<Connection> = {
        let host = host(lua);
        let mut connections = host.connections.borrow_mut();
        let dead: std::collections::HashSet<InstanceId> =
            dead.iter().copied().filter(|id| connections.targets.contains_key(id)).collect();
        if dead.is_empty() {
            Vec::new()
        } else {
            connections.take_where(|c| c.signal.target().is_some_and(|t| dead.contains(&t)))
        }
    };
    for conn in gone {
        let _ = lua.remove_registry_value(conn.handler);
    }
    forget_tweens(lua);
    crate::lenient::forget(lua, &dead);
}

fn fire_heard_changes(lua: &Lua, changes: &[Change]) -> Vec<ScriptError> {
    // What was set since the last batch is in this one.
    let mut heard = std::mem::take(&mut *host(lua).values_heard.borrow_mut());
    let (any_watched, tags_heard, tools_heard) = {
        let host = host(lua);
        let conns = host.connections.borrow();
        (!conns.watched.is_empty(), conns.tags > 0, conns.tools > 0)
    };
    let sets_heard = host(lua).tag_sets_heard.get();
    if !any_watched && !tags_heard && !tools_heard && !sets_heard {
        return Vec::new();
    }
    // CollectionService.TagAdded fires when the first instance in the game
    // gets a tag, and TagRemoved when the last one loses it.
    let tag_set = |tag: &str, added: bool| {
        if !sets_heard {
            return;
        }
        let count = host(lua).tree.borrow().tagged(tag).len();
        if (added && count == 1) || (!added && count == 0) {
            let event = if added { "TagAdded" } else { "TagRemoved" };
            let args = lua.create_string(tag).map(|t| MultiValue::from_iter([Value::String(t)]));
            if let Err(err) =
                args.and_then(|a| crate::lenient::fire_service_event(lua, "CollectionService", event, a))
            {
                host(lua)
                    .errors
                    .borrow_mut()
                    .push(ScriptError { script: event.to_owned(), message: err.to_string() });
            }
        }
    };
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
                // A value object's Changed hears only its Value, and passes
                // the new value; everything else passes the property's name.
                let class = host(lua).tree.borrow().class(*id).ok();
                if class.is_some_and(is_value_object) {
                    let fired =
                        name == "Value" && heard.get_mut(id).filter(|n| **n > 0).map(|n| *n -= 1).is_some();
                    if name == "Value" && !fired {
                        fire(lua, SignalKind::Changed(*id), || {
                            let object = push_instance(lua, *id)?;
                            Ok(std::iter::once(object.get::<Value>(name)?).collect())
                        });
                    }
                } else {
                    fire(lua, SignalKind::Changed(*id), || {
                        Ok(std::iter::once(Value::String(lua.create_string(name)?)).collect())
                    });
                }
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
            Change::TagAdded { id, tag } if tags_heard || sets_heard => {
                tag_set(tag, true);
                if let Some(n) = interned(lua, tag).filter(|_| tags_heard) {
                    with_instance(SignalKind::TagAdded(n), *id);
                }
            }
            Change::TagRemoved { id, tag } if tags_heard || sets_heard => {
                tag_set(tag, false);
                if let Some(n) = interned(lua, tag).filter(|_| tags_heard) {
                    with_instance(SignalKind::TagRemoved(n), *id);
                }
            }
            Change::Destroyed(id) if watched(id) || tags_heard || sets_heard => {
                if watched(id) {
                    fire(lua, SignalKind::Destroying(*id), || Ok(MultiValue::new()));
                }
                if !tags_heard && !sets_heard {
                    continue;
                }
                let tags = host(lua).tree.borrow().tags(*id).map(|t| t.to_vec()).unwrap_or_default();
                for tag in &tags {
                    tag_set(tag, false);
                }
                if !tags_heard {
                    continue;
                }
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

/// Fires a GUI object's `InputBegan` (or `InputEnded`) for a mouse button.
pub(crate) fn gui_input(
    lua: &Lua,
    object: InstanceId,
    input: crate::UserInput,
    began: bool,
) -> Vec<ScriptError> {
    let key = if began { "InputBegan" } else { "InputEnded" };
    let args = lua
        .create_userdata(crate::services::InputObject(input, began))
        .map(|o| std::iter::once(Value::UserData(o)).collect());
    if let Err(err) = args.and_then(|a| crate::lenient::fire_event(lua, object, key, a)) {
        host(lua).errors.borrow_mut().push(ScriptError { script: key.to_owned(), message: err.to_string() });
    }
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
pub(crate) const CLOSE_KEY: &str = "playlibre.bind_to_close";
const CLOSING: &str = "BindToClose";

/// Shuts the game down as the reference platform does: every function given to
/// `game:BindToClose` runs, and the game keeps going (up to 30 seconds of
/// game time, as on the reference platform) until they have all finished.
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
