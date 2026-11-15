//! Bevy integration: mirroring parts onto entities, drawing them, cameras,
//! and turning keyboard input into character movement.

use std::collections::HashMap;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use libroxel_instance::{
    ClassKind, EnumValue, InstanceId, InstanceTree, Material, NormalId, PartShape, Property,
};
use libroxel_script::SharedTree;

use crate::{CharacterInput, LogEntry, PlaySession};

/// The world being shown, plus the play session when one is running.
///
/// Lua is single-threaded, so this is a non-send resource.
pub struct EngineState {
    pub tree: SharedTree,
    pub session: Option<PlaySession>,
    /// Every log line since the app started (the UI shows these).
    pub log: Vec<LogEntry>,
    /// Particle bursts waiting to be sent out by the renderer.
    pub emits: Vec<(InstanceId, u32)>,
}

impl EngineState {
    pub fn new(tree: InstanceTree) -> Self {
        Self {
            tree: std::rc::Rc::new(std::cell::RefCell::new(tree)),
            session: None,
            log: Vec::new(),
            emits: Vec::new(),
        }
    }

    pub fn is_playing(&self) -> bool {
        self.session.is_some()
    }
}

/// Set by the host app when its own UI is using the mouse or keyboard, so
/// the 3D view doesn't react at the same time.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct InputCapture {
    pub pointer: bool,
    pub keyboard: bool,
}

/// This frame's character input, filled from the keyboard.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct PlayInput(pub CharacterInput);

/// Links an entity to the part it mirrors.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartLink(pub InstanceId);

/// The part's name, for debugging and lookups.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct PartName(pub String);

/// How the part should look. The renderer turns this into a material.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct PartAppearance {
    pub color: Color,
    pub transparency: f32,
    pub material: Material,
    /// How many times surface textures repeat across the part.
    pub texture_repeat: f32,
    /// How mirror-like it is, from 0 to 1.
    pub reflectance: f32,
    /// Whether it casts a shadow.
    pub shadow: bool,
}

/// Which mesh the part is drawn with.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartGeometry(pub PartShape);

/// The model file a part is drawn with instead of its shape (empty for none).
#[derive(Component, Debug, Clone, PartialEq, Eq, Default)]
pub struct PartMeshFile(pub String);

/// The picture wrapped over a part's model (empty for none).
#[derive(Component, Debug, Clone, PartialEq, Eq, Default)]
pub struct PartMeshTexture(pub String);

/// The transform that draws a part of `shape` and `size` at `pos` with `orientation`.
///
/// Meshes are unit-sized; balls use their smallest side and cylinders (which
/// lie along X) use the smaller of their other two sides as the diameter.
pub fn part_transform(
    pos: libroxel_instance::Vec3,
    size: libroxel_instance::Vec3,
    orientation: libroxel_instance::Vec3,
    shape: PartShape,
) -> Transform {
    let size = to_bevy(size).max(Vec3::splat(0.01));
    let scale = match shape {
        PartShape::Ball => Vec3::splat(size.min_element()),
        PartShape::Cylinder => {
            let d = size.y.min(size.z);
            Vec3::new(size.x, d, d)
        }
        PartShape::Block | PartShape::Wedge => size,
    };
    Transform::from_translation(to_bevy(pos))
        .with_rotation(crate::physics::orientation_quat(orientation))
        .with_scale(scale)
}

/// Reads a part's shape, defaulting to a block.
pub fn part_shape(tree: &InstanceTree, id: InstanceId) -> PartShape {
    match tree.get_enum(id, Property::Shape) {
        Some(EnumValue::Shape(shape)) => shape,
        _ => PartShape::Block,
    }
}

/// While playing, a part is drawn out to this many times its biggest side
/// (and at least `DETAIL_MIN` studs away).
const DETAIL_PER_STUD: f32 = 60.0;
const DETAIL_MIN: f32 = 150.0;

/// How many parts are brought into view in one frame, nearest first.
const SPAWN_BUDGET: usize = 1500;

/// How far from the camera parts are drawn while playing: the workspace's
/// `StreamingTargetRadius` (Roblox's default is 1024 studs), or everything
/// when its `StreamingEnabled` is off.
fn streaming_radius(tree: &libroxel_instance::InstanceTree) -> f32 {
    let ws = tree.workspace();
    if matches!(
        tree.get_extra(ws, "StreamingEnabled"),
        Ok(Some(libroxel_instance::PropertyValue::Bool(false)))
    ) {
        return f32::INFINITY;
    }
    match tree.get_extra(ws, "StreamingTargetRadius") {
        Ok(Some(libroxel_instance::PropertyValue::Number(r))) if r > 0.0 => (r as f32).max(64.0),
        _ => 1024.0,
    }
}

/// Nanoseconds spent copying parts into the 3D view, for the speed report.
static SYNC_NANOS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many parts are in the 3D view, how many of them are drawn merged
/// into clusters, and how many clusters there are.
pub fn cluster_counts() -> (usize, usize, usize) {
    crate::clusters::counts()
}

/// Seconds spent copying parts into the 3D view since the last call.
pub fn take_sync_seconds() -> f64 {
    SYNC_NANOS.swap(0, std::sync::atomic::Ordering::Relaxed) as f64 / 1e9
}

pub(crate) struct TimedSync(pub(crate) std::time::Instant);

impl Drop for TimedSync {
    fn drop(&mut self) {
        let took = self.0.elapsed().as_nanos() as u64;
        SYNC_NANOS.fetch_add(took, std::sync::atomic::Ordering::Relaxed);
    }
}

/// What `sync_parts` remembers between frames, to skip the parts that
/// haven't changed.
#[derive(Default)]
struct PartSync {
    /// The tree it last looked at (a new one is looked over in full).
    tree: usize,
    /// Where it last read the tree's touched log, and that log's epoch.
    read: usize,
    epoch: u64,
    /// The workspace's structure when it last looked it over.
    structure: u64,
    /// Where the camera was then.
    eye: Option<Vec3>,
    /// Whether parts in range were left to spawn next frame.
    pending: bool,
    /// Parts out of range when last measured.
    far: FastSet<InstanceId>,
    frames: u64,
}

/// Each part's entity, and the revision of the part it last showed.
#[derive(Resource, Default)]
struct PartEntities(FastMap<InstanceId, Entity>, FastMap<InstanceId, u64>);

/// Hash maps with a fast hash, for ones looked up thousands of times a frame.
type FastMap<K, V> = bevy::platform::collections::HashMap<K, V>;
type FastSet<K> = bevy::platform::collections::HashSet<K>;

/// Mirrors the tree into the ECS and runs the play session each frame.
/// Works headless; add [`RenderPlugin`] to draw.
pub struct EnginePlugin;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum EngineSet {
    /// Reading input into [`PlayInput`].
    Input,
    /// Stepping scripts and physics.
    Simulate,
    /// Copying the tree into entities.
    Sync,
}

impl Plugin for EnginePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PartEntities>()
            .init_resource::<PlayInput>()
            .init_resource::<InputCapture>()
            .configure_sets(Update, (EngineSet::Input, EngineSet::Simulate, EngineSet::Sync).chain())
            .add_systems(Update, step_session.in_set(EngineSet::Simulate))
            .add_systems(Update, sync_parts.in_set(EngineSet::Sync));
    }
}

fn step_session(mut engine: NonSendMut<EngineState>, time: Res<Time>, input: Res<PlayInput>) {
    let engine = &mut *engine;
    let Some(session) = engine.session.as_mut() else { return };
    // Clamp long frames (a window drag, a breakpoint) so physics stays sane.
    let dt = time.delta_secs().min(0.1);
    session.step(dt, input.0);
    engine.log.extend(session.take_log());
    engine.emits.extend(session.take_emits());
}

pub(crate) fn to_bevy(v: libroxel_instance::Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// What [`sync_parts`] keeps up to date on each part's entity.
type PartPieces = (
    &'static mut Transform,
    &'static mut PartAppearance,
    &'static mut PartName,
    &'static mut PartGeometry,
    &'static mut PartMeshFile,
    &'static mut PartMeshTexture,
);

/// Mirrors workspace parts onto entities.
///
/// This does a full pass each frame, which is fine for hundreds of parts.
/// It will move to the tree's change journal when worlds get bigger.
fn sync_parts(
    engine: NonSend<EngineState>,
    mut entities: ResMut<PartEntities>,
    mut commands: Commands,
    mut parts: Query<PartPieces, With<PartLink>>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<PartLink>)>,
    mut sync: Local<PartSync>,
    quality: Option<Res<GraphicsQuality>>,
) {
    let _timed = TimedSync(std::time::Instant::now());
    let mut tree = engine.tree.borrow_mut();
    // While playing, the session reads the change journal (for Changed events).
    if !engine.is_playing() {
        tree.drain_changes();
    }

    // Most frames most of a big map is unchanged: only parts whose properties
    // changed (the tree's touched log) are copied, and the whole workspace
    // is looked over only when parts come or go, the camera moves a fair
    // bit, or now and then to be sure.
    let eye = cameras.iter().next().map(|t| t.translation());
    let detail = engine.is_playing();
    let radius = if detail {
        // (No further than the camera sees at this graphics level.)
        streaming_radius(&tree).min(quality.as_ref().map_or(f32::INFINITY, |q| q.view_distance() + 50.0))
    } else {
        f32::INFINITY
    };
    let limited = detail || radius.is_finite();
    let tree_ptr = std::rc::Rc::as_ptr(&engine.tree) as usize;
    let (epoch, _) = tree.touched_since(0);
    sync.frames += 1;
    let full = sync.tree != tree_ptr
        || sync.epoch != epoch
        || tree.touched_len() < sync.read
        || sync.frames.is_multiple_of(120);
    let moved_far = match (eye, sync.eye) {
        (Some(e), Some(old)) => e.distance(old) > 8.0,
        (a, b) => a.is_some() != b.is_some(),
    };
    let scan = full
        || sync.structure != tree.workspace_structure()
        || sync.pending
        || (limited && (moved_far || sync.frames.is_multiple_of(30)));
    let mut work: Vec<InstanceId> = Vec::new();
    if full {
        // Everything spawned is compared with its revision below.
    } else {
        let (_, touched) = tree.touched_since(sync.read);
        let PartEntities(spawned, seen) = &*entities;
        let mut once = FastSet::default();
        for id in touched {
            if spawned.contains_key(id)
                && once.insert(*id)
                && seen.get(id) != Some(&tree.revision(*id).unwrap_or(0))
            {
                work.push(*id);
            }
        }
    }
    sync.tree = tree_ptr;
    sync.epoch = epoch;
    sync.read = tree.touched_len();
    if scan {
        sync.structure = tree.workspace_structure();
        sync.eye = eye;
        let live: Vec<InstanceId> = tree.descendants_of_class(tree.workspace(), ClassKind::Part);
        let live_set: FastSet<InstanceId> = live.iter().copied().collect();
        // Out-of-range parts are measured again only when the camera moved
        // a fair bit (or now and then); otherwise the ones measured before
        // are skipped.
        let measure = full || moved_far || sync.frames.is_multiple_of(30);
        if measure {
            sync.far.clear();
        }

        // Like Roblox's streaming: while playing, only parts within the
        // workspace's streaming radius of the camera are drawn, and new parts
        // come in nearest first, a batch a frame, so a big map opens at once
        // instead of building every part before the first frame.
        // Like Roblox at lower graphics levels, small parts stop being drawn
        // sooner than big ones: a pebble far off can't be seen anyway. How
        // far a part is past where it stops being drawn (1 = right there).
        let reach = |id: InstanceId| -> f32 {
            let Some(eye) = eye else { return 0.0 };
            let (Some(pos), Some(size)) =
                (tree.get_vec3(id, Property::Position), tree.get_vec3(id, Property::Size))
            else {
                return 0.0;
            };
            let d = (Vec3::new(pos.x, pos.y, pos.z).distance(eye) - size.length() * 0.5).max(0.0);
            let limit = if detail {
                radius.min((size.x.max(size.y).max(size.z) * DETAIL_PER_STUD).max(DETAIL_MIN))
            } else {
                radius
            };
            d / limit
        };

        let PartEntities(spawned, seen) = &mut *entities;
        spawned.retain(|id, entity| {
            // Gone, or well out of range (a little past it, so edges don't flicker).
            let keep = live_set.contains(id) && (!limited || !measure || reach(*id) <= 1.1);
            if !keep {
                commands.entity(*entity).despawn();
            }
            keep
        });
        seen.retain(|id, _| spawned.contains_key(id));

        let mut fresh: Vec<(f32, InstanceId)> = Vec::new();
        for id in live {
            if spawned.contains_key(&id) {
                // Unchanged since last frame: nothing to copy.
                if full && seen.get(&id) != Some(&tree.revision(id).unwrap_or(0)) {
                    work.push(id);
                }
                continue;
            }
            if !measure && sync.far.contains(&id) {
                continue;
            }
            let d = reach(id);
            if d <= 1.0 {
                fresh.push((d, id));
            } else if limited {
                sync.far.insert(id);
            }
        }
        sync.pending = fresh.len() > SPAWN_BUDGET;
        if sync.pending {
            fresh.select_nth_unstable_by(SPAWN_BUDGET, |a, b| a.0.total_cmp(&b.0));
            fresh.truncate(SPAWN_BUDGET);
        }
        work.extend(fresh.into_iter().map(|(_, id)| id));
    } else if full {
        let PartEntities(spawned, seen) = &*entities;
        work.extend(
            spawned.keys().copied().filter(|id| seen.get(id) != Some(&tree.revision(*id).unwrap_or(0))),
        );
    }
    let PartEntities(spawned, seen) = &mut *entities;

    for id in work {
        let revision = tree.revision(id).unwrap_or(0);
        let (Some(pos), Some(size)) =
            (tree.get_vec3(id, Property::Position), tree.get_vec3(id, Property::Size))
        else {
            continue;
        };
        let color =
            tree.get_color(id, Property::Color).map(|c| Color::srgb(c.r, c.g, c.b)).unwrap_or(Color::WHITE);
        // A local modifier (set by camera scripts to see through walls) fades
        // the part further, as in Roblox.
        let modifier = match tree.get_extra(id, "LocalTransparencyModifier") {
            Ok(Some(libroxel_instance::PropertyValue::Number(m))) => m.clamp(0.0, 1.0) as f32,
            _ => 0.0,
        };
        let own = tree.get_number(id, Property::Transparency).unwrap_or(0.0) as f32;
        let transparency = 1.0 - (1.0 - own) * (1.0 - modifier);
        let material = match tree.get_enum(id, Property::Material) {
            Some(EnumValue::Material(m)) => m,
            _ => Material::Plastic,
        };
        let mesh_file =
            PartMeshFile(tree.get_string(id, Property::MeshId).unwrap_or_default().trim().to_owned());
        let mesh_texture =
            PartMeshTexture(tree.get_string(id, Property::MeshTexture).unwrap_or_default().trim().to_owned());
        // A model is stretched over the part's whole box.
        let shape = if mesh_file.0.is_empty() { part_shape(&tree, id) } else { PartShape::Block };
        let orientation = tree.get_vec3(id, Property::Orientation).unwrap_or_default();
        let transform = part_transform(pos, size, orientation, shape);
        let texture_repeat = (size.x.max(size.y).max(size.z) / 8.0).max(0.25);
        let reflectance = tree.get_number(id, Property::Reflectance).unwrap_or(0.0) as f32;
        let shadow = tree.get_bool(id, Property::CastShadow) != Some(false);
        let appearance =
            PartAppearance { color, transparency, material, texture_repeat, reflectance, shadow };
        let name = PartName(tree.name(id).unwrap_or_default().to_owned());
        let geometry = PartGeometry(shape);

        match spawned.get(&id).and_then(|e| parts.get_mut(*e).ok()) {
            Some((mut t, mut a, mut n, mut g, mut m, mut mt)) => {
                m.set_if_neq(mesh_file);
                mt.set_if_neq(mesh_texture);
                t.set_if_neq(transform);
                a.set_if_neq(appearance);
                n.set_if_neq(name);
                g.set_if_neq(geometry);
                seen.insert(id, revision);
            }
            None if !spawned.contains_key(&id) => {
                let entity = commands
                    .spawn((PartLink(id), name, transform, appearance, geometry, mesh_file, mesh_texture))
                    .id();
                spawned.insert(id, entity);
                seen.insert(id, revision);
            }
            None => {} // spawned this frame; commands not applied yet
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering and cameras
// ---------------------------------------------------------------------------

/// Draws parts, adds a light, and drives the [`CameraRig`] camera.
pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_scene)
            .init_resource::<crate::devices::VirtualInput>()
            .add_systems(Update, (camera_controls, play_input, forward_keys).chain().in_set(EngineSet::Input))
            .init_resource::<crate::mesh_files::MeshFiles>()
            .init_resource::<MeshTextures>()
            .init_resource::<SharedMaterials>()
            .init_resource::<crate::clusters::Clusters>()
            .add_systems(
                PostUpdate,
                (
                    attach_visuals,
                    update_materials,
                    update_meshes,
                    use_mesh_files,
                    use_mesh_textures,
                    crate::clusters::cluster_parts,
                )
                    .chain(),
            )
            // The camera moves in Update, right after the world, and its global
            // transform is brought up to date at once, so GUIs and name tags
            // drawn later this frame line up with where the camera is now.
            .add_systems(
                Update,
                (follow_camera, refresh_camera_global, hide_self_in_first_person)
                    .chain()
                    .after(EngineSet::Sync),
            )
            .add_systems(Update, (sync_lights, draw_blasts, sync_ropes, sync_decals).after(EngineSet::Sync))
            .add_systems(Update, cover_view)
            .add_systems(Update, crate::terrain_view::sync_terrain.after(EngineSet::Sync))
            .add_systems(Update, update_lighting.after(EngineSet::Sync))
            .init_resource::<GraphicsQuality>()
            .add_systems(Update, auto_quality.after(EngineSet::Sync))
            .add_plugins((crate::effects::EffectsPlugin, crate::sounds::SoundPlugin));
        crate::view_effects::build(app);
    }
}

/// One unit-sized mesh per part shape.
#[derive(Resource)]
pub(crate) struct PartMeshes {
    pub(crate) block: Handle<Mesh>,
    pub(crate) ball: Handle<Mesh>,
    pub(crate) cylinder: Handle<Mesh>,
    pub(crate) wedge: Handle<Mesh>,
    /// A 1 by 1 square facing +Z, for decals.
    pub(crate) quad: Handle<Mesh>,
}

impl PartMeshes {
    pub(crate) fn get(&self, shape: PartShape) -> Handle<Mesh> {
        match shape {
            PartShape::Block => self.block.clone(),
            PartShape::Ball => self.ball.clone(),
            PartShape::Cylinder => self.cylinder.clone(),
            PartShape::Wedge => self.wedge.clone(),
        }
    }
}

/// Whether the game's GUI covers the whole 3D view with something solid
/// (a loading screen): then the view isn't drawn at all, and the frames it
/// saves go to the game.
pub static VIEW_COVERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn cover_view(mut cameras: Query<&mut Camera, With<CameraRig>>) {
    let covered = VIEW_COVERED.load(std::sync::atomic::Ordering::Relaxed)
        || !libroxel_script::RENDERING_3D.load(std::sync::atomic::Ordering::Relaxed);
    for mut camera in &mut cameras {
        if camera.is_active == covered {
            camera.is_active = !covered;
        }
    }
}

/// Generated surface textures for the textured materials.
#[derive(Resource)]
pub(crate) struct MaterialTextures {
    wood: Handle<Image>,
    brick: Handle<Image>,
    grass: Handle<Image>,
    sand: Handle<Image>,
    rock: Handle<Image>,
    snow: Handle<Image>,
}

/// A unit ramp: full height at +Z, sloping down to -Z.
fn wedge_mesh() -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let h = 0.5;
    let slope = Vec3::new(0.0, 1.0, -1.0).normalize();
    // (corners, normal, uvs) for each face.
    let faces: [(Vec<Vec3>, Vec3); 5] = [
        (
            vec![Vec3::new(-h, -h, -h), Vec3::new(h, -h, -h), Vec3::new(h, -h, h), Vec3::new(-h, -h, h)],
            Vec3::NEG_Y,
        ),
        (vec![Vec3::new(-h, -h, h), Vec3::new(h, -h, h), Vec3::new(h, h, h), Vec3::new(-h, h, h)], Vec3::Z),
        (vec![Vec3::new(-h, -h, -h), Vec3::new(h, -h, -h), Vec3::new(h, h, h), Vec3::new(-h, h, h)], slope),
        (vec![Vec3::new(-h, -h, -h), Vec3::new(-h, -h, h), Vec3::new(-h, h, h)], Vec3::NEG_X),
        (vec![Vec3::new(h, -h, -h), Vec3::new(h, -h, h), Vec3::new(h, h, h)], Vec3::X),
    ];
    let (mut positions, mut normals, mut uvs, mut indices) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (corners, normal) in faces {
        let base = positions.len() as u32;
        for c in &corners {
            positions.push(c.to_array());
            normals.push(normal.to_array());
            // Project onto the face's two other axes for texture coordinates.
            let uv = if normal.x.abs() > 0.5 {
                Vec2::new(c.z, c.y)
            } else if normal.y.abs() > 0.9 {
                Vec2::new(c.x, c.z)
            } else {
                Vec2::new(c.x, c.y)
            };
            uvs.push((uv + Vec2::splat(0.5)).to_array());
        }
        for k in 1..corners.len() as u32 - 1 {
            let (a, b, c) = (corners[0], corners[k as usize], corners[k as usize + 1]);
            // Wind counter-clockwise as seen from outside.
            if (b - a).cross(c - a).dot(normal) >= 0.0 {
                indices.extend([base, base + k, base + k + 1]);
            } else {
                indices.extend([base, base + k + 1, base + k]);
            }
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// A cheap repeatable hash for texture noise.
fn noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut n = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263) ^ seed.wrapping_mul(2_246_822_519);
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    (n ^ (n >> 16)) as f32 / u32::MAX as f32
}

/// Smooth noise that repeats every `period` cells (so textures tile):
/// random values on a grid, blended with a smooth curve.
fn tile_noise(x: f32, y: f32, period: u32, seed: u32) -> f32 {
    let (fx, fy) = (x.floor(), y.floor());
    let (tx, ty) = (x - fx, y - fy);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let p = period.max(1) as i64;
    let at = |ix: i64, iy: i64| noise(ix.rem_euclid(p) as u32, iy.rem_euclid(p) as u32, seed);
    let (ix, iy) = (fx as i64, fy as i64);
    let top = at(ix, iy) + (at(ix + 1, iy) - at(ix, iy)) * sx;
    let bottom = at(ix, iy + 1) + (at(ix + 1, iy + 1) - at(ix, iy + 1)) * sx;
    top + (bottom - top) * sy
}

/// Layers of [`tile_noise`], each twice as fine and half as strong, from
/// `cells` cells across the texture (0 to 1, about 0.5 on average).
fn fbm(u: f32, v: f32, cells: u32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut weight, mut total, mut n) = (0.0, 1.0, 0.0, cells);
    for o in 0..octaves {
        sum += tile_noise(u * n as f32, v * n as f32, n, seed.wrapping_add(o * 101)) * weight;
        total += weight;
        weight *= 0.5;
        n *= 2;
    }
    sum / total
}

/// Pixels across a surface texture (one covers 8 studs: 32 a stud).
const TEXTURE_SIZE: u32 = 256;

/// A greyscale texture that tints the part's colour, made from `shade`
/// (given where in the texture, 0 to 1 each way), smooth and with
/// mipmaps so it stays calm far away.
fn make_texture(shade: impl Fn(f32, f32) -> f32) -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let size = TEXTURE_SIZE;
    let mut level: Vec<f32> = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = ((x as f32 + 0.5) / size as f32, (y as f32 + 0.5) / size as f32);
            level.push(shade(u, v).clamp(0.0, 1.0));
        }
    }
    // Every mip level, each half the last (averaging 2 x 2 pixels).
    let mut data = Vec::new();
    let mut side = size;
    let mut levels = 0;
    loop {
        for &g in &level {
            let v = (g * 255.0).round() as u8;
            data.extend([v, v, v, 255]);
        }
        levels += 1;
        if side == 1 {
            break;
        }
        let half = side / 2;
        let mut next = Vec::with_capacity((half * half) as usize);
        for y in 0..half {
            for x in 0..half {
                let i = |dx: u32, dy: u32| level[((y * 2 + dy) * side + x * 2 + dx) as usize];
                next.push((i(0, 0) + i(1, 0) + i(0, 1) + i(1, 1)) * 0.25);
            }
        }
        level = next;
        side = half;
    }
    let first = (size * size * 4) as usize;
    let mut image = Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data[..first].to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    // The smaller levels follow the full-size one in the same data.
    image.texture_descriptor.mip_level_count = levels;
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 4,
        ..default()
    });
    image
}

fn material_textures(images: &mut Assets<Image>) -> MaterialTextures {
    // Planks a quarter of the texture wide, grain running along them.
    let wood = make_texture(|u, v| {
        let plank = (v * 4.0).floor();
        let across = v * 4.0 - plank;
        let warp = fbm(u, v, 2, 3, 7) * 3.0;
        let grain = ((across * 18.0 + warp + plank * 1.7) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let fine = fbm(u * 1.0, v * 8.0, 4, 4, 9);
        let tone = 0.74 + noise(plank as u32, 0, 13) * 0.12;
        let seam = if !(0.025..=0.975).contains(&across) { -0.22 } else { 0.0 };
        // Each plank ends somewhere along its length.
        let end = ((u + noise(plank as u32, 1, 17)) * 2.0).fract();
        let butt = if end < 0.006 { -0.18 } else { 0.0 };
        tone + grain * 0.1 + fine * 0.1 + seam + butt
    });
    // Bricks 1/4 wide and 1/8 high, every other row moved half a brick.
    let brick = make_texture(|u, v| {
        let row = (v * 8.0).floor();
        let shift = if (row as u32).is_multiple_of(2) { 0.0 } else { 0.125 };
        let col = ((u + shift) * 4.0).floor();
        let (bx, by) = (((u + shift) * 4.0).fract(), (v * 8.0).fract());
        let edge = bx.min(1.0 - bx * 1.0).min(by * 0.5).min((1.0 - by) * 0.5);
        let rough = fbm(u, v, 16, 3, 5);
        if edge < 0.025 {
            0.9 + rough * 0.08
        } else {
            let tone = 0.62 + noise(col as u32 & 3, row as u32, 11) * 0.2;
            let worn = (edge * 30.0).min(1.0);
            tone * (0.9 + 0.1 * worn) + (rough - 0.5) * 0.14
        }
    });
    let grass = make_texture(|u, v| {
        let patches = fbm(u, v, 4, 3, 17);
        let blades = fbm(u, v, 64, 2, 19);
        0.62 + patches * 0.22 + blades * 0.18
    });
    let sand = make_texture(|u, v| {
        let ripples = ((v * 10.0 + fbm(u, v, 3, 3, 21) * 2.0) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let grains = noise((u * 256.0) as u32, (v * 256.0) as u32, 23);
        0.8 + ripples * 0.06 + fbm(u, v, 8, 3, 25) * 0.08 + grains * 0.06
    });
    let rock = make_texture(|u, v| {
        let blotch = fbm(u, v, 3, 5, 29);
        // Thin dark cracks where a noise field crosses its middle.
        let ridge = (fbm(u, v, 4, 3, 31) - 0.5).abs();
        let crack = if ridge < 0.015 { -0.2 * (1.0 - ridge / 0.015) } else { 0.0 };
        0.6 + blotch * 0.32 + crack
    });
    let snow = make_texture(|u, v| 0.9 + fbm(u, v, 6, 4, 37) * 0.1);
    MaterialTextures {
        wood: images.add(wood),
        brick: images.add(brick),
        grass: images.add(grass),
        sand: images.add(sand),
        rock: images.add(rock),
        snow: images.add(snow),
    }
}

/// How the camera moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraMode {
    /// Free flight: hold right mouse to look, WASD to move, Q/E down/up.
    Fly,
    /// Orbit the character: hold right mouse to turn, scroll to zoom.
    Follow,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct CameraRig {
    pub mode: CameraMode,
    pub yaw: f32,
    pub pitch: f32,
    /// Follow mode: distance behind the character (eased towards `zoom`).
    pub distance: f32,
    /// Follow mode: the distance the mouse wheel asked for.
    pub zoom: f32,
    /// Follow mode: how far away the camera is drawn, which walls can cut short.
    pub shown: f32,
    /// Follow mode: the character it last followed, to swing round behind a new one.
    pub following: Option<InstanceId>,
    /// Fly mode: studs per second.
    pub fly_speed: f32,
}

impl Default for CameraRig {
    fn default() -> Self {
        let d = crate::camera::START_DISTANCE;
        Self {
            mode: CameraMode::Fly,
            yaw: -0.6,
            pitch: -0.45,
            distance: d,
            zoom: d,
            shown: d,
            following: None,
            fly_speed: 30.0,
        }
    }
}

impl CameraRig {
    pub fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }
}

fn setup_scene(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut images: ResMut<Assets<Image>>) {
    // Parts are unit meshes scaled by their Size.
    commands.insert_resource(PartMeshes {
        block: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        ball: meshes.add(Sphere::new(0.5).mesh().uv(32, 18)),
        cylinder: meshes.add(
            Mesh::from(Cylinder::new(0.5, 1.0))
                .rotated_by(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
        ),
        wedge: meshes.add(wedge_mesh()),
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
    });
    commands.insert_resource(material_textures(&mut images));
    let rig = CameraRig::default();
    commands.spawn((
        Camera3d::default(),
        rig,
        // Lighting's FogStart and FogEnd move this; by default it is too far away to see.
        bevy::pbr::DistanceFog {
            color: Color::srgb(0.75, 0.75, 0.75),
            falloff: bevy::pbr::FogFalloff::Linear { start: 0.0, end: 100_000.0 },
            ..default()
        },
        Transform::from_xyz(-18.0, 16.0, 24.0).with_rotation(rig.rotation()),
    ));
    commands.spawn((
        Sun,
        DirectionalLight { illuminance: 9_000.0, shadow_maps_enabled: true, ..default() },
        // Two shadow cascades instead of four: each one draws every part
        // again, which a big map can't afford.
        bevy::light::CascadeShadowConfigBuilder {
            num_cascades: 2,
            first_cascade_far_bound: 40.0,
            maximum_distance: 220.0,
            ..default()
        }
        .build(),
        Transform::from_xyz(8.0, 20.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(ClearColor(Color::srgb(0.53, 0.72, 0.9)));
}

/// The world's main light, moved by the time of day.
#[derive(Component)]
struct Sun;

/// How much the 3D view draws, like Roblox's automatic graphics quality:
/// when frames come too slowly for a few seconds it steps down (smoothing
/// edges off, then shadows off, then a shorter view distance), and steps
/// back up once there is room again.
#[derive(Resource, Debug)]
pub struct GraphicsQuality {
    /// 3 is everything; 0 is the least.
    pub level: u8,
    /// The highest level to go back up to for now (lowered when a level
    /// turned out too slow, so it doesn't flip back and forth).
    ceiling: u8,
    /// Smoothed seconds a frame.
    average: f32,
    slow_for: f32,
    fast_for: f32,
    since_drop: f32,
    applied: Option<u8>,
}

impl Default for GraphicsQuality {
    fn default() -> Self {
        Self {
            level: 3,
            ceiling: 3,
            average: 1.0 / 60.0,
            slow_for: 0.0,
            fast_for: 0.0,
            since_drop: 0.0,
            applied: None,
        }
    }
}

impl GraphicsQuality {
    /// How far away parts are still drawn at each level, in studs.
    const VIEW_DISTANCE: [f32; 4] = [350.0, 600.0, 1000.0, 1000.0];

    /// Counts one frame of `dt` seconds; returns whether the level changed.
    pub fn frame(&mut self, dt: f32) -> bool {
        let dt = dt.clamp(0.0, 0.5);
        self.average += (dt - self.average) * 0.1;
        self.since_drop += dt;
        self.slow_for = if self.average > 1.0 / 40.0 { self.slow_for + dt } else { 0.0 };
        self.fast_for = if self.average < 1.0 / 55.0 { self.fast_for + dt } else { 0.0 };
        // A minute after a drop, the level that was too slow may be tried again.
        if self.since_drop > 60.0 {
            self.ceiling = 3;
        }
        if self.slow_for > 2.0 && self.level > 0 {
            self.ceiling = self.level - 1;
            self.level -= 1;
            self.slow_for = 0.0;
            self.since_drop = 0.0;
            return true;
        }
        if self.fast_for > 6.0 && self.level < self.ceiling {
            self.level += 1;
            self.fast_for = 0.0;
            return true;
        }
        false
    }

    /// Whether edges are smoothed (multisampling).
    pub fn smooth_edges(&self) -> bool {
        self.level >= 3
    }

    /// Whether the sun casts shadows.
    pub fn shadows(&self) -> bool {
        self.level >= 2
    }

    /// How far away parts are still drawn, in studs.
    pub fn view_distance(&self) -> f32 {
        Self::VIEW_DISTANCE[usize::from(self.level.min(3))]
    }
}

fn auto_quality(
    time: Res<Time>,
    mut quality: ResMut<GraphicsQuality>,
    mut cameras: Query<(&mut Msaa, &mut Projection), With<CameraRig>>,
    mut sun: Query<&mut DirectionalLight, With<Sun>>,
) {
    quality.frame(time.delta_secs());
    if quality.applied == Some(quality.level) {
        return;
    }
    quality.applied = Some(quality.level);
    for (mut msaa, mut projection) in &mut cameras {
        let want = if quality.smooth_edges() { Msaa::Sample4 } else { Msaa::Off };
        if *msaa != want {
            *msaa = want;
        }
        if let Projection::Perspective(p) = &mut *projection {
            p.far = quality.view_distance();
        }
    }
    for mut light in &mut sun {
        light.shadow_maps_enabled = quality.shadows();
    }
}

/// How the sky and sun look at a time of day (hours, 0 to 24) and brightness.
pub fn sky_at(clock: f32, brightness: f32) -> (Vec3, f32, Color, f32) {
    use std::f32::consts::PI;
    // Sunrise at 6, noon overhead, sunset at 18.
    let angle = (clock - 6.0) / 12.0 * PI;
    let height = angle.sin();
    let dir = Vec3::new(angle.cos(), height.max(0.05), 0.35).normalize();
    let day = (height * 3.0 + 0.4).clamp(0.0, 1.0);
    let dusk = (1.0 - (height.abs() * 4.0)).clamp(0.0, 1.0);
    let night = Vec3::new(0.02, 0.03, 0.09);
    let noon = Vec3::new(0.53, 0.72, 0.9);
    let glow = Vec3::new(0.95, 0.55, 0.35);
    let sky = night.lerp(noon, day).lerp(glow, dusk * 0.5 * day.max(0.3));
    let scale = (brightness / 2.0).clamp(0.0, 3.0);
    let illuminance = (300.0 + 8_700.0 * day) * scale;
    let ambient = (60.0 + 440.0 * day) * scale.max(0.2);
    (dir, illuminance, Color::srgb(sky.x, sky.y, sky.z), ambient)
}

fn update_lighting(
    engine: NonSend<EngineState>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut fogs: Query<&mut bevy::pbr::DistanceFog>,
) {
    let (clock, brightness, tint, exposure, fog, haze) = {
        let tree = engine.tree.borrow();
        let lighting = tree.service(ClassKind::Lighting);
        let atmosphere =
            lighting.and_then(|l| tree.find_first_child_of_class(l, ClassKind::Atmosphere).ok().flatten());
        let read = |p| lighting.and_then(|l| tree.get_number(l, p)).map(|n| n as f32);
        let colour = |p| lighting.and_then(|l| tree.get_color(l, p));
        (
            read(Property::ClockTime).unwrap_or(14.0),
            read(Property::Brightness).unwrap_or(2.0),
            // Outdoors, Roblox lights with OutdoorAmbient (Ambient is for indoors);
            // the brighter of the two lights the world here.
            {
                let ambient =
                    colour(Property::Ambient).unwrap_or(libroxel_instance::Color3::new(0.5, 0.5, 0.5));
                match lighting.map(|l| tree.get_extra(l, "OutdoorAmbient")) {
                    Some(Ok(Some(libroxel_instance::PropertyValue::Color3(o))))
                        if o.r.max(o.g).max(o.b) > ambient.r.max(ambient.g).max(ambient.b) =>
                    {
                        o
                    }
                    _ => ambient,
                }
            },
            match lighting.map(|l| tree.get_extra(l, "ExposureCompensation")) {
                Some(Ok(Some(libroxel_instance::PropertyValue::Number(e)))) => e.clamp(-3.0, 3.0) as f32,
                _ => 0.0,
            },
            (
                colour(Property::FogColor).unwrap_or(libroxel_instance::Color3::new(0.75, 0.75, 0.75)),
                read(Property::FogStart).unwrap_or(0.0),
                read(Property::FogEnd).unwrap_or(100_000.0),
            ),
            atmosphere.map(|a| atmosphere_fog(&tree, a)),
        )
    };
    let (dir, illuminance, sky, _) = sky_at(clock, brightness);
    // Exposure brightens (or darkens) everything, a stop at a time.
    let exposure = 2f32.powf(exposure);
    let illuminance = illuminance * exposure;
    // The ambient colour lights every face the same at any time of day, as in
    // Roblox: a game that keeps it bright at night has bright nights. 0.5 grey
    // is the normal amount; coloured ambient tints the shadows.
    let strongest = tint.r.max(tint.g).max(tint.b);
    let ambient_brightness = 500.0 * strongest * 2.0 * exposure;
    let ambient_color = if strongest > 0.0 {
        Color::srgb(tint.r / strongest, tint.g / strongest, tint.b / strongest)
    } else {
        Color::WHITE
    };
    if ambient.color != ambient_color {
        ambient.color = ambient_color;
    }
    let (fog_colour, fog_start, fog_end) = fog;
    let fog_end = fog_end.max(fog_start + 0.01);
    // An atmosphere in Lighting replaces the fog with haze that thickens
    // with distance.
    let (fog_colour, falloff) = match haze {
        Some((colour, density)) => (colour, bevy::pbr::FogFalloff::Exponential { density }),
        None => (fog_colour, bevy::pbr::FogFalloff::Linear { start: fog_start, end: fog_end }),
    };
    let fog_colour = Color::srgb(fog_colour.r, fog_colour.g, fog_colour.b);
    for mut f in &mut fogs {
        let changed = match (&f.falloff, &falloff) {
            (
                bevy::pbr::FogFalloff::Linear { start, end },
                bevy::pbr::FogFalloff::Linear { start: s, end: e },
            ) => start != s || end != e,
            (
                bevy::pbr::FogFalloff::Exponential { density },
                bevy::pbr::FogFalloff::Exponential { density: d },
            ) => density != d,
            _ => true,
        };
        if changed {
            f.falloff = falloff.clone();
        }
        if f.color != fog_colour {
            f.color = fog_colour;
        }
    }
    for (mut light, mut transform) in &mut sun {
        if (light.illuminance - illuminance).abs() > 0.5 {
            light.illuminance = illuminance;
        }
        let wanted = Transform::from_translation(dir * 30.0).looking_at(Vec3::ZERO, Vec3::Y);
        if transform.rotation.angle_between(wanted.rotation) > 1e-4 {
            *transform = wanted;
        }
    }
    if clear.0 != sky {
        clear.0 = sky;
    }
    if (ambient.brightness - ambient_brightness).abs() > 0.5 {
        ambient.brightness = ambient_brightness;
    }
}

/// An atmosphere's haze: its colour and how fast it thickens (per stud).
pub fn atmosphere_fog(tree: &InstanceTree, atmosphere: InstanceId) -> (libroxel_instance::Color3, f32) {
    let n = |p, d: f64| tree.get_number(atmosphere, p).unwrap_or(d) as f32;
    let density = n(Property::Density, 0.395).clamp(0.0, 1.0) * 0.004
        + n(Property::Haze, 0.0).clamp(0.0, 10.0) * 0.0015;
    let colour = tree
        .get_color(atmosphere, Property::Color)
        .unwrap_or(libroxel_instance::Color3::new(0.78, 0.78, 0.78));
    (colour, density)
}

pub(crate) fn material_for(appearance: &PartAppearance, textures: &MaterialTextures) -> StandardMaterial {
    let mut alpha = (1.0 - appearance.transparency).clamp(0.0, 1.0);
    if appearance.material == Material::Glass {
        alpha = alpha.min(0.45);
    }
    let mut material = StandardMaterial {
        base_color: appearance.color.with_alpha(alpha),
        alpha_mode: if alpha < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
        perceptual_roughness: 0.85,
        uv_transform: bevy::math::Affine2::from_scale(Vec2::splat(appearance.texture_repeat)),
        ..default()
    };
    match appearance.material {
        Material::Plastic => {}
        Material::SmoothPlastic => material.perceptual_roughness = 0.35,
        Material::Neon => {
            material.unlit = true;
            material.emissive = appearance.color.to_linear() * 2.0;
        }
        Material::Metal => {
            material.metallic = 0.65;
            material.perceptual_roughness = 0.3;
        }
        Material::Glass => {
            material.perceptual_roughness = 0.05;
            material.reflectance = 0.9;
        }
        Material::Wood => material.base_color_texture = Some(textures.wood.clone()),
        Material::Brick => material.base_color_texture = Some(textures.brick.clone()),
        Material::Grass => {
            material.base_color_texture = Some(textures.grass.clone());
            material.perceptual_roughness = 1.0;
        }
        Material::Sand => {
            material.base_color_texture = Some(textures.sand.clone());
            material.perceptual_roughness = 1.0;
        }
        Material::Rock => {
            material.base_color_texture = Some(textures.rock.clone());
            material.perceptual_roughness = 0.95;
        }
        Material::Snow => {
            material.base_color_texture = Some(textures.snow.clone());
            material.perceptual_roughness = 0.6;
        }
        Material::Water => {
            material.perceptual_roughness = 0.08;
            material.reflectance = 0.7;
        }
        Material::Air => {}
        // Stony: the rock surface, rough.
        Material::Slate
        | Material::Granite
        | Material::Basalt
        | Material::Limestone
        | Material::Cobblestone
        | Material::Pebble
        | Material::CrackedLava => {
            material.base_color_texture = Some(textures.rock.clone());
            material.perceptual_roughness = 0.9;
            if appearance.material == Material::CrackedLava {
                material.emissive = appearance.color.to_linear() * 0.6;
            }
        }
        Material::Marble => {
            material.base_color_texture = Some(textures.rock.clone());
            material.perceptual_roughness = 0.25;
        }
        // Grainy: the sand surface.
        Material::Concrete
        | Material::Asphalt
        | Material::Pavement
        | Material::Sandstone
        | Material::Salt
        | Material::Ground
        | Material::Mud
        | Material::Fabric => {
            material.base_color_texture = Some(textures.sand.clone());
            material.perceptual_roughness = 1.0;
        }
        Material::LeafyGrass => {
            material.base_color_texture = Some(textures.grass.clone());
            material.perceptual_roughness = 1.0;
        }
        Material::WoodPlanks => material.base_color_texture = Some(textures.wood.clone()),
        Material::Ice | Material::Glacier => {
            material.perceptual_roughness = 0.1;
            material.reflectance = 0.8;
        }
        Material::Foil => {
            material.metallic = 0.9;
            material.perceptual_roughness = 0.2;
        }
        Material::DiamondPlate | Material::CorrodedMetal => {
            material.base_color_texture = Some(textures.rock.clone());
            material.metallic = 0.6;
            material.perceptual_roughness = 0.5;
        }
        Material::ForceField => {
            material.unlit = true;
            material.emissive = appearance.color.to_linear();
            material.base_color = appearance.color.with_alpha(alpha.min(0.5));
            material.alpha_mode = AlphaMode::Add;
        }
    }
    let shine = appearance.reflectance.clamp(0.0, 1.0);
    if shine > 0.0 {
        material.metallic = material.metallic.max(shine);
        material.reflectance = material.reflectance.max(0.5 + 0.5 * shine);
        material.perceptual_roughness = material.perceptual_roughness.min(1.0 - 0.9 * shine);
    }
    material
}

pub(crate) fn visibility_for(appearance: &PartAppearance) -> Visibility {
    if appearance.transparency >= 0.999 { Visibility::Hidden } else { Visibility::Inherited }
}

/// One material for every part that looks the same, so the GPU draws them
/// together in big batches instead of one by one (a big map has thousands
/// of parts but only a few hundred looks).
#[derive(Resource, Default)]
pub(crate) struct SharedMaterials(std::collections::HashMap<LookKey, Handle<StandardMaterial>>);

/// A part's look, rounded to what can be told apart on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LookKey([u16; 4], u16, Material, u16, u16);

impl LookKey {
    fn of(a: &PartAppearance) -> Self {
        let c = a.color.to_srgba();
        let q = |x: f32| (x.clamp(0.0, 1.0) * 1000.0).round() as u16;
        let colour = [q(c.red), q(c.green), q(c.blue), q(c.alpha)];
        // Textured materials repeat by size: a quarter step is as fine as shows.
        let repeat = (a.texture_repeat * 4.0).round().clamp(0.0, 60000.0) as u16;
        Self(colour, q(a.transparency), a.material, repeat, q(a.reflectance))
    }
}

impl SharedMaterials {
    pub(crate) fn get(
        &mut self,
        appearance: &PartAppearance,
        textures: &MaterialTextures,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        self.0
            .entry(LookKey::of(appearance))
            .or_insert_with(|| materials.add(material_for(appearance, textures)))
            .clone()
    }
}

fn attach_visuals(
    mut commands: Commands,
    meshes: Res<PartMeshes>,
    textures: Res<MaterialTextures>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shared: ResMut<SharedMaterials>,
    new_parts: Query<(Entity, &PartAppearance, &PartGeometry), Added<PartLink>>,
) {
    for (entity, appearance, geometry) in &new_parts {
        let mut e = commands.entity(entity);
        e.insert((
            Mesh3d(meshes.get(geometry.0)),
            MeshMaterial3d(shared.get(appearance, &textures, &mut materials)),
            visibility_for(appearance),
        ));
        if !appearance.shadow {
            e.insert(bevy::light::NotShadowCaster);
        }
    }
}

/// A part whose look changed: its material, visibility and whether it
/// casts no shadow now.
type ChangedLook<'a> = (
    Entity,
    &'a PartAppearance,
    &'a mut MeshMaterial3d<StandardMaterial>,
    &'a mut Visibility,
    Has<bevy::light::NotShadowCaster>,
    &'a PartMeshTexture,
);

fn update_materials(
    mut commands: Commands,
    textures: Res<MaterialTextures>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shared: ResMut<SharedMaterials>,
    mut changed: Query<ChangedLook, Changed<PartAppearance>>,
) {
    for (entity, appearance, mut handle, mut visibility, shadowless, texture) in &mut changed {
        // A part with its own picture has its own material (see use_mesh_textures).
        if texture.0.is_empty() {
            let wanted = shared.get(appearance, &textures, &mut materials);
            if handle.0 != wanted {
                handle.0 = wanted;
            }
        }
        visibility.set_if_neq(visibility_for(appearance));
        if appearance.shadow == shadowless {
            if appearance.shadow {
                commands.entity(entity).remove::<bevy::light::NotShadowCaster>();
            } else {
                commands.entity(entity).insert(bevy::light::NotShadowCaster);
            }
        }
    }
}

fn update_meshes(
    meshes: Res<PartMeshes>,
    mut changed: Query<(&PartGeometry, &mut Mesh3d), Changed<PartGeometry>>,
) {
    for (geometry, mut mesh) in &mut changed {
        let wanted = meshes.get(geometry.0);
        if mesh.0 != wanted {
            mesh.0 = wanted;
        }
    }
}

/// Draws parts that have a model file with that model (once it's read),
/// and puts their own shape back when the file is taken away.
fn use_mesh_files(
    part_meshes: Res<PartMeshes>,
    mut files: ResMut<crate::mesh_files::MeshFiles>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut parts: Query<(Ref<PartMeshFile>, &PartGeometry, &mut Mesh3d)>,
) {
    for (file, geometry, mut mesh) in &mut parts {
        let wanted = if file.0.is_empty() {
            if !file.is_changed() {
                continue;
            }
            part_meshes.get(geometry.0)
        } else {
            files.get(&file.0, &mut meshes).unwrap_or_else(|| part_meshes.get(geometry.0))
        };
        if mesh.0 != wanted {
            mesh.0 = wanted;
        }
    }
}

/// Pictures for parts' models, by name (None when one couldn't be read).
#[derive(Resource, Default)]
struct MeshTextures {
    loaded: std::collections::HashMap<String, Option<Handle<Image>>>,
    generation: u64,
}

/// Wraps a part's picture over its model, in place of its colour.
fn use_mesh_textures(
    textures: Res<MaterialTextures>,
    mut cache: ResMut<MeshTextures>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shared: ResMut<SharedMaterials>,
    mut parts: Query<(Ref<PartMeshTexture>, Ref<PartAppearance>, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let generation = crate::pictures::generation();
    if cache.generation != generation {
        cache.loaded.clear();
        cache.generation = generation;
    }
    for (texture, appearance, mut handle) in &mut parts {
        if !texture.is_changed() && !appearance.is_changed() {
            continue;
        }
        if texture.0.is_empty() {
            if texture.is_changed() {
                handle.0 = shared.get(&appearance, &textures, &mut materials);
            }
            continue;
        }
        // Its own material, so the picture isn't put on every part that looks the same.
        let own = materials.add(material_for(&appearance, &textures));
        handle.0 = own;
        let Some(mut material) = materials.get_mut(&handle.0) else { continue };
        let picture = cache
            .loaded
            .entry(texture.0.clone())
            .or_insert_with(|| {
                let bytes = crate::pictures::bytes_for(&texture.0)?;
                let image = crate::pictures::decode(&bytes, bevy::image::ImageSampler::linear())?;
                Some(images.add(image))
            })
            .clone();
        if let Some(picture) = picture {
            let alpha = material.base_color.alpha();
            material.base_color = Color::srgba(1.0, 1.0, 1.0, alpha);
            material.base_color_texture = Some(picture);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn camera_controls(
    capture: Res<InputCapture>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    gamepads: Query<&Gamepad>,
    mut touch: ResMut<crate::devices::VirtualInput>,
    mut rigs: Query<(&mut CameraRig, &mut Transform)>,
) {
    let looking = mouse.pressed(MouseButton::Right) && !capture.pointer;
    let pad = crate::devices::read_pads(gamepads.iter());
    // Fingers dragging and the right stick look round too.
    let turn = pad.look * crate::devices::LOOK_SPEED * time.delta_secs() + touch.look * 0.006;
    let pad_zoom = pad.zoom + std::mem::take(&mut touch.zoom);
    touch.look = Vec2::ZERO;
    let scroll_lines = if capture.pointer {
        0.0
    } else {
        match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        }
    };
    for (mut rig, mut transform) in &mut rigs {
        if looking {
            rig.yaw -= motion.delta.x * 0.005;
            rig.pitch = (rig.pitch - motion.delta.y * 0.005).clamp(-1.4, 1.4);
        }
        rig.yaw -= turn.x;
        rig.pitch = (rig.pitch + turn.y).clamp(-1.4, 1.4);
        // Following, the comma and full stop keys swing the camera round
        // in steps, and I and O zoom, as well as the mouse.
        let mut zoom_lines = scroll_lines + pad_zoom;
        rig.yaw += pad.swing * std::f32::consts::FRAC_PI_4;
        if rig.mode == CameraMode::Follow && !capture.keyboard {
            if keys.just_pressed(KeyCode::Comma) {
                rig.yaw += std::f32::consts::FRAC_PI_4;
            }
            if keys.just_pressed(KeyCode::Period) {
                rig.yaw -= std::f32::consts::FRAC_PI_4;
            }
            if keys.just_pressed(KeyCode::KeyI) {
                zoom_lines += 1.0;
            }
            if keys.just_pressed(KeyCode::KeyO) {
                zoom_lines -= 1.0;
            }
        }
        transform.rotation = rig.rotation();
        match rig.mode {
            CameraMode::Fly => {
                if !capture.keyboard && looking {
                    let mut dir = Vec3::ZERO;
                    let (fwd, right) = (transform.forward().as_vec3(), transform.right().as_vec3());
                    if keys.pressed(KeyCode::KeyW) {
                        dir += fwd;
                    }
                    if keys.pressed(KeyCode::KeyS) {
                        dir -= fwd;
                    }
                    if keys.pressed(KeyCode::KeyD) {
                        dir += right;
                    }
                    if keys.pressed(KeyCode::KeyA) {
                        dir -= right;
                    }
                    if keys.pressed(KeyCode::KeyE) {
                        dir += Vec3::Y;
                    }
                    if keys.pressed(KeyCode::KeyQ) {
                        dir -= Vec3::Y;
                    }
                    let boost = if keys.pressed(KeyCode::ShiftLeft) { 3.0 } else { 1.0 };
                    transform.translation +=
                        dir.normalize_or_zero() * rig.fly_speed * boost * time.delta_secs();
                }
                if scroll_lines != 0.0 {
                    let fwd = transform.forward().as_vec3();
                    transform.translation += fwd * scroll_lines * 3.0;
                }
            }
            CameraMode::Follow => {
                rig.zoom = crate::camera::zoom(rig.zoom, zoom_lines);
            }
        }
    }
}

/// The script-facing key for a Bevy key, if scripts know it.
pub fn script_key(key: KeyCode) -> Option<libroxel_instance::KeyCode> {
    use libroxel_instance::KeyCode as K;
    Some(match key {
        KeyCode::KeyA => K::A,
        KeyCode::KeyB => K::B,
        KeyCode::KeyC => K::C,
        KeyCode::KeyD => K::D,
        KeyCode::KeyE => K::E,
        KeyCode::KeyF => K::F,
        KeyCode::KeyG => K::G,
        KeyCode::KeyH => K::H,
        KeyCode::KeyI => K::I,
        KeyCode::KeyJ => K::J,
        KeyCode::KeyK => K::K,
        KeyCode::KeyL => K::L,
        KeyCode::KeyM => K::M,
        KeyCode::KeyN => K::N,
        KeyCode::KeyO => K::O,
        KeyCode::KeyP => K::P,
        KeyCode::KeyQ => K::Q,
        KeyCode::KeyR => K::R,
        KeyCode::KeyS => K::S,
        KeyCode::KeyT => K::T,
        KeyCode::KeyU => K::U,
        KeyCode::KeyV => K::V,
        KeyCode::KeyW => K::W,
        KeyCode::KeyX => K::X,
        KeyCode::KeyY => K::Y,
        KeyCode::KeyZ => K::Z,
        KeyCode::Digit0 => K::Zero,
        KeyCode::Digit1 => K::One,
        KeyCode::Digit2 => K::Two,
        KeyCode::Digit3 => K::Three,
        KeyCode::Digit4 => K::Four,
        KeyCode::Digit5 => K::Five,
        KeyCode::Digit6 => K::Six,
        KeyCode::Digit7 => K::Seven,
        KeyCode::Digit8 => K::Eight,
        KeyCode::Digit9 => K::Nine,
        KeyCode::Space => K::Space,
        KeyCode::Enter => K::Return,
        KeyCode::Tab => K::Tab,
        KeyCode::Backspace => K::Backspace,
        KeyCode::Escape => K::Escape,
        KeyCode::ShiftLeft => K::LeftShift,
        KeyCode::ShiftRight => K::RightShift,
        KeyCode::ControlLeft => K::LeftControl,
        KeyCode::ControlRight => K::RightControl,
        KeyCode::AltLeft => K::LeftAlt,
        KeyCode::AltRight => K::RightAlt,
        KeyCode::ArrowUp => K::Up,
        KeyCode::ArrowDown => K::Down,
        KeyCode::ArrowLeft => K::Left,
        KeyCode::ArrowRight => K::Right,
        KeyCode::Slash => K::Slash,
        _ => return None,
    })
}

/// Passes key presses and releases to the play session's scripts and prompts.
fn forward_keys(
    mut engine: NonSendMut<EngineState>,
    capture: Res<InputCapture>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    touch: Res<crate::devices::VirtualInput>,
    mut last: Local<libroxel_instance::UserInputType>,
) {
    use libroxel_instance::UserInputType;
    let engine = &mut *engine;
    let Some(session) = engine.session.as_mut() else { return };
    // A controller's buttons stand in for the mouse button and keys.
    let pad = crate::devices::read_pads(gamepads.iter());
    if let Some(down) = pad.click {
        if down {
            session.input_began(libroxel_script::UserInput::MouseButton1, false);
        } else {
            session.input_ended(libroxel_script::UserInput::MouseButton1, false);
        }
    }
    if let Some(down) = pad.interact {
        let key = session.active_prompt().map(|p| p.key).unwrap_or(libroxel_instance::KeyCode::E);
        if down {
            session.input_began(libroxel_script::UserInput::Key(key), false);
        } else {
            session.input_ended(libroxel_script::UserInput::Key(key), false);
        }
    }
    if pad.tool {
        session.input_began(libroxel_script::UserInput::Key(libroxel_instance::KeyCode::One), false);
        session.input_ended(libroxel_script::UserInput::Key(libroxel_instance::KeyCode::One), false);
    }
    // What scripts are told the player plays with.
    if pad.used {
        *last = UserInputType::Gamepad1;
    } else if touch.stick != Vec2::ZERO || touch.jump || touch.look != Vec2::ZERO {
        *last = UserInputType::Touch;
    } else if keys.get_just_pressed().next().is_some() {
        *last = UserInputType::Keyboard;
    } else if mouse.just_pressed(MouseButton::Left) {
        *last = UserInputType::MouseButton1;
    } else if mouse.just_pressed(MouseButton::Right) {
        *last = UserInputType::MouseButton2;
    }
    session.set_devices(libroxel_script::Devices {
        touch: touch.touched,
        gamepad: !gamepads.is_empty(),
        last: *last,
    });
    if !capture.keyboard {
        for key in keys.get_just_pressed().filter_map(|k| script_key(*k)) {
            session.input_began(libroxel_script::UserInput::Key(key), false);
        }
    }
    // Releases always go through, so nothing is left held down.
    for key in keys.get_just_released().filter_map(|k| script_key(*k)) {
        session.input_ended(libroxel_script::UserInput::Key(key), false);
    }
    engine.log.extend(session.take_log());
}

/// WASD relative to where the camera faces, Space to jump.
fn play_input(
    engine: NonSend<EngineState>,
    capture: Res<InputCapture>,
    keys: Res<ButtonInput<KeyCode>>,
    rigs: Query<&Transform, With<CameraRig>>,
    gamepads: Query<&Gamepad>,
    touch: Res<crate::devices::VirtualInput>,
    mut input: ResMut<PlayInput>,
) {
    input.0 = CharacterInput::default();
    if !engine.is_playing() {
        return;
    }
    // A controller's left stick and the on-screen stick walk (and drive).
    let pad = crate::devices::read_pads(gamepads.iter());
    let stick = (pad.stick + touch.stick).clamp_length_max(1.0);
    let look = rigs.iter().next().map(|t| t.forward().as_vec3()).unwrap_or(Vec3::NEG_Z);
    let walk = crate::devices::walk_direction(stick, look);
    input.0 = CharacterInput {
        move_dir: libroxel_instance::Vec3::new(walk.x, 0.0, walk.z),
        jump: pad.jump || touch.jump,
        throttle: stick.y,
        steer: stick.x,
    };
    if capture.keyboard {
        return;
    }
    // Relative to where the camera looks, flattened onto the ground.
    let look = rigs.iter().next().map(|t| t.forward().as_vec3()).unwrap_or(Vec3::NEG_Z);
    let forward = Vec3::new(look.x, 0.0, look.z).try_normalize().unwrap_or(Vec3::NEG_Z);
    let right = Vec3::new(-forward.z, 0.0, forward.x);
    let mut dir = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        dir += forward;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        dir -= forward;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        dir += right;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        dir -= right;
    }
    let dir = dir.normalize_or_zero();
    let axis = |plus: [KeyCode; 2], minus: [KeyCode; 2]| {
        (plus.iter().any(|k| keys.pressed(*k)) as i32 - minus.iter().any(|k| keys.pressed(*k)) as i32) as f32
    };
    if dir != Vec3::ZERO {
        input.0.move_dir = libroxel_instance::Vec3::new(dir.x, 0.0, dir.z);
    }
    input.0.jump |= keys.pressed(KeyCode::Space);
    let throttle = axis([KeyCode::KeyW, KeyCode::ArrowUp], [KeyCode::KeyS, KeyCode::ArrowDown]);
    let steer = axis([KeyCode::KeyD, KeyCode::ArrowRight], [KeyCode::KeyA, KeyCode::ArrowLeft]);
    if throttle != 0.0 {
        input.0.throttle = throttle;
    }
    if steer != 0.0 {
        input.0.steer = steer;
    }
}

/// Copies the camera's transform into its global transform straight away
/// (cameras have no parent), instead of waiting for transform propagation.
fn refresh_camera_global(mut rigs: Query<(&Transform, &mut GlobalTransform), With<CameraRig>>) {
    for (transform, mut global) in &mut rigs {
        let fresh = GlobalTransform::from(*transform);
        if *global != fresh {
            *global = fresh;
        }
    }
}

/// What the follow camera follows: the part, its model, where it is, and
/// which way it faces (degrees).
struct Followed {
    root: InstanceId,
    model: Option<InstanceId>,
    position: libroxel_instance::Vec3,
    yaw: f32,
    /// Which way the vehicle being driven faces (degrees), if any.
    driving: Option<f32>,
}

/// How quickly the camera swings round behind a vehicle that turns.
const CHASE_RATE: f32 = 3.0;

/// The angle `from` eased round towards `to` (radians), the short way.
fn ease_angle(from: f32, to: f32, rate: f32, dt: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let gap = (to - from + PI).rem_euclid(TAU) - PI;
    from + gap * (1.0 - (-rate * dt).exp())
}

/// Places a follow camera round `followed`: swung behind a newly followed
/// character, zoom eased, and pulled in front of any wall in the way.
fn place_follow(
    rig: &mut CameraRig,
    transform: &mut Transform,
    tree: &InstanceTree,
    followed: &Followed,
    dt: f32,
) {
    use crate::camera;
    if rig.following != Some(followed.root) {
        if rig.following.is_none() {
            // Just started playing: start behind, at the usual distance.
            (rig.zoom, rig.distance) = (camera::START_DISTANCE, camera::START_DISTANCE);
            rig.pitch = camera::START_PITCH;
        }
        rig.yaw = camera::behind(followed.yaw);
        rig.following = Some(followed.root);
        rig.shown = rig.distance;
    }
    // Driving: the camera keeps swinging round behind the vehicle, so
    // forward on the keys is always forward on the screen.
    if let Some(yaw) = followed.driving {
        rig.yaw = ease_angle(rig.yaw, camera::behind(yaw), CHASE_RATE, dt);
    }
    rig.distance = camera::ease(rig.distance, rig.zoom, dt);
    let focus = to_bevy(followed.position) + Vec3::Y * camera::FOCUS_HEIGHT;
    transform.rotation = rig.rotation();
    let back = -transform.forward().as_vec3();
    let free = camera::unblocked(
        tree,
        libroxel_instance::Vec3::new(focus.x, focus.y, focus.z),
        libroxel_instance::Vec3::new(back.x, back.y, back.z),
        rig.distance,
        followed.model,
    );
    // Walls cut in at once; when the way clears the camera eases back out.
    rig.shown = if free < rig.shown { free } else { camera::ease(rig.shown, free, dt) };
    transform.translation = focus + back * rig.shown;
}

/// Whether the follow camera is in the character's head.
pub fn first_person(rig: &CameraRig) -> bool {
    rig.mode == CameraMode::Follow && rig.following.is_some() && rig.shown <= crate::camera::FIRST_PERSON
}

fn follow_camera(
    engine: NonSend<EngineState>,
    time: Res<Time>,
    mut rigs: Query<(&mut CameraRig, &mut Transform, &mut Projection)>,
) {
    let dt = time.delta_secs().min(0.1);
    let session = engine.session.as_ref();
    let followed = session.and_then(|s| s.focus()).and_then(|root| {
        let tree = engine.tree.borrow();
        if tree.is_destroyed(root).unwrap_or(true) {
            return None;
        }
        let position = tree.get_vec3(root, Property::Position)?;
        let yaw = tree.get_vec3(root, Property::Orientation).unwrap_or_default().y;
        let character = session.and_then(|s| s.character());
        let model = character.map(|c| c.model);
        let driving = character.and_then(|c| {
            tree.descendants_of_class(tree.workspace(), ClassKind::VehicleSeat)
                .into_iter()
                .find(|s| tree.get_ref(*s, Property::Occupant) == Some(c.humanoid))
                .and_then(|s| tree.get_vec3(s, Property::Orientation))
                .map(|o| o.y)
        });
        Some(Followed { root, model, position, yaw, driving })
    });
    // The workspace camera: scripts can take it over, or read where it is.
    let camera = session.and_then(|s| s.camera());
    let (scripted, fov) = camera.map_or((false, 70.0), |cam| {
        let tree = engine.tree.borrow();
        let scripted = tree.get_enum(cam, Property::CameraType)
            == Some(EnumValue::CameraType(libroxel_instance::CameraType::Scriptable));
        (scripted, tree.get_number(cam, Property::FieldOfView).unwrap_or(70.0) as f32)
    });
    for (mut rig, mut transform, mut projection) in &mut rigs {
        let wanted_fov = if engine.is_playing() { fov.to_radians() } else { 70f32.to_radians() };
        if let Projection::Perspective(p) = &mut *projection
            && (p.fov - wanted_fov).abs() > 1e-4
        {
            p.fov = wanted_fov;
        }
        if engine.is_playing() {
            rig.mode = CameraMode::Follow;
        } else if rig.mode == CameraMode::Follow {
            rig.mode = CameraMode::Fly;
        }
        if rig.mode != CameraMode::Follow {
            rig.following = None;
            continue;
        }
        let Some(cam) = camera else {
            if let Some(f) = &followed {
                place_follow(&mut rig, &mut transform, &engine.tree.borrow(), f, dt);
            }
            continue;
        };
        let mut tree = engine.tree.borrow_mut();
        if scripted {
            let pos = tree.get_vec3(cam, Property::Position).unwrap_or_default();
            let orientation = tree.get_vec3(cam, Property::Orientation).unwrap_or_default();
            transform.translation = to_bevy(pos);
            transform.rotation = crate::physics::orientation_quat(orientation);
            continue;
        }
        if let Some(f) = &followed {
            place_follow(&mut rig, &mut transform, &tree, f, dt);
        }
        // Tell scripts where the camera is.
        let (y, x, z) = transform.rotation.to_euler(EulerRot::YXZ);
        let round = |r: f32| (r.to_degrees() * 1000.0).round() / 1000.0;
        let t = transform.translation;
        let _ = tree.set(
            cam,
            Property::Position,
            libroxel_instance::PropertyValue::Vec3(libroxel_instance::Vec3::new(t.x, t.y, t.z)),
        );
        let _ = tree.set(
            cam,
            Property::Orientation,
            libroxel_instance::PropertyValue::Vec3(libroxel_instance::Vec3::new(
                round(x),
                round(y),
                round(z),
            )),
        );
    }
}

/// In first person, hides the player's own body so it doesn't fill the
/// view; shows it again when the camera pulls back out.
fn hide_self_in_first_person(
    engine: NonSend<EngineState>,
    entities: Res<PartEntities>,
    rigs: Query<&CameraRig>,
    mut hidden: Local<Vec<InstanceId>>,
    mut parts: Query<(&PartAppearance, &mut Visibility), With<PartLink>>,
) {
    let inside = rigs.iter().any(first_person);
    let wanted: Vec<InstanceId> = match engine.session.as_ref().and_then(|s| s.character()) {
        Some(c) if inside => engine.tree.borrow().descendants_of_class(c.model, ClassKind::Part),
        _ => Vec::new(),
    };
    if wanted == *hidden {
        return;
    }
    for id in hidden.iter().filter(|id| !wanted.contains(id)) {
        if let Some((appearance, mut visibility)) = entities.0.get(id).and_then(|e| parts.get_mut(*e).ok()) {
            visibility.set_if_neq(visibility_for(appearance));
        }
    }
    for id in &wanted {
        if let Some((_, mut visibility)) = entities.0.get(id).and_then(|e| parts.get_mut(*e).ok()) {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
    *hidden = wanted;
}

/// Links a light entity to the light instance it shows.
#[derive(Component)]
struct LightLink(InstanceId);

/// Puts a Bevy light at every enabled point and spot light inside a part.
fn sync_lights(
    engine: NonSend<EngineState>,
    mut commands: Commands,
    mut points: Query<(Entity, &LightLink, &mut PointLight, &mut Transform), Without<SpotLight>>,
    mut spots: Query<(Entity, &LightLink, &mut SpotLight, &mut Transform), Without<PointLight>>,
) {
    let tree = engine.tree.borrow();
    let mut wanted: HashMap<InstanceId, (bool, Transform, Color, f32, f32, f32)> = HashMap::new();
    for class in [ClassKind::PointLight, ClassKind::SpotLight] {
        for light in tree.descendants_of_class(tree.workspace(), class) {
            if tree.get_bool(light, Property::Enabled) != Some(true) {
                continue;
            }
            let Some(part) = tree.parent(light).ok().flatten() else { continue };
            let Some(pos) = tree.get_vec3(part, Property::Position) else { continue };
            let rotation = crate::physics::orientation_quat(
                tree.get_vec3(part, Property::Orientation).unwrap_or_default(),
            );
            let c = tree.get_color(light, Property::Color).unwrap_or(libroxel_instance::Color3::WHITE);
            let brightness = tree.get_number(light, Property::Brightness).unwrap_or(1.0).max(0.0) as f32;
            let range = tree.get_number(light, Property::Range).unwrap_or(8.0).max(0.0) as f32;
            let angle = tree.get_number(light, Property::Angle).unwrap_or(90.0) as f32;
            let transform = Transform::from_translation(to_bevy(pos)).with_rotation(rotation);
            wanted.insert(
                light,
                (
                    class == ClassKind::SpotLight,
                    transform,
                    Color::srgb(c.r, c.g, c.b),
                    brightness,
                    range,
                    angle,
                ),
            );
        }
    }
    // Brightness 1 lights the edge of the range about as much as a dim room.
    let intensity = |brightness: f32, range: f32| brightness * 4_000.0 * range.max(1.0).powi(2);
    for (entity, link, mut light, mut transform) in &mut points {
        match wanted.remove(&link.0) {
            Some((false, t, color, brightness, range, _)) => {
                light.color = color;
                light.intensity = intensity(brightness, range);
                light.range = range;
                transform.set_if_neq(t);
            }
            _ => commands.entity(entity).despawn(),
        }
    }
    for (entity, link, mut light, mut transform) in &mut spots {
        match wanted.get(&link.0).copied() {
            Some((true, t, color, brightness, range, angle)) => {
                wanted.remove(&link.0);
                light.color = color;
                light.intensity = intensity(brightness, range);
                light.range = range;
                light.outer_angle = (angle / 2.0).to_radians().min(1.5);
                light.inner_angle = light.outer_angle * 0.7;
                transform.set_if_neq(t);
            }
            _ => commands.entity(entity).despawn(),
        }
    }
    for (id, (spot, transform, color, brightness, range, angle)) in wanted {
        if spot {
            let outer = (angle / 2.0).to_radians().min(1.5);
            commands.spawn((
                LightLink(id),
                transform,
                SpotLight {
                    color,
                    intensity: intensity(brightness, range),
                    range,
                    outer_angle: outer,
                    inner_angle: outer * 0.7,
                    shadow_maps_enabled: false,
                    ..default()
                },
            ));
        } else {
            commands.spawn((
                LightLink(id),
                transform,
                PointLight {
                    color,
                    intensity: intensity(brightness, range),
                    range,
                    shadow_maps_enabled: false,
                    ..default()
                },
            ));
        }
    }
}

/// Marks the glowing ball drawn for an explosion.
#[derive(Component)]
struct BlastVisual;

/// Draws each explosion as a growing, fading ball of fire.
/// Links a drawn rope or spring to its constraint.
#[derive(Component)]
struct RopeLink(InstanceId, libroxel_instance::Color3);

/// Draws every visible rope and spring as a thin rod between its attachments.
fn sync_ropes(
    engine: NonSend<EngineState>,
    mut commands: Commands,
    meshes: Option<Res<PartMeshes>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut drawn: Query<(Entity, &mut RopeLink, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(meshes) = meshes else { return };
    let tree = engine.tree.borrow();
    let mut wanted: HashMap<InstanceId, (Transform, libroxel_instance::Color3)> = HashMap::new();
    for class in [ClassKind::RopeConstraint, ClassKind::SpringConstraint, ClassKind::RodConstraint] {
        for id in tree.descendants_of_class(tree.workspace(), class) {
            if tree.get_bool(id, Property::Visible) != Some(true) {
                continue;
            }
            let point = |p| {
                tree.get_ref(id, p)
                    .and_then(|a| crate::movers::attachment_world(&tree, a))
                    .map(|(v, _)| to_bevy(v))
            };
            let (Some(a), Some(b)) = (point(Property::Attachment0), point(Property::Attachment1)) else {
                continue;
            };
            let length = a.distance(b);
            if length < 1e-3 {
                continue;
            }
            let thick = tree.get_number(id, Property::Thickness).unwrap_or(0.1).max(0.02) as f32;
            // The cylinder mesh lies along X.
            let rotation = Quat::from_rotation_arc(Vec3::X, (b - a) / length);
            let transform = Transform::from_translation((a + b) / 2.0)
                .with_rotation(rotation)
                .with_scale(Vec3::new(length, thick, thick));
            let colour =
                tree.get_color(id, Property::Color).unwrap_or(libroxel_instance::Color3::MEDIUM_GREY);
            wanted.insert(id, (transform, colour));
        }
    }
    let material = |c: libroxel_instance::Color3| StandardMaterial {
        base_color: Color::srgb(c.r, c.g, c.b),
        perceptual_roughness: 0.9,
        ..default()
    };
    for (entity, mut link, mut transform, handle) in &mut drawn {
        match wanted.remove(&link.0) {
            Some((t, colour)) => {
                transform.set_if_neq(t);
                if link.1 != colour {
                    link.1 = colour;
                    if let Some(mut m) = materials.get_mut(&handle.0) {
                        m.base_color = Color::srgb(colour.r, colour.g, colour.b);
                    }
                }
            }
            None => commands.entity(entity).despawn(),
        }
    }
    for (id, (transform, colour)) in wanted {
        commands.spawn((
            RopeLink(id, colour),
            Mesh3d(meshes.cylinder.clone()),
            MeshMaterial3d(materials.add(material(colour))),
            transform,
        ));
    }
}

/// Where a decal on `face` of a part goes: a unit square facing +Z, turned
/// and stretched over that side and lifted just off it. Also returns the
/// side's width and height in studs.
pub fn decal_transform(
    pos: libroxel_instance::Vec3,
    size: libroxel_instance::Vec3,
    orientation: libroxel_instance::Vec3,
    face: NormalId,
) -> (Transform, Vec2) {
    // The side's outward direction, and which way is up on it.
    let (normal, up) = match face {
        NormalId::Right => (Vec3::X, Vec3::Y),
        NormalId::Left => (Vec3::NEG_X, Vec3::Y),
        NormalId::Back => (Vec3::Z, Vec3::Y),
        NormalId::Front => (Vec3::NEG_Z, Vec3::Y),
        NormalId::Top => (Vec3::Y, Vec3::NEG_Z),
        NormalId::Bottom => (Vec3::NEG_Y, Vec3::Z),
    };
    let right = up.cross(normal);
    let size = to_bevy(size);
    let extent = |axis: Vec3| (size * axis).abs().element_sum();
    let (width, height) = (extent(right), extent(up));
    let part = crate::physics::orientation_quat(orientation);
    let lift = extent(normal) / 2.0 + 0.01;
    let transform = Transform::from_translation(to_bevy(pos) + part * (normal * lift))
        .with_rotation(part * Quat::from_mat3(&Mat3::from_cols(right, up, normal)))
        .with_scale(Vec3::new(width, height, 1.0));
    (transform, Vec2::new(width, height))
}

/// Links a drawn decal or texture to its instance, with what its material shows.
#[derive(Component)]
struct DecalLink(InstanceId, DecalLook);

#[derive(Debug, Clone, PartialEq)]
struct DecalLook {
    picture: String,
    tint: libroxel_instance::Color3,
    transparency: f32,
    /// How many times the picture repeats across and down, for textures.
    tiles: Option<Vec2>,
}

/// Loaded pictures for decals: tiling ones repeat at the edges.
#[derive(Default)]
struct DecalPictures {
    generation: u64,
    loaded: HashMap<(String, bool), Option<Handle<Image>>>,
}

/// Draws every decal and texture on the side of its part.
fn sync_decals(
    engine: NonSend<EngineState>,
    mut commands: Commands,
    meshes: Option<Res<PartMeshes>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut pictures: Local<DecalPictures>,
    mut drawn: Query<(Entity, &mut DecalLink, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
) {
    use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
    let Some(meshes) = meshes else { return };
    let tree = engine.tree.borrow();
    let mut wanted: HashMap<InstanceId, (Transform, DecalLook)> = HashMap::new();
    for class in [ClassKind::Decal, ClassKind::Texture] {
        for id in tree.descendants_of_class(tree.workspace(), class) {
            let Some(part) =
                tree.parent(id).ok().flatten().filter(|p| tree.class(*p).is_ok_and(|c| c.is_physical()))
            else {
                continue;
            };
            let (Some(pos), Some(size)) =
                (tree.get_vec3(part, Property::Position), tree.get_vec3(part, Property::Size))
            else {
                continue;
            };
            let face = match tree.get_enum(id, Property::Face) {
                Some(EnumValue::NormalId(face)) => face,
                _ => NormalId::Front,
            };
            let orientation = tree.get_vec3(part, Property::Orientation).unwrap_or_default();
            let (transform, extent) = decal_transform(pos, size, orientation, face);
            let tiles = (class == ClassKind::Texture).then(|| {
                let per = |p| (tree.get_number(id, p).unwrap_or(2.0) as f32).max(0.01);
                extent / Vec2::new(per(Property::StudsPerTileU), per(Property::StudsPerTileV))
            });
            let look = DecalLook {
                picture: tree.get_string(id, Property::Texture).unwrap_or_default().trim().to_owned(),
                tint: tree.get_color(id, Property::DecalColor).unwrap_or(libroxel_instance::Color3::WHITE),
                transparency: tree.get_number(id, Property::Transparency).unwrap_or(0.0) as f32,
                tiles,
            };
            if look.picture.is_empty() || look.transparency >= 0.999 {
                continue;
            }
            wanted.insert(id, (transform, look));
        }
    }
    drop(tree);

    if pictures.generation != crate::pictures::generation() {
        pictures.generation = crate::pictures::generation();
        pictures.loaded.clear();
    }
    let mut material_for = |look: &DecalLook| -> Option<StandardMaterial> {
        let key = (look.picture.clone(), look.tiles.is_some());
        let picture = pictures
            .loaded
            .entry(key)
            .or_insert_with(|| {
                let sampler = if look.tiles.is_some() {
                    ImageSampler::Descriptor(ImageSamplerDescriptor {
                        address_mode_u: ImageAddressMode::Repeat,
                        address_mode_v: ImageAddressMode::Repeat,
                        ..ImageSamplerDescriptor::linear()
                    })
                } else {
                    ImageSampler::linear()
                };
                let image = crate::pictures::decode(&crate::pictures::bytes_for(&look.picture)?, sampler)?;
                Some(images.add(image))
            })
            .clone()?;
        let t = look.tint;
        Some(StandardMaterial {
            base_color: Color::srgba(t.r, t.g, t.b, (1.0 - look.transparency).clamp(0.0, 1.0)),
            base_color_texture: Some(picture),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.8,
            uv_transform: bevy::math::Affine2::from_scale(look.tiles.unwrap_or(Vec2::ONE)),
            // Drawn over the part's own side rather than fighting it.
            depth_bias: 1.0,
            ..default()
        })
    };
    for (entity, mut link, mut transform, handle) in &mut drawn {
        match wanted.remove(&link.0) {
            Some((t, look)) => {
                transform.set_if_neq(t);
                if link.1 != look {
                    match material_for(&look) {
                        Some(material) => {
                            if let Some(mut m) = materials.get_mut(&handle.0) {
                                *m = material;
                            }
                            link.1 = look;
                        }
                        None => commands.entity(entity).despawn(),
                    }
                }
            }
            None => commands.entity(entity).despawn(),
        }
    }
    for (id, (transform, look)) in wanted {
        let Some(material) = material_for(&look) else { continue };
        commands.spawn((
            DecalLink(id, look),
            Mesh3d(meshes.quad.clone()),
            MeshMaterial3d(materials.add(material)),
            transform,
            bevy::light::NotShadowCaster,
        ));
    }
}

fn draw_blasts(
    engine: NonSend<EngineState>,
    mut commands: Commands,
    meshes: Option<Res<PartMeshes>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    visuals: Query<Entity, With<BlastVisual>>,
) {
    for entity in &visuals {
        commands.entity(entity).despawn();
    }
    let (Some(session), Some(meshes)) = (engine.session.as_ref(), meshes) else { return };
    for blast in session.blasts() {
        let t = (blast.age / crate::session::BLAST_TIME).clamp(0.0, 1.0);
        let size = blast.radius.max(0.5) * 2.0 * (0.3 + 0.7 * t.sqrt());
        let alpha = (1.0 - t) * 0.85;
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.55, 0.1, alpha),
            emissive: LinearRgba::rgb(6.0 * (1.0 - t), 2.5 * (1.0 - t), 0.3),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        });
        commands.spawn((
            BlastVisual,
            Mesh3d(meshes.ball.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(to_bevy(blast.position)).with_scale(Vec3::splat(size)),
            bevy::light::NotShadowCaster,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libroxel_instance::Vec3 as V;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn decals_cover_their_side_and_face_out() {
        let (pos, size) = (V::new(0.0, 5.0, 0.0), V::new(4.0, 2.0, 6.0));
        let (t, extent) = decal_transform(pos, size, V::ZERO, NormalId::Front);
        assert_eq!(extent, Vec2::new(4.0, 2.0));
        assert!(close(t.translation, Vec3::new(0.0, 5.0, -3.01)));
        // The square's +Z faces out of the front, and its top stays up.
        assert!(close(t.rotation * Vec3::Z, Vec3::NEG_Z));
        assert!(close(t.rotation * Vec3::Y, Vec3::Y));
        let (t, extent) = decal_transform(pos, size, V::ZERO, NormalId::Top);
        assert_eq!(extent, Vec2::new(4.0, 6.0));
        assert!(close(t.translation, Vec3::new(0.0, 6.01, 0.0)));
        assert!(close(t.rotation * Vec3::Z, Vec3::Y));
        // A part turned a quarter about Y takes its decal round with it.
        let (t, extent) = decal_transform(pos, size, V::new(0.0, 90.0, 0.0), NormalId::Right);
        assert_eq!(extent, Vec2::new(6.0, 2.0));
        assert!(close(t.rotation * Vec3::Z, Vec3::NEG_Z), "{:?}", t.rotation * Vec3::Z);
        assert!(close(t.translation, Vec3::new(0.0, 5.0, -2.01)));
    }
}
