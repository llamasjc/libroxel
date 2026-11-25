//! Physics: loose parts, characters, and touches.
//!
//! Loose (unanchored) parts are real rigid bodies (see [`crate::rigid`]):
//! they fall, tumble, roll, stack and knock each other over whichever way
//! they are turned, and a model welded together moves as one solid thing.
//!
//! Characters move the way players expect instead: their root part stays
//! upright, walks up small ledges and ramps, and stops dead against walls.
//! Anchored parts can be rotated and shaped: boxes and parts turned in
//! right angles are handled as axis-aligned boxes (exact and cheap), while
//! tilted boxes and wedges use a separating-axis test and balls use a
//! sphere test, so ramps and round obstacles work. Characters shove the
//! loose parts they walk into. Every frame we also work out which pairs of
//! parts started touching, for the `Touched` event.

use std::collections::{HashMap, HashSet};

use bevy::platform::collections::HashMap as FastMap;

use bevy::math::{EulerRot, Mat3, Quat, Vec3 as GVec3};
use libroxel_instance::{
    ClassKind, EnumValue, InstanceId, InstanceTree, PartShape, Property, PropertyValue, Vec3,
};
use libroxel_script::names;

use crate::rigid;
pub use crate::rigid::{JointKind, JointSpec};

/// Downward acceleration, in studs per second squared.
pub const GRAVITY: f32 = 196.2;
/// Unanchored parts that fall below this height are destroyed.
pub const FALLEN_PARTS_HEIGHT: f32 = -100.0;

/// The height below which loose parts are destroyed: the workspace's
/// FallenPartsDestroyHeight, or [`FALLEN_PARTS_HEIGHT`].
pub fn fallen_parts_height(tree: &InstanceTree) -> f32 {
    tree.get_number(tree.workspace(), Property::FallenPartsDestroyHeight)
        .map_or(FALLEN_PARTS_HEIGHT, |h| h as f32)
}
/// The longest slice of time simulated in one go; longer frames are split.
const MAX_SUBSTEP: f32 = 1.0 / 60.0;
const MAX_SUBSTEPS: usize = 8;
/// How far apart two boxes may be and still count as touching.
const TOUCH_MARGIN: f32 = 0.05;
const EPS: f32 = 1e-4;
/// Worlds with at most this many parts skip the grid and compare every pair.
const BROADPHASE_MIN: usize = 64;
/// The size of a broadphase grid cell, in studs.
const CELL: f32 = 8.0;
/// Below this many parts, every part is simulated.
const NEAR_MOVERS_MIN: usize = 2000;
/// Anchored parts within about this far (in studs) of a moving part take part in a step.
const MOVER_MARGIN: f32 = 8.0;
/// Boxes covering more cells than this are compared with everything.
const MAX_CELLS: i64 = 64;
/// The size of the broadphase's coarse cells (for big boxes), in studs.
const COARSE_CELL: f32 = 64.0;
/// Moving boxes climb onto anything whose top is at most this far above their bottom.
pub const STEP_HEIGHT: f32 = 0.6;
/// Terrain is made of one-stud steps, so moving things climb those.
pub const TERRAIN_STEP: f32 = 1.05;
/// How much bigger, sideways, a character is to the loose parts it shoves
/// than to the walls that stop it, so walking into a crate pushes it.
const SHOVE_REACH: f32 = 0.15;

/// Turns an orientation in degrees (Y, then X, then Z) into a rotation.
/// How far a humanoid's legs reach below its root part: Roblox's
/// `HipHeight` for an R15 rig, else the lowest of the rig's other parts.
fn hip_height(tree: &InstanceTree, root: InstanceId) -> f32 {
    let Some(model) = tree.parent(root).ok().flatten() else { return 0.0 };
    let Some(humanoid) = tree.find_first_child_of_class(model, ClassKind::Humanoid).ok().flatten() else {
        return 0.0;
    };
    let number = |name: &str| match tree.get_extra(humanoid, name) {
        Ok(Some(PropertyValue::Number(n))) => Some(n as f32),
        _ => None,
    };
    let (Some(pos), Some(size)) =
        (tree.get_vec3(root, Property::Position), tree.get_vec3(root, Property::Size))
    else {
        return 0.0;
    };
    let bottom = pos.y - size.y * 0.5;
    if number("RigType") == Some(1.0)
        && let Some(hip) = number("HipHeight").filter(|h| *h > 0.0)
    {
        return hip.min(64.0);
    }
    let mut lowest = bottom;
    for &part in tree.children(model).unwrap_or_default() {
        if part == root || !tree.class(part).is_ok_and(|c| c.is_physical()) {
            continue;
        }
        let (Some(p), Some(s)) =
            (tree.get_vec3(part, Property::Position), tree.get_vec3(part, Property::Size))
        else {
            continue;
        };
        let rot =
            Mat3::from_quat(orientation_quat(tree.get_vec3(part, Property::Orientation).unwrap_or_default()));
        let reach = rot.row(1).abs().dot(GVec3::new(s.x, s.y, s.z)) * 0.5;
        lowest = lowest.min(p.y - reach);
    }
    (bottom - lowest).clamp(0.0, 64.0)
}

pub fn orientation_quat(degrees: Vec3) -> Quat {
    Quat::from_euler(EulerRot::YXZ, degrees.y.to_radians(), degrees.x.to_radians(), degrees.z.to_radians())
}

/// The orientation in degrees (Y, then X, then Z) of a rotation.
pub fn degrees_of(q: Quat) -> Vec3 {
    let (y, x, z) = q.to_euler(EulerRot::YXZ);
    Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
}

fn g(v: Vec3) -> GVec3 {
    GVec3::new(v.x, v.y, v.z)
}

fn l(v: GVec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// The exact shape a body collides with. `Body::half` is always its
/// axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Collider {
    /// Exactly its bounding box.
    Aabb,
    /// A box or wedge with any rotation. `half` is in the part's own axes.
    Hull {
        rotation: Mat3,
        half: GVec3,
        wedge: bool,
    },
    Sphere {
        radius: f32,
    },
    /// One convex piece of a part with a model (see
    /// [`crate::collision_shapes`]); `centre` is the part's middle.
    Convex {
        shape: u32,
        piece: u32,
        centre: GVec3,
        rotation: Mat3,
        size: GVec3,
    },
}

#[derive(Debug, Clone, Copy)]
struct Body {
    id: InstanceId,
    pos: Vec3,
    half: Vec3,
    anchored: bool,
    /// Placed from outside this step: it doesn't move, but still touches.
    held: bool,
    can_collide: bool,
    collider: Collider,
    /// How high a ledge on this body moving things climb onto without stopping.
    climb: f32,
    /// In water this step: `Some(true)` floats up, `Some(false)` hangs
    /// (characters, who swim by themselves).
    water: Option<bool>,
    /// A loose part the rigid-body simulation moves (characters don't
    /// move it themselves; it is in their way like a wall, though they shove it).
    sim: bool,
    /// For a humanoid's root that doesn't collide itself (as in most Roblox
    /// rigs): how far its legs reach below it. The body is stretched down by
    /// this much so the humanoid stands on its feet, like Roblox's hip height.
    hip: f32,
}

impl Body {
    fn min(&self) -> Vec3 {
        self.pos - self.half
    }
    fn max(&self) -> Vec3 {
        self.pos + self.half
    }
}

/// A convex shape as points plus the directions a separating axis can come from.
struct Hull {
    points: Vec<GVec3>,
    normals: Vec<GVec3>,
    edges: Vec<GVec3>,
}

impl Body {
    fn hull(&self) -> Hull {
        let c = g(self.pos);
        match self.collider {
            Collider::Hull { rotation, half, wedge } => {
                let corner = |x: f32, y: f32, z: f32| c + rotation * (half * GVec3::new(x, y, z));
                let mut points = Vec::with_capacity(8);
                for x in [-1.0, 1.0] {
                    for z in [-1.0, 1.0] {
                        points.push(corner(x, -1.0, z));
                        // A wedge only keeps its top edge at the back.
                        if !wedge || z > 0.0 {
                            points.push(corner(x, 1.0, z));
                        }
                    }
                }
                let mut normals = vec![rotation.x_axis, rotation.y_axis, rotation.z_axis];
                let mut edges = normals.clone();
                if wedge {
                    normals.push((rotation * GVec3::new(0.0, half.z, -half.y)).normalize_or_zero());
                    edges.push((rotation * GVec3::new(0.0, half.y, half.z)).normalize_or_zero());
                }
                Hull { points, normals, edges }
            }
            Collider::Convex { shape, piece, centre, rotation, size } => {
                match crate::collision_shapes::shape(shape) {
                    Some(found) => {
                        let (points, normals, edges) =
                            crate::collision_shapes::placed(found.get(piece), centre, rotation, size);
                        Hull { points, normals, edges }
                    }
                    None => Hull { points: vec![c], normals: vec![GVec3::Y], edges: vec![GVec3::X] },
                }
            }
            _ => {
                let h = g(self.half);
                let mut points = Vec::with_capacity(8);
                for x in [-1.0, 1.0] {
                    for y in [-1.0, 1.0] {
                        for z in [-1.0, 1.0] {
                            points.push(c + h * GVec3::new(x, y, z));
                        }
                    }
                }
                Hull {
                    points,
                    normals: vec![GVec3::X, GVec3::Y, GVec3::Z],
                    edges: vec![GVec3::X, GVec3::Y, GVec3::Z],
                }
            }
        }
    }
}

fn project(points: &[GVec3], axis: GVec3) -> (f32, f32) {
    points.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
        let d = p.dot(axis);
        (lo.min(d), hi.max(d))
    })
}

/// How to push `mover` (always a box) out of `obstacle`: the direction,
/// pointing away from the obstacle, and the depth. `None` if they are more
/// than `margin` apart.
fn penetration(mover: &Body, obstacle: &Body, margin: f32) -> Option<(GVec3, f32)> {
    if let Collider::Sphere { radius } = obstacle.collider {
        let centre = g(obstacle.pos);
        let (lo, hi) = (g(mover.min()), g(mover.max()));
        let closest = centre.clamp(lo, hi);
        let offset = closest - centre;
        let dist = offset.length();
        if dist > radius + margin {
            return None;
        }
        if dist > 1e-5 {
            return Some((offset / dist, radius - dist));
        }
        // The sphere's centre is inside the box: push out the shortest way.
        let mut best = (GVec3::Y, f32::INFINITY);
        for axis in [GVec3::X, GVec3::Y, GVec3::Z] {
            for sign in [-1.0, 1.0] {
                let n = axis * sign;
                let depth = -(g(mover.pos) - centre).dot(n) + g(mover.half).dot(axis) + radius;
                if depth < best.1 {
                    best = (n, depth);
                }
            }
        }
        return Some(best);
    }

    let (a, b) = (mover.hull(), obstacle.hull());
    let mut axes: Vec<GVec3> = a.normals.iter().chain(&b.normals).copied().collect();
    for ea in &a.edges {
        for eb in &b.edges {
            let axis = ea.cross(*eb);
            if axis.length_squared() > 1e-6 {
                axes.push(axis.normalize());
            }
        }
    }
    let between = g(mover.pos) - g(obstacle.pos);
    let mut best: Option<(GVec3, f32)> = None;
    for axis in axes {
        let (alo, ahi) = project(&a.points, axis);
        let (blo, bhi) = project(&b.points, axis);
        let overlap = ahi.min(bhi) - alo.max(blo);
        if overlap < -margin {
            return None;
        }
        if best.is_none_or(|(_, d)| overlap < d) {
            let n = if between.dot(axis) < 0.0 { -axis } else { axis };
            best = Some((n, overlap));
        }
    }
    best
}

fn touching(a: &Body, b: &Body) -> bool {
    if !overlaps(a, b, TOUCH_MARGIN) {
        return false;
    }
    match (a.collider, b.collider) {
        (Collider::Aabb, Collider::Aabb) => true,
        (Collider::Aabb, _) => penetration(a, b, TOUCH_MARGIN).is_some(),
        (_, Collider::Aabb) => penetration(b, a, TOUCH_MARGIN).is_some(),
        // Two anchored shapes never get here; touches need something that moves.
        _ => true,
    }
}

fn overlaps(a: &Body, b: &Body, margin: f32) -> bool {
    let (amin, amax, bmin, bmax) = (a.min(), a.max(), b.min(), b.max());
    amin.x < bmax.x + margin
        && amax.x > bmin.x - margin
        && amin.y < bmax.y + margin
        && amax.y > bmin.y - margin
        && amin.z < bmax.z + margin
        && amax.z > bmin.z - margin
}

fn axis(v: Vec3, i: usize) -> f32 {
    [v.x, v.y, v.z][i]
}

fn set_axis(v: &mut Vec3, i: usize, value: f32) {
    match i {
        0 => v.x = value,
        1 => v.y = value,
        _ => v.z = value,
    }
}

/// The bodies a part with a model collides as: one per convex piece if it
/// stays put, or one hull round it all if it moves.
fn mesh_bodies(
    pos: Vec3,
    size: Vec3,
    orientation: Vec3,
    shape: u32,
    whole: bool,
) -> Vec<(Vec3, Vec3, Collider)> {
    let Some(found) = crate::collision_shapes::shape(shape) else { return Vec::new() };
    let rotation = Mat3::from_quat(orientation_quat(orientation));
    let (centre, size) = (g(pos), g(size));
    let pieces: Vec<u32> =
        if whole { vec![crate::collision_shapes::WHOLE] } else { (0..found.pieces.len() as u32).collect() };
    pieces
        .into_iter()
        .filter_map(|piece| {
            let (points, ..) = crate::collision_shapes::placed(found.get(piece), centre, rotation, size);
            let lo = points.iter().copied().reduce(GVec3::min)?;
            let hi = points.iter().copied().reduce(GVec3::max)?;
            let collider = Collider::Convex { shape, piece, centre, rotation, size };
            Some((l((lo + hi) * 0.5), l((hi - lo) * 0.5), collider))
        })
        .collect()
}

/// The collision shape for a part's `MeshId`, if it has a model.
fn mesh_shape(tree: &InstanceTree, id: InstanceId) -> Option<u32> {
    let mesh = tree.get_string(id, Property::MeshId)?;
    if mesh.trim().is_empty() {
        return None;
    }
    crate::collision_shapes::for_mesh(&mesh)
}

/// Picks the cheapest exact collider for an anchored part.
fn collider_for(size: Vec3, shape: PartShape, orientation: Vec3) -> (Vec3, Collider) {
    let rotation = Mat3::from_quat(orientation_quat(orientation));
    let local_half = g(size * 0.5);
    // World bounding box of the rotated box.
    let abs = Mat3::from_cols(rotation.x_axis.abs(), rotation.y_axis.abs(), rotation.z_axis.abs());
    let bounds = l(abs * local_half);
    if shape == PartShape::Ball {
        let radius = size.x.min(size.y).min(size.z) * 0.5;
        return (Vec3::new(radius, radius, radius), Collider::Sphere { radius });
    }
    let wedge = shape == PartShape::Wedge;
    let right_angles = rotation.to_cols_array().iter().all(|v| {
        let a = v.abs();
        a < 1e-4 || (a - 1.0).abs() < 1e-4
    });
    // Cylinders collide as the box around them.
    if right_angles && !wedge {
        (bounds, Collider::Aabb)
    } else {
        (bounds, Collider::Hull { rotation, half: local_half, wedge })
    }
}

/// What happened during a step.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct StepResult {
    /// Pairs that started touching this step. Each pair appears once.
    pub touches: Vec<(InstanceId, InstanceId)>,
    /// Pairs that stopped touching this step.
    pub untouches: Vec<(InstanceId, InstanceId)>,
    /// Unanchored parts that fell out of the world (the caller destroys them).
    pub fell: Vec<InstanceId>,
}

/// Physics state that isn't stored in the tree, such as velocities.
#[derive(Debug, Default)]
pub struct Physics {
    velocities: HashMap<InstanceId, Vec3>,
    /// How fast moving parts spin, in radians per second about each world axis.
    spins: HashMap<InstanceId, Vec3>,
    grounded: HashSet<InstanceId>,
    /// Moving parts that were in water last step.
    wet: HashSet<InstanceId>,
    contacts: HashSet<(InstanceId, InstanceId)>,
    /// Parts moved by something else (a weld) that stand still in a step.
    carried: HashSet<InstanceId>,
    /// Each standing humanoid's hip height, measured when first seen (so
    /// swinging legs don't make it bob).
    hips: HashMap<InstanceId, f32>,
    /// Where the world's parts are, so a big map's far-off parts are skipped
    /// without looking at each of them every step.
    index: std::cell::RefCell<libroxel_instance::geometry::PartIndex>,
    /// Parts placed from outside for the next step (a character another
    /// computer moves): they stay put but still touch things.
    held: HashSet<InstanceId>,
    /// Pairs that pass through each other and never touch (parts of one assembly).
    ignored: HashSet<(InstanceId, InstanceId)>,
    /// Parts a spinner turns, or a vehicle seat drives, this step: they
    /// slide over what they rest on instead of being held back by it.
    driven: HashSet<InstanceId>,
    /// How carried parts move this step (a turning hinge arm), so they push
    /// what they run into: a point on the turning axis, the spin about it,
    /// and how fast that point moves.
    motions: HashMap<InstanceId, (GVec3, GVec3, GVec3)>,
    /// Parts welded to a root, and where on the root they sit (position
    /// and rotation in the root's frame).
    welded: HashMap<InstanceId, (InstanceId, (Vec3, Quat))>,
    /// Loose parts as rigid bodies.
    rigid: rigid::Rigid,
    /// Hinges, ropes and springs joining parts.
    joints: Vec<JointSpec>,
    /// Collision groups that pass through each other, and each part's group.
    groups: Groups,
    /// Where steps have spent their time, for the speed report.
    timing: PhysicsTiming,
    /// Loose parts that have stood still, and those asleep: like Roblox, a
    /// part at rest is left out of the simulation (and costs nothing) until
    /// something moves it or comes near it.
    rest: HashMap<InstanceId, Rest>,
    /// Which turn of the slow round of waking sleeping parts this step is.
    since_wake_all: u32,
    /// How far the tree's log of moved parts has been read (and its epoch),
    /// to wake sleeping parts near anything that moved or went away.
    moved_read: (u64, usize),
    /// The terrain's version last step.
    terrain_version: u64,
}

/// How still a loose part has been: where it last was, and for how many
/// steps it hasn't moved.
#[derive(Debug, Clone, Copy)]
struct Rest {
    pos: Vec3,
    orientation: Vec3,
    still: u32,
    /// Whether it was on something when last simulated.
    grounded: bool,
    /// The terrain round it when it fell asleep.
    terrain: u64,
}

/// Steps a part must stand still before it falls asleep.
const SLEEP_STEPS: u32 = 20;
/// How far a part may drift (studs, degrees) and still count as still.
const STILL_MOVE: f32 = 0.01;
const STILL_TURN: f32 = 0.2;
/// Every so many steps each sleeping part wakes for a moment, in case what
/// it rested on went away without moving (made CanCollide false).
const WAKE_ALL_STEPS: u32 = 600;

/// Where physics steps spent their time (seconds, summed over `steps`), and
/// how much they had to do: for the speed report.
#[derive(Debug, Default, Clone)]
pub struct PhysicsTiming {
    pub steps: u32,
    /// Finding the parts near anything that moves, and reading them.
    pub gather: f64,
    /// Finding which boxes could meet.
    pub broadphase: f64,
    /// Walking characters.
    pub characters: f64,
    /// Loose parts (the rigid body solver).
    pub loose: f64,
    /// Touched and TouchEnded.
    pub touches: f64,
    /// Bodies looked at, moving ones, and loose parts, summed over steps.
    pub bodies: u64,
    pub movers: u64,
}

/// Which collision groups pass through each other, and which group each
/// part is in (parts in the default group aren't listed).
#[derive(Debug, Default)]
pub(crate) struct Groups {
    /// Pairs of group numbers that don't collide (each pair in order), the
    /// numbers being places in `names`.
    apart: HashSet<(u32, u32)>,
    /// The groups named in those pairs.
    names: HashMap<String, u32>,
    /// The group number of each part in this step that is in one of them.
    of: HashMap<InstanceId, u32>,
}

impl Groups {
    /// Sets which pairs of group names pass through each other.
    fn set(&mut self, pairs: impl IntoIterator<Item = (String, String)>) {
        self.names.clear();
        self.apart.clear();
        for (a, b) in pairs {
            let next = self.names.len() as u32;
            let a = *self.names.entry(a).or_insert(next);
            let next = self.names.len() as u32;
            let b = *self.names.entry(b).or_insert(next);
            self.apart.insert((a.min(b), a.max(b)));
        }
    }

    /// Whether `a` and `b` pass through each other for being in groups
    /// that don't collide.
    pub(crate) fn apart(&self, a: InstanceId, b: InstanceId) -> bool {
        if self.apart.is_empty() {
            return false;
        }
        let default = self.names.get(libroxel_instance::DEFAULT_COLLISION_GROUP).copied();
        let (Some(ga), Some(gb)) =
            (self.of.get(&a).copied().or(default), self.of.get(&b).copied().or(default))
        else {
            return false;
        };
        self.apart.contains(&(ga.min(gb), ga.max(gb)))
    }

    /// Notes the group of each of `parts` (the ones in this step).
    fn refresh(&mut self, tree: &InstanceTree, parts: impl Iterator<Item = InstanceId>) {
        self.of.clear();
        if self.apart.is_empty() {
            return;
        }
        let default = libroxel_instance::DEFAULT_COLLISION_GROUP;
        for part in parts {
            if let Some(g) = tree.get_str(part, Property::CollisionGroup)
                && g != default
            {
                // A group no rule names collides with everything.
                let n = self.names.get(g).copied().unwrap_or(u32::MAX);
                self.of.insert(part, n);
            }
        }
    }
}

fn ordered(a: InstanceId, b: InstanceId) -> (InstanceId, InstanceId) {
    if a <= b { (a, b) } else { (b, a) }
}

impl Physics {
    /// Where steps spent their time since the last call.
    pub fn take_timing(&mut self) -> PhysicsTiming {
        std::mem::take(&mut self.timing)
    }

    pub fn velocity(&self, id: InstanceId) -> Vec3 {
        self.velocities.get(&id).copied().unwrap_or_default()
    }

    pub fn set_velocity(&mut self, id: InstanceId, velocity: Vec3) {
        if velocity.length() > 0.05 {
            self.wake(id);
        }
        self.velocities.insert(id, velocity);
    }

    /// How fast `id` spins, in radians per second about each world axis.
    pub fn angular_velocity(&self, id: InstanceId) -> Vec3 {
        self.spins.get(&id).copied().unwrap_or_default()
    }

    pub fn set_angular_velocity(&mut self, id: InstanceId, spin: Vec3) {
        if spin.length() > 0.05 {
            self.wake(id);
        }
        if spin == Vec3::ZERO {
            self.spins.remove(&id);
        } else {
            self.spins.insert(id, spin);
        }
    }

    /// Spins `id` on behalf of something that keeps turning it (a spinner),
    /// so for the next step it turns freely over what it rests on.
    pub fn drive_spin(&mut self, id: InstanceId, spin: Vec3) {
        self.set_angular_velocity(id, spin);
        self.driven.insert(id);
    }

    /// Lets `id` slide freely over what it rests on for the next step,
    /// because something else steers it (a vehicle seat).
    pub fn glide(&mut self, id: InstanceId) {
        self.driven.insert(id);
    }

    /// Every spinning part's angular velocity.
    pub fn angular_velocities(&self) -> &HashMap<InstanceId, Vec3> {
        &self.spins
    }

    /// Every moving part's velocity.
    pub fn velocities(&self) -> &HashMap<InstanceId, Vec3> {
        &self.velocities
    }

    /// Sets which parts are carried by welds (they act as anchored during a
    /// step) and which pairs of parts belong together and ignore each other.
    pub fn set_assemblies(
        &mut self,
        carried: HashSet<InstanceId>,
        together: impl IntoIterator<Item = (InstanceId, InstanceId)>,
    ) {
        for id in &carried {
            self.velocities.remove(id);
            self.spins.remove(id);
        }
        self.carried = carried;
        self.ignored = together.into_iter().map(|(a, b)| ordered(a, b)).collect();
    }

    /// Says which parts are welded to which roots, and where on the root
    /// each sits, so a loose welded model moves as one solid body.
    pub fn set_welds(&mut self, welded: HashMap<InstanceId, (InstanceId, (Vec3, Quat))>) {
        self.welded = welded;
    }

    /// Sets the joints (hinges, ropes, springs) between parts.
    pub fn set_joints(&mut self, joints: Vec<JointSpec>) {
        self.joints = joints;
    }

    /// Lets `a` and `b` pass through each other for the next step.
    pub fn pass_through(&mut self, a: InstanceId, b: InstanceId) {
        if a != b {
            self.ignored.insert(ordered(a, b));
        }
    }

    /// Also holds `id` still for the next step, passing through `together`.
    pub fn carry(&mut self, id: InstanceId, together: &[InstanceId]) {
        self.velocities.remove(&id);
        self.spins.remove(&id);
        self.carried.insert(id);
        for &other in together {
            self.ignored.insert(ordered(id, other));
        }
    }

    /// Says how a carried part moves during the next step (it turns at
    /// `spin` about an axis through `pivot`, which moves at `velocity`), so
    /// loose things it runs into are shoved along with it.
    pub fn set_motion(&mut self, id: InstanceId, pivot: Vec3, spin: Vec3, velocity: Vec3) {
        self.motions.insert(id, (g(pivot), g(spin), g(velocity)));
    }

    /// Whether `id` is held still by something else this step (a weld or a hinge).
    pub fn is_carried(&self, id: InstanceId) -> bool {
        self.carried.contains(&id)
    }

    fn ignores(&self, a: InstanceId, b: InstanceId) -> bool {
        (!self.ignored.is_empty() && self.ignored.contains(&ordered(a, b))) || self.groups.apart(a, b)
    }

    /// Sets which collision groups pass through each other: pairs of group
    /// names (in any order).
    pub fn set_collision_rules(&mut self, apart: impl IntoIterator<Item = (String, String)>) {
        self.groups.set(apart);
    }

    /// Keeps `id` where it is for the next step (something else moved it
    /// there), while it still touches what it overlaps.
    pub fn hold(&mut self, id: InstanceId) {
        self.wake(id);
        self.held.insert(id);
    }

    /// Whether `id` was in terrain water during the last step.
    pub fn is_in_water(&self, id: InstanceId) -> bool {
        self.wet.contains(&id)
    }

    /// Whether `id` ended the last step resting on something.
    pub fn is_grounded(&self, id: InstanceId) -> bool {
        self.grounded.contains(&id) || (self.asleep(id) && self.rest.get(&id).is_some_and(|r| r.grounded))
    }

    fn collect_bodies(
        &self,
        tree: &InstanceTree,
        parts: &[InstanceId],
        dt: f32,
        gravity: f32,
        everything: bool,
    ) -> Vec<Body> {
        let mut bodies = self.part_bodies(tree, parts);
        // Loose things in terrain water float (or, for characters, hang there).
        if let Some(voxels) = tree.terrain().and_then(|t| tree.voxels(t).ok()).filter(|v| !v.is_empty()) {
            for body in bodies.iter_mut().filter(|b| !b.anchored && !b.held) {
                if voxels.water_at(body.pos) {
                    body.water = Some(tree.name(body.id) != Ok(names::character::ROOT_PART));
                }
            }
        }
        self.add_terrain(tree, &mut bodies, dt, gravity, everything);
        bodies
    }

    /// Terrain cells near anything that moves, as boxes (runs of cells merged).
    /// Only near characters, unless `everything` (for touches).
    fn add_terrain(
        &self,
        tree: &InstanceTree,
        bodies: &mut Vec<Body>,
        dt: f32,
        gravity: f32,
        everything: bool,
    ) {
        let Some(terrain) = tree.terrain() else { return };
        let Ok(voxels) = tree.voxels(terrain) else { return };
        if voxels.is_empty() {
            return;
        }
        let mut seen = HashSet::new();
        let mut found = Vec::new();
        for body in bodies.iter().filter(|b| !b.anchored && (everything || !b.sim)) {
            let v = self.velocity(body.id);
            let reach = (v.length() + gravity.abs() * dt) * dt + TERRAIN_STEP + 1.0;
            let r = Vec3::new(reach, reach, reach);
            for (lo, hi) in voxels.boxes_in(body.min() - r, body.max() + r) {
                let key = (lo.x.to_bits(), lo.y.to_bits(), lo.z.to_bits(), hi.x.to_bits());
                if seen.insert(key) {
                    found.push(Body {
                        id: terrain,
                        pos: (lo + hi) * 0.5,
                        half: (hi - lo) * 0.5,
                        anchored: true,
                        held: false,
                        can_collide: true,
                        collider: Collider::Aabb,
                        climb: TERRAIN_STEP,
                        water: None,
                        sim: false,
                        hip: 0.0,
                    });
                }
            }
        }
        bodies.extend(found);
    }

    /// The parts worth simulating: everything that moves, and the anchored
    /// parts near something that moves. A big map's thousands of anchored
    /// parts far from every mover can't touch anything this step.
    /// Whether `id` is a loose part asleep (at rest, left out of steps).
    fn asleep(&self, id: InstanceId) -> bool {
        // A welded group sleeps and wakes as one, as its root does.
        let root = self.welded.get(&id).map_or(id, |(r, _)| *r);
        self.rest.get(&root).is_some_and(|r| r.still >= SLEEP_STEPS)
    }

    /// Wakes `id`'s welded group (something pushed it).
    fn wake(&mut self, id: InstanceId) {
        let root = self.welded.get(&id).map_or(id, |(r, _)| *r);
        if let Some(r) = self.rest.get_mut(&root) {
            r.still = 0;
        }
    }

    /// A hash of the terrain under and round `pos` (a part `r` studs
    /// across), to tell when it is dug out from under a sleeping part.
    fn terrain_print(voxels: Option<&libroxel_instance::voxels::Voxels>, pos: Vec3, r: f32) -> u64 {
        use libroxel_instance::voxels::{CHUNK, cell_at};
        let Some(voxels) = voxels.filter(|v| !v.is_empty()) else { return 0 };
        let (lo, hi) = (cell_at(pos - Vec3::new(r, r + 1.0, r)), cell_at(pos + Vec3::new(r, r, r)));
        let mut print = 0u64;
        for x in lo.0.div_euclid(CHUNK)..=hi.0.div_euclid(CHUNK) {
            for y in lo.1.div_euclid(CHUNK)..=hi.1.div_euclid(CHUNK) {
                for z in lo.2.div_euclid(CHUNK)..=hi.2.div_euclid(CHUNK) {
                    print = print.rotate_left(7) ^ voxels.chunk_fingerprint((x, y, z));
                }
            }
        }
        print
    }

    /// Wakes sleeping parts that something else moved or pushed (a script
    /// set where they are or how fast they go), and now and then all of
    /// them.
    fn wake_moved(&mut self, tree: &InstanceTree) {
        // A few each step, in turn, so they don't all wake at once.
        self.since_wake_all = (self.since_wake_all + 1) % WAKE_ALL_STEPS;
        let turn = self.since_wake_all;
        // Near something that moved, turned up or went away (a platform
        // sliding under it, the floor it lay on destroyed): awake.
        let (epoch, moved) = tree.moved_since(self.moved_read.1);
        let mut near: HashSet<InstanceId> = HashSet::new();
        let mut all = false;
        if epoch != self.moved_read.0 || moved.len() > 4096 {
            all = true;
        } else if self.rest.values().any(|r| r.still >= SLEEP_STEPS) {
            // The sleeping parts by 16-stud cell, so each moved part looks
            // only at the few sleeping near it.
            const CELL: f32 = 16.0;
            let cell = |v: Vec3| {
                ((v.x / CELL).floor() as i32, (v.y / CELL).floor() as i32, (v.z / CELL).floor() as i32)
            };
            let bounds = |id: InstanceId, grow: f32| {
                let pos = tree.get_vec3(id, Property::Position)?;
                let r = tree.get_vec3(id, Property::Size)?.length() * 0.5 + grow;
                Some((pos - Vec3::new(r, r, r), pos + Vec3::new(r, r, r)))
            };
            type Sleeper = (InstanceId, Vec3, Vec3);
            let mut sleeping: HashMap<(i32, i32, i32), Vec<Sleeper>> = HashMap::new();
            for (id, _) in self.rest.iter().filter(|(_, r)| r.still >= SLEEP_STEPS) {
                let Some((lo, hi)) = bounds(*id, 0.0) else { continue };
                let (a, b) = (cell(lo), cell(hi));
                for x in a.0..=b.0 {
                    for y in a.1..=b.1 {
                        for z in a.2..=b.2 {
                            sleeping.entry((x, y, z)).or_default().push((*id, lo, hi));
                        }
                    }
                }
            }
            // (A loose part settling, moved a hair by the solver, wakes nothing.)
            let settling = |id: InstanceId| {
                self.rest.contains_key(&id)
                    && self.velocity(id).length() < 0.1
                    && self.spins.get(&id).is_none_or(|v| v.length() < 0.1)
            };
            let mut seen = HashSet::new();
            for id in moved {
                if !seen.insert(*id) || settling(*id) {
                    continue;
                }
                let Some((lo, hi)) = bounds(*id, 1.0) else { continue };
                let (a, b) = (cell(lo), cell(hi));
                let meets = |slo: &Vec3, shi: &Vec3| {
                    slo.x <= hi.x
                        && lo.x <= shi.x
                        && slo.y <= hi.y
                        && lo.y <= shi.y
                        && slo.z <= hi.z
                        && lo.z <= shi.z
                };
                if (b.0 - a.0 + 1) * (b.1 - a.1 + 1) * (b.2 - a.2 + 1) > 512 {
                    // Something huge moved or went: each sleeping part is looked at.
                    for list in sleeping.values() {
                        near.extend(list.iter().filter(|(_, slo, shi)| meets(slo, shi)).map(|(s, ..)| *s));
                    }
                    continue;
                }
                for x in a.0..=b.0 {
                    for y in a.1..=b.1 {
                        for z in a.2..=b.2 {
                            for (s, slo, shi) in sleeping.get(&(x, y, z)).into_iter().flatten() {
                                if meets(slo, shi) {
                                    near.insert(*s);
                                }
                            }
                        }
                    }
                }
            }
        }
        self.moved_read = (tree.moved_epoch(), tree.moved_len());
        // The terrain changed: those it was dug out from under wake.
        let voxels = tree.terrain().and_then(|t| tree.voxels(t).ok());
        let version = voxels.map_or(0, |v| v.version());
        let terrain_changed = version != self.terrain_version;
        self.terrain_version = version;
        let (velocities, spins, held) = (&self.velocities, &self.spins, &self.held);
        let jointed: HashSet<InstanceId> = self.joints.iter().flat_map(|j| [j.part0, j.part1]).collect();
        let mut woken = Vec::new();
        self.rest.retain(|id, r| {
            if r.still < SLEEP_STEPS {
                return tree.exists(*id) && !tree.is_destroyed(*id).unwrap_or(true);
            }
            let (Some(pos), Some(orientation)) =
                (tree.get_vec3(*id, Property::Position), tree.get_vec3(*id, Property::Orientation))
            else {
                return false;
            };
            let pushed = velocities.get(id).is_some_and(|v| v.length() > 0.05)
                || spins.get(id).is_some_and(|v| v.length() > 0.05);
            if all
                || (id.to_bits() % u64::from(WAKE_ALL_STEPS)) as u32 == turn
                || near.contains(id)
                || (terrain_changed && {
                    let size = tree.get_vec3(*id, Property::Size).unwrap_or_default();
                    Self::terrain_print(voxels, pos, size.length() * 0.5) != r.terrain
                })
                || pushed
                || held.contains(id)
                || jointed.contains(id)
                || tree.get_bool(*id, Property::Anchored) != Some(false)
                || (pos - r.pos).length() > STILL_MOVE
                || (orientation - r.orientation).length() > STILL_TURN
            {
                r.still = 0;
                r.pos = pos;
                r.orientation = orientation;
                woken.push(*id);
            }
            true
        });
        // (A part of a welded group wakes the whole group.)
        for id in woken {
            self.wake(id);
        }
    }

    /// Wakes sleeping parts that something moving is about to run into, so
    /// they get pushed instead of standing like a wall.
    fn wake_in_the_way(&mut self, tree: &InstanceTree, parts: &[InstanceId]) {
        let sleepers: Vec<InstanceId> = parts.iter().copied().filter(|id| self.asleep(*id)).collect();
        if sleepers.is_empty() {
            return;
        }
        let bounds = |id: InstanceId, grow: f32| {
            let pos = tree.get_vec3(id, Property::Position)?;
            let r = tree.get_vec3(id, Property::Size)?.length() * 0.5 + grow;
            Some((pos - Vec3::new(r, r, r), pos + Vec3::new(r, r, r)))
        };
        let fast: Vec<(Vec3, Vec3)> = parts
            .iter()
            .copied()
            .filter(|id| !self.asleep(*id) && tree.get_bool(*id, Property::Anchored) == Some(false))
            .filter_map(|id| {
                let root = self.welded.get(&id).map_or(id, |(r, _)| *r);
                let speed = self.velocity(root).length();
                (speed > 0.3 || self.held.contains(&id)).then(|| bounds(id, speed * 0.1 + 0.5)).flatten()
            })
            .collect();
        for id in sleepers {
            let Some((lo, hi)) = bounds(id, 0.0) else { continue };
            if fast.iter().any(|(a, b)| {
                lo.x <= b.x && a.x <= hi.x && lo.y <= b.y && a.y <= hi.y && lo.z <= b.z && a.z <= hi.z
            }) {
                self.wake(id);
            }
        }
    }

    /// Notes which loose parts moved this step: those that didn't for long
    /// enough fall asleep.
    fn note_rest(&mut self, tree: &InstanceTree, bodies: &[Body]) {
        let voxels = tree.terrain().and_then(|t| tree.voxels(t).ok());
        // (Welded groups stay awake for now: their parts are put back on
        // their root after each step, which a sleeping root would upset.)
        let mut roots: HashSet<InstanceId> = self.welded.values().map(|(r, _)| *r).collect();
        // Nor do parts joined by hinges, ropes or springs.
        roots.extend(self.joints.iter().flat_map(|j| [j.part0, j.part1]));
        let mut done = HashSet::new();
        for body in bodies
            .iter()
            .filter(|b| b.sim && !b.held && !roots.contains(&b.id) && !self.welded.contains_key(&b.id))
        {
            if !done.insert(body.id) {
                continue;
            }
            let (Some(pos), Some(orientation)) =
                (tree.get_vec3(body.id, Property::Position), tree.get_vec3(body.id, Property::Orientation))
            else {
                continue;
            };
            let fast = self.velocity(body.id).length() > 0.1
                || self.spins.get(&body.id).is_some_and(|v| v.length() > 0.1);
            let grounded = self.grounded.contains(&body.id);
            let r =
                self.rest.entry(body.id).or_insert(Rest { pos, orientation, still: 0, grounded, terrain: 0 });
            r.grounded = grounded;
            if fast
                || (pos - r.pos).length() > STILL_MOVE
                || (orientation - r.orientation).length() > STILL_TURN
            {
                *r = Rest { pos, orientation, still: 0, grounded, terrain: 0 };
            } else {
                r.still = (r.still + 1).min(SLEEP_STEPS);
                if r.still == SLEEP_STEPS {
                    let size = tree.get_vec3(body.id, Property::Size).unwrap_or_default();
                    r.terrain = Self::terrain_print(voxels, pos, size.length() * 0.5);
                }
            }
        }
    }

    fn parts_near_movers(&self, tree: &InstanceTree) -> Vec<InstanceId> {
        let mut index = self.index.borrow_mut();
        index.refresh(tree);
        if index.len() < NEAR_MOVERS_MIN {
            return tree.descendants_of_class(tree.workspace(), ClassKind::Part);
        }
        let reach = |id: InstanceId| tree.get_vec3(id, Property::Size).map_or(0.0, |s| s.length() * 0.5);
        let movers: Vec<InstanceId> = index
            .loose()
            .filter(|id| !self.carried.contains(id) && !self.asleep(*id))
            .chain(self.held.iter().copied())
            .collect();
        // Each mover's box, grown by how far it could get in a slow frame
        // and a margin; the index looks every cell they cover up once.
        let boxes: Vec<(Vec3, Vec3)> = movers
            .iter()
            .filter_map(|id| {
                let pos = tree.get_vec3(*id, Property::Position)?;
                let r = reach(*id) + self.velocity(*id).length() * 0.1 + MOVER_MARGIN;
                Some((pos - Vec3::new(r, r, r), pos + Vec3::new(r, r, r)))
            })
            .collect();
        let mut out: HashSet<InstanceId> = index.near_boxes(&boxes).into_iter().collect();
        out.extend(movers.iter().copied());
        out.extend(self.held.iter().copied());
        // In tree order, so steps don't depend on hashing.
        let mut out: Vec<InstanceId> = out.into_iter().collect();
        out.sort_by_key(|id| id.to_bits());
        out
    }

    fn part_bodies(&self, tree: &InstanceTree, parts: &[InstanceId]) -> Vec<Body> {
        parts
            .iter()
            .copied()
            .flat_map(|id| -> Vec<Body> {
                let one = |b: Option<Body>| b.into_iter().collect::<Vec<_>>();
                let Some(size) = tree.get_vec3(id, Property::Size) else { return Vec::new() };
                let Some(anchored) = tree.get_bool(id, Property::Anchored) else { return Vec::new() };
                // A sleeping part stands still like an anchored one until woken.
                let anchored = anchored || self.carried.contains(&id) || self.asleep(id);
                let shape = match tree.get_enum(id, Property::Shape) {
                    Some(EnumValue::Shape(shape)) => shape,
                    _ => PartShape::Block,
                };
                let orientation = tree.get_vec3(id, Property::Orientation).unwrap_or_default();
                let held = self.held.contains(&id);
                let character = tree.name(id) == Ok(names::character::ROOT_PART);
                let sim = !anchored && !held && !character;
                let Some(pos) = tree.get_vec3(id, Property::Position) else { return Vec::new() };
                let Some(can_collide) = tree.get_bool(id, Property::CanCollide) else { return Vec::new() };
                if !character && let Some(mesh) = mesh_shape(tree, id) {
                    let pieces = mesh_bodies(pos, size, orientation, mesh, sim || held);
                    if !pieces.is_empty() {
                        return pieces
                            .into_iter()
                            .map(|(pos, half, collider)| Body {
                                id,
                                pos,
                                half,
                                anchored,
                                held,
                                can_collide,
                                collider,
                                climb: STEP_HEIGHT,
                                water: None,
                                sim,
                                hip: 0.0,
                            })
                            .collect();
                    }
                }
                let (mut half, collider) = if character && !anchored {
                    // Characters stay upright, whichever way they face.
                    (size * 0.5, Collider::Aabb)
                } else {
                    collider_for(size, shape, orientation)
                };
                // A humanoid stands on its legs even when its root doesn't
                // collide (Roblox floats it at its hip height).
                let hip = if character && !anchored && !can_collide && !held {
                    self.hips.get(&id).copied().unwrap_or_else(|| hip_height(tree, id))
                } else {
                    0.0
                };
                let mut pos = pos;
                if hip > 0.0 {
                    pos.y -= hip * 0.5;
                    half.y += hip * 0.5;
                }
                one(Some(Body {
                    id,
                    pos,
                    half,
                    anchored,
                    held,
                    can_collide: can_collide || hip > 0.0,
                    collider,
                    climb: STEP_HEIGHT,
                    water: None,
                    sim,
                    hip,
                }))
            })
            .collect()
    }

    /// What each part is to the rigid-body simulation this step.
    fn specs(&self, tree: &InstanceTree, bodies: &[Body]) -> Vec<rigid::Spec> {
        let dynamic: HashSet<InstanceId> =
            bodies.iter().filter(|b| b.sim && !self.carried.contains(&b.id)).map(|b| b.id).collect();
        let mut specs = Vec::with_capacity(bodies.len());
        let mut seen = HashSet::new();
        let terrain = tree.terrain();
        for body in bodies {
            let id = body.id;
            // A part with a model is several bodies here, but one part there.
            if !seen.insert(id) {
                continue;
            }
            if Some(id) == terrain {
                continue;
            }
            let Some(size) = tree.get_vec3(id, Property::Size) else { continue };
            let shape = match tree.get_enum(id, Property::Shape) {
                Some(EnumValue::Shape(shape)) => shape,
                _ => PartShape::Block,
            };
            let character = !body.anchored && !body.sim && !body.held;
            let orientation = tree.get_vec3(id, Property::Orientation).unwrap_or_default();
            let (rot, orientation) = if character {
                (Quat::IDENTITY, Vec3::ZERO)
            } else {
                (orientation_quat(orientation), orientation)
            };
            let tree_anchored = tree.get_bool(id, Property::Anchored) != Some(false) || self.asleep(id);
            let role = if tree_anchored {
                rigid::Role::Fixed
            } else if let Some((root, offset)) = self.welded.get(&id).filter(|(r, _)| dynamic.contains(r)) {
                rigid::Role::Member { root: *root, offset: *offset }
            } else if dynamic.contains(&id) {
                rigid::Role::Dynamic
            } else {
                rigid::Role::Kinematic
            };
            // What it's made of; characters keep a body that walks the same on anything.
            let made_of = if character {
                libroxel_instance::physical::Physical::of_material(libroxel_instance::Material::Plastic)
            } else {
                libroxel_instance::physical::of_part(tree, id)
            };
            let made_of = libroxel_instance::physical::Physical {
                elasticity: if character { 0.1 } else { bounce(made_of.elasticity) },
                ..made_of
            };
            // Characters reach a little further sideways, to shove what they walk into.
            let size = if character {
                let size = Vec3::new(size.x, body.half.y * 2.0, size.z);
                size + Vec3::new(2.0 * SHOVE_REACH, 0.0, 2.0 * SHOVE_REACH)
            } else {
                size
            };
            specs.push(rigid::Spec {
                id,
                role,
                pos: match body.collider {
                    Collider::Convex { centre, .. } => l(centre),
                    _ => body.pos,
                },
                rot,
                orientation,
                size,
                shape: if character { PartShape::Block } else { shape },
                mesh: if character { None } else { mesh_shape(tree, id) },
                can_collide: body.can_collide,
                floats: body.water,
                density: made_of.density,
                friction: {
                    let root = self.welded.get(&id).map_or(id, |(r, _)| *r);
                    if self.driven.contains(&root) { 0.0 } else { made_of.friction }
                },
                elasticity: made_of.elasticity,
            });
        }
        specs
    }

    /// Advances the simulation by `dt` seconds.
    pub fn step(&mut self, tree: &mut InstanceTree, dt: f32) -> StepResult {
        let mut clock = std::time::Instant::now();
        let mut lap = || {
            let now = std::time::Instant::now();
            let took = (now - clock).as_secs_f64();
            clock = now;
            took
        };
        let gravity = tree.get_number(tree.workspace(), Property::Gravity).map_or(GRAVITY, |g| g as f32);
        // Which parts are near anything that moves: found once a step (the
        // touch check after the move reuses it; nothing moves far in a step).
        self.wake_moved(tree);
        let parts = self.parts_near_movers(tree);
        self.wake_in_the_way(tree, &parts);
        let mut bodies = self.collect_bodies(tree, &parts, dt, gravity, false);
        let mut gather = lap();
        self.groups.refresh(tree, bodies.iter().map(|b| b.id));
        let fallen_height = fallen_parts_height(tree);
        let live: HashSet<InstanceId> = bodies.iter().map(|b| b.id).collect();
        self.velocities.retain(|id, _| live.contains(id));
        self.spins.retain(|id, _| live.contains(id));
        self.hips.retain(|id, _| live.contains(id));
        for body in bodies.iter().filter(|b| b.hip > 0.0) {
            self.hips.entry(body.id).or_insert(body.hip);
        }
        self.grounded.clear();
        self.wet = bodies.iter().filter(|b| b.water.is_some()).map(|b| b.id).collect();

        // Characters first, against the world as it stands.
        let substeps = ((dt / MAX_SUBSTEP).ceil() as usize).clamp(1, MAX_SUBSTEPS);
        let h = dt / substeps as f32;
        lap();
        let near = self.broadphase(&bodies, dt, gravity);
        let mut broadphase = lap();
        for step in 1..=substeps {
            let slowing = if step == substeps { dt } else { 0.0 };
            self.integrate(&mut bodies, &near, h, slowing, gravity);
        }

        // Then everything loose, shoved by where the characters went.
        let characters = lap();
        let specs = self.specs(tree, &bodies);
        let voxels = tree.terrain().and_then(|t| tree.voxels(t).ok());
        let moved = self.rigid.step(
            &specs,
            &self.ignored,
            &self.groups,
            &mut self.velocities,
            &mut self.spins,
            voxels,
            &self.joints,
            gravity,
            dt,
        );

        let loose = lap();
        let mut result = StepResult::default();
        for body in &bodies {
            if body.anchored || body.held || body.sim {
                continue;
            }
            let pos = Vec3::new(body.pos.x, body.pos.y + body.hip * 0.5, body.pos.z);
            if tree.get_vec3(body.id, Property::Position) != Some(pos) {
                let _ = tree.set(body.id, Property::Position, PropertyValue::Vec3(pos));
            }
            if pos.y < fallen_height {
                result.fell.push(body.id);
            }
        }
        for m in &moved {
            if self.rigid.is_grounded(m.id) {
                self.grounded.insert(m.id);
            }
            if tree.get_vec3(m.id, Property::Position) != Some(m.pos) {
                let _ = tree.set(m.id, Property::Position, PropertyValue::Vec3(m.pos));
            }
            if tree.get_vec3(m.id, Property::Orientation) != Some(m.orientation) {
                let _ = tree.set(m.id, Property::Orientation, PropertyValue::Vec3(m.orientation));
            }
            if m.pos.y < fallen_height {
                result.fell.push(m.id);
            }
        }

        self.note_rest(tree, &bodies);

        // Touch detection, where everything is now: only pairs where something can move.
        lap();
        let bodies = self.collect_bodies(tree, &parts, dt, gravity, true);
        gather += lap();
        let near = self.broadphase(&bodies, dt, gravity);
        broadphase += lap();
        let mut current = HashSet::new();
        for (i, a) in bodies.iter().enumerate() {
            if a.anchored {
                continue;
            }
            for &j in &near[i] {
                let b = &bodies[j];
                if (j < i && !b.anchored) || self.ignores(a.id, b.id) {
                    continue; // a moving pair is checked once, from the lower index
                }
                if touching(a, b) {
                    current.insert(ordered(a.id, b.id));
                }
            }
        }
        let touch_time = lap();
        let t = &mut self.timing;
        t.steps += 1;
        t.gather += gather;
        t.broadphase += broadphase;
        t.characters += characters;
        t.loose += loose;
        t.touches += touch_time;
        t.bodies += bodies.len() as u64;
        t.movers += bodies.iter().filter(|b| !b.anchored).count() as u64;
        let mut touches: Vec<_> = current.difference(&self.contacts).copied().collect();
        touches.sort();
        result.touches = touches;
        // Pairs nothing looked at this step (a sleeping part on the floor)
        // still touch.
        let looked: HashSet<InstanceId> = bodies.iter().filter(|b| !b.anchored).map(|b| b.id).collect();
        for pair in &self.contacts {
            if !looked.contains(&pair.0)
                && !looked.contains(&pair.1)
                && tree.exists(pair.0)
                && tree.exists(pair.1)
                && !tree.is_destroyed(pair.0).unwrap_or(true)
                && !tree.is_destroyed(pair.1).unwrap_or(true)
                && (self.asleep(pair.0) || self.asleep(pair.1))
            {
                current.insert(*pair);
            }
        }
        let mut untouches: Vec<_> = self.contacts.difference(&current).copied().collect();
        untouches.sort();
        result.untouches = untouches;
        self.contacts = current;
        self.held.clear();
        self.driven.clear();
        self.motions.clear();
        result
    }

    /// For each moving body, the bodies it could meet during a step of `dt`
    /// seconds, in index order. Small worlds check everything; bigger ones
    /// put boxes (grown by how far they can move) in a grid of cells and
    /// only compare boxes that share a cell. Huge boxes, like a baseplate,
    /// are compared with everything instead of filling thousands of cells.
    fn broadphase(&self, bodies: &[Body], dt: f32, gravity: f32) -> Vec<Vec<usize>> {
        let n = bodies.len();
        if n <= BROADPHASE_MIN {
            return (0..n)
                .map(|i| if bodies[i].anchored { Vec::new() } else { (0..n).filter(|j| *j != i).collect() })
                .collect();
        }
        // Each box grown by the most it could move this step.
        let grown: Vec<(Vec3, Vec3)> = bodies
            .iter()
            .map(|b| {
                let reach = if b.anchored {
                    TOUCH_MARGIN + EPS
                } else {
                    let v = self.velocity(b.id);
                    (v.length() + gravity.abs() * dt) * dt + STEP_HEIGHT + TOUCH_MARGIN + 1.0
                };
                let r = Vec3::new(reach, reach, reach);
                (b.min() - r, b.max() + r)
            })
            .collect();
        // Two grids: small boxes in fine cells, big ones (floors, walls) in
        // coarse cells, and only the giant ones (a baseplate) are checked
        // against everything. A map with hundreds of big floor parts would
        // otherwise compare every mover with every one of them.
        // Only movers go in the grids (fine cells for small ones, coarse
        // cells as well for big boxes to find them by); every box then looks
        // up the movers in its own cells. Most boxes are anchored and never
        // need a cell of their own, and a map with hundreds of big floor
        // parts doesn't compare every mover with every one of them.
        let cell_of = |v: f32| (v / CELL).floor() as i32;
        let coarse_of = |v: f32| (v / COARSE_CELL).floor() as i32;
        let span = |lo: Vec3, hi: Vec3, of: &dyn Fn(f32) -> i32| {
            let (a, b) = ((of(lo.x), of(lo.y), of(lo.z)), (of(hi.x), of(hi.y), of(hi.z)));
            let cells = (b.0 - a.0 + 1) as i64 * (b.1 - a.1 + 1) as i64 * (b.2 - a.2 + 1) as i64;
            (a, b, cells)
        };
        // (A fast hash: these maps are rebuilt twice a frame.)
        let mut fine: FastMap<(i32, i32, i32), Vec<usize>> = FastMap::default();
        let mut coarse: FastMap<(i32, i32, i32), Vec<usize>> = FastMap::default();
        // Movers too big for fine cells, and ones too big even for coarse cells.
        let (mut big_movers, mut giant_movers) = (Vec::new(), Vec::new());
        let add =
            |grid: &mut FastMap<(i32, i32, i32), Vec<usize>>, a: (i32, i32, i32), b: (i32, i32, i32), i| {
                for x in a.0..=b.0 {
                    for y in a.1..=b.1 {
                        for z in a.2..=b.2 {
                            grid.entry((x, y, z)).or_default().push(i);
                        }
                    }
                }
            };
        let movers: Vec<usize> = (0..n).filter(|i| !bodies[*i].anchored).collect();
        for &i in &movers {
            let (lo, hi) = grown[i];
            let (a, b, cells) = span(lo, hi, &coarse_of);
            if cells > MAX_CELLS {
                giant_movers.push(i);
                continue;
            }
            add(&mut coarse, a, b, i);
            let (a, b, cells) = span(lo, hi, &cell_of);
            if cells > MAX_CELLS {
                big_movers.push(i);
            } else {
                add(&mut fine, a, b, i);
            }
        }
        let meet = |a: &(Vec3, Vec3), b: &(Vec3, Vec3)| {
            a.0.x <= b.1.x
                && b.0.x <= a.1.x
                && a.0.y <= b.1.y
                && b.0.y <= a.1.y
                && a.0.z <= b.1.z
                && b.0.z <= a.1.z
        };
        let mut near = vec![Vec::new(); n];
        let mut found: Vec<usize> = Vec::new();
        for j in 0..n {
            let (lo, hi) = grown[j];
            found.clear();
            found.extend_from_slice(&giant_movers);
            let (a, b, cells) = span(lo, hi, &cell_of);
            let (grid, a, b) = if cells <= MAX_CELLS {
                found.extend_from_slice(&big_movers);
                (&fine, a, b)
            } else {
                let (a, b, cells) = span(lo, hi, &coarse_of);
                if cells > MAX_CELLS {
                    // A giant box (a baseplate): every mover is a candidate.
                    for &m in movers.iter().filter(|m| **m != j) {
                        if meet(&grown[m], &grown[j]) {
                            near[m].push(j);
                        }
                    }
                    continue;
                }
                (&coarse, a, b)
            };
            for x in a.0..=b.0 {
                for y in a.1..=b.1 {
                    for z in a.2..=b.2 {
                        if let Some(list) = grid.get(&(x, y, z)) {
                            found.extend_from_slice(list);
                        }
                    }
                }
            }
            // (A mover spans several cells: look at it once.)
            found.sort_unstable();
            found.dedup();
            for &m in &found {
                if m != j && meet(&grown[m], &grown[j]) {
                    near[m].push(j);
                }
            }
        }
        for list in &mut near {
            list.sort_unstable();
            list.dedup();
        }
        near
    }

    fn integrate(&mut self, bodies: &mut [Body], near: &[Vec<usize>], h: f32, slowing: f32, gravity: f32) {
        for i in 0..bodies.len() {
            if bodies[i].anchored || bodies[i].held || bodies[i].sim {
                continue;
            }
            let id = bodies[i].id;
            let mut v = self.velocity(id);
            match bodies[i].water {
                None => v.y -= gravity * h,
                Some(floats) => {
                    if floats {
                        v.y += gravity * 0.3 * h;
                    }
                    // Water slows everything down.
                    v = v * (1.0 - 3.0 * h).max(0.0);
                }
            }
            let mut grounded = false;

            // Move one axis at a time (Y first, so standing on the floor
            // never blocks walking), pushing out of anything solid.
            for ax in [1, 0, 2] {
                let delta = axis(v, ax) * h;
                if delta == 0.0 {
                    continue;
                }
                let mut pos = bodies[i].pos;
                let moved = axis(pos, ax) + delta;
                set_axis(&mut pos, ax, moved);
                bodies[i].pos = pos;
                if !bodies[i].can_collide {
                    continue;
                }
                for &j in &near[i] {
                    if i == j
                        || !bodies[j].can_collide
                        || bodies[j].collider != Collider::Aabb
                        || !overlaps(&bodies[i], &bodies[j], -EPS)
                        || self.ignores(bodies[i].id, bodies[j].id)
                    {
                        continue;
                    }
                    let other = bodies[j];
                    // Walk up small ledges (like the top of a ramp) instead of stopping.
                    let rise = other.max().y - bodies[i].min().y;
                    if ax != 1 && rise > 0.0 && rise <= other.climb {
                        bodies[i].pos.y += rise + EPS;
                        continue;
                    }
                    let half = axis(bodies[i].half, ax);
                    let mut pos = bodies[i].pos;
                    if delta > 0.0 {
                        let edge = axis(other.min(), ax) - half;
                        set_axis(&mut pos, ax, edge);
                    } else {
                        let edge = axis(other.max(), ax) + half;
                        set_axis(&mut pos, ax, edge);
                        if ax == 1 {
                            grounded = true;
                        }
                    }
                    bodies[i].pos = pos;
                    set_axis(&mut v, ax, 0.0);
                }
            }

            // Then push out of tilted and round shapes along the shortest way.
            if bodies[i].can_collide {
                for _ in 0..3 {
                    let mut pushed = false;
                    for &j in &near[i] {
                        let other = bodies[j];
                        let moving = self.motions.get(&other.id).copied();
                        if i == j
                            || !other.can_collide
                            || (other.collider == Collider::Aabb && moving.is_none())
                            || !overlaps(&bodies[i], &other, -EPS)
                            || self.ignores(bodies[i].id, other.id)
                        {
                            continue;
                        }
                        let Some((n, depth)) = penetration(&bodies[i], &other, 0.0) else { continue };
                        if depth <= EPS {
                            continue;
                        }
                        pushed = true;
                        let mut pos = g(bodies[i].pos);
                        let mut vel = g(v);
                        if n.y > 0.7 {
                            // Standing on a gentle slope: lift straight up so we don't slide.
                            pos.y += (depth / n.y).min(depth * 2.0);
                            vel.y = vel.y.max(0.0);
                            grounded = true;
                        } else {
                            pos += n * depth;
                            let into = vel.dot(n);
                            if into < 0.0 {
                                vel -= n * into;
                            }
                            if n.y > 0.3 {
                                grounded = true;
                            }
                        }
                        // Something turning (a hinge arm) carries what it shoves.
                        if let Some((pivot, spin, along)) = moving {
                            let surface = along + spin.cross(pos - pivot);
                            let gap = surface.dot(n) - vel.dot(n);
                            if gap > 0.0 {
                                vel += n * gap;
                            }
                        }
                        bodies[i].pos = l(pos);
                        v = l(vel);
                    }
                    if !pushed {
                        break;
                    }
                }
            }

            if grounded {
                self.grounded.insert(id);
                // Ground friction for loose parts, once a frame so walking
                // is as fast at a low frame rate.
                let damp = (1.0 - 8.0 * slowing).max(0.0);
                v.x *= damp;
                v.z *= damp;
            }
            self.velocities.insert(id, v);
        }
    }
}

/// How much a part bounces in the simulation, for Roblox's Elasticity:
/// up to plastic's 0.5, barely (as parts in Roblox do), then more and
/// more, to almost all the way back up at 1.
fn bounce(elasticity: f32) -> f32 {
    let e = elasticity.clamp(0.0, 1.0);
    if e <= 0.5 { e * 0.2 } else { 0.1 + (e - 0.5) * 1.7 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_part(tree: &mut InstanceTree, name: &str, pos: Vec3, size: Vec3, anchored: bool) -> InstanceId {
        let id = tree.create(ClassKind::Part, name).unwrap();
        tree.set_parent(id, Some(tree.workspace())).unwrap();
        tree.set(id, Property::Position, PropertyValue::Vec3(pos)).unwrap();
        tree.set(id, Property::Size, PropertyValue::Vec3(size)).unwrap();
        tree.set(id, Property::Anchored, PropertyValue::Bool(anchored)).unwrap();
        id
    }

    #[test]
    fn big_worlds_use_the_grid_and_still_collide() {
        let mut tree = InstanceTree::new();
        let floor =
            add_part(&mut tree, "Floor", Vec3::new(0.0, -0.5, 0.0), Vec3::new(500.0, 1.0, 500.0), true);
        let mut crates = Vec::new();
        for i in 0..150 {
            let (x, z) = ((i % 15) as f32 * 6.0 - 45.0, (i / 15) as f32 * 6.0 - 30.0);
            crates.push(add_part(&mut tree, "Crate", Vec3::new(x, 6.0, z), Vec3::new(2.0, 2.0, 2.0), false));
            // Scenery beside each crate, which the grid has to keep track of too.
            add_part(&mut tree, "Rock", Vec3::new(x + 3.0, 1.0, z), Vec3::new(1.0, 2.0, 1.0), true);
        }
        // One crate drops onto a pillar instead of the floor.
        let pillar =
            add_part(&mut tree, "Pillar", Vec3::new(200.0, 5.0, 200.0), Vec3::new(2.0, 10.0, 2.0), true);
        let perched =
            add_part(&mut tree, "Perched", Vec3::new(200.0, 14.0, 200.0), Vec3::new(2.0, 2.0, 2.0), false);
        assert!(tree.descendants(tree.workspace()).unwrap().len() > BROADPHASE_MIN);
        let mut physics = Physics::default();
        let mut touched = HashSet::new();
        for _ in 0..120 {
            touched.extend(physics.step(&mut tree, 1.0 / 60.0).touches);
        }
        for c in crates {
            let y = tree.get_vec3(c, Property::Position).unwrap().y;
            assert!((y - 1.0).abs() < 0.01, "landed on the floor: {y}");
            assert!(touched.contains(&ordered(floor, c)));
        }
        let y = tree.get_vec3(perched, Property::Position).unwrap().y;
        assert!((y - 11.0).abs() < 0.01, "landed on the pillar: {y}");
        assert!(touched.contains(&ordered(pillar, perched)));
    }

    /// How far a box pushed along a floor of `material` slides in two seconds.
    fn slide_on(material: libroxel_instance::Material) -> f32 {
        let mut tree = InstanceTree::new();
        let floor =
            add_part(&mut tree, "Floor", Vec3::new(0.0, -0.5, 0.0), Vec3::new(400.0, 1.0, 40.0), true);
        tree.set(floor, Property::Material, PropertyValue::Enum(EnumValue::Material(material))).unwrap();
        let block =
            add_part(&mut tree, "Block", Vec3::new(-150.0, 1.0, 0.0), Vec3::new(2.0, 2.0, 2.0), false);
        let mut physics = Physics::default();
        physics.step(&mut tree, 1.0 / 60.0);
        physics.set_velocity(block, Vec3::new(20.0, 0.0, 0.0));
        for _ in 0..120 {
            physics.step(&mut tree, 1.0 / 60.0);
        }
        tree.get_vec3(block, Property::Position).unwrap().x + 150.0
    }

    #[test]
    fn ice_is_slippery_and_concrete_grips() {
        use libroxel_instance::Material;
        let (ice, concrete) = (slide_on(Material::Ice), slide_on(Material::Concrete));
        assert!(ice > concrete * 2.0, "ice {ice}, concrete {concrete}");
    }

    #[test]
    fn parts_fall_and_land_on_the_floor() {
        let mut tree = InstanceTree::new();
        let floor = add_part(&mut tree, "Floor", Vec3::new(0.0, -0.5, 0.0), Vec3::new(20.0, 1.0, 20.0), true);
        let crate_ = add_part(&mut tree, "Crate", Vec3::new(0.0, 10.0, 0.0), Vec3::new(2.0, 2.0, 2.0), false);
        let mut physics = Physics::default();
        let mut touched = Vec::new();
        for _ in 0..120 {
            touched.extend(physics.step(&mut tree, 1.0 / 60.0).touches);
        }
        let pos = tree.get_vec3(crate_, Property::Position).unwrap();
        assert!((pos.y - 1.0).abs() < 0.01, "rests on top of the floor, got {pos:?}");
        assert!(physics.is_grounded(crate_));
        assert_eq!(touched, vec![ordered(floor, crate_)], "touch fires once, not every frame");
        assert_eq!(tree.get_vec3(floor, Property::Position), Some(Vec3::new(0.0, -0.5, 0.0)));
    }

    #[test]
    fn non_colliding_parts_are_passed_through_but_still_touched() {
        let mut tree = InstanceTree::new();
        add_part(&mut tree, "Floor", Vec3::new(0.0, -10.5, 0.0), Vec3::new(20.0, 1.0, 20.0), true);
        let ghost = add_part(&mut tree, "Ghost", Vec3::new(0.0, 0.0, 0.0), Vec3::new(20.0, 1.0, 20.0), true);
        tree.set(ghost, Property::CanCollide, PropertyValue::Bool(false)).unwrap();
        let ball = add_part(&mut tree, "Ball", Vec3::new(0.0, 5.0, 0.0), Vec3::ONE, false);
        let mut physics = Physics::default();
        let mut touched = Vec::new();
        for _ in 0..120 {
            touched.extend(physics.step(&mut tree, 1.0 / 60.0).touches);
        }
        assert!(tree.get_vec3(ball, Property::Position).unwrap().y < -9.0, "fell through the ghost");
        assert!(touched.contains(&ordered(ghost, ball)));
    }

    #[test]
    fn walls_stop_sideways_motion() {
        let mut tree = InstanceTree::new();
        add_part(&mut tree, "Floor", Vec3::new(0.0, -0.5, 0.0), Vec3::new(40.0, 1.0, 40.0), true);
        add_part(&mut tree, "Wall", Vec3::new(5.0, 5.0, 0.0), Vec3::new(1.0, 10.0, 10.0), true);
        let mover = add_part(&mut tree, "Mover", Vec3::new(0.0, 1.0, 0.0), Vec3::new(2.0, 2.0, 2.0), false);
        let mut physics = Physics::default();
        for _ in 0..60 {
            let v = physics.velocity(mover);
            physics.set_velocity(mover, Vec3::new(20.0, v.y, 0.0));
            physics.step(&mut tree, 1.0 / 60.0);
        }
        let pos = tree.get_vec3(mover, Property::Position).unwrap();
        assert!((pos.x - 3.5).abs() < 0.01, "stopped against the wall, got {pos:?}");
    }

    fn rotate(tree: &mut InstanceTree, id: InstanceId, degrees: Vec3) {
        tree.set(id, Property::Orientation, PropertyValue::Vec3(degrees)).unwrap();
    }

    #[test]
    fn right_angle_turns_swap_the_box() {
        let mut tree = InstanceTree::new();
        // A 20-long beam turned 90 degrees now runs along Z.
        let beam = add_part(&mut tree, "Beam", Vec3::new(0.0, -0.5, 0.0), Vec3::new(20.0, 1.0, 2.0), true);
        rotate(&mut tree, beam, Vec3::new(0.0, 90.0, 0.0));
        let on = add_part(&mut tree, "On", Vec3::new(0.0, 5.0, 8.0), Vec3::ONE, false);
        let off = add_part(&mut tree, "Off", Vec3::new(8.0, 5.0, 0.0), Vec3::ONE, false);
        let mut physics = Physics::default();
        for _ in 0..90 {
            physics.step(&mut tree, 1.0 / 60.0);
        }
        assert!((tree.get_vec3(on, Property::Position).unwrap().y - 0.5).abs() < 0.01);
        assert!(tree.get_vec3(off, Property::Position).unwrap().y < -1.0, "fell past the turned beam");
    }

    #[test]
    fn tilted_ramps_and_wedges_hold_things_up() {
        for wedge in [false, true] {
            let mut tree = InstanceTree::new();
            let ramp = add_part(&mut tree, "Ramp", Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0), true);
            if wedge {
                let shape = PropertyValue::Enum(EnumValue::Shape(PartShape::Wedge));
                tree.set(ramp, Property::Shape, shape).unwrap();
            } else {
                rotate(&mut tree, ramp, Vec3::new(30.0, 0.0, 0.0));
            }
            let crate_ = add_part(&mut tree, "Crate", Vec3::new(0.0, 12.0, 0.0), Vec3::ONE, false);
            let mut physics = Physics::default();
            let mut touched = false;
            // Half a second: it has landed on the slope (and, being plastic
            // on a steep slope, started to slide down it).
            for _ in 0..30 {
                touched |= !physics.step(&mut tree, 1.0 / 60.0).touches.is_empty();
            }
            let y = tree.get_vec3(crate_, Property::Position).unwrap().y;
            assert!(y > -5.0 && y < 7.0, "on the slope (wedge: {wedge}), got y = {y}");
            assert!(touched && physics.is_grounded(crate_));
        }
    }

    #[test]
    fn balls_are_round() {
        let mut tree = InstanceTree::new();
        let ball = add_part(&mut tree, "Ball", Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0), true);
        tree.set(ball, Property::Shape, PropertyValue::Enum(EnumValue::Shape(PartShape::Ball))).unwrap();
        // Over the middle it lands on top; near the corner of the bounding box it misses.
        let top = add_part(&mut tree, "Top", Vec3::new(0.0, 6.0, 0.0), Vec3::ONE, false);
        let corner = add_part(&mut tree, "Corner", Vec3::new(1.8, 6.0, 1.8), Vec3::new(0.5, 0.5, 0.5), false);
        let mut physics = Physics::default();
        for _ in 0..120 {
            physics.step(&mut tree, 1.0 / 60.0);
        }
        assert!((tree.get_vec3(top, Property::Position).unwrap().y - 2.5).abs() < 0.05);
        assert!(tree.get_vec3(corner, Property::Position).unwrap().y < 0.0, "slipped past the curve");
    }

    #[test]
    fn falling_out_of_the_world_is_reported() {
        let mut tree = InstanceTree::new();
        let rock =
            add_part(&mut tree, "Rock", Vec3::new(0.0, FALLEN_PARTS_HEIGHT + 1.0, 0.0), Vec3::ONE, false);
        let mut physics = Physics::default();
        let mut fell = Vec::new();
        for _ in 0..30 {
            fell.extend(physics.step(&mut tree, 1.0 / 60.0).fell);
        }
        assert!(fell.contains(&rock));
    }
}
