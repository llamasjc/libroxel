//! A play session: what runs between pressing Play and Stop.

use libroxel_instance::{InstanceId, InstanceTree};
use libroxel_script::{OutputLevel, ScriptRuntime, SharedTree};

use crate::character::{Character, CharacterInput};
use crate::physics::Physics;

/// How long a character stays dead before respawning, in seconds.
pub const RESPAWN_DELAY: f32 = 2.0;

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

/// Scripts, physics and a character, all running against one tree.
pub struct PlaySession {
    runtime: ScriptRuntime,
    physics: Physics,
    character: Option<Character>,
    respawn_in: Option<f32>,
    log: Vec<LogEntry>,
}

impl PlaySession {
    /// Spawns a character (when `with_character`) and starts every script.
    pub fn start(tree: SharedTree, with_character: bool) -> Result<Self, String> {
        let character = with_character.then(|| Character::spawn(&mut tree.borrow_mut()));
        let runtime = ScriptRuntime::new(tree).map_err(|e| format!("could not start Luau: {e}"))?;
        let mut session =
            Self { runtime, physics: Physics::default(), character, respawn_in: None, log: Vec::new() };
        let errors = session.runtime.run_tree_scripts();
        session.collect(errors);
        Ok(session)
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
        let errors = self.runtime.step(dt as f64);
        self.collect(errors);

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
                let mut t = tree.borrow_mut();
                let _ = t.destroy(character.model);
                self.character = Some(Character::spawn(&mut t));
                drop(t);
                self.info("Respawned");
            }
        } else if character.is_dead(&tree.borrow()) {
            self.respawn_in = Some(RESPAWN_DELAY);
            self.info("Your character died");
        }
    }
}

/// Convenience for tests and tools: a session over a fresh copy of `tree`.
pub fn start_on(tree: InstanceTree, with_character: bool) -> Result<PlaySession, String> {
    PlaySession::start(std::rc::Rc::new(std::cell::RefCell::new(tree)), with_character)
}
