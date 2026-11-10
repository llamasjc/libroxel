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

use std::cell::RefCell;
use std::rc::Rc;

use libroxel_instance::InstanceTree;
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

    /// Runs a script to completion. Signal handlers it connects stay alive.
    pub fn run(&self, script_name: &str, source: &str) -> Result<(), ScriptError> {
        let fail =
            |err: mlua::Error| ScriptError { script: script_name.to_owned(), message: err.to_string() };
        bindings::set_current_script(&self.lua, script_name);
        // Each script gets its own global environment that falls back to the
        // shared, read-only globals, so scripts can't leak globals into each other.
        let env = self.lua.create_table().map_err(fail)?;
        let meta = self.lua.create_table().map_err(fail)?;
        meta.set("__index", self.lua.globals()).map_err(fail)?;
        env.set_metatable(Some(meta)).map_err(fail)?;
        self.lua.load(source).set_name(format!("={script_name}")).set_environment(env).exec().map_err(fail)
    }

    /// Advances time by `dt` seconds, firing the per-frame signal. Errors
    /// from individual handlers are returned; the other handlers still run.
    pub fn step(&self, dt: f64) -> Vec<ScriptError> {
        bindings::fire_heartbeat(&self.lua, dt)
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
