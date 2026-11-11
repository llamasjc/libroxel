//! Drawing terrain: one mesh per chunk and material, with only the faces
//! that look out onto air. A chunk is rebuilt when its cells change.

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use libroxel_instance::voxels::Voxels;
use libroxel_instance::{InstanceId, Material};

use crate::view::{EngineState, MaterialTextures, PartAppearance, material_for};

/// Studs per repeat of a material's texture on terrain.
const TEXTURE_STUDS: f32 = 8.0;

/// A chunk's faces for one material.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Faces {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// The six sides of a cell: outward normal and its four corners (counter-clockwise from outside).
const SIDES: [([i32; 3], [[f32; 3]; 4]); 6] = [
    ([1, 0, 0], [[1.0, 0.0, 1.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0]]),
    ([-1, 0, 0], [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0]]),
    ([0, 1, 0], [[0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]),
    ([0, -1, 0], [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]]),
    ([0, 0, 1], [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]]),
    ([0, 0, -1], [[1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]]),
];

/// The faces of one chunk that look out onto air, by material.
pub(crate) fn chunk_faces(voxels: &Voxels, chunk: (i32, i32, i32)) -> HashMap<Material, Faces> {
    let mut out: HashMap<Material, Faces> = HashMap::new();
    for ((x, y, z), material) in voxels.cells_in_chunk(chunk) {
        for (normal, corners) in &SIDES {
            if voxels.is_solid(x + normal[0], y + normal[1], z + normal[2]) {
                continue;
            }
            let faces = out.entry(material).or_default();
            let base = faces.positions.len() as u32;
            for c in corners {
                let p = [x as f32 + c[0], y as f32 + c[1], z as f32 + c[2]];
                faces.positions.push(p);
                faces.normals.push([normal[0] as f32, normal[1] as f32, normal[2] as f32]);
                // Textures run across the world, so neighbouring cells line up.
                let uv = if normal[0] != 0 {
                    [p[2], -p[1]]
                } else if normal[1] != 0 {
                    [p[0], p[2]]
                } else {
                    [p[0], -p[1]]
                };
                faces.uvs.push([uv[0] / TEXTURE_STUDS, uv[1] / TEXTURE_STUDS]);
            }
            faces.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    out
}

fn mesh_of(faces: Faces) -> Mesh {
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, faces.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, faces.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, faces.uvs)
        .with_inserted_indices(Indices::U32(faces.indices))
}

/// The colour each terrain material is drawn in.
pub fn terrain_color(material: Material) -> Color {
    let (r, g, b) = match material {
        Material::Grass => (106, 168, 79),
        Material::Sand => (222, 204, 150),
        Material::Rock => (128, 126, 122),
        Material::Snow => (240, 244, 250),
        Material::Brick => (170, 92, 70),
        Material::Wood => (150, 105, 65),
        Material::Metal => (150, 150, 160),
        Material::Neon => (90, 200, 255),
        Material::Glass => (170, 210, 240),
        _ => (163, 162, 165),
    };
    Color::srgb_u8(r, g, b)
}

/// What's drawn for the terrain now.
#[derive(Default)]
pub(crate) struct TerrainDrawn {
    terrain: Option<InstanceId>,
    version: u64,
    /// Each drawn chunk's fingerprint and its entities.
    chunks: HashMap<(i32, i32, i32), (u64, Vec<Entity>)>,
    materials: HashMap<Material, Handle<StandardMaterial>>,
}

pub(crate) fn sync_terrain(
    engine: NonSend<EngineState>,
    mut commands: Commands,
    textures: Option<Res<MaterialTextures>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut drawn: Local<TerrainDrawn>,
) {
    let Some(textures) = textures else { return };
    let tree = engine.tree.borrow();
    let terrain = tree.terrain();
    let version = terrain.and_then(|t| tree.voxels(t).ok()).map_or(0, |v| v.version());
    if terrain == drawn.terrain && version == drawn.version {
        return;
    }
    if terrain != drawn.terrain {
        for (_, (_, entities)) in drawn.chunks.drain() {
            for e in entities {
                commands.entity(e).despawn();
            }
        }
    }
    drawn.terrain = terrain;
    drawn.version = version;
    let empty = Voxels::default();
    let voxels = terrain.and_then(|t| tree.voxels(t).ok()).unwrap_or(&empty);
    // A chunk is redrawn when it or a neighbour changed (its edge faces depend on them).
    let mut now: HashMap<(i32, i32, i32), u64> =
        voxels.chunks().map(|c| (c, voxels.chunk_fingerprint(c))).collect();
    let changed: HashSet<(i32, i32, i32)> = now
        .iter()
        .filter(|(c, f)| drawn.chunks.get(c).map(|d| d.0) != Some(**f))
        .map(|(c, _)| *c)
        .chain(drawn.chunks.keys().filter(|c| !now.contains_key(c)).copied())
        .collect();
    let mut redraw = HashSet::new();
    for c in &changed {
        for d in [(0, 0, 0), (1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
            redraw.insert((c.0 + d.0, c.1 + d.1, c.2 + d.2));
        }
    }
    for chunk in redraw {
        if let Some((_, entities)) = drawn.chunks.remove(&chunk) {
            for e in entities {
                commands.entity(e).despawn();
            }
        }
        let Some(fingerprint) = now.remove(&chunk) else { continue };
        let mut entities = Vec::new();
        for (material, faces) in chunk_faces(voxels, chunk) {
            let handle = drawn
                .materials
                .entry(material)
                .or_insert_with(|| {
                    let look = PartAppearance {
                        color: terrain_color(material),
                        transparency: 0.0,
                        material,
                        texture_repeat: 1.0,
                    };
                    materials.add(material_for(&look, &textures))
                })
                .clone();
            entities.push(
                commands
                    .spawn((Mesh3d(meshes.add(mesh_of(faces))), MeshMaterial3d(handle), Transform::IDENTITY))
                    .id(),
            );
        }
        drawn.chunks.insert(chunk, (fingerprint, entities));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libroxel_instance::Vec3;

    #[test]
    fn only_faces_onto_air_are_drawn() {
        let mut v = Voxels::new();
        let axes = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)];
        // A 2 x 1 x 1 bar: 10 outside faces, the two touching ones hidden.
        v.fill_block(Vec3::new(1.0, 0.5, 0.5), axes, Vec3::new(2.0, 1.0, 1.0), Some(Material::Rock));
        let faces = chunk_faces(&v, (0, 0, 0));
        let rock = &faces[&Material::Rock];
        assert_eq!(rock.positions.len(), 10 * 4);
        assert_eq!(rock.indices.len(), 10 * 6);
        // Each face winds counter-clockwise seen from outside.
        for quad in rock.indices.chunks(6) {
            let p = |i: u32| bevy::math::Vec3::from_array(rock.positions[i as usize]);
            let n = bevy::math::Vec3::from_array(rock.normals[quad[0] as usize]);
            let (a, b, c) = (p(quad[0]), p(quad[1]), p(quad[2]));
            assert!((b - a).cross(c - a).dot(n) > 0.0);
        }
    }

    #[test]
    fn faces_between_chunks_are_hidden() {
        let mut v = Voxels::new();
        let axes = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)];
        v.fill_block(Vec3::new(16.0, 0.5, 0.5), axes, Vec3::new(2.0, 1.0, 1.0), Some(Material::Grass));
        assert_eq!(v.chunks().count(), 2);
        let left = chunk_faces(&v, (0, 0, 0));
        assert_eq!(left[&Material::Grass].positions.len(), 5 * 4);
    }
}
