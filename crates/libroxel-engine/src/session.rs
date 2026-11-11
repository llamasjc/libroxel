//! A play session: what runs between pressing Play and Stop.

use libroxel_instance::{ClassKind, EnumValue, InstanceId, InstanceTree, KeyCode, Property, PropertyValue};
use libroxel_script::{OutputLevel, ScriptRuntime, SharedTree, UserInput};

use crate::character::{Character, CharacterInput};
use crate::physics::Physics;

/// How long a character stays dead before respawning, in seconds.
pub const RESPAWN_DELAY: f32 = 2.0;
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
}

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
            session.copy_starter_gui(&tree, false);
            let errors = session.runtime.player_added(player);
            session.collect(errors);
            session.spawn_character(&tree);
        }
        Ok(session)
    }

    fn spawn_character(&mut self, tree: &SharedTree) {
        let Some(player) = self.player else { return };
        let character = {
            let mut t = tree.borrow_mut();
            let name = t.name(player).unwrap_or(LOCAL_PLAYER_NAME).to_owned();
            let character = Character::spawn_named(&mut t, &name);
            let _ = t.set(player, Property::Character, PropertyValue::Ref(Some(character.model)));
            character
        };
        self.character = Some(character);
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
        let result = {
            let mut tree = tree.borrow_mut();
            if let Some(c) = self.character.filter(|c| !c.is_dead(&tree)) {
                c.drive(&tree, &mut self.physics, input);
            }
            let result = self.physics.step(&mut tree, dt);
            if let Some(c) = self.character.filter(|c| !c.is_dead(&tree)) {
                c.follow(&mut tree);
            }
            result
        };

        for (a, b) in result.touches {
            let mut errors = self.runtime.fire_touched(a, b);
            errors.extend(self.runtime.fire_touched(b, a));
            self.collect(errors);
        }
        for id in result.fell {
            let _ = tree.borrow_mut().destroy(id);
        }

        self.handle_death(&tree, dt);
    }

    fn handle_death(&mut self, tree: &SharedTree, dt: f32) {
        let Some(character) = self.character else { return };
        if let Some(remaining) = self.respawn_in.as_mut() {
            *remaining -= dt;
            if *remaining <= 0.0 {
                self.respawn_in = None;
                let _ = tree.borrow_mut().destroy(character.model);
                self.copy_starter_gui(tree, true);
                self.spawn_character(tree);
                self.info("Respawned");
            }
        } else if character.is_dead(&tree.borrow()) {
            self.respawn_in = Some(RESPAWN_DELAY);
            self.info("Your character died");
            let errors = self.runtime.humanoid_died(character.humanoid);
            self.collect(errors);
        }
    }
}

/// Convenience for tests and tools: a session over a fresh copy of `tree`.
pub fn start_on(tree: InstanceTree, with_character: bool) -> Result<PlaySession, String> {
    PlaySession::start(std::rc::Rc::new(std::cell::RefCell::new(tree)), with_character)
}
