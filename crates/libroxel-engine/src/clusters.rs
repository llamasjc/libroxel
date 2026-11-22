//! Parts that keep still are drawn merged, like Roblox's clusters.
//!
//! A big map has tens of thousands of parts, and every one drawn on its own
//! costs the CPU and the graphics driver a little each frame. Roblox merges
//! parts that don't move into one mesh per area; this does the same: once a
//! part has kept still for a while, it joins a merged mesh with the other
//! still parts near it of the same surface material and see-through-ness, whatever their colour
//! or size (the colour goes on each corner of the mesh and the texture's
//! scale into its texture coordinates), and its own entity is hidden. When
//! any part of a cluster moves or changes, the cluster breaks up at once
//! (its parts draw on their own again) and forms again once they are still.

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use libroxel_instance::Material;

use crate::view::{
    MaterialTextures, PartAppearance, PartLink, PartMeshFile, PartMeshTexture, material_for, visibility_for,
};

type FastMap<K, V> = bevy::platform::collections::HashMap<K, V>;

/// The side of the cube a cluster covers, in studs.
const CELL: f32 = 64.0;
/// Frames a part keeps still before it joins a cluster.
const CALM_FRAMES: u32 = 90;
/// How often still parts are gathered into clusters, in frames.
const CHECK_EVERY: u32 = 15;
/// Parts whose mesh has more corners than this keep their own (shared) mesh.
const MAX_VERTICES: usize = 200;
/// A group needs this many parts to be worth merging.
const MIN_MEMBERS: usize = 3;
/// At most this many parts are merged in one check, so a big map clusters
/// over a few frames instead of in one long one.
const BUDGET: usize = 6000;
/// Frames a new cluster is drawn before its parts (or the cluster it
/// replaces) are hidden. The GPU needs a few frames before a new mesh
/// draws, and hiding the parts at once made whole areas blink out.
const SWAP_FRAMES: u32 = 8;

static COUNTS: [std::sync::atomic::AtomicUsize; 3] = [const { std::sync::atomic::AtomicUsize::new(0) }; 3];

/// Parts in the view, parts merged, clusters.
pub(crate) fn counts() -> (usize, usize, usize) {
    let get = |i: usize| COUNTS[i].load(std::sync::atomic::Ordering::Relaxed);
    (get(0), get(1), get(2))
}

/// A surface: the material, how shiny (rounded), whether it casts a
/// shadow, and how see-through it is (rounded).
type Surface = (Material, u16, bool, u16);
type Key = (IVec3, Surface);

fn surface(look: &PartAppearance) -> Option<Surface> {
    // Parts whose glow or look depends on their colour keep their own material.
    let own = matches!(look.material, Material::CrackedLava | Material::ForceField | Material::Glass);
    let round = |x: f32| (x.clamp(0.0, 1.0) * 100.0).round() as u16;
    (!own).then(|| (look.material, round(look.reflectance), look.shadow, round(look.transparency)))
}

/// A merged mesh standing in for its member parts.
#[derive(Component)]
pub struct Cluster {
    key: Key,
    members: Vec<Entity>,
}

#[derive(Resource, Default)]
pub(crate) struct Clusters {
    frame: u32,
    /// The frame each part last changed.
    calm_since: FastMap<Entity, u32>,
    /// How many times each part broke its cluster (restless parts wait longer).
    restless: FastMap<Entity, u32>,
    member_of: FastMap<Entity, Entity>,
    by_key: FastMap<Key, Entity>,
    /// A white material for each surface; the parts' colours are on the mesh.
    materials: FastMap<Surface, Handle<StandardMaterial>>,
    /// Parts whose visibility something else looks after.
    kept_apart: Vec<Entity>,
    /// New clusters still showing alongside their parts, with the cluster
    /// each replaces and the frame they take over.
    swaps: Vec<Swap>,
}

struct Swap {
    cluster: Entity,
    old: Option<Entity>,
    at: u32,
}

impl Clusters {
    /// Keeps a part out of clusters for a good while: something else shows
    /// and hides it.
    pub(crate) fn keep_apart(&mut self, part: Entity) {
        self.kept_apart.push(part);
    }
}

/// Any change to how a part is drawn.
type Changes = Or<(
    Changed<Transform>,
    Changed<Mesh3d>,
    Changed<MeshMaterial3d<StandardMaterial>>,
    Changed<PartAppearance>,
    Changed<PartMeshFile>,
    Changed<PartMeshTexture>,
)>;

type PartView<'a> = (
    Entity,
    &'a Transform,
    &'a Mesh3d,
    &'a MeshMaterial3d<StandardMaterial>,
    &'a PartAppearance,
    &'a PartMeshFile,
    &'a PartMeshTexture,
    &'a mut Visibility,
);

#[allow(clippy::too_many_arguments)]
pub(crate) fn cluster_parts(
    mut commands: Commands,
    mut state: ResMut<Clusters>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    textures: Res<MaterialTextures>,
    mut removed: RemovedComponents<PartLink>,
    mut parts: Query<PartView, With<PartLink>>,
    moved: Query<Entity, (With<PartLink>, Changes)>,
    clusters: Query<&Cluster>,
) {
    let _timed = crate::view::TimedSync(std::time::Instant::now());
    let state = &mut *state;
    state.frame = state.frame.wrapping_add(1);
    let frame = state.frame;
    let mut broken: Vec<Entity> = Vec::new();
    for gone in removed.read() {
        state.calm_since.remove(&gone);
        state.restless.remove(&gone);
        if let Some(cluster) = state.member_of.remove(&gone) {
            broken.push(cluster);
        }
    }
    for entity in &moved {
        state.calm_since.insert(entity, frame);
        if let Some(cluster) = state.member_of.get(&entity) {
            broken.push(*cluster);
            *state.restless.entry(entity).or_default() += 1;
        }
    }
    for entity in std::mem::take(&mut state.kept_apart) {
        // Shown or hidden by something else (first person): it draws on its own.
        state.calm_since.insert(entity, frame);
        *state.restless.entry(entity).or_default() += 10;
        if let Some(cluster) = state.member_of.get(&entity) {
            broken.push(*cluster);
        }
    }
    broken.sort_unstable();
    broken.dedup();
    for cluster in broken {
        dissolve(&mut commands, state, &mut parts, &clusters, cluster);
    }

    let due: Vec<Swap> = {
        let (due, waiting) = std::mem::take(&mut state.swaps).into_iter().partition(|s| frame >= s.at);
        state.swaps = waiting;
        due
    };
    for swap in due {
        if let Ok(cluster) = clusters.get(swap.cluster) {
            for member in &cluster.members {
                if state.member_of.get(member) == Some(&swap.cluster)
                    && let Ok(mut item) = parts.get_mut(*member)
                {
                    *item.7 = Visibility::Hidden;
                }
            }
        }
        if let Some(old) = swap.old {
            commands.entity(old).despawn();
        }
    }

    if !frame.is_multiple_of(CHECK_EVERY) {
        return;
    }
    let mut groups: FastMap<Key, Vec<Entity>> = FastMap::default();
    for (entity, transform, mesh, _, look, file, texture, visibility) in &parts {
        if state.member_of.contains_key(&entity)
            || *visibility == Visibility::Hidden
            || !file.0.is_empty()
            || !texture.0.is_empty()
        {
            continue;
        }
        let Some(surface) = surface(look) else {
            continue;
        };
        let since = state.calm_since.get(&entity).copied().unwrap_or(0);
        let wait = CALM_FRAMES * (1 + 4 * state.restless.get(&entity).copied().unwrap_or(0).min(10));
        if frame.wrapping_sub(since) < wait {
            continue;
        }
        if !meshes.get(&mesh.0).is_some_and(|m| m.count_vertices() <= MAX_VERTICES) {
            continue;
        }
        let cell = (transform.translation / CELL).floor().as_ivec3();
        groups.entry((cell, surface)).or_default().push(entity);
    }
    let mut done = 0;
    for (key, mut members) in groups {
        if done >= BUDGET {
            break;
        }
        // Parts already merged here are merged again with the newcomers;
        // the old cluster stays until the new one takes over.
        let old = state.by_key.get(&key).copied();
        if let Some(old) = old {
            let Ok(cluster) = clusters.get(old) else {
                continue;
            };
            if state.swaps.iter().any(|s| s.cluster == old) {
                continue;
            }
            members.extend(cluster.members.iter().copied());
        } else if members.len() < MIN_MEMBERS {
            continue;
        }
        done += members.len();
        let Some((mesh, aabb)) = merge(&meshes, &parts, &members) else {
            continue;
        };
        let surface = key.1;
        let material = state
            .materials
            .entry(surface)
            .or_insert_with(|| {
                let white = PartAppearance {
                    color: Color::WHITE,
                    transparency: f32::from(surface.3) / 100.0,
                    material: surface.0,
                    texture_repeat: 1.0,
                    reflectance: f32::from(surface.1) / 100.0,
                    shadow: surface.2,
                };
                let mut material = material_for(&white, &textures);
                // Neon is unlit in its own colour, which is on the mesh; a
                // white glow would wash it out.
                if surface.0 == Material::Neon {
                    material.emissive = LinearRgba::BLACK;
                }
                materials.add(material)
            })
            .clone();
        let mut cluster = commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            Visibility::Visible,
            aabb,
        ));
        if !surface.2 {
            cluster.insert(bevy::light::NotShadowCaster);
        }
        let cluster = cluster.id();
        for member in &members {
            state.member_of.insert(*member, cluster);
        }
        commands.entity(cluster).insert(Cluster { key, members });
        state.by_key.insert(key, cluster);
        state.swaps.push(Swap { cluster, old, at: frame + SWAP_FRAMES });
    }
    use std::sync::atomic::Ordering::Relaxed;
    COUNTS[0].store(parts.iter().count(), Relaxed);
    COUNTS[1].store(state.member_of.len(), Relaxed);
    COUNTS[2].store(state.by_key.len(), Relaxed);
}

/// Breaks a cluster up: its parts draw on their own again.
fn dissolve(
    commands: &mut Commands,
    state: &mut Clusters,
    parts: &mut Query<PartView, With<PartLink>>,
    clusters: &Query<&Cluster>,
    cluster: Entity,
) {
    let Ok(found) = clusters.get(cluster) else {
        return;
    };
    if let Some(at) = state.swaps.iter().position(|s| s.cluster == cluster)
        && let Some(old) = state.swaps.remove(at).old
    {
        commands.entity(old).despawn();
    }
    if state.by_key.get(&found.key) == Some(&cluster) {
        state.by_key.remove(&found.key);
    }
    for member in &found.members {
        if state.member_of.get(member) == Some(&cluster) {
            state.member_of.remove(member);
        }
        if let Ok(mut item) = parts.get_mut(*member)
            && *item.7 == Visibility::Hidden
        {
            *item.7 = visibility_for(item.4);
        }
    }
    commands.entity(cluster).despawn();
}

/// One mesh of all the members, placed where they are, each corner in its
/// part's colour.
fn merge(
    meshes: &Assets<Mesh>,
    parts: &Query<PartView, With<PartLink>>,
    members: &[Entity],
) -> Option<(Mesh, Aabb)> {
    let mut positions = Vec::<[f32; 3]>::new();
    let mut normals = Vec::<[f32; 3]>::new();
    let mut uvs = Vec::<[f32; 2]>::new();
    let mut colours = Vec::<[f32; 4]>::new();
    let mut indices = Vec::<u32>::new();
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for member in members {
        let Ok((_, transform, mesh, _, look, ..)) = parts.get(*member) else {
            continue;
        };
        let Some(mesh) = meshes.get(&mesh.0) else {
            continue;
        };
        let Some(VertexAttributeValues::Float32x3(points)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            continue;
        };
        let affine = transform.compute_affine();
        let turn = Mat3::from(affine.matrix3).inverse().transpose();
        let base = positions.len() as u32;
        for p in points {
            let p = affine.transform_point3(Vec3::from_array(*p));
            lo = lo.min(p);
            hi = hi.max(p);
            positions.push(p.to_array());
        }
        match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(VertexAttributeValues::Float32x3(n)) if n.len() == points.len() => {
                normals
                    .extend(n.iter().map(|n| (turn * Vec3::from_array(*n)).normalize_or_zero().to_array()));
            }
            _ => normals.extend(std::iter::repeat_n([0.0, 1.0, 0.0], points.len())),
        }
        let repeat = look.texture_repeat;
        match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
            Some(VertexAttributeValues::Float32x2(uv)) if uv.len() == points.len() => {
                uvs.extend(uv.iter().map(|[u, v]| [u * repeat, v * repeat]));
            }
            _ => uvs.extend(std::iter::repeat_n([0.0, 0.0], points.len())),
        }
        colours.extend(std::iter::repeat_n(look.color.to_linear().to_f32_array(), points.len()));
        match mesh.indices() {
            Some(list) => indices.extend(list.iter().map(|i| base + i as u32)),
            None => indices.extend(base..base + points.len() as u32),
        }
    }
    if positions.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
    mesh.insert_indices(Indices::U32(indices));
    Some((mesh, Aabb::from_min_max(lo, hi)))
}
