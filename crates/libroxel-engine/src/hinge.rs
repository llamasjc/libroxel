//! Hinges: a `HingeConstraint` joins two parts so that one side (the arm)
//! can only turn about an axis on the other (the base). The axis runs along
//! Attachment0's X axis, through Attachment0.
//!
//! The base is Attachment0's part unless only Attachment1's side is
//! anchored. The arm is the other part with everything welded to it. Arms
//! don't move on their own in the physics step: they stand still (things
//! bump into them) and are turned into place afterwards, so they stay on
//! the hinge however fast they go.
//!
//! The hinge angle (`CurrentAngle`) is how far Attachment1's Y axis is
//! turned from Attachment0's, about Attachment0's X axis. With
//! `ActuatorType` None the arm swings under gravity; a Motor turns it at
//! `AngularVelocity` radians a second with up to `MotorMaxTorque`; a Servo
//! turns it to `TargetAngle` degrees at up to `AngularSpeed`, with up to
//! `ServoMaxTorque`.
//!
//! A turning arm shoves loose things it runs into. Round arms (balls, and
//! cylinders whose length runs along the hinge) on a loose base are wheels:
//! where one rests on the ground it holds the base up, and as it turns it
//! drives the base along, gripping the ground so it doesn't slide sideways.

use std::collections::HashMap;

use libroxel_instance::{
    ActuatorType, CFrame, ClassKind, EnumValue, InstanceId, InstanceTree, Property, PropertyValue, Rotation,
    Vec3,
};
use libroxel_script::names;

use crate::physics::{GRAVITY, Physics};

/// How quickly a wheel on the ground brings its base to the wheel's speed
/// (and stops it sliding sideways), per second.
const WHEEL_GRIP: f32 = 10.0;
use crate::weld::assemblies;

/// What a hinge needs to remember between steps.
#[derive(Debug, Clone)]
struct Joint {
    base: InstanceId,
    /// Attachment0's frame in the base part's space: the axis is its X.
    pivot: CFrame,
    /// +1 when the arm holds Attachment1, -1 when it holds Attachment0.
    sign: f32,
    /// The hinge angle when each arm part's rest pose was taken, in radians.
    rest_angle: f32,
    /// Each arm part's pose in the pivot's space, at `rest_angle`.
    rest: Vec<(InstanceId, CFrame)>,
    /// The hinge angle now, and how fast it changes (radians per second).
    angle: f32,
    speed: f32,
    /// Where each arm part was last put, to notice when a script moves one.
    placed: Vec<(InstanceId, (Vec3, Vec3))>,
}

/// The engine's view of every hinge.
#[derive(Debug, Default)]
pub struct Hinges {
    joints: HashMap<InstanceId, Joint>,
    /// Hinges in the order to place them: a base before the arms on it.
    order: Vec<InstanceId>,
}

fn pose(tree: &InstanceTree, part: InstanceId) -> Option<(Vec3, Vec3)> {
    Some((
        tree.get_vec3(part, Property::Position)?,
        tree.get_vec3(part, Property::Orientation).unwrap_or_default(),
    ))
}

fn cframe(pose: (Vec3, Vec3)) -> CFrame {
    CFrame::new(pose.0, Rotation::from_orientation(pose.1))
}

/// An attachment's frame in the world, and the part it's on.
pub fn attachment_frame(tree: &InstanceTree, attachment: InstanceId) -> Option<(CFrame, InstanceId)> {
    if tree.class(attachment).ok()? != ClassKind::Attachment || tree.is_destroyed(attachment).ok()? {
        return None;
    }
    let part = tree.parent(attachment).ok()??;
    if !tree.class(part).ok()?.is_physical() {
        return None;
    }
    let local = CFrame::new(
        tree.get_vec3(attachment, Property::Position).unwrap_or_default(),
        Rotation::from_orientation(tree.get_vec3(attachment, Property::Orientation).unwrap_or_default()),
    );
    Some((cframe(pose(tree, part)?) * local, part))
}

/// How far `b`'s Y axis is turned from `a`'s about `a`'s X axis, in radians.
fn angle_between(a: &CFrame, b: &CFrame) -> f32 {
    let (x, y0, y1) = (a.rot.x, a.rot.y, b.rot.y);
    x.dot(y0.cross(y1)).atan2(y0.dot(y1))
}

/// The shortest turn from `from` to `to`, in radians.
fn wrap(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let a = (a + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
    if a <= -std::f32::consts::PI { a + tau } else { a }
}

fn mass_of(tree: &InstanceTree, part: InstanceId) -> f32 {
    let s = tree.get_vec3(part, Property::Size).unwrap_or_default();
    (s.x * s.y * s.z * names::extras::DENSITY).max(0.01)
}

fn number(tree: &InstanceTree, id: InstanceId, p: Property) -> f32 {
    tree.get_number(id, p).unwrap_or(0.0) as f32
}

/// What the wheels on one loose base add up to this step.
#[derive(Debug, Default)]
struct Drive {
    /// How many wheels touch the ground.
    wheels: f32,
    /// The wheels' forward directions, added up.
    ahead: Vec3,
    /// The wheels' rolling speeds along their forward directions, added up.
    speed: f32,
    /// How far the deepest wheel has sunk into the ground.
    sunk: f32,
    /// For turning: each wheel's push about the base's middle, added up,
    /// and how far out the wheels are (squared), added up.
    twist: f32,
    reach: f32,
}

/// Where a wheel meets the ground.
struct Contact {
    point: Vec3,
    /// How far into the ground the wheel reaches (negative: just above it).
    sunk: f32,
}

/// If `part` is a wheel turning about `axis` (a ball, or a cylinder lying
/// along it) and it is on the ground (not `car`), where it touches.
fn wheel_contact(tree: &InstanceTree, part: InstanceId, axis: Vec3, car: &[InstanceId]) -> Option<Contact> {
    let size = tree.get_vec3(part, Property::Size)?;
    let at = tree.get_vec3(part, Property::Position)?;
    let radius = match tree.get_enum(part, Property::Shape) {
        Some(EnumValue::Shape(libroxel_instance::PartShape::Ball)) => size.x.min(size.y).min(size.z) / 2.0,
        Some(EnumValue::Shape(libroxel_instance::PartShape::Cylinder)) => {
            let along =
                Rotation::from_orientation(tree.get_vec3(part, Property::Orientation).unwrap_or_default())
                    .apply(Vec3::new(1.0, 0.0, 0.0));
            if along.dot(axis).abs() < 0.9 {
                return None;
            }
            size.y.min(size.z) / 2.0
        }
        _ => return None,
    };
    // Rolling needs an axis that lies roughly flat.
    if axis.y.abs() > 0.5 {
        return None;
    }
    let reach = radius + 0.1;
    let hit = libroxel_instance::geometry::raycast(tree, at, Vec3::new(0.0, -reach, 0.0), |p| {
        !car.contains(&p) && tree.get_bool(p, Property::CanCollide) != Some(false)
    })?;
    Some(Contact { point: hit.position, sunk: radius - hit.distance })
}

/// Changes `speed` towards `wanted` by at most `most`.
fn approach(speed: f32, wanted: f32, most: f32) -> f32 {
    speed + (wanted - speed).clamp(-most, most)
}

impl Hinges {
    /// Works out every hinge's arm, turns the arms' speeds by gravity and
    /// their actuators, and tells physics to hold the arms still.
    pub fn before_physics(&mut self, tree: &InstanceTree, physics: &mut Physics, dt: f32) {
        let ws = tree.workspace();
        let hinges: Vec<InstanceId> = tree
            .descendants_of_class(ws, ClassKind::HingeConstraint)
            .into_iter()
            .filter(|h| tree.get_bool(*h, Property::Enabled) == Some(true))
            .collect();
        if hinges.is_empty() {
            self.joints.clear();
            self.order.clear();
            return;
        }
        let groups = assemblies(tree);
        let group_of = |part: InstanceId| -> Vec<InstanceId> {
            groups.iter().find(|g| g.contains(&part)).cloned().unwrap_or_else(|| vec![part])
        };
        let anchored =
            |parts: &[InstanceId]| parts.iter().any(|p| tree.get_bool(*p, Property::Anchored) != Some(false));

        let mut joints = HashMap::new();
        // Arm parts of each hinge, to order them and keep arms apart.
        let mut arms: Vec<(InstanceId, InstanceId, Vec<InstanceId>)> = Vec::new();
        for hinge in hinges {
            let a0 = tree.get_ref(hinge, Property::Attachment0);
            let a1 = tree.get_ref(hinge, Property::Attachment1);
            let (Some((f0, p0)), Some((f1, p1))) =
                (a0.and_then(|a| attachment_frame(tree, a)), a1.and_then(|a| attachment_frame(tree, a)))
            else {
                continue;
            };
            let (g0, g1) = (group_of(p0), group_of(p1));
            if g0.contains(&p1) {
                continue; // welded together: nothing can turn
            }
            let swap = anchored(&g1) && !anchored(&g0);
            let (base, arm, sign) = if swap { (p1, g0, -1.0) } else { (p0, g1, 1.0) };
            if anchored(&arm) || arm.iter().any(|p| tree.name(*p) == Ok(names::character::ROOT_PART)) {
                continue;
            }
            let Some(base_cf) = pose(tree, base).map(cframe) else { continue };
            let angle = angle_between(&f0, &f1);
            let now: Vec<(InstanceId, (Vec3, Vec3))> =
                arm.iter().filter_map(|p| Some((*p, pose(tree, *p)?))).collect();
            let joint = match self.joints.remove(&hinge) {
                // Same hinge, same parts, and nobody moved the arm.
                Some(j) if j.base == base && j.sign == sign && j.placed == now => j,
                old => {
                    let pivot_world = f0;
                    let rest = now.iter().map(|(p, at)| (*p, pivot_world.inverse() * cframe(*at))).collect();
                    Joint {
                        base,
                        pivot: base_cf.inverse() * pivot_world,
                        sign,
                        rest_angle: angle,
                        rest,
                        angle,
                        speed: old.map_or(0.0, |j| j.speed),
                        placed: now,
                    }
                }
            };
            arms.push((hinge, base, arm));
            joints.insert(hinge, joint);
        }

        // Turn each arm: gravity, then whatever drives the hinge.
        let gravity = tree.get_number(ws, Property::Gravity).map_or(GRAVITY, |g| g as f32);
        for (hinge, joint) in joints.iter_mut() {
            let Some(base_cf) = pose(tree, joint.base).map(cframe) else { continue };
            let pivot = base_cf * joint.pivot;
            let axis = pivot.rot.x;
            let (mut inertia, mut torque) = (0.0, 0.0);
            for (part, at) in &joint.placed {
                let m = mass_of(tree, *part);
                let s = tree.get_vec3(*part, Property::Size).unwrap_or_default();
                let r = at.0 - pivot.pos;
                let off_axis = r - axis * r.dot(axis);
                inertia += m * (off_axis.dot(off_axis) + s.dot(s) / 18.0);
                torque += axis.dot(r.cross(Vec3::new(0.0, -gravity * m, 0.0)));
            }
            let inertia = inertia.max(0.01);
            // Torque turns the arm; the hinge angle goes the same way or the other.
            let mut speed = joint.speed + joint.sign * torque / inertia * dt;
            match tree.get_enum(*hinge, Property::ActuatorType) {
                Some(EnumValue::ActuatorType(ActuatorType::Motor)) => {
                    let wanted = number(tree, *hinge, Property::MotorVelocity);
                    let most = number(tree, *hinge, Property::MotorMaxTorque) / inertia * dt;
                    speed = approach(speed, wanted, most);
                }
                Some(EnumValue::ActuatorType(ActuatorType::Servo)) => {
                    let target = number(tree, *hinge, Property::TargetAngle).to_radians();
                    let fastest = number(tree, *hinge, Property::AngularSpeed).abs();
                    let wanted = (wrap(target - joint.angle) / dt.max(1e-4)).clamp(-fastest, fastest);
                    let most = number(tree, *hinge, Property::ServoMaxTorque) / inertia * dt;
                    speed = approach(speed, wanted, most);
                }
                _ => speed *= (1.0 - 0.2 * dt).max(0.0),
            }
            joint.speed = speed;
            joint.angle = wrap(joint.angle + speed * dt);
        }

        // Bases before the arms that hang from them.
        let mut order = Vec::new();
        let mut left = arms.clone();
        while !left.is_empty() {
            let before = left.len();
            left.retain(|(hinge, base, _)| {
                let waits = arms
                    .iter()
                    .any(|(other, _, arm)| other != hinge && arm.contains(base) && !order.contains(other));
                if !waits {
                    order.push(*hinge);
                }
                waits
            });
            if left.len() == before {
                // A loop of hinges: place the rest in any order.
                order.extend(left.drain(..).map(|(h, _, _)| h));
            }
        }
        // Wheels hold up and drive their bases, then every arm is held still
        // for the step, shoving what it meets as it turns.
        let mut drives: HashMap<InstanceId, Drive> = HashMap::new();
        for (hinge, base, arm) in &arms {
            let base_parts = group_of(*base);
            let mover = base_parts.iter().copied().find(|p| !physics.is_carried(*p)).unwrap_or(*base);
            let Some(joint) = joints.get(hinge) else { continue };
            let Some(base_cf) = pose(tree, joint.base).map(cframe) else { continue };
            let pivot = base_cf * joint.pivot;
            let spin = pivot.rot.x * (joint.sign * joint.speed);
            let base_velocity = if anchored(&base_parts) { Vec3::ZERO } else { physics.velocity(mover) };
            if !anchored(&base_parts) {
                let car: Vec<InstanceId> = arms
                    .iter()
                    .filter(|(_, b, _)| base_parts.contains(b))
                    .flat_map(|(_, _, a)| a.iter().copied())
                    .chain(base_parts.iter().copied())
                    .collect();
                for &part in arm {
                    if let Some(contact) = wheel_contact(tree, part, pivot.rot.x, &car) {
                        let drive = drives.entry(mover).or_default();
                        // The bottom of the wheel moves backwards as it turns,
                        // so on the ground the base goes the other way.
                        let r = contact.point - pivot.pos;
                        let rim = spin.cross(r - pivot.rot.x * r.dot(pivot.rot.x));
                        let mut ahead = pivot.rot.x.cross(Vec3::new(0.0, 1.0, 0.0)).normalize_or_zero();
                        // Hinges on either side of a car often face opposite ways.
                        if ahead.dot(drive.ahead) < 0.0 {
                            ahead = ahead * -1.0;
                        }
                        drive.wheels += 1.0;
                        drive.ahead = drive.ahead + ahead;
                        let roll = -rim.dot(ahead);
                        drive.speed += roll;
                        // Wheels rolling at different speeds turn the base,
                        // like a tank's tracks.
                        if let Some(middle) = tree.get_vec3(mover, Property::Position) {
                            let out = Vec3::new(contact.point.x - middle.x, 0.0, contact.point.z - middle.z);
                            let push = ahead * roll;
                            drive.twist += out.z * push.x - out.x * push.z;
                            drive.reach += out.dot(out);
                        }
                        drive.sunk =
                            if drive.wheels > 1.0 { drive.sunk.max(contact.sunk) } else { contact.sunk };
                    }
                }
            }
            for &part in arm {
                physics.carry(part, &base_parts);
                physics.set_motion(part, pivot.pos, spin, base_velocity);
            }
        }
        let gravity = tree.get_number(ws, Property::Gravity).map_or(GRAVITY, |g| g as f32);
        for (mover, drive) in drives {
            let ahead = (drive.ahead * (1.0 / drive.wheels)).normalize_or_zero();
            let mut v = physics.velocity(mover);
            // Held up by the wheels: rise out of the ground, and cancel this
            // step's fall (gravity is added again in the step).
            if drive.sunk > -0.02 {
                v.y = v.y.max(0.0).max(drive.sunk.max(0.0) / dt.max(1e-4)) + gravity * dt;
            }
            let grip = (WHEEL_GRIP * dt).min(1.0);
            let flat = Vec3::new(v.x, 0.0, v.z);
            let along = flat.dot(ahead);
            let side = flat - ahead * along;
            let wanted = drive.speed / drive.wheels;
            let along = along + (wanted - along) * grip;
            let side = side * (1.0 - grip);
            let flat = ahead * along + side;
            physics.set_velocity(mover, Vec3::new(flat.x, v.y, flat.z));
            // Turning about the vertical, towards what the wheels ask for.
            let spin = physics.angular_velocity(mover);
            let turn = if drive.reach > 1e-3 { drive.twist / drive.reach } else { 0.0 };
            let yaw = spin.y + (turn - spin.y) * grip;
            physics.drive_spin(mover, Vec3::new(spin.x * (1.0 - grip), yaw, spin.z * (1.0 - grip)));
        }
        self.joints = joints;
        self.order = order;
    }

    /// Turns every arm to its hinge's angle, and reports the angles.
    /// Call after the physics step and after welds place their parts.
    pub fn after_physics(&mut self, tree: &mut InstanceTree) {
        for hinge in &self.order {
            let Some(joint) = self.joints.get_mut(hinge) else { continue };
            let Some(base_cf) = pose(tree, joint.base).map(cframe) else { continue };
            let turn = Rotation::about_x(joint.sign * (joint.angle - joint.rest_angle));
            let pivot = base_cf * joint.pivot * CFrame::new(Vec3::ZERO, turn);
            joint.placed.clear();
            for (part, rest) in &joint.rest {
                let cf = pivot * *rest;
                let at = (cf.pos, cf.rot.to_orientation());
                if pose(tree, *part) != Some(at) {
                    let _ = tree.set(*part, Property::Position, PropertyValue::Vec3(at.0));
                    let _ = tree.set(*part, Property::Orientation, PropertyValue::Vec3(at.1));
                }
                joint.placed.push((*part, at));
            }
            let degrees = (joint.angle.to_degrees() * 1000.0).round() as f64 / 1000.0;
            if tree.get_number(*hinge, Property::CurrentAngle) != Some(degrees) {
                let _ = tree.set(*hinge, Property::CurrentAngle, PropertyValue::Number(degrees));
            }
        }
    }

    /// The hinge angle of `hinge` in radians, if it's working.
    pub fn angle(&self, hinge: InstanceId) -> Option<f32> {
        self.joints.get(&hinge).map(|j| j.angle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_wrap_the_short_way() {
        let pi = std::f32::consts::PI;
        assert!((wrap(3.0 * pi / 2.0) + pi / 2.0).abs() < 1e-5);
        assert!((wrap(-3.0 * pi / 2.0) - pi / 2.0).abs() < 1e-5);
        assert!((wrap(0.25) - 0.25).abs() < 1e-6);
    }
}
