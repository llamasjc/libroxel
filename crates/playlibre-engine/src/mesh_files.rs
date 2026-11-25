//! 3D models from files, for parts with a `MeshId`: Wavefront .obj files
//! and the reference editor's .mesh files, found the same way as pictures (see [`pictures`](crate::pictures)). A
//! model is stretched to fit its part's Size, as a box would be; it still
//! bumps into things as that box.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// A model read from a file: triangles, with a normal and a texture
/// position at each corner, fitted into a 1 by 1 by 1 box round the origin.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// How big the model was in its file, in studs (its natural size).
    pub size: Vec3,
}

fn index(token: Option<&str>, len: usize) -> Option<usize> {
    let n: i64 = token.filter(|t| !t.is_empty())?.parse().ok()?;
    let i = if n < 0 { len as i64 + n } else { n - 1 };
    (0..len as i64).contains(&i).then_some(i as usize)
}

/// Reads an .obj file's text. Faces with more than three corners are split
/// into triangles; corners without normals get their face's.
pub fn parse_obj(text: &str) -> Option<Model> {
    let (mut vs, mut vts, mut vns) = (Vec::new(), Vec::new(), Vec::new());
    let mut corners: Vec<(usize, Option<usize>, Option<usize>)> = Vec::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let numbers = |words: std::str::SplitWhitespace| -> Vec<f32> {
            words.filter_map(|w| w.parse::<f32>().ok()).collect()
        };
        match words.next() {
            Some("v") => {
                let n = numbers(words);
                vs.push(Vec3::new(*n.first()?, *n.get(1)?, *n.get(2)?));
            }
            Some("vt") => {
                let n = numbers(words);
                vts.push(Vec2::new(*n.first()?, n.get(1).copied().unwrap_or(0.0)));
            }
            Some("vn") => {
                let n = numbers(words);
                vns.push(Vec3::new(*n.first()?, *n.get(1)?, *n.get(2)?));
            }
            Some("f") => {
                let face: Vec<_> = words
                    .filter_map(|w| {
                        let mut parts = w.split('/');
                        let v = index(parts.next(), vs.len())?;
                        Some((v, index(parts.next(), vts.len()), index(parts.next(), vns.len())))
                    })
                    .collect();
                for i in 1..face.len().saturating_sub(1) {
                    corners.extend([face[0], face[i], face[i + 1]]);
                }
            }
            _ => {}
        }
    }
    let soup = corners
        .iter()
        .map(|&(v, t, n)| (vs[v], t.map(|t| Vec2::new(vts[t].x, 1.0 - vts[t].y)), n.map(|n| vns[n])))
        .collect();
    fit(soup)
}

/// A corner of a triangle: where it is, its texture position (rows from
/// the top) and its normal, if the file gave them.
type Corner = (Vec3, Option<Vec2>, Option<Vec3>);

/// Triangles (three corners each) fitted into a 1 by 1 by 1 box.
fn fit(corners: Vec<Corner>) -> Option<Model> {
    if corners.len() < 3 {
        return None;
    }
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for (v, ..) in &corners {
        lo = lo.min(*v);
        hi = hi.max(*v);
    }
    if !lo.is_finite() || !hi.is_finite() {
        return None;
    }
    let size = hi - lo;
    let centre = (lo + hi) / 2.0;
    // A flat side stays flat rather than becoming infinitely thin maths.
    let scale = Vec3::new(1.0 / size.x.max(1e-4), 1.0 / size.y.max(1e-4), 1.0 / size.z.max(1e-4));
    let mut model = Model { positions: Vec::new(), normals: Vec::new(), uvs: Vec::new(), size };
    for tri in corners.as_chunks::<3>().0 {
        let p: Vec<Vec3> = tri.iter().map(|c| (c.0 - centre) * scale).collect();
        let flat = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or(Vec3::Y);
        for (corner, at) in tri.iter().zip(p) {
            model.positions.push(at.to_array());
            // Stretching to fit bends normals the other way.
            let n = corner.2.map_or(flat, |n| (n / scale).normalize_or(flat));
            model.normals.push(n.to_array());
            model.uvs.push(corner.1.unwrap_or(Vec2::ZERO).to_array());
        }
    }
    Some(model)
}

/// Reads a mesh file in the format the reference editor saves and serves meshes
/// in (versions 1 to 5), using its most detailed level.
pub fn parse_asset_mesh(bytes: &[u8]) -> Option<Model> {
    use rbx_mesh::mesh::{Mesh, Vertices2};
    let mesh = rbx_mesh::read_mesh_versioned(std::io::Cursor::new(bytes)).ok()?;
    let corner = |pos: [f32; 3], tex: [f32; 2], norm: [f32; 3]| -> Corner {
        (Vec3::from_array(pos), Some(Vec2::from_array(tex)), Some(Vec3::from_array(norm)))
    };
    let pick = |vertices: Vec<Corner>, faces: &[rbx_mesh::mesh::Face2], lods: &[rbx_mesh::mesh::Lod3]| {
        // The first level of detail is the full one.
        let end = match lods {
            [_, next, ..] if (next.0 as usize) <= faces.len() && next.0 > 0 => next.0 as usize,
            _ => faces.len(),
        };
        let mut soup = Vec::with_capacity(end * 3);
        for face in &faces[..end] {
            let ids = [face.0[0].0, face.0[1].0, face.0[2].0];
            if ids.iter().all(|&i| (i as usize) < vertices.len()) {
                soup.extend(ids.map(|i| vertices[i as usize]));
            }
        }
        soup
    };
    let from_v2 = |v: &Vertices2| -> Vec<Corner> {
        match v {
            Vertices2::Full(v) => v.iter().map(|v| corner(v.pos, v.tex, v.norm)).collect(),
            Vertices2::Truncated(v) => v.iter().map(|v| corner(v.pos, v.tex, v.norm)).collect(),
        }
    };
    let soup = match mesh {
        Mesh::V1(m) => m.vertices.iter().map(|v| corner(v.pos, [v.tex[0], v.tex[1]], v.norm)).collect(),
        Mesh::V2(m) => pick(from_v2(&m.vertices), &m.faces, &[]),
        Mesh::V3(m) => pick(from_v2(&m.vertices), &m.faces, &m.lods),
        Mesh::V4(m) => {
            let v = m.vertices.iter().map(|v| corner(v.pos, v.tex, v.norm)).collect();
            pick(v, &m.faces, &m.lods)
        }
        Mesh::V5(m) => {
            let v = m.vertices.iter().map(|v| corner(v.pos, v.tex, v.norm)).collect();
            pick(v, &m.faces, &m.lods)
        }
    };
    fit(soup)
}

/// Reads a union's shape as the reference platform serves it for a union's asset.
pub fn parse_union(bytes: &[u8]) -> Option<Model> {
    use rbx_mesh::union_graphics::UnionGraphics;
    let corner = |pos: [f32; 3], tex: [f32; 2], norm: [f32; 3]| -> Corner {
        (Vec3::from_array(pos), Some(Vec2::from_array(tex)), Some(Vec3::from_array(norm)))
    };
    let mut soup = Vec::new();
    match rbx_mesh::read_union_graphics_versioned(std::io::Cursor::new(bytes)).ok()? {
        UnionGraphics::CSGK(_) => return None,
        UnionGraphics::V2(m) => {
            let v = &m.mesh.vertices;
            for f in &m.mesh.faces {
                let ids = [f[0].0, f[1].0, f[2].0].map(|i| i as usize);
                if ids.iter().all(|&i| i < v.len()) {
                    soup.extend(ids.map(|i| corner(v[i].pos, v[i].tex, v[i].norm)));
                }
            }
        }
        UnionGraphics::V4(m) => {
            let v = &m.mesh.vertices;
            for f in &m.mesh.faces {
                let ids = [f[0].0, f[1].0, f[2].0].map(|i| i as usize);
                if ids.iter().all(|&i| i < v.len()) {
                    soup.extend(ids.map(|i| corner(v[i].pos, v[i].tex, v[i].norm)));
                }
            }
        }
        UnionGraphics::V5(m) => {
            let n = m.positions.len();
            for f in m.faces.indices.as_chunks::<3>().0 {
                let ids = f.map(|i| i as usize);
                if ids.iter().all(|&i| i < n) {
                    soup.extend(ids.map(|i| {
                        (
                            Vec3::from_array(m.positions[i]),
                            m.tex.get(i).map(|t| Vec2::from_array(*t)),
                            m.normals.get(i).map(|n| Vec3::from_array(n.0)),
                        )
                    }));
                }
            }
        }
    }
    fit(soup)
}

/// Reads a model from a file's bytes, whichever kind it is.
pub fn parse(bytes: &[u8]) -> Option<Model> {
    if bytes.starts_with(b"version ") {
        parse_asset_mesh(bytes)
    } else if bytes.starts_with(b"CSG") {
        parse_union(bytes)
    } else {
        parse_obj(&String::from_utf8_lossy(bytes))
    }
}

impl Model {
    /// The model as a mesh for the graphics chip. Its triangles come as
    /// three corners each, so most corners appear several times; each is
    /// kept once and the triangles point at it, which the chip then works
    /// out once instead of for every triangle that shares it. The mesh is
    /// kept only on the graphics side once it's there (its bounds stay).
    pub fn to_mesh(&self) -> Mesh {
        let (positions, normals, uvs, indices) = weld(&self.positions, &self.normals, &self.uvs);
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
            .with_inserted_indices(indices);
        crate::view::pack_mesh(&mut mesh);
        mesh
    }
}

type Welded = (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>, Indices);

/// Corners that are exactly the same (place, normal and texture position)
/// kept once, with the triangles as indices into them.
fn weld(positions: &[[f32; 3]], normals: &[[f32; 3]], uvs: &[[f32; 2]]) -> Welded {
    let mut seen: HashMap<[u32; 8], u32> = HashMap::with_capacity(positions.len() / 2);
    let (mut p, mut n, mut t) = (Vec::new(), Vec::new(), Vec::new());
    let mut indices = Vec::with_capacity(positions.len());
    for (i, at) in positions.iter().enumerate() {
        let normal = normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
        let uv = uvs.get(i).copied().unwrap_or([0.0, 0.0]);
        // (+0.0 and -0.0 are the same corner.)
        let bits = |x: f32| (x + 0.0).to_bits();
        let key = [
            bits(at[0]),
            bits(at[1]),
            bits(at[2]),
            bits(normal[0]),
            bits(normal[1]),
            bits(normal[2]),
            bits(uv[0]),
            bits(uv[1]),
        ];
        let index = *seen.entry(key).or_insert_with(|| {
            p.push(*at);
            n.push(normal);
            t.push(uv);
            (p.len() - 1) as u32
        });
        indices.push(index);
    }
    (p, n, t, Indices::U32(indices))
}

/// Reads the model a `MeshId` names, if it can be found and read: an
/// .obj or .mesh file, or an asset number (see [`crate::pictures`]) whose
/// file is either.
pub fn load(name: &str) -> Option<Model> {
    let name = name.trim();
    let lower = name.to_ascii_lowercase();
    let bytes = if crate::pictures::asset_id(name).is_some() {
        crate::pictures::bytes_for_kind(name, &["mesh", "obj"])?
    } else if lower.ends_with(".obj") || lower.ends_with(".mesh") || lower.starts_with("rbxasset://") {
        crate::pictures::bytes_for(name)?
    } else {
        return None;
    };
    parse(&bytes)
}

/// How finely [`Model::simplified`] keeps a model's shape: corners closer
/// than a tenth of its size each way merge.
const FAR_CELLS: f32 = 10.0;

/// Models with fewer triangles than this are drawn as they are at any
/// distance (a far copy would save little).
const FAR_MIN_TRIANGLES: usize = 400;

impl Model {
    /// A rougher copy of the model for drawing far away, where its small
    /// details are smaller than a pixel: corners in the same small box
    /// (a tenth of the model's size each way) become one, and triangles
    /// that shrink to nothing go. Sharp edges stay sharp (corners facing
    /// different ways don't merge their normals) and textures stay put
    /// (nor do corners from different parts of the picture). None when
    /// the model is small already, or the copy wouldn't be much smaller.
    pub fn simplified(&self) -> Option<Model> {
        let triangles = self.positions.len() / 3;
        if triangles < FAR_MIN_TRIANGLES {
            return None;
        }
        let cell = |p: [f32; 3]| {
            let c = |x: f32| ((x + 0.5) * FAR_CELLS).floor().clamp(0.0, FAR_CELLS - 1.0) as i32;
            IVec3::new(c(p[0]), c(p[1]), c(p[2]))
        };
        // Each box's corners averaged, so neighbouring triangles still meet.
        let mut middles: HashMap<IVec3, (Vec3, f32)> = HashMap::new();
        for p in &self.positions {
            let m = middles.entry(cell(*p)).or_insert((Vec3::ZERO, 0.0));
            m.0 += Vec3::from(*p);
            m.1 += 1.0;
        }
        // Normals and texture positions averaged among the corners that
        // face much the same way and show much the same part of the picture.
        let facing = |n: [f32; 3]| {
            let n = Vec3::from(n);
            let axis = n.abs().max_position();
            (axis as i32 + 1) * if n[axis] < 0.0 { -1 } else { 1 }
        };
        let key = |i: usize| {
            let uv = self.uvs.get(i).copied().unwrap_or_default();
            (
                cell(self.positions[i]),
                facing(self.normals[i]),
                IVec2::new((uv[0] * 8.0).floor() as i32, (uv[1] * 8.0).floor() as i32),
            )
        };
        let mut looks: HashMap<(IVec3, i32, IVec2), (Vec3, Vec2)> = HashMap::new();
        for i in 0..self.positions.len() {
            let l = looks.entry(key(i)).or_insert((Vec3::ZERO, Vec2::ZERO));
            l.0 += Vec3::from(self.normals[i]);
            l.1 += Vec2::from(self.uvs.get(i).copied().unwrap_or_default());
        }
        let counts = {
            let mut counts: HashMap<(IVec3, i32, IVec2), f32> = HashMap::new();
            for i in 0..self.positions.len() {
                *counts.entry(key(i)).or_default() += 1.0;
            }
            counts
        };
        let mut far = Model { positions: Vec::new(), normals: Vec::new(), uvs: Vec::new(), size: self.size };
        for t in 0..triangles {
            let corners = [t * 3, t * 3 + 1, t * 3 + 2];
            let cells = corners.map(|i| cell(self.positions[i]));
            if cells[0] == cells[1] || cells[1] == cells[2] || cells[0] == cells[2] {
                continue;
            }
            for i in corners {
                let (m, n) = middles[&cell(self.positions[i])];
                let k = key(i);
                let (normal, uv) = looks[&k];
                far.positions.push((m / n).to_array());
                far.normals.push(normal.normalize_or(Vec3::from(self.normals[i])).to_array());
                far.uvs.push((uv / counts[&k]).to_array());
            }
        }
        (far.positions.len() / 3 * 2 <= triangles && !far.positions.is_empty()).then_some(far)
    }
}

/// Models already read, by name (None when a name couldn't be read).
#[derive(Resource, Default)]
pub(crate) struct MeshFiles {
    loaded: HashMap<String, Option<Handle<Mesh>>>,
    /// Each detailed model's rough copy, by the model's mesh, and back.
    far_of: HashMap<AssetId<Mesh>, Handle<Mesh>>,
    near_of: HashMap<AssetId<Mesh>, Handle<Mesh>>,
    generation: u64,
}

impl MeshFiles {
    pub(crate) fn get(&mut self, name: &str, meshes: &mut Assets<Mesh>) -> Option<Handle<Mesh>> {
        let generation = crate::pictures::generation();
        if generation != self.generation {
            self.loaded.clear();
            self.far_of.clear();
            self.near_of.clear();
            self.generation = generation;
        }
        if let Some(known) = self.loaded.get(name) {
            return known.clone();
        }
        let made = load(name).map(|model| {
            let near = meshes.add(model.to_mesh());
            let far = model.simplified().map(|far| meshes.add(far.to_mesh()));
            if let Some(far) = far {
                self.near_of.insert(far.id(), near.clone());
                self.far_of.insert(near.id(), far);
            }
            near
        });
        self.loaded.insert(name.to_owned(), made.clone());
        made
    }

    /// Whether no model has a rough copy (nothing to switch).
    pub(crate) fn is_empty(&self) -> bool {
        self.far_of.is_empty()
    }

    /// For a model's mesh (either copy), its full and rough copies, if it
    /// has a rough one.
    pub(crate) fn both(&self, mesh: AssetId<Mesh>) -> Option<(Handle<Mesh>, Handle<Mesh>)> {
        if let Some(far) = self.far_of.get(&mesh) {
            let near = self.near_of.get(&far.id())?;
            return Some((near.clone(), far.clone()));
        }
        let near = self.near_of.get(&mesh)?;
        Some((near.clone(), self.far_of.get(&near.id())?.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PYRAMID: &str = "# a pyramid\nv 0 0 0\nv 4 0 0\nv 4 0 2\nv 0 0 2\nv 2 3 1\n\
        vt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\n\
        f 1/1 2/2 3/3 4/4\nf 1 2 5\nf 2 3 5\nf 3 4 5\nf -2 1 5\n";

    #[test]
    fn an_obj_file_becomes_triangles_fitted_into_a_unit_box() {
        let model = parse_obj(PYRAMID).unwrap();
        // The square base splits into two triangles, plus four sides.
        assert_eq!(model.positions.len(), 6 * 3);
        assert_eq!(model.size, Vec3::new(4.0, 3.0, 2.0));
        let (lo, hi) = model.positions.iter().fold((Vec3::splat(9.0), Vec3::splat(-9.0)), |(lo, hi), p| {
            (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
        });
        assert_eq!((lo, hi), (Vec3::splat(-0.5), Vec3::splat(0.5)));
        assert!(model.normals.iter().all(|n| (Vec3::from(*n).length() - 1.0).abs() < 1e-4));
        assert_eq!(model.uvs[2], [1.0, 0.0], "texture rows count down from the top");
        assert!(parse_obj("# nothing\nv 1 2 3\n").is_none());
        assert!(load("model.png").is_none(), "only .obj and .mesh files are models");
    }

    #[test]
    fn shared_corners_are_kept_once() {
        let model = parse_obj(PYRAMID).unwrap();
        let (points, normals, uvs, indices) = weld(&model.positions, &model.normals, &model.uvs);
        // 18 corners as triangles; the base's two triangles share two corners.
        assert_eq!(indices.len(), 18);
        assert_eq!(points.len(), 16);
        assert_eq!((normals.len(), uvs.len()), (16, 16));
        // Every triangle still has the same corners as in the file.
        let rebuilt: Vec<[f32; 3]> = indices.iter().map(|i| points[i]).collect();
        assert_eq!(rebuilt, model.positions);

        // As drawn: 16-bit indices, packed corners, and its box kept.
        let mesh = model.to_mesh();
        assert!(matches!(mesh.indices(), Some(Indices::U16(i)) if i.len() == 18));
        assert_eq!(mesh.count_vertices(), 16);
        assert!(matches!(
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
            Some(bevy::mesh::VertexAttributeValues::Snorm16x2(n)) if n.len() == 16
        ));
    }

    /// A smooth ball of `rings` by `rings * 2` squares, as an .obj file.
    fn ball(rings: usize) -> String {
        let segments = rings * 2;
        let mut text = String::new();
        for r in 0..=rings {
            let a = std::f32::consts::PI * r as f32 / rings as f32;
            for s in 0..=segments {
                let b = std::f32::consts::TAU * s as f32 / segments as f32;
                let (x, y, z) = (a.sin() * b.cos(), a.cos(), a.sin() * b.sin());
                text += &format!("v {x} {y} {z}\nvn {x} {y} {z}\n");
            }
        }
        let at = |r: usize, s: usize| r * (segments + 1) + s + 1;
        for r in 0..rings {
            for s in 0..segments {
                let (a, b, c, d) = (at(r, s), at(r, s + 1), at(r + 1, s + 1), at(r + 1, s));
                text += &format!("f {a}//{a} {b}//{b} {c}//{c}\nf {a}//{a} {c}//{c} {d}//{d}\n");
            }
        }
        text
    }

    #[test]
    fn detailed_models_get_a_rough_copy_for_far_away() {
        let model = parse_obj(&ball(32)).unwrap();
        let far = model.simplified().expect("a detailed ball has a rough copy");
        let (near, rough) = (model.positions.len() / 3, far.positions.len() / 3);
        assert!(rough * 4 <= near, "{near} triangles became {rough}");
        assert!(rough >= 100, "still a ball, not a blob: {rough}");
        // Still the same ball: every corner near its surface, normals outwards.
        for (p, n) in far.positions.iter().zip(&far.normals) {
            let (p, n) = (Vec3::from(*p), Vec3::from(*n));
            assert!((p.length() - 0.5).abs() < 0.08, "{p}");
            assert!(p.normalize().dot(n) > 0.8, "{p} {n}");
        }
        assert_eq!(far.size, model.size);
        // Small models are drawn as they are.
        assert!(parse_obj(PYRAMID).unwrap().simplified().is_none());
        assert!(parse_obj(&ball(6)).unwrap().simplified().is_none());
    }

    #[test]
    fn an_asset_mesh_file_is_read() {
        // Version 1.00 files are twice the size they mean.
        let text = "version 1.00\n1\n[0,0,0][0,1,0][0,0,0][4,0,0][0,1,0][1,0,0][0,0,8][0,1,0][0,1,0]";
        let model = parse(text.as_bytes()).unwrap();
        assert_eq!(model.positions.len(), 3);
        assert_eq!(model.size, Vec3::new(2.0, 0.0, 4.0));
        assert!(parse(b"version 9.99\nnot a mesh").is_none());
    }
}
