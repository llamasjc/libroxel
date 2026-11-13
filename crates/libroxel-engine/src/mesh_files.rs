//! 3D models from files, for parts with a `MeshId`: Wavefront .obj files,
//! found the same way as pictures (see [`pictures`](crate::pictures)). A
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
    if corners.is_empty() {
        return None;
    }
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for &(v, ..) in &corners {
        lo = lo.min(vs[v]);
        hi = hi.max(vs[v]);
    }
    let size = hi - lo;
    let centre = (lo + hi) / 2.0;
    // A flat side stays flat rather than becoming infinitely thin maths.
    let scale = Vec3::new(1.0 / size.x.max(1e-4), 1.0 / size.y.max(1e-4), 1.0 / size.z.max(1e-4));
    let mut model = Model { positions: Vec::new(), normals: Vec::new(), uvs: Vec::new(), size };
    for tri in corners.chunks(3) {
        let p: Vec<Vec3> = tri.iter().map(|c| (vs[c.0] - centre) * scale).collect();
        let flat = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or(Vec3::Y);
        for (corner, at) in tri.iter().zip(p) {
            model.positions.push(at.to_array());
            // Stretching to fit bends normals the other way.
            let n = corner.2.map_or(flat, |n| (vns[n] / scale).normalize_or(flat));
            model.normals.push(n.to_array());
            let uv = corner.1.map_or(Vec2::ZERO, |t| vts[t]);
            model.uvs.push([uv.x, 1.0 - uv.y]);
        }
    }
    Some(model)
}

impl Model {
    pub fn to_mesh(&self) -> Mesh {
        let count = self.positions.len() as u32;
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs.clone())
            .with_inserted_indices(Indices::U32((0..count).collect()))
    }
}

/// Reads the model a `MeshId` names, if it can be found and read.
pub fn load(name: &str) -> Option<Model> {
    let name = name.trim();
    if !name.to_ascii_lowercase().ends_with(".obj") {
        return None;
    }
    let bytes = crate::pictures::bytes_for(name)?;
    parse_obj(&String::from_utf8_lossy(&bytes))
}

/// Models already read, by name (None when a name couldn't be read).
#[derive(Resource, Default)]
pub(crate) struct MeshFiles {
    loaded: HashMap<String, Option<Handle<Mesh>>>,
    generation: u64,
}

impl MeshFiles {
    pub(crate) fn get(&mut self, name: &str, meshes: &mut Assets<Mesh>) -> Option<Handle<Mesh>> {
        let generation = crate::pictures::generation();
        if generation != self.generation {
            self.loaded.clear();
            self.generation = generation;
        }
        self.loaded
            .entry(name.to_owned())
            .or_insert_with(|| load(name).map(|m| meshes.add(m.to_mesh())))
            .clone()
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
        assert!(load("model.png").is_none(), "only .obj files are models");
    }
}
