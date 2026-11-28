//! The camera, part velocities, text box focus and explosions: things that
//! scripts reach through the tree but that the engine owns.

use std::collections::HashMap;

use mlua::{Lua, MetaMethod, MultiValue, UserData, UserDataFields, UserDataMethods, Value};
use playlibre_instance::{ClassKind, InstanceId, Property, PropertyValue, Vec2, Vec3};

use crate::ScriptError;
use crate::bindings::{
    LuaInstance, LuaVector3, SignalKind, as_vector3, fire, host, part_cframe, push_instance, require_class,
    take_errors, tree_err,
};
use crate::names::{extras, methods};
use crate::types::LuaVector2;

/// A change to a part's velocity that a script asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VelocityRequest {
    /// Set the velocity outright.
    Set(Vec3),
    /// Add to the velocity (already divided by the part's mass).
    Add(Vec3),
    /// Set how fast it spins, in radians per second about each axis.
    Spin(Vec3),
}

/// Engine-owned state that scripts can read and ask to change.
#[derive(Debug, Default)]
pub(crate) struct WorldState {
    /// The velocity of every moving part, as of the last physics step.
    velocities: HashMap<InstanceId, Vec3>,
    /// How fast every spinning part spins, as of the last physics step.
    spins: HashMap<InstanceId, Vec3>,
    requests: Vec<(InstanceId, VelocityRequest)>,
    /// The text box the player is typing into.
    focused: Option<InstanceId>,
    /// The 3D view's size in pixels.
    viewport: Vec2,
    /// Where each screen GUI object was last drawn: corner and size.
    gui_rects: HashMap<InstanceId, [f32; 4]>,
    /// The same, in the order they were drawn.
    gui_order: Vec<InstanceId>,
    /// What the player plays with.
    pub(crate) devices: crate::Devices,
    /// A strength `HapticService:SetMotor` set the vibration motor to,
    /// waiting for the app to take it.
    pub(crate) vibration: Option<f32>,
    /// Humanoids asked to walk somewhere (or to stop, with `None`).
    moves: Vec<(InstanceId, Option<Vec3>)>,
    /// Players whose character scripts asked to (re)spawn.
    loads: Vec<InstanceId>,
    /// Players `Kick` removed, and why.
    kicks: Vec<(InstanceId, String)>,
    /// Places `RequestStreamAroundAsync` asked to send players the parts
    /// around, and for how many seconds.
    stream_requests: Vec<(InstanceId, Vec3, f32)>,
    /// `AddPersistentPlayer` (true) and `RemovePersistentPlayer` (false)
    /// calls: model, player.
    persistent_players: Vec<(InstanceId, InstanceId, bool)>,
    /// Seats scripts asked to sit humanoids in, with `Seat:Sit`.
    sits: Vec<(InstanceId, InstanceId)>,
    /// Particle emitters asked to send out particles at once, and how many.
    pub(crate) emits: Vec<(InstanceId, u32)>,
    /// Sounds that `Play` or `Stop` sent back to the beginning.
    pub(crate) sound_restarts: Vec<InstanceId>,
    /// Tools that were just used (their `Activated` fired).
    pub(crate) swings: Vec<InstanceId>,
    pub(crate) badges: Badges,
    pub(crate) passes: Passes,
    pub(crate) products: Products,
    pub(crate) teleports: Teleports,
}

/// Which game, place and server this is: `game.GameId`, `game.PlaceId`,
/// `game.JobId`, `game.PrivateServerId` and `game.PrivateServerOwnerId`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameIdentity {
    pub game_id: i64,
    pub place_id: i64,
    pub job_id: String,
    /// The server's own code on a private or reserved server.
    pub private_server_id: String,
    /// The owner's user id on a private server (0 on a reserved one).
    pub private_server_owner_id: i64,
}

/// A teleport a script asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct Teleport {
    pub user_id: i64,
    pub place_id: i64,
    /// A reserved or private server's code.
    pub access_code: Option<String>,
    /// What `GetJoinData().TeleportData` gives there.
    pub data: Option<serde_json::Value>,
}

/// What a player arrived with: the place they came from and its data.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JoinData {
    pub source_place_id: i64,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Default)]
pub(crate) struct Teleports {
    pub(crate) identity: GameIdentity,
    pub(crate) queued: Vec<Teleport>,
    pub(crate) joins: HashMap<i64, JoinData>,
    /// On a player's computer: what they arrived with.
    pub(crate) local: JoinData,
}

/// A game pass the game sells, as the Hub describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct PassInfo {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub price: u32,
}

/// A developer product the game sells, as the Hub describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProductInfo {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub price: u32,
}

/// A purchase of a developer product, for `ProcessReceipt`.
#[derive(Debug, Clone, PartialEq)]
pub struct ReceiptInfo {
    pub id: i64,
    pub user_id: i64,
    pub product_id: i64,
    pub price: u32,
}

#[derive(Debug, Default)]
pub(crate) struct Products {
    pub(crate) on: bool,
    pub(crate) info: Vec<ProductInfo>,
    /// `PromptProductPurchase` calls waiting for the host: user id, product id.
    pub(crate) prompts: Vec<(i64, i64)>,
    /// Receipts `ProcessReceipt` is looking at.
    pub(crate) busy: std::collections::HashSet<i64>,
    /// Receipts `ProcessReceipt` answered, and whether it granted them.
    pub(crate) done: Vec<(i64, bool)>,
}

#[derive(Debug, Default)]
pub(crate) struct Passes {
    /// Whether the host keeps track of passes at all (a published game on
    /// a Hub). Until it says so, nobody owns any and prompts close unbought.
    pub(crate) on: bool,
    pub(crate) info: Vec<PassInfo>,
    /// User id, pass id.
    pub(crate) owned: std::collections::HashSet<(i64, i64)>,
    /// Users whose passes the host has looked up.
    pub(crate) known: std::collections::HashSet<i64>,
    /// `PromptGamePassPurchase` calls waiting for the host: user id, pass id.
    pub(crate) prompts: Vec<(i64, i64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BadgeInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Default)]
pub(crate) struct Badges {
    pub(crate) info: Vec<BadgeInfo>,
    pub(crate) owned: std::collections::HashSet<(i64, String)>,
    pub(crate) awards: Vec<(i64, String)>,
}

fn runtime_err(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

/// A part's mass: its volume times the density.
fn mass(lua: &Lua, id: InstanceId) -> f32 {
    playlibre_instance::physical::mass(&host(lua).tree.borrow(), id)
}

fn velocity(lua: &Lua, id: InstanceId) -> Vec3 {
    let host = host(lua);
    let world = host.world.borrow();
    let mut v = world.velocities.get(&id).copied().unwrap_or_default();
    // Requests not yet applied are already visible to scripts.
    for (target, request) in &world.requests {
        if *target == id {
            match request {
                VelocityRequest::Set(set) => v = *set,
                VelocityRequest::Add(add) => v = v + *add,
                VelocityRequest::Spin(_) => {}
            }
        }
    }
    v
}

fn spin(lua: &Lua, id: InstanceId) -> Vec3 {
    let host = host(lua);
    let world = host.world.borrow();
    let mut spin = world.spins.get(&id).copied().unwrap_or_default();
    for (target, request) in &world.requests {
        if let (true, VelocityRequest::Spin(s)) = (*target == id, request) {
            spin = *s;
        }
    }
    spin
}

/// A part's velocity, as scripts see it.
pub(crate) fn velocity_of(lua: &Lua, id: InstanceId) -> Vec3 {
    velocity(lua, id)
}

/// How fast a part spins, as scripts see it.
pub(crate) fn spin_of(lua: &Lua, id: InstanceId) -> Vec3 {
    spin(lua, id)
}

/// Asks for a part to spin at `spin`.
pub(crate) fn request_spin(lua: &Lua, id: InstanceId, spin: Vec3) {
    request(lua, id, VelocityRequest::Spin(spin));
}

fn request(lua: &Lua, id: InstanceId, request: VelocityRequest) {
    host(lua).world.borrow_mut().requests.push((id, request));
}

/// The workspace's camera, made if there isn't one yet.
fn current_camera(lua: &Lua) -> mlua::Result<InstanceId> {
    let host = host(lua);
    let mut tree = host.tree.borrow_mut();
    let ws = tree.workspace();
    if let Some(cam) = tree.find_first_child_of_class(ws, ClassKind::Camera).map_err(tree_err)? {
        return Ok(cam);
    }
    let cam = tree.create(ClassKind::Camera, extras::CAMERA_NAME).map_err(tree_err)?;
    tree.set_parent(cam, Some(ws)).map_err(tree_err)?;
    Ok(cam)
}

/// Members beyond the saved properties: `Some` when `key` is one of them.
const TEAM_COLOR: &str = "TeamColor";

pub(crate) fn index(lua: &Lua, id: InstanceId, class: ClassKind, key: &str) -> mlua::Result<Option<Value>> {
    Ok(Some(match key {
        extras::CURRENT_CAMERA if class == ClassKind::Workspace => {
            Value::UserData(push_instance(lua, current_camera(lua)?)?)
        }
        extras::TERRAIN if class == ClassKind::Workspace => {
            let terrain = host(lua).tree.borrow_mut().terrain_or_make().map_err(tree_err)?;
            Value::UserData(push_instance(lua, terrain)?)
        }
        extras::ASSEMBLY_LINEAR_VELOCITY if class.is_physical() => {
            crate::bindings::vector_value(velocity(lua, id))
        }
        extras::ASSEMBLY_ANGULAR_VELOCITY if class.is_physical() => {
            crate::bindings::vector_value(spin(lua, id))
        }
        // A method on seats, but a property on humanoids, so it's looked up here.
        methods::SIT if class.is_seat() => {
            Value::Function(lua.create_function(|lua, (seat, humanoid): (Value, Value)| {
                let seat = crate::bindings::as_instance(&seat)
                    .ok_or_else(|| runtime_err("call Sit with a colon"))?;
                let humanoid = crate::bindings::as_instance(&humanoid)
                    .filter(|h| host(lua).tree.borrow().class(*h) == Ok(ClassKind::Humanoid))
                    .ok_or_else(|| runtime_err("Sit expects a Humanoid"))?;
                host(lua).world.borrow_mut().sits.push((seat, humanoid));
                Ok(())
            })?)
        }
        // The team's colour, or the last one set.
        TEAM_COLOR if class == ClassKind::Player => {
            let colour = {
                let host = host(lua);
                let tree = host.tree.borrow();
                let team = tree.get_ref(id, Property::Team).filter(|t| tree.class(*t) == Ok(ClassKind::Team));
                match team.map(|t| tree.get(t, Property::TeamColor)) {
                    Some(Ok(PropertyValue::Color3(c))) => Some(c),
                    _ => match tree.get_extra(id, TEAM_COLOR) {
                        Ok(Some(PropertyValue::Color3(c))) => Some(c),
                        _ => None,
                    },
                }
            };
            crate::compat::brick_for(
                lua,
                colour.unwrap_or(playlibre_instance::Color3::from_rgb8(242, 243, 243)),
            )?
        }
        extras::VIEWPORT_SIZE if class == ClassKind::Camera => {
            let size = host(lua).world.borrow().viewport;
            Value::UserData(lua.create_userdata(LuaVector2(size))?)
        }
        _ => return Ok(None),
    }))
}

/// Sets a member beyond the saved properties. Returns whether `key` was one.
pub(crate) fn new_index(
    lua: &Lua,
    id: InstanceId,
    class: ClassKind,
    key: &str,
    value: &Value,
) -> mlua::Result<bool> {
    match key {
        extras::ASSEMBLY_LINEAR_VELOCITY if class.is_physical() => {
            let v = as_vector3(value).ok_or_else(|| runtime_err(format!("{key} expects a Vector3")))?;
            request(lua, id, VelocityRequest::Set(v));
            Ok(true)
        }
        extras::ASSEMBLY_ANGULAR_VELOCITY if class.is_physical() => {
            let v = as_vector3(value).ok_or_else(|| runtime_err(format!("{key} expects a Vector3")))?;
            request(lua, id, VelocityRequest::Spin(v));
            Ok(true)
        }
        // Who sits where is up to the engine.
        _ if matches!(
            crate::names::property_from_name(class, key),
            Some(Property::Occupant | Property::SeatPart)
        ) =>
        {
            Err(runtime_err(format!("{key} can't be set; use Seat:Sit, or set Humanoid.Sit to false")))
        }
        // Putting a player on a team takes them off neutral, and back.
        _ if class == ClassKind::Player
            && Some(Property::Team) == crate::names::property_from_name(class, key) =>
        {
            let team = match value {
                Value::Nil => None,
                other => Some(
                    crate::bindings::as_instance(other)
                        .filter(|t| host(lua).tree.borrow().class(*t) == Ok(ClassKind::Team))
                        .ok_or_else(|| runtime_err("Team expects a Team or nil"))?,
                ),
            };
            let host = host(lua);
            let mut tree = host.tree.borrow_mut();
            tree.set(id, Property::Team, PropertyValue::Ref(team)).map_err(tree_err)?;
            tree.set(id, Property::Neutral, PropertyValue::Bool(team.is_none())).map_err(tree_err)?;
            Ok(true)
        }
        // A team colour puts the player on that colour's team.
        TEAM_COLOR if class == ClassKind::Player => {
            let Some(PropertyValue::Color3(colour)) =
                crate::bindings::property_from_lua(Property::TeamColor, value)
            else {
                return Err(runtime_err("TeamColor expects a BrickColor"));
            };
            let host = host(lua);
            let mut tree = host.tree.borrow_mut();
            let team = tree.service(ClassKind::Teams).and_then(|teams| {
                tree.children(teams).ok()?.iter().copied().find(|t| {
                    tree.class(*t) == Ok(ClassKind::Team)
                        && tree.get(*t, Property::TeamColor).ok() == Some(PropertyValue::Color3(colour))
                })
            });
            tree.set_extra(id, TEAM_COLOR, Some(PropertyValue::Color3(colour))).map_err(tree_err)?;
            if let Some(team) = team {
                tree.set(id, Property::Team, PropertyValue::Ref(Some(team))).map_err(tree_err)?;
                tree.set(id, Property::Neutral, PropertyValue::Bool(false)).map_err(tree_err)?;
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// What follows from a property a script set: health stays between 0 and
/// the most.
pub(crate) fn after_set(lua: &Lua, id: InstanceId, class: ClassKind, property: Property) -> mlua::Result<()> {
    if class != ClassKind::Humanoid || !matches!(property, Property::Health | Property::MaxHealth) {
        return Ok(());
    }
    let host = host(lua);
    let mut tree = host.tree.borrow_mut();
    let most = tree.get_number(id, Property::MaxHealth).unwrap_or(100.0).max(0.0);
    let health = tree.get_number(id, Property::Health).unwrap_or(0.0);
    let kept = health.clamp(0.0, most);
    if kept != health {
        tree.set(id, Property::Health, PropertyValue::Number(kept)).map_err(tree_err)?;
    }
    Ok(())
}

/// Where `point` shows on a view of `size` pixels through the camera:
/// pixels from the top-left, then depth in studs, and whether it is in view.
fn project(lua: &Lua, camera: InstanceId, point: Vec3) -> mlua::Result<(Vec3, bool)> {
    let cf = part_cframe(lua, camera)?;
    let fov = host(lua).tree.borrow().get_number(camera, Property::FieldOfView).unwrap_or(70.0) as f32;
    let size = host(lua).world.borrow().viewport;
    let local = cf.inverse().point_to_world(point);
    let depth = -local.z;
    if depth <= 1e-4 {
        return Ok((Vec3::new(0.0, 0.0, depth), false));
    }
    let focal = (size.y / 2.0) / (fov.to_radians() / 2.0).tan();
    let x = size.x / 2.0 + focal * local.x / depth;
    let y = size.y / 2.0 - focal * local.y / depth;
    let on_screen = (0.0..=size.x).contains(&x) && (0.0..=size.y).contains(&y);
    Ok((Vec3::new(x, y, depth), on_screen))
}

pub(crate) fn add_instance_methods<M: UserDataMethods<LuaInstance>>(m: &mut M) {
    m.add_method(methods::MOVE_TO, |lua, this, (target, part): (Value, Value)| {
        // A model's MoveTo puts it there; a humanoid's walks there.
        if crate::bindings::class_of(lua, this.0)? == ClassKind::Model {
            return crate::compat::model_move_to(lua, this.0, target);
        }
        require_class(lua, this.0, ClassKind::Humanoid, methods::MOVE_TO)?;
        let at = as_vector3(&target).ok_or_else(|| runtime_err("MoveTo expects a Vector3"))?;
        // With a part, the point keeps where it is on the part: the walk
        // follows the part as it moves.
        use crate::names::character::{WALK_OFFSET, WALK_TO_PART};
        let part = crate::bindings::as_instance(&part).filter(|p| {
            host(lua).tree.borrow().class(*p).is_ok_and(|c| c.is_physical() && c != ClassKind::Terrain)
        });
        let follow = match part {
            Some(part) => {
                let local = crate::bindings::part_cframe(lua, part)?.inverse().point_to_world(at);
                Some((PropertyValue::Ref(Some(part)), PropertyValue::Vec3(local)))
            }
            None => None,
        };
        {
            let host = host(lua);
            let mut tree = host.tree.borrow_mut();
            let (part, offset) = follow.unzip();
            let _ = tree.set_extra(this.0, WALK_TO_PART, part);
            let _ = tree.set_extra(this.0, WALK_OFFSET, offset);
        }
        host(lua).world.borrow_mut().moves.push((this.0, Some(at)));
        Ok(())
    });
    m.add_method(methods::LOAD_CHARACTER, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::Player, methods::LOAD_CHARACTER)?;
        let host = host(lua);
        let mut world = host.world.borrow_mut();
        if !world.loads.contains(&this.0) {
            world.loads.push(this.0);
        }
        Ok(())
    });
    m.add_method(methods::KICK, |lua, this, why: Option<String>| {
        require_class(lua, this.0, ClassKind::Player, methods::KICK)?;
        let why =
            why.filter(|w| !w.trim().is_empty()).unwrap_or_else(|| "You were removed from the game".into());
        host(lua).world.borrow_mut().kicks.push((this.0, why.chars().take(300).collect()));
        Ok(())
    });
    m.add_method(methods::REQUEST_STREAM_AROUND_ASYNC, |lua, this, (at, timeout): (Value, Option<f32>)| {
        require_class(lua, this.0, ClassKind::Player, methods::REQUEST_STREAM_AROUND_ASYNC)?;
        let at = as_vector3(&at).ok_or_else(|| {
            mlua::Error::RuntimeError(format!("{} expects a Vector3", methods::REQUEST_STREAM_AROUND_ASYNC))
        })?;
        // The area stays sent for a while; the request returns at once, the
        // parts arriving with the next step.
        let keep = timeout.filter(|t| t.is_finite() && *t > 0.0).unwrap_or(30.0).min(600.0);
        host(lua).world.borrow_mut().stream_requests.push((this.0, at, keep));
        Ok(())
    });
    for (method, add) in [(methods::ADD_PERSISTENT_PLAYER, true), (methods::REMOVE_PERSISTENT_PLAYER, false)]
    {
        m.add_method(method, move |lua, this, player: Value| {
            require_class(lua, this.0, ClassKind::Model, method)?;
            let Some(player) = crate::bindings::as_instance(&player) else { return Ok(()) };
            require_class(lua, player, ClassKind::Player, method)?;
            host(lua).world.borrow_mut().persistent_players.push((this.0, player, add));
            Ok(())
        });
    }
    m.add_method(methods::GET_TEAMS, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::Teams, methods::GET_TEAMS)?;
        let teams: Vec<InstanceId> = {
            let host = host(lua);
            let tree = host.tree.borrow();
            let children = tree.children(this.0).map_err(tree_err)?;
            children.iter().copied().filter(|c| tree.class(*c) == Ok(ClassKind::Team)).collect()
        };
        teams.into_iter().map(|id| push_instance(lua, id)).collect::<mlua::Result<Vec<_>>>()
    });
    m.add_method(methods::GET_MASS, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::Part, methods::GET_MASS)?;
        Ok(mass(lua, this.0))
    });
    m.add_method(methods::APPLY_IMPULSE, |lua, this, impulse: Value| {
        require_class(lua, this.0, ClassKind::Part, methods::APPLY_IMPULSE)?;
        let impulse = as_vector3(&impulse).ok_or_else(|| runtime_err("ApplyImpulse expects a Vector3"))?;
        let m = mass(lua, this.0);
        request(lua, this.0, VelocityRequest::Add(impulse / m));
        Ok(())
    });
    m.add_method("ApplyImpulseAtPosition", |lua, this, (impulse, at): (Value, Value)| {
        require_class(lua, this.0, ClassKind::Part, "ApplyImpulseAtPosition")?;
        let impulse =
            as_vector3(&impulse).ok_or_else(|| runtime_err("ApplyImpulseAtPosition expects a Vector3"))?;
        let at = as_vector3(&at).ok_or_else(|| runtime_err("ApplyImpulseAtPosition expects a position"))?;
        let m = mass(lua, this.0);
        request(lua, this.0, VelocityRequest::Add(impulse / m));
        // Off-centre pushes spin the part too (as if it were a solid ball).
        let centre = part_cframe(lua, this.0)?.pos;
        let size = host(lua).tree.borrow().get_vec3(this.0, Property::Size).unwrap_or(Vec3::ONE);
        let radius = (size.length() * 0.5).max(0.1);
        let inertia = 0.4 * m * radius * radius;
        let spin = spin(lua, this.0) + (at - centre).cross(impulse) / inertia;
        request(lua, this.0, VelocityRequest::Spin(spin));
        Ok(())
    });
    for name in [methods::WORLD_TO_VIEWPORT_POINT, methods::WORLD_TO_SCREEN_POINT] {
        m.add_method(name, move |lua, this, point: Value| {
            require_class(lua, this.0, ClassKind::Camera, name)?;
            let point = as_vector3(&point).ok_or_else(|| runtime_err(format!("{name} expects a Vector3")))?;
            let (screen, visible) = project(lua, this.0, point)?;
            Ok((LuaVector3(screen), visible))
        });
    }
    m.add_method(methods::CAPTURE_FOCUS, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::TextBox, methods::CAPTURE_FOCUS)?;
        focus(lua, this.0);
        Ok(())
    });
    m.add_method(methods::RELEASE_FOCUS, |lua, this, enter: Option<bool>| {
        require_class(lua, this.0, ClassKind::TextBox, methods::RELEASE_FOCUS)?;
        if host(lua).world.borrow().focused == Some(this.0) {
            release(lua, enter.unwrap_or(false));
        }
        Ok(())
    });
    m.add_method(methods::IS_FOCUSED, |lua, this, ()| {
        require_class(lua, this.0, ClassKind::TextBox, methods::IS_FOCUSED)?;
        Ok(host(lua).world.borrow().focused == Some(this.0))
    });
}

/// Starts typing into `text_box`: lets go of any other one, empties it if
/// it clears on focus, and fires `Focused`.
pub(crate) fn focus(lua: &Lua, text_box: InstanceId) {
    let current = host(lua).world.borrow().focused;
    if current == Some(text_box) {
        return;
    }
    if current.is_some() {
        release(lua, false);
    }
    host(lua).world.borrow_mut().focused = Some(text_box);
    {
        let host = host(lua);
        let mut tree = host.tree.borrow_mut();
        if tree.get_bool(text_box, Property::ClearTextOnFocus) == Some(true) {
            let _ = tree.set(text_box, Property::Text, PropertyValue::String(String::new()));
        }
    }
    fire(lua, SignalKind::TextBoxFocused(text_box), || Ok(MultiValue::new()));
}

/// Stops typing, firing `FocusLost(enterPressed)` on the focused text box.
pub(crate) fn release(lua: &Lua, enter: bool) {
    let Some(text_box) = host(lua).world.borrow_mut().focused.take() else { return };
    fire(
        lua,
        SignalKind::TextBoxFocusLost(text_box),
        || Ok(std::iter::once(Value::Boolean(enter)).collect()),
    );
}

// Random numbers.

/// A seeded random number generator (a 64-bit permuted congruential one).
struct LuaRandom(std::cell::Cell<u64>);

impl LuaRandom {
    fn new(seed: u64) -> Self {
        let r = LuaRandom(std::cell::Cell::new(seed.wrapping_add(0x9E37_79B9_7F4A_7C15)));
        r.next();
        r
    }

    /// A number in [0, 1).
    fn next(&self) -> f64 {
        let state =
            self.0.get().wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0.set(state);
        let mut x = state;
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^= x >> 33;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

impl UserData for LuaRandom {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, crate::names::random::TYPE_NAME);
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        use crate::names::random::*;
        m.add_method(NEXT_NUMBER, |_, this, (min, max): (Option<f64>, Option<f64>)| {
            let (min, max) = (min.unwrap_or(0.0), max.unwrap_or(1.0));
            Ok(min + (max - min) * this.next())
        });
        m.add_method(NEXT_INTEGER, |_, this, (min, max): (i64, i64)| {
            if max < min {
                return Err(runtime_err("NextInteger: the largest number is smaller than the smallest"));
            }
            let span = (max - min + 1) as f64;
            Ok(min + ((this.next() * span).floor() as i64).min(max - min))
        });
        m.add_method("NextUnitVector", |_, this, ()| {
            // A point on the sphere, picked evenly.
            let z = this.next() * 2.0 - 1.0;
            let angle = this.next() * std::f64::consts::TAU;
            let r = (1.0 - z * z).max(0.0).sqrt();
            Ok(LuaVector3(Vec3::new((r * angle.cos()) as f32, (r * angle.sin()) as f32, z as f32)))
        });
        m.add_method("Shuffle", |_, this, list: mlua::Table| {
            let n = list.raw_len();
            for i in (2..=n).rev() {
                let j = 1 + ((this.next() * i as f64).floor() as usize).min(i - 1);
                let (a, b): (Value, Value) = (list.raw_get(i)?, list.raw_get(j)?);
                list.raw_set(i, b)?;
                list.raw_set(j, a)?;
            }
            Ok(())
        });
        m.add_method("Clone", |_, this, ()| Ok(LuaRandom(std::cell::Cell::new(this.0.get()))));
    }
}

/// Installs `Random`.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let random = lua.create_table()?;
    random.set(
        crate::names::random::NEW,
        lua.create_function(|lua, seed: Option<f64>| {
            let seed = match seed {
                Some(s) => s.to_bits(),
                // No seed: something different each time.
                None => {
                    let clock = host(lua).clock.get().to_bits();
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos() as u64)
                        .unwrap_or(0);
                    clock ^ now
                }
            };
            Ok(LuaRandom::new(seed))
        })?,
    )?;
    lua.globals().set(crate::names::globals::RANDOM, random)?;
    Ok(())
}

// Host entry points.

pub(crate) fn take_move_requests(lua: &Lua) -> Vec<(InstanceId, Option<Vec3>)> {
    std::mem::take(&mut host(lua).world.borrow_mut().moves)
}

pub(crate) fn take_sit_requests(lua: &Lua) -> Vec<(InstanceId, InstanceId)> {
    std::mem::take(&mut host(lua).world.borrow_mut().sits)
}

pub(crate) fn seated(lua: &Lua, humanoid: InstanceId, seat: Option<InstanceId>) -> Vec<ScriptError> {
    fire(lua, SignalKind::Seated(humanoid), || {
        let mut args = MultiValue::new();
        args.push_back(Value::Boolean(seat.is_some()));
        args.push_back(match seat {
            Some(s) => Value::UserData(push_instance(lua, s)?),
            None => Value::Nil,
        });
        Ok(args)
    });
    take_errors(lua)
}

pub(crate) fn take_persistent_players(lua: &Lua) -> Vec<(InstanceId, InstanceId, bool)> {
    std::mem::take(&mut host(lua).world.borrow_mut().persistent_players)
}

pub(crate) fn take_stream_requests(lua: &Lua) -> Vec<(InstanceId, Vec3, f32)> {
    std::mem::take(&mut host(lua).world.borrow_mut().stream_requests)
}

pub(crate) fn take_kicks(lua: &Lua) -> Vec<(InstanceId, String)> {
    std::mem::take(&mut host(lua).world.borrow_mut().kicks)
}

pub(crate) fn take_load_requests(lua: &Lua) -> Vec<InstanceId> {
    std::mem::take(&mut host(lua).world.borrow_mut().loads)
}

pub(crate) fn character_removing(lua: &Lua, player: InstanceId, character: InstanceId) -> Vec<ScriptError> {
    fire(lua, SignalKind::CharacterRemoving(player), || {
        Ok(std::iter::once(Value::UserData(push_instance(lua, character)?)).collect())
    });
    take_errors(lua)
}

pub(crate) fn move_finished(lua: &Lua, humanoid: InstanceId, reached: bool) -> Vec<ScriptError> {
    fire(
        lua,
        SignalKind::MoveToFinished(humanoid),
        || Ok(std::iter::once(Value::Boolean(reached)).collect()),
    );
    take_errors(lua)
}

pub(crate) fn focused_text_box(lua: &Lua) -> Option<InstanceId> {
    let focused = host(lua).world.borrow().focused?;
    let host = host(lua);
    let tree = host.tree.borrow();
    tree.exists(focused).then_some(focused).filter(|id| !tree.is_destroyed(*id).unwrap_or(true))
}

pub(crate) fn set_devices(lua: &Lua, devices: crate::Devices) {
    let before = std::mem::replace(&mut host(lua).world.borrow_mut().devices, devices);
    if before.last != devices.last {
        let item = crate::bindings::LuaEnumItem(playlibre_instance::EnumValue::UserInputType(devices.last));
        let result = crate::lenient::fire_service_event(
            lua,
            crate::names::services::USER_INPUT_SERVICE,
            "LastInputTypeChanged",
            item,
        );
        crate::services::report(lua, result);
    }
}

/// The screen GUI objects drawn over a point (pixels from the view's corner).
pub(crate) fn gui_under(lua: &Lua, x: f32, y: f32) -> Vec<InstanceId> {
    host(lua)
        .world
        .borrow()
        .gui_rects
        .iter()
        .filter(|(_, r)| x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3])
        .map(|(id, _)| *id)
        .collect()
}

/// The GUI objects inside `container` drawn over a point, topmost first,
/// leaving out those cut off there by a frame that clips them.
pub(crate) fn gui_at(lua: &Lua, container: InstanceId, x: f32, y: f32) -> Vec<InstanceId> {
    let host = host(lua);
    let world = host.world.borrow();
    let tree = host.tree.borrow();
    let inside = |r: &[f32; 4]| x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3];
    let shows = |id: InstanceId| {
        let mut at = tree.parent(id).ok().flatten();
        while let Some(a) = at {
            if a == container {
                return true;
            }
            if tree.get_bool(a, Property::ClipsDescendants) == Some(true)
                && world.gui_rects.get(&a).is_some_and(|r| !inside(r))
            {
                return false;
            }
            at = tree.parent(a).ok().flatten();
        }
        false
    };
    world
        .gui_order
        .iter()
        .rev()
        .copied()
        .filter(|id| world.gui_rects.get(id).is_some_and(inside) && shows(*id))
        .collect()
}

pub(crate) fn set_gui_rects(lua: &Lua, rects: Vec<(InstanceId, [f32; 4])>) {
    host(lua).world.borrow_mut().gui_order = rects.iter().map(|(id, _)| *id).collect();
    let new: HashMap<InstanceId, [f32; 4]> = rects.into_iter().collect();
    let old = std::mem::replace(&mut host(lua).world.borrow_mut().gui_rects, new.clone());
    // Scripts that lay things out by hand listen for these.
    for (id, r) in &new {
        let before = old.get(id);
        if before.is_some_and(|b| b[2] != r[2] || b[3] != r[3]) {
            let _ = crate::lenient::fire_changed(lua, *id, "AbsoluteSize");
        }
        if before.is_some_and(|b| b[0] != r[0] || b[1] != r[1]) {
            let _ = crate::lenient::fire_changed(lua, *id, "AbsolutePosition");
        }
    }
}

/// Where a screen GUI object was last drawn, if it was.
pub(crate) fn gui_rect(lua: &Lua, id: InstanceId) -> Option<[f32; 4]> {
    host(lua).world.borrow().gui_rects.get(&id).copied()
}

pub(crate) fn set_viewport(lua: &Lua, size: Vec2) {
    let old = std::mem::replace(&mut host(lua).world.borrow_mut().viewport, size);
    if old != size {
        // Scripts laying out their GUI listen for this.
        let camera = {
            let host = host(lua);
            let tree = host.tree.borrow();
            tree.find_first_child_of_class(tree.workspace(), ClassKind::Camera).ok().flatten()
        };
        if let Some(camera) = camera {
            let _ = crate::lenient::fire_changed(lua, camera, extras::VIEWPORT_SIZE);
        }
    }
}

pub(crate) fn set_velocities(lua: &Lua, velocities: HashMap<InstanceId, Vec3>) {
    host(lua).world.borrow_mut().velocities = velocities;
}

pub(crate) fn set_angular_velocities(lua: &Lua, spins: HashMap<InstanceId, Vec3>) {
    host(lua).world.borrow_mut().spins = spins;
}

pub(crate) fn take_velocity_requests(lua: &Lua) -> Vec<(InstanceId, VelocityRequest)> {
    std::mem::take(&mut host(lua).world.borrow_mut().requests)
}

pub(crate) fn explosion_hit(
    lua: &Lua,
    explosion: InstanceId,
    part: InstanceId,
    distance: f32,
) -> Vec<ScriptError> {
    fire(lua, SignalKind::ExplosionHit(explosion), || {
        let mut args = MultiValue::new();
        args.push_back(Value::UserData(push_instance(lua, part)?));
        args.push_back(Value::Number(distance as f64));
        Ok(args)
    });
    take_errors(lua)
}

pub(crate) fn text_box_focus(lua: &Lua, text_box: Option<InstanceId>, enter: bool) -> Vec<ScriptError> {
    match text_box {
        Some(id) => focus(lua, id),
        None => release(lua, enter),
    }
    take_errors(lua)
}
