//! A play session: what runs between pressing Play and Stop.

use libroxel_instance::{
    ClassKind, EnumValue, InstanceId, InstanceTree, KeyCode, Property, PropertyValue, Rotation, Vec3,
};
use libroxel_script::{
    MouseRay, NetRole, OutputLevel, ScriptRuntime, SharedTree, UserInput, VelocityRequest, names,
};

use crate::character::{Character, CharacterInput};
use crate::physics::Physics;

/// How long a character stays dead before respawning, in seconds, unless
/// the players service's `RespawnTime` says otherwise.
pub const RESPAWN_DELAY: f32 = 5.0;

/// Seconds after joining before a player's first character appears by
/// itself. Like Roblox, where it waits for the player's game to load: the
/// place's scripts get to run first, and one that turns
/// `CharacterAutoLoads` off in that time spawns the character itself.
pub const FIRST_SPAWN_DELAY: f32 = 0.5;
/// The name of the local player, unless [`set_local_player_name`] gave
/// another (the app uses the player's account name).
pub const LOCAL_PLAYER_NAME: &str = "Player1";

thread_local! {
    static LOCAL_NAME: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Names this computer's player in the sessions this thread starts from now
/// on (play tests and hosted games), such as the account's name. `None`
/// (or a blank name) goes back to [`LOCAL_PLAYER_NAME`].
pub fn set_local_player_name(name: Option<&str>) {
    let name = name.map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned);
    LOCAL_NAME.with(|local| *local.borrow_mut() = name);
}

/// Who this computer's player is in a session started now.
pub fn local_player_name() -> String {
    LOCAL_NAME.with(|local| local.borrow().clone()).unwrap_or_else(|| LOCAL_PLAYER_NAME.to_owned())
}

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
pub(crate) struct PromptHold {
    prompt: InstanceId,
    key: KeyCode,
    held: f32,
    triggered: bool,
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

/// Seconds after a jump sound before another jump makes one.
const JUMP_QUIET: f32 = 0.35;

/// How fast the `n`th jump sound plays: a little higher or lower each time.
fn jump_pitch(n: u32) -> f64 {
    let h = n.wrapping_mul(0x9e37_79b9).rotate_left(13).wrapping_mul(0x85eb_ca6b);
    0.92 + 0.16 * (h >> 8) as f64 / (1u32 << 24) as f64
}

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
/// One player in the game, and what the session keeps track of for them.
#[derive(Debug)]
pub(crate) struct Avatar {
    pub(crate) player: InstanceId,
    /// Their GUI container. A player on another computer has an empty one
    /// here: their GUIs run on their computer.
    pub(crate) gui: Option<InstanceId>,
    /// Their tool container.
    pub(crate) backpack: Option<InstanceId>,
    /// Their tools in hotbar order (slot 1 first).
    pub(crate) hotbar: Vec<InstanceId>,
    pub(crate) character: Option<Character>,
    pub(crate) respawn_in: Option<f32>,
    /// Seconds before their first character appears by itself.
    pub(crate) first_spawn_in: Option<f32>,
    /// Whether the current character's death has been seen.
    pub(crate) death_seen: bool,
    pub(crate) hold: Option<PromptHold>,
    /// The seat their character sits in.
    pub(crate) seating: crate::seat::Seating,
    /// How far through a stride the character's walk is, in radians.
    pub(crate) stride: f32,
    /// Seconds left of swinging their tool.
    pub(crate) slash: f32,
    /// Whether their character stood on something, and was in water, last step.
    pub(crate) was_grounded: bool,
    pub(crate) was_wet: bool,
    /// Seconds before a jump makes a sound again.
    pub(crate) jump_quiet: f32,
    /// How many jumps have made a sound.
    pub(crate) jumps: u32,
    /// Whether they play on another computer, so their input comes over
    /// the network and their GUIs and local scripts run there.
    pub(crate) remote: bool,
    /// Their latest input, for players on other computers.
    pub(crate) input: CharacterInput,
    /// For a player on another computer, which moves its own character:
    /// where it last said the character is.
    pub(crate) moved_to: Option<Vec3>,
    /// The force field a spawn location gave their character, and the
    /// seconds before it goes.
    pub(crate) shield: Option<(InstanceId, f32)>,
    /// How many times the host has put that character somewhere itself
    /// (a teleport); moves sent before the latest one are ignored.
    pub(crate) teleports: u64,
    /// How they chose to look; without one they get the usual look.
    pub(crate) look: Option<crate::character::Look>,
    /// For a player on another computer: how much further it may say its
    /// character went.
    pub(crate) travel: Option<crate::net::Allowance>,
}

impl Avatar {
    pub(crate) fn new(player: InstanceId, remote: bool) -> Self {
        Self {
            player,
            gui: None,
            backpack: None,
            hotbar: Vec::new(),
            character: None,
            respawn_in: None,
            first_spawn_in: None,
            death_seen: false,
            hold: None,
            seating: Default::default(),
            stride: 0.0,
            slash: 0.0,
            was_grounded: false,
            was_wet: false,
            jump_quiet: 0.0,
            jumps: 0,
            remote,
            input: CharacterInput::default(),
            moved_to: None,
            shield: None,
            teleports: 0,
            look: None,
            travel: None,
        }
    }

    /// Whether their computer moves their character (it's alive and not
    /// sitting): the host then only puts it where it is told.
    pub(crate) fn moves_itself(&self, tree: &InstanceTree) -> bool {
        self.remote && self.seating.seat().is_none() && self.character.is_some_and(|c| !c.is_dead(tree))
    }
}

/// Scripts, physics and the players' characters, all running against one tree.
/// Where the last frames' time went, in seconds summed since the last
/// [`PlaySession::take_frame_times`].
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameTimes {
    pub frames: u32,
    /// Firing Changed, ChildAdded and the other change events.
    pub change_events: f64,
    /// Resuming waiting scripts and the frame signals.
    pub scripts: f64,
    /// Moving parts and characters.
    pub physics: f64,
    /// Everything else in a frame.
    pub other: f64,
    /// A joined game only: taking in what the host sent (reading the
    /// socket, decrypting, unpacking and parsing).
    pub receive: f64,
    /// A joined game only: putting the host's instances into the tree.
    pub apply: f64,
    /// A joined game only: looking through the workspace for explosions.
    pub explosion_scan: f64,
    /// A joined game only: bytes read from the host.
    pub bytes_in: u64,
    /// A joined game only: the host's operations applied.
    pub ops_applied: u64,
    /// A joined game only: the longest one message took to apply.
    pub slowest_apply: f64,
    /// A joined game only: the most messages from the host still waiting
    /// at the end of a frame (growing means falling behind the host).
    pub most_waiting: usize,
}

impl FrameTimes {
    /// The seconds the game step took, summed.
    pub fn step_seconds(&self) -> f64 {
        self.change_events
            + self.scripts
            + self.physics
            + self.other
            + self.receive
            + self.apply
            + self.explosion_scan
    }

    /// Adds `t`'s sums to these.
    pub fn add(&mut self, t: &FrameTimes) {
        self.frames += t.frames;
        self.change_events += t.change_events;
        self.scripts += t.scripts;
        self.physics += t.physics;
        self.other += t.other;
        self.receive += t.receive;
        self.apply += t.apply;
        self.explosion_scan += t.explosion_scan;
        self.bytes_in += t.bytes_in;
        self.ops_applied += t.ops_applied;
        self.slowest_apply = self.slowest_apply.max(t.slowest_apply);
        self.most_waiting = self.most_waiting.max(t.most_waiting);
    }
}

pub struct PlaySession {
    pub(crate) frame_times: FrameTimes,
    pub(crate) runtime: ScriptRuntime,
    pub(crate) physics: Physics,
    /// Everyone in the game. The one on this computer (if any) is the one
    /// that isn't `remote`.
    pub(crate) avatars: Vec<Avatar>,
    pub(crate) log: Vec<LogEntry>,
    /// Explosions that have already gone off.
    pub(crate) exploded: std::collections::HashSet<InstanceId>,
    /// Humanoids whose Died event has fired (it fires once).
    pub(crate) died: std::collections::HashSet<InstanceId>,
    /// Explosions still being drawn: where, how big, and how long ago.
    pub(crate) blasts: Vec<Blast>,
    /// Particle bursts scripts asked for, for the renderer to send out.
    pub(crate) emits: Vec<(InstanceId, u32)>,
    /// Sounds that `Play` or `Stop` sent back to their beginning.
    sound_restarts: Vec<InstanceId>,
    /// The GUI objects under the pointer, outermost first.
    hovered: Vec<InstanceId>,
    /// The workspace camera the view follows or obeys.
    pub(crate) camera: Option<InstanceId>,
    /// Walks that scripts started with `MoveTo`, and humanoids that died.
    npcs: crate::npc::Npcs,
    /// Parts that welds hold together.
    welds: crate::weld::Welds,
    /// Parts that hinges let turn.
    hinges: crate::hinge::Hinges,
    /// The network connection, when the game is hosted or joined.
    pub(crate) link: crate::net::Link,
    /// The next `UserId` to give a player who joins.
    pub(crate) next_user_id: u32,
    pub(crate) voice: crate::voice::Voice,
}

impl PlaySession {
    /// Spawns a character (when `with_character`) and starts every script.
    ///
    /// Scripts start first, then the player joins (firing the join event),
    /// then their character spawns, so scripts see the same order as in a
    /// real game.
    pub fn start(tree: SharedTree, with_character: bool) -> Result<Self, String> {
        // A new game draws its 3D view until a script says not to.
        libroxel_script::RENDERING_3D.store(true, std::sync::atomic::Ordering::Relaxed);
        Self::start_inner(tree, with_character, None, NetRole::Solo)
    }

    /// Like [`start`](Self::start) with a character, with data stores
    /// holding `saved_data` (JSON from an earlier session's
    /// [`save_data`](Self::save_data)) before any script runs.
    pub fn start_with_data(tree: SharedTree, saved_data: &str) -> Result<Self, String> {
        Self::start_inner(tree, true, Some(saved_data), NetRole::Solo)
    }

    /// A session with no scripts started and no one in it, for `role`.
    pub(crate) fn empty(tree: SharedTree, role: NetRole) -> Result<Self, String> {
        let runtime = ScriptRuntime::new(tree.clone()).map_err(|e| format!("could not start Luau: {e}"))?;
        runtime.set_net_role(role);
        let mut session = Self {
            frame_times: FrameTimes::default(),
            runtime,
            physics: Physics::default(),
            avatars: Vec::new(),
            log: Vec::new(),
            exploded: Default::default(),
            died: Default::default(),
            blasts: Vec::new(),
            emits: Vec::new(),
            sound_restarts: Vec::new(),
            hovered: Vec::new(),
            camera: None,
            npcs: Default::default(),
            welds: Default::default(),
            hinges: Default::default(),
            link: Default::default(),
            next_user_id: 1,
            voice: Default::default(),
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
        Ok(session)
    }

    pub(crate) fn start_inner(
        tree: SharedTree,
        with_character: bool,
        saved_data: Option<&str>,
        role: NetRole,
    ) -> Result<Self, String> {
        let mut session = Self::empty(tree, role)?;
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
            session.add_player(&local_player_name(), false);
        }
        Ok(session)
    }

    /// Adds a player called `name` (made unique): their player object, GUI
    /// container and backpack, a team, the join event, then their character.
    pub(crate) fn add_player(&mut self, name: &str, remote: bool) -> InstanceId {
        let tree = self.runtime.tree().clone();
        let user_id = self.next_user_id;
        self.next_user_id += 1;
        let player = {
            let mut t = tree.borrow_mut();
            let players = t.service(ClassKind::Players).expect("every tree has a players service");
            let taken = |t: &InstanceTree, n: &str| t.find_first_child(players, n).ok().flatten().is_some();
            let mut unique = name.to_owned();
            let mut n = 2;
            while taken(&t, &unique) {
                unique = format!("{name}{n}");
                n += 1;
            }
            let player = t.create(ClassKind::Player, unique).expect("players can be created");
            t.set_parent(player, Some(players)).expect("fresh players can be parented");
            t.set(player, Property::UserId, PropertyValue::Number(f64::from(user_id)))
                .expect("players have ids");
            player
        };
        let mut avatar = Avatar::new(player, remote);
        {
            let mut t = tree.borrow_mut();
            let gui = t.create(ClassKind::PlayerGui, "PlayerGui").expect("player GUIs can be created");
            t.set_parent(gui, Some(player)).expect("fresh GUIs can be parented");
            avatar.gui = Some(gui);
            let backpack = t.create(ClassKind::Backpack, "Backpack").expect("backpacks can be created");
            t.set_parent(backpack, Some(player)).expect("fresh backpacks can be parented");
            avatar.backpack = Some(backpack);
            let scripts =
                t.create(ClassKind::PlayerScripts, "PlayerScripts").expect("script folders can be created");
            t.set_parent(scripts, Some(player)).expect("fresh folders can be parented");
        }
        self.avatars.push(avatar);
        let index = self.avatars.len() - 1;
        if !remote && self.runtime.net_role() != NetRole::Solo {
            self.runtime.set_local_player(Some(player));
        }
        self.copy_starter_gui(index, false);
        self.copy_player_scripts(index);
        auto_assign_team(&mut tree.borrow_mut(), player);
        let errors = self.runtime.player_added(player);
        self.collect(errors);
        if let Some(index) = self.avatar_of(player) {
            self.avatars[index].first_spawn_in = Some(FIRST_SPAWN_DELAY);
        }
        player
    }

    /// Takes `player` out of the game: the leave event, then their
    /// character and player object go.
    pub(crate) fn leave(&mut self, player: InstanceId) {
        let Some(index) = self.avatar_of(player) else { return };
        let errors = self.runtime.player_removing(player);
        self.collect(errors);
        let Some(index) = self.avatar_of(player).or(Some(index)).filter(|i| *i < self.avatars.len()) else {
            return;
        };
        let avatar = self.avatars.remove(index);
        let mut tree = self.runtime.tree().borrow_mut();
        if let Some(c) = avatar.character {
            let _ = tree.destroy(c.model);
        }
        let _ = tree.destroy(player);
    }

    /// Kills the local player's character, as Roblox's Reset Character does.
    pub fn reset_character(&mut self) {
        if self.runtime.net_role() == NetRole::Client {
            self.send_to_server(crate::net::ClientSays::Reset);
            return;
        }
        if let Some(index) = self.local() {
            self.reset_character_for(index);
        }
    }

    pub(crate) fn reset_character_for(&mut self, index: usize) {
        let Some(c) = self.avatars[index].character else { return };
        let _ =
            self.runtime.tree().borrow_mut().set(c.humanoid, Property::Health, PropertyValue::Number(0.0));
    }

    /// Whether scripts left Reset Character on (`SetCore("ResetButtonCallback", false)` turns it off).
    pub fn can_reset(&self) -> bool {
        self.character().is_some() && self.runtime.reset_allowed()
    }

    /// Which avatar is `player`'s.
    pub(crate) fn avatar_of(&self, player: InstanceId) -> Option<usize> {
        self.avatars.iter().position(|a| a.player == player)
    }

    /// The avatar of the player on this computer.
    fn local(&self) -> Option<usize> {
        self.avatars.iter().position(|a| !a.remote)
    }

    fn spawn_character(&mut self, index: usize) {
        let tree = self.runtime.tree().clone();
        let player = self.avatars[index].player;
        let character = {
            let mut t = tree.borrow_mut();
            let name = t.name(player).unwrap_or(LOCAL_PLAYER_NAME).to_owned();
            let team = t
                .get_ref(player, Property::Team)
                .filter(|_| t.get_bool(player, Property::Neutral) == Some(false))
                .and_then(|team| t.get_color(team, Property::TeamColor));
            let n = t.get_number(player, Property::UserId).unwrap_or(1.0).max(1.0) as usize - 1;
            let look =
                self.avatars[index].look.clone().unwrap_or_else(|| crate::character::Look::for_player(n));
            let character = Character::spawn_wearing(&mut t, &name, team, look);
            let _ = t.set(player, Property::Character, PropertyValue::Ref(Some(character.model)));
            // A spawn location shields whoever appears on it for a while.
            let shield = crate::character::spawn_location_for(&t, team)
                .and_then(|s| t.get_number(s, Property::SpawnShield))
                .unwrap_or(0.0);
            if shield > 0.0
                && let Ok(field) = t.create(ClassKind::ForceField, "ForceField")
            {
                let _ = t.set_parent(field, Some(character.model));
                self.avatars[index].shield = Some((field, shield as f32));
            }
            character
        };
        self.fill_backpack(index);
        self.copy_character_scripts(index, character.model);
        let avatar = &mut self.avatars[index];
        avatar.character = Some(character);
        avatar.moved_to = None;
        avatar.death_seen = false;
        avatar.respawn_in = None;
        avatar.first_spawn_in = None;
        let errors = self.runtime.character_added(player, character.model);
        self.collect(errors);
    }

    /// Spawns now the characters of players who joined but whose first
    /// character hasn't appeared yet ([`FIRST_SPAWN_DELAY`]), as if their
    /// game had finished loading.
    pub fn load_characters_now(&mut self) {
        let tree = self.runtime.tree().clone();
        for index in 0..self.avatars.len() {
            if self.avatars[index].first_spawn_in.take().is_some()
                && self.avatars[index].character.is_none()
                && self.auto_loads(&tree)
            {
                self.spawn_character(index);
            }
        }
    }

    /// Makes `player` look like `look`, now and every time they respawn.
    pub fn set_look(&mut self, player: InstanceId, look: crate::character::Look) {
        let Some(index) = self.avatar_of(player) else { return };
        self.avatars[index].look = Some(look.clone());
        if let Some(character) = self.avatars[index].character {
            character.restyle(&mut self.runtime.tree().borrow_mut(), look);
        }
    }

    /// Makes the player on this computer look like `look`. In a joined
    /// game the host is told, and everyone sees it.
    pub fn set_local_look(&mut self, look: crate::character::Look) {
        if self.is_joined() {
            // The host dresses the character; the change comes back from there.
            self.send_look(look);
        } else if let Some(player) = self.player() {
            self.set_look(player, look);
        }
    }

    /// The local player's object, while a character is being played.
    pub fn player(&self) -> Option<InstanceId> {
        self.local().map(|i| self.avatars[i].player)
    }

    /// The local player's GUI container, whose screen GUIs are drawn over the view.
    pub fn player_gui(&self) -> Option<InstanceId> {
        self.local().and_then(|i| self.avatars[i].gui)
    }

    /// Everyone in the game, the local player (if any) included.
    pub fn players(&self) -> Vec<InstanceId> {
        self.avatars.iter().map(|a| a.player).collect()
    }

    /// Copies the starter GUIs into the player's GUI and starts their local
    /// scripts. With `respawn`, only GUIs that reset on spawn are replaced.
    /// Players on other computers get theirs there.
    pub(crate) fn copy_starter_gui(&mut self, index: usize, respawn: bool) {
        let tree = self.runtime.tree().clone();
        let avatar = &self.avatars[index];
        let Some(gui) = avatar.gui.filter(|_| !avatar.remote) else { return };
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

    /// Copies what's in `StarterPlayerScripts` into the player's
    /// `PlayerScripts`, once, and starts the local scripts there for the
    /// player on this computer. (Players on other computers start theirs
    /// when the copy reaches them.)
    fn copy_player_scripts(&mut self, index: usize) {
        let tree = self.runtime.tree().clone();
        let player = self.avatars[index].player;
        let copies = {
            let mut t = tree.borrow_mut();
            let Some(into) = t.find_first_child_of_class(player, ClassKind::PlayerScripts).ok().flatten()
            else {
                return;
            };
            let from = t.starter_scripts(ClassKind::StarterPlayerScripts);
            copy_children(&mut t, from, into)
        };
        if !self.avatars[index].remote {
            for copy in copies {
                let errors = self.runtime.run_local_scripts(copy);
                self.collect(errors);
            }
        }
    }

    /// Puts a copy of what's in `StarterCharacterScripts` in a character
    /// that just spawned, and starts its scripts (its local scripts only
    /// for the player on this computer).
    fn copy_character_scripts(&mut self, index: usize, model: InstanceId) {
        let tree = self.runtime.tree().clone();
        let copies = {
            let mut t = tree.borrow_mut();
            let from = t.starter_scripts(ClassKind::StarterCharacterScripts);
            copy_children(&mut t, from, model)
        };
        let remote = self.avatars[index].remote;
        for copy in copies {
            let mut errors = self.runtime.run_scripts(copy);
            if !remote {
                errors.extend(self.runtime.run_local_scripts(copy));
            }
            self.collect(errors);
        }
    }

    /// Gives the player a fresh copy of every starter tool, replacing what
    /// their backpack held, and starts the tools' scripts (their local
    /// scripts only for the player on this computer).
    fn fill_backpack(&mut self, index: usize) {
        let tree = self.runtime.tree().clone();
        let Some(backpack) = self.avatars[index].backpack else { return };
        let remote = self.avatars[index].remote;
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
        self.avatars[index].hotbar.clear();
        for copy in copies {
            let mut errors = self.runtime.run_scripts(copy);
            if !remote {
                errors.extend(self.runtime.run_local_scripts(copy));
            }
            self.collect(errors);
        }
        self.refresh_hotbar(index);
    }

    /// Keeps a hotbar in step with the tools the player has: ones that
    /// are gone drop out, new ones go at the end.
    pub(crate) fn refresh_hotbar(&mut self, index: usize) {
        let tree = self.runtime.tree().borrow();
        let avatar = &mut self.avatars[index];
        let mut owned: Vec<InstanceId> = Vec::new();
        for container in [avatar.backpack, avatar.character.map(|c| c.model)].into_iter().flatten() {
            if let Ok(children) = tree.children(container) {
                owned.extend(children.iter().copied().filter(|c| tree.class(*c) == Ok(ClassKind::Tool)));
            }
        }
        avatar.hotbar.retain(|t| owned.contains(t));
        for tool in owned {
            if !avatar.hotbar.contains(&tool) {
                avatar.hotbar.push(tool);
            }
        }
    }

    /// The player's tools in hotbar order, each with whether it's held.
    pub fn hotbar(&self) -> Vec<(InstanceId, bool)> {
        let Some(avatar) = self.local().map(|i| &self.avatars[i]) else { return Vec::new() };
        let tree = self.runtime.tree().borrow();
        let model = avatar.character.map(|c| c.model);
        avatar
            .hotbar
            .iter()
            .map(|t| (*t, model.is_some() && tree.parent(*t).ok().flatten() == model))
            .collect()
    }

    /// Holds the tool in hotbar `slot` (from 0), or puts it away if it's
    /// already held. Holding one tool puts any other away.
    pub fn toggle_tool(&mut self, slot: usize) {
        if self.runtime.net_role() == NetRole::Client {
            self.send_to_server(crate::net::ClientSays::Tool(slot));
            return;
        }
        if let Some(index) = self.local() {
            self.toggle_tool_for(index, slot);
        }
    }

    pub(crate) fn toggle_tool_for(&mut self, index: usize, slot: usize) {
        let avatar = &self.avatars[index];
        let (Some(tool), Some(character), Some(backpack)) =
            (avatar.hotbar.get(slot).copied(), avatar.character, avatar.backpack)
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

    /// Starts the arm swing of whoever holds each tool that was just used.
    fn start_swings(&mut self) {
        let swings = self.runtime.take_tool_swings();
        if swings.is_empty() {
            return;
        }
        let tree = self.runtime.tree().borrow();
        for tool in swings {
            let holder = tree.parent(tool).ok().flatten();
            for avatar in &mut self.avatars {
                if avatar.character.is_some_and(|c| Some(c.model) == holder) && avatar.slash <= 0.0 {
                    avatar.slash = crate::character::SLASH_TIME;
                }
            }
        }
    }

    /// Tells scripts where the mouse points (`None` when it's off the view).
    pub fn set_mouse(&mut self, mouse: Option<MouseRay>) {
        self.runtime.set_mouse(mouse);
    }

    /// Turns a character to face where it walks and puts its tool's
    /// handle in its hand.
    pub(crate) fn pose_character(&mut self, index: usize, input: CharacterInput, dt: f32) {
        let avatar = &mut self.avatars[index];
        let Some(character) = avatar.character else { return };
        let mut tree = self.runtime.tree().borrow_mut();
        if character.is_dead(&tree) {
            return;
        }
        // Jumping off the ground and falling into water make sounds.
        let (grounded, wet) =
            (self.physics.is_grounded(character.root), self.physics.is_in_water(character.root));
        let rising = self.physics.velocity(character.root).y > 10.0;
        let mut sounds = Vec::new();
        avatar.jump_quiet = (avatar.jump_quiet - dt).max(0.0);
        if avatar.was_grounded && !grounded && rising && avatar.jump_quiet <= 0.0 {
            avatar.jump_quiet = JUMP_QUIET;
            avatar.jumps = avatar.jumps.wrapping_add(1);
            sounds.push(("Jumping", Some(jump_pitch(avatar.jumps))));
        }
        if wet && !avatar.was_wet {
            sounds.push(("Splash", None));
        }
        (avatar.was_grounded, avatar.was_wet) = (grounded, wet);
        for (name, pitch) in sounds {
            if let Ok(Some(sound)) = tree.find_first_child(character.root, name)
                && tree.class(sound) == Ok(ClassKind::Sound)
            {
                if let Some(pitch) = pitch {
                    let _ = tree.set(sound, Property::PlaybackSpeed, PropertyValue::Number(pitch));
                }
                let _ = tree.set(sound, Property::Playing, PropertyValue::Bool(true));
                self.sound_restarts.push(sound);
            }
        }
        let sitting =
            avatar.seating.seat().is_some() || tree.get_ref(character.humanoid, Property::SeatPart).is_some();
        let dir = input.move_dir;
        if let Some(yaw) = input.face.filter(|_| !sitting) {
            let facing = PropertyValue::Vec3(Vec3::new(0.0, yaw, 0.0));
            let _ = tree.set(character.root, Property::Orientation, facing);
        } else if !sitting
            && (dir.x != 0.0 || dir.z != 0.0)
            && crate::character::auto_rotates(&tree, character.humanoid)
        {
            // Turn smoothly, the short way round, rather than snapping.
            let now = tree.get_vec3(character.root, Property::Orientation).unwrap_or_default().y;
            let yaw = crate::character::turn_toward(now, crate::character::yaw_of(dir), dt);
            let facing = PropertyValue::Vec3(Vec3::new(0.0, yaw, 0.0));
            let _ = tree.set(character.root, Property::Orientation, facing);
        }
        // Arms and legs swing with the walking speed.
        let v = self.physics.velocity(character.root);
        let speed = (v.x * v.x + v.z * v.z).sqrt();
        let moved = if sitting { 0.0 } else { speed * dt };
        avatar.stride = crate::character::stride_after(avatar.stride, moved, dt);
        let tool = tree.find_first_child_of_class(character.model, ClassKind::Tool).ok().flatten();
        let swing = crate::character::swing_for(avatar.stride);
        if tool.is_none() {
            avatar.slash = 0.0;
        }
        let slash = if avatar.slash > 0.0 { crate::character::slash_angle(avatar.slash) } else { 0.0 };
        avatar.slash = (avatar.slash - dt).max(0.0);
        let anim = self.runtime.animation_pose(character.humanoid);
        let pose = crate::character::Pose { swing, sitting, holding: tool.is_some(), slash, anim };
        character.pose(&mut tree, pose);
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
        let (offset, tip) = crate::character::hand_offset(slash);
        let at = pos + Rotation::from_orientation(orientation).apply(offset);
        let orientation = Vec3::new(tip.to_degrees(), orientation.y, orientation.z);
        let _ = tree.set(handle, Property::Anchored, PropertyValue::Bool(true));
        let _ = tree.set(handle, Property::CanCollide, PropertyValue::Bool(false));
        let _ = tree.set(handle, Property::Position, PropertyValue::Vec3(at));
        let _ = tree.set(handle, Property::Orientation, PropertyValue::Vec3(orientation));
    }

    /// The left mouse button went down (or up) on a GUI button.
    pub fn press_button(&mut self, button: InstanceId, down: bool) {
        let errors = self.runtime.press_button(button, down);
        self.collect(errors);
    }

    /// The player clicked a GUI button.
    pub fn click_button(&mut self, button: InstanceId) {
        let errors = self.runtime.click_button(button);
        self.collect(errors);
    }

    /// The pointer is over `object` (the topmost GUI object under it), or
    /// over no GUI. Fires `MouseLeave` on what it left and `MouseEnter` on
    /// what it moved onto, counting the GUI objects `object` is inside.
    pub fn gui_hover(&mut self, object: Option<InstanceId>) {
        let now: Vec<InstanceId> = {
            let shared = self.runtime.tree().clone();
            let tree = shared.borrow();
            let mut chain = Vec::new();
            let mut current = object;
            while let Some(id) = current.filter(|id| tree.class(*id).is_ok_and(|c| c.is_gui_object())) {
                chain.push(id);
                current = tree.parent(id).ok().flatten();
            }
            chain.reverse();
            chain
        };
        let before = std::mem::replace(&mut self.hovered, now.clone());
        for id in before.iter().rev().filter(|id| !now.contains(id)) {
            let errors = self.runtime.mouse_over(*id, false);
            self.collect(errors);
        }
        for id in now.iter().filter(|id| !before.contains(id)) {
            let errors = self.runtime.mouse_over(*id, true);
            self.collect(errors);
        }
    }

    /// A mouse button went down (or up) over the GUI: fires `InputBegan` (or
    /// `InputEnded`) on the hovered objects, topmost first, until one that
    /// takes input (a button, a text box or an Active object).
    pub fn gui_input(&mut self, input: UserInput, began: bool) {
        let chain = self.hovered.clone();
        for id in chain.into_iter().rev() {
            let sinks = {
                let tree = self.runtime.tree().borrow();
                tree.class(id).is_ok_and(|c| c.is_button() || c == ClassKind::TextBox)
                    || matches!(tree.get_extra(id, "Active"), Ok(Some(PropertyValue::Bool(true))))
            };
            let errors = self.runtime.gui_input(id, input, began);
            self.collect(errors);
            if sinks {
                break;
            }
        }
    }

    /// The player clicked a part. Fires its click detector if it has one and
    /// the character is close enough. Returns whether the part was clickable.
    pub fn click_part(&mut self, part: InstanceId) -> bool {
        if self.runtime.net_role() == NetRole::Client {
            let clickable = {
                let tree = self.runtime.tree().borrow();
                tree.find_first_child_of_class(part, ClassKind::ClickDetector).ok().flatten().is_some()
            };
            if clickable {
                self.send_to_server(crate::net::ClientSays::Click(part));
            }
            return clickable;
        }
        match self.local() {
            Some(index) => self.click_part_for(index, part),
            None => false,
        }
    }

    pub(crate) fn click_part_for(&mut self, index: usize, part: InstanceId) -> bool {
        let avatar = &self.avatars[index];
        let player = avatar.player;
        let (detector, in_reach) = {
            let tree = self.runtime.tree().borrow();
            let Some(detector) =
                tree.find_first_child_of_class(part, ClassKind::ClickDetector).ok().flatten()
            else {
                return false;
            };
            let reach = tree.get_number(detector, Property::MaxActivationDistance).unwrap_or(32.0) as f32;
            let from = avatar.character.and_then(|c| tree.get_vec3(c.root, Property::Position));
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

    /// Ends the session: every player leaves (firing the leave event, where
    /// games usually save), so call this before throwing the session away.
    pub fn stop(&mut self) {
        if self.runtime.net_role() == NetRole::Client {
            self.link = Default::default();
            return;
        }
        for avatar in std::mem::take(&mut self.avatars) {
            let errors = self.runtime.player_removing(avatar.player);
            self.collect(errors);
        }
        let errors = self.runtime.close();
        self.collect(errors);
        self.link = Default::default();
    }

    /// A key or mouse button went down. `processed` means the game's UI used
    /// it already (for example, a click on a button), so prompts ignore it.
    pub fn input_began(&mut self, input: UserInput, processed: bool) {
        let errors = self.runtime.input_began(input, processed);
        self.collect(errors);
        if self.runtime.net_role() == NetRole::Client && input == UserInput::MouseButton1 && !processed {
            self.send_to_server(crate::net::ClientSays::Activate(true));
        }
        if let (UserInput::Key(key), false) = (input, processed)
            && let Some(slot) = SLOT_KEYS.iter().position(|k| *k == key)
        {
            self.toggle_tool(slot);
        }
        if let (UserInput::Key(key), false, Some(index)) = (input, processed, self.local())
            && let Some(active) = self.prompts_for(index).into_iter().find(|p| p.key == key)
        {
            self.avatars[index].hold =
                Some(PromptHold { prompt: active.prompt, key, held: 0.0, triggered: false });
            self.advance_hold(index, 0.0);
        }
    }

    /// A key or mouse button came back up.
    pub fn input_ended(&mut self, input: UserInput, processed: bool) {
        let errors = self.runtime.input_ended(input, processed);
        self.collect(errors);
        if self.runtime.net_role() == NetRole::Client && input == UserInput::MouseButton1 {
            self.send_to_server(crate::net::ClientSays::Activate(false));
        }
        if let (UserInput::Key(key), Some(index)) = (input, self.local())
            && let Some(hold) = self.avatars[index].hold
            && hold.key == key
        {
            self.release_hold(index);
        }
    }

    /// Uses a prompt straight away, as when the player clicks it.
    pub fn trigger_prompt(&mut self, prompt: InstanceId) {
        let Some(index) = self.local() else { return };
        if !self.in_reach(index, prompt) {
            return;
        }
        self.prompt_fired(index, prompt, false);
        self.prompt_fired(index, prompt, true);
    }

    /// A player used (or, when `ended`, let go of) a prompt. On a player's
    /// computer its own scripts hear it and the server is told.
    fn prompt_fired(&mut self, index: usize, prompt: InstanceId, ended: bool) {
        if self.runtime.net_role() == NetRole::Client {
            self.send_to_server(crate::net::ClientSays::Prompt { prompt, ended });
        }
        let player = self.avatars[index].player;
        let errors = if ended {
            self.runtime.prompt_trigger_ended(prompt, player)
        } else {
            self.runtime.prompt_triggered(prompt, player)
        };
        self.collect(errors);
    }

    /// The server's check that `player` (on another computer) may use `prompt`.
    pub(crate) fn remote_prompt(&mut self, index: usize, prompt: InstanceId, ended: bool) {
        if ended || self.in_reach(index, prompt) {
            self.prompt_fired(index, prompt, ended);
        }
    }

    /// The nearest enabled prompt in reach of the character, if any.
    pub fn active_prompt(&self) -> Option<ActivePrompt> {
        self.prompts_for(self.local()?).into_iter().next()
    }

    /// The prompts shown to the player: the nearest in reach for each key,
    /// nearest first.
    pub fn active_prompts(&self) -> Vec<ActivePrompt> {
        self.local().map(|i| self.prompts_for(i)).unwrap_or_default()
    }

    fn in_reach(&self, index: usize, prompt: InstanceId) -> bool {
        self.prompts_for(index).iter().any(|a| a.prompt == prompt)
    }

    fn prompts_for(&self, index: usize) -> Vec<ActivePrompt> {
        let avatar = &self.avatars[index];
        let Some(character) = avatar.character else { return Vec::new() };
        let tree = self.runtime.tree().borrow();
        if character.is_dead(&tree) {
            return Vec::new();
        }
        let Some(from) = tree.get_vec3(character.root, Property::Position) else { return Vec::new() };
        let mut best: Vec<(f32, ActivePrompt)> = Vec::new();
        for prompt in tree.descendants_of_class(tree.workspace(), ClassKind::ProximityPrompt) {
            if tree.get_bool(prompt, Property::Enabled) == Some(false) {
                continue;
            }
            let Some(part) = tree.parent(prompt).ok().flatten() else { continue };
            let Some(at) = tree.get_vec3(part, Property::Position) else { continue };
            let distance = (at - from).length();
            let reach = tree.get_number(prompt, Property::MaxActivationDistance).unwrap_or(10.0) as f32;
            if distance > reach {
                continue;
            }
            let key = match tree.get_enum(prompt, Property::KeyboardKeyCode) {
                Some(EnumValue::KeyCode(k)) => k,
                _ => KeyCode::E,
            };
            if best.iter().any(|(d, a)| a.key == key && *d <= distance) {
                continue;
            }
            let duration = tree.get_number(prompt, Property::HoldDuration).unwrap_or(0.0) as f32;
            let progress = match avatar.hold {
                Some(h) if h.prompt == prompt && duration > 0.0 => (h.held / duration).min(1.0),
                Some(h) if h.prompt == prompt => 1.0,
                _ => 0.0,
            };
            best.retain(|(_, a)| a.key != key);
            best.push((distance, ActivePrompt { prompt, part, key, progress }));
        }
        best.sort_by(|a, b| a.0.total_cmp(&b.0));
        best.into_iter().map(|(_, a)| a).collect()
    }

    /// Moves a held prompt on by `dt`, triggering it once the hold is long enough.
    pub(crate) fn advance_hold(&mut self, index: usize, dt: f32) {
        let Some(mut hold) = self.avatars[index].hold else { return };
        if !self.in_reach(index, hold.prompt) {
            // Walked away or the prompt was turned off.
            self.release_hold(index);
            return;
        }
        hold.held += dt;
        let duration =
            self.runtime.tree().borrow().get_number(hold.prompt, Property::HoldDuration).unwrap_or(0.0)
                as f32;
        if !hold.triggered && hold.held >= duration {
            hold.triggered = true;
            self.avatars[index].hold = Some(hold);
            self.prompt_fired(index, hold.prompt, false);
        } else {
            self.avatars[index].hold = Some(hold);
        }
    }

    fn release_hold(&mut self, index: usize) {
        if let Some(hold) = self.avatars[index].hold.take()
            && hold.triggered
        {
            self.prompt_fired(index, hold.prompt, true);
        }
    }

    pub fn runtime(&self) -> &ScriptRuntime {
        &self.runtime
    }

    /// The physics state (velocities and the like), to give parts a push.
    pub fn physics_mut(&mut self) -> &mut Physics {
        &mut self.physics
    }

    /// The local player's character.
    pub fn character(&self) -> Option<Character> {
        self.local().and_then(|i| self.avatars[i].character)
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

    /// Particle bursts (`ParticleEmitter:Emit`) since the last call.
    pub fn take_emits(&mut self) -> Vec<(InstanceId, u32)> {
        std::mem::take(&mut self.emits)
    }

    /// Sounds that `Play` or `Stop` sent back to their beginning since the last call.
    pub fn take_sound_restarts(&mut self) -> Vec<InstanceId> {
        std::mem::take(&mut self.sound_restarts)
    }

    /// A sound played to its end: it stops and fires `Ended`.
    pub fn sound_ended(&mut self, sound: InstanceId) {
        let errors = self.runtime.sound_ended(sound);
        self.collect(errors);
    }

    /// Explosions being drawn right now.
    pub fn blasts(&self) -> &[Blast] {
        &self.blasts
    }

    /// Tells scripts what the player plays with (touch, a controller).
    pub fn set_devices(&mut self, devices: libroxel_script::Devices) {
        let errors = self.runtime.set_devices(devices);
        self.collect(errors);
    }

    /// Passes a finger on the screen to scripts (see [`libroxel_script::Runtime::touch`]).
    pub fn touch(&mut self, touch: crate::devices::ScreenTouch, time: f64) {
        let errors = self.runtime.touch(touch.id, touch.phase, touch.x, touch.y, touch.processed, time);
        self.collect(errors);
    }

    /// Lets scripts' long presses see time pass.
    pub fn touch_tick(&mut self, time: f64) {
        let errors = self.runtime.touch_tick(time);
        self.collect(errors);
    }

    /// The touch buttons the game's actions asked for.
    pub fn touch_buttons(&self) -> Vec<libroxel_script::TouchButton> {
        self.runtime.touch_buttons()
    }

    /// A touch button went down or up.
    pub fn press_touch_button(&mut self, name: &str, pressed: bool) {
        let errors = self.runtime.press_touch_button(name, pressed);
        self.collect(errors);
    }

    /// A new strength (0 to 1) for the phone's vibration motor, if a script set one.
    pub fn take_vibration(&self) -> Option<f32> {
        self.runtime.take_vibration()
    }

    /// Whether the game wants the on-screen stick and jump button.
    pub fn touch_controls_shown(&self) -> bool {
        self.runtime.touch_controls_shown()
    }

    /// Tells scripts where each screen GUI object was drawn: its top left
    /// corner and size, in pixels from the view's corner.
    pub fn set_gui_rects(&mut self, rects: Vec<(InstanceId, [f32; 4])>) {
        self.runtime.set_gui_rects(rects);
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
        self.character().map(|c| c.root)
    }

    /// Adds an engine note to the Output (like "Respawned").
    pub fn note(&mut self, text: impl Into<String>) {
        self.info(text);
    }

    pub(crate) fn info(&mut self, text: impl Into<String>) {
        self.log.push(LogEntry { level: LogLevel::Info, time: self.runtime.clock(), text: text.into() });
    }

    pub(crate) fn warn(&mut self, text: impl Into<String>) {
        self.log.push(LogEntry { level: LogLevel::Warn, time: self.runtime.clock(), text: text.into() });
    }

    pub(crate) fn collect(&mut self, errors: Vec<libroxel_script::ScriptError>) {
        let time = self.runtime.clock();
        for line in self.runtime.take_output() {
            let level = match line.level {
                OutputLevel::Print => LogLevel::Info,
                OutputLevel::Warn => LogLevel::Warn,
            };
            self.log.push(LogEntry { level, time, text: line.text });
        }
        for err in errors {
            self.log.push(LogEntry { level: LogLevel::Error, time, text: tidy_trace(&err.to_string()) });
        }
    }

    /// Takes the log lines produced since the last call.
    pub fn take_log(&mut self) -> Vec<LogEntry> {
        std::mem::take(&mut self.log)
    }

    /// Runs one frame: scripts, then character movement and physics, then
    /// touch events, then deaths and respawns. `input` drives the local
    /// player's character.
    ///
    /// A hosted game also hears from the players who joined and sends them
    /// what changed; a joined one only sends input and shows what the
    /// server sends (see [`crate::net`]).
    pub fn step(&mut self, dt: f32, input: CharacterInput) {
        match self.runtime.net_role() {
            NetRole::Client => return self.client_step(dt, input),
            NetRole::Server => self.serve_incoming(),
            NetRole::Solo => {}
        }
        self.simulate(dt, input);
        if self.runtime.net_role() == NetRole::Server {
            self.serve_outgoing();
        }
    }

    /// Fires the Changed events of what scripts changed this frame, and of
    /// what those handlers change in turn, before the frame is drawn (as
    /// Roblox does): a screen that a handler turns straight back on never
    /// shows off for a frame.
    pub(crate) fn settle_changes(&mut self) {
        for _ in 0..4 {
            let changes = self.runtime.tree().borrow_mut().drain_changes();
            if changes.is_empty() {
                break;
            }
            let errors = self.runtime.fire_changes(&changes);
            self.collect(errors);
            self.notice_deaths(&changes);
        }
    }

    /// Fires Died for humanoids whose health a change took to 0, whatever
    /// they belong to (as Roblox does): players' characters and walking
    /// characters are noticed as they move too, but a humanoid with no
    /// body to move only through this.
    fn notice_deaths(&mut self, changes: &[libroxel_instance::Change]) {
        let fallen: Vec<InstanceId> = {
            let tree = self.runtime.tree().borrow();
            changes
                .iter()
                .filter_map(|c| match c {
                    libroxel_instance::Change::PropertyChanged { id, property: Property::Health } => {
                        Some(*id)
                    }
                    _ => None,
                })
                .filter(|id| tree.class(*id) == Ok(ClassKind::Humanoid))
                .filter(|id| tree.get_number(*id, Property::Health).is_some_and(|h| h <= 0.0))
                .collect()
        };
        for humanoid in fallen {
            self.fire_died(humanoid);
        }
    }

    /// Fires `humanoid`'s Died event, the first time only.
    pub(crate) fn fire_died(&mut self, humanoid: InstanceId) {
        if self.died.insert(humanoid) {
            let errors = self.runtime.humanoid_died(humanoid);
            self.collect(errors);
        }
    }

    fn simulate(&mut self, dt: f32, input: CharacterInput) {
        let began = std::time::Instant::now();
        // Everything that changed since last frame, for Changed events.
        let changes = self.runtime.tree().borrow_mut().drain_changes();
        let errors = self.runtime.fire_changes(&changes);
        self.collect(errors);
        self.notice_deaths(&changes);
        let changed = began.elapsed().as_secs_f64();
        let errors = self.runtime.step(dt as f64);
        self.collect(errors);
        self.settle_changes();
        let scripted = began.elapsed().as_secs_f64();
        let physics_time;
        for index in 0..self.avatars.len() {
            self.advance_hold(index, dt);
        }

        let tree = self.runtime.tree().clone();
        self.explode(&tree);
        self.blasts.retain_mut(|b| {
            b.age += dt;
            b.age < BLAST_TIME
        });
        for (player, why) in self.runtime.take_kicks() {
            self.kick(player, &why);
        }
        for player in self.runtime.take_load_requests() {
            if let Some(index) = self.avatar_of(player) {
                self.respawn(index);
            }
        }
        let mut walks_ended = Vec::new();
        for (humanoid, target) in self.runtime.take_move_requests() {
            walks_ended.extend(self.npcs.request(humanoid, target).map(|h| (h, false)));
        }
        let local = self.local();
        let mut inputs: Vec<CharacterInput> = self
            .avatars
            .iter()
            .enumerate()
            .map(|(i, a)| if Some(i) == local { input } else { a.input })
            .collect();
        let mut seat_changes = Vec::new();
        let (result, died) = {
            let mut tree = tree.borrow_mut();
            for (avatar, input) in self.avatars.iter().zip(inputs.iter_mut()) {
                // A player's own character walks where MoveTo sends it until they steer.
                if let Some(c) = avatar.character.filter(|c| !c.is_dead(&tree)) {
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
            }
            self.emits.extend(self.runtime.take_emit_requests());
            self.sound_restarts.extend(self.runtime.take_sound_restarts());
            for (seat, humanoid) in self.runtime.take_sit_requests() {
                for avatar in &mut self.avatars {
                    if let Some(c) = avatar.character.filter(|c| c.humanoid == humanoid && !c.is_dead(&tree))
                    {
                        if avatar.seating.seat().is_some_and(|s| s != seat) {
                            seat_changes.extend(avatar.seating.stand(&mut tree, c));
                        }
                        seat_changes.extend(avatar.seating.sit(&mut tree, c, seat));
                    }
                }
            }
            for (avatar, input) in self.avatars.iter_mut().zip(inputs.iter_mut()) {
                let was_seated = avatar.seating.seat().is_some();
                let (seated_input, change) =
                    avatar.seating.before_physics(&mut tree, avatar.character, *input, dt);
                seat_changes.extend(change);
                if was_seated {
                    // Sitting: the keys went to the seat. Jumping off leaps up.
                    if let Some(c) = avatar.character.filter(|_| input.jump && seated_input.jump) {
                        let power = tree.get_number(c.humanoid, Property::JumpPower).unwrap_or(50.0) as f32;
                        self.physics.set_velocity(c.root, Vec3::new(0.0, power, 0.0));
                    }
                    *input = CharacterInput::default();
                }
            }
            self.physics.set_collision_rules(self.runtime.collision_rules());
            for (id, request) in self.runtime.take_velocity_requests() {
                if tree.get_bool(id, Property::Anchored) != Some(false) {
                    continue;
                }
                let v = match request {
                    VelocityRequest::Set(v) => v,
                    VelocityRequest::Add(dv) => self.physics.velocity(id) + dv,
                    VelocityRequest::Spin(spin) => {
                        let root = self.welds.root_for(id);
                        self.physics.set_angular_velocity(root, spin);
                        continue;
                    }
                };
                self.physics.set_velocity(id, v);
            }
            for (avatar, input) in self.avatars.iter().zip(inputs.iter()) {
                if avatar.moves_itself(&tree) {
                    if let Some(c) = avatar.character {
                        self.physics.hold(c.root);
                    }
                } else if let Some(c) = avatar.character.filter(|c| !c.is_dead(&tree)) {
                    c.drive(&tree, &mut self.physics, *input, dt);
                    c.share_moving(&mut tree, &self.physics);
                }
            }
            let player_models: Vec<InstanceId> =
                self.avatars.iter().filter_map(|a| a.character.map(|c| c.model)).collect();
            let poses = self.npcs.before_physics(&mut tree, &mut self.physics, &player_models, dt);
            crate::seat::drive_vehicles(&mut tree, &mut self.physics, dt);
            crate::weld::pose_joints(&mut tree, dt);
            self.welds.before_physics(&tree, &mut self.physics);
            let ties = crate::movers::apply(&tree, &self.welds, &mut self.physics, dt);
            self.hinges.before_physics(&tree, &mut self.physics, dt, ties);
            for avatar in &self.avatars {
                avatar.seating.hold(&tree, &mut self.physics, avatar.character);
            }
            let physics_began = std::time::Instant::now();
            let result = self.physics.step(&mut tree, dt);
            physics_time = physics_began.elapsed().as_secs_f64();
            for avatar in &self.avatars {
                if let Some(c) = avatar.character.filter(|c| !c.is_dead(&tree)) {
                    c.follow(&mut tree);
                }
            }
            let runtime = &self.runtime;
            let anim = |humanoid| runtime.animation_pose(humanoid);
            let (ended, died) = self.npcs.after_physics(&mut tree, &poses, dt, &anim);
            self.welds.after_physics(&mut tree);
            self.hinges.after_physics(&mut tree);
            for avatar in &mut self.avatars {
                avatar.seating.after_physics(&mut tree, avatar.character);
                // Touching a free seat sits the character down.
                if let Some(c) = avatar.character.filter(|c| !c.is_dead(&tree))
                    && avatar.seating.can_sit()
                    && let Some(seat) = crate::seat::touched_seat(&tree, c.root, &result.touches)
                {
                    seat_changes.extend(avatar.seating.sit(&mut tree, c, seat));
                    avatar.seating.after_physics(&mut tree, avatar.character);
                }
            }
            walks_ended.extend(ended);
            for avatar in &self.avatars {
                if let Some(c) = avatar.character {
                    walks_ended
                        .extend(self.npcs.finish_walks(&tree, dt, |h| (h == c.humanoid).then_some(c.root)));
                }
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
            {
                let mut t = tree.borrow_mut();
                if let Some(model) = t.parent(humanoid).ok().flatten() {
                    crate::character::fall_apart(&mut t, &mut self.physics, model);
                }
            }
            self.fire_died(humanoid);
        }
        self.runtime.set_velocities(self.physics.velocities().clone());
        self.runtime.set_angular_velocities(self.physics.angular_velocities().clone());
        self.start_swings();
        for (index, input) in inputs.into_iter().enumerate() {
            self.pose_character(index, input, dt);
            self.refresh_hotbar(index);
        }

        let touchable = |a: InstanceId, b: InstanceId| {
            let t = tree.borrow();
            [a, b].iter().all(|p| t.get_bool(*p, Property::CanTouch) != Some(false))
        };
        for (a, b) in result.touches {
            if !touchable(a, b) {
                continue;
            }
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

        for index in 0..self.avatars.len() {
            self.handle_death(index, dt);
        }
        let t = &mut self.frame_times;
        t.frames += 1;
        t.change_events += changed;
        t.scripts += scripted - changed;
        t.physics += physics_time;
        t.other += began.elapsed().as_secs_f64() - scripted - physics_time;
    }

    /// Where frame time went since the last call, and resets the sums.
    pub fn take_frame_times(&mut self) -> FrameTimes {
        std::mem::take(&mut self.frame_times)
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
                let shielded = model.is_some_and(|m| {
                    t.find_first_child_of_class(m, ClassKind::ForceField).ok().flatten().is_some()
                });
                if let Some(humanoid) = model
                    .filter(|_| !shielded)
                    .and_then(|m| t.find_first_child_of_class(m, ClassKind::Humanoid).ok().flatten())
                {
                    let _ = t.set(humanoid, Property::Health, PropertyValue::Number(0.0));
                }
                if t.get_bool(part, Property::Anchored) == Some(false) && pressure > 0.0 {
                    let mass = libroxel_instance::physical::mass(&t, part).max(0.5);
                    let away = (pos - at).normalize_or_zero() + Vec3::new(0.0, 0.6, 0.0);
                    let speed = (pressure / 5_000.0).min(250.0) / mass.sqrt()
                        * (1.0 - distance / radius.max(0.01) * 0.5);
                    let v = self.physics.velocity(part) + away.normalize_or_zero() * speed;
                    self.physics.set_velocity(part, v);
                }
            }
        }
    }

    fn handle_death(&mut self, index: usize, dt: f32) {
        let tree = self.runtime.tree().clone();
        if let Some(left) = self.avatars[index].first_spawn_in.as_mut() {
            *left -= dt;
            if *left <= 0.0 {
                self.avatars[index].first_spawn_in = None;
                if self.avatars[index].character.is_none() && self.auto_loads(&tree) {
                    self.spawn_character(index);
                }
            }
        }
        let avatar = &mut self.avatars[index];
        // The spawn shield wears off.
        if let Some((field, left)) = avatar.shield.as_mut() {
            *left -= dt;
            if *left <= 0.0 {
                let _ = tree.borrow_mut().destroy(*field);
                avatar.shield = None;
            }
        }
        let Some(character) = avatar.character else { return };
        if let Some(remaining) = avatar.respawn_in.as_mut() {
            *remaining -= dt;
            if *remaining <= 0.0 {
                self.respawn(index);
                if !self.avatars[index].remote {
                    self.info("Respawned");
                }
            }
        } else if !avatar.death_seen && character.is_dead(&tree.borrow()) {
            avatar.death_seen = true;
            if !avatar.remote {
                self.info("Your character died");
            }
            crate::character::fall_apart(&mut tree.borrow_mut(), &mut self.physics, character.model);
            self.fire_died(character.humanoid);
            // Without auto-loading, a script brings the character back with LoadCharacter.
            if self.auto_loads(&tree) {
                let delay = {
                    let t = tree.borrow();
                    t.service(ClassKind::Players)
                        .and_then(|p| t.get_number(p, Property::RespawnTime))
                        .map_or(RESPAWN_DELAY, |d| d as f32)
                };
                self.avatars[index].respawn_in = Some(delay);
            }
        }
    }

    /// Whether characters spawn by themselves (the players service's `CharacterAutoLoads`).
    fn auto_loads(&self, tree: &SharedTree) -> bool {
        let t = tree.borrow();
        t.service(ClassKind::Players).and_then(|p| t.get_bool(p, Property::CharacterAutoLoads)) != Some(false)
    }

    /// Removes a player's character, if any, and spawns a fresh one.
    fn respawn(&mut self, index: usize) {
        let tree = self.runtime.tree().clone();
        self.avatars[index].respawn_in = None;
        if let Some(c) = self.avatars[index].character {
            let change = self.avatars[index].seating.stand(&mut tree.borrow_mut(), c);
            if let Some(change) = change {
                let errors = self.runtime.seated(change.humanoid, change.seat);
                self.collect(errors);
            }
        }
        let player = self.avatars[index].player;
        if let Some(character) = self.avatars[index].character.take() {
            let errors = self.runtime.character_removing(player, character.model);
            self.collect(errors);
            let _ = tree.borrow_mut().destroy(character.model);
            self.copy_starter_gui(index, true);
        }
        self.spawn_character(index);
    }
}

/// Copies every child of `from` (if there is one) into `into`, returning the copies.
fn copy_children(tree: &mut InstanceTree, from: Option<InstanceId>, into: InstanceId) -> Vec<InstanceId> {
    let Some(from) = from else { return Vec::new() };
    let children = tree.children(from).map(<[InstanceId]>::to_vec).unwrap_or_default();
    children
        .into_iter()
        .filter_map(|child| {
            let copy = tree.clone_subtree(child).ok()?;
            tree.set_parent(copy, Some(into)).ok()?;
            Some(copy)
        })
        .collect()
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

/// Convenience for tests and tools: a session over a fresh copy of `tree`,
/// with the character there from the start (see
/// [`PlaySession::load_characters_now`]).
pub fn start_on(tree: InstanceTree, with_character: bool) -> Result<PlaySession, String> {
    let mut session = PlaySession::start(std::rc::Rc::new(std::cell::RefCell::new(tree)), with_character)?;
    session.load_characters_now();
    Ok(session)
}

/// An error's text without the stack frames that are Libroxel's own
/// machinery rather than the game's scripts.
fn tidy_trace(text: &str) -> String {
    text.lines()
        .filter(|l| {
            let l = l.trim();
            l != "[C]: in ?" && !l.starts_with("__mlua")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
