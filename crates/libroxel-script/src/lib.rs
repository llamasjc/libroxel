//! Luau scripting for Libroxel.
//!
//! A [`ScriptRuntime`] owns one sandboxed Luau VM bound to an
//! [`InstanceTree`]. Scripts read and change the tree through the API
//! described in [`names`]; the host drives time forward with
//! [`ScriptRuntime::step`], which fires the per-frame signal.
//!
//! ```
//! use libroxel_script::ScriptRuntime;
//!
//! let runtime = ScriptRuntime::new_with_tree(Default::default()).unwrap();
//! runtime.run("hello", r#"
//!     local part = Instance.new("Part")
//!     part.Parent = workspace
//!     game:GetService("RunService").Heartbeat:Connect(function(dt)
//!         part.Position += Vector3.new(dt, 0, 0)
//!     end)
//! "#).unwrap();
//! runtime.step(0.5);
//! ```

mod animation;
mod bindings;
mod calls;
mod compat;
mod effects;
mod items;
pub mod names;
mod net;
mod pathfinding;
mod services;
mod sound;
mod terrain;
mod types;
mod world;

use std::cell::RefCell;
use std::rc::Rc;

use libroxel_instance::{InstanceId, InstanceTree, KeyCode};
use mlua::Lua;

pub use mlua;
pub use net::{NetMessage, NetRole, decode as decode_net_value, encode as encode_net_value, map_instances};
pub use world::VelocityRequest;

/// The tree, shared between the runtime and the host engine.
///
/// The VM is single-threaded, so this is a plain `Rc<RefCell<_>>`. The
/// host must not hold a borrow while calling into the runtime.
pub type SharedTree = Rc<RefCell<InstanceTree>>;

/// An error raised by a script, with the script's name.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{script}: {message}")]
pub struct ScriptError {
    pub script: String,
    pub message: String,
}

/// How a line of script output was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputLevel {
    Print,
    Warn,
}

/// One line printed by a script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLine {
    pub level: OutputLevel,
    pub text: String,
}

/// A key or mouse button, as the input service reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserInput {
    Key(KeyCode),
    MouseButton1,
    MouseButton2,
}

/// Where the player's mouse points: a ray into the world from the camera,
/// and the pointer's position on screen in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MouseRay {
    pub origin: libroxel_instance::Vec3,
    /// A unit direction.
    pub direction: libroxel_instance::Vec3,
    pub x: f32,
    pub y: f32,
}

/// How much memory one game's scripts may use together. A script that
/// tries to use more gets a "not enough memory" error instead of taking
/// the whole computer (or server) down with it.
pub const MEMORY_LIMIT: usize = 512 * 1024 * 1024;

/// A sandboxed Luau VM bound to an instance tree.
pub struct ScriptRuntime {
    lua: Lua,
    tree: SharedTree,
}

impl ScriptRuntime {
    /// Creates a runtime bound to `tree`.
    pub fn new(tree: SharedTree) -> mlua::Result<Self> {
        let lua = Lua::new();
        bindings::install(&lua, tree.clone())?;
        // Freeze the standard library and our globals; each script gets its
        // own global environment on top.
        lua.sandbox(true)?;
        lua.set_memory_limit(MEMORY_LIMIT)?;
        Ok(Self { lua, tree })
    }

    /// Creates a runtime that owns a fresh tree.
    pub fn new_with_tree(tree: InstanceTree) -> mlua::Result<Self> {
        Self::new(Rc::new(RefCell::new(tree)))
    }

    /// The tree this runtime is bound to.
    pub fn tree(&self) -> &SharedTree {
        &self.tree
    }

    /// Runs source code as a new script. Signal handlers it connects stay
    /// alive, and if it calls `task.wait` it continues during later steps.
    /// Errors raised before its first wait are returned here; later ones
    /// come back from [`step`](Self::step).
    pub fn run(&self, script_name: &str, source: &str) -> Result<(), ScriptError> {
        bindings::run_chunk(&self.lua, script_name, source, None)
    }

    /// Starts every enabled `Script` instance in the tree, in tree order,
    /// with `script` bound to the instance. Returns their startup errors.
    pub fn run_tree_scripts(&self) -> Vec<ScriptError> {
        bindings::run_tree_scripts(&self.lua)
    }

    /// Advances time by `dt` seconds: resumes threads whose wait is over,
    /// then fires the per-frame signal. Returns every script error raised
    /// since the last call; one failing script never stops the others.
    pub fn step(&self, dt: f64) -> Vec<ScriptError> {
        bindings::step(&self.lua, dt)
    }

    /// How the animations playing on `humanoid` turn its character's limbs
    /// now, and whether they move its legs. `None` while none are playing.
    pub fn animation_pose(&self, humanoid: InstanceId) -> Option<(libroxel_instance::animation::Pose, bool)> {
        animation::pose_for(&self.lua, humanoid)
    }

    /// Fires `part`'s touch event with `other` as the argument.
    pub fn fire_touched(&self, part: InstanceId, other: InstanceId) -> Vec<ScriptError> {
        bindings::fire_touched(&self.lua, part, other)
    }

    /// Fires `part.TouchEnded(other)`.
    pub fn fire_touch_ended(&self, part: InstanceId, other: InstanceId) -> Vec<ScriptError> {
        bindings::fire_touch_ended(&self.lua, part, other)
    }

    /// Takes the `Seat:Sit(humanoid)` calls since the last call, as (seat, humanoid).
    pub fn take_sit_requests(&self) -> Vec<(InstanceId, InstanceId)> {
        world::take_sit_requests(&self.lua)
    }

    /// Particle emitters scripts asked to `Emit`, with how many particles.
    pub fn take_emit_requests(&self) -> Vec<(InstanceId, u32)> {
        std::mem::take(&mut bindings::host(&self.lua).world.borrow_mut().emits)
    }

    /// Takes the tools used since the last call (each `Activated` that fired).
    pub fn take_tool_swings(&self) -> Vec<InstanceId> {
        std::mem::take(&mut bindings::host(&self.lua).world.borrow_mut().swings)
    }

    /// Takes the sounds that `Play` or `Stop` sent back to their beginning
    /// since the last call.
    pub fn take_sound_restarts(&self) -> Vec<InstanceId> {
        sound::take_sound_restarts(&self.lua)
    }

    /// A sound played to its end: sets `Playing` to false and fires `Ended`.
    pub fn sound_ended(&self, sound: InstanceId) -> Vec<ScriptError> {
        sound::sound_ended(&self.lua, sound)
    }

    /// Fires `humanoid.Seated(seat ~= nil, seat)`.
    pub fn seated(&self, humanoid: InstanceId, seat: Option<InstanceId>) -> Vec<ScriptError> {
        world::seated(&self.lua, humanoid, seat)
    }

    /// Takes the players whose characters `LoadCharacter` asked to respawn.
    pub fn take_load_requests(&self) -> Vec<InstanceId> {
        world::take_load_requests(&self.lua)
    }

    /// Fires `player.CharacterRemoving(character)`.
    pub fn character_removing(&self, player: InstanceId, character: InstanceId) -> Vec<ScriptError> {
        world::character_removing(&self.lua, player, character)
    }

    /// Starts every enabled `LocalScript` under `ancestor` (such as a
    /// player's GUI after it is copied in).
    pub fn run_local_scripts(&self, ancestor: InstanceId) -> Vec<ScriptError> {
        bindings::run_scripts_under(&self.lua, ancestor, libroxel_instance::ClassKind::LocalScript)
    }

    /// Fires change events (`Changed`, `GetPropertyChangedSignal`,
    /// `ChildAdded`) for changes the host has drained from the tree.
    pub fn fire_changes(&self, changes: &[libroxel_instance::Change]) -> Vec<ScriptError> {
        bindings::fire_changes(&self.lua, changes)
    }

    /// Fires a text button's click events, as if the player clicked it.
    /// Fires a GUI object's `MouseEnter` (or `MouseLeave` when `entered` is false).
    pub fn mouse_over(&self, object: InstanceId, entered: bool) -> Vec<ScriptError> {
        bindings::mouse_over(&self.lua, object, entered)
    }

    pub fn click_button(&self, button: InstanceId) -> Vec<ScriptError> {
        bindings::click_button(&self.lua, button)
    }

    /// Fires a click detector's click event with the player who clicked.
    pub fn click_detector(&self, detector: InstanceId, player: InstanceId) -> Vec<ScriptError> {
        bindings::fire_with_instance(&self.lua, bindings::SignalKind::MouseClick(detector), player)
    }

    /// Fires the players service's join event for `player`.
    pub fn player_added(&self, player: InstanceId) -> Vec<ScriptError> {
        bindings::fire_with_instance(&self.lua, bindings::SignalKind::PlayerAdded, player)
    }

    /// Fires the players service's leave event for `player`.
    pub fn player_removing(&self, player: InstanceId) -> Vec<ScriptError> {
        bindings::fire_with_instance(&self.lua, bindings::SignalKind::PlayerRemoving, player)
    }

    /// Fires `player`'s character event with its new `model`.
    pub fn character_added(&self, player: InstanceId, model: InstanceId) -> Vec<ScriptError> {
        bindings::fire_with_instance(&self.lua, bindings::SignalKind::CharacterAdded(player), model)
    }

    /// Fires `humanoid`'s death event.
    pub fn humanoid_died(&self, humanoid: InstanceId) -> Vec<ScriptError> {
        bindings::fire_plain(&self.lua, bindings::SignalKind::Died(humanoid))
    }

    /// A key or mouse button went down. `processed` is true when the
    /// game's own UI used it (for example, a click on a GUI button).
    pub fn input_began(&self, input: UserInput, processed: bool) -> Vec<ScriptError> {
        services::input(&self.lua, input, true, processed)
    }

    /// A key or mouse button came back up.
    pub fn input_ended(&self, input: UserInput, processed: bool) -> Vec<ScriptError> {
        services::input(&self.lua, input, false, processed)
    }

    /// Fires a proximity prompt's `Triggered` event with the player who used it.
    pub fn prompt_triggered(&self, prompt: InstanceId, player: InstanceId) -> Vec<ScriptError> {
        services::prompt(&self.lua, prompt, player, false)
    }

    /// Fires a proximity prompt's `TriggerEnded` event when the player lets go.
    pub fn prompt_trigger_ended(&self, prompt: InstanceId, player: InstanceId) -> Vec<ScriptError> {
        services::prompt(&self.lua, prompt, player, true)
    }

    /// Replaces the saved data that data stores read from, with JSON from [`save_data`](Self::save_data).
    pub fn load_data(&self, json: &str) -> Result<(), String> {
        services::load_data(&self.lua, json)
    }

    /// Everything data stores hold, as JSON to keep between play sessions.
    pub fn save_data(&self) -> String {
        services::save_data(&self.lua)
    }

    /// Tells scripts where the mouse points (for `Player:GetMouse()`), or
    /// that it is off the view with `None`.
    pub fn set_mouse(&self, mouse: Option<MouseRay>) {
        items::set_mouse(&self.lua, mouse)
    }

    /// Starts every enabled `Script` under `ancestor` (such as a tool copied into a backpack).
    pub fn run_scripts(&self, ancestor: InstanceId) -> Vec<ScriptError> {
        bindings::run_scripts_under(&self.lua, ancestor, libroxel_instance::ClassKind::Script)
    }

    /// Tells scripts how big the 3D view is, in pixels (for the camera).
    pub fn set_viewport(&self, width: f32, height: f32) {
        world::set_viewport(&self.lua, libroxel_instance::Vec2::new(width, height))
    }

    /// Tells scripts how fast each moving part is going, after a physics step.
    pub fn set_velocities(&self, velocities: std::collections::HashMap<InstanceId, libroxel_instance::Vec3>) {
        world::set_velocities(&self.lua, velocities)
    }

    /// Tells scripts how fast each part spins (`AssemblyAngularVelocity`).
    pub fn set_angular_velocities(
        &self,
        spins: std::collections::HashMap<InstanceId, libroxel_instance::Vec3>,
    ) {
        world::set_angular_velocities(&self.lua, spins)
    }

    /// Takes the velocity changes scripts asked for since the last call, in order.
    /// Pairs of collision groups that scripts said pass through each other.
    pub fn collision_rules(&self) -> Vec<(String, String)> {
        compat::collision_rules(&self.lua)
    }

    pub fn take_velocity_requests(&self) -> Vec<(InstanceId, VelocityRequest)> {
        world::take_velocity_requests(&self.lua)
    }

    /// Fires an explosion's `Hit` event for one part it reached.
    pub fn explosion_hit(&self, explosion: InstanceId, part: InstanceId, distance: f32) -> Vec<ScriptError> {
        world::explosion_hit(&self.lua, explosion, part, distance)
    }

    /// Takes the walks scripts asked humanoids for since the last call:
    /// a point to walk to, or `None` to stop.
    pub fn take_move_requests(&self) -> Vec<(InstanceId, Option<libroxel_instance::Vec3>)> {
        world::take_move_requests(&self.lua)
    }

    /// Fires a humanoid's `MoveToFinished` with whether it got there.
    pub fn move_finished(&self, humanoid: InstanceId, reached: bool) -> Vec<ScriptError> {
        world::move_finished(&self.lua, humanoid, reached)
    }

    /// The text box the player is typing into, if any.
    pub fn focused_text_box(&self) -> Option<InstanceId> {
        world::focused_text_box(&self.lua)
    }

    /// The player clicked into a text box (`Some`), or left the focused one
    /// (`None`, with whether they pressed Enter).
    pub fn focus_text_box(&self, text_box: Option<InstanceId>, enter: bool) -> Vec<ScriptError> {
        world::text_box_focus(&self.lua, text_box, enter)
    }

    /// Seconds of game time so far (the sum of every `step`).
    pub fn clock(&self) -> f64 {
        bindings::clock(&self.lua)
    }

    /// Number of threads paused in a wait or delay.
    pub fn waiting_count(&self) -> usize {
        bindings::waiting_count(&self.lua)
    }

    /// Takes everything scripts have printed since the last call.
    pub fn take_output(&self) -> Vec<OutputLine> {
        bindings::take_output(&self.lua)
    }

    /// The number of live signal connections.
    pub fn connection_count(&self) -> usize {
        bindings::connection_count(&self.lua)
    }

    /// Says which side of a networked game this runtime is. Remote events
    /// fired toward the other side become [`NetMessage`]s.
    pub fn set_net_role(&self, role: NetRole) {
        bindings::host(&self.lua).net.borrow_mut().role = role;
    }

    pub fn net_role(&self) -> NetRole {
        net::role(&self.lua)
    }

    /// Names the player on this computer (`Players.LocalPlayer`). Without
    /// one, a game on one computer uses the first player, and a server has none.
    pub fn set_local_player(&self, player: Option<InstanceId>) {
        bindings::host(&self.lua).net.borrow_mut().local_player = player;
    }

    /// Takes what scripts sent toward the other side since the last call.
    pub fn take_net_messages(&self) -> Vec<NetMessage> {
        std::mem::take(&mut bindings::host(&self.lua).net.borrow_mut().outbox)
    }

    /// On the server: `player`'s computer fired `remote` with `args` (as
    /// [`encode_net_value`] makes them, in this runtime's ids).
    pub fn deliver_server_event(
        &self,
        remote: InstanceId,
        player: InstanceId,
        args: &libroxel_instance::serde_json::Value,
    ) -> Vec<ScriptError> {
        net::server_event(&self.lua, remote, player, args)
    }

    /// On a player's computer: the server fired `remote` at it.
    pub fn deliver_client_event(
        &self,
        remote: InstanceId,
        args: &libroxel_instance::serde_json::Value,
    ) -> Vec<ScriptError> {
        net::client_event(&self.lua, remote, args)
    }

    /// On the server: `player` called `remote:InvokeServer`. The answer
    /// comes out later as a [`NetMessage::InvokeResult`].
    pub fn deliver_invoke(
        &self,
        call: u64,
        remote: InstanceId,
        player: InstanceId,
        args: &libroxel_instance::serde_json::Value,
    ) -> Vec<ScriptError> {
        net::serve_invoke(&self.lua, call, remote, player, args)
    }

    /// On a player's computer: the server answered call `call`.
    pub fn deliver_invoke_result(&self, call: u64, ok: bool, values: libroxel_instance::serde_json::Value) {
        net::invoke_result(&self.lua, call, ok, values)
    }

    /// Fires `tool`'s `Activated` (or `Deactivated`), as when a player on
    /// another computer clicks while holding it.
    pub fn activate_tool(&self, tool: InstanceId, down: bool) -> Vec<ScriptError> {
        items::activate(&self.lua, tool, down);
        bindings::take_errors(&self.lua)
    }
}
