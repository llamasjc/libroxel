//! The shapes parts with a `MeshId` collide as: a set of convex pieces,
//! fitted (like the drawn model) into a 1 by 1 by 1 box and stretched to
//! the part's size.
//!
//! A model can come with its own collision file beside it, `name.obj`
//! with `name.collision.obj`, holding one convex piece per `o` group (an
//! imported union's collision shape is saved this way). Otherwise each
//! separate lump of the model becomes one piece: its convex hull.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bevy::math::{Mat3, Vec3 as GVec3};
use rapier3d::glamx::Vec3 as RVec;

/// One convex piece, in the unit box.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub points: Vec<GVec3>,
    /// Its faces' directions (one of each pair of opposite ones).
    pub normals: Vec<GVec3>,
    /// Its edges' directions.
    pub edges: Vec<GVec3>,
}

/// A part's collision shape: its pieces, and one hull round all of them
/// (for loose parts, which collide as one convex thing).
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub pieces: Vec<Piece>,
    pub whole: Piece,
}

impl Shape {
    fn new(pieces: Vec<Piece>) -> Option<Shape> {
        let all: Vec<GVec3> = pieces.iter().flat_map(|p| p.points.iter().copied()).collect();
        let whole = piece(&all)?;
        Some(Shape { pieces, whole })
    }

    /// Piece `i`, or the whole hull for [`WHOLE`].
    pub fn get(&self, i: u32) -> &Piece {
        self.pieces.get(i as usize).unwrap_or(&self.whole)
    }
}

/// The piece number meaning the hull round every piece.
pub const WHOLE: u32 = u32::MAX;

/// Separate lumps past this many are merged into one hull.
const MAX_PIECES: usize = 96;

#[derive(Default)]
struct Store {
    by_name: HashMap<String, Option<u32>>,
    shapes: Vec<Arc<Shape>>,
    generation: u64,
}

static STORE: std::sync::LazyLock<Mutex<Store>> = std::sync::LazyLock::new(|| Mutex::new(Store::default()));

/// The collision shape for a `MeshId`, if its model can be read. The
/// number stays valid for as long as the program runs.
pub fn for_mesh(name: &str) -> Option<u32> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let mut store = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let generation = crate::pictures::generation();
    if store.generation != generation {
        store.by_name.clear();
        store.generation = generation;
    }
    if let Some(found) = store.by_name.get(name) {
        return *found;
    }
    let made = build(name).and_then(Shape::new).map(|shape| {
        store.shapes.push(Arc::new(shape));
        (store.shapes.len() - 1) as u32
    });
    store.by_name.insert(name.to_owned(), made);
    made
}

/// A shape [`for_mesh`] gave.
pub fn shape(shape: u32) -> Option<Arc<Shape>> {
    let store = STORE.lock().unwrap_or_else(|e| e.into_inner());
    store.shapes.get(shape as usize).cloned()
}

fn build(name: &str) -> Option<Vec<Piece>> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".obj") && !lower.starts_with(crate::pictures::HUB_ASSET) {
        let companion = format!("{}.collision.obj", &name[..name.len() - 4]);
        if let Some(bytes) = crate::pictures::bytes_for(&companion)
            && let Some(pieces) = from_groups(&String::from_utf8_lossy(&bytes))
        {
            return Some(pieces);
        }
    }
    let model = crate::mesh_files::load(name)?;
    let points: Vec<GVec3> = model.positions.iter().map(|p| GVec3::from_array(*p)).collect();
    Some(from_lumps(&points))
}

/// Pieces from .obj text with one `o` group per piece, fitted together.
pub fn from_groups(text: &str) -> Option<Vec<Piece>> {
    let mut groups: Vec<Vec<GVec3>> = Vec::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("o" | "g") => groups.push(Vec::new()),
            Some("v") => {
                let n: Vec<f32> = words.filter_map(|w| w.parse().ok()).collect();
                if n.len() >= 3 {
                    if groups.is_empty() {
                        groups.push(Vec::new());
                    }
                    groups.last_mut()?.push(GVec3::new(n[0], n[1], n[2]));
                }
            }
            _ => {}
        }
    }
    groups.retain(|g| g.len() >= 4);
    let all: Vec<GVec3> = groups.iter().flatten().copied().collect();
    let (lo, hi) = bounds(&all)?;
    let (centre, scale) = ((lo + hi) / 2.0, (hi - lo).max(GVec3::splat(1e-4)).recip());
    let pieces: Vec<Piece> = groups
        .iter()
        .filter_map(|g| piece(&g.iter().map(|p| (*p - centre) * scale).collect::<Vec<_>>()))
        .collect();
    (!pieces.is_empty()).then_some(pieces)
}

/// Triangle corners (three per triangle) split into connected lumps, one
/// hull each.
pub fn from_lumps(corners: &[GVec3]) -> Vec<Piece> {
    // Corners at the same place are the same vertex.
    let key = |p: GVec3| ((p.x * 1e4).round() as i64, (p.y * 1e4).round() as i64, (p.z * 1e4).round() as i64);
    let mut ids: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let vertex: Vec<usize> = corners
        .iter()
        .map(|p| {
            let n = ids.len();
            *ids.entry(key(*p)).or_insert(n)
        })
        .collect();
    let mut parent: Vec<usize> = (0..ids.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for tri in vertex.as_chunks::<3>().0 {
        for pair in [(tri[0], tri[1]), (tri[1], tri[2])] {
            let (a, b) = (root(&mut parent, pair.0), root(&mut parent, pair.1));
            if a != b {
                parent[a] = b;
            }
        }
    }
    let mut lumps: HashMap<usize, Vec<GVec3>> = HashMap::new();
    for (corner, v) in corners.iter().zip(&vertex) {
        let r = root(&mut parent, *v);
        lumps.entry(r).or_default().push(*corner);
    }
    let mut lumps: Vec<Vec<GVec3>> = lumps.into_values().collect();
    lumps.sort_by_key(|l| std::cmp::Reverse(l.len()));
    if lumps.len() > MAX_PIECES {
        let rest: Vec<GVec3> = lumps.drain(MAX_PIECES - 1..).flatten().collect();
        lumps.push(rest);
    }
    let pieces: Vec<Piece> = lumps.iter().filter_map(|l| piece(l)).collect();
    if pieces.is_empty() {
        // Flat or tiny: the whole box.
        return piece(&box_corners()).into_iter().collect();
    }
    pieces
}

fn box_corners() -> Vec<GVec3> {
    let mut v = Vec::new();
    for x in [-0.5, 0.5] {
        for y in [-0.5, 0.5] {
            for z in [-0.5, 0.5] {
                v.push(GVec3::new(x, y, z));
            }
        }
    }
    v
}

fn bounds(points: &[GVec3]) -> Option<(GVec3, GVec3)> {
    let lo = points.iter().copied().reduce(GVec3::min)?;
    let hi = points.iter().copied().reduce(GVec3::max)?;
    Some((lo, hi))
}

/// The convex hull of some points, as a piece.
fn piece(points: &[GVec3]) -> Option<Piece> {
    let (lo, hi) = bounds(points)?;
    let size = hi - lo;
    // A lump with no thickness gets a sliver of one, so it still blocks.
    let mut points = points.to_vec();
    for axis in 0..3 {
        if size[axis] < 1e-3 {
            let extra: Vec<GVec3> = points
                .iter()
                .map(|p| {
                    let mut q = *p;
                    q[axis] += 0.02;
                    q
                })
                .collect();
            points.extend(extra);
        }
    }
    let input: Vec<RVec> = points.iter().map(|p| RVec::new(p.x, p.y, p.z)).collect();
    let (vertices, faces) = rapier3d::parry::transformation::convex_hull(&input);
    if faces.is_empty() {
        return None;
    }
    let vertices: Vec<GVec3> = vertices.iter().map(|v| GVec3::new(v.x, v.y, v.z)).collect();
    let mut normals: Vec<GVec3> = Vec::new();
    let mut edges: Vec<GVec3> = Vec::new();
    let add = |list: &mut Vec<GVec3>, d: GVec3| {
        let Some(d) = d.try_normalize() else { return };
        if !list.iter().any(|e| e.dot(d).abs() > 0.9995) {
            list.push(d);
        }
    };
    for f in &faces {
        let [a, b, c] = f.map(|i| vertices[i as usize]);
        add(&mut normals, (b - a).cross(c - a));
        add(&mut edges, b - a);
        add(&mut edges, c - b);
        add(&mut edges, a - c);
    }
    Some(Piece { points: vertices, normals, edges })
}

/// A piece placed in the world: its points, face directions and edge
/// directions for a part `size` big, turned by `rotation`, centred at `centre`.
pub fn placed(
    piece: &Piece,
    centre: GVec3,
    rotation: Mat3,
    size: GVec3,
) -> (Vec<GVec3>, Vec<GVec3>, Vec<GVec3>) {
    let size = size.max(GVec3::splat(1e-3));
    let points = piece.points.iter().map(|p| centre + rotation * (*p * size)).collect();
    // Stretching bends face directions the other way.
    let normals = piece.normals.iter().map(|n| (rotation * (*n / size)).normalize_or(GVec3::Y)).collect();
    let edges = piece.edges.iter().map(|e| (rotation * (*e * size)).normalize_or(GVec3::X)).collect();
    (points, normals, edges)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(at: GVec3, half: f32) -> Vec<GVec3> {
        // Twelve triangles of a cube, as corners.
        let c = |x: f32, y: f32, z: f32| at + GVec3::new(x, y, z) * half;
        let v = [
            c(-1., -1., -1.),
            c(1., -1., -1.),
            c(1., 1., -1.),
            c(-1., 1., -1.),
            c(-1., -1., 1.),
            c(1., -1., 1.),
            c(1., 1., 1.),
            c(-1., 1., 1.),
        ];
        let faces = [
            [0, 1, 2],
            [0, 2, 3],
            [4, 6, 5],
            [4, 7, 6],
            [0, 4, 5],
            [0, 5, 1],
            [3, 2, 6],
            [3, 6, 7],
            [0, 3, 7],
            [0, 7, 4],
            [1, 5, 6],
            [1, 6, 2],
        ];
        faces.iter().flat_map(|f| f.map(|i| v[i])).collect()
    }

    #[test]
    fn separate_lumps_become_separate_pieces() {
        let mut corners = cube(GVec3::new(-0.4, 0.0, 0.0), 0.1);
        corners.extend(cube(GVec3::new(0.4, 0.0, 0.0), 0.1));
        let pieces = from_lumps(&corners);
        assert_eq!(pieces.len(), 2);
        for p in &pieces {
            assert_eq!(p.points.len(), 8);
            assert_eq!(p.normals.len(), 3);
            assert!(p.edges.len() >= 3, "a cube's edges, and its triangles' diagonals");
        }
        // Nothing between the two lumps.
        let hi = pieces
            .iter()
            .map(|p| p.points.iter().map(|q| q.x).fold(f32::MIN, f32::max))
            .fold(f32::MAX, f32::min);
        assert!(hi < 0.0);
    }

    #[test]
    fn groups_are_fitted_together() {
        let text = "o a\nv 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\no b\nv 3 0 0\nv 4 0 0\nv 3 1 0\nv 3 0 1\n";
        let pieces = from_groups(text).unwrap();
        assert_eq!(pieces.len(), 2);
        let xs: Vec<f32> = pieces.iter().flat_map(|p| p.points.iter().map(|q| q.x)).collect();
        assert!(xs.iter().all(|x| (-0.5..=0.5).contains(x)));
        assert!(xs.contains(&-0.5) && xs.contains(&0.5));
        assert!(from_groups("v 1 2 3\n").is_none());
    }

    #[test]
    fn a_flat_lump_gets_some_thickness() {
        let corners =
            vec![GVec3::new(-0.5, 0.0, -0.5), GVec3::new(0.5, 0.0, -0.5), GVec3::new(0.5, 0.0, 0.5)];
        assert_eq!(from_lumps(&corners).len(), 1);
    }
}
