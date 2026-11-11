//! The player's character: a model in the workspace that scripts can see,
//! moved by input instead of by physics forces.

use libroxel_instance::{
    ClassKind, Color3, InstanceId, InstanceTree, Property, PropertyValue, Rotation, Vec3,
};
use libroxel_script::names::character as names;

use crate::physics::Physics;

/// The size of the character's collision box.
pub const BODY_SIZE: Vec3 = Vec3::new(2.0, 4.0, 1.5);
const HEAD_SIZE: Vec3 = Vec3::new(1.6, 1.4, 1.4);
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
    pub limbs: Limbs,
}

// ---------------------------------------------------------------------------
// The body: a blocky figure drawn around the (invisible) root part.
// ---------------------------------------------------------------------------

/// The parts of a character's body other than its root and head. They are
/// anchored and don't collide; [`pose_rig`] puts them in place every frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limbs {
    pub torso: InstanceId,
    pub left_arm: InstanceId,
    pub right_arm: InstanceId,
    pub left_leg: InstanceId,
    pub right_leg: InstanceId,
    pub left_eye: InstanceId,
    pub right_eye: InstanceId,
}

/// Part names, which scripts can find with `FindFirstChild`.
pub mod part_names {
    pub const HEAD: &str = "Head";
    pub const TORSO: &str = "Torso";
    pub const LEFT_ARM: &str = "LeftArm";
    pub const RIGHT_ARM: &str = "RightArm";
    pub const LEFT_LEG: &str = "LeftLeg";
    pub const RIGHT_LEG: &str = "RightLeg";
    pub const LEFT_EYE: &str = "LeftEye";
    pub const RIGHT_EYE: &str = "RightEye";
}

/// The colours a body is made in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyColors {
    pub skin: Color3,
    pub shirt: Color3,
    pub pants: Color3,
}

/// Shirt colours, handed out to players in turn by their user id.
pub const SHIRTS: [[u8; 3]; 6] =
    [[236, 98, 74], [124, 92, 214], [32, 168, 150], [242, 170, 40], [226, 86, 150], [70, 150, 230]];

impl BodyColors {
    /// The usual look, with the shirt for player number `n`.
    pub fn for_player(n: usize) -> BodyColors {
        let [r, g, b] = SHIRTS[n % SHIRTS.len()];
        BodyColors {
            skin: Color3::from_rgb8(238, 196, 160),
            shirt: Color3::from_rgb8(r, g, b),
            pants: Color3::from_rgb8(50, 56, 84),
        }
    }
}

const TORSO_SIZE: Vec3 = Vec3::new(2.2, 1.8, 1.1);
const ARM_SIZE: Vec3 = Vec3::new(0.8, 1.9, 0.9);
const LEG_SIZE: Vec3 = Vec3::new(0.95, 2.0, 1.0);
const EYE_SIZE: Vec3 = Vec3::new(0.25, 0.35, 0.05);
/// Where things sit relative to the root part's middle (the hips).
const SHOULDER: Vec3 = Vec3::new(1.5, 1.7, 0.0);
const HIP: Vec3 = Vec3::new(0.5, 0.0, 0.0);
const TORSO_AT: Vec3 = Vec3::new(0.0, 0.9, 0.0);
const HEAD_AT: Vec3 = Vec3::new(0.0, 2.55, 0.0);
const EYE_AT: Vec3 = Vec3::new(0.35, 2.65, -0.71);
/// Where a held tool's handle goes, relative to the root, with the arm raised.
pub const HAND_OFFSET: Vec3 = Vec3::new(1.5, 1.6, -2.3);

/// How the body is held this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pose {
    /// How far arms and legs swing from straight down, in radians.
    pub swing: f32,
    pub sitting: bool,
    /// The right arm points forward, holding a tool.
    pub holding: bool,
}

/// Adds a head and limbs to `model`, coloured `colors`. Returns the head
/// and the limbs; place them with [`pose_rig`].
pub fn build_rig(tree: &mut InstanceTree, model: InstanceId, colors: BodyColors) -> (InstanceId, Limbs) {
    use part_names::*;
    let mut make = |name: &str, size: Vec3, color: Color3| {
        let id = tree.create(ClassKind::Part, name).expect("parts can be created");
        tree.set_parent(id, Some(model)).expect("fresh parts can be parented");
        set(tree, id, Property::Size, PropertyValue::Vec3(size));
        set(tree, id, Property::Color, PropertyValue::Color3(color));
        set(tree, id, Property::Anchored, PropertyValue::Bool(true));
        set(tree, id, Property::CanCollide, PropertyValue::Bool(false));
        id
    };
    let head = make(HEAD, HEAD_SIZE, colors.skin);
    let dark = Color3::from_rgb8(30, 30, 40);
    let limbs = Limbs {
        torso: make(TORSO, TORSO_SIZE, colors.shirt),
        left_arm: make(LEFT_ARM, ARM_SIZE, colors.skin),
        right_arm: make(RIGHT_ARM, ARM_SIZE, colors.skin),
        left_leg: make(LEFT_LEG, LEG_SIZE, colors.pants),
        right_leg: make(RIGHT_LEG, LEG_SIZE, colors.pants),
        left_eye: make(LEFT_EYE, EYE_SIZE, dark),
        right_eye: make(RIGHT_EYE, EYE_SIZE, dark),
    };
    (head, limbs)
}

/// Finds the head and limbs [`build_rig`] made in `model`, if they are all there.
pub fn find_rig(tree: &InstanceTree, model: InstanceId) -> Option<(InstanceId, Limbs)> {
    use part_names::*;
    let find = |name: &str| tree.find_first_child(model, name).ok().flatten();
    Some((
        find(HEAD)?,
        Limbs {
            torso: find(TORSO)?,
            left_arm: find(LEFT_ARM)?,
            right_arm: find(RIGHT_ARM)?,
            left_leg: find(LEFT_LEG)?,
            right_leg: find(RIGHT_LEG)?,
            left_eye: find(LEFT_EYE)?,
            right_eye: find(RIGHT_EYE)?,
        },
    ))
}

impl Limbs {
    /// Every limb part.
    pub fn parts(&self) -> [InstanceId; 7] {
        [
            self.torso,
            self.left_arm,
            self.right_arm,
            self.left_leg,
            self.right_leg,
            self.left_eye,
            self.right_eye,
        ]
    }
}

/// Puts the head and limbs around `root` (facing its yaw) in `pose`.
pub fn pose_rig(tree: &mut InstanceTree, root: InstanceId, head: InstanceId, limbs: &Limbs, pose: Pose) {
    let Some(at) = tree.get_vec3(root, Property::Position) else { return };
    let yaw = tree.get_vec3(root, Property::Orientation).unwrap_or_default().y;
    let turn = Rotation::about_y(yaw.to_radians());
    let mut place = |id: InstanceId, pivot: Vec3, hang: Vec3, tilt: f32| {
        let offset = pivot + Rotation::about_x(tilt).apply(hang);
        let pos = at + turn.apply(offset);
        let orientation = Vec3::new(tilt.to_degrees(), yaw, 0.0);
        if tree.get_vec3(id, Property::Position) != Some(pos) {
            let _ = tree.set(id, Property::Position, PropertyValue::Vec3(pos));
        }
        if tree.get_vec3(id, Property::Orientation) != Some(orientation) {
            let _ = tree.set(id, Property::Orientation, PropertyValue::Vec3(orientation));
        }
    };
    let (legs, arms) =
        if pose.sitting { (90f32.to_radians(), 20f32.to_radians()) } else { (pose.swing, pose.swing) };
    let (left_leg, right_leg) = if pose.sitting { (legs, legs) } else { (legs, -legs) };
    let (left_arm, mut right_arm) = if pose.sitting { (arms, arms) } else { (-arms, arms) };
    if pose.holding {
        right_arm = 90f32.to_radians();
    }
    let flip = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
    let still = Vec3::ZERO;
    place(head, HEAD_AT, still, 0.0);
    place(limbs.left_eye, flip(EYE_AT), still, 0.0);
    place(limbs.right_eye, EYE_AT, still, 0.0);
    place(limbs.torso, TORSO_AT, still, 0.0);
    place(limbs.left_arm, flip(SHOULDER), Vec3::new(0.0, -0.85, 0.0), left_arm);
    place(limbs.right_arm, SHOULDER, Vec3::new(0.0, -0.85, 0.0), right_arm);
    place(limbs.left_leg, flip(HIP), Vec3::new(0.0, -1.0, 0.0), left_leg);
    place(limbs.right_leg, HIP, Vec3::new(0.0, -1.0, 0.0), right_leg);
}

/// How far limbs swing at `speed` studs per second, `phase` radians into a stride.
pub fn swing_for(speed: f32, phase: f32) -> f32 {
    phase.sin() * 0.7 * (speed / 12.0).min(1.0)
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
        Self::spawn_dressed(tree, name, team, BodyColors::for_player(0))
    }

    /// Like [`spawn_for_team`](Self::spawn_for_team), with the body coloured `colors`.
    pub fn spawn_dressed(
        tree: &mut InstanceTree,
        name: &str,
        team: Option<Color3>,
        colors: BodyColors,
    ) -> Character {
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
        // The root is the collision box; the body drawn around it is the rig.
        set(tree, root, Property::Transparency, PropertyValue::Number(1.0));
        let (head, limbs) = build_rig(tree, model, colors);
        let humanoid = make(tree, ClassKind::Humanoid, names::HUMANOID, model);
        // New characters start with the starter player's walk speed and jump power.
        if let Some(starter) = tree.service(ClassKind::StarterPlayer) {
            for (from, to) in [
                (Property::CharacterWalkSpeed, Property::WalkSpeed),
                (Property::CharacterJumpPower, Property::JumpPower),
            ] {
                if let Some(n) = tree.get_number(starter, from) {
                    set(tree, humanoid, to, PropertyValue::Number(n));
                }
            }
        }
        let character = Character { model, root, head, humanoid, limbs };
        character.pose(tree, Pose::default());
        character
    }

    /// The character a model made by [`spawn_dressed`](Self::spawn_dressed)
    /// is, found by its parts' names (as on a computer that was sent it).
    pub fn from_model(tree: &InstanceTree, model: InstanceId) -> Option<Character> {
        let root = tree.find_first_child(model, names::ROOT_PART).ok().flatten()?;
        let humanoid = tree.find_first_child_of_class(model, ClassKind::Humanoid).ok().flatten()?;
        let (head, limbs) = find_rig(tree, model)?;
        Some(Character { model, root, head, humanoid, limbs })
    }

    /// Puts the body around the root in `pose`.
    pub fn pose(&self, tree: &mut InstanceTree, pose: Pose) {
        pose_rig(tree, self.root, self.head, &self.limbs, pose);
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
                .is_some_and(|p| p.y < crate::physics::fallen_parts_height(tree))
    }

    /// Turns input into velocity. Call before [`Physics::step`].
    pub fn drive(&self, tree: &InstanceTree, physics: &mut Physics, input: CharacterInput) {
        drive(tree, physics, self.root, self.humanoid, input);
    }

    /// Keeps the body on the root, standing straight. Call after
    /// [`Physics::step`]; the session poses it properly later in the frame.
    pub fn follow(&self, tree: &mut InstanceTree) {
        self.pose(tree, Pose::default());
    }
}

/// How fast swimmers rise holding Space, and drift up otherwise (studs a second).
const SWIM_UP: f32 = 10.0;
const FLOAT_UP: f32 = 2.0;

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
        if physics.is_in_water(root) {
            // Swimming: a little slower. Swimmers drift up to the top and
            // tread water there; Space swims up faster, and at the top
            // leaps out (onto a bank, say).
            v.x = dir.x * walk * 0.75;
            v.z = dir.z * walk * 0.75;
            let head = tree.get_vec3(root, Property::Position).map(|p| p + Vec3::new(0.0, 1.5, 0.0));
            let surfacing = head.is_some_and(|h| {
                let wet = tree.terrain().and_then(|t| tree.voxels(t).ok()).is_some_and(|w| w.water_at(h));
                !wet
            });
            v.y = match (input.jump, surfacing) {
                (true, true) => jump * 0.8,
                (true, false) => SWIM_UP,
                (false, true) => 0.0,
                (false, false) => FLOAT_UP,
            };
            physics.set_velocity(root, v);
            return;
        }
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
