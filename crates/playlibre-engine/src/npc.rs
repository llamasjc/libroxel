//! Humanoids other than the player's character: any model in the workspace
//! with a `Humanoid` and an unanchored `HumanoidRootPart` walks where
//! scripts send it with `MoveTo`, jumps when its `Jump` is set, and carries
//! the rest of its parts along with its root.

// A fast fixed hash: these maps are read thousands of times a step.
use bevy::platform::collections::{HashMap, HashSet};

use playlibre_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue, Rotation, Vec3};
use playlibre_script::names;

use crate::character::{CharacterInput, drive};
use crate::physics::Physics;

/// How long a walk may take before it gives up, in seconds.
pub const MOVE_TIMEOUT: f32 = 8.0;
/// How close (across the ground) counts as there.
const ARRIVED: f32 = 1.0;

#[derive(Debug, Clone, Copy)]
struct Walk {
    target: Vec3,
    elapsed: f32,
}

/// A walking model as it was before physics moved it.
#[derive(Debug, Clone, Copy)]
pub struct Pose {
    humanoid: InstanceId,
    model: InstanceId,
    root: InstanceId,
    pos: Vec3,
    yaw: f32,
    dir: Vec3,
}

/// Where every humanoid is walking to, and which have died.
#[derive(Debug, Default)]
pub struct Npcs {
    walks: HashMap<InstanceId, Walk>,
    dead: HashSet<InstanceId>,
    /// How far through a stride each walking model is, for swinging its limbs.
    strides: HashMap<InstanceId, f32>,
}

/// How the animations playing on a humanoid turn its limbs, if any play.
pub type AnimationPose<'a> = dyn Fn(InstanceId) -> Option<(playlibre_instance::animation::Pose, bool)> + 'a;

/// The direction across the ground from `from` to `to`, and how far.
fn toward(from: Vec3, to: Vec3) -> (Vec3, f32) {
    let flat = Vec3::new(to.x - from.x, 0.0, to.z - from.z);
    (flat.normalize_or_zero(), flat.length())
}

impl Npcs {
    /// Starts (or with `None`, stops) a walk. Returns the humanoid if a walk
    /// it was already on was cut short, so its finish event can fire.
    pub fn request(&mut self, humanoid: InstanceId, target: Option<Vec3>) -> Option<InstanceId> {
        let replaced = match target {
            Some(target) => self.walks.insert(humanoid, Walk { target, elapsed: 0.0 }),
            None => self.walks.remove(&humanoid),
        };
        replaced.map(|_| humanoid)
    }

    /// Moves the end of each walk started with `MoveTo(point, part)` to
    /// where that point on the part is now, so the walk follows the part.
    pub fn follow_parts(&mut self, tree: &InstanceTree) {
        use names::character::{WALK_OFFSET, WALK_TO_PART};
        for (&humanoid, walk) in self.walks.iter_mut() {
            let Ok(Some(PropertyValue::Ref(Some(part)))) = tree.get_extra(humanoid, WALK_TO_PART) else {
                continue;
            };
            let Ok(Some(PropertyValue::Vec3(local))) = tree.get_extra(humanoid, WALK_OFFSET) else {
                continue;
            };
            if tree.is_destroyed(part).unwrap_or(true) {
                continue;
            }
            let Some(pos) = tree.get_vec3(part, Property::Position) else { continue };
            let turn =
                Rotation::from_orientation(tree.get_vec3(part, Property::Orientation).unwrap_or_default());
            walk.target = pos + turn.apply(local);
        }
    }

    /// Whether `humanoid` is walking somewhere.
    pub fn is_walking(&self, humanoid: InstanceId) -> bool {
        self.walks.contains_key(&humanoid)
    }

    /// Input that walks `humanoid` (whose root is at `pos`) toward its target.
    pub fn input_for(&self, humanoid: InstanceId, pos: Vec3) -> CharacterInput {
        match self.walks.get(&humanoid) {
            Some(walk) => {
                CharacterInput { move_dir: toward(pos, walk.target).0, jump: false, ..Default::default() }
            }
            None => CharacterInput::default(),
        }
    }

    /// Sets every model's walk going. `players` are the players' character
    /// models, which the session drives itself. Returns where each one was,
    /// for [`after_physics`](Self::after_physics).
    pub fn before_physics(
        &mut self,
        tree: &mut InstanceTree,
        physics: &mut Physics,
        players: &[InstanceId],
        dt: f32,
    ) -> Vec<Pose> {
        let mut poses = Vec::new();
        for humanoid in tree.descendants_of_class(tree.workspace(), ClassKind::Humanoid) {
            let Some(model) = tree.parent(humanoid).ok().flatten() else { continue };
            if players.contains(&model) || tree.class(model) != Ok(ClassKind::Model) {
                continue;
            }
            let Some(root) = tree.find_first_child(model, names::character::ROOT_PART).ok().flatten() else {
                continue;
            };
            if !tree.class(root).is_ok_and(|c| c.is_physical())
                || tree.get_bool(root, Property::Anchored) != Some(false)
            {
                continue;
            }
            let Some(pos) = tree.get_vec3(root, Property::Position) else { continue };
            let yaw = tree.get_vec3(root, Property::Orientation).unwrap_or_default().y;
            let alive = tree.get_number(humanoid, Property::Health).is_some_and(|h| h > 0.0);
            let mut input = if alive { self.input_for(humanoid, pos) } else { CharacterInput::default() };
            if alive && tree.get_bool(humanoid, Property::Jump) == Some(true) {
                input.jump = true;
                let _ = tree.set(humanoid, Property::Jump, PropertyValue::Bool(false));
            }
            drive(tree, physics, root, humanoid, input, dt);
            poses.push(Pose { humanoid, model, root, pos, yaw, dir: input.move_dir });
        }
        poses
    }

    /// Carries each model's other parts along with its root, turns it to
    /// face where it walks, and ends walks that arrived or took too long.
    /// Returns the walks that ended (with whether they arrived) and the
    /// humanoids that just died.
    pub fn after_physics(
        &mut self,
        tree: &mut InstanceTree,
        poses: &[Pose],
        dt: f32,
        anim: &AnimationPose<'_>,
    ) -> (Vec<(InstanceId, bool)>, Vec<InstanceId>) {
        for pose in poses {
            let Some(new_pos) = tree.get_vec3(pose.root, Property::Position) else { continue };
            let new_yaw = if (pose.dir.x != 0.0 || pose.dir.z != 0.0)
                && crate::character::auto_rotates(tree, pose.humanoid)
            {
                crate::character::turn_toward(pose.yaw, crate::character::yaw_of(pose.dir), dt)
            } else {
                pose.yaw
            };
            let turn = Rotation::about_y((new_yaw - pose.yaw).to_radians());
            if new_yaw != pose.yaw {
                let o = tree.get_vec3(pose.root, Property::Orientation).unwrap_or_default();
                let _ = tree.set(
                    pose.root,
                    Property::Orientation,
                    PropertyValue::Vec3(Vec3::new(o.x, new_yaw, o.z)),
                );
            }
            // A model with a body walks properly: limbs swing as it goes.
            let rig = crate::character::find_rig(tree, pose.model);
            if let Some((head, limbs)) = rig {
                let moved = Vec3::new(new_pos.x - pose.pos.x, 0.0, new_pos.z - pose.pos.z).length();
                let stride = self.strides.entry(pose.humanoid).or_insert(0.0);
                *stride = crate::character::stride_after(*stride, moved, dt);
                let swing = crate::character::swing_for(*stride);
                crate::character::pose_rig(
                    tree,
                    pose.root,
                    head,
                    &limbs,
                    crate::character::Pose { swing, anim: anim(pose.humanoid), ..Default::default() },
                );
            }
            let posed: Vec<InstanceId> = rig
                .map(|(head, limbs)| std::iter::once(head).chain(limbs.parts()).collect())
                .unwrap_or_default();
            for part in tree.descendants_of_class(pose.model, ClassKind::Part) {
                // Tools are placed by the tool code, not carried; the body is posed above.
                if part == pose.root || in_tool(tree, part, pose.model) || posed.contains(&part) {
                    continue;
                }
                let Some(p) = tree.get_vec3(part, Property::Position) else { continue };
                let moved = new_pos + turn.apply(p - pose.pos);
                let _ = tree.set(part, Property::Anchored, PropertyValue::Bool(true));
                let _ = tree.set(part, Property::CanCollide, PropertyValue::Bool(false));
                let _ = tree.set(part, Property::Position, PropertyValue::Vec3(moved));
                if new_yaw != pose.yaw {
                    let o = tree.get_vec3(part, Property::Orientation).unwrap_or_default();
                    let turned = Vec3::new(o.x, o.y + new_yaw - pose.yaw, o.z);
                    let _ = tree.set(part, Property::Orientation, PropertyValue::Vec3(turned));
                }
            }
        }

        let mut finished = Vec::new();
        let mut died = Vec::new();
        for pose in poses {
            let dead = tree.get_number(pose.humanoid, Property::Health).is_none_or(|h| h <= 0.0);
            if dead && self.dead.insert(pose.humanoid) {
                died.push(pose.humanoid);
                if self.walks.remove(&pose.humanoid).is_some() {
                    finished.push((pose.humanoid, false));
                }
            }
        }
        finished
            .extend(self.finish_walks(tree, dt, |h| poses.iter().find(|p| p.humanoid == h).map(|p| p.root)));
        (finished, died)
    }

    /// Ends the walks of humanoids (whose roots `root_of` finds) that got
    /// there or ran out of time. Walks whose humanoid is gone end too.
    pub fn finish_walks(
        &mut self,
        tree: &InstanceTree,
        dt: f32,
        root_of: impl Fn(InstanceId) -> Option<InstanceId>,
    ) -> Vec<(InstanceId, bool)> {
        let mut done = Vec::new();
        for (&humanoid, walk) in self.walks.iter_mut() {
            let Some(root) = root_of(humanoid) else { continue };
            let Some(pos) = tree.get_vec3(root, Property::Position) else { continue };
            walk.elapsed += dt;
            if toward(pos, walk.target).1 <= ARRIVED {
                done.push((humanoid, true));
            } else if walk.elapsed >= MOVE_TIMEOUT {
                done.push((humanoid, false));
            }
        }
        for (humanoid, _) in &done {
            self.walks.remove(humanoid);
        }
        // Humanoids that were destroyed never finish.
        self.walks.retain(|h, _| tree.exists(*h) && !tree.is_destroyed(*h).unwrap_or(true));
        done
    }
}

fn in_tool(tree: &InstanceTree, part: InstanceId, model: InstanceId) -> bool {
    let mut current = tree.parent(part).ok().flatten();
    while let Some(id) = current {
        if id == model {
            return false;
        }
        if tree.class(id) == Ok(ClassKind::Tool) {
            return true;
        }
        current = tree.parent(id).ok().flatten();
    }
    false
}
