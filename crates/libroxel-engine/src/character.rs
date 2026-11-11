//! The player's character: a model in the workspace that scripts can see,
//! moved by input instead of by physics forces.

use libroxel_instance::{ClassKind, Color3, InstanceId, InstanceTree, Property, PropertyValue, Vec3};
use libroxel_script::names::character as names;

use crate::physics::Physics;

/// The size of the character's collision box.
pub const BODY_SIZE: Vec3 = Vec3::new(2.0, 4.0, 1.5);
const HEAD_SIZE: Vec3 = Vec3::new(1.2, 1.2, 1.2);
const DEFAULT_SPAWN: Vec3 = Vec3::new(0.0, 10.0, 0.0);

/// What the player is asking the character to do this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CharacterInput {
    /// Desired direction in world space. Y is ignored; length is clamped to 1.
    pub move_dir: Vec3,
    pub jump: bool,
    /// The forward (1) and back (-1) keys, for driving a vehicle seat.
    pub throttle: f32,
    /// The right (1) and left (-1) keys, for driving a vehicle seat.
    pub steer: f32,
}

/// The instances that make up a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Character {
    pub model: InstanceId,
    pub root: InstanceId,
    pub head: InstanceId,
    pub humanoid: InstanceId,
}

/// Where a new character should appear: on top of the first spawn location.
pub fn spawn_point(tree: &InstanceTree) -> Vec3 {
    spawn_point_for(tree, None)
}

fn same_color(a: Color3, b: Color3) -> bool {
    (a.r - b.r).abs() < 0.01 && (a.g - b.g).abs() < 0.01 && (a.b - b.b).abs() < 0.01
}

/// Where a character on a team of colour `team` should appear: the first
/// spawn location for that team, else the first neutral one, else any.
pub fn spawn_point_for(tree: &InstanceTree, team: Option<Color3>) -> Vec3 {
    let spawns = tree.descendants_of_class(tree.workspace(), ClassKind::SpawnLocation);
    let neutral = |id: InstanceId| tree.get_bool(id, Property::Neutral) != Some(false);
    let for_team = |id: InstanceId| {
        !neutral(id)
            && team.zip(tree.get_color(id, Property::TeamColor)).is_some_and(|(t, c)| same_color(t, c))
    };
    let chosen = spawns
        .iter()
        .copied()
        .find(|s| for_team(*s))
        .or_else(|| spawns.iter().copied().find(|s| neutral(*s)))
        .or_else(|| spawns.first().copied());
    chosen
        .and_then(|id| {
            let pos = tree.get_vec3(id, Property::Position)?;
            let size = tree.get_vec3(id, Property::Size)?;
            Some(pos + Vec3::new(0.0, size.y / 2.0 + BODY_SIZE.y / 2.0 + 0.1, 0.0))
        })
        .unwrap_or(DEFAULT_SPAWN)
}

fn set(tree: &mut InstanceTree, id: InstanceId, property: Property, value: PropertyValue) {
    tree.set(id, property, value).expect("character properties are valid");
}

impl Character {
    /// Builds a character in the workspace at the spawn point.
    pub fn spawn(tree: &mut InstanceTree) -> Character {
        Self::spawn_named(tree, names::MODEL)
    }

    /// Like [`spawn`](Self::spawn), with the model called `name` (usually the player's name).
    pub fn spawn_named(tree: &mut InstanceTree, name: &str) -> Character {
        Self::spawn_for_team(tree, name, None)
    }

    /// Like [`spawn_named`](Self::spawn_named), at a spawn for the team with colour `team`.
    pub fn spawn_for_team(tree: &mut InstanceTree, name: &str, team: Option<Color3>) -> Character {
        let at = spawn_point_for(tree, team);
        let ws = tree.workspace();
        let make = |tree: &mut InstanceTree, class, name: &str, parent| {
            let id = tree.create(class, name).expect("character classes are creatable");
            tree.set_parent(id, Some(parent)).expect("fresh instances can be parented");
            id
        };
        let model = make(tree, ClassKind::Model, name, ws);
        let root = make(tree, ClassKind::Part, names::ROOT_PART, model);
        set(tree, root, Property::Size, PropertyValue::Vec3(BODY_SIZE));
        set(tree, root, Property::Position, PropertyValue::Vec3(at));
        set(tree, root, Property::Color, PropertyValue::Color3(Color3::from_rgb8(52, 110, 196)));
        let head = make(tree, ClassKind::Part, "Head", model);
        set(tree, head, Property::Size, PropertyValue::Vec3(HEAD_SIZE));
        set(tree, head, Property::Anchored, PropertyValue::Bool(true));
        set(tree, head, Property::CanCollide, PropertyValue::Bool(false));
        set(tree, head, Property::Color, PropertyValue::Color3(Color3::from_rgb8(245, 205, 48)));
        let humanoid = make(tree, ClassKind::Humanoid, names::HUMANOID, model);
        let character = Character { model, root, head, humanoid };
        character.place_head(tree);
        character
    }

    fn place_head(&self, tree: &mut InstanceTree) {
        if let Some(pos) = tree.get_vec3(self.root, Property::Position) {
            let head_pos = pos + Vec3::new(0.0, BODY_SIZE.y / 2.0 + HEAD_SIZE.y / 2.0, 0.0);
            let _ = tree.set(self.head, Property::Position, PropertyValue::Vec3(head_pos));
        }
    }

    /// Whether the character has died: no health left, or a script took it apart.
    pub fn is_dead(&self, tree: &InstanceTree) -> bool {
        let gone = |id| !tree.exists(id) || tree.is_destroyed(id).unwrap_or(true);
        let detached = |id| !tree.is_descendant_of(id, tree.workspace()).unwrap_or(false);
        gone(self.root)
            || detached(self.root)
            || gone(self.humanoid)
            || tree.get_number(self.humanoid, Property::Health).is_some_and(|h| h <= 0.0)
            || tree
                .get_vec3(self.root, Property::Position)
                .is_some_and(|p| p.y < crate::physics::FALLEN_PARTS_HEIGHT)
    }

    /// Turns input into velocity. Call before [`Physics::step`].
    pub fn drive(&self, tree: &InstanceTree, physics: &mut Physics, input: CharacterInput) {
        drive(tree, physics, self.root, self.humanoid, input);
    }

    /// Keeps the head on the body. Call after [`Physics::step`].
    pub fn follow(&self, tree: &mut InstanceTree) {
        self.place_head(tree);
    }
}

/// Moves any humanoid's root part: walking at its walk speed in the input's
/// direction, and jumping when on the ground.
pub fn drive(
    tree: &InstanceTree,
    physics: &mut Physics,
    root: InstanceId,
    humanoid: InstanceId,
    input: CharacterInput,
) {
    {
        let walk = tree.get_number(humanoid, Property::WalkSpeed).unwrap_or(16.0) as f32;
        let jump = tree.get_number(humanoid, Property::JumpPower).unwrap_or(50.0) as f32;
        let mut dir = Vec3::new(input.move_dir.x, 0.0, input.move_dir.z);
        if dir.length() > 1.0 {
            dir = dir.normalize_or_zero();
        }
        let mut v = physics.velocity(root);
        let (want_x, want_z) = (dir.x * walk, dir.z * walk);
        let flung = (v.x * v.x + v.z * v.z).sqrt() > walk + 1.0;
        if flung && !physics.is_grounded(root) {
            // Thrown by something (a launch pad, an explosion): keep the
            // momentum in the air, steering only a little.
            v.x += (want_x - v.x) * 0.03;
            v.z += (want_z - v.z) * 0.03;
        } else {
            v.x = want_x;
            v.z = want_z;
        }
        if input.jump && physics.is_grounded(root) {
            v.y = jump;
        }
        physics.set_velocity(root, v);
    }
}
