//! A small box physics engine.
//!
//! Parts have no rotation yet, so every part is an axis-aligned box and
//! collisions are exact and cheap. Unanchored parts fall under gravity and
//! stop on anything with `CanCollide`. Every frame we also work out which
//! pairs of parts started touching, for the `Touched` event.

use std::collections::{HashMap, HashSet};

use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue, Vec3};

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

#[derive(Debug, Clone, Copy)]
struct Body {
    id: InstanceId,
    pos: Vec3,
    half: Vec3,
    anchored: bool,
    can_collide: bool,
}

impl Body {
    fn min(&self) -> Vec3 {
        self.pos - self.half
    }
    fn max(&self) -> Vec3 {
        self.pos + self.half
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

/// What happened during a step.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct StepResult {
    /// Pairs that started touching this step. Each pair appears once.
    pub touches: Vec<(InstanceId, InstanceId)>,
    /// Unanchored parts that fell out of the world (the caller destroys them).
    pub fell: Vec<InstanceId>,
}

/// Physics state that isn't stored in the tree, such as velocities.
#[derive(Debug, Default)]
pub struct Physics {
    velocities: HashMap<InstanceId, Vec3>,
    grounded: HashSet<InstanceId>,
    contacts: HashSet<(InstanceId, InstanceId)>,
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

    /// Whether `id` ended the last step resting on something.
    pub fn is_grounded(&self, id: InstanceId) -> bool {
        self.grounded.contains(&id)
    }

    fn collect_bodies(tree: &InstanceTree) -> Vec<Body> {
        tree.descendants_of_class(tree.workspace(), ClassKind::Part)
            .into_iter()
            .filter_map(|id| {
                Some(Body {
                    id,
                    pos: tree.get_vec3(id, Property::Position)?,
                    half: tree.get_vec3(id, Property::Size)? * 0.5,
                    anchored: tree.get_bool(id, Property::Anchored)?,
                    can_collide: tree.get_bool(id, Property::CanCollide)?,
                })
            })
            .collect()
    }

    /// Advances the simulation by `dt` seconds.
    pub fn step(&mut self, tree: &mut InstanceTree, dt: f32) -> StepResult {
        let mut bodies = Self::collect_bodies(tree);
        let live: HashSet<InstanceId> = bodies.iter().map(|b| b.id).collect();
        self.velocities.retain(|id, _| live.contains(id));
        self.grounded.clear();

        let substeps = ((dt / MAX_SUBSTEP).ceil() as usize).clamp(1, MAX_SUBSTEPS);
        let h = dt / substeps as f32;
        for _ in 0..substeps {
            self.integrate(&mut bodies, h);
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
                if a.anchored && b.anchored {
                    continue;
                }
                if overlaps(a, b, TOUCH_MARGIN) {
                    current.insert(ordered(a.id, b.id));
                }
            }
        }
        let mut touches: Vec<_> = current.difference(&self.contacts).copied().collect();
        touches.sort();
        result.touches = touches;
        self.contacts = current;
        result
    }

    fn integrate(&mut self, bodies: &mut [Body], h: f32) {
        for i in 0..bodies.len() {
            if bodies[i].anchored {
                continue;
            }
            let id = bodies[i].id;
            let mut v = self.velocity(id);
            v.y -= GRAVITY * h;
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
                    if i == j || !bodies[j].can_collide || !overlaps(&bodies[i], &bodies[j], -EPS) {
                        continue;
                    }
                    let other = bodies[j];
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
