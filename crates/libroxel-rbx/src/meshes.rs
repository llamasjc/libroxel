//! Shapes stored inside a file, turned into Wavefront .obj text the
//! engine can draw: a union's drawn shape (`MeshData`), or when that isn't
//! in the file (a published union keeps it online), the convex pieces of
//! its collision shape (`PhysicsData`).

use std::fmt::Write;

use rbx_mesh::union_graphics::UnionGraphics;
use rbx_mesh::union_physics::UnionPhysics;

/// Triangles with optional normals and texture positions at each corner.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Triangles {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Corners, three per triangle, as indices into the lists above.
    pub corners: Vec<u32>,
}

impl Triangles {
    fn add_indexed(&mut self, positions: &[[f32; 3]], faces: impl IntoIterator<Item = [u32; 3]>) {
        let start = self.positions.len() as u32;
        self.positions.extend_from_slice(positions);
        for face in faces {
            if face.iter().all(|&i| (i as usize) < positions.len()) {
                self.corners.extend(face.map(|i| start + i));
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.corners.is_empty()
    }

    /// As .obj text.
    pub fn to_obj(&self) -> String {
        let mut out = String::from("# Made by Libroxel from a union in an imported file\n");
        for p in &self.positions {
            let _ = writeln!(out, "v {} {} {}", p[0], p[1], p[2]);
        }
        let with_normals = self.normals.len() == self.positions.len();
        let with_uvs = self.uvs.len() == self.positions.len();
        if with_normals {
            for n in &self.normals {
                let _ = writeln!(out, "vn {} {} {}", n[0], n[1], n[2]);
            }
        }
        if with_uvs {
            for t in &self.uvs {
                // .obj texture rows count up from the bottom.
                let _ = writeln!(out, "vt {} {}", t[0], 1.0 - t[1]);
            }
        }
        for tri in self.corners.as_chunks::<3>().0 {
            out.push('f');
            for &i in tri {
                let i = i + 1;
                let _ = match (with_uvs, with_normals) {
                    (true, true) => write!(out, " {i}/{i}/{i}"),
                    (false, true) => write!(out, " {i}//{i}"),
                    (true, false) => write!(out, " {i}/{i}"),
                    (false, false) => write!(out, " {i}"),
                };
            }
            out.push('\n');
        }
        out
    }
}

/// A union's drawn shape, from its `MeshData`. None for a published union,
/// whose shape is kept online rather than in the file.
pub fn union_graphics(bytes: &[u8]) -> Option<Triangles> {
    let mut t = Triangles::default();
    match rbx_mesh::read_union_graphics_versioned(std::io::Cursor::new(bytes)).ok()? {
        UnionGraphics::CSGK(_) => return None,
        UnionGraphics::V2(m) => {
            let v = &m.mesh.vertices;
            t.normals = v.iter().map(|v| v.norm).collect();
            t.uvs = v.iter().map(|v| v.tex).collect();
            let positions: Vec<_> = v.iter().map(|v| v.pos).collect();
            t.add_indexed(&positions, m.mesh.faces.iter().map(|f| [f[0].0, f[1].0, f[2].0]));
        }
        UnionGraphics::V4(m) => {
            let v = &m.mesh.vertices;
            t.normals = v.iter().map(|v| v.norm).collect();
            t.uvs = v.iter().map(|v| v.tex).collect();
            let positions: Vec<_> = v.iter().map(|v| v.pos).collect();
            t.add_indexed(&positions, m.mesh.faces.iter().map(|f| [f[0].0, f[1].0, f[2].0]));
        }
        UnionGraphics::V5(m) => {
            if m.normals.len() == m.positions.len() {
                t.normals = m.normals.iter().map(|n| n.0).collect();
            }
            if m.tex.len() == m.positions.len() {
                t.uvs = m.tex.clone();
            }
            let faces = m.faces.indices.as_chunks::<3>().0.iter().copied();
            t.add_indexed(&m.positions, faces);
        }
    }
    (!t.is_empty()).then_some(t)
}

/// A union's collision shape, from its `PhysicsData`: one or more convex
/// pieces. Good enough to show the shape when the drawn one isn't in the file.
pub fn union_physics(bytes: &[u8]) -> Option<Triangles> {
    let mut t = Triangles::default();
    let mut add = |m: &rbx_mesh::union_physics::Mesh| {
        t.add_indexed(&m.positions, m.faces.iter().map(|f| [f[0].0, f[1].0, f[2].0]));
    };
    match rbx_mesh::read_union_physics_versioned(std::io::Cursor::new(bytes)).ok()? {
        UnionPhysics::CSGK(_) | UnionPhysics::Block(_) => return None,
        UnionPhysics::V3(p) => p.meshes.iter().for_each(&mut add),
        UnionPhysics::V5(p) => p.meshes.iter().for_each(&mut add),
        UnionPhysics::V6(p) => add(&p.mesh),
        UnionPhysics::V7(p) => p.meshes.iter().for_each(&mut add),
        UnionPhysics::V8(p) => {
            for hull in p.mesh.hulls.iter_hulls() {
                t.add_indexed(hull.positions, hull.faces.iter().copied());
            }
        }
    }
    (!t.is_empty()).then_some(t)
}

/// A union's collision shape as .obj text with one `o` group per convex
/// piece, the way the engine reads a model's collision file.
pub fn union_collision(bytes: &[u8]) -> Option<String> {
    let mut pieces: Vec<Vec<[f32; 3]>> = Vec::new();
    let mut add = |m: &rbx_mesh::union_physics::Mesh| pieces.push(m.positions.clone());
    match rbx_mesh::read_union_physics_versioned(std::io::Cursor::new(bytes)).ok()? {
        UnionPhysics::CSGK(_) | UnionPhysics::Block(_) => return None,
        UnionPhysics::V3(p) => p.meshes.iter().for_each(&mut add),
        UnionPhysics::V5(p) => p.meshes.iter().for_each(&mut add),
        UnionPhysics::V6(p) => add(&p.mesh),
        UnionPhysics::V7(p) => p.meshes.iter().for_each(&mut add),
        UnionPhysics::V8(p) => {
            for hull in p.mesh.hulls.iter_hulls() {
                pieces.push(hull.positions.to_vec());
            }
        }
    }
    pieces.retain(|p| p.len() >= 4);
    if pieces.is_empty() {
        return None;
    }
    let mut out = String::from("# A union's collision shape, one convex piece per group, made by Libroxel\n");
    for (i, piece) in pieces.iter().enumerate() {
        let _ = writeln!(out, "o piece{i}");
        for p in piece {
            let _ = writeln!(out, "v {} {} {}", p[0], p[1], p[2]);
        }
    }
    Some(out)
}

/// A file name for a made model that's the same whenever its contents are.
pub fn file_name(prefix: &str, text: &str) -> String {
    // FNV-1a: small, and the same on every computer.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        hash = (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{prefix}-{hash:016x}.obj")
}

/// A corner wedge: a square base with its top at one corner (the +X, -Z
/// one), fitting a 1 × 1 × 1 box.
pub fn corner_wedge() -> String {
    "# A corner wedge, made by Libroxel\n\
     v -0.5 -0.5 -0.5\nv 0.5 -0.5 -0.5\nv 0.5 -0.5 0.5\nv -0.5 -0.5 0.5\nv 0.5 0.5 -0.5\n\
     f 1 2 3\nf 1 3 4\nf 2 5 3\nf 1 5 2\nf 4 3 5\nf 1 4 5\n"
        .to_owned()
}

/// Adds a box from `lo` to `hi` to .obj text whose vertices so far number `count`.
fn add_box(out: &mut String, count: &mut u32, lo: [f32; 3], hi: [f32; 3]) {
    for y in [lo[1], hi[1]] {
        for z in [lo[2], hi[2]] {
            for x in [lo[0], hi[0]] {
                let _ = writeln!(out, "v {x} {y} {z}");
            }
        }
    }
    let b = *count;
    for f in [[1, 3, 4, 2], [5, 6, 8, 7], [1, 2, 6, 5], [3, 7, 8, 4], [1, 5, 7, 3], [2, 4, 8, 6]] {
        let _ = writeln!(out, "f {} {} {} {}", f[0] + b, f[1] + b, f[2] + b, f[3] + b);
    }
    *count += 8;
}

/// A truss `cells` 2-stud cells long on each side: bars along every
/// outside edge of the cells. Returns the model and its collision shape (a
/// solid box, so a truss can be stood on and climbed like one).
pub fn truss(cells: [u32; 3]) -> (String, String) {
    let n = cells.map(|c| c.clamp(1, 64));
    let size = n.map(|c| c as f32 * 2.0);
    let half = size.map(|s| s / 2.0);
    let bar = 0.18;
    let mut out = String::from("# A truss, made by Libroxel\n");
    let mut count = 0;
    // An edge along `axis` at grid position (i, j) on the other two axes is
    // on the outside if either of those is at an end.
    for axis in 0..3 {
        let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
        for i in 0..=n[a] {
            for j in 0..=n[b] {
                let outside = i == 0 || i == n[a] || j == 0 || j == n[b];
                if !outside {
                    continue;
                }
                let mut lo = [0.0; 3];
                let mut hi = [0.0; 3];
                lo[axis] = -half[axis];
                hi[axis] = half[axis];
                for (k, at) in [(a, i), (b, j)] {
                    let p = -half[k] + at as f32 * 2.0;
                    lo[k] = (p - bar).max(-half[k]);
                    hi[k] = (p + bar).min(half[k]);
                }
                add_box(&mut out, &mut count, lo, hi);
            }
        }
    }
    let mut solid = String::from("# A truss's collision shape: a box\no box\n");
    let mut ignored = 0;
    add_box(&mut solid, &mut ignored, half.map(|h| -h), half);
    (out, solid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obj_text_lists_corners_from_one() {
        let mut t = Triangles::default();
        t.add_indexed(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], [[0, 1, 2], [0, 1, 9]]);
        // The face pointing past the end is dropped.
        assert_eq!(t.corners, vec![0, 1, 2]);
        let obj = t.to_obj();
        assert!(obj.contains("v 1 0 0\n"), "{obj}");
        assert!(obj.ends_with("f 1 2 3\n"), "{obj}");
    }

    #[test]
    fn names_follow_contents() {
        assert_eq!(file_name("union", "a"), file_name("union", "a"));
        assert_ne!(file_name("union", "a"), file_name("union", "b"));
        assert!(file_name("union", "a").ends_with(".obj"));
    }

    #[test]
    fn a_truss_is_bars_round_its_cells() {
        let (model, solid) = truss([1, 5, 1]);
        let boxes = model.lines().filter(|l| l.starts_with("f ")).count() / 6;
        // Four posts, then rings of four bars at each of the six levels.
        assert_eq!(boxes, 4 + 6 * 4);
        assert!(model.contains("v -1 -5 -1\n"), "{model}");
        assert_eq!(solid.lines().filter(|l| l.starts_with("v ")).count(), 8);
    }

    #[test]
    fn rubbish_is_not_a_union() {
        assert!(union_graphics(b"nothing here").is_none());
        assert!(union_physics(b"nothing here").is_none());
    }
}
