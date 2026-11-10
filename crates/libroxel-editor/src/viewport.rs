//! The 3D view: picking, the move/scale gizmo, the selection box and grid.

use bevy::prelude::*;
use libroxel_engine::view::EngineState;
use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue};
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
    let inv = Vec3::new(1.0 / dir.x, 1.0 / dir.y, 1.0 / dir.z);
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
        let (min, max) = (to_bevy(pos) - to_bevy(size) / 2.0, to_bevy(pos) + to_bevy(size) / 2.0);
        // Slab test.
        let t1 = (min - origin) * inv;
        let t2 = (max - origin) * inv;
        let near = t1.min(t2).max_element();
        let far = t1.max(t2).min_element();
        if far >= near.max(0.0) {
            let t = near.max(0.0);
            if best.is_none_or(|(bt, _)| t < bt) {
                best = Some((t, id));
            }
        }
    }
    best.map(|(_, id)| id)
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
    let (cam, gt) = camera;
    let modes = match editor.tool {
        Tool::Scale => GizmoMode::all_scale(),
        _ => GizmoMode::all_translate(),
    };
    editor.gizmo.update_config(GizmoConfig {
        view_matrix: row_matrix(gt.to_matrix().inverse()),
        projection_matrix: row_matrix(cam.clip_from_view()),
        viewport,
        modes,
        orientation: GizmoOrientation::Global,
        snapping: editor.snap > 0.0,
        snap_distance: editor.snap.max(0.01),
        snap_scale: 0.25,
        ..*editor.gizmo.config()
    });
    let target = transform_gizmo_egui::math::Transform {
        scale: v3(to_bevy(size)),
        rotation: mint::Quaternion { v: mint::Vector3 { x: 0.0, y: 0.0, z: 0.0 }, s: 1.0 },
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
            _ => {
                let pos = from_bevy(from_v3(new.translation));
                let _ = tree.set(id, Property::Position, PropertyValue::Vec3(pos));
            }
        }
    }
    active
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
    let transform = Transform::from_translation(to_bevy(pos)).with_scale(to_bevy(size) + Vec3::splat(0.08));
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
