//! Movers and constraints: things that push loose parts around while
//! playing. Each one acts on the assembly its attachment's part belongs to
//! (see [`weld`](crate::weld)), by changing that assembly's velocity just
//! before the physics step.
//!
//! - `LinearVelocity` and `BodyVelocity` push towards a velocity.
//! - `VectorForce` and `BodyForce` push steadily.
//! - `AlignPosition` and `BodyPosition` pull towards a point, like a stiff
//!   spring with damping.
//! - `RopeConstraint` keeps two attachments within a length.
//! - `SpringConstraint` pulls two attachments towards a length.
//! - `AngularVelocity` and `BodyAngularVelocity` spin towards an angular
//!   velocity. An assembly spins about its root part's middle.
//!
//! Springs are solved implicitly, so even very stiff ones stay steady.

use libroxel_instance::{CFrame, ClassKind, InstanceId, InstanceTree, Property, Rotation, Vec3};
use libroxel_script::names;

use crate::physics::{GRAVITY, Physics};
use crate::weld::Welds;

/// Where an attachment is in the world, and the part it's on.
pub fn attachment_world(tree: &InstanceTree, attachment: InstanceId) -> Option<(Vec3, InstanceId)> {
    if tree.class(attachment).ok()? != ClassKind::Attachment || tree.is_destroyed(attachment).ok()? {
        return None;
    }
    let part = tree.parent(attachment).ok()??;
    if !tree.class(part).ok()?.is_physical() {
        return None;
    }
    let pos = tree.get_vec3(part, Property::Position)?;
    let rot = Rotation::from_orientation(tree.get_vec3(part, Property::Orientation).unwrap_or_default());
    let offset = tree.get_vec3(attachment, Property::Position).unwrap_or_default();
    Some((CFrame::new(pos, rot).point_to_world(offset), part))
}

/// One end of a constraint: the assembly it moves and how heavy that is.
#[derive(Debug, Clone, Copy)]
struct End {
    root: InstanceId,
    point: Vec3,
    /// 1 / mass, or 0 when nothing can move it (anchored).
    inv_mass: f32,
}

fn mass_of(tree: &InstanceTree, part: InstanceId) -> f32 {
    let s = tree.get_vec3(part, Property::Size).unwrap_or_default();
    (s.x * s.y * s.z * names::extras::DENSITY).max(0.01)
}

fn end(tree: &InstanceTree, welds: &Welds, attachment: Option<InstanceId>) -> Option<End> {
    let (point, part) = attachment_world(tree, attachment?)?;
    let root = welds.root_for(part);
    let anchored = tree.get_bool(root, Property::Anchored) != Some(false);
    let inv_mass = if anchored {
        0.0
    } else {
        let mass: f32 = mass_of(tree, root) + welds.carried_by(root).map(|p| mass_of(tree, p)).sum::<f32>();
        1.0 / mass
    };
    Some(End { root, point, inv_mass })
}

/// The part a legacy body mover is inside, as an end.
fn parent_end(tree: &InstanceTree, welds: &Welds, mover: InstanceId) -> Option<End> {
    let part = tree.parent(mover).ok()??;
    if !tree.class(part).ok()?.is_physical() {
        return None;
    }
    let root = welds.root_for(part);
    if tree.get_bool(root, Property::Anchored) != Some(false) {
        return None;
    }
    let mass: f32 = mass_of(tree, root) + welds.carried_by(root).map(|p| mass_of(tree, p)).sum::<f32>();
    Some(End { root, point: tree.get_vec3(part, Property::Position)?, inv_mass: 1.0 / mass })
}

/// Roughly how hard an assembly is to spin, about any axis through its root.
fn inertia_of(tree: &InstanceTree, welds: &Welds, root: InstanceId) -> f32 {
    let at = tree.get_vec3(root, Property::Position).unwrap_or_default();
    let one = |p: InstanceId| {
        let s = tree.get_vec3(p, Property::Size).unwrap_or_default();
        let r = tree.get_vec3(p, Property::Position).unwrap_or_default() - at;
        // A box's own spin, averaged over the three axes, plus its distance.
        mass_of(tree, p) * (s.dot(s) / 18.0 + r.dot(r) * 2.0 / 3.0)
    };
    (one(root) + welds.carried_by(root).map(one).sum::<f32>()).max(0.01)
}

fn enabled(tree: &InstanceTree, id: InstanceId) -> bool {
    !tree.class(id).is_ok_and(|c| c.has_property(Property::Enabled))
        || tree.get_bool(id, Property::Enabled) == Some(true)
}

fn number(tree: &InstanceTree, id: InstanceId, p: Property) -> f32 {
    tree.get_number(id, p).unwrap_or(0.0) as f32
}

fn vec(tree: &InstanceTree, id: InstanceId, p: Property) -> Vec3 {
    tree.get_vec3(id, p).unwrap_or_default()
}

fn clamp_length(v: Vec3, max: f32) -> Vec3 {
    let len = v.length();
    if len > max && len > 0.0 { v * (max / len) } else { v }
}

fn clamp_axes(v: Vec3, max: Vec3) -> Vec3 {
    let m = Vec3::new(max.x.abs(), max.y.abs(), max.z.abs());
    Vec3::new(v.x.clamp(-m.x, m.x), v.y.clamp(-m.y, m.y), v.z.clamp(-m.z, m.z))
}

/// The velocity change (this step) of a mass on an implicit spring-damper
/// pulling it towards `target`: stiffness `k` and damping `c` per unit mass.
fn spring_dv(pos: Vec3, vel: Vec3, target: Vec3, k: f32, c: f32, dt: f32) -> Vec3 {
    let new_v = (vel + (target - pos) * (k * dt)) / (1.0 + c * dt + k * dt * dt);
    new_v - vel
}

/// Applies every enabled mover and constraint in the workspace. Call after
/// [`Welds::before_physics`] and before [`Physics::step`].
pub fn apply(tree: &InstanceTree, welds: &Welds, physics: &mut Physics, dt: f32) {
    if dt <= 0.0 {
        return;
    }
    let ws = tree.workspace();
    let gravity = tree.get_number(ws, Property::Gravity).map_or(GRAVITY, |g| g as f32);
    let up = Vec3::new(0.0, gravity, 0.0);
    // Velocities this step will end with if nothing else pushes: physics
    // adds gravity during the step, so constraints aim for what's left.
    let predicted = |physics: &Physics, e: &End| {
        if e.inv_mass > 0.0 { physics.velocity(e.root) - up * dt } else { Vec3::ZERO }
    };
    let push = |physics: &mut Physics, e: &End, dv: Vec3| {
        if e.inv_mass > 0.0 {
            let v = physics.velocity(e.root);
            physics.set_velocity(e.root, v + dv);
        }
    };

    for id in tree.descendants(ws).unwrap_or_default() {
        let Ok(class) = tree.class(id) else { continue };
        if !matches!(
            class,
            ClassKind::LinearVelocity
                | ClassKind::VectorForce
                | ClassKind::AlignPosition
                | ClassKind::RopeConstraint
                | ClassKind::SpringConstraint
                | ClassKind::BodyVelocity
                | ClassKind::BodyPosition
                | ClassKind::BodyForce
                | ClassKind::AngularVelocity
                | ClassKind::BodyAngularVelocity
        ) || !enabled(tree, id)
        {
            continue;
        }
        let a0 = tree.get_ref(id, Property::Attachment0);
        let a1 = tree.get_ref(id, Property::Attachment1);
        match class {
            ClassKind::LinearVelocity => {
                let Some(e) = end(tree, welds, a0).filter(|e| e.inv_mass > 0.0) else { continue };
                let target = vec(tree, id, Property::VectorVelocity);
                let v = physics.velocity(e.root);
                // Reach the target this step if the force allows, holding against gravity too.
                let accel = (target - v) / dt + up;
                let max = number(tree, id, Property::MaxForce) * e.inv_mass;
                push(physics, &e, clamp_length(accel, max) * dt);
            }
            ClassKind::VectorForce | ClassKind::BodyForce => {
                let e = if class == ClassKind::BodyForce {
                    parent_end(tree, welds, id)
                } else {
                    end(tree, welds, a0)
                };
                let Some(e) = e.filter(|e| e.inv_mass > 0.0) else { continue };
                push(physics, &e, vec(tree, id, Property::Force) * e.inv_mass * dt);
            }
            ClassKind::BodyVelocity => {
                let Some(e) = parent_end(tree, welds, id) else { continue };
                let target = vec(tree, id, Property::Velocity);
                let v = physics.velocity(e.root);
                let rate = (number(tree, id, Property::BodyPower) * e.inv_mass).min(1.0 / dt);
                let accel = (target - v) * rate + up;
                let max = vec(tree, id, Property::BodyMaxForce) * e.inv_mass;
                push(physics, &e, clamp_axes(accel, max) * dt);
            }
            ClassKind::BodyPosition => {
                let Some(e) = parent_end(tree, welds, id) else { continue };
                let target = vec(tree, id, Property::Position);
                let k = number(tree, id, Property::BodyPower) * e.inv_mass;
                let c = number(tree, id, Property::BodyDamping) * e.inv_mass;
                let v = physics.velocity(e.root);
                let accel = spring_dv(e.point, v, target, k, c, dt) / dt + up;
                let max = vec(tree, id, Property::BodyMaxForce) * e.inv_mass;
                push(physics, &e, clamp_axes(accel, max) * dt);
            }
            ClassKind::AlignPosition => {
                let Some(e) = end(tree, welds, a0).filter(|e| e.inv_mass > 0.0) else { continue };
                let target = match end(tree, welds, a1) {
                    Some(other) => other.point,
                    None if a1.is_none() => vec(tree, id, Property::Position),
                    None => continue,
                };
                let v = physics.velocity(e.root);
                let wanted = if tree.get_bool(id, Property::RigidityEnabled) == Some(true) {
                    (target - e.point) / dt
                } else {
                    let r = number(tree, id, Property::Responsiveness).max(0.01);
                    v + spring_dv(e.point, v, target, r * r, 2.0 * r, dt)
                };
                let wanted = clamp_length(wanted, number(tree, id, Property::MaxVelocity));
                let accel = (wanted - v) / dt + up;
                let max = if tree.get_bool(id, Property::RigidityEnabled) == Some(true) {
                    f32::INFINITY
                } else {
                    number(tree, id, Property::MaxForce) * e.inv_mass
                };
                push(physics, &e, clamp_length(accel, max) * dt);
            }
            ClassKind::RopeConstraint | ClassKind::SpringConstraint => {
                let (Some(e0), Some(e1)) = (end(tree, welds, a0), end(tree, welds, a1)) else { continue };
                let w = e0.inv_mass + e1.inv_mass;
                if w <= 0.0 || e0.root == e1.root {
                    continue;
                }
                let d = e1.point - e0.point;
                let dist = d.length();
                if dist < 1e-4 {
                    continue;
                }
                let n = d / dist;
                // How fast the ends are moving apart, by the end of the step.
                let rel = (predicted(physics, &e1) - predicted(physics, &e0)).dot(n);
                let wanted = if class == ClassKind::RopeConstraint {
                    let slack = dist - number(tree, id, Property::Length);
                    if slack <= 0.0 && rel * dt + slack <= 0.0 {
                        continue; // slack, and staying slack this step
                    }
                    // Pull back in: no more stretching, and close a share of the gap.
                    rel.min(-slack.max(0.0) * 0.4 / dt)
                } else {
                    let m_eff = 1.0 / w;
                    let k = number(tree, id, Property::Stiffness) / m_eff;
                    let c = number(tree, id, Property::Damping) / m_eff;
                    let x = dist - number(tree, id, Property::FreeLength);
                    (rel - dt * k * x) / (1.0 + dt * c + dt * dt * k)
                };
                let change = wanted - rel;
                if change.abs() < 1e-6 {
                    continue;
                }
                // Share the change between the ends by how easy each is to move.
                let impulse = change / w;
                push(physics, &e1, n * (impulse * e1.inv_mass));
                push(physics, &e0, n * (-impulse * e0.inv_mass));
            }
            ClassKind::AngularVelocity => {
                let Some(e) = end(tree, welds, a0).filter(|e| e.inv_mass > 0.0) else { continue };
                let target = vec(tree, id, Property::AngularVelocity);
                let spin = physics.angular_velocity(e.root);
                let most = number(tree, id, Property::MaxTorque) / inertia_of(tree, welds, e.root) * dt;
                physics.drive_spin(e.root, spin + clamp_length(target - spin, most));
            }
            ClassKind::BodyAngularVelocity => {
                let Some(e) = parent_end(tree, welds, id) else { continue };
                let target = vec(tree, id, Property::AngularVelocity);
                let spin = physics.angular_velocity(e.root);
                let inertia = inertia_of(tree, welds, e.root);
                let rate = (number(tree, id, Property::BodyPower) / inertia).min(1.0 / dt);
                let most = vec(tree, id, Property::BodyMaxTorque) * (dt / inertia);
                physics.drive_spin(e.root, spin + clamp_axes((target - spin) * (rate * dt), most));
            }
            _ => {}
        }
    }
}
