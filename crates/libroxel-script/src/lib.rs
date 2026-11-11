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

mod bindings;
pub mod names;
mod services;
mod types;

use std::cell::RefCell;
use std::rc::Rc;

use libroxel_instance::{InstanceId, InstanceTree, KeyCode};
use mlua::Lua;

pub use mlua;

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

    /// Fires `part`'s touch event with `other` as the argument.
    pub fn fire_touched(&self, part: InstanceId, other: InstanceId) -> Vec<ScriptError> {
        bindings::fire_touched(&self.lua, part, other)
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
}
