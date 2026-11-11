//! Bevy integration: mirroring parts onto entities, drawing them, cameras,
//! and turning keyboard input into character movement.

use std::collections::HashMap;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use libroxel_instance::{ClassKind, EnumValue, InstanceId, InstanceTree, Material, PartShape, Property};
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
    pub material: Material,
    /// How many times surface textures repeat across the part.
    pub texture_repeat: f32,
}

/// Which mesh the part is drawn with.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartGeometry(pub PartShape);

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
    mut parts: Query<(&mut Transform, &mut PartAppearance, &mut PartName, &mut PartGeometry), With<PartLink>>,
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
        let material = match tree.get_enum(id, Property::Material) {
            Some(EnumValue::Material(m)) => m,
            _ => Material::Plastic,
        };
        let shape = part_shape(&tree, id);
        let orientation = tree.get_vec3(id, Property::Orientation).unwrap_or_default();
        let transform = part_transform(pos, size, orientation, shape);
        let texture_repeat = (size.x.max(size.y).max(size.z) / 8.0).max(0.25);
        let appearance = PartAppearance { color, transparency, material, texture_repeat };
        let name = PartName(tree.name(id).unwrap_or_default().to_owned());
        let geometry = PartGeometry(shape);

        match entities.0.get(&id).and_then(|e| parts.get_mut(*e).ok()) {
            Some((mut t, mut a, mut n, mut g)) => {
                t.set_if_neq(transform);
                a.set_if_neq(appearance);
                n.set_if_neq(name);
                g.set_if_neq(geometry);
            }
            None if !entities.0.contains_key(&id) => {
                let entity = commands.spawn((PartLink(id), name, transform, appearance, geometry)).id();
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
            .add_systems(PostUpdate, (attach_visuals, update_materials, update_meshes).chain())
            .add_systems(PostUpdate, follow_camera.before(TransformSystems::Propagate));
    }
}

/// One unit-sized mesh per part shape.
#[derive(Resource)]
struct PartMeshes {
    block: Handle<Mesh>,
    ball: Handle<Mesh>,
    cylinder: Handle<Mesh>,
    wedge: Handle<Mesh>,
}

impl PartMeshes {
    fn get(&self, shape: PartShape) -> Handle<Mesh> {
        match shape {
            PartShape::Block => self.block.clone(),
            PartShape::Ball => self.ball.clone(),
            PartShape::Cylinder => self.cylinder.clone(),
            PartShape::Wedge => self.wedge.clone(),
        }
    }
}

/// Generated surface textures for the textured materials.
#[derive(Resource)]
struct MaterialTextures {
    wood: Handle<Image>,
    brick: Handle<Image>,
    grass: Handle<Image>,
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

/// A small greyscale texture that tints the part's colour. Chunky pixels
/// on purpose, to match the blocky look.
fn make_texture(shade: impl Fn(u32, u32) -> f32) -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    const SIZE: u32 = 32;
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let v = (shade(x, y).clamp(0.0, 1.0) * 255.0) as u8;
            data.extend([v, v, v, 255]);
        }
    }
    let mut image = Image::new(
        Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::nearest()
    });
    image
}

fn material_textures(images: &mut Assets<Image>) -> MaterialTextures {
    let wood = make_texture(|x, y| {
        // Long grain running along U, with a few wobbles.
        let wobble = (noise(0, y / 4, 7) - 0.5) * 3.0;
        let ring = ((y as f32 + wobble) * 0.9).sin() * 0.5 + 0.5;
        0.72 + ring * 0.18 + noise(x / 8, y, 3) * 0.08
    });
    let brick = make_texture(|x, y| {
        let row = y / 8;
        let offset = if row % 2 == 0 { 0 } else { 8 };
        let mortar = y % 8 == 0 || (x + offset) % 16 == 0;
        if mortar {
            0.97
        } else {
            0.66 + noise((x + offset) / 16, row, 11) * 0.18 + noise(x / 2, y / 2, 5) * 0.06
        }
    });
    let grass = make_texture(|x, y| 0.68 + noise(x / 2, y / 2, 17) * 0.3);
    MaterialTextures { wood: images.add(wood), brick: images.add(brick), grass: images.add(grass) }
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
    });
    commands.insert_resource(material_textures(&mut images));
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

fn material_for(appearance: &PartAppearance, textures: &MaterialTextures) -> StandardMaterial {
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
    }
    material
}

fn visibility_for(appearance: &PartAppearance) -> Visibility {
    if appearance.transparency >= 0.999 { Visibility::Hidden } else { Visibility::Inherited }
}

fn attach_visuals(
    mut commands: Commands,
    meshes: Res<PartMeshes>,
    textures: Res<MaterialTextures>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_parts: Query<(Entity, &PartAppearance, &PartGeometry), Added<PartLink>>,
) {
    for (entity, appearance, geometry) in &new_parts {
        commands.entity(entity).insert((
            Mesh3d(meshes.get(geometry.0)),
            MeshMaterial3d(materials.add(material_for(appearance, &textures))),
            visibility_for(appearance),
        ));
    }
}

fn update_materials(
    textures: Res<MaterialTextures>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut changed: Query<
        (&PartAppearance, &MeshMaterial3d<StandardMaterial>, &mut Visibility),
        Changed<PartAppearance>,
    >,
) {
    for (appearance, handle, mut visibility) in &mut changed {
        if let Some(mut material) = materials.get_mut(&handle.0) {
            *material = material_for(appearance, &textures);
        }
        visibility.set_if_neq(visibility_for(appearance));
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
