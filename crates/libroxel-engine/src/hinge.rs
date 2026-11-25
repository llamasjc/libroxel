//! Hinges: a `HingeConstraint` joins two parts so that Attachment1's part
//! can only turn about an axis on Attachment0's part. The axis runs along
//! Attachment0's X axis, through Attachment0.
//!
//! Hinges are real joints in the physics engine, so whatever hangs off
//! them swings, spins and rolls for real: a free arm swings under gravity,
//! a wheel on a hinge rolls along the ground and carries its car, and a
//! turning arm shoves what it runs into.
//!
//! The hinge angle (`CurrentAngle`) is how far Attachment1's Y axis is
//! turned from Attachment0's, about Attachment0's X axis. With
//! `ActuatorType` None the hinge turns freely; a Motor turns it at
//! `MotorVelocity` radians a second with up to `MotorMaxTorque`; a Servo
//! turns it to `TargetAngle` degrees at up to `AngularSpeed`, with up to
//! `ServoMaxTorque`.
//!
//! Sliders and ball joints are handled here too. A `PrismaticConstraint`
//! lets Attachment1's part slide along Attachment0's X axis without
//! turning (between `LowerLimit` and `UpperLimit` studs when
//! `LimitsEnabled`), and reports how far along it is as `CurrentPosition`:
//! with a spring beside it, that is suspension. A `BallSocketConstraint`
//! holds the two attachments together and lets the part turn any way.

// A fast fixed hash: these maps are read thousands of times a step.
use bevy::platform::collections::HashMap;

use libroxel_instance::{
    ActuatorType, CFrame, ClassKind, EnumValue, InstanceId, InstanceTree, Property, PropertyValue, Rotation,
    Vec3,
};
use libroxel_script::names;

use crate::physics::{JointKind, JointSpec, Physics, orientation_quat};
use crate::weld::assemblies;

/// The engine's view of every hinge.
#[derive(Debug, Default)]
pub struct Hinges {
    /// Each working hinge, and its two attachments.
    working: HashMap<InstanceId, (InstanceId, InstanceId)>,
    /// Each working slider, and its two attachments.
    sliders: HashMap<InstanceId, (InstanceId, InstanceId)>,
}

/// How far Attachment1 is along Attachment0's X axis, in studs.
fn slid(tree: &InstanceTree, a0: InstanceId, a1: InstanceId) -> Option<f32> {
    let (f0, _) = attachment_frame(tree, a0)?;
    let (f1, _) = attachment_frame(tree, a1)?;
    Some((f1.pos - f0.pos).dot(f0.rot.x))
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

/// An attachment's frame in its part's space, and the part it's on.
pub fn attachment_local(tree: &InstanceTree, attachment: InstanceId) -> Option<(CFrame, InstanceId)> {
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
    Some((local, part))
}

/// An attachment's frame in the world, and the part it's on.
pub fn attachment_frame(tree: &InstanceTree, attachment: InstanceId) -> Option<(CFrame, InstanceId)> {
    let (local, part) = attachment_local(tree, attachment)?;
    Some((cframe(pose(tree, part)?) * local, part))
}

/// A frame as a position and rotation, for the physics engine.
pub(crate) fn frame(cf: CFrame) -> (Vec3, bevy::math::Quat) {
    (cf.pos, orientation_quat(cf.rot.to_orientation()))
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

fn number(tree: &InstanceTree, id: InstanceId, p: Property) -> f32 {
    tree.get_number(id, p).unwrap_or(0.0) as f32
}

/// The hinge angle now, in radians.
fn angle_of(tree: &InstanceTree, a0: InstanceId, a1: InstanceId) -> Option<f32> {
    let (f0, _) = attachment_frame(tree, a0)?;
    let (f1, _) = attachment_frame(tree, a1)?;
    Some(angle_between(&f0, &f1))
}

impl Hinges {
    /// Works out every hinge and its motor, and hands them to physics as
    /// joints (alongside `others`, the other joints this step).
    pub fn before_physics(
        &mut self,
        tree: &InstanceTree,
        physics: &mut Physics,
        dt: f32,
        others: Vec<JointSpec>,
    ) {
        let ws = tree.workspace();
        let groups = assemblies(tree);
        let welded = |a: InstanceId, b: InstanceId| groups.iter().any(|g| g.contains(&a) && g.contains(&b));
        let mut joints = others;
        self.working.clear();
        for hinge in tree.descendants_of_class(ws, ClassKind::HingeConstraint) {
            if tree.get_bool(hinge, Property::Enabled) != Some(true) {
                continue;
            }
            let (Some(a0), Some(a1)) =
                (tree.get_ref(hinge, Property::Attachment0), tree.get_ref(hinge, Property::Attachment1))
            else {
                continue;
            };
            let (Some((l0, p0)), Some((l1, p1))) = (attachment_local(tree, a0), attachment_local(tree, a1))
            else {
                continue;
            };
            let character = |p: InstanceId| tree.name(p) == Ok(names::character::ROOT_PART);
            if p0 == p1 || welded(p0, p1) || character(p0) || character(p1) {
                continue;
            }
            let angle = angle_of(tree, a0, a1).unwrap_or(0.0);
            let actuator = tree.get_enum(hinge, Property::ActuatorType);
            let servo = actuator == Some(EnumValue::ActuatorType(ActuatorType::Servo));
            let motor = match actuator {
                Some(EnumValue::ActuatorType(ActuatorType::Motor)) => Some((
                    number(tree, hinge, Property::MotorVelocity),
                    number(tree, hinge, Property::MotorMaxTorque),
                )),
                Some(EnumValue::ActuatorType(ActuatorType::Servo)) => {
                    let target = number(tree, hinge, Property::TargetAngle).to_radians();
                    let fastest = number(tree, hinge, Property::AngularSpeed).abs();
                    // Head for the target, arriving this step if it is close.
                    let wanted = (wrap(target - angle) / dt.max(1e-4)).clamp(-fastest, fastest);
                    Some((wanted, number(tree, hinge, Property::ServoMaxTorque)))
                }
                _ => None,
            };
            joints.push(JointSpec {
                id: hinge,
                part0: p0,
                part1: p1,
                frame0: frame(l0),
                frame1: frame(l1),
                kind: JointKind::Hinge { motor, servo },
            });
            self.working.insert(hinge, (a0, a1));
        }
        self.sliders.clear();
        for class in [ClassKind::PrismaticConstraint, ClassKind::BallSocketConstraint] {
            for id in tree.descendants_of_class(ws, class) {
                if tree.get_bool(id, Property::Enabled) != Some(true) {
                    continue;
                }
                let (Some(a0), Some(a1)) =
                    (tree.get_ref(id, Property::Attachment0), tree.get_ref(id, Property::Attachment1))
                else {
                    continue;
                };
                let (Some((l0, p0)), Some((l1, p1))) =
                    (attachment_local(tree, a0), attachment_local(tree, a1))
                else {
                    continue;
                };
                let character = |p: InstanceId| tree.name(p) == Ok(names::character::ROOT_PART);
                if p0 == p1 || welded(p0, p1) || character(p0) || character(p1) {
                    continue;
                }
                let kind = if class == ClassKind::PrismaticConstraint {
                    self.sliders.insert(id, (a0, a1));
                    let limits = (tree.get_bool(id, Property::LimitsEnabled) == Some(true)).then(|| {
                        let lo = number(tree, id, Property::LowerLimit);
                        let hi = number(tree, id, Property::UpperLimit);
                        (lo.min(hi), hi.max(lo))
                    });
                    JointKind::Slider { limits }
                } else {
                    JointKind::Ball
                };
                joints.push(JointSpec {
                    id,
                    part0: p0,
                    part1: p1,
                    frame0: frame(l0),
                    frame1: frame(l1),
                    kind,
                });
            }
        }
        physics.set_joints(joints);
    }

    /// Reports every hinge's angle and slider's position. Call after the
    /// physics step.
    pub fn after_physics(&mut self, tree: &mut InstanceTree) {
        for (&hinge, &(a0, a1)) in &self.working {
            let Some(angle) = angle_of(tree, a0, a1) else { continue };
            let degrees = (angle.to_degrees() * 1000.0).round() as f64 / 1000.0;
            if tree.get_number(hinge, Property::CurrentAngle) != Some(degrees) {
                let _ = tree.set(hinge, Property::CurrentAngle, PropertyValue::Number(degrees));
            }
        }
        for (&slider, &(a0, a1)) in &self.sliders {
            let Some(at) = slid(tree, a0, a1) else { continue };
            let studs = (at * 1000.0).round() as f64 / 1000.0;
            if tree.get_number(slider, Property::CurrentPosition) != Some(studs) {
                let _ = tree.set(slider, Property::CurrentPosition, PropertyValue::Number(studs));
            }
        }
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
