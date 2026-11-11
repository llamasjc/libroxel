//! A play session: what runs between pressing Play and Stop.

use libroxel_instance::{
    ClassKind, EnumValue, InstanceId, InstanceTree, KeyCode, Property, PropertyValue, Rotation, Vec3,
};
use libroxel_script::{MouseRay, OutputLevel, ScriptRuntime, SharedTree, UserInput, VelocityRequest, names};

use crate::character::{Character, CharacterInput};
use crate::physics::Physics;

/// How long a character stays dead before respawning, in seconds, unless
/// the players service's `RespawnTime` says otherwise.
pub const RESPAWN_DELAY: f32 = 5.0;
/// The name of the local player.
pub const LOCAL_PLAYER_NAME: &str = "Player1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// A line for the output panel.
#[derive(Debug, Clone, PartialEq)]
pub struct LogEntry {
    pub level: LogLevel,
    /// Seconds of game time when it happened.
    pub time: f64,
    pub text: String,
}

/// The proximity prompt the player can use right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActivePrompt {
    pub prompt: InstanceId,
    /// The part it is in, which the prompt is drawn beside.
    pub part: InstanceId,
    pub key: KeyCode,
    /// How far through holding the key the player is, 0 to 1.
    pub progress: f32,
}

/// A prompt key being held down.
#[derive(Debug, Clone, Copy)]
struct PromptHold {
    prompt: InstanceId,
    key: KeyCode,
    held: f32,
    triggered: bool,
}

/// Scripts, physics and a character, all running against one tree.
pub struct PlaySession {
    runtime: ScriptRuntime,
    physics: Physics,
    character: Option<Character>,
    /// The local player's object under the players service.
    player: Option<InstanceId>,
    /// The local player's GUI container.
    player_gui: Option<InstanceId>,
    respawn_in: Option<f32>,
    log: Vec<LogEntry>,
    hold: Option<PromptHold>,
    /// The local player's tool container.
    backpack: Option<InstanceId>,
    /// The player's tools in hotbar order (slot 1 first).
    hotbar: Vec<InstanceId>,
    /// Explosions that have already gone off.
    exploded: std::collections::HashSet<InstanceId>,
    /// Explosions still being drawn: where, how big, and how long ago.
    blasts: Vec<Blast>,
    /// The workspace camera the view follows or obeys.
    camera: Option<InstanceId>,
    /// Walks that scripts started with `MoveTo`, and humanoids that died.
    npcs: crate::npc::Npcs,
    /// Parts that welds hold together.
    welds: crate::weld::Welds,
    /// Whether the current character's death has been seen.
    death_seen: bool,
    /// The seat the player's character sits in.
    seating: crate::seat::Seating,
    /// How far through a stride the character's walk is, in radians.
    stride: f32,
}

/// An explosion's flash, for drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blast {
    pub position: Vec3,
    pub radius: f32,
    /// Seconds since it went off.
    pub age: f32,
}

/// How long an explosion's flash lasts, in seconds.
pub const BLAST_TIME: f32 = 0.6;

use crate::character::HAND_OFFSET;
/// Radians of stride per stud walked.
const STRIDE_PER_STUD: f32 = 0.55;

/// The number keys that pick hotbar slots 1 to 9.
const SLOT_KEYS: [KeyCode; 9] = [
    KeyCode::One,
    KeyCode::Two,
    KeyCode::Three,
    KeyCode::Four,
    KeyCode::Five,
    KeyCode::Six,
    KeyCode::Seven,
    KeyCode::Eight,
    KeyCode::Nine,
];

impl PlaySession {
    /// Spawns a character (when `with_character`) and starts every script.
    ///
    /// Scripts start first, then the player joins (firing the join event),
    /// then their character spawns, so scripts see the same order as in a
    /// real game.
    pub fn start(tree: SharedTree, with_character: bool) -> Result<Self, String> {
        Self::start_inner(tree, with_character, None)
    }

    /// Like [`start`](Self::start) with a character, with data stores
    /// holding `saved_data` (JSON from an earlier session's
    /// [`save_data`](Self::save_data)) before any script runs.
    pub fn start_with_data(tree: SharedTree, saved_data: &str) -> Result<Self, String> {
        Self::start_inner(tree, true, Some(saved_data))
    }

    fn start_inner(tree: SharedTree, with_character: bool, saved_data: Option<&str>) -> Result<Self, String> {
        let runtime = ScriptRuntime::new(tree.clone()).map_err(|e| format!("could not start Luau: {e}"))?;
        let mut session = Self {
            runtime,
            physics: Physics::default(),
            character: None,
            player: None,
            player_gui: None,
            respawn_in: None,
            log: Vec::new(),
            hold: None,
            backpack: None,
            hotbar: Vec::new(),
            exploded: Default::default(),
            blasts: Vec::new(),
            camera: None,
            npcs: Default::default(),
            welds: Default::default(),
            death_seen: false,
            seating: Default::default(),
            stride: 0.0,
        };
        // Scripts find the camera through the workspace, so it exists from the start.
        session.camera = {
            let mut t = tree.borrow_mut();
            let ws = t.workspace();
            match t.find_first_child_of_class(ws, ClassKind::Camera).ok().flatten() {
                Some(cam) => Some(cam),
                None => {
                    let cam = t
                        .create(ClassKind::Camera, names::extras::CAMERA_NAME)
                        .expect("cameras can be created");
                    t.set_parent(cam, Some(ws)).expect("fresh cameras can be parented");
                    Some(cam)
                }
            }
        };
        if let Some(json) = saved_data
            && let Err(err) = session.runtime.load_data(json)
        {
            session.log.push(LogEntry {
                level: LogLevel::Warn,
                time: 0.0,
                text: format!("Saved data could not be read, starting fresh: {err}"),
            });
        }
        let errors = session.runtime.run_tree_scripts();
        session.collect(errors);
        if with_character {
            let player = {
                let mut t = tree.borrow_mut();
                let players = t.service(ClassKind::Players).expect("every tree has a players service");
                let player = t.create(ClassKind::Player, LOCAL_PLAYER_NAME).expect("players can be created");
                t.set_parent(player, Some(players)).expect("fresh players can be parented");
                t.set(player, Property::UserId, PropertyValue::Number(1.0)).expect("players have ids");
                player
            };
            session.player = Some(player);
            let gui = {
                let mut t = tree.borrow_mut();
                let gui = t.create(ClassKind::PlayerGui, "PlayerGui").expect("player GUIs can be created");
                t.set_parent(gui, Some(player)).expect("fresh GUIs can be parented");
                gui
            };
            session.player_gui = Some(gui);
            let backpack = {
                let mut t = tree.borrow_mut();
                let backpack = t.create(ClassKind::Backpack, "Backpack").expect("backpacks can be created");
                t.set_parent(backpack, Some(player)).expect("fresh backpacks can be parented");
                backpack
            };
            session.backpack = Some(backpack);
            session.copy_starter_gui(&tree, false);
            auto_assign_team(&mut tree.borrow_mut(), player);
            let errors = session.runtime.player_added(player);
            session.collect(errors);
            if session.auto_loads(&tree) {
                session.spawn_character(&tree);
            }
        }
        Ok(session)
    }

    fn spawn_character(&mut self, tree: &SharedTree) {
        let Some(player) = self.player else { return };
        let character = {
            let mut t = tree.borrow_mut();
            let name = t.name(player).unwrap_or(LOCAL_PLAYER_NAME).to_owned();
            let team = t
                .get_ref(player, Property::Team)
                .filter(|_| t.get_bool(player, Property::Neutral) == Some(false))
                .and_then(|team| t.get_color(team, Property::TeamColor));
            let n = t.get_number(player, Property::UserId).unwrap_or(1.0).max(1.0) as usize - 1;
            let colors = crate::character::BodyColors::for_player(n);
            let character = Character::spawn_dressed(&mut t, &name, team, colors);
            let _ = t.set(player, Property::Character, PropertyValue::Ref(Some(character.model)));
            character
        };
        self.fill_backpack(tree);
        self.character = Some(character);
        self.death_seen = false;
        self.respawn_in = None;
        let errors = self.runtime.character_added(player, character.model);
        self.collect(errors);
    }

    /// The local player's object, while a character is being played.
    pub fn player(&self) -> Option<InstanceId> {
        self.player
    }

    /// The local player's GUI container, whose screen GUIs are drawn over the view.
    pub fn player_gui(&self) -> Option<InstanceId> {
        self.player_gui
    }

    /// Copies the starter GUIs into the player's GUI and starts their local
    /// scripts. With `respawn`, only GUIs that reset on spawn are replaced.
    fn copy_starter_gui(&mut self, tree: &SharedTree, respawn: bool) {
        let Some(gui) = self.player_gui else { return };
        let copies: Vec<InstanceId> = {
            let mut t = tree.borrow_mut();
            let resets =
                |t: &InstanceTree, id: InstanceId| t.get_bool(id, Property::ResetOnSpawn) != Some(false);
            if respawn {
                let old: Vec<InstanceId> =
                    t.children(gui).unwrap_or_default().iter().copied().filter(|c| resets(&t, *c)).collect();
                for id in old {
                    let _ = t.destroy(id);
                }
            }
            let Some(starter) = t.service(ClassKind::StarterGui) else { return };
            let templates: Vec<InstanceId> = t
                .children(starter)
                .unwrap_or_default()
                .iter()
                .copied()
                .filter(|c| !respawn || resets(&t, *c))
                .collect();
            templates
                .into_iter()
                .filter_map(|template| {
                    let copy = t.clone_subtree(template).ok()?;
                    t.set_parent(copy, Some(gui)).ok()?;
                    Some(copy)
                })
                .collect()
        };
        for copy in copies {
            let errors = self.runtime.run_local_scripts(copy);
            self.collect(errors);
        }
    }

    /// Gives the player a fresh copy of every starter tool, replacing what
    /// their backpack held, and starts the tools' scripts.
    fn fill_backpack(&mut self, tree: &SharedTree) {
        let Some(backpack) = self.backpack else { return };
        let copies: Vec<InstanceId> = {
            let mut t = tree.borrow_mut();
            for old in t.children(backpack).unwrap_or_default().to_vec() {
                let _ = t.destroy(old);
            }
            let Some(pack) = t.service(ClassKind::StarterPack) else { return };
            let tools: Vec<InstanceId> = t
                .children(pack)
                .unwrap_or_default()
                .iter()
                .copied()
                .filter(|c| t.class(*c) == Ok(ClassKind::Tool))
                .collect();
            tools
                .into_iter()
                .filter_map(|tool| {
                    let copy = t.clone_subtree(tool).ok()?;
                    t.set_parent(copy, Some(backpack)).ok()?;
                    Some(copy)
                })
                .collect()
        };
        self.hotbar.clear();
        for copy in copies {
            let mut errors = self.runtime.run_scripts(copy);
            errors.extend(self.runtime.run_local_scripts(copy));
            self.collect(errors);
        }
        self.refresh_hotbar();
    }

    /// Keeps the hotbar in step with the tools the player has: ones that
    /// are gone drop out, new ones go at the end.
    fn refresh_hotbar(&mut self) {
        let tree = self.runtime.tree().borrow();
        let mut owned: Vec<InstanceId> = Vec::new();
        for container in [self.backpack, self.character.map(|c| c.model)].into_iter().flatten() {
            if let Ok(children) = tree.children(container) {
                owned.extend(children.iter().copied().filter(|c| tree.class(*c) == Ok(ClassKind::Tool)));
            }
        }
        self.hotbar.retain(|t| owned.contains(t));
        for tool in owned {
            if !self.hotbar.contains(&tool) {
                self.hotbar.push(tool);
            }
        }
    }

    /// The player's tools in hotbar order, each with whether it's held.
    pub fn hotbar(&self) -> Vec<(InstanceId, bool)> {
        let tree = self.runtime.tree().borrow();
        let model = self.character.map(|c| c.model);
        self.hotbar.iter().map(|t| (*t, model.is_some() && tree.parent(*t).ok().flatten() == model)).collect()
    }

    /// Holds the tool in hotbar `slot` (from 0), or puts it away if it's
    /// already held. Holding one tool puts any other away.
    pub fn toggle_tool(&mut self, slot: usize) {
        let (Some(tool), Some(character), Some(backpack)) =
            (self.hotbar.get(slot).copied(), self.character, self.backpack)
        else {
            return;
        };
        let mut tree = self.runtime.tree().borrow_mut();
        if character.is_dead(&tree) {
            return;
        }
        let held = tree.parent(tool).ok().flatten() == Some(character.model);
        let holding: Vec<InstanceId> = tree
            .children(character.model)
            .unwrap_or_default()
            .iter()
            .copied()
            .filter(|c| tree.class(*c) == Ok(ClassKind::Tool))
            .collect();
        for other in holding {
            let _ = tree.set_parent(other, Some(backpack));
        }
        if !held {
            let _ = tree.set_parent(tool, Some(character.model));
        }
    }

    /// Tells scripts where the mouse points (`None` when it's off the view).
    pub fn set_mouse(&mut self, mouse: Option<MouseRay>) {
        self.runtime.set_mouse(mouse);
    }

    /// Turns the character to face where it walks and puts its tool's
    /// handle in its hand.
    fn pose_character(&mut self, input: CharacterInput, dt: f32) {
        let Some(character) = self.character else { return };
        let mut tree = self.runtime.tree().borrow_mut();
        if character.is_dead(&tree) {
            return;
        }
        let dir = input.move_dir;
        if dir.x != 0.0 || dir.z != 0.0 {
            let yaw = (-dir.x).atan2(-dir.z).to_degrees();
            let facing = PropertyValue::Vec3(Vec3::new(0.0, yaw, 0.0));
            let _ = tree.set(character.root, Property::Orientation, facing);
        }
        // Arms and legs swing with the walking speed.
        let v = self.physics.velocity(character.root);
        let speed = (v.x * v.x + v.z * v.z).sqrt();
        let sitting = self.seating.seat().is_some();
        if speed > 0.5 && !sitting {
            self.stride = (self.stride + speed * dt * STRIDE_PER_STUD) % std::f32::consts::TAU;
        } else {
            self.stride = 0.0;
        }
        let tool = tree.find_first_child_of_class(character.model, ClassKind::Tool).ok().flatten();
        let swing = crate::character::swing_for(speed, self.stride);
        character.pose(&mut tree, crate::character::Pose { swing, sitting, holding: tool.is_some() });
        let Some(tool) = tool else {
            return;
        };
        let Some(handle) = tree.find_first_child(tool, names::TOOL_HANDLE).ok().flatten() else { return };
        let (Some(pos), orientation) = (
            tree.get_vec3(character.root, Property::Position),
            tree.get_vec3(character.root, Property::Orientation),
        ) else {
            return;
        };
        let orientation = orientation.unwrap_or_default();
        let at = pos + Rotation::from_orientation(orientation).apply(HAND_OFFSET);
        let _ = tree.set(handle, Property::Anchored, PropertyValue::Bool(true));
        let _ = tree.set(handle, Property::CanCollide, PropertyValue::Bool(false));
        let _ = tree.set(handle, Property::Position, PropertyValue::Vec3(at));
        let _ = tree.set(handle, Property::Orientation, PropertyValue::Vec3(orientation));
    }

    /// The player clicked a GUI button.
    pub fn click_button(&mut self, button: InstanceId) {
        let errors = self.runtime.click_button(button);
        self.collect(errors);
    }

    /// The player clicked a part. Fires its click detector if it has one and
    /// the character is close enough. Returns whether the part was clickable.
    pub fn click_part(&mut self, part: InstanceId) -> bool {
        let Some(player) = self.player else { return false };
        let (detector, in_reach) = {
            let tree = self.runtime.tree().borrow();
            let Some(detector) =
                tree.find_first_child_of_class(part, ClassKind::ClickDetector).ok().flatten()
            else {
                return false;
            };
            let reach = tree.get_number(detector, Property::MaxActivationDistance).unwrap_or(32.0) as f32;
            let from = self.character.and_then(|c| tree.get_vec3(c.root, Property::Position));
            let to = tree.get_vec3(part, Property::Position);
            let in_reach = match (from, to) {
                (Some(a), Some(b)) => (a - b).length() <= reach,
                _ => false,
            };
            (detector, in_reach)
        };
        if in_reach {
            let errors = self.runtime.click_detector(detector, player);
            self.collect(errors);
        }
        true
    }

    /// What data stores hold now, as JSON.
    pub fn save_data(&self) -> String {
        self.runtime.save_data()
    }

    /// Ends the session: the player leaves (firing the leave event, where
    /// games usually save), so call this before throwing the session away.
    pub fn stop(&mut self) {
        if let Some(player) = self.player.take() {
            let errors = self.runtime.player_removing(player);
            self.collect(errors);
        }
    }

    /// A key or mouse button went down. `processed` means the game's UI used
    /// it already (for example, a click on a button), so prompts ignore it.
    pub fn input_began(&mut self, input: UserInput, processed: bool) {
        let errors = self.runtime.input_began(input, processed);
        self.collect(errors);
        if let (UserInput::Key(key), false) = (input, processed)
            && let Some(slot) = SLOT_KEYS.iter().position(|k| *k == key)
        {
            self.toggle_tool(slot);
        }
        if let (UserInput::Key(key), false) = (input, processed)
            && let Some(active) = self.active_prompt()
            && active.key == key
        {
            self.hold = Some(PromptHold { prompt: active.prompt, key, held: 0.0, triggered: false });
            self.advance_hold(0.0);
        }
    }

    /// A key or mouse button came back up.
    pub fn input_ended(&mut self, input: UserInput, processed: bool) {
        let errors = self.runtime.input_ended(input, processed);
        self.collect(errors);
        if let (UserInput::Key(key), Some(hold)) = (input, self.hold)
            && hold.key == key
        {
            self.release_hold();
        }
    }

    /// Uses a prompt straight away, as when the player clicks it.
    pub fn trigger_prompt(&mut self, prompt: InstanceId) {
        let Some(player) = self.player else { return };
        if self.active_prompt().is_none_or(|a| a.prompt != prompt) {
            return;
        }
        let mut errors = self.runtime.prompt_triggered(prompt, player);
        errors.extend(self.runtime.prompt_trigger_ended(prompt, player));
        self.collect(errors);
    }

    /// The nearest enabled prompt in reach of the character, if any.
    pub fn active_prompt(&self) -> Option<ActivePrompt> {
        let character = self.character?;
        let tree = self.runtime.tree().borrow();
        if character.is_dead(&tree) {
            return None;
        }
        let from = tree.get_vec3(character.root, Property::Position)?;
        let mut best: Option<(f32, ActivePrompt)> = None;
        for prompt in tree.descendants_of_class(tree.workspace(), ClassKind::ProximityPrompt) {
            if tree.get_bool(prompt, Property::Enabled) == Some(false) {
                continue;
            }
            let Some(part) = tree.parent(prompt).ok().flatten() else { continue };
            let Some(at) = tree.get_vec3(part, Property::Position) else { continue };
            let distance = (at - from).length();
            let reach = tree.get_number(prompt, Property::MaxActivationDistance).unwrap_or(10.0) as f32;
            if distance > reach || best.is_some_and(|(d, _)| d <= distance) {
                continue;
            }
            let key = match tree.get_enum(prompt, Property::KeyboardKeyCode) {
                Some(EnumValue::KeyCode(k)) => k,
                _ => KeyCode::E,
            };
            let duration = tree.get_number(prompt, Property::HoldDuration).unwrap_or(0.0) as f32;
            let progress = match self.hold {
                Some(h) if h.prompt == prompt && duration > 0.0 => (h.held / duration).min(1.0),
                Some(h) if h.prompt == prompt => 1.0,
                _ => 0.0,
            };
            best = Some((distance, ActivePrompt { prompt, part, key, progress }));
        }
        best.map(|(_, a)| a)
    }

    /// Moves a held prompt on by `dt`, triggering it once the hold is long enough.
    fn advance_hold(&mut self, dt: f32) {
        let Some(mut hold) = self.hold else { return };
        if self.active_prompt().is_none_or(|a| a.prompt != hold.prompt) {
            // Walked away or the prompt was turned off.
            self.release_hold();
            return;
        }
        hold.held += dt;
        let duration =
            self.runtime.tree().borrow().get_number(hold.prompt, Property::HoldDuration).unwrap_or(0.0)
                as f32;
        if !hold.triggered
            && hold.held >= duration
            && let Some(player) = self.player
        {
            hold.triggered = true;
            self.hold = Some(hold);
            let errors = self.runtime.prompt_triggered(hold.prompt, player);
            self.collect(errors);
        } else {
            self.hold = Some(hold);
        }
    }

    fn release_hold(&mut self) {
        if let Some(hold) = self.hold.take()
            && hold.triggered
            && let Some(player) = self.player
        {
            let errors = self.runtime.prompt_trigger_ended(hold.prompt, player);
            self.collect(errors);
        }
    }

    pub fn runtime(&self) -> &ScriptRuntime {
        &self.runtime
    }

    pub fn character(&self) -> Option<Character> {
        self.character
    }

    /// The workspace camera: the view writes where it is each frame, or
    /// follows it when scripts have taken it over.
    pub fn camera(&self) -> Option<InstanceId> {
        let tree = self.runtime.tree().borrow();
        let ws = tree.workspace();
        self.camera
            .filter(|c| tree.parent(*c).ok().flatten() == Some(ws))
            .or_else(|| tree.find_first_child_of_class(ws, ClassKind::Camera).ok().flatten())
    }

    /// Explosions being drawn right now.
    pub fn blasts(&self) -> &[Blast] {
        &self.blasts
    }

    /// Tells scripts how big the view is, in pixels.
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.runtime.set_viewport(width, height);
    }

    /// The text box the player is typing into, if any.
    pub fn focused_text_box(&self) -> Option<InstanceId> {
        self.runtime.focused_text_box()
    }

    /// The player clicked into a text box (`Some`) or left the focused one
    /// (`None`, saying whether they pressed Enter).
    pub fn focus_text_box(&mut self, text_box: Option<InstanceId>, enter: bool) {
        let errors = self.runtime.focus_text_box(text_box, enter);
        self.collect(errors);
    }

    /// The part the camera should follow.
    pub fn focus(&self) -> Option<InstanceId> {
        self.character.map(|c| c.root)
    }

    fn info(&mut self, text: impl Into<String>) {
        self.log.push(LogEntry { level: LogLevel::Info, time: self.runtime.clock(), text: text.into() });
    }

    fn collect(&mut self, errors: Vec<libroxel_script::ScriptError>) {
        let time = self.runtime.clock();
        for line in self.runtime.take_output() {
            let level = match line.level {
                OutputLevel::Print => LogLevel::Info,
                OutputLevel::Warn => LogLevel::Warn,
            };
            self.log.push(LogEntry { level, time, text: line.text });
        }
        for err in errors {
            self.log.push(LogEntry { level: LogLevel::Error, time, text: err.to_string() });
        }
    }

    /// Takes the log lines produced since the last call.
    pub fn take_log(&mut self) -> Vec<LogEntry> {
        std::mem::take(&mut self.log)
    }

    /// Runs one frame: scripts, then character movement and physics, then
    /// touch events, then deaths and respawns.
    pub fn step(&mut self, dt: f32, input: CharacterInput) {
        // Everything that changed since last frame, for Changed events.
        let changes = self.runtime.tree().borrow_mut().drain_changes();
        let errors = self.runtime.fire_changes(&changes);
        self.collect(errors);
        let errors = self.runtime.step(dt as f64);
        self.collect(errors);
        self.advance_hold(dt);

        let tree = self.runtime.tree().clone();
        self.explode(&tree);
        self.blasts.retain_mut(|b| {
            b.age += dt;
            b.age < BLAST_TIME
        });
        for player in self.runtime.take_load_requests() {
            if Some(player) == self.player {
                self.respawn(&tree);
            }
        }
        let mut walks_ended = Vec::new();
        for (humanoid, target) in self.runtime.take_move_requests() {
            walks_ended.extend(self.npcs.request(humanoid, target).map(|h| (h, false)));
        }
        let mut input = input;
        let mut seat_changes = Vec::new();
        let (result, died) = {
            let mut tree = tree.borrow_mut();
            // The player's own character walks where MoveTo sends it until they steer.
            if let Some(c) = self.character.filter(|c| !c.is_dead(&tree)) {
                let steering = input.move_dir.x != 0.0 || input.move_dir.z != 0.0;
                if steering {
                    walks_ended.extend(self.npcs.request(c.humanoid, None).map(|h| (h, false)));
                } else if let Some(pos) = tree.get_vec3(c.root, Property::Position) {
                    input.move_dir = self.npcs.input_for(c.humanoid, pos).move_dir;
                }
                if tree.get_bool(c.humanoid, Property::Jump) == Some(true) {
                    input.jump = true;
                    let _ = tree.set(c.humanoid, Property::Jump, PropertyValue::Bool(false));
                }
            }
            for (seat, humanoid) in self.runtime.take_sit_requests() {
                if let Some(c) = self.character.filter(|c| c.humanoid == humanoid && !c.is_dead(&tree)) {
                    if self.seating.seat().is_some_and(|s| s != seat) {
                        seat_changes.extend(self.seating.stand(&mut tree, c));
                    }
                    seat_changes.extend(self.seating.sit(&mut tree, c, seat));
                }
            }
            let was_seated = self.seating.seat().is_some();
            let (seated_input, change) = self.seating.before_physics(&mut tree, self.character, input, dt);
            seat_changes.extend(change);
            if was_seated {
                // Sitting: the keys went to the seat. Jumping off leaps up.
                if let Some(c) = self.character.filter(|_| input.jump && seated_input.jump) {
                    let power = tree.get_number(c.humanoid, Property::JumpPower).unwrap_or(50.0) as f32;
                    self.physics.set_velocity(c.root, Vec3::new(0.0, power, 0.0));
                }
                input = CharacterInput::default();
            }
            for (id, request) in self.runtime.take_velocity_requests() {
                if tree.get_bool(id, Property::Anchored) != Some(false) {
                    continue;
                }
                let v = match request {
                    VelocityRequest::Set(v) => v,
                    VelocityRequest::Add(dv) => self.physics.velocity(id) + dv,
                };
                self.physics.set_velocity(id, v);
            }
            if let Some(c) = self.character.filter(|c| !c.is_dead(&tree)) {
                c.drive(&tree, &mut self.physics, input);
            }
            let player_model = self.character.map(|c| c.model);
            let poses = self.npcs.before_physics(&mut tree, &mut self.physics, player_model);
            crate::seat::drive_vehicles(&mut tree, &mut self.physics, dt);
            self.welds.before_physics(&tree, &mut self.physics);
            crate::movers::apply(&tree, &self.welds, &mut self.physics, dt);
            self.seating.hold(&tree, &mut self.physics, self.character);
            let result = self.physics.step(&mut tree, dt);
            if let Some(c) = self.character.filter(|c| !c.is_dead(&tree)) {
                c.follow(&mut tree);
            }
            let (ended, died) = self.npcs.after_physics(&mut tree, &poses, dt);
            self.welds.after_physics(&mut tree);
            self.seating.after_physics(&mut tree, self.character);
            // Touching a free seat sits the character down.
            if let Some(c) = self.character.filter(|c| !c.is_dead(&tree))
                && self.seating.can_sit()
                && let Some(seat) = crate::seat::touched_seat(&tree, c.root, &result.touches)
            {
                seat_changes.extend(self.seating.sit(&mut tree, c, seat));
                self.seating.after_physics(&mut tree, self.character);
            }
            walks_ended.extend(ended);
            if let Some(c) = self.character {
                walks_ended
                    .extend(self.npcs.finish_walks(&tree, dt, |h| (h == c.humanoid).then_some(c.root)));
            }
            (result, died)
        };
        for change in seat_changes {
            let errors = self.runtime.seated(change.humanoid, change.seat);
            self.collect(errors);
        }
        for (humanoid, reached) in walks_ended {
            let errors = self.runtime.move_finished(humanoid, reached);
            self.collect(errors);
        }
        for humanoid in died {
            let errors = self.runtime.humanoid_died(humanoid);
            self.collect(errors);
        }
        self.runtime.set_velocities(self.physics.velocities().clone());
        self.pose_character(input, dt);
        self.refresh_hotbar();

        for (a, b) in result.touches {
            let mut errors = self.runtime.fire_touched(a, b);
            errors.extend(self.runtime.fire_touched(b, a));
            self.collect(errors);
        }
        for (a, b) in result.untouches {
            let alive = {
                let t = tree.borrow();
                [a, b].iter().all(|p| t.exists(*p) && !t.is_destroyed(*p).unwrap_or(true))
            };
            if alive {
                let mut errors = self.runtime.fire_touch_ended(a, b);
                errors.extend(self.runtime.fire_touch_ended(b, a));
                self.collect(errors);
            }
        }
        for id in result.fell {
            let _ = tree.borrow_mut().destroy(id);
        }

        self.handle_death(&tree, dt);
    }

    /// Sets off every explosion that has arrived in the workspace: each
    /// part in reach is hit, characters in reach die, and loose parts are
    /// thrown away from the middle.
    fn explode(&mut self, tree: &SharedTree) {
        let fresh: Vec<InstanceId> = {
            let t = tree.borrow();
            t.descendants_of_class(t.workspace(), ClassKind::Explosion)
                .into_iter()
                .filter(|e| !self.exploded.contains(e))
                .collect()
        };
        for explosion in fresh {
            self.exploded.insert(explosion);
            let (at, radius, pressure, hits) = {
                let t = tree.borrow();
                let at = t.get_vec3(explosion, Property::Position).unwrap_or_default();
                let radius = t.get_number(explosion, Property::BlastRadius).unwrap_or(4.0) as f32;
                let pressure = t.get_number(explosion, Property::BlastPressure).unwrap_or(0.0) as f32;
                let mut hits = Vec::new();
                for part in t.descendants_of_class(t.workspace(), ClassKind::Part) {
                    let (Some(pos), Some(size)) =
                        (t.get_vec3(part, Property::Position), t.get_vec3(part, Property::Size))
                    else {
                        continue;
                    };
                    // The distance to the nearest point of the part's box.
                    let half = size / 2.0;
                    let d = pos - at;
                    let out = |d: f32, h: f32| (d.abs() - h).max(0.0);
                    let gap = Vec3::new(out(d.x, half.x), out(d.y, half.y), out(d.z, half.z)).length();
                    if gap <= radius {
                        hits.push((part, gap, pos));
                    }
                }
                (at, radius, pressure, hits)
            };
            self.blasts.push(Blast { position: at, radius, age: 0.0 });
            for &(part, distance, pos) in &hits {
                let errors = self.runtime.explosion_hit(explosion, part, distance);
                self.collect(errors);
                let mut t = tree.borrow_mut();
                if t.is_destroyed(part).unwrap_or(true) {
                    continue;
                }
                // Anything in a character's model takes the character with it.
                let model = t.parent(part).ok().flatten();
                if let Some(humanoid) =
                    model.and_then(|m| t.find_first_child_of_class(m, ClassKind::Humanoid).ok().flatten())
                {
                    let _ = t.set(humanoid, Property::Health, PropertyValue::Number(0.0));
                }
                if t.get_bool(part, Property::Anchored) == Some(false) && pressure > 0.0 {
                    let size = t.get_vec3(part, Property::Size).unwrap_or_default();
                    let mass = (size.x * size.y * size.z * names::extras::DENSITY).max(0.5);
                    let away = (pos - at).normalize_or_zero() + Vec3::new(0.0, 0.6, 0.0);
                    let speed = (pressure / 5_000.0).min(250.0) / mass.sqrt()
                        * (1.0 - distance / radius.max(0.01) * 0.5);
                    let v = self.physics.velocity(part) + away.normalize_or_zero() * speed;
                    self.physics.set_velocity(part, v);
                }
            }
        }
    }

    fn handle_death(&mut self, tree: &SharedTree, dt: f32) {
        let Some(character) = self.character else { return };
        if let Some(remaining) = self.respawn_in.as_mut() {
            *remaining -= dt;
            if *remaining <= 0.0 {
                self.respawn(tree);
                self.info("Respawned");
            }
        } else if !self.death_seen && character.is_dead(&tree.borrow()) {
            self.death_seen = true;
            self.info("Your character died");
            let errors = self.runtime.humanoid_died(character.humanoid);
            self.collect(errors);
            // Without auto-loading, a script brings the character back with LoadCharacter.
            if self.auto_loads(tree) {
                let delay = {
                    let t = tree.borrow();
                    t.service(ClassKind::Players)
                        .and_then(|p| t.get_number(p, Property::RespawnTime))
                        .map_or(RESPAWN_DELAY, |d| d as f32)
                };
                self.respawn_in = Some(delay);
            }
        }
    }

    /// Whether characters spawn by themselves (the players service's `CharacterAutoLoads`).
    fn auto_loads(&self, tree: &SharedTree) -> bool {
        let t = tree.borrow();
        t.service(ClassKind::Players).and_then(|p| t.get_bool(p, Property::CharacterAutoLoads)) != Some(false)
    }

    /// Removes the player's character, if any, and spawns a fresh one.
    fn respawn(&mut self, tree: &SharedTree) {
        self.respawn_in = None;
        if let Some(c) = self.character {
            let change = self.seating.stand(&mut tree.borrow_mut(), c);
            if let Some(change) = change {
                let errors = self.runtime.seated(change.humanoid, change.seat);
                self.collect(errors);
            }
        }
        if let (Some(player), Some(character)) = (self.player, self.character.take()) {
            let errors = self.runtime.character_removing(player, character.model);
            self.collect(errors);
            let _ = tree.borrow_mut().destroy(character.model);
            self.copy_starter_gui(tree, true);
        }
        self.spawn_character(tree);
    }
}

/// Puts a player who just joined on the auto-assignable team with the
/// fewest players, if the game has teams.
fn auto_assign_team(tree: &mut InstanceTree, player: InstanceId) {
    let Some(teams) = tree.service(ClassKind::Teams) else { return };
    let players: Vec<InstanceId> = tree
        .service(ClassKind::Players)
        .and_then(|p| tree.children(p).ok())
        .map(|c| c.to_vec())
        .unwrap_or_default();
    let candidates: Vec<InstanceId> = tree
        .children(teams)
        .unwrap_or_default()
        .iter()
        .copied()
        .filter(|t| {
            tree.class(*t) == Ok(ClassKind::Team) && tree.get_bool(*t, Property::AutoAssignable) == Some(true)
        })
        .collect();
    let count =
        |team: InstanceId| players.iter().filter(|p| tree.get_ref(**p, Property::Team) == Some(team)).count();
    // The first of the smallest teams, so the result doesn't depend on chance.
    let Some(team) = candidates.iter().copied().min_by_key(|t| count(*t)) else { return };
    let _ = tree.set(player, Property::Team, PropertyValue::Ref(Some(team)));
    let _ = tree.set(player, Property::Neutral, PropertyValue::Bool(false));
}

/// Convenience for tests and tools: a session over a fresh copy of `tree`.
pub fn start_on(tree: InstanceTree, with_character: bool) -> Result<PlaySession, String> {
    PlaySession::start(std::rc::Rc::new(std::cell::RefCell::new(tree)), with_character)
}
