//! Visual effects drawn by the renderer: particle emitters, fire, smoke and
//! sparkles, and highlights. They only exist on screen; the instance tree
//! holds their settings and nothing about each particle.
//!
//! Each particle is a small entity. Its colour and fade come from one of a
//! handful of materials per emitter, picked by how far through its life it
//! is, so particles never need a material of their own.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use bevy::render::render_resource::Face;
use libroxel_instance::{ClassKind, Color3, EnumValue, InstanceId, InstanceTree, PartShape, Property};

use crate::view::{EngineSet, EngineState, PartMeshes};

/// Most particles one emitter keeps alive at once.
const MAX_PARTICLES: usize = 400;
/// Materials per emitter, from the start of a particle's life to its end.
const STAGES: usize = 4;

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Emitters>()
            .init_resource::<Highlights>()
            .add_systems(Startup, setup_effect_meshes)
            .add_systems(Update, (update_particles, sync_highlights).after(EngineSet::Sync));
    }
}

#[derive(Resource)]
struct EffectMeshes {
    puff: Handle<Mesh>,
    cube: Handle<Mesh>,
}

fn setup_effect_meshes(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(EffectMeshes {
        puff: meshes.add(Sphere::new(0.5).mesh().ico(1).expect("a low ico sphere is valid")),
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
    });
}

fn to_bevy(v: libroxel_instance::Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn rgb(c: Color3) -> Vec3 {
    Vec3::new(c.r, c.g, c.b)
}

/// How an emitter's particles look from birth to death.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Look {
    start: Vec3,
    end: Vec3,
    alpha: (f32, f32),
    /// 0 is lit like a part; 1 glows.
    glow: f32,
    cube: bool,
}

/// What an emitter does, read from its instance each frame.
#[derive(Debug, Clone, Copy)]
struct Settings {
    rate: f32,
    life: (f32, f32),
    speed: (f32, f32),
    /// Degrees away from straight out.
    spread: f32,
    size: (f32, f32),
    accel: Vec3,
    drag: f32,
    look: Look,
    /// Where particles start: a box (centre, half size, rotation).
    origin: (Vec3, Vec3, Quat),
    /// The direction particles leave in.
    up: Vec3,
}

fn range(tree: &InstanceTree, id: InstanceId, p: Property) -> (f32, f32) {
    let v = tree.get_vec2(id, p).unwrap_or_default();
    (v.x.min(v.y), v.x.max(v.y))
}

fn num(tree: &InstanceTree, id: InstanceId, p: Property, default: f32) -> f32 {
    tree.get_number(id, p).map_or(default, |n| n as f32)
}

fn colour(tree: &InstanceTree, id: InstanceId, p: Property) -> Vec3 {
    tree.get_color(id, p).map_or(Vec3::ONE, rgb)
}

/// Where an effect inside `parent` comes from: a part's box, or an attachment's point.
fn origin_of(tree: &InstanceTree, parent: InstanceId) -> Option<((Vec3, Vec3, Quat), Vec3)> {
    let class = tree.class(parent).ok()?;
    if class == ClassKind::Attachment {
        let (point, part) = crate::movers::attachment_world(tree, parent)?;
        let rot =
            crate::physics::orientation_quat(tree.get_vec3(part, Property::Orientation).unwrap_or_default());
        return Some(((to_bevy(point), Vec3::ZERO, rot), rot * Vec3::Y));
    }
    if !class.is_physical() {
        return None;
    }
    let pos = to_bevy(tree.get_vec3(parent, Property::Position)?);
    let half = to_bevy(tree.get_vec3(parent, Property::Size)?) / 2.0;
    let rot =
        crate::physics::orientation_quat(tree.get_vec3(parent, Property::Orientation).unwrap_or_default());
    Some(((pos, half, rot), rot * Vec3::Y))
}

fn settings(tree: &InstanceTree, id: InstanceId, class: ClassKind) -> Option<Settings> {
    let parent = tree.parent(id).ok()??;
    let ((centre, half, rot), up) = origin_of(tree, parent)?;
    Some(match class {
        ClassKind::ParticleEmitter => {
            let spread = tree.get_vec2(id, Property::SpreadAngle).unwrap_or_default();
            let size = num(tree, id, Property::EffectSize, 1.0);
            let c = colour(tree, id, Property::Color);
            let alpha = 1.0 - num(tree, id, Property::Transparency, 0.0);
            Settings {
                rate: num(tree, id, Property::Rate, 20.0),
                life: range(tree, id, Property::Lifetime),
                speed: range(tree, id, Property::Speed),
                spread: spread.x.abs().max(spread.y.abs()),
                size: (size, size),
                accel: to_bevy(tree.get_vec3(id, Property::Acceleration).unwrap_or_default()),
                drag: num(tree, id, Property::Drag, 0.0),
                look: Look {
                    start: c,
                    end: c,
                    alpha: (alpha, alpha),
                    glow: num(tree, id, Property::LightEmission, 0.0),
                    cube: false,
                },
                origin: (centre, half, rot),
                up,
            }
        }
        ClassKind::Fire => {
            let size = num(tree, id, Property::EffectSize, 5.0).max(0.2);
            let heat = num(tree, id, Property::Heat, 9.0);
            let scale = (size / 5.0).sqrt();
            Settings {
                rate: 45.0,
                life: (0.5 * scale, 0.9 * scale),
                speed: (heat * 0.35, heat * 0.6),
                spread: 12.0,
                size: (size * 0.45, size * 0.1),
                accel: Vec3::Y * heat * 0.4,
                drag: 0.5,
                look: Look {
                    start: colour(tree, id, Property::Color),
                    end: colour(tree, id, Property::SecondaryColor),
                    alpha: (0.9, 0.35),
                    glow: 1.0,
                    cube: false,
                },
                origin: (centre, Vec3::new(half.x, 0.0, half.z) * 0.4, rot),
                up: Vec3::Y,
            }
        }
        ClassKind::Smoke => {
            let size = num(tree, id, Property::EffectSize, 1.0).max(0.1);
            let rise = num(tree, id, Property::RiseVelocity, 1.0);
            let opacity = num(tree, id, Property::Opacity, 0.5);
            let c = colour(tree, id, Property::Color);
            Settings {
                rate: 10.0,
                life: (4.0, 6.0),
                speed: (rise * 0.8 + 0.3, rise * 1.2 + 0.6),
                spread: 18.0,
                size: (size * 0.8, size * 2.5),
                accel: Vec3::ZERO,
                drag: 0.1,
                look: Look { start: c, end: c, alpha: (opacity * 0.8, 0.0), glow: 0.0, cube: false },
                origin: (centre, Vec3::new(half.x, 0.0, half.z) * 0.3, rot),
                up: Vec3::Y,
            }
        }
        ClassKind::Sparkles => {
            let c = colour(tree, id, Property::SparkleColor);
            Settings {
                rate: 25.0,
                life: (0.6, 1.2),
                speed: (2.0, 4.0),
                spread: 180.0,
                size: (0.3, 0.1),
                accel: Vec3::ZERO,
                drag: 1.0,
                look: Look { start: c, end: Vec3::ONE, alpha: (1.0, 0.3), glow: 1.0, cube: true },
                origin: (centre, half * 0.8, rot),
                up: Vec3::Y,
            }
        }
        _ => return None,
    })
}

struct Particle {
    entity: Entity,
    pos: Vec3,
    vel: Vec3,
    age: f32,
    life: f32,
    spin: Quat,
    stage: usize,
}

struct Emitter {
    particles: Vec<Particle>,
    materials: Vec<Handle<StandardMaterial>>,
    look: Option<Look>,
    carry: f32,
    rng: u64,
}

impl Emitter {
    fn new(id: InstanceId) -> Self {
        let seed = format!("{id}")
            .bytes()
            .fold(0x9E37_79B9_7F4A_7C15u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01B3));
        Self { particles: Vec::new(), materials: Vec::new(), look: None, carry: 0.0, rng: seed | 1 }
    }

    /// A number in [0, 1).
    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    fn between(&mut self, (lo, hi): (f32, f32)) -> f32 {
        lo + (hi - lo) * self.random()
    }
}

#[derive(Resource, Default)]
struct Emitters(HashMap<InstanceId, Emitter>);

fn stage_material(look: &Look, stage: usize) -> StandardMaterial {
    let t = stage as f32 / (STAGES - 1) as f32;
    let c = look.start.lerp(look.end, t);
    let alpha = (look.alpha.0 + (look.alpha.1 - look.alpha.0) * t).clamp(0.0, 1.0);
    StandardMaterial {
        base_color: Color::srgba(c.x, c.y, c.z, alpha),
        emissive: LinearRgba::rgb(c.x, c.y, c.z) * (look.glow * 2.0),
        unlit: look.glow >= 0.5,
        alpha_mode: if alpha < 0.999 { AlphaMode::Blend } else { AlphaMode::Opaque },
        perceptual_roughness: 0.9,
        ..default()
    }
}

fn update_particles(
    mut engine: NonSendMut<EngineState>,
    time: Res<Time>,
    meshes: Option<Res<EffectMeshes>>,
    mut emitters: ResMut<Emitters>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    mut transforms: Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(meshes) = meshes else { return };
    let dt = time.delta_secs().min(0.1);
    let bursts: HashMap<InstanceId, u32> = engine.emits.drain(..).fold(HashMap::new(), |mut m, (id, n)| {
        *m.entry(id).or_default() += n;
        m
    });
    let tree = engine.tree.borrow();
    let mut seen = HashSet::new();
    for class in [ClassKind::ParticleEmitter, ClassKind::Fire, ClassKind::Smoke, ClassKind::Sparkles] {
        for id in tree.descendants_of_class(tree.workspace(), class) {
            seen.insert(id);
            let settings = settings(&tree, id, class);
            let enabled = tree.get_bool(id, Property::Enabled) == Some(true);
            let burst = bursts.get(&id).copied().unwrap_or(0);
            let emitter = emitters.0.entry(id).or_insert_with(|| Emitter::new(id));
            let Some(s) = settings else { continue };
            if emitter.look != Some(s.look) {
                emitter.materials = (0..STAGES).map(|i| materials.add(stage_material(&s.look, i))).collect();
                emitter.look = Some(s.look);
                for p in &mut emitter.particles {
                    p.stage = usize::MAX; // pick up the new materials
                }
            }
            // How many to send out this frame.
            let mut count = burst as usize;
            if enabled {
                emitter.carry += s.rate.max(0.0) * dt;
                let whole = emitter.carry.floor();
                emitter.carry -= whole;
                count += whole as usize;
            }
            let room = MAX_PARTICLES.saturating_sub(emitter.particles.len());
            for _ in 0..count.min(room) {
                let (centre, half, rot) = s.origin;
                let local = Vec3::new(
                    (emitter.random() * 2.0 - 1.0) * half.x,
                    (emitter.random() * 2.0 - 1.0) * half.y,
                    (emitter.random() * 2.0 - 1.0) * half.z,
                );
                let pos = centre + rot * local;
                // A direction within `spread` degrees of straight out.
                let tilt = (emitter.random() * s.spread.clamp(0.0, 180.0)).to_radians();
                let turn = emitter.random() * std::f32::consts::TAU;
                let side = s.up.any_orthonormal_vector();
                let dir = Quat::from_axis_angle(s.up, turn) * (Quat::from_axis_angle(side, tilt) * s.up);
                let vel = dir * emitter.between(s.speed);
                let life = emitter.between(s.life).max(0.05);
                let spin = Quat::from_euler(
                    EulerRot::YXZ,
                    emitter.random() * 6.3,
                    emitter.random() * 6.3,
                    emitter.random() * 6.3,
                );
                let mesh = if s.look.cube { meshes.cube.clone() } else { meshes.puff.clone() };
                let entity = commands
                    .spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(emitter.materials[0].clone()),
                        Transform::from_translation(pos).with_scale(Vec3::splat(s.size.0)),
                        bevy::light::NotShadowCaster,
                    ))
                    .id();
                emitter.particles.push(Particle { entity, pos, vel, age: 0.0, life, spin, stage: 0 });
            }
            // Move, age and draw what's alive.
            let materials_now = emitter.materials.clone();
            emitter.particles.retain_mut(|p| {
                p.age += dt;
                if p.age >= p.life {
                    commands.entity(p.entity).despawn();
                    return false;
                }
                p.vel += s.accel * dt;
                p.vel *= (1.0 - s.drag * dt).max(0.0);
                p.pos += p.vel * dt;
                let t = p.age / p.life;
                // Grow or shrink through life, and shrink away at the very end.
                let mut size = s.size.0 + (s.size.1 - s.size.0) * t;
                if t > 0.85 {
                    size *= 1.0 - (t - 0.85) / 0.15;
                }
                let stage = ((t * STAGES as f32) as usize).min(STAGES - 1);
                if let Ok((mut transform, mut material)) = transforms.get_mut(p.entity) {
                    *transform = Transform::from_translation(p.pos)
                        .with_rotation(p.spin)
                        .with_scale(Vec3::splat(size.max(0.001)));
                    if stage != p.stage {
                        material.0 = materials_now[stage].clone();
                    }
                }
                p.stage = stage;
                true
            });
        }
    }
    // Emitters that are gone take their particles with them.
    emitters.0.retain(|id, emitter| {
        if seen.contains(id) {
            return true;
        }
        for p in &emitter.particles {
            commands.entity(p.entity).despawn();
        }
        false
    });
}

/// How many particles are on screen, for tests.
pub fn particle_count(world: &World) -> usize {
    world.get_resource::<Emitters>().map_or(0, |e| e.0.values().map(|e| e.particles.len()).sum())
}

// ---------------------------------------------------------------------------
// Highlights
// ---------------------------------------------------------------------------

/// The fill and outline drawn over one part for one highlight.
struct Overlay {
    fill: Entity,
    outline: Entity,
}

/// Fill colour and transparency, then outline colour and transparency.
type HighlightLook = (Vec3, f32, Vec3, f32);

struct HighlightState {
    fill: Handle<StandardMaterial>,
    outline: Handle<StandardMaterial>,
    look: HighlightLook,
    parts: HashMap<InstanceId, Overlay>,
}

#[derive(Resource, Default)]
struct Highlights(HashMap<InstanceId, HighlightState>);

/// How far the outline sticks out past the part, in studs.
const OUTLINE: f32 = 0.22;

fn highlight_materials(look: HighlightLook) -> (StandardMaterial, StandardMaterial) {
    let (fill, fill_t, line, line_t) = look;
    let alpha = |t: f32| (1.0 - t).clamp(0.0, 1.0);
    let mode = |a: f32| if a < 0.999 { AlphaMode::Blend } else { AlphaMode::Opaque };
    (
        StandardMaterial {
            base_color: Color::srgba(fill.x, fill.y, fill.z, alpha(fill_t)),
            unlit: true,
            alpha_mode: mode(alpha(fill_t)),
            ..default()
        },
        StandardMaterial {
            base_color: Color::srgba(line.x, line.y, line.z, alpha(line_t)),
            unlit: true,
            alpha_mode: mode(alpha(line_t)),
            // Only the inside faces of a slightly bigger copy: an outline around the part.
            cull_mode: Some(Face::Front),
            ..default()
        },
    )
}

fn sync_highlights(
    engine: NonSend<EngineState>,
    part_meshes: Option<Res<PartMeshes>>,
    mut highlights: ResMut<Highlights>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    mut transforms: Query<&mut Transform>,
    time: Res<Time>,
) {
    let Some(part_meshes) = part_meshes else { return };
    let tree = engine.tree.borrow();
    let mut wanted: HashMap<InstanceId, (HighlightLook, Vec<InstanceId>)> = HashMap::new();
    // A force field shows as a pale shimmer over its character, drawn like a highlight.
    let shimmer = (time.elapsed_secs() * 5.0).sin() * 0.5 + 0.5;
    for id in tree.descendants_of_class(tree.workspace(), ClassKind::ForceField) {
        if tree.get_bool(id, Property::Visible) == Some(false) {
            continue;
        }
        let Some(target) = tree.parent(id).ok().flatten() else { continue };
        let mut parts = tree.descendants_of_class(target, ClassKind::Part);
        parts.retain(|p| tree.get_number(*p, Property::Transparency).unwrap_or(0.0) < 0.99);
        let look = (
            Vec3::new(0.55, 0.8, 1.0),
            0.62 + shimmer * 0.25,
            Vec3::new(0.75, 0.92, 1.0),
            0.1 + shimmer * 0.5,
        );
        wanted.insert(id, (look, parts));
    }
    for id in tree.descendants_of_class(tree.root(), ClassKind::Highlight) {
        if tree.get_bool(id, Property::Enabled) != Some(true) {
            continue;
        }
        let Some(target) = tree.get_ref(id, Property::Adornee).or_else(|| tree.parent(id).ok().flatten())
        else {
            continue;
        };
        if !tree.is_descendant_of(target, tree.workspace()).unwrap_or(false) {
            continue;
        }
        let mut parts =
            if tree.class(target).is_ok_and(|c| c.is_physical()) { vec![target] } else { Vec::new() };
        parts.extend(tree.descendants_of_class(target, ClassKind::Part));
        parts.retain(|p| tree.get_number(*p, Property::Transparency).unwrap_or(0.0) < 0.99);
        let look = (
            colour(&tree, id, Property::FillColor),
            num(&tree, id, Property::FillTransparency, 0.5),
            colour(&tree, id, Property::OutlineColor),
            num(&tree, id, Property::OutlineTransparency, 0.0),
        );
        wanted.insert(id, (look, parts));
    }
    highlights.0.retain(|id, state| {
        if wanted.contains_key(id) {
            return true;
        }
        for o in state.parts.values() {
            commands.entity(o.fill).despawn();
            commands.entity(o.outline).despawn();
        }
        false
    });
    for (id, (look, parts)) in wanted {
        let state = highlights.0.entry(id).or_insert_with(|| {
            let (fill, outline) = highlight_materials(look);
            HighlightState {
                fill: materials.add(fill),
                outline: materials.add(outline),
                look,
                parts: HashMap::new(),
            }
        });
        if state.look != look {
            let (fill, outline) = highlight_materials(look);
            if let Some(mut m) = materials.get_mut(&state.fill) {
                *m = fill;
            }
            if let Some(mut m) = materials.get_mut(&state.outline) {
                *m = outline;
            }
            state.look = look;
        }
        let keep: HashSet<InstanceId> = parts.iter().copied().collect();
        state.parts.retain(|part, o| {
            if keep.contains(part) {
                return true;
            }
            commands.entity(o.fill).despawn();
            commands.entity(o.outline).despawn();
            false
        });
        for part in parts {
            let (Some(pos), Some(size)) =
                (tree.get_vec3(part, Property::Position), tree.get_vec3(part, Property::Size))
            else {
                continue;
            };
            let rot = crate::physics::orientation_quat(
                tree.get_vec3(part, Property::Orientation).unwrap_or_default(),
            );
            let shape = match tree.get_enum(part, Property::Shape) {
                Some(EnumValue::Shape(s)) => s,
                _ => PartShape::Block,
            };
            let size = to_bevy(size);
            let at = |grow: f32| {
                Transform::from_translation(to_bevy(pos))
                    .with_rotation(rot)
                    .with_scale(size + Vec3::splat(grow))
            };
            match state.parts.get(&part) {
                Some(o) => {
                    if let Ok(mut t) = transforms.get_mut(o.fill) {
                        t.set_if_neq(at(0.04));
                    }
                    if let Ok(mut t) = transforms.get_mut(o.outline) {
                        t.set_if_neq(at(OUTLINE));
                    }
                }
                None => {
                    let mesh = part_meshes.get(shape);
                    let fill = commands
                        .spawn((
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(state.fill.clone()),
                            at(0.04),
                            bevy::light::NotShadowCaster,
                        ))
                        .id();
                    let outline = commands
                        .spawn((
                            Mesh3d(mesh),
                            MeshMaterial3d(state.outline.clone()),
                            at(OUTLINE),
                            bevy::light::NotShadowCaster,
                        ))
                        .id();
                    state.parts.insert(part, Overlay { fill, outline });
                }
            }
        }
    }
}
