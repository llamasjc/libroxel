//! A small box physics engine.
//!
//! Moving (unanchored) parts are axis-aligned boxes; they don't tumble.
//! Anchored parts can be rotated and shaped: boxes and parts turned in
//! right angles are handled as axis-aligned boxes (exact and cheap), while
//! tilted boxes and wedges use a separating-axis test and balls use a
//! sphere test, so ramps and round obstacles work. Unanchored parts fall
//! under gravity and stop on anything with `CanCollide`. Every frame we
//! also work out which pairs of parts started touching, for the `Touched`
//! event.

use std::collections::{HashMap, HashSet};

use bevy::math::{EulerRot, Mat3, Quat, Vec3 as GVec3};
use libroxel_instance::{
    ClassKind, EnumValue, InstanceId, InstanceTree, PartShape, Property, PropertyValue, Vec3,
};

/// Downward acceleration, in studs per second squared.
pub const GRAVITY: f32 = 196.2;
/// Unanchored parts that fall below this height are destroyed.
pub const FALLEN_PARTS_HEIGHT: f32 = -100.0;
/// The longest slice of time simulated in one go; longer frames are split.
const MAX_SUBSTEP: f32 = 1.0 / 60.0;
const MAX_SUBSTEPS: usize = 8;
/// How far apart two boxes may be and still count as touching.
const TOUCH_MARGIN: f32 = 0.05;
const EPS: f32 = 1e-4;
/// Moving boxes climb onto anything whose top is at most this far above their bottom.
pub const STEP_HEIGHT: f32 = 0.6;

/// Turns an orientation in degrees (Y, then X, then Z) into a rotation.
pub fn orientation_quat(degrees: Vec3) -> Quat {
    Quat::from_euler(EulerRot::YXZ, degrees.y.to_radians(), degrees.x.to_radians(), degrees.z.to_radians())
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
}

#[derive(Debug, Clone, Copy)]
struct Body {
    id: InstanceId,
    pos: Vec3,
    half: Vec3,
    anchored: bool,
    can_collide: bool,
    collider: Collider,
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
    grounded: HashSet<InstanceId>,
    contacts: HashSet<(InstanceId, InstanceId)>,
    /// Parts moved by something else (a weld) that stand still in a step.
    carried: HashSet<InstanceId>,
    /// Pairs that pass through each other and never touch (parts of one assembly).
    ignored: HashSet<(InstanceId, InstanceId)>,
}

fn ordered(a: InstanceId, b: InstanceId) -> (InstanceId, InstanceId) {
    if a <= b { (a, b) } else { (b, a) }
}

impl Physics {
    pub fn velocity(&self, id: InstanceId) -> Vec3 {
        self.velocities.get(&id).copied().unwrap_or_default()
    }

    pub fn set_velocity(&mut self, id: InstanceId, velocity: Vec3) {
        self.velocities.insert(id, velocity);
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
        }
        self.carried = carried;
        self.ignored = together.into_iter().map(|(a, b)| ordered(a, b)).collect();
    }

    /// Also holds `id` still for the next step, passing through `together`.
    pub fn carry(&mut self, id: InstanceId, together: &[InstanceId]) {
        self.velocities.remove(&id);
        self.carried.insert(id);
        for &other in together {
            self.ignored.insert(ordered(id, other));
        }
    }

    fn ignores(&self, a: InstanceId, b: InstanceId) -> bool {
        !self.ignored.is_empty() && self.ignored.contains(&ordered(a, b))
    }

    /// Whether `id` ended the last step resting on something.
    pub fn is_grounded(&self, id: InstanceId) -> bool {
        self.grounded.contains(&id)
    }

    fn collect_bodies(&self, tree: &InstanceTree) -> Vec<Body> {
        tree.descendants_of_class(tree.workspace(), ClassKind::Part)
            .into_iter()
            .filter_map(|id| {
                let size = tree.get_vec3(id, Property::Size)?;
                let anchored = tree.get_bool(id, Property::Anchored)? || self.carried.contains(&id);
                let shape = match tree.get_enum(id, Property::Shape) {
                    Some(EnumValue::Shape(shape)) => shape,
                    _ => PartShape::Block,
                };
                let orientation = tree.get_vec3(id, Property::Orientation).unwrap_or_default();
                let (half, collider) = if anchored {
                    collider_for(size, shape, orientation)
                } else {
                    // Moving parts stay upright boxes.
                    (size * 0.5, Collider::Aabb)
                };
                Some(Body {
                    id,
                    pos: tree.get_vec3(id, Property::Position)?,
                    half,
                    anchored,
                    can_collide: tree.get_bool(id, Property::CanCollide)?,
                    collider,
                })
            })
            .collect()
    }

    /// Advances the simulation by `dt` seconds.
    pub fn step(&mut self, tree: &mut InstanceTree, dt: f32) -> StepResult {
        let mut bodies = self.collect_bodies(tree);
        let gravity = tree.get_number(tree.workspace(), Property::Gravity).map_or(GRAVITY, |g| g as f32);
        let live: HashSet<InstanceId> = bodies.iter().map(|b| b.id).collect();
        self.velocities.retain(|id, _| live.contains(id));
        self.grounded.clear();

        let substeps = ((dt / MAX_SUBSTEP).ceil() as usize).clamp(1, MAX_SUBSTEPS);
        let h = dt / substeps as f32;
        for _ in 0..substeps {
            self.integrate(&mut bodies, h, gravity);
        }

        let mut result = StepResult::default();
        for body in &bodies {
            if body.anchored {
                continue;
            }
            if tree.get_vec3(body.id, Property::Position) != Some(body.pos) {
                let _ = tree.set(body.id, Property::Position, PropertyValue::Vec3(body.pos));
            }
            if body.pos.y < FALLEN_PARTS_HEIGHT {
                result.fell.push(body.id);
            }
        }

        // Touch detection: only pairs where something can move.
        let mut current = HashSet::new();
        for (i, a) in bodies.iter().enumerate() {
            for b in &bodies[i + 1..] {
                if (a.anchored && b.anchored) || self.ignores(a.id, b.id) {
                    continue;
                }
                if touching(a, b) {
                    current.insert(ordered(a.id, b.id));
                }
            }
        }
        let mut touches: Vec<_> = current.difference(&self.contacts).copied().collect();
        touches.sort();
        result.touches = touches;
        let mut untouches: Vec<_> = self.contacts.difference(&current).copied().collect();
        untouches.sort();
        result.untouches = untouches;
        self.contacts = current;
        result
    }

    fn integrate(&mut self, bodies: &mut [Body], h: f32, gravity: f32) {
        for i in 0..bodies.len() {
            if bodies[i].anchored {
                continue;
            }
            let id = bodies[i].id;
            let mut v = self.velocity(id);
            v.y -= gravity * h;
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
                for j in 0..bodies.len() {
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
                    if ax != 1 && rise > 0.0 && rise <= STEP_HEIGHT {
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
                    for j in 0..bodies.len() {
                        let other = bodies[j];
                        if i == j
                            || !other.can_collide
                            || other.collider == Collider::Aabb
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
                // Ground friction for loose parts.
                let damp = (1.0 - 8.0 * h).max(0.0);
                v.x *= damp;
                v.z *= damp;
            }
            self.velocities.insert(id, v);
        }
    }
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
        assert!((pos.y - 1.0).abs() < 1e-3, "rests on top of the floor, got {pos:?}");
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
        assert!((pos.x - 3.5).abs() < 1e-3, "stopped against the wall, got {pos:?}");
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
        assert!((tree.get_vec3(on, Property::Position).unwrap().y - 0.5).abs() < 1e-3);
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
            for _ in 0..120 {
                touched |= !physics.step(&mut tree, 1.0 / 60.0).touches.is_empty();
            }
            let y = tree.get_vec3(crate_, Property::Position).unwrap().y;
            assert!(y > 0.0 && y < 7.0, "rests on the slope (wedge: {wedge}), got y = {y}");
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
