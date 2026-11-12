//! The follow camera's feel: how far it sits, how it zooms, and keeping it
//! out of walls. Kept apart from the drawing code so it can be tested.

use libroxel_instance::{InstanceId, InstanceTree, Property, Vec3, geometry};

/// How far behind the character the follow camera starts (studs).
pub const START_DISTANCE: f32 = 12.5;
/// How far it looks down when it starts (radians).
pub const START_PITCH: f32 = -0.35;
/// How close and how far the follow camera can zoom (studs). At the
/// closest it is in the character's head.
pub const MIN_DISTANCE: f32 = 0.5;
pub const MAX_DISTANCE: f32 = 128.0;
/// At or below this, the camera is in first person.
pub const FIRST_PERSON: f32 = 1.0;
/// How high above the root part the camera looks at (studs): the head.
pub const FOCUS_HEIGHT: f32 = 1.5;

/// The distance after zooming by `lines` mouse-wheel notches (in is
/// positive). Each notch moves a share of the way, so zooming feels the
/// same close up and far out; zooming in from close snaps to first person.
pub fn zoom(distance: f32, lines: f32) -> f32 {
    if lines == 0.0 {
        return distance;
    }
    let zoomed = distance * 0.8f32.powf(lines);
    if lines > 0.0 && zoomed < 2.0 {
        return MIN_DISTANCE;
    }
    if lines < 0.0 && distance <= FIRST_PERSON {
        return 2.0f32.max(zoomed);
    }
    zoomed.clamp(MIN_DISTANCE, MAX_DISTANCE)
}

/// Moves a distance towards the one wanted, smoothly but quickly.
pub fn ease(distance: f32, wanted: f32, dt: f32) -> f32 {
    if (wanted - distance).abs() < 0.01 {
        return wanted;
    }
    distance + (wanted - distance) * (1.0 - (-14.0 * dt).exp())
}

/// How far from `focus`, along the unit vector `back`, the camera can sit
/// without a wall between it and the character: `distance`, or closer if
/// something solid is in the way. The character's own model, see-through
/// parts and parts that don't collide never get in the way.
pub fn unblocked(
    tree: &InstanceTree,
    focus: Vec3,
    back: Vec3,
    distance: f32,
    model: Option<InstanceId>,
) -> f32 {
    if distance <= FIRST_PERSON {
        return distance;
    }
    let keep = |part: InstanceId| {
        if model.is_some_and(|m| part == m || tree.is_descendant_of(part, m).unwrap_or(false)) {
            return false;
        }
        if Some(part) == tree.terrain() {
            return true;
        }
        tree.get_bool(part, Property::CanCollide) != Some(false)
            && tree.get_number(part, Property::Transparency).unwrap_or(0.0) < 0.95
    };
    match geometry::raycast(tree, focus, back * distance, keep) {
        // Sit a little in front of the wall, so its face doesn't fill the view.
        Some(hit) => (hit.distance - 0.4).max(FIRST_PERSON.min(distance)),
        None => distance,
    }
}

/// The camera yaw (radians, about the up axis) that looks the way a
/// character with this yaw (degrees, as in its Orientation) faces: from behind.
pub fn behind(character_yaw: f32) -> f32 {
    character_yaw.to_radians()
}

#[cfg(test)]
mod tests {
    use super::*;
    use libroxel_instance::{ClassKind, PropertyValue};

    #[test]
    fn zoom_moves_a_share_and_snaps_into_first_person() {
        assert_eq!(zoom(10.0, 0.0), 10.0);
        assert!((zoom(10.0, 1.0) - 8.0).abs() < 1e-4);
        assert!((zoom(8.0, -1.0) - 10.0).abs() < 1e-4);
        assert_eq!(zoom(2.2, 1.0), MIN_DISTANCE);
        assert_eq!(zoom(MIN_DISTANCE, -1.0), 2.0);
        assert_eq!(zoom(120.0, -3.0), MAX_DISTANCE);
        let mut d = 4.0;
        for _ in 0..60 {
            d = ease(d, 10.0, 1.0 / 60.0);
        }
        assert!((d - 10.0).abs() < 0.01);
    }

    #[test]
    fn walls_pull_the_camera_in_but_the_character_does_not() {
        let mut tree = InstanceTree::new();
        let ws = tree.workspace();
        let wall = tree.create(ClassKind::Part, "Wall").unwrap();
        tree.set_parent(wall, Some(ws)).unwrap();
        tree.set(wall, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, 0.0, 6.0))).unwrap();
        tree.set(wall, Property::Size, PropertyValue::Vec3(Vec3::new(20.0, 20.0, 1.0))).unwrap();
        let back = Vec3::new(0.0, 0.0, 1.0);
        let d = unblocked(&tree, Vec3::ZERO, back, 12.0, None);
        assert!((d - 5.1).abs() < 0.01, "{d}");
        // Glass and ghosts don't count.
        tree.set(wall, Property::Transparency, PropertyValue::Number(1.0)).unwrap();
        assert_eq!(unblocked(&tree, Vec3::ZERO, back, 12.0, None), 12.0);
        tree.set(wall, Property::Transparency, PropertyValue::Number(0.0)).unwrap();
        tree.set(wall, Property::CanCollide, PropertyValue::Bool(false)).unwrap();
        assert_eq!(unblocked(&tree, Vec3::ZERO, back, 12.0, None), 12.0);
        tree.set(wall, Property::CanCollide, PropertyValue::Bool(true)).unwrap();
        // Nor does the character's own body.
        let model = tree.create(ClassKind::Model, "Me").unwrap();
        tree.set_parent(model, Some(ws)).unwrap();
        tree.set_parent(wall, Some(model)).unwrap();
        assert_eq!(unblocked(&tree, Vec3::ZERO, back, 12.0, Some(model)), 12.0);
        // Nothing to do in first person.
        assert_eq!(unblocked(&tree, Vec3::ZERO, back, 0.5, None), 0.5);
    }
}
