//! The 3D view: picking, the move/scale/rotate gizmo, the selection box and grid.

use bevy::prelude::*;
use libroxel_engine::physics::orientation_quat;
use libroxel_engine::view::{EngineState, part_shape};
use libroxel_instance::{ClassKind, InstanceId, InstanceTree, PartShape, Property, PropertyValue};
use transform_gizmo_egui::mint;

use crate::{Editor, Tool};

fn to_bevy(v: libroxel_instance::Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn from_bevy(v: Vec3) -> libroxel_instance::Vec3 {
    libroxel_instance::Vec3::new(v.x, v.y, v.z)
}

/// The nearest part under a ray, if any.
pub fn pick(tree: &InstanceTree, ray: Ray3d) -> Option<InstanceId> {
    let origin = ray.origin;
    let dir = ray.direction.as_vec3();
    let mut best: Option<(f32, InstanceId)> = None;
    for id in tree.descendants_of_class(tree.workspace(), ClassKind::Part) {
        let (Some(pos), Some(size)) =
            (tree.get_vec3(id, Property::Position), tree.get_vec3(id, Property::Size))
        else {
            continue;
        };
        if tree.get_number(id, Property::Transparency).unwrap_or(0.0) >= 1.0 {
            continue;
        }
        // Work in the part's own space, where it is an upright box.
        let rotation = orientation_quat(tree.get_vec3(id, Property::Orientation).unwrap_or_default());
        let inverse = rotation.inverse();
        let local_origin = inverse * (origin - to_bevy(pos));
        let local_dir = inverse * dir;
        let half = to_bevy(size) / 2.0;
        let hit = if part_shape(tree, id) == PartShape::Ball {
            ray_sphere(local_origin, local_dir, half.min_element())
        } else {
            ray_box(local_origin, local_dir, half)
        };
        if let Some(t) = hit
            && best.is_none_or(|(bt, _)| t < bt)
        {
            best = Some((t, id));
        }
    }
    best.map(|(_, id)| id)
}

/// Distance along the ray to a box centred on the origin (slab test).
fn ray_box(origin: Vec3, dir: Vec3, half: Vec3) -> Option<f32> {
    let inv = Vec3::new(1.0 / dir.x, 1.0 / dir.y, 1.0 / dir.z);
    let t1 = (-half - origin) * inv;
    let t2 = (half - origin) * inv;
    let near = t1.min(t2).max_element();
    let far = t1.max(t2).min_element();
    (far >= near.max(0.0)).then(|| near.max(0.0))
}

/// Distance along the ray to a sphere centred on the origin.
fn ray_sphere(origin: Vec3, dir: Vec3, radius: f32) -> Option<f32> {
    let b = origin.dot(dir);
    let c = origin.length_squared() - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let far = -b + disc.sqrt();
    (far >= 0.0).then(|| (-b - disc.sqrt()).max(0.0))
}

fn row_matrix(m: Mat4) -> mint::RowMatrix4<f64> {
    let cols = m.to_cols_array_2d();
    let mut rows = [[0.0f64; 4]; 4];
    for (c, col) in cols.iter().enumerate() {
        for (r, value) in col.iter().enumerate() {
            rows[r][c] = *value as f64;
        }
    }
    rows.into()
}

fn v3(v: Vec3) -> mint::Vector3<f64> {
    mint::Vector3 { x: v.x as f64, y: v.y as f64, z: v.z as f64 }
}

fn from_v3(v: mint::Vector3<f64>) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, v.z as f32)
}

/// Draws the gizmo for the selected part and applies any drag. Returns
/// whether the gizmo is under the pointer or being dragged.
pub fn gizmo(
    ui: &bevy_egui::egui::Ui,
    editor: &mut Editor,
    tree: &mut InstanceTree,
    camera: (&Camera, &GlobalTransform),
    viewport: bevy_egui::egui::Rect,
    now: f64,
    editing_allowed: bool,
) -> bool {
    use transform_gizmo_egui::{GizmoConfig, GizmoExt, GizmoMode, GizmoOrientation, GizmoResult};

    let Some(id) = editor.selection else { return false };
    if editor.tool == Tool::Select || !editing_allowed {
        return false;
    }
    let (Some(pos), Some(size)) = (tree.get_vec3(id, Property::Position), tree.get_vec3(id, Property::Size))
    else {
        return false;
    };
    let orientation = tree.get_vec3(id, Property::Orientation);
    let rotation = orientation_quat(orientation.unwrap_or_default());
    let (cam, gt) = camera;
    let (modes, gizmo_orientation, snapping) = match editor.tool {
        Tool::Scale => (GizmoMode::all_scale(), GizmoOrientation::Local, editor.snap > 0.0),
        Tool::Rotate if orientation.is_some() => {
            (GizmoMode::all_rotate(), GizmoOrientation::Local, editor.angle_snap > 0.0)
        }
        Tool::Rotate => return false,
        _ => (GizmoMode::all_translate(), GizmoOrientation::Global, editor.snap > 0.0),
    };
    editor.gizmo.update_config(GizmoConfig {
        view_matrix: row_matrix(gt.to_matrix().inverse()),
        projection_matrix: row_matrix(cam.clip_from_view()),
        viewport,
        modes,
        orientation: gizmo_orientation,
        snapping,
        snap_distance: editor.snap.max(0.01),
        snap_scale: 0.25,
        snap_angle: editor.angle_snap.max(1.0).to_radians(),
        ..*editor.gizmo.config()
    });
    let target = transform_gizmo_egui::math::Transform {
        scale: v3(to_bevy(size)),
        rotation: mint::Quaternion {
            v: mint::Vector3 { x: rotation.x as f64, y: rotation.y as f64, z: rotation.z as f64 },
            s: rotation.w as f64,
        },
        translation: v3(to_bevy(pos)),
    };
    let result = editor.gizmo.interact(ui, &[target]);
    let active = editor.gizmo.is_focused() || result.is_some();
    if let Some((result, transforms)) = result
        && let Some(new) = transforms.first()
    {
        editor.history.begin_edit(tree, now);
        editor.dirty = true;
        match result {
            GizmoResult::Scale { .. } => {
                let size = from_bevy(from_v3(new.scale).max(Vec3::splat(0.05)));
                let _ = tree.set(id, Property::Size, PropertyValue::Vec3(size));
            }
            GizmoResult::Rotation { .. } | GizmoResult::Arcball { .. } => {
                let q = new.rotation;
                let quat = Quat::from_xyzw(q.v.x as f32, q.v.y as f32, q.v.z as f32, q.s as f32).normalize();
                let _ = tree.set(id, Property::Orientation, PropertyValue::Vec3(degrees_from_quat(quat)));
            }
            _ => {
                let pos = from_bevy(from_v3(new.translation));
                let _ = tree.set(id, Property::Position, PropertyValue::Vec3(pos));
            }
        }
    }
    active
}

/// The orientation (degrees around Y, then X, then Z) for a rotation,
/// rounded so repeated drags don't collect float noise.
pub fn degrees_from_quat(q: Quat) -> libroxel_instance::Vec3 {
    let (y, x, z) = q.to_euler(EulerRot::YXZ);
    let round = |r: f32| {
        let d = (r.to_degrees() * 100.0).round() / 100.0;
        if d == -0.0 { 0.0 } else { d }
    };
    libroxel_instance::Vec3::new(round(x), round(y), round(z))
}

/// Outlines the selected part.
pub fn draw_selection(editor: NonSend<Editor>, engine: NonSend<EngineState>, mut gizmos: Gizmos) {
    let Some(id) = editor.selection else { return };
    let tree = engine.tree.borrow();
    if tree.is_destroyed(id).unwrap_or(true) {
        return;
    }
    let (Some(pos), Some(size)) = (tree.get_vec3(id, Property::Position), tree.get_vec3(id, Property::Size))
    else {
        return;
    };
    let rotation = orientation_quat(tree.get_vec3(id, Property::Orientation).unwrap_or_default());
    let transform = Transform::from_translation(to_bevy(pos))
        .with_rotation(rotation)
        .with_scale(to_bevy(size) + Vec3::splat(0.08));
    gizmos.cube(transform, Color::srgb(1.0, 0.8, 0.1));
}

/// A faint grid at ground level while editing.
pub fn draw_grid(editor: NonSend<Editor>, engine: NonSend<EngineState>, mut gizmos: Gizmos) {
    if engine.is_playing() || !editor.show_grid {
        return;
    }
    let spacing = if editor.snap >= 1.0 { editor.snap * 4.0 } else { 4.0 };
    gizmos.grid(
        Isometry3d::new(Vec3::new(0.0, 0.02, 0.0), Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        UVec2::splat(32),
        Vec2::splat(spacing),
        Color::srgba(1.0, 1.0, 1.0, 0.08),
    );
}

/// Moves the fly camera to look at the selection.
pub fn frame_selection(tree: &InstanceTree, id: InstanceId, transform: &mut Transform) {
    let (Some(pos), Some(size)) = (tree.get_vec3(id, Property::Position), tree.get_vec3(id, Property::Size))
    else {
        return;
    };
    let radius = to_bevy(size).length().max(2.0);
    let back = transform.forward().as_vec3() * radius * 2.0;
    transform.translation = to_bevy(pos) - back;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_round_trips_through_quaternions() {
        for degrees in [[0.0, 45.0, 0.0], [30.0, 0.0, 0.0], [10.0, 20.0, 30.0], [0.0, 180.0, 0.0]] {
            let v = libroxel_instance::Vec3::new(degrees[0], degrees[1], degrees[2]);
            let back = degrees_from_quat(orientation_quat(v));
            let q1 = orientation_quat(v);
            let q2 = orientation_quat(back);
            assert!(q1.dot(q2).abs() > 0.9999, "{v:?} came back as {back:?}");
        }
    }

    #[test]
    fn picking_respects_rotation_and_balls() {
        let mut tree = InstanceTree::new();
        let ws = tree.workspace();
        let beam = tree.create(ClassKind::Part, "Beam").unwrap();
        tree.set_parent(beam, Some(ws)).unwrap();
        tree.set(beam, Property::Size, PropertyValue::Vec3(libroxel_instance::Vec3::new(20.0, 1.0, 1.0)))
            .unwrap();
        tree.set(
            beam,
            Property::Orientation,
            PropertyValue::Vec3(libroxel_instance::Vec3::new(0.0, 90.0, 0.0)),
        )
        .unwrap();
        let down = |x: f32, z: f32| Ray3d::new(Vec3::new(x, 10.0, z), Dir3::NEG_Y);
        assert_eq!(pick(&tree, down(0.0, 8.0)), Some(beam), "the turned beam runs along Z");
        assert_eq!(pick(&tree, down(8.0, 0.0)), None);

        let ball = tree.create(ClassKind::Part, "Ball").unwrap();
        tree.set_parent(ball, Some(ws)).unwrap();
        tree.set(ball, Property::Position, PropertyValue::Vec3(libroxel_instance::Vec3::new(50.0, 0.0, 0.0)))
            .unwrap();
        tree.set(ball, Property::Size, PropertyValue::Vec3(libroxel_instance::Vec3::new(4.0, 4.0, 4.0)))
            .unwrap();
        let shape = PropertyValue::Enum(libroxel_instance::EnumValue::Shape(PartShape::Ball));
        tree.set(ball, Property::Shape, shape).unwrap();
        assert_eq!(pick(&tree, down(50.0, 0.0)), Some(ball));
        assert_eq!(pick(&tree, down(51.8, 1.8)), None, "the corner of the box is empty");
    }
}
