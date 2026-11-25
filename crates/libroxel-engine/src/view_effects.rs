//! Drawing the look classes: bloom and colour correction (in Lighting or
//! the camera), selection boxes and beams. Atmosphere is drawn with the
//! fog, in `view::update_lighting`.

use std::collections::HashMap;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::post_process::bloom::{Bloom, BloomPrefilter};
use bevy::prelude::*;
use bevy::render::view::ColorGrading;
use libroxel_instance::geometry::Region;
use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property};

use crate::view::{CameraRig, EngineSet, EngineState, PartMeshes, to_bevy};

pub(crate) fn build(app: &mut App) {
    app.add_systems(
        Update,
        (sync_screen_effects, draw_selection_boxes, sync_beams, sync_sky, sync_trails).after(EngineSet::Sync),
    );
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

pub fn base_grading() -> ColorGrading {
    let mut out = ColorGrading::default();
    out.global.post_saturation = 1.3;
    for section in [&mut out.shadows, &mut out.midtones, &mut out.highlights] {
        section.contrast = 1.1;
    }
    out
}

/// The colour grading the tree's colour correction asks for (the default
/// when there's none).
pub fn grading(tree: &InstanceTree) -> ColorGrading {
    let mut out = base_grading();
    let Some(id) = effect(tree, ClassKind::ColorCorrectionEffect) else { return out };
    let n = |p| tree.get_number(id, p).unwrap_or(0.0) as f32;
    out.global.exposure = n(Property::Brightness) * 2.0;
    out.global.post_saturation *= (1.0 + n(Property::Saturation)).max(0.0);
    let contrast = (1.0 + n(Property::Contrast)).max(0.0);
    for section in [&mut out.shadows, &mut out.midtones, &mut out.highlights] {
        section.contrast *= contrast;
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
    quality: Option<Res<crate::view::GraphicsQuality>>,
) {
    let tree = engine.tree.borrow();
    let wanted = bloom_look(&tree).filter(|_| quality.as_ref().is_none_or(|q| q.bloom()));
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

/// How far away the sun, moon and stars are drawn: behind anything a map
/// holds, as the reference platform's sky is. The camera's depth reaches to infinity, so only
/// culling has to be told to keep them (see [`NoFrustumCulling`]).
const SKY_DISTANCE: f32 = 60_000.0;

/// What a sky in Lighting draws at the current time of day.
#[derive(Debug, Clone, PartialEq)]
pub struct SkyLook {
    /// Which way the sun is, and its width in degrees (None when hidden or set).
    pub sun: Option<(Vec3, f32)>,
    /// Which way the moon is, and its width in degrees.
    pub moon: Option<(Vec3, f32)>,
    /// How many stars, and how bright they are (0 by day, 1 at night).
    pub stars: (u32, f32),
}

/// The sky the tree asks for: the first Sky in Lighting, if any.
pub fn sky_look(tree: &InstanceTree) -> Option<SkyLook> {
    use std::f32::consts::PI;
    let lighting = tree.service(ClassKind::Lighting)?;
    let sky = tree.find_first_child_of_class(lighting, ClassKind::Sky).ok().flatten()?;
    let n = |p, d: f64| tree.get_number(sky, p).unwrap_or(d) as f32;
    let clock = tree.get_number(lighting, Property::ClockTime).unwrap_or(14.0) as f32;
    // The same path as the light in `view::sky_at`: up at 6, overhead at
    // noon, down at 18. The moon is opposite.
    let angle = (clock - 6.0) / 12.0 * PI;
    let sun_dir = Vec3::new(angle.cos(), angle.sin(), 0.35).normalize();
    let shown = tree.get_bool(sky, Property::CelestialBodiesShown) != Some(false);
    let up = |dir: Vec3, size: f32| (shown && dir.y > -0.1 && size > 0.0).then_some((dir, size.min(60.0)));
    let night = (1.0 - (angle.sin() * 4.0 + 0.6)).clamp(0.0, 1.0);
    Some(SkyLook {
        sun: up(sun_dir, n(Property::SunAngularSize, 21.0)),
        moon: up(-sun_dir, n(Property::MoonAngularSize, 11.0)),
        stars: (n(Property::StarCount, 3000.0).clamp(0.0, 10_000.0) as u32, night),
    })
}

/// A repeatable random number in 0..1 for star `i` (the same every time).
fn star_noise(i: u32, salt: u32) -> f32 {
    let mut h = i.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFF_FFFF) as f32 / 0x100_0000 as f32
}

/// Where the stars sit: `count` points scattered at random over the upper
/// sky (unit directions), the same every time. Evenly spaced stars read as
/// a grid of dots, so each one is placed at random.
pub fn star_directions(count: u32) -> Vec<Vec3> {
    (0..count)
        .map(|i| {
            // Uniform over the top half of a sphere, dipping a little below the horizon.
            let y = 1.0 - star_noise(i, 1) * 1.1;
            let r = (1.0 - y * y).max(0.0).sqrt();
            let a = star_noise(i, 2) * std::f32::consts::TAU;
            Vec3::new(r * a.cos(), y, r * a.sin())
        })
        .collect()
}

fn star_mesh(count: u32) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for (i, dir) in star_directions(count).into_iter().enumerate() {
        let centre = dir * SKY_DISTANCE;
        let side = dir.any_orthonormal_vector();
        let other = dir.cross(side);
        // Mostly tiny points (about a pixel), with a few brighter, bigger ones.
        let big = star_noise(i as u32, 3);
        let size = SKY_DISTANCE * (0.0009 + big * big * big * 0.0016);
        let base = positions.len() as u32;
        for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            positions.push((centre + (side * a + other * b) * size).to_array());
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_indices(Indices::U32(indices))
}

fn glow_material(colour: Color, alpha: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: colour.with_alpha(alpha),
        emissive: LinearRgba::from(colour) * 2.0,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        fog_enabled: false,
        cull_mode: None,
        ..default()
    }
}

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum SkyPiece {
    Sun,
    Moon,
    Stars,
}

#[derive(Default)]
struct SkyState {
    stars: Option<(u32, Handle<Mesh>)>,
    brightness: f32,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_sky(
    engine: NonSend<EngineState>,
    mut commands: Commands,
    part_meshes: Option<Res<PartMeshes>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    rigs: Query<&Transform, (With<CameraRig>, Without<SkyPiece>)>,
    mut pieces: Query<(
        Entity,
        &SkyPiece,
        &mut Transform,
        &mut Visibility,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut state: Local<SkyState>,
) {
    let Some(part_meshes) = part_meshes else { return };
    let look = sky_look(&engine.tree.borrow());
    let Some(look) = look else {
        for (entity, ..) in &pieces {
            commands.entity(entity).despawn();
        }
        state.stars = None;
        return;
    };
    let eye = rigs.iter().next().map_or(Vec3::ZERO, |t| t.translation);
    let star_count = look.stars.0;
    if state.stars.as_ref().map(|(n, _)| *n) != Some(star_count) {
        for (entity, piece, ..) in &pieces {
            if *piece == SkyPiece::Stars {
                commands.entity(entity).despawn();
            }
        }
        let mesh = meshes.add(star_mesh(star_count));
        commands.spawn((
            SkyPiece::Stars,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials.add(glow_material(Color::WHITE, 0.0))),
            Transform::from_translation(eye),
            bevy::light::NotShadowCaster,
            NoFrustumCulling,
        ));
        state.stars = Some((star_count, mesh));
        state.brightness = -1.0;
    }
    let mut have = [false, false];
    for (_, piece, mut transform, mut visibility, handle) in &mut pieces {
        let (body, slot) = match piece {
            SkyPiece::Sun => (look.sun, 0),
            SkyPiece::Moon => (look.moon, 1),
            SkyPiece::Stars => {
                transform.translation = eye;
                if (state.brightness - look.stars.1).abs() > 0.01 {
                    state.brightness = look.stars.1;
                    if let Some(mut m) = materials.get_mut(&handle.0) {
                        m.base_color = Color::WHITE.with_alpha(look.stars.1 * 0.9);
                    }
                }
                visibility.set_if_neq(if look.stars.1 > 0.01 {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
                continue;
            }
        };
        have[slot] = true;
        match body {
            Some((dir, degrees)) => {
                let width = 2.0 * SKY_DISTANCE * (degrees.to_radians() / 2.0).tan() * 0.4;
                *transform =
                    Transform::from_translation(eye + dir * SKY_DISTANCE).with_scale(Vec3::splat(width));
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
    for (slot, piece, colour) in
        [(0, SkyPiece::Sun, Color::srgb(1.0, 0.95, 0.75)), (1, SkyPiece::Moon, Color::srgb(0.85, 0.88, 0.95))]
    {
        if !have[slot] {
            commands.spawn((
                piece,
                Mesh3d(part_meshes.ball.clone()),
                MeshMaterial3d(materials.add(glow_material(colour, 1.0))),
                Transform::from_translation(eye + Vec3::Y * SKY_DISTANCE),
                Visibility::Hidden,
                bevy::light::NotShadowCaster,
                NoFrustumCulling,
            ));
        }
    }
}

/// The trail a pair of attachments leaves: where they were, and when.
#[derive(Debug, Default, Clone)]
pub struct TrailHistory {
    points: std::collections::VecDeque<(f32, Vec3, Vec3)>,
}

impl TrailHistory {
    /// Notes where the attachments are at `time`, and forgets what is older
    /// than `lifetime`. A point is only added once they've moved a little.
    pub fn record(&mut self, time: f32, a: Vec3, b: Vec3, lifetime: f32) {
        let moved = self.points.back().is_none_or(|(_, pa, pb)| pa.distance(a) + pb.distance(b) > 0.1);
        if moved {
            self.points.push_back((time, a, b));
        } else if let Some(last) = self.points.back_mut() {
            // Still standing: the newest end stays fresh.
            last.0 = time;
        }
        while self.points.front().is_some_and(|(t, ..)| time - *t > lifetime) {
            self.points.pop_front();
        }
    }

    /// The strip's corners, oldest first, with how faded each pair is
    /// (0 is new, 1 is about to vanish).
    pub fn strip(&self, time: f32, lifetime: f32) -> Vec<(Vec3, Vec3, f32)> {
        self.points
            .iter()
            .map(|(t, a, b)| (*a, *b, ((time - t) / lifetime.max(1e-3)).clamp(0.0, 1.0)))
            .collect()
    }

    pub fn clear(&mut self) {
        self.points.clear();
    }
}

fn trail_mesh(strip: &[(Vec3, Vec3, f32)], transparency: f32) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let mut positions = Vec::new();
    let mut colours = Vec::new();
    let mut indices = Vec::new();
    for (i, (a, b, age)) in strip.iter().enumerate() {
        let alpha = (1.0 - age) * (1.0 - transparency);
        positions.push(a.to_array());
        positions.push(b.to_array());
        colours.push([1.0, 1.0, 1.0, alpha]);
        colours.push([1.0, 1.0, 1.0, alpha]);
        if i > 0 {
            let n = (i * 2) as u32;
            indices.extend([n - 2, n - 1, n + 1, n - 2, n + 1, n]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours)
        .with_inserted_indices(Indices::U32(indices))
}

#[derive(Component)]
struct TrailLink(InstanceId);

/// A trail's corners, material and transparency this frame.
type TrailFrame = (Vec<(Vec3, Vec3, f32)>, StandardMaterial, f32);

#[allow(clippy::too_many_arguments)]
fn sync_trails(
    engine: NonSend<EngineState>,
    time: Res<Time>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    drawn: Query<(Entity, &TrailLink, &Mesh3d, &MeshMaterial3d<StandardMaterial>)>,
    mut histories: Local<HashMap<InstanceId, TrailHistory>>,
) {
    let now = time.elapsed_secs();
    let tree = engine.tree.borrow();
    let mut live: HashMap<InstanceId, TrailFrame> = HashMap::new();
    for id in tree.descendants_of_class(tree.workspace(), ClassKind::Trail) {
        let point = |p| {
            tree.get_ref(id, p)
                .and_then(|a| crate::movers::attachment_world(&tree, a))
                .map(|(v, _)| to_bevy(v))
        };
        let (Some(a), Some(b)) = (point(Property::Attachment0), point(Property::Attachment1)) else {
            continue;
        };
        let n = |p, d: f64| tree.get_number(id, p).unwrap_or(d) as f32;
        let lifetime = n(Property::TrailLifetime, 2.0).max(0.01);
        let history = histories.entry(id).or_default();
        if tree.get_bool(id, Property::Enabled) == Some(false) {
            history.clear();
            continue;
        }
        history.record(now, a, b, lifetime);
        let colour = tree.get_color(id, Property::Color).unwrap_or(libroxel_instance::Color3::WHITE);
        let transparency = n(Property::Transparency, 0.0).clamp(0.0, 1.0);
        let material = beam_material(colour, 0.0, n(Property::LightEmission, 0.0).clamp(0.0, 1.0));
        live.insert(id, (history.strip(now, lifetime), material, transparency));
    }
    histories.retain(|id, _| tree.class(*id) == Ok(ClassKind::Trail));
    for (entity, link, mesh, material) in &drawn {
        match live.remove(&link.0) {
            Some((strip, wanted, transparency)) => {
                if let Some(mut m) = meshes.get_mut(&mesh.0) {
                    *m = trail_mesh(&strip, transparency);
                }
                if let Some(mut m) = materials.get_mut(&material.0)
                    && m.base_color != wanted.base_color
                {
                    *m = wanted;
                }
            }
            None => commands.entity(entity).despawn(),
        }
    }
    for (id, (strip, material, transparency)) in live {
        commands.spawn((
            TrailLink(id),
            Mesh3d(meshes.add(trail_mesh(&strip, transparency))),
            MeshMaterial3d(materials.add(material)),
            Transform::IDENTITY,
            bevy::light::NotShadowCaster,
        ));
    }
}
