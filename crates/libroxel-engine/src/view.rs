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
    pub confine: bool,
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
/// `StreamingTargetRadius` (the reference platform's default is 1024 studs), or everything
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
    /// Those parts, farthest first, so each frame takes the nearest off
    /// the end without looking over the workspace again.
    backlog: Vec<InstanceId>,
    /// Parts out of range when last measured.
    far: FastSet<InstanceId>,
    frames: u64,
    /// Frames the view was hidden (see [`view_hidden`]).
    hidden_frames: u64,
    /// Where it last read the tree's log of what moved, came or went, and
    /// that log's epoch.
    moved_read: usize,
    moved_epoch: u64,
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
            .init_resource::<StepTime>()
            .init_resource::<PlayInput>()
            .init_resource::<InputCapture>()
            .configure_sets(Update, (EngineSet::Input, EngineSet::Simulate, EngineSet::Sync).chain())
            .add_systems(Update, step_session.in_set(EngineSet::Simulate))
            .add_systems(Update, sync_parts.in_set(EngineSet::Sync));
    }
}

/// How long the last frame's game step (scripts and physics) took, in seconds.
#[derive(Resource, Default)]
pub struct StepTime(pub f32);

fn step_session(
    mut engine: NonSendMut<EngineState>,
    time: Res<Time>,
    input: Res<PlayInput>,
    mut took: ResMut<StepTime>,
) {
    let engine = &mut *engine;
    let Some(session) = engine.session.as_mut() else {
        took.0 = 0.0;
        return;
    };
    // Clamp long frames (a window drag, a breakpoint) so physics stays sane.
    let dt = time.delta_secs().min(0.1);
    let began = std::time::Instant::now();
    let stepped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.step(dt, input.0)));
    took.0 = began.elapsed().as_secs_f32();
    if let Err(why) = stepped {
        let why = why
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| why.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let mut log = session.take_log();
        log.push(crate::session::LogEntry {
            level: crate::session::LogLevel::Error,
            time: 0.0,
            text: format!("The game stopped after an error: {why}"),
        });
        engine.log.extend(log);
        engine.session = None;
        return;
    }
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
    // Nothing of the 3D view shows behind a loading screen: keep up only now
    // and then (so the world is there when it goes), leaving the frames to
    // the loading itself.
    if engine.is_playing() && view_hidden() {
        sync.hidden_frames += 1;
        if !sync.hidden_frames.is_multiple_of(10) {
            return;
        }
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
    let small = quality.as_ref().map_or(1.0, |q| q.small_part_reach());
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
    let periodic = limited && (moved_far || sync.frames.is_multiple_of(30));
    let scan = full || sync.structure != tree.workspace_structure() || sync.pending || periodic;
    // When only parts came or went (a game building its world adds some
    // nearly every frame), just those are looked at, from the tree's log of
    // what moved, instead of every part in the workspace.
    let (moved_epoch, came_or_went) = tree.moved_since(sync.moved_read);
    let incremental =
        scan && !full && !periodic && moved_epoch == sync.moved_epoch && came_or_went.len() < 20_000;

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
    // Like the reference platform's streaming: while playing, only parts within the
    // workspace's streaming radius of the camera are drawn, and new parts
    // come in nearest first, a batch a frame, so a big map opens at once
    // instead of building every part before the first frame.
    // Like the reference platform at lower graphics levels, small parts stop being drawn
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
            radius.min((size.x.max(size.y).max(size.z) * DETAIL_PER_STUD * small).max(DETAIL_MIN * small))
        } else {
            radius
        };
        d / limit
    };

    if incremental {
        sync.structure = tree.workspace_structure();
        let PartEntities(spawned, seen) = &mut *entities;
        let mut once = FastSet::default();
        let mut fresh: Vec<(f32, InstanceId)> = Vec::new();
        for id in came_or_went {
            if !once.insert(*id) {
                continue;
            }
            let mut ids = tree.descendants(*id).unwrap_or_default();
            ids.push(*id);
            for id in ids {
                let live = tree.class(id).is_ok_and(|c| c.is_a(ClassKind::Part))
                    && !tree.is_destroyed(id).unwrap_or(true)
                    && tree.is_descendant_of(id, tree.workspace()).unwrap_or(false);
                match spawned.get(&id) {
                    Some(entity) if !live => {
                        commands.entity(*entity).despawn();
                        spawned.remove(&id);
                        seen.remove(&id);
                    }
                    None if live => {
                        sync.far.remove(&id);
                        let d = reach(id);
                        if d <= 1.0 {
                            fresh.push((d, id));
                        } else if limited {
                            sync.far.insert(id);
                        }
                    }
                    _ => {}
                }
            }
        }
        // Parts that just came in range all spawn, however many (the view
        // keeps up with the world as it arrives); what's left of the budget
        // goes to the nearest of the parts still waiting.
        let room = SPAWN_BUDGET.saturating_sub(fresh.len());
        let arrived: FastSet<InstanceId> = fresh.iter().map(|(_, id)| *id).collect();
        work.extend(fresh.into_iter().map(|(_, id)| id));
        let mut taken = 0;
        while taken < room
            && let Some(id) = sync.backlog.pop()
        {
            let live = !spawned.contains_key(&id)
                && !arrived.contains(&id)
                && !tree.is_destroyed(id).unwrap_or(true)
                && tree.is_descendant_of(id, tree.workspace()).unwrap_or(false);
            if live {
                work.push(id);
                taken += 1;
            }
        }
        sync.pending = !sync.backlog.is_empty();
    } else if scan {
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
        sync.backlog.clear();
        if sync.pending {
            // Farthest first: the nearest come off the end, a budget a frame.
            fresh.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
            let now = fresh.split_off(fresh.len() - SPAWN_BUDGET);
            sync.backlog = fresh.into_iter().map(|(_, id)| id).collect();
            fresh = now;
        }
        work.extend(fresh.into_iter().map(|(_, id)| id));
    } else if full {
        let PartEntities(spawned, seen) = &*entities;
        work.extend(
            spawned.keys().copied().filter(|id| seen.get(id) != Some(&tree.revision(*id).unwrap_or(0))),
        );
    }
    sync.moved_epoch = tree.moved_epoch();
    sync.moved_read = tree.moved_len();
    VIEW_FILLING.store(sync.pending, std::sync::atomic::Ordering::Relaxed);
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
        // the part further, as on the reference platform.
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
        let name = tree.name(id).unwrap_or_default();
        let geometry = PartGeometry(shape);

        match spawned.get(&id).and_then(|e| parts.get_mut(*e).ok()) {
            Some((mut t, mut a, mut n, mut g, mut m, mut mt)) => {
                m.set_if_neq(mesh_file);
                mt.set_if_neq(mesh_texture);
                t.set_if_neq(transform);
                a.set_if_neq(appearance);
                // (Compared first: a part that moves every frame keeps its name.)
                if n.0 != name {
                    n.0 = name.to_owned();
                }
                g.set_if_neq(geometry);
                seen.insert(id, revision);
            }
            None if !spawned.contains_key(&id) => {
                let entity = commands
                    .spawn((
                        PartLink(id),
                        PartName(name.to_owned()),
                        transform,
                        appearance,
                        geometry,
                        mesh_file,
                        mesh_texture,
                    ))
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
            .init_resource::<crate::devices::ScreenTouches>()
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
                    model_detail,
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
                (follow_camera, refresh_camera_global, hide_self_in_first_person, lock_cursor)
                    .chain()
                    .after(EngineSet::Sync),
            )
            .add_systems(Update, (sync_lights, draw_blasts, sync_ropes, sync_decals).after(EngineSet::Sync))
            .add_systems(Update, cover_view)
            .add_systems(Update, crate::terrain_view::sync_terrain.after(EngineSet::Sync))
            .add_systems(Update, update_lighting.after(EngineSet::Sync))
            .init_resource::<GraphicsQuality>()
            .init_resource::<SelfFade>()
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

/// Packs a mesh that is only drawn, never read back, into fewer bytes for
/// the graphics chip (28 bytes a corner instead of 48 with colours, 20
/// instead of 32 without): normals as two 16-bit numbers, texture
/// positions and colours as 16-bit fractions, and 16-bit indices when
/// there are few enough corners. Texture positions stay full size when
/// they span so far that a step would show (a wobbling texture). Places
/// stay full size: packed ones weren't drawn at all.
pub(crate) fn pack_mesh(mesh: &mut Mesh) {
    use bevy::mesh::{AttributeQuantization, VertexAttributeValues};
    let uv_span = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(v)) => {
            let (lo, hi) = v
                .iter()
                .fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| (lo.min(Vec2::from(*p)), hi.max(Vec2::from(*p))));
            (hi - lo).max_element()
        }
        _ => f32::INFINITY,
    };
    let _ = mesh.compress_normals();
    if uv_span <= 64.0 {
        let _ = mesh.compress_uv0();
    }
    if mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some() {
        let _ = mesh.quantize_colors(AttributeQuantization::Unorm16);
    }
    mesh.compress_indices();
}

/// Whether the game's GUI covers the whole 3D view with something solid
/// (a loading screen): then the view isn't drawn at all, and the frames it
/// saves go to the game.
pub static VIEW_COVERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub static VIEW_FILLING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set by the app while its own loading screen covers the whole view: the
/// world isn't drawn behind it (parts still come into the view, so it is
/// ready when the screen goes), and a joined game takes in the host's world
/// in bigger pieces a frame.
pub static LOADING_SCREEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the 3D view isn't being drawn: a loading screen covers it, or
/// the game turned 3D rendering off.
pub fn view_hidden() -> bool {
    VIEW_COVERED.load(std::sync::atomic::Ordering::Relaxed)
        || !libroxel_script::RENDERING_3D.load(std::sync::atomic::Ordering::Relaxed)
}

fn cover_view(mut cameras: Query<&mut Camera, With<CameraRig>>) {
    let covered = VIEW_COVERED.load(std::sync::atomic::Ordering::Relaxed)
        || !libroxel_script::RENDERING_3D.load(std::sync::atomic::Ordering::Relaxed)
        || LOADING_SCREEN.load(std::sync::atomic::Ordering::Relaxed);
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
        // Three shadow cascades instead of four: each one draws every part
        // again, which a big map can't afford.
        bevy::light::CascadeShadowConfigBuilder {
            num_cascades: 3,
            first_cascade_far_bound: 50.0,
            maximum_distance: 800.0,
            ..default()
        }
        .build(),
        Transform::from_xyz(8.0, 20.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(ClearColor(Color::srgb(0.42, 0.66, 0.96)));
}

/// The world's main light, moved by the time of day.
#[derive(Component)]
struct Sun;

/// How much the 3D view draws, like the reference platform's graphics quality: 1 is the
/// least and 10 everything. On automatic it steps down when frames come
/// slower than the screen shows them (up to 60 a second) and back up once
/// there is room again.
#[derive(Resource, Debug)]
pub struct GraphicsQuality {
    pub auto: bool,
    /// 1 to 10.
    pub level: u8,
    ceiling: u8,
    /// Frames a second to keep up.
    target: f32,
    average: f32,
    slow_for: f32,
    fast_for: f32,
    since_drop: f32,
    /// Automatic turned edge smoothing off; it stays off until a manual pick.
    smoothing_dropped: bool,
    applied: Option<u8>,
    /// A phone or tablet: see [`GraphicsQuality::phone`].
    phone: bool,
}

impl Default for GraphicsQuality {
    fn default() -> Self {
        Self::new(true, 6)
    }
}

impl GraphicsQuality {
    pub const MAX: u8 = 10;
    const VIEW_DISTANCE: [f32; 10] = [150.0, 200.0, 275.0, 350.0, 450.0, 600.0, 750.0, 900.0, 1000.0, 1500.0];
    const LIGHTS: [usize; 10] = [0, 0, 4, 8, 16, 32, usize::MAX, usize::MAX, usize::MAX, usize::MAX];
    const PARTICLES: [f32; 10] = [0.2, 0.3, 0.4, 0.5, 0.6, 0.75, 0.9, 1.0, 1.0, 1.0];

    pub fn new(auto: bool, level: u8) -> Self {
        let level = level.clamp(1, Self::MAX);
        Self {
            auto,
            level,
            ceiling: Self::MAX,
            target: 60.0,
            average: 1.0 / 60.0,
            slow_for: 0.0,
            fast_for: 0.0,
            since_drop: 0.0,
            smoothing_dropped: false,
            applied: None,
            phone: false,
        }
    }

    /// For a phone or tablet, whose graphics chip does far less than a
    /// computer's: automatic starts low (no shadows) and climbs no higher
    /// than the middle, and bloom and edge smoothing, which cost phones the
    /// most, stay off unless a level of 8 or more is picked.
    pub fn phone(mut self) -> Self {
        self.phone = true;
        if self.auto {
            self.level = self.level.min(Self::PHONE_START);
            self.ceiling = Self::PHONE_TOP;
        }
        self
    }

    const PHONE_START: u8 = 3;
    const PHONE_TOP: u8 = 6;

    /// The highest level automatic climbs back to.
    fn top(&self) -> u8 {
        if self.phone { Self::PHONE_TOP } else { Self::MAX }
    }

    /// Switches to automatic, or to a fixed level.
    pub fn set(&mut self, auto: bool, level: u8) {
        let target = self.target;
        let average = self.average;
        let phone = self.phone;
        *self = Self::new(auto, level);
        self.phone = phone;
        if phone && auto {
            self.ceiling = Self::PHONE_TOP.max(self.level);
        }
        self.target = target;
        self.average = average;
    }

    /// The frame rate automatic aims for: the screen's, at most 60.
    pub fn set_target(&mut self, fps: f32) {
        self.target = fps.clamp(24.0, 60.0);
    }

    /// Counts one frame of `dt` seconds; returns whether the level changed.
    pub fn frame(&mut self, dt: f32) -> bool {
        if !self.auto {
            return false;
        }
        let dt = dt.clamp(0.0, 0.5);
        let goal = 1.0 / self.target;
        self.average += (dt - self.average) * 0.1;
        self.since_drop += dt;
        self.slow_for = if self.average > goal * 1.1 { self.slow_for + dt } else { 0.0 };
        self.fast_for = if self.average < goal * 1.04 { self.fast_for + dt } else { 0.0 };
        if self.since_drop > 60.0 {
            self.ceiling = self.top();
        }
        if self.slow_for > 1.5 && self.level > 1 {
            let step = if self.average > goal * 1.6 { 2 } else { 1 };
            self.level = self.level.saturating_sub(step).max(1);
            self.ceiling = self.level;
            self.slow_for = 0.0;
            self.fast_for = 0.0;
            self.since_drop = 0.0;
            return true;
        }
        if self.fast_for > 4.0 && self.level < self.ceiling {
            self.level += 1;
            self.fast_for = 0.0;
            return true;
        }
        false
    }

    fn index(&self) -> usize {
        usize::from(self.level.clamp(1, Self::MAX) - 1)
    }

    /// Whether the sun casts shadows.
    pub fn shadows(&self) -> bool {
        self.level >= 5
    }

    /// Shadow cascades, and the size of each one's map.
    pub fn shadow_detail(&self) -> (usize, usize) {
        if self.phone {
            return if self.level >= 8 { (2, 2048) } else { (1, 1024) };
        }
        match self.level {
            0..=5 => (1, 1024),
            6 => (2, 1024),
            7 => (2, 2048),
            8 | 9 => (3, 2048),
            _ => (3, 4096),
        }
    }

    /// Whether this is a phone or tablet (see [`GraphicsQuality::phone`]).
    pub fn is_phone(&self) -> bool {
        self.phone
    }

    /// How shadow edges are softened. A phone or tablet takes one blended
    /// look at the shadow map for each pixel instead of nine below level
    /// 8: shadow edges are a little harder, and the 3D view much cheaper.
    pub fn shadow_filter(&self) -> bevy::light::ShadowFilteringMethod {
        if self.phone && self.level < 8 {
            bevy::light::ShadowFilteringMethod::Hardware2x2
        } else {
            bevy::light::ShadowFilteringMethod::Gaussian
        }
    }

    /// How far the sun's shadows reach, in studs. A phone or tablet draws
    /// every part again into its one shadow map, so it keeps that to the
    /// parts near the player, where shadows show most.
    pub fn shadow_distance(&self) -> f32 {
        let far = self.view_distance().min(800.0);
        if self.phone && self.level < 8 { far.min(250.0) } else { far }
    }

    /// How soon small parts stop being drawn while playing, as a share of
    /// the usual distance: a phone or tablet at the lower levels leaves
    /// them out sooner, where they are only a few pixels anyway.
    pub fn small_part_reach(&self) -> f32 {
        match (self.phone, self.level) {
            (true, 0..=3) => 0.5,
            (true, 4..=6) => 0.7,
            _ => 1.0,
        }
    }

    /// How many times its size away a detailed model is drawn with its
    /// rough copy (see [`crate::mesh_files::Model::simplified`]); sooner on
    /// a phone or tablet below level 8.
    pub fn far_model(&self) -> f32 {
        if self.phone && self.level < 8 { FAR_MODEL * 0.6 } else { FAR_MODEL }
    }

    /// Whether edges are smoothed (MSAA).
    pub fn smoothing(&self) -> bool {
        self.level >= if self.phone { 8 } else { 4 } && !self.smoothing_dropped
    }

    pub fn bloom(&self) -> bool {
        self.level >= if self.phone { 8 } else { 6 }
    }

    /// How many point and spot lights are drawn (the nearest ones).
    pub fn lights(&self) -> usize {
        Self::LIGHTS[self.index()]
    }

    /// How many of the particles emitters ask for are sent out.
    pub fn particles(&self) -> f32 {
        Self::PARTICLES[self.index()]
    }

    /// How far away parts are still drawn, in studs.
    pub fn view_distance(&self) -> f32 {
        Self::VIEW_DISTANCE[self.index()]
    }

    /// On a phone or tablet, how many pixels the 3D view draws for each
    /// point of the screen (menus and text stay at the screen's own). A
    /// phone's screen packs in three or more; the 3D view looks much the
    /// same at far fewer, and each one it skips is time saved. `None` on a
    /// computer: the 3D view draws every pixel.
    pub fn render_density(&self) -> Option<f32> {
        self.phone.then_some(match self.level {
            1 | 2 => 1.0,
            3 | 4 => 1.25,
            5 | 6 => 1.5,
            7 | 8 => 2.0,
            _ => 3.0,
        })
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn auto_quality(
    time: Res<Time>,
    step: Res<StepTime>,
    monitors: Query<&bevy::window::Monitor, With<bevy::window::PrimaryMonitor>>,
    mut quality: ResMut<GraphicsQuality>,
    mut shadow_map: ResMut<bevy::light::DirectionalLightShadowMap>,
    mut cameras: Query<(Entity, &mut Projection, &mut Msaa), With<CameraRig>>,
    mut sun: Query<(&mut DirectionalLight, &mut bevy::light::CascadeShadowConfig), With<Sun>>,
    mut commands: Commands,
) {
    if let Some(hz) = monitors.iter().next().and_then(|m| m.refresh_rate_millihertz) {
        quality.set_target(hz as f32 / 1000.0);
    }
    quality.frame((time.delta_secs() - step.0).max(0.0));
    if quality.applied == Some(quality.level) {
        return;
    }
    if quality.auto && quality.level < 4 {
        quality.smoothing_dropped = true;
    }
    quality.applied = Some(quality.level);
    let msaa = if quality.smoothing() { Msaa::Sample4 } else { Msaa::Off };
    for (entity, mut projection, mut samples) in &mut cameras {
        if let Projection::Perspective(p) = &mut *projection {
            p.far = quality.view_distance();
        }
        samples.set_if_neq(msaa);
        commands.entity(entity).insert(quality.shadow_filter());
    }
    let (cascades, size) = quality.shadow_detail();
    if shadow_map.size != size {
        shadow_map.size = size;
    }
    for (mut light, mut config) in &mut sun {
        light.shadow_maps_enabled = quality.shadows();
        *config = bevy::light::CascadeShadowConfigBuilder {
            num_cascades: cascades,
            first_cascade_far_bound: 50.0,
            maximum_distance: quality.shadow_distance(),
            ..default()
        }
        .build();
    }
}

/// How the sky and sun look at a time of day (hours, 0 to 24) and brightness.
pub fn sky_at(clock: f32, brightness: f32) -> (Vec3, f32, Color, Color) {
    use std::f32::consts::PI;
    // Sunrise at 6, noon overhead, sunset at 18.
    let angle = (clock - 6.0) / 12.0 * PI;
    let height = angle.sin();
    let dir = Vec3::new(angle.cos(), height.max(0.05), 0.35).normalize();
    let day = (height * 3.0 + 0.4).clamp(0.0, 1.0);
    let dusk = (1.0 - (height.abs() * 4.0)).clamp(0.0, 1.0);
    let night = Vec3::new(0.02, 0.03, 0.09);
    let noon = Vec3::new(0.42, 0.66, 0.96);
    let glow = Vec3::new(0.95, 0.55, 0.35);
    let sky = night.lerp(noon, day).lerp(glow, dusk * 0.5 * day.max(0.3));
    let scale = (brightness / 2.0).clamp(0.0, 3.0);
    let illuminance = (300.0 + 9_700.0 * day) * scale;
    let moon = Vec3::new(0.6, 0.7, 1.0);
    let white = Vec3::new(1.0, 0.95, 0.86);
    let sunset = Vec3::new(1.0, 0.55, 0.3);
    let sun = moon.lerp(white, day).lerp(sunset, dusk * day);
    (dir, illuminance, Color::srgb(sky.x, sky.y, sky.z), Color::srgb(sun.x, sun.y, sun.z))
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
            // Outdoors, the reference platform lights with OutdoorAmbient (Ambient is for indoors);
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
    let (dir, illuminance, sky, sun_colour) = sky_at(clock, brightness);
    // Exposure brightens (or darkens) everything, a stop at a time.
    let exposure = 2f32.powf(exposure);
    let illuminance = illuminance * exposure;
    // The ambient colour lights every face the same at any time of day, as in
    // the reference platform: a game that keeps it bright at night has bright nights. 0.5 grey
    // is the normal amount; coloured ambient tints the shadows.
    let strongest = tint.r.max(tint.g).max(tint.b);
    let ambient_brightness = 400.0 * strongest * 2.0 * exposure;
    let sky_tint = sky.to_srgba();
    let sky_top = sky_tint.red.max(sky_tint.green).max(sky_tint.blue).max(0.01);
    let ambient_color = if strongest > 0.0 {
        let mix = |c: f32, s: f32| c / strongest * (0.65 + 0.35 * s / sky_top);
        Color::srgb(mix(tint.r, sky_tint.red), mix(tint.g, sky_tint.green), mix(tint.b, sky_tint.blue))
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
        if light.color != sun_colour {
            light.color = sun_colour;
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
    mut parts: Query<(Ref<PartMeshFile>, Ref<PartGeometry>, &mut Mesh3d)>,
    mut generation: Local<u64>,
) {
    // A model is read once (or found missing) and stays so until the
    // game's files change: only parts whose file or shape changed since
    // then need looking at.
    let all = std::mem::replace(&mut *generation, crate::pictures::generation()) != *generation;
    for (file, geometry, mut mesh) in &mut parts {
        if !all && !file.is_changed() && !geometry.is_changed() {
            continue;
        }
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

/// How far away, in times its biggest side, a detailed model switches to
/// its rough copy (and, a little nearer, back): its smallest details are
/// then about a pixel or two across.
const FAR_MODEL: f32 = 40.0;

/// Draws detailed models far from the camera with their rough copy (see
/// [`crate::mesh_files::Model::simplified`]), a few times a second.
fn model_detail(
    files: Res<crate::mesh_files::MeshFiles>,
    quality: Option<Res<GraphicsQuality>>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<PartLink>)>,
    mut parts: Query<(&PartMeshFile, &Transform, &mut Mesh3d), With<PartLink>>,
    mut frame: Local<u32>,
) {
    *frame = frame.wrapping_add(1);
    if !frame.is_multiple_of(10) || files.is_empty() {
        return;
    }
    let Some(eye) = cameras.iter().next().map(GlobalTransform::translation) else {
        return;
    };
    let switch = quality.as_ref().map_or(FAR_MODEL, |q| q.far_model());
    for (file, transform, mut mesh) in &mut parts {
        if file.0.is_empty() {
            continue;
        }
        let Some((near, far)) = files.both(mesh.0.id()) else {
            continue;
        };
        let size = transform.scale.max_element().max(0.01);
        let away = transform.translation.distance(eye) / size;
        let showing_far = mesh.0 == far;
        let wanted = if away > switch || (showing_far && away > switch * 0.9) { far } else { near };
        if mesh.0 != wanted {
            mesh.0 = wanted;
        }
    }
}

/// Pictures for parts' models, by name (None when one couldn't be read),
/// and one material for every part that shows the same picture the same
/// way, so they draw together.
#[derive(Resource, Default)]
struct MeshTextures {
    loaded: std::collections::HashMap<String, Option<Handle<Image>>>,
    /// Pictures being read and unpacked away from the frame, so a map full
    /// of them doesn't stall while they come in.
    loading: std::collections::HashMap<String, bevy::tasks::Task<Option<Image>>>,
    /// One material for each look and picture, shared by every part with
    /// both, so they draw together.
    materials: std::collections::HashMap<(LookKey, String), Handle<StandardMaterial>>,
    /// Parts showing their colour until their picture arrives.
    waiting: std::collections::HashSet<Entity>,
    generation: u64,
}

/// A part that may wrap a picture over its model.
type TexturedPart = (
    Entity,
    Ref<'static, PartMeshTexture>,
    Ref<'static, PartAppearance>,
    &'static mut MeshMaterial3d<StandardMaterial>,
);

/// Wraps a part's picture over its model, in place of its colour.
fn use_mesh_textures(
    textures: Res<MaterialTextures>,
    mut cache: ResMut<MeshTextures>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shared: ResMut<SharedMaterials>,
    mut parts: Query<TexturedPart>,
) {
    let generation = crate::pictures::generation();
    // Other folders: the same names may be other pictures now, so waiting
    // parts look again.
    let mut arrived = cache.generation != generation;
    if arrived {
        cache.loaded.clear();
        cache.loading.clear();
        cache.materials.clear();
        cache.generation = generation;
    }
    let cache = &mut *cache;
    cache.loading.retain(|name, task| {
        if !task.is_finished() {
            return true;
        }
        let image = bevy::tasks::block_on(bevy::tasks::poll_once(task)).flatten();
        cache.loaded.insert(name.clone(), image.map(|i| images.add(i)));
        arrived = true;
        false
    });
    let any_changed = parts.iter().any(|(_, t, a, _)| t.is_changed() || a.is_changed());
    if !arrived && !any_changed {
        return;
    }
    if arrived {
        // Forget parts that are gone.
        cache.waiting.retain(|e| parts.contains(*e));
    }
    for (entity, texture, appearance, mut handle) in &mut parts {
        let due =
            texture.is_changed() || appearance.is_changed() || (arrived && cache.waiting.contains(&entity));
        if !due {
            continue;
        }
        if texture.0.is_empty() {
            cache.waiting.remove(&entity);
            if texture.is_changed() {
                handle.0 = shared.get(&appearance, &textures, &mut materials);
            }
            continue;
        }
        let picture = match cache.loaded.get(&texture.0) {
            Some(picture) => picture.clone(),
            None => {
                if !cache.loading.contains_key(&texture.0) {
                    let name = texture.0.clone();
                    let pool = bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
                    let task = pool.spawn(async move {
                        let bytes = crate::pictures::bytes_for(&name)?;
                        crate::pictures::decode_for_view(&bytes, bevy::image::ImageSampler::linear())
                    });
                    cache.loading.insert(texture.0.clone(), task);
                }
                // Its colour until the picture is ready.
                cache.waiting.insert(entity);
                handle.0 = shared.get(&appearance, &textures, &mut materials);
                continue;
            }
        };
        cache.waiting.remove(&entity);
        let Some(picture) = picture else {
            handle.0 = shared.get(&appearance, &textures, &mut materials);
            continue;
        };
        let key = (LookKey::of(&appearance), texture.0.clone());
        let material = cache
            .materials
            .entry(key)
            .or_insert_with(|| {
                let mut material = material_for(&appearance, &textures);
                let alpha = material.base_color.alpha();
                material.base_color = Color::srgba(1.0, 1.0, 1.0, alpha);
                material.base_color_texture = Some(picture);
                materials.add(material)
            })
            .clone();
        if handle.0 != material {
            handle.0 = material;
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
    let held = mouse.pressed(MouseButton::Right) && !capture.pointer;
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
        // In first person the mouse looks round without a button held.
        let looking = held || (first_person(&rig) && !capture.keyboard);
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
        rig.yaw = (rig.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
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
#[allow(clippy::too_many_arguments)]
fn forward_keys(
    mut engine: NonSendMut<EngineState>,
    capture: Res<InputCapture>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    touch: Res<crate::devices::VirtualInput>,
    mut screen: ResMut<crate::devices::ScreenTouches>,
    time: Res<Time>,
    mut last: Local<libroxel_instance::UserInputType>,
    mut vibrating: Local<bool>,
) {
    use libroxel_instance::UserInputType;
    let engine = &mut *engine;
    let Some(session) = engine.session.as_mut() else {
        screen.touches.clear();
        screen.presses.clear();
        screen.buttons.clear();
        screen.controls_hidden = false;
        // A game that stops leaves the motor off.
        if std::mem::take(&mut *vibrating) {
            screen.vibration = Some(0.0);
        }
        return;
    };
    for (name, pressed) in std::mem::take(&mut screen.presses) {
        session.press_touch_button(&name, pressed);
    }
    screen.buttons = session.touch_buttons();
    if let Some(strength) = session.take_vibration() {
        *vibrating = strength > 0.0;
        screen.vibration = Some(strength);
    }
    let now = time.elapsed_secs_f64();
    let touches = std::mem::take(&mut screen.touches);
    let touched_now = !touches.is_empty();
    for one in touches {
        session.touch(one, now);
    }
    session.touch_tick(now);
    screen.controls_hidden = !session.touch_controls_shown();
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
    } else if touched_now || touch.stick != Vec2::ZERO || touch.jump || touch.look != Vec2::ZERO {
        *last = UserInputType::Touch;
    } else if keys.get_just_pressed().next().is_some() {
        *last = UserInputType::Keyboard;
    } else if mouse.just_pressed(MouseButton::Left) {
        *last = UserInputType::MouseButton1;
    } else if mouse.just_pressed(MouseButton::Right) {
        *last = UserInputType::MouseButton2;
    }
    // Phones and tablets have neither a keyboard nor a mouse (unless one is
    // plugged in, which scripts then see once it's used).
    let phone = cfg!(any(target_os = "android", target_os = "ios"));
    session.set_devices(libroxel_script::Devices {
        touch: touch.touched,
        gamepad: !gamepads.is_empty(),
        last: *last,
        keyboard: !phone || *last == UserInputType::Keyboard,
        mouse: !phone || matches!(*last, UserInputType::MouseButton1 | UserInputType::MouseButton2),
        vibration: cfg!(target_os = "android"),
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
    rigs: Query<(&Transform, &CameraRig)>,
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
    let look = rigs.iter().next().map(|(t, _)| t.forward().as_vec3()).unwrap_or(Vec3::NEG_Z);
    let walk = crate::devices::walk_direction(stick, look);
    // In first person the character faces where the camera looks.
    let face = rigs.iter().find(|(_, rig)| first_person(rig)).map(|(_, rig)| rig.yaw.to_degrees());
    input.0 = CharacterInput {
        move_dir: libroxel_instance::Vec3::new(walk.x, 0.0, walk.z),
        jump: pad.jump || touch.jump,
        throttle: stick.y,
        steer: stick.x,
        face,
    };
    if capture.keyboard {
        return;
    }
    // Relative to where the camera looks, flattened onto the ground.
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
    (locked, closest, farthest): (bool, f32, f32),
    dt: f32,
) {
    use crate::camera;
    let zoom = |zoom: f32| {
        if locked {
            camera::MIN_DISTANCE
        } else {
            zoom.clamp(closest.max(camera::MIN_DISTANCE), farthest.max(closest).min(camera::MAX_DISTANCE))
        }
    };
    if rig.following != Some(followed.root) {
        if rig.following.is_none() {
            // Just started playing: start behind, at the usual distance.
            rig.zoom = zoom(camera::START_DISTANCE);
            rig.distance = rig.zoom;
            rig.pitch = if rig.zoom <= camera::FIRST_PERSON { 0.0 } else { camera::START_PITCH };
        }
        rig.yaw = camera::behind(followed.yaw);
        rig.following = Some(followed.root);
        rig.shown = rig.distance;
    }
    rig.zoom = zoom(rig.zoom);
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

/// What the camera follows when its subject is `subject`: the root part of a
/// humanoid's model, a part itself, or a model's primary (else first) part.
/// `None` for the player's own humanoid, which is followed as usual, and for
/// anything that isn't in the world.
fn subject_followed(
    tree: &InstanceTree,
    subject: InstanceId,
    own_humanoid: Option<InstanceId>,
) -> Option<Followed> {
    if Some(subject) == own_humanoid || tree.is_destroyed(subject).unwrap_or(true) {
        return None;
    }
    let first_part = |model: InstanceId| {
        tree.get_ref(model, Property::PrimaryPart)
            .or_else(|| tree.find_first_child(model, "HumanoidRootPart").ok().flatten())
            .or_else(|| tree.descendants_of_class(model, ClassKind::Part).into_iter().next())
    };
    let mut humanoid = None;
    let (root, model) = match tree.class(subject).ok()? {
        ClassKind::Humanoid => {
            let model = tree.parent(subject).ok().flatten()?;
            humanoid = Some(subject);
            (first_part(model)?, Some(model))
        }
        ClassKind::Model => (first_part(subject)?, Some(subject)),
        class if class.is_physical() && class != ClassKind::Terrain => {
            let model =
                tree.parent(subject).ok().flatten().filter(|m| tree.class(*m) == Ok(ClassKind::Model));
            (subject, model)
        }
        _ => return None,
    };
    if !tree.is_descendant_of(root, tree.workspace()).unwrap_or(false) {
        return None;
    }
    let offset = humanoid.map_or(Default::default(), |h| crate::character::camera_offset(tree, h, root));
    let position = tree.get_vec3(root, Property::Position)? + offset;
    let yaw = tree.get_vec3(root, Property::Orientation).unwrap_or_default().y;
    Some(Followed { root, model, position, yaw, driving: None })
}

/// Whether the follow camera is in the character's head.
pub fn first_person(rig: &CameraRig) -> bool {
    rig.mode == CameraMode::Follow && rig.following.is_some() && rig.zoom <= crate::camera::FIRST_PERSON
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
        let character = session.and_then(|s| s.character());
        let offset = character
            .map_or(Default::default(), |c| crate::character::camera_offset(&tree, c.humanoid, root));
        let position = tree.get_vec3(root, Property::Position)? + offset;
        let yaw = tree.get_vec3(root, Property::Orientation).unwrap_or_default().y;
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
    // A script can point the camera at something else to follow.
    let own_humanoid = session.and_then(|s| s.character()).map(|c| c.humanoid);
    let followed = camera
        .and_then(|cam| {
            let tree = engine.tree.borrow();
            let subject = tree.get_ref(cam, Property::CameraSubject)?;
            subject_followed(&tree, subject, own_humanoid)
        })
        .or(followed);
    let (scripted, fov) = camera.map_or((false, 70.0), |cam| {
        let tree = engine.tree.borrow();
        let scripted = tree.get_enum(cam, Property::CameraType)
            == Some(EnumValue::CameraType(libroxel_instance::CameraType::Scriptable));
        (scripted, tree.get_number(cam, Property::FieldOfView).unwrap_or(70.0) as f32)
    });
    let limits = session.map_or((false, 0.0, f32::MAX), |s| s.runtime().camera_settings());
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
                place_follow(&mut rig, &mut transform, &engine.tree.borrow(), f, limits, dt);
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
            place_follow(&mut rig, &mut transform, &tree, f, limits, dt);
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

/// How faded the player's own character is.
#[derive(Resource, Default, PartialEq)]
struct SelfFade {
    model: Option<InstanceId>,
    fade: f32,
}

/// Fades the player's own body as the camera comes in close, and hides it
/// in first person. Held tools stay.
fn hide_self_in_first_person(
    engine: NonSend<EngineState>,
    entities: Res<PartEntities>,
    rigs: Query<&CameraRig>,
    mut faded: Local<std::collections::HashMap<Entity, (f32, f32)>>,
    mut parts: Query<&mut PartAppearance, With<PartLink>>,
    mut clusters: ResMut<crate::clusters::Clusters>,
    mut own: ResMut<SelfFade>,
) {
    let shown = rigs
        .iter()
        .filter(|rig| rig.mode == CameraMode::Follow && rig.following.is_some())
        .map(|rig| rig.shown)
        .reduce(f32::min);
    let fade = shown.map_or(0.0, crate::camera::self_fade);
    let model = engine.session.as_ref().and_then(|s| s.character()).map(|c| c.model);
    own.set_if_neq(SelfFade { model, fade });
    let wanted: Vec<Entity> = match engine.session.as_ref().and_then(|s| s.character()) {
        Some(c) if fade > 0.0 => {
            let tree = engine.tree.borrow();
            tree.descendants_of_class(c.model, ClassKind::Part)
                .into_iter()
                .filter(|id| {
                    let mut at = tree.parent(*id).ok().flatten();
                    while let Some(a) = at.filter(|a| *a != c.model) {
                        if tree.class(a) == Ok(ClassKind::Tool) {
                            return false;
                        }
                        at = tree.parent(a).ok().flatten();
                    }
                    true
                })
                .filter_map(|id| entities.0.get(&id).copied())
                .collect()
        }
        _ => Vec::new(),
    };
    faded.retain(|entity, (own, put)| {
        if wanted.contains(entity) {
            return true;
        }
        if let Ok(mut appearance) = parts.get_mut(*entity)
            && appearance.transparency == *put
        {
            appearance.transparency = *own;
            clusters.keep_apart(*entity);
        }
        false
    });
    for entity in wanted {
        let Ok(mut appearance) = parts.get_mut(entity) else { continue };
        let own = match faded.get(&entity) {
            Some((own, put)) if appearance.transparency == *put => *own,
            _ => appearance.transparency,
        };
        let put = 1.0 - (1.0 - own) * (1.0 - fade);
        if appearance.transparency != put {
            appearance.transparency = put;
            clusters.keep_apart(entity);
        }
        faded.insert(entity, (own, put));
    }
}

/// In first person the mouse pointer is hidden and held in the middle of
/// the window, so moving the mouse only looks round. Turning the camera
/// with the right button held keeps it where it was.
fn lock_cursor(
    engine: NonSend<EngineState>,
    capture: Res<InputCapture>,
    mouse: Res<ButtonInput<MouseButton>>,
    rigs: Query<&CameraRig>,
    mut windows: Query<(&mut Window, &mut bevy::window::CursorOptions), With<bevy::window::PrimaryWindow>>,
    mut held_at: Local<Option<Vec2>>,
) {
    let playing = engine.is_playing() && !capture.keyboard;
    let first = playing && rigs.iter().any(first_person);
    let turning = playing
        && !first
        && mouse.pressed(MouseButton::Right)
        && (held_at.is_some() || !capture.pointer)
        && rigs.iter().any(|r| r.mode == CameraMode::Follow);
    for (mut window, mut cursor) in &mut windows {
        if turning && held_at.is_none() {
            *held_at = window.cursor_position();
        }
        if !turning && let Some(at) = held_at.take() {
            window.set_cursor_position(Some(at));
        }
        let lock = window.focused && (first || turning);
        let mode = if lock {
            bevy::window::CursorGrabMode::Locked
        } else if window.focused && playing && capture.confine {
            bevy::window::CursorGrabMode::Confined
        } else {
            bevy::window::CursorGrabMode::None
        };
        if cursor.grab_mode != mode {
            cursor.grab_mode = mode;
            cursor.visible = !lock;
        }
        if lock {
            let keep = held_at.unwrap_or(Vec2::new(window.width(), window.height()) / 2.0);
            if window.cursor_position().is_some_and(|at| at.distance(keep) > 1.0) {
                window.set_cursor_position(Some(keep));
            }
        }
    }
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
    cameras: Query<&GlobalTransform, With<CameraRig>>,
    quality: Res<GraphicsQuality>,
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
    let most = quality.lights();
    if wanted.len() > most {
        let eye = cameras.iter().next().map_or(Vec3::ZERO, |t| t.translation());
        let mut near: Vec<_> = wanted.drain().collect();
        near.sort_by(|a, b| {
            a.1.1.translation.distance_squared(eye).total_cmp(&b.1.1.translation.distance_squared(eye))
        });
        near.truncate(most);
        wanted.extend(near);
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

/// Loaded pictures for decals (tiling ones repeat at the edges), and one
/// material for every decal that looks the same, so they draw together.
#[derive(Default)]
struct DecalPictures {
    generation: u64,
    loaded: HashMap<(String, bool), Option<Handle<Image>>>,
    materials: HashMap<DecalKey, Handle<StandardMaterial>>,
}

/// A decal's look, rounded to what can be told apart on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DecalKey(AssetId<Image>, [u16; 3], u16, Option<[u32; 2]>);

impl DecalKey {
    fn of(picture: &Handle<Image>, look: &DecalLook) -> Self {
        let q = |x: f32| (x.clamp(0.0, 1.0) * 1000.0).round() as u16;
        let t = look.tint;
        // Repeats to a sixteenth, as fine as shows.
        let tiles = look.tiles.map(|v| [(v.x * 16.0).round() as u32, (v.y * 16.0).round() as u32]);
        Self(picture.id(), [q(t.r), q(t.g), q(t.b)], q(look.transparency), tiles)
    }
}

/// What [`sync_decals`] remembers between frames, to look again only at
/// the decals whose instance or part changed.
#[derive(Default)]
struct DecalSync {
    tree: usize,
    epoch: u64,
    read: usize,
    structure: u64,
    fade: (u32, Option<InstanceId>),
    /// Every decal and texture in the workspace.
    ids: Vec<InstanceId>,
}

/// Draws every decal and texture on the side of its part.
#[allow(clippy::too_many_arguments)]
fn sync_decals(
    engine: NonSend<EngineState>,
    mut commands: Commands,
    meshes: Option<Res<PartMeshes>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut pictures: Local<DecalPictures>,
    mut sync: Local<DecalSync>,
    mut drawn: Query<(Entity, &mut DecalLink, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
    own: Res<SelfFade>,
) {
    use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
    let Some(meshes) = meshes else { return };
    let tree = engine.tree.borrow();

    // Most frames no decal or the part it's on changed: only the ones that
    // did are looked at again (all of them when decals came or went, the
    // pictures or the player's own fading changed, or the tree is new).
    let tree_ptr = std::rc::Rc::as_ptr(&engine.tree) as usize;
    let (epoch, _) = tree.touched_since(0);
    let fade = (own.fade.to_bits(), own.model);
    let generation = crate::pictures::generation();
    let full = sync.tree != tree_ptr
        || sync.epoch != epoch
        || tree.touched_len() < sync.read
        || sync.structure != tree.workspace_structure()
        || sync.fade != fade
        || pictures.generation != generation;
    let touched: FastSet<InstanceId> =
        if full { FastSet::default() } else { tree.touched_since(sync.read).1.iter().copied().collect() };
    sync.read = tree.touched_len();
    if !full && touched.is_empty() {
        return;
    }
    if full {
        sync.tree = tree_ptr;
        sync.epoch = epoch;
        sync.structure = tree.workspace_structure();
        sync.fade = fade;
        sync.ids = [ClassKind::Decal, ClassKind::Texture]
            .into_iter()
            .flat_map(|class| tree.descendants_of_class(tree.workspace(), class))
            .collect();
    }

    let faded = |part: InstanceId| {
        let Some(model) = own.model else { return false };
        let mut at = tree.parent(part).ok().flatten();
        while let Some(a) = at {
            if a == model {
                return true;
            }
            if tree.class(a) == Ok(ClassKind::Tool) {
                return false;
            }
            at = tree.parent(a).ok().flatten();
        }
        false
    };
    // Each decal looked at, and how it should be drawn (None: not at all).
    let mut wanted: FastMap<InstanceId, Option<(Transform, DecalLook)>> = FastMap::default();
    for &id in &sync.ids {
        let part = tree.parent(id).ok().flatten();
        if !full && !touched.contains(&id) && !part.is_some_and(|p| touched.contains(&p)) {
            continue;
        }
        let look = (|| {
            let part = part.filter(|p| tree.class(*p).is_ok_and(|c| c.is_physical()))?;
            let (pos, size) =
                (tree.get_vec3(part, Property::Position)?, tree.get_vec3(part, Property::Size)?);
            let face = match tree.get_enum(id, Property::Face) {
                Some(EnumValue::NormalId(face)) => face,
                _ => NormalId::Front,
            };
            let orientation = tree.get_vec3(part, Property::Orientation).unwrap_or_default();
            let (transform, extent) = decal_transform(pos, size, orientation, face);
            let tiles = (tree.class(id) == Ok(ClassKind::Texture)).then(|| {
                let per = |p| (tree.get_number(id, p).unwrap_or(2.0) as f32).max(0.01);
                extent / Vec2::new(per(Property::StudsPerTileU), per(Property::StudsPerTileV))
            });
            let mut transparency = tree.get_number(id, Property::Transparency).unwrap_or(0.0) as f32;
            if own.fade > 0.0 && faded(part) {
                transparency = 1.0 - (1.0 - transparency) * (1.0 - own.fade);
            }
            let picture = tree.get_string(id, Property::Texture).unwrap_or_default();
            let picture = picture.trim();
            if picture.is_empty() || transparency >= 0.999 {
                return None;
            }
            let look = DecalLook {
                picture: picture.to_owned(),
                tint: tree.get_color(id, Property::DecalColor).unwrap_or(libroxel_instance::Color3::WHITE),
                transparency,
                tiles,
            };
            Some((transform, look))
        })();
        wanted.insert(id, look);
    }
    drop(tree);

    if pictures.generation != generation {
        pictures.generation = generation;
        pictures.loaded.clear();
        pictures.materials.clear();
    }
    let pictures = &mut *pictures;
    let mut material_for = |look: &DecalLook| -> Option<Handle<StandardMaterial>> {
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
                let bytes = crate::pictures::bytes_for(&look.picture)?;
                let image = crate::pictures::decode_for_view(&bytes, sampler)?;
                Some(images.add(image))
            })
            .clone()?;
        let handle = pictures.materials.entry(DecalKey::of(&picture, look)).or_insert_with(|| {
            let t = look.tint;
            materials.add(StandardMaterial {
                base_color: Color::srgba(t.r, t.g, t.b, (1.0 - look.transparency).clamp(0.0, 1.0)),
                base_color_texture: Some(picture),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.8,
                uv_transform: bevy::math::Affine2::from_scale(look.tiles.unwrap_or(Vec2::ONE)),
                // Drawn over the part's own side rather than fighting it.
                depth_bias: 1.0,
                ..default()
            })
        });
        Some(handle.clone())
    };
    for (entity, mut link, mut transform, mut handle) in &mut drawn {
        let Some(now) = wanted.remove(&link.0) else {
            if full {
                commands.entity(entity).despawn();
            }
            continue;
        };
        match now {
            Some((t, look)) => {
                transform.set_if_neq(t);
                if link.1 != look {
                    match material_for(&look) {
                        Some(material) => {
                            if handle.0 != material {
                                handle.0 = material;
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
    let mut spawned = Vec::new();
    for (id, now) in wanted {
        let Some((transform, look)) = now else { continue };
        let Some(material) = material_for(&look) else { continue };
        spawned.push(material.id());
        commands.spawn((
            DecalLink(id, look),
            Mesh3d(meshes.quad.clone()),
            MeshMaterial3d(material),
            transform,
            bevy::light::NotShadowCaster,
        ));
    }
    // Looks no decal shows any more (one that faded out bit by bit made a
    // look for every step) are let go once there are many of them.
    let count = drawn.iter().len() + spawned.len();
    if pictures.materials.len() > 2 * count + 64 {
        let used: FastSet<AssetId<StandardMaterial>> =
            drawn.iter().map(|(_, _, _, m)| m.0.id()).chain(spawned).collect();
        pictures.materials.retain(|_, m| used.contains(&m.id()));
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

    #[test]
    fn the_camera_follows_its_subject() {
        let mut tree = InstanceTree::new();
        let ws = tree.workspace();
        let car = tree.create(ClassKind::Model, "Car").unwrap();
        tree.set_parent(car, Some(ws)).unwrap();
        let body = tree.create(ClassKind::Part, "Body").unwrap();
        tree.set_parent(body, Some(car)).unwrap();
        tree.set(body, Property::Position, libroxel_instance::PropertyValue::Vec3(V::new(5.0, 1.0, 2.0)))
            .unwrap();
        let npc = tree.create(ClassKind::Model, "Npc").unwrap();
        tree.set_parent(npc, Some(ws)).unwrap();
        let root = tree.create(ClassKind::Part, "HumanoidRootPart").unwrap();
        tree.set_parent(root, Some(npc)).unwrap();
        let humanoid = tree.create(ClassKind::Humanoid, "Humanoid").unwrap();
        tree.set_parent(humanoid, Some(npc)).unwrap();
        let stored = tree.create(ClassKind::Part, "Stored").unwrap();

        let f = subject_followed(&tree, body, None).unwrap();
        assert_eq!((f.root, f.model), (body, Some(car)));
        assert_eq!(f.position, V::new(5.0, 1.0, 2.0));
        assert_eq!(subject_followed(&tree, car, None).unwrap().root, body);
        let f = subject_followed(&tree, humanoid, None).unwrap();
        assert_eq!((f.root, f.model), (root, Some(npc)));
        // The player's own humanoid is followed the usual way, and nothing
        // outside the world is followed at all.
        assert!(subject_followed(&tree, humanoid, Some(humanoid)).is_none());
        assert!(subject_followed(&tree, stored, None).is_none());
    }

    #[test]
    fn auto_quality_holds_sixty() {
        let mut q = GraphicsQuality::new(true, 10);
        for _ in 0..600 {
            q.frame(1.0 / 30.0);
        }
        assert!(q.level <= 3, "{}", q.level);
        let low = q.level;
        for _ in 0..600 {
            q.frame(1.0 / 60.0);
        }
        assert_eq!(q.level, low);
        for _ in 0..(70 * 60) {
            q.frame(1.0 / 60.0);
        }
        assert!(q.level > low);
        let mut fixed = GraphicsQuality::new(false, 1);
        for _ in 0..600 {
            fixed.frame(1.0 / 200.0);
        }
        assert_eq!(fixed.level, 1);
        assert!(!fixed.shadows() && !fixed.smoothing() && fixed.lights() == 0);
    }

    #[test]
    fn phones_start_low_and_stay_in_the_middle() {
        let mut q = GraphicsQuality::default().phone();
        assert_eq!(q.level, 3);
        assert!(!q.shadows() && !q.smoothing() && !q.bloom());
        // However fast it runs, automatic stops at the middle.
        for _ in 0..(200 * 60) {
            q.frame(1.0 / 60.0);
        }
        assert_eq!(q.level, 6);
        assert!(!q.smoothing() && !q.bloom(), "the costliest effects stay off");
        // Picking a level by hand still reaches the top.
        q.set(false, 10);
        assert!(q.smoothing() && q.bloom() && q.shadows());
        assert_eq!(q.shadow_detail(), (2, 2048));
        // And back on automatic, it still stops at the middle.
        q.set(true, 3);
        for _ in 0..(200 * 60) {
            q.frame(1.0 / 60.0);
        }
        assert_eq!(q.level, 6);
        assert_eq!(q.render_density(), Some(1.5));
        assert_eq!(GraphicsQuality::default().render_density(), None, "computers draw every pixel");
    }

    #[test]
    fn phones_spend_less_on_shadows_small_parts_and_far_models() {
        use bevy::light::ShadowFilteringMethod as Filter;
        let computer = GraphicsQuality::new(false, 5);
        let phone = GraphicsQuality::new(false, 5).phone();
        assert!(phone.is_phone() && !computer.is_phone());
        assert_eq!(
            (computer.shadow_filter(), phone.shadow_filter()),
            (Filter::Gaussian, Filter::Hardware2x2)
        );
        assert!(phone.shadow_distance() < computer.shadow_distance());
        assert!(phone.small_part_reach() < 1.0 && computer.small_part_reach() == 1.0);
        assert!(phone.far_model() < computer.far_model());
        // A phone set to the top draws as a computer does.
        let top = GraphicsQuality::new(false, 10).phone();
        assert_eq!(top.shadow_filter(), Filter::Gaussian);
        assert_eq!(top.shadow_distance(), GraphicsQuality::new(false, 10).shadow_distance());
        assert_eq!((top.small_part_reach(), top.far_model()), (1.0, FAR_MODEL));
    }

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
