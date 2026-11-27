//! Luau scripting for PlayLibre.
//!
//! A [`ScriptRuntime`] owns one sandboxed Luau VM bound to an
//! [`InstanceTree`]. Scripts read and change the tree through the API
//! described in [`names`]; the host drives time forward with
//! [`ScriptRuntime::step`], which fires the per-frame signal.
//!
//! ```
//! use playlibre_script::ScriptRuntime;
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
pub mod drawn;
mod effects;
pub mod filter;
mod items;
mod keyframes;
mod lenient;
pub mod names;
mod net;
mod pathfinding;
mod reflection;
mod services;
pub mod shield;
mod sound;
mod terrain;
mod types;
mod world;

use std::cell::RefCell;
use std::rc::Rc;

use mlua::Lua;
use playlibre_instance::{InstanceId, InstanceTree, KeyCode, Vec3};

pub use mlua;

/// Whether the 3D view should be drawn: a script can turn it off
/// (`RunService:Set3dRenderingEnabled(false)`, as loading screens do).
pub static RENDERING_3D: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
pub use net::{NetMessage, NetRole, decode as decode_net_value, encode as encode_net_value, map_instances};
pub use world::{
    BadgeInfo, GameIdentity, JoinData, PassInfo, ProductInfo, ReceiptInfo, Teleport, VelocityRequest,
};

/// What data stores hold: store, then key, then value.
pub type SavedData =
    std::collections::BTreeMap<String, std::collections::BTreeMap<String, serde_json::Value>>;

/// Measures GUI text: the text, its size in points and the width it wraps
/// at, giving how wide and tall it is. The app hands in the one its GUI
/// draws with, so `TextService:GetTextSize` matches what players see.
pub type TextMeasure = fn(&str, f32, f32) -> (f32, f32);

static TEXT_MEASURE: std::sync::OnceLock<TextMeasure> = std::sync::OnceLock::new();

/// Sets how [`TextMeasure`] text is measured. Only the first call counts.
pub fn set_text_measure(measure: TextMeasure) {
    let _ = TEXT_MEASURE.set(measure);
}

/// How big `text` is at `size`, wrapped at `width`: with the app's measure
/// if it gave one, or estimated (half a point of width a character, wrapped
/// at spaces, one `size` a line).
pub(crate) fn measure_text(text: &str, size: f32, width: f32) -> (f32, f32) {
    if text.is_empty() {
        return (0.0, size);
    }
    if let Some(measure) = TEXT_MEASURE.get() {
        return measure(text, size, width);
    }
    let char_width = size * 0.5;
    let max_chars = if width.is_finite() && width > 0.0 {
        (width / char_width).floor().max(1.0) as usize
    } else {
        usize::MAX
    };
    let mut lines = 0usize;
    let mut widest = 0usize;
    for line in text.split('\n') {
        let mut current = 0usize;
        lines += 1;
        for word in line.split(' ') {
            let n = word.chars().count();
            let needed = if current == 0 { n } else { current + 1 + n };
            if needed <= max_chars || current == 0 {
                current = needed;
                // A word longer than the line breaks across lines.
                while current > max_chars {
                    widest = widest.max(max_chars);
                    current -= max_chars;
                    lines += 1;
                }
            } else {
                widest = widest.max(current);
                lines += 1;
                current = n.min(max_chars);
            }
        }
        widest = widest.max(current);
    }
    (widest as f32 * char_width, lines as f32 * size)
}

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

/// What the player plays with, for scripts' input service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Devices {
    /// They have touched the screen.
    pub touch: bool,
    /// A game controller is plugged in.
    pub gamepad: bool,
    /// The kind of input they used last.
    pub last: playlibre_instance::UserInputType,
    /// There is a keyboard (not on phones and tablets).
    pub keyboard: bool,
    /// There is a mouse (not on phones and tablets).
    pub mouse: bool,
    /// The device can vibrate (phones), for `HapticService`.
    pub vibration: bool,
}

impl Default for Devices {
    fn default() -> Self {
        Devices {
            touch: false,
            gamepad: false,
            last: playlibre_instance::UserInputType::default(),
            keyboard: true,
            mouse: true,
            vibration: false,
        }
    }
}

/// A button on the touch screen that a script asked for with
/// `ContextActionService:BindAction(name, handler, true, ...)`.
#[derive(Debug, Clone, PartialEq)]
pub struct TouchButton {
    /// The action's name, to press it with [`Runtime::press_touch_button`].
    pub name: String,
    /// What `SetTitle` put on it.
    pub title: Option<String>,
    /// What `SetImage` put on it.
    pub image: Option<String>,
    /// Where `SetPosition` put it: X scale, X offset, Y scale, Y offset.
    pub position: Option<[f32; 4]>,
}

/// Where a finger on the screen is in its touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase {
    Began,
    Moved,
    Ended,
}

/// Where the player's mouse points: a ray into the world from the camera,
/// and the pointer's position on screen in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MouseRay {
    pub origin: playlibre_instance::Vec3,
    /// A unit direction.
    pub direction: playlibre_instance::Vec3,
    pub x: f32,
    pub y: f32,
}

/// The space the reference platform's top bar takes at the top of the screen, in pixels:
/// what `GuiService:GetGuiInset()` reports, and where screen GUIs that
/// don't ignore the inset start.
pub const GUI_INSET: f32 = 58.0;

/// How much memory one game's scripts may use together on a computer. A
/// script that tries to use more gets a "not enough memory" error instead
/// of taking the whole computer (or server) down with it.
pub const MEMORY_LIMIT: usize = 2048 * 1024 * 1024;
pub const PLUGIN_MEMORY_LIMIT: usize = 256 * 1024 * 1024;

/// The same on a phone or tablet, which has far less to spare.
pub const MOBILE_MEMORY_LIMIT: usize = 768 * 1024 * 1024;

/// The same for a computer that joined someone else's game: it runs only
/// its own player's local scripts, and the game is someone else's code.
pub const JOINED_MEMORY_LIMIT: usize = 512 * 1024 * 1024;

/// [`MEMORY_LIMIT`], or [`MOBILE_MEMORY_LIMIT`] on a phone or tablet.
pub fn device_memory_limit() -> usize {
    if cfg!(any(target_os = "android", target_os = "ios")) { MOBILE_MEMORY_LIMIT } else { MEMORY_LIMIT }
}

/// Script threads waiting at one place: see [`ScriptRuntime::waiting_report`].
#[derive(Debug, Clone, PartialEq)]
pub struct WaitingThread {
    /// The script the threads belong to.
    pub script: String,
    /// Where they wait: the stack, innermost first.
    pub trace: String,
    /// How many threads wait there.
    pub count: usize,
    /// The longest any of them has waited so far, in seconds.
    pub seconds: f64,
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
        // As the reference platform runs live games: optimised, with inlining, and turned
        // into machine code where the computer allows it.
        // Like the reference platform: these globals are instances whose members change, so
        // reads through them (`workspace.CurrentCamera`) are never cached.
        lua.set_compiler(
            mlua::chunk::Compiler::new().set_optimization_level(2).set_debug_level(1).set_mutable_globals([
                "game",
                "Game",
                "workspace",
                "Workspace",
                "script",
                "shared",
                "plugin",
                "_G",
            ]),
        );
        #[cfg(not(target_arch = "arm"))]
        lua.enable_jit(!cfg!(target_os = "ios"));
        bindings::install(&lua, tree.clone())?;
        // Freeze the standard library and our globals; each script gets its
        // own global environment on top.
        lua.sandbox(true)?;
        lenient::after_sandbox(&lua)?;
        lua.set_memory_limit(device_memory_limit())?;
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

    /// Those of `ids` that scripts may still reach (an object for one is
    /// alive in a script, or something is connected to its events), so
    /// they must not be freed yet.
    pub fn held(&self, ids: &[InstanceId]) -> std::collections::HashSet<InstanceId> {
        bindings::held(&self.lua, ids)
    }

    /// Runs source code as a new script. Signal handlers it connects stay
    /// alive, and if it calls `task.wait` it continues during later steps.
    /// Errors raised before its first wait are returned here; later ones
    /// come back from [`step`](Self::step).
    pub fn run(&self, script_name: &str, source: &str) -> Result<(), ScriptError> {
        bindings::run_chunk(&self.lua, script_name, source, None)
    }

    pub fn run_plugin(
        &self,
        name: &str,
        source: &str,
        selection: &[InstanceId],
    ) -> Result<Vec<InstanceId>, ScriptError> {
        let _ = self.lua.set_memory_limit(PLUGIN_MEMORY_LIMIT);
        let picked = Rc::new(RefCell::new(selection.to_vec()));
        bindings::run_plugin(&self.lua, name, source, picked.clone())?;
        Ok(picked.take())
    }

    /// Starts every enabled `Script` instance in the tree, in tree order,
    /// with `script` bound to the instance. Returns their startup errors.
    pub fn run_tree_scripts(&self) -> Vec<ScriptError> {
        self.register_saved_animations();
        bindings::run_tree_scripts(&self.lua)
    }

    /// Registers each keyframe sequence a file reader brought in for an
    /// asset address (marked with [`playlibre_instance::ANIMATION_ASSET`]).
    fn register_saved_animations(&self) {
        let marked: Vec<(String, InstanceId)> = {
            let tree = self.tree().borrow();
            let Ok(all) = tree.descendants(tree.root()) else { return };
            all.into_iter()
                .filter_map(|id| match tree.get_extra(id, playlibre_instance::ANIMATION_ASSET) {
                    Ok(Some(playlibre_instance::PropertyValue::String(address))) => Some((address, id)),
                    _ => None,
                })
                .collect()
        };
        for (address, id) in marked {
            let _ = self.register_animation(&address, id);
        }
    }

    /// Advances time by `dt` seconds: resumes threads whose wait is over,
    /// then fires the per-frame signal. Returns every script error raised
    /// since the last call; one failing script never stops the others.
    pub fn step(&self, dt: f64) -> Vec<ScriptError> {
        bindings::step(&self.lua, dt)
    }

    /// How the animations playing on `humanoid` turn its character's limbs
    /// now, and whether they move its legs. `None` while none are playing.
    pub fn animation_pose(
        &self,
        humanoid: InstanceId,
    ) -> Option<(playlibre_instance::animation::Pose, bool)> {
        animation::pose_for(&self.lua, humanoid)
    }

    /// Makes animations whose `AnimationId` is `id` (such as
    /// `rbxassetid://123`; other spellings of the same asset match too)
    /// play the keyframe sequence `sequence`, as it is now. This is how an
    /// animation kept in a local file is supplied for an asset id.
    pub fn register_animation(&self, id: &str, sequence: InstanceId) -> mlua::Result<()> {
        animation::register(&self.lua, id, sequence)
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

    /// Takes the `AddPersistentPlayer` (true) and `RemovePersistentPlayer`
    /// (false) calls: model, player.
    pub fn take_persistent_players(&self) -> Vec<(InstanceId, InstanceId, bool)> {
        world::take_persistent_players(&self.lua)
    }

    /// Takes the places `RequestStreamAroundAsync` asked to stream to
    /// players: player, where, and for how many seconds.
    pub fn take_stream_requests(&self) -> Vec<(InstanceId, Vec3, f32)> {
        world::take_stream_requests(&self.lua)
    }

    /// Takes the badges `AwardBadge` gave: user id, badge id.
    pub fn take_badge_awards(&self) -> Vec<(i64, String)> {
        std::mem::take(&mut bindings::host(&self.lua).world.borrow_mut().badges.awards)
    }

    pub fn set_badge_info(&self, info: Vec<BadgeInfo>) {
        bindings::host(&self.lua).world.borrow_mut().badges.info = info;
    }

    pub fn add_owned_badges(&self, user_id: i64, ids: impl IntoIterator<Item = String>) {
        let host = bindings::host(&self.lua);
        let mut world = host.world.borrow_mut();
        world.badges.owned.extend(ids.into_iter().map(|id| (user_id, id)));
    }

    /// The game's passes, from the Hub. From now on `UserOwnsGamePassAsync`
    /// waits for each player's passes (see [`add_owned_passes`](Self::add_owned_passes))
    /// and purchase prompts go to the host (see [`take_pass_prompts`](Self::take_pass_prompts)).
    pub fn set_pass_info(&self, info: Vec<PassInfo>) {
        let host = bindings::host(&self.lua);
        let mut world = host.world.borrow_mut();
        world.passes.on = true;
        world.passes.info = info;
    }

    /// The passes a player owns, once the host has looked them up.
    pub fn add_owned_passes(&self, user_id: i64, ids: impl IntoIterator<Item = i64>) {
        let host = bindings::host(&self.lua);
        let mut world = host.world.borrow_mut();
        world.passes.known.insert(user_id);
        world.passes.owned.extend(ids.into_iter().map(|id| (user_id, id)));
    }

    /// Takes the `PromptGamePassPurchase` calls waiting: user id, pass id.
    pub fn take_pass_prompts(&self) -> Vec<(i64, i64)> {
        std::mem::take(&mut bindings::host(&self.lua).world.borrow_mut().passes.prompts)
    }

    /// A purchase prompt closed: fires `PromptGamePassPurchaseFinished` for
    /// the player, after noting the pass as theirs when they bought it.
    pub fn pass_prompt_finished(&self, user_id: i64, pass_id: i64, bought: bool) -> Vec<ScriptError> {
        if bought {
            let host = bindings::host(&self.lua);
            host.world.borrow_mut().passes.owned.insert((user_id, pass_id));
        }
        let source = format!(
            "local userId, passId, bought = {user_id}, {pass_id}, {bought}
             local market = game:GetService(\"MarketplaceService\")
             for _, player in game:GetService(\"Players\"):GetPlayers() do
                 if player.UserId == userId then
                     market.PromptGamePassPurchaseFinished:Fire(player, passId, bought)
                     return
                 end
             end"
        );
        match self.run("MarketplaceService", &source) {
            Ok(()) => bindings::take_errors(&self.lua),
            Err(e) => vec![e],
        }
    }

    /// Which game, place and server this is (`game.PlaceId` and the like).
    pub fn set_game_identity(&self, identity: GameIdentity) {
        bindings::host(&self.lua).world.borrow_mut().teleports.identity = identity;
    }

    /// What a player arrived with, for `Player:GetJoinData()`.
    pub fn set_join_data(&self, user_id: i64, join: JoinData) {
        bindings::host(&self.lua).world.borrow_mut().teleports.joins.insert(user_id, join);
    }

    /// On a player's computer: what they arrived with, for
    /// `TeleportService:GetLocalPlayerTeleportData()`.
    pub fn set_local_join_data(&self, join: JoinData) {
        bindings::host(&self.lua).world.borrow_mut().teleports.local = join;
    }

    /// Takes the teleports scripts asked for.
    pub fn take_teleports(&self) -> Vec<Teleport> {
        std::mem::take(&mut bindings::host(&self.lua).world.borrow_mut().teleports.queued)
    }

    /// The game's developer products, from the Hub. From now on purchase
    /// prompts for them go to the host (see [`take_product_prompts`](Self::take_product_prompts)).
    pub fn set_product_info(&self, info: Vec<ProductInfo>) {
        let host = bindings::host(&self.lua);
        let mut world = host.world.borrow_mut();
        world.products.on = true;
        world.products.info = info;
    }

    /// Takes the `PromptProductPurchase` calls waiting: user id, product id.
    pub fn take_product_prompts(&self) -> Vec<(i64, i64)> {
        std::mem::take(&mut bindings::host(&self.lua).world.borrow_mut().products.prompts)
    }

    /// A product's purchase prompt closed: fires `PromptProductPurchaseFinished`.
    pub fn product_prompt_finished(&self, user_id: i64, product_id: i64, bought: bool) -> Vec<ScriptError> {
        let source = format!(
            "game:GetService(\"MarketplaceService\").PromptProductPurchaseFinished:Fire({user_id}, {product_id}, {bought})"
        );
        match self.run("MarketplaceService", &source) {
            Ok(()) => bindings::take_errors(&self.lua),
            Err(e) => vec![e],
        }
    }

    /// Hands a purchase to `MarketplaceService.ProcessReceipt`, unless it is
    /// already looking at it. Its answer comes back from
    /// [`take_receipt_results`](Self::take_receipt_results); without a
    /// callback the receipt isn't granted.
    pub fn process_receipt(&self, receipt: &ReceiptInfo) -> Vec<ScriptError> {
        if !bindings::host(&self.lua).world.borrow_mut().products.busy.insert(receipt.id) {
            return Vec::new();
        }
        let start = || -> mlua::Result<()> {
            let done = self.lua.create_function(|lua, (id, granted): (i64, bool)| {
                let host = bindings::host(lua);
                let mut world = host.world.borrow_mut();
                if world.products.busy.remove(&id) {
                    world.products.done.push((id, granted));
                }
                Ok(())
            })?;
            let func = self
                .lua
                .load(
                    "local done, id, userId, productId, spent = ...
                     local market = game:GetService(\"MarketplaceService\")
                     local callback = rawget(market, \"ProcessReceipt\")
                     if type(callback) ~= \"function\" then
                         done(id, false)
                         return
                     end
                     local ok, decision = pcall(callback, {
                         PlayerId = userId,
                         PlaceIdWherePurchased = game.PlaceId,
                         PurchaseId = tostring(id),
                         ProductId = productId,
                         CurrencyType = Enum.CurrencyType.Robux,
                         CurrencySpent = spent,
                     })
                     done(id, ok and decision == Enum.ProductPurchaseDecision.PurchaseGranted)
                     if not ok then
                         error(decision, 0)
                     end",
                )
                .set_name("=MarketplaceService")
                .into_function()?;
            let args = (done, receipt.id, receipt.user_id, receipt.product_id, receipt.price);
            let args = mlua::IntoLuaMulti::into_lua_multi(args, &self.lua)?;
            bindings::spawn(&self.lua, func, args, "MarketplaceService", None)
        };
        if let Err(e) = start() {
            bindings::host(&self.lua).world.borrow_mut().products.busy.remove(&receipt.id);
            return vec![ScriptError { script: "MarketplaceService".into(), message: e.to_string() }];
        }
        bindings::take_errors(&self.lua)
    }

    /// Takes what `ProcessReceipt` answered: receipt id, whether it granted it.
    pub fn take_receipt_results(&self) -> Vec<(i64, bool)> {
        std::mem::take(&mut bindings::host(&self.lua).world.borrow_mut().products.done)
    }

    /// Takes the players `Kick` removed, with why.
    pub fn take_kicks(&self) -> Vec<(InstanceId, String)> {
        world::take_kicks(&self.lua)
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
        bindings::run_scripts_under(&self.lua, ancestor, playlibre_instance::ClassKind::LocalScript)
    }

    /// Fires change events (`Changed`, `GetPropertyChangedSignal`,
    /// `ChildAdded`) for changes the host has drained from the tree.
    pub fn fire_changes(&self, changes: &[playlibre_instance::Change]) -> Vec<ScriptError> {
        bindings::fire_changes(&self.lua, changes)
    }

    /// Fires a text button's click events, as if the player clicked it.
    /// Fires a GUI object's `MouseEnter` (or `MouseLeave` when `entered` is false).
    pub fn mouse_over(&self, object: InstanceId, entered: bool) -> Vec<ScriptError> {
        bindings::mouse_over(&self.lua, object, entered)
    }

    /// Fires a GUI button's `MouseButton1Down` (or `MouseButton1Up`).
    pub fn press_button(&self, button: InstanceId, down: bool) -> Vec<ScriptError> {
        bindings::press_button(&self.lua, button, down)
    }

    /// Fires a GUI object's `InputBegan` (or `InputEnded`) for a mouse button.
    pub fn gui_input(&self, object: InstanceId, input: UserInput, began: bool) -> Vec<ScriptError> {
        bindings::gui_input(&self.lua, object, input, began)
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
    /// Shuts the game down: runs what scripts gave `game:BindToClose`
    /// until it finishes (call after every player has left).
    pub fn close(&self) -> Vec<ScriptError> {
        bindings::close(&self.lua)
    }

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

    /// Everything data stores hold: store, then key, then value. A server
    /// that keeps its data elsewhere compares two of these to see what
    /// scripts changed.
    pub fn data_snapshot(&self) -> SavedData {
        bindings::host(&self.lua).data.borrow().clone()
    }

    /// Puts a value saved elsewhere (another server of the same game) into
    /// a data store, or removes the key with `None`, without counting it as
    /// a change scripts made.
    pub fn set_saved_value(&self, store: &str, key: &str, value: Option<serde_json::Value>) {
        services::set_saved_value(&self.lua, store, key, value)
    }

    /// Tells scripts where the mouse points (for `Player:GetMouse()`), or
    /// that it is off the view with `None`.
    pub fn set_mouse(&self, mouse: Option<MouseRay>) {
        items::set_mouse(&self.lua, mouse)
    }

    /// Starts every enabled `Script` under `ancestor` (such as a tool copied into a backpack).
    pub fn run_scripts(&self, ancestor: InstanceId) -> Vec<ScriptError> {
        bindings::run_scripts_under(&self.lua, ancestor, playlibre_instance::ClassKind::Script)
    }

    /// Tells scripts what the player plays with (touch, a controller).
    pub fn set_devices(&self, devices: Devices) -> Vec<ScriptError> {
        world::set_devices(&self.lua, devices);
        bindings::take_errors(&self.lua)
    }

    /// A finger touched, moved on or left the screen at `x`, `y` (pixels
    /// from the view's top left corner) at `time` seconds. `processed` is
    /// true when the game's UI or the on-screen controls took it. Scripts
    /// hear of it through the input service's touch events and gestures.
    pub fn touch(
        &self,
        id: u64,
        phase: TouchPhase,
        x: f32,
        y: f32,
        processed: bool,
        time: f64,
    ) -> Vec<ScriptError> {
        let phase = match phase {
            TouchPhase::Began => "began",
            TouchPhase::Moved => "moved",
            TouchPhase::Ended => "ended",
        };
        let result = lenient::touch_event(&self.lua, id, phase, x, y, processed, time);
        services::report(&self.lua, result);
        bindings::take_errors(&self.lua)
    }

    /// Lets time pass for fingers held still (a long press), at `time` seconds.
    pub fn touch_tick(&self, time: f64) -> Vec<ScriptError> {
        let result = lenient::touch_tick(&self.lua, time);
        services::report(&self.lua, result);
        bindings::take_errors(&self.lua)
    }

    /// The touch buttons scripts bound actions to, in the order bound.
    pub fn touch_buttons(&self) -> Vec<TouchButton> {
        compat::touch_buttons(&self.lua).unwrap_or_default()
    }

    /// A touch button went down (`pressed`) or up: runs its action.
    pub fn press_touch_button(&self, name: &str, pressed: bool) -> Vec<ScriptError> {
        if let Err(err) = compat::press_touch_button(&self.lua, name, pressed) {
            bindings::host(&self.lua).errors.borrow_mut().push(ScriptError {
                script: names::compat::CONTEXT_ACTION_SERVICE.to_owned(),
                message: err.to_string(),
            });
        }
        bindings::take_errors(&self.lua)
    }

    /// The strength (0 to 1) a script last set the phone's vibration motor
    /// to with `HapticService:SetMotor`, if it changed since last asked.
    pub fn take_vibration(&self) -> Option<f32> {
        bindings::host(&self.lua).world.borrow_mut().vibration.take()
    }

    /// Whether the game wants the on-screen stick and jump button (scripts
    /// can hide them as on the reference platform).
    pub fn touch_controls_shown(&self) -> bool {
        lenient::touch_controls_shown(&self.lua)
    }

    /// Tells scripts where each screen GUI object was drawn (its corner
    /// and size, in pixels from the view's corner).
    pub fn set_gui_rects(&self, rects: Vec<(playlibre_instance::InstanceId, [f32; 4])>) {
        world::set_gui_rects(&self.lua, rects)
    }

    /// Tells scripts how big the 3D view is, in pixels (for the camera).
    /// Whether scripts left one of the reference platform's own on-screen parts on
    /// (`StarterGui:SetCoreGuiEnabled`), such as "PlayerList".
    pub fn core_gui_enabled(&self, kind: &str) -> bool {
        self.lua
            .load("local kind = ... return game:GetService('StarterGui'):GetCoreGuiEnabled(Enum.CoreGuiType[kind])")
            .set_name("=core gui")
            .call::<bool>(kind)
            .unwrap_or(true)
    }

    /// The local player's camera: whether it is held in first person
    /// (`CameraMode.LockFirstPerson`), and how close and far it may zoom.
    pub fn camera_settings(&self) -> (bool, f32, f32) {
        self.lua
            .load(
                "local p = game:GetService('Players').LocalPlayer \
                 local s = game:GetService('StarterPlayer') \
                 local function lock(o) return o ~= nil and string.find(tostring(o.CameraMode), 'LockFirstPerson') ~= nil end \
                 local low, high = s.CameraMinZoomDistance, s.CameraMaxZoomDistance \
                 if p then low, high = math.max(low, p.CameraMinZoomDistance), math.min(high, p.CameraMaxZoomDistance) end \
                 return lock(p) or lock(s), low, high",
            )
            .set_name("=camera")
            .call::<(bool, f32, f32)>(())
            .unwrap_or((false, 0.5, 400.0))
    }

    pub fn reset_allowed(&self) -> bool {
        self.lua
            .load("return game:GetService('StarterGui'):GetCore('ResetButtonCallback') ~= false")
            .set_name("=reset")
            .call::<bool>(())
            .unwrap_or(true)
    }

    pub fn set_viewport(&self, width: f32, height: f32) {
        world::set_viewport(&self.lua, playlibre_instance::Vec2::new(width, height))
    }

    /// Tells scripts how fast each moving part is going, after a physics step.
    pub fn set_velocities(
        &self,
        velocities: std::collections::HashMap<InstanceId, playlibre_instance::Vec3>,
    ) {
        world::set_velocities(&self.lua, velocities)
    }

    /// Tells scripts how fast each part spins (`AssemblyAngularVelocity`).
    pub fn set_angular_velocities(
        &self,
        spins: std::collections::HashMap<InstanceId, playlibre_instance::Vec3>,
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
    pub fn take_move_requests(&self) -> Vec<(InstanceId, Option<playlibre_instance::Vec3>)> {
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

    /// Every script thread that is waiting (on `task.wait`, `WaitForChild`,
    /// an event and so on): where it is waiting, for how long, and how many
    /// threads wait at that same place. Longest waits first.
    /// Each script's total running time in seconds and how many times it
    /// ran, slowest first, for a Script Performance view.
    pub fn script_times(&self) -> Vec<(String, f64, u64)> {
        let host = bindings::host(&self.lua);
        let mut out: Vec<(String, f64, u64)> =
            host.script_time.borrow().iter().map(|(k, v)| (k.clone(), v.0, v.1)).collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1));
        out
    }

    /// What each waiting script thread is waiting in, longest first.
    pub fn waiting_report(&self) -> Vec<WaitingThread> {
        bindings::waiting_report(&self.lua)
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
        if role == NetRole::Client {
            self.set_memory_limit(JOINED_MEMORY_LIMIT.min(device_memory_limit()));
        }
    }

    /// Changes how much memory the game's scripts may use together.
    pub fn set_memory_limit(&self, bytes: usize) {
        bindings::host(&self.lua).memory_limit.set(bytes);
        let _ = self.lua.set_memory_limit(bytes);
    }

    /// How much memory the game's scripts may use together.
    pub fn memory_limit(&self) -> usize {
        bindings::host(&self.lua).memory_limit.get()
    }

    /// How much memory the game's scripts use together right now, in bytes
    /// (for soak tests that check a game doesn't leak).
    pub fn used_memory(&self) -> usize {
        self.lua.used_memory()
    }

    /// Collects all of the scripts' garbage now, rather than bit by bit
    /// over the next frames. Objects for destroyed instances that no
    /// script holds go too, so those instances can be freed.
    pub fn collect_garbage(&self) {
        // A second pass takes what the first one's finalizers let go.
        let _ = self.lua.gc_collect();
        let _ = self.lua.gc_collect();
    }

    /// Runs every waiting thread each step, however long it takes, so a
    /// game plays out the same on a slow computer (for tests).
    pub fn set_steady(&self, on: bool) {
        bindings::host(&self.lua).steady.set(on);
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
        args: &playlibre_instance::serde_json::Value,
    ) -> Vec<ScriptError> {
        net::server_event(&self.lua, remote, player, args)
    }

    /// On a player's computer: the server fired `remote` at it.
    pub fn deliver_client_event(
        &self,
        remote: InstanceId,
        args: &playlibre_instance::serde_json::Value,
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
        args: &playlibre_instance::serde_json::Value,
    ) -> Vec<ScriptError> {
        net::serve_invoke(&self.lua, call, remote, player, args)
    }

    /// On a player's computer: the server answered call `call`.
    pub fn deliver_invoke_result(&self, call: u64, ok: bool, values: playlibre_instance::serde_json::Value) {
        net::invoke_result(&self.lua, call, ok, values)
    }

    /// Fires `tool`'s `Activated` (or `Deactivated`), as when a player on
    /// another computer clicks while holding it.
    pub fn activate_tool(&self, tool: InstanceId, down: bool) -> Vec<ScriptError> {
        items::activate(&self.lua, tool, down);
        bindings::take_errors(&self.lua)
    }
}
