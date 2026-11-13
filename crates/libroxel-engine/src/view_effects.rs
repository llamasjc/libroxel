//! Drawing the look classes: bloom and colour correction (in Lighting or
//! the camera), selection boxes and beams. Atmosphere is drawn with the
//! fog, in `view::update_lighting`.

use std::collections::HashMap;

use bevy::post_process::bloom::{Bloom, BloomPrefilter};
use bevy::prelude::*;
use bevy::render::view::ColorGrading;
use libroxel_instance::geometry::Region;
use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property};

use crate::view::{CameraRig, EngineSet, EngineState, PartMeshes, to_bevy};

pub(crate) fn build(app: &mut App) {
    app.add_systems(Update, (sync_screen_effects, draw_selection_boxes, sync_beams).after(EngineSet::Sync));
}

/// The first enabled `class` inside Lighting or the workspace's camera.
fn effect(tree: &InstanceTree, class: ClassKind) -> Option<InstanceId> {
    let ws = tree.workspace();
    let camera = tree.find_first_child_of_class(ws, ClassKind::Camera).ok().flatten();
    [tree.service(ClassKind::Lighting), camera]
        .into_iter()
        .flatten()
        .flat_map(|holder| tree.children(holder).map(|c| c.to_vec()).unwrap_or_default())
        .find(|c| tree.class(*c) == Ok(class) && tree.get_bool(*c, Property::Enabled) != Some(false))
}

/// How strong, how wide and from how bright bloom is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BloomLook {
    pub intensity: f32,
    pub spread: f32,
    pub threshold: f32,
}

/// The bloom the tree asks for, if any.
pub fn bloom_look(tree: &InstanceTree) -> Option<BloomLook> {
    let id = effect(tree, ClassKind::BloomEffect)?;
    let n = |p, d: f64| tree.get_number(id, p).unwrap_or(d) as f32;
    Some(BloomLook {
        // A bloom of intensity 1 is a soft glow; 0 is none.
        intensity: (n(Property::BloomIntensity, 1.0) * 0.25).clamp(0.0, 1.0),
        spread: (n(Property::BloomSize, 24.0) / 56.0).clamp(0.0, 0.95),
        threshold: (n(Property::Threshold, 2.0) * 0.5).max(0.0),
    })
}

/// The colour grading the tree's colour correction asks for (the default
/// when there's none).
pub fn grading(tree: &InstanceTree) -> ColorGrading {
    let mut out = ColorGrading::default();
    let Some(id) = effect(tree, ClassKind::ColorCorrectionEffect) else { return out };
    let n = |p| tree.get_number(id, p).unwrap_or(0.0) as f32;
    out.global.exposure = n(Property::Brightness) * 2.0;
    out.global.post_saturation = (1.0 + n(Property::Saturation)).max(0.0);
    let contrast = (1.0 + n(Property::Contrast)).max(0.0);
    for section in [&mut out.shadows, &mut out.midtones, &mut out.highlights] {
        section.contrast = contrast;
    }
    // A tint warms (more red) or cools (more blue) the picture, and tints
    // it green or magenta.
    if let Some(tint) = tree.get_color(id, Property::TintColor) {
        out.global.temperature = (tint.r - tint.b).clamp(-1.0, 1.0);
        out.global.tint = ((tint.r + tint.b) / 2.0 - tint.g).clamp(-1.0, 1.0);
        let strongest = tint.r.max(tint.g).max(tint.b);
        if strongest < 1.0 {
            out.global.exposure += strongest.max(0.01).log2();
        }
    }
    out
}

#[allow(clippy::type_complexity)]
fn sync_screen_effects(
    engine: NonSend<EngineState>,
    mut commands: Commands,
    cameras: Query<(Entity, Option<&Bloom>, Option<&ColorGrading>), With<CameraRig>>,
    mut shown: Local<Option<BloomLook>>,
) {
    let tree = engine.tree.borrow();
    let wanted = bloom_look(&tree);
    let grade = grading(&tree);
    for (entity, bloom, current) in &cameras {
        if wanted != *shown || (wanted.is_some() != bloom.is_some()) {
            match wanted {
                Some(look) => {
                    commands.entity(entity).insert(Bloom {
                        intensity: look.intensity,
                        low_frequency_boost: look.spread,
                        prefilter: BloomPrefilter { threshold: look.threshold, threshold_softness: 0.3 },
                        ..Bloom::NATURAL
                    });
                }
                None => {
                    commands.entity(entity).remove::<Bloom>();
                }
            }
        }
        if current != Some(&grade) {
            commands.entity(entity).insert(grade.clone());
        }
    }
    *shown = wanted;
}

/// The smallest upright box round `id`: a part's box, or everything in a model.
fn bounds(tree: &InstanceTree, id: InstanceId) -> Option<(libroxel_instance::Vec3, libroxel_instance::Vec3)> {
    let class = tree.class(id).ok()?;
    let parts = if class.is_physical() {
        vec![id]
    } else {
        tree.descendants(id)
            .ok()?
            .into_iter()
            .filter(|d| tree.class(*d).is_ok_and(|c| c.is_physical()))
            .collect()
    };
    let mut out: Option<(libroxel_instance::Vec3, libroxel_instance::Vec3)> = None;
    for p in parts {
        let Some((lo, hi)) = Region::of_part(tree, p).map(Region::bounds) else { continue };
        out = Some(match out {
            None => (lo, hi),
            Some((a, b)) => (
                libroxel_instance::Vec3::new(a.x.min(lo.x), a.y.min(lo.y), a.z.min(lo.z)),
                libroxel_instance::Vec3::new(b.x.max(hi.x), b.y.max(hi.y), b.z.max(hi.z)),
            ),
        });
    }
    out
}

/// Each visible selection box: its colour and the box it outlines (the
/// Adornee's, or its parent's when that is empty).
pub fn selection_boxes(tree: &InstanceTree) -> Vec<(libroxel_instance::Color3, Vec3, Vec3)> {
    let mut out = Vec::new();
    for id in tree.descendants_of_class(tree.root(), ClassKind::SelectionBox) {
        if tree.get_bool(id, Property::Visible) == Some(false) {
            continue;
        }
        let target = tree.get_ref(id, Property::Adornee).or_else(|| tree.parent(id).ok().flatten());
        let Some((lo, hi)) = target.and_then(|t| bounds(tree, t)) else { continue };
        let colour = tree.get_color(id, Property::SelectionColor).unwrap_or(libroxel_instance::Color3::WHITE);
        let pad = tree.get_number(id, Property::LineThickness).unwrap_or(0.15) as f32 / 2.0;
        let (lo, hi) = (to_bevy(lo) - Vec3::splat(pad), to_bevy(hi) + Vec3::splat(pad));
        out.push((colour, (lo + hi) / 2.0, hi - lo));
    }
    out
}

fn draw_selection_boxes(engine: NonSend<EngineState>, mut gizmos: Gizmos) {
    let tree = engine.tree.borrow();
    for (c, center, size) in selection_boxes(&tree) {
        gizmos.cube(Transform::from_translation(center).with_scale(size), Color::srgb(c.r, c.g, c.b));
    }
}

#[derive(Component)]
struct BeamLink(InstanceId, (libroxel_instance::Color3, u32, u32));

/// Each enabled beam: where it runs from and to, its widths, colour,
/// transparency and glow.
pub struct BeamShape {
    pub id: InstanceId,
    pub from: Vec3,
    pub to: Vec3,
    pub width: f32,
    pub colour: libroxel_instance::Color3,
    pub transparency: f32,
    pub glow: f32,
}

pub fn beams(tree: &InstanceTree) -> Vec<BeamShape> {
    let mut out = Vec::new();
    for id in tree.descendants_of_class(tree.workspace(), ClassKind::Beam) {
        if tree.get_bool(id, Property::Enabled) == Some(false) {
            continue;
        }
        let point = |p| {
            tree.get_ref(id, p)
                .and_then(|a| crate::movers::attachment_world(tree, a))
                .map(|(v, _)| to_bevy(v))
        };
        let (Some(from), Some(to)) = (point(Property::Attachment0), point(Property::Attachment1)) else {
            continue;
        };
        let n = |p, d: f64| tree.get_number(id, p).unwrap_or(d) as f32;
        out.push(BeamShape {
            id,
            from,
            to,
            width: ((n(Property::Width0, 1.0) + n(Property::Width1, 1.0)) / 2.0).max(0.01),
            colour: tree.get_color(id, Property::Color).unwrap_or(libroxel_instance::Color3::WHITE),
            transparency: n(Property::Transparency, 0.0).clamp(0.0, 1.0),
            glow: n(Property::LightEmission, 0.0).clamp(0.0, 1.0),
        });
    }
    out
}

fn beam_material(c: libroxel_instance::Color3, transparency: f32, glow: f32) -> StandardMaterial {
    let base = Color::srgba(c.r, c.g, c.b, 1.0 - transparency);
    StandardMaterial {
        base_color: base,
        emissive: LinearRgba::from(Color::srgb(c.r, c.g, c.b)) * (glow * 2.0),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        double_sided: true,
        ..default()
    }
}

fn sync_beams(
    engine: NonSend<EngineState>,
    mut commands: Commands,
    meshes: Option<Res<PartMeshes>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    rigs: Query<&Transform, (With<CameraRig>, Without<BeamLink>)>,
    mut drawn: Query<(Entity, &mut BeamLink, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(meshes) = meshes else { return };
    let eye = rigs.iter().next().map_or(Vec3::ZERO, |t| t.translation);
    let tree = engine.tree.borrow();
    let mut wanted: HashMap<InstanceId, (Transform, BeamShape)> = HashMap::new();
    for beam in beams(&tree) {
        let along = beam.to - beam.from;
        let length = along.length();
        if length < 1e-3 {
            continue;
        }
        let x = along / length;
        let mid = (beam.from + beam.to) / 2.0;
        // The strip turns about its length to face the camera.
        let to_eye = (eye - mid).normalize_or(Vec3::Z);
        let y = x.cross(to_eye).normalize_or(Vec3::Y);
        let z = x.cross(y);
        let transform = Transform::from_translation(mid)
            .with_rotation(Quat::from_mat3(&Mat3::from_cols(x, y, z)))
            .with_scale(Vec3::new(length, beam.width, 1.0));
        wanted.insert(beam.id, (transform, beam));
    }
    let key = |b: &BeamShape| (b.colour, b.transparency.to_bits(), b.glow.to_bits());
    for (entity, mut link, mut transform, handle) in &mut drawn {
        match wanted.remove(&link.0) {
            Some((t, beam)) => {
                transform.set_if_neq(t);
                if link.1 != key(&beam) {
                    link.1 = key(&beam);
                    if let Some(mut m) = materials.get_mut(&handle.0) {
                        *m = beam_material(beam.colour, beam.transparency, beam.glow);
                    }
                }
            }
            None => commands.entity(entity).despawn(),
        }
    }
    for (id, (transform, beam)) in wanted {
        commands.spawn((
            BeamLink(id, key(&beam)),
            Mesh3d(meshes.quad.clone()),
            MeshMaterial3d(materials.add(beam_material(beam.colour, beam.transparency, beam.glow))),
            transform,
        ));
    }
}
