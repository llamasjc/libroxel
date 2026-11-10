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
    tree.descendants_of_class(tree.workspace(), ClassKind::SpawnLocation)
        .into_iter()
        .find_map(|id| {
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
        let at = spawn_point(tree);
        let ws = tree.workspace();
        let make = |tree: &mut InstanceTree, class, name: &str, parent| {
            let id = tree.create(class, name).expect("character classes are creatable");
            tree.set_parent(id, Some(parent)).expect("fresh instances can be parented");
            id
        };
        let model = make(tree, ClassKind::Model, names::MODEL, ws);
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
        let walk = tree.get_number(self.humanoid, Property::WalkSpeed).unwrap_or(16.0) as f32;
        let jump = tree.get_number(self.humanoid, Property::JumpPower).unwrap_or(50.0) as f32;
        let mut dir = Vec3::new(input.move_dir.x, 0.0, input.move_dir.z);
        if dir.length() > 1.0 {
            dir = dir.normalize_or_zero();
        }
        let mut v = physics.velocity(self.root);
        v.x = dir.x * walk;
        v.z = dir.z * walk;
        if input.jump && physics.is_grounded(self.root) {
            v.y = jump;
        }
        physics.set_velocity(self.root, v);
    }

    /// Keeps the head on the body. Call after [`Physics::step`].
    pub fn follow(&self, tree: &mut InstanceTree) {
        self.place_head(tree);
    }
}
