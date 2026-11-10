//! Bevy integration: mirroring parts onto entities, drawing them, cameras,
//! and turning keyboard input into character movement.

use std::collections::HashMap;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property};
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
}

impl EngineState {
    pub fn new(tree: InstanceTree) -> Self {
        Self { tree: std::rc::Rc::new(std::cell::RefCell::new(tree)), session: None, log: Vec::new() }
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
}

#[derive(Resource, Default)]
struct PartEntities(HashMap<InstanceId, Entity>);

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
}

fn to_bevy(v: libroxel_instance::Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// Mirrors workspace parts onto entities.
///
/// This does a full pass each frame, which is fine for hundreds of parts.
/// It will move to the tree's change journal when worlds get bigger.
fn sync_parts(
    engine: NonSend<EngineState>,
    mut entities: ResMut<PartEntities>,
    mut commands: Commands,
    mut parts: Query<(&mut Transform, &mut PartAppearance, &mut PartName), With<PartLink>>,
) {
    let mut tree = engine.tree.borrow_mut();
    tree.drain_changes();

    let live: Vec<InstanceId> = tree.descendants_of_class(tree.workspace(), ClassKind::Part);
    let live_set: std::collections::HashSet<_> = live.iter().copied().collect();

    entities.0.retain(|id, entity| {
        let keep = live_set.contains(id);
        if !keep {
            commands.entity(*entity).despawn();
        }
        keep
    });

    for id in live {
        let (Some(pos), Some(size)) =
            (tree.get_vec3(id, Property::Position), tree.get_vec3(id, Property::Size))
        else {
            continue;
        };
        let color =
            tree.get_color(id, Property::Color).map(|c| Color::srgb(c.r, c.g, c.b)).unwrap_or(Color::WHITE);
        let transparency = tree.get_number(id, Property::Transparency).unwrap_or(0.0) as f32;
        let transform =
            Transform::from_translation(to_bevy(pos)).with_scale(to_bevy(size).max(Vec3::splat(0.01)));
        let appearance = PartAppearance { color, transparency };
        let name = PartName(tree.name(id).unwrap_or_default().to_owned());

        match entities.0.get(&id).and_then(|e| parts.get_mut(*e).ok()) {
            Some((mut t, mut a, mut n)) => {
                t.set_if_neq(transform);
                a.set_if_neq(appearance);
                n.set_if_neq(name);
            }
            None if !entities.0.contains_key(&id) => {
                let entity = commands.spawn((PartLink(id), name, transform, appearance)).id();
                entities.0.insert(id, entity);
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
            .add_systems(Update, (camera_controls, play_input).chain().in_set(EngineSet::Input))
            .add_systems(PostUpdate, (attach_visuals, update_materials).chain())
            .add_systems(PostUpdate, follow_camera.before(TransformSystems::Propagate));
    }
}

#[derive(Resource)]
struct UnitCube(Handle<Mesh>);

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
    /// Follow mode: distance behind the character.
    pub distance: f32,
    /// Fly mode: studs per second.
    pub fly_speed: f32,
}

impl Default for CameraRig {
    fn default() -> Self {
        Self { mode: CameraMode::Fly, yaw: -0.6, pitch: -0.45, distance: 16.0, fly_speed: 30.0 }
    }
}

impl CameraRig {
    pub fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }
}

fn setup_scene(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    // Parts are unit cubes scaled by their Size.
    commands.insert_resource(UnitCube(meshes.add(Cuboid::new(1.0, 1.0, 1.0))));
    let rig = CameraRig::default();
    commands.spawn((
        Camera3d::default(),
        rig,
        Transform::from_xyz(-18.0, 16.0, 24.0).with_rotation(rig.rotation()),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 9_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(8.0, 20.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(ClearColor(Color::srgb(0.53, 0.72, 0.9)));
}

fn material_for(appearance: &PartAppearance) -> StandardMaterial {
    let alpha = (1.0 - appearance.transparency).clamp(0.0, 1.0);
    StandardMaterial {
        base_color: appearance.color.with_alpha(alpha),
        alpha_mode: if alpha < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
        perceptual_roughness: 0.85,
        ..default()
    }
}

fn visibility_for(appearance: &PartAppearance) -> Visibility {
    if appearance.transparency >= 0.999 { Visibility::Hidden } else { Visibility::Inherited }
}

fn attach_visuals(
    mut commands: Commands,
    cube: Res<UnitCube>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_parts: Query<(Entity, &PartAppearance), Added<PartLink>>,
) {
    for (entity, appearance) in &new_parts {
        commands.entity(entity).insert((
            Mesh3d(cube.0.clone()),
            MeshMaterial3d(materials.add(material_for(appearance))),
            visibility_for(appearance),
        ));
    }
}

fn update_materials(
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut changed: Query<
        (&PartAppearance, &MeshMaterial3d<StandardMaterial>, &mut Visibility),
        Changed<PartAppearance>,
    >,
) {
    for (appearance, handle, mut visibility) in &mut changed {
        if let Some(mut material) = materials.get_mut(&handle.0) {
            *material = material_for(appearance);
        }
        visibility.set_if_neq(visibility_for(appearance));
    }
}

fn camera_controls(
    capture: Res<InputCapture>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    mut rigs: Query<(&mut CameraRig, &mut Transform)>,
) {
    let looking = mouse.pressed(MouseButton::Right) && !capture.pointer;
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
            rig.pitch = (rig.pitch - motion.delta.y * 0.005).clamp(-1.5, 1.5);
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
                rig.distance = (rig.distance - scroll_lines * 2.0).clamp(4.0, 80.0);
            }
        }
    }
}

/// WASD relative to where the camera faces, Space to jump.
fn play_input(
    engine: NonSend<EngineState>,
    capture: Res<InputCapture>,
    keys: Res<ButtonInput<KeyCode>>,
    rigs: Query<&CameraRig>,
    mut input: ResMut<PlayInput>,
) {
    input.0 = CharacterInput::default();
    if !engine.is_playing() || capture.keyboard {
        return;
    }
    let yaw = rigs.iter().next().map(|r| r.yaw).unwrap_or(0.0);
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
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
    input.0 = CharacterInput {
        move_dir: libroxel_instance::Vec3::new(dir.x, 0.0, dir.z),
        jump: keys.pressed(KeyCode::Space),
    };
}

fn follow_camera(engine: NonSend<EngineState>, mut rigs: Query<(&mut CameraRig, &mut Transform)>) {
    let focus = engine.session.as_ref().and_then(|s| s.focus()).and_then(|id| {
        let tree = engine.tree.borrow();
        if tree.is_destroyed(id).unwrap_or(true) {
            return None;
        }
        tree.get_vec3(id, Property::Position)
    });
    for (mut rig, mut transform) in &mut rigs {
        if engine.is_playing() {
            rig.mode = CameraMode::Follow;
        } else if rig.mode == CameraMode::Follow {
            rig.mode = CameraMode::Fly;
        }
        if rig.mode != CameraMode::Follow {
            continue;
        }
        if let Some(target) = focus {
            let target = to_bevy(target) + Vec3::Y * 2.0;
            transform.rotation = rig.rotation();
            transform.translation = target - transform.forward().as_vec3() * rig.distance;
        }
    }
}
