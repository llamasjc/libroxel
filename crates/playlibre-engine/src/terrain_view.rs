//! Drawing terrain: one mesh per chunk and material. Solid ground is drawn
//! smooth (a surface threaded between the cells' middles, rounded off over
//! their neighbours), so hills slope and balls of ground look round; water
//! is drawn as the faces of its cells that look out onto air. A chunk is
//! rebuilt when its cells change.

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use playlibre_instance::voxels::{CHUNK, Voxels};
use playlibre_instance::{InstanceId, Material};

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

/// How far round a cell's neighbours the ground is smoothed: cells sampled
/// either side of a chunk, beyond the cells it draws.
const PAD: i32 = 3;
const SPAN: i32 = CHUNK + 2 * PAD;

/// A chunk's solid cells and their neighbours, and how full of ground each
/// place is once smoothed over its neighbours (over a half for solid cells,
/// under for the rest).
struct Field {
    origin: (i32, i32, i32),
    solid: Vec<Option<Material>>,
    density: Vec<f32>,
}

impl Field {
    fn new(voxels: &Voxels, chunk: (i32, i32, i32)) -> Self {
        let origin = (chunk.0 * CHUNK - PAD, chunk.1 * CHUNK - PAD, chunk.2 * CHUNK - PAD);
        let n = (SPAN * SPAN * SPAN) as usize;
        let mut solid = vec![None; n];
        for y in 0..SPAN {
            for z in 0..SPAN {
                for x in 0..SPAN {
                    let m = voxels.get(origin.0 + x, origin.1 + y, origin.2 + z);
                    solid[Self::at(x, y, z)] = m.filter(|m| *m != Material::Water);
                }
            }
        }
        let mut density = vec![0.0; n];
        for y in 1..SPAN - 1 {
            for z in 1..SPAN - 1 {
                for x in 1..SPAN - 1 {
                    let mut around = 0.0;
                    for (dx, dy, dz) in NEIGHBOURS {
                        if solid[Self::at(x + dx, y + dy, z + dz)].is_some() {
                            around += 1.0;
                        }
                    }
                    let own = if solid[Self::at(x, y, z)].is_some() { 1.0 } else { 0.0 };
                    density[Self::at(x, y, z)] = 0.5 * own + 0.5 * around / NEIGHBOURS.len() as f32;
                }
            }
        }
        Field { origin, solid, density }
    }

    fn at(x: i32, y: i32, z: i32) -> usize {
        ((y * SPAN + z) * SPAN + x) as usize
    }

    /// Local coordinates of a world cell, if it's sampled.
    fn local(&self, c: (i32, i32, i32)) -> Option<(i32, i32, i32)> {
        let l = (c.0 - self.origin.0, c.1 - self.origin.1, c.2 - self.origin.2);
        let inside = |v: i32| (1..SPAN - 1).contains(&v);
        (inside(l.0) && inside(l.1) && inside(l.2)).then_some(l)
    }

    fn solid(&self, c: (i32, i32, i32)) -> Option<Material> {
        let l = self.local(c)?;
        self.solid[Self::at(l.0, l.1, l.2)]
    }

    fn density(&self, c: (i32, i32, i32)) -> f32 {
        self.local(c).map_or(0.0, |l| self.density[Self::at(l.0, l.1, l.2)])
    }

    /// The surface's point in the box between the middles of cell `c` and
    /// its seven neighbours above, and the surface's outward direction there.
    fn vertex(&self, c: (i32, i32, i32)) -> Point {
        let corner =
            |i: usize| (c.0 + (i & 1) as i32, c.1 + ((i >> 1) & 1) as i32, c.2 + ((i >> 2) & 1) as i32);
        let d: [f32; 8] = std::array::from_fn(|i| self.density(corner(i)));
        let full: [bool; 8] = std::array::from_fn(|i| self.solid(corner(i)).is_some());
        let mut sum = [0.0f32; 3];
        let mut count = 0.0;
        for (a, b) in BOX_EDGES {
            if full[a] == full[b] {
                continue;
            }
            // Where the smoothed fullness crosses a half along this edge.
            let t = if (d[b] - d[a]).abs() > 1e-6 {
                ((0.5 - d[a]) / (d[b] - d[a])).clamp(0.05, 0.95)
            } else {
                0.5
            };
            for (k, s) in sum.iter_mut().enumerate() {
                let (pa, pb) = (((a >> k) & 1) as f32, ((b >> k) & 1) as f32);
                *s += pa + (pb - pa) * t;
            }
            count += 1.0;
        }
        let local = if count > 0.0 { sum.map(|s| s / count) } else { [0.5; 3] };
        // Cell middles are at whole numbers plus a half.
        let pos = [c.0 as f32 + 0.5 + local[0], c.1 as f32 + 0.5 + local[1], c.2 as f32 + 0.5 + local[2]];
        // Outward: from fuller towards emptier.
        let mut g = [0.0f32; 3];
        for (i, v) in d.iter().enumerate() {
            for (k, gk) in g.iter_mut().enumerate() {
                *gk += if (i >> k) & 1 == 1 { -v } else { *v };
            }
        }
        let len = (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt();
        let normal = if len > 1e-6 { g.map(|v| v / len) } else { [0.0, 1.0, 0.0] };
        (pos, normal)
    }
}

/// A point on the surface, and the surface's outward direction there.
type Point = ([f32; 3], [f32; 3]);

/// The 26 cells round a cell.
const NEIGHBOURS: [(i32, i32, i32); 26] = {
    let mut out = [(0, 0, 0); 26];
    let mut i = 0;
    let mut n = 0;
    while n < 27 {
        let (x, y, z) = (n % 3 - 1, (n / 3) % 3 - 1, n / 9 - 1);
        if x != 0 || y != 0 || z != 0 {
            out[i] = (x, y, z);
            i += 1;
        }
        n += 1;
    }
    out
};

/// The twelve edges of a box, by corner (bit 0 is X, 1 is Y, 2 is Z).
const BOX_EDGES: [(usize, usize); 12] =
    [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];

/// Where a texture lands on a point of terrain facing `normal`: projected
/// along whichever axis it faces most, so neighbouring cells line up.
fn terrain_uv(p: [f32; 3], normal: [f32; 3]) -> [f32; 2] {
    let (ax, ay, az) = (normal[0].abs(), normal[1].abs(), normal[2].abs());
    let uv = if ax >= ay && ax >= az {
        [p[2], -p[1]]
    } else if ay >= az {
        [p[0], p[2]]
    } else {
        [p[0], -p[1]]
    };
    [uv[0] / TEXTURE_STUDS, uv[1] / TEXTURE_STUDS]
}

/// One chunk's smooth ground, by material: a face across every pair of
/// neighbouring cells where ground meets air or water, joining the
/// surface's points in the four boxes round them.
pub(crate) fn smooth_faces(voxels: &Voxels, chunk: (i32, i32, i32)) -> HashMap<Material, Faces> {
    let field = Field::new(voxels, chunk);
    let mut out: HashMap<Material, Faces> = HashMap::new();
    let mut points: HashMap<(i32, i32, i32), Point> = HashMap::new();
    let start = (chunk.0 * CHUNK, chunk.1 * CHUNK, chunk.2 * CHUNK);
    for y in 0..CHUNK {
        for z in 0..CHUNK {
            for x in 0..CHUNK {
                let a = (start.0 + x, start.1 + y, start.2 + z);
                for axis in 0..3 {
                    let step = |c: (i32, i32, i32), k: usize, by: i32| match k {
                        0 => (c.0 + by, c.1, c.2),
                        1 => (c.0, c.1 + by, c.2),
                        _ => (c.0, c.1, c.2 + by),
                    };
                    let b = step(a, axis, 1);
                    let (ma, mb) = (field.solid(a), field.solid(b));
                    // Ground on one side only. A pair is drawn by the chunk
                    // holding its lower cell.
                    let (material, outward) = match (ma, mb) {
                        (Some(m), None) => (m, 1.0),
                        (None, Some(m)) => (m, -1.0),
                        _ => continue,
                    };
                    // The four boxes round the edge between a and b.
                    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                    let boxes =
                        [(0, 0), (1, 0), (1, 1), (0, 1)].map(|(du, dv)| step(step(a, u, -du), v, -dv));
                    let quad = boxes.map(|c| *points.entry(c).or_insert_with(|| field.vertex(c)));
                    let faces = out.entry(material).or_default();
                    let base = faces.positions.len() as u32;
                    for (pos, normal) in quad {
                        faces.positions.push(pos);
                        faces.normals.push(normal);
                        faces.uvs.push(terrain_uv(pos, normal));
                    }
                    // Wind it to face out of the ground.
                    let p = |i: usize| bevy::math::Vec3::from_array(quad[i].0);
                    let facing = (p(1) - p(0)).cross(p(2) - p(0))[axis] * outward;
                    if facing >= 0.0 {
                        faces.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
                    } else {
                        faces.indices.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
                    }
                }
            }
        }
    }
    out
}

/// The faces of one chunk that look out onto air, by material.
pub(crate) fn chunk_faces(voxels: &Voxels, chunk: (i32, i32, i32)) -> HashMap<Material, Faces> {
    let mut out: HashMap<Material, Faces> = HashMap::new();
    for ((x, y, z), material) in voxels.cells_in_chunk(chunk) {
        for (normal, corners) in &SIDES {
            let (nx, ny, nz) = (x + normal[0], y + normal[1], z + normal[2]);
            // Solid cells show through water; water only shows against air.
            let hidden = if material == Material::Water {
                voxels.get(nx, ny, nz).is_some()
            } else {
                voxels.is_solid(nx, ny, nz)
            };
            if hidden {
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

/// A chunk's mesh, packed small and kept only on the graphics side (it's
/// made again from the cells when they change).
fn mesh_of(faces: Faces) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, faces.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, faces.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, faces.uvs)
        .with_inserted_indices(Indices::U32(faces.indices));
    crate::view::pack_mesh(&mut mesh);
    mesh
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
        Material::Water => (50, 130, 210),
        Material::Slate => (63, 127, 107),
        Material::Concrete => (127, 102, 63),
        Material::Cobblestone => (132, 123, 90),
        Material::Ice => (129, 194, 224),
        Material::WoodPlanks => (139, 109, 79),
        Material::Mud => (58, 46, 36),
        Material::Ground => (102, 92, 59),
        Material::Basalt => (30, 30, 37),
        Material::Asphalt => (115, 123, 107),
        Material::LeafyGrass => (115, 132, 74),
        Material::Salt => (198, 189, 181),
        Material::Limestone => (206, 173, 148),
        Material::Pavement => (148, 148, 140),
        Material::Sandstone => (137, 90, 71),
        Material::Glacier => (101, 176, 234),
        Material::CrackedLava => (232, 156, 74),
        _ => (163, 162, 165),
    };
    Color::srgb_u8(r, g, b)
}

/// What's drawn for the terrain now.
#[derive(Default)]
pub(crate) struct TerrainDrawn {
    terrain: Option<InstanceId>,
    version: u64,
    /// The terrain's revision when its material colours were last applied.
    revision: u64,
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
    let revision = terrain.and_then(|t| tree.revision(t).ok()).unwrap_or(0);
    // A place (or a script) can change what colour each material is.
    let colour =
        |m: Material| match terrain.and_then(|t| tree.get_extra(t, &format!("MaterialColor.{m:?}")).ok()) {
            Some(Some(playlibre_instance::PropertyValue::Color3(c))) => Color::srgb(c.r, c.g, c.b),
            _ => terrain_color(m),
        };
    if revision != drawn.revision {
        drawn.revision = revision;
        for (m, handle) in &drawn.materials {
            if let Some(mut mat) = materials.get_mut(handle) {
                let alpha = mat.base_color.alpha();
                mat.base_color = colour(*m).with_alpha(alpha);
            }
        }
    }
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
        let mut faces = smooth_faces(voxels, chunk);
        if let Some(water) = chunk_faces(voxels, chunk).remove(&Material::Water) {
            faces.insert(Material::Water, water);
        }
        for (material, faces) in faces {
            let handle = drawn
                .materials
                .entry(material)
                .or_insert_with(|| {
                    let look = PartAppearance {
                        color: colour(material),
                        transparency: if material == Material::Water { 0.45 } else { 0.0 },
                        material,
                        texture_repeat: 1.0,
                        reflectance: 0.0,
                        shadow: true,
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
    use playlibre_instance::Vec3;

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
    fn smooth_ground_is_closed_round_and_faces_out() {
        let mut v = Voxels::new();
        v.fill_ball(Vec3::new(8.0, 8.0, 8.0), 5.0, Some(Material::Rock));
        let faces = smooth_faces(&v, (0, 0, 0));
        let rock = &faces[&Material::Rock];
        assert!(!rock.indices.is_empty());
        let centre = bevy::math::Vec3::new(8.0, 8.0, 8.0);
        let mut far: f32 = 0.0;
        let mut near = f32::MAX;
        for tri in rock.indices.chunks(3) {
            let p = |i: u32| bevy::math::Vec3::from_array(rock.positions[i as usize]);
            let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
            let mid = (a + b + c) / 3.0;
            // Every face looks away from the middle...
            assert!((b - a).cross(c - a).dot(mid - centre) > 0.0, "{a} {b} {c}");
            far = far.max(mid.distance(centre));
            near = near.min(mid.distance(centre));
        }
        // ...and the ball comes out round, not cubed.
        assert!(far - near < 1.2, "{near}..{far}");
        for n in &rock.normals {
            assert!((bevy::math::Vec3::from_array(*n).length() - 1.0).abs() < 1e-3);
        }
    }

    #[test]
    fn smooth_ground_has_no_gap_between_chunks() {
        let mut v = Voxels::new();
        let axes = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)];
        v.fill_block(Vec3::new(16.0, 2.0, 8.0), axes, Vec3::new(20.0, 4.0, 10.0), Some(Material::Grass));
        // Every edge of the surface is shared by exactly two faces, across
        // both chunks together.
        let mut edges: HashMap<([i32; 3], [i32; 3]), i32> = HashMap::new();
        let key = |p: [f32; 3]| p.map(|v| (v * 1000.0).round() as i32);
        for chunk in [(0, 0, 0), (1, 0, 0), (-1, 0, 0), (0, -1, 0), (1, -1, 0), (0, 0, -1), (1, 0, -1)] {
            let Some(grass) = smooth_faces(&v, chunk).remove(&Material::Grass) else { continue };
            for tri in grass.indices.chunks(3) {
                for i in 0..3 {
                    let (a, b) = (
                        key(grass.positions[tri[i] as usize]),
                        key(grass.positions[tri[(i + 1) % 3] as usize]),
                    );
                    let e = if a < b { (a, b) } else { (b, a) };
                    *edges.entry(e).or_default() += 1;
                }
            }
        }
        assert!(!edges.is_empty());
        assert!(edges.values().all(|n| *n == 2), "{:?}", edges.values().filter(|n| **n != 2).count());
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
