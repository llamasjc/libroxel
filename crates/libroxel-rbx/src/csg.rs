//! Solid shapes joined and cut by planes (a BSP tree, as in csg.js), for
//! rebuilding a union from the parts it was made of.

use std::ops::{Add, Mul, Neg, Sub};

const EPSILON: f64 = 1e-5;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl V3 {
    pub const fn new(x: f64, y: f64, z: f64) -> V3 {
        V3 { x, y, z }
    }

    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: V3) -> V3 {
        V3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }

    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    pub fn normalize(self) -> V3 {
        let l = self.length();
        if l > 0.0 { self * (1.0 / l) } else { self }
    }

    pub fn lerp(self, o: V3, t: f64) -> V3 {
        self + (o - self) * t
    }

    pub fn mul_each(self, o: V3) -> V3 {
        V3::new(self.x * o.x, self.y * o.y, self.z * o.z)
    }

    pub fn from_f32(p: [f32; 3]) -> V3 {
        V3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
    }

    pub fn to_f32(self) -> [f32; 3] {
        [self.x as f32, self.y as f32, self.z as f32]
    }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        V3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        V3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f64> for V3 {
    type Output = V3;
    fn mul(self, s: f64) -> V3 {
        V3::new(self.x * s, self.y * s, self.z * s)
    }
}

impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        V3::new(-self.x, -self.y, -self.z)
    }
}

/// A placement: a rotation (by columns) then a move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub x: V3,
    pub y: V3,
    pub z: V3,
    pub at: V3,
}

impl Frame {
    pub const IDENTITY: Frame = Frame {
        x: V3::new(1.0, 0.0, 0.0),
        y: V3::new(0.0, 1.0, 0.0),
        z: V3::new(0.0, 0.0, 1.0),
        at: V3::new(0.0, 0.0, 0.0),
    };

    pub fn turn(&self, v: V3) -> V3 {
        self.x * v.x + self.y * v.y + self.z * v.z
    }

    pub fn point(&self, v: V3) -> V3 {
        self.turn(v) + self.at
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub pos: V3,
    pub normal: V3,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Plane {
    normal: V3,
    w: f64,
}

impl Plane {
    fn from_points(a: V3, b: V3, c: V3) -> Option<Plane> {
        let n = (b - a).cross(c - a);
        if n.length() < 1e-12 {
            return None;
        }
        let normal = n.normalize();
        Some(Plane { normal, w: normal.dot(a) })
    }

    fn flip(&mut self) {
        self.normal = -self.normal;
        self.w = -self.w;
    }
}

/// A flat convex polygon, its corners anticlockwise seen from outside.
#[derive(Debug, Clone, PartialEq)]
pub struct Polygon {
    pub vertices: Vec<Vertex>,
    plane: Plane,
}

impl Polygon {
    pub fn new(vertices: Vec<Vertex>) -> Option<Polygon> {
        if vertices.len() < 3 {
            return None;
        }
        let a = vertices[0].pos;
        let mut best: Option<Plane> = None;
        let mut area = 0.0;
        for i in 1..vertices.len() - 1 {
            let n = (vertices[i].pos - a).cross(vertices[i + 1].pos - a);
            if n.length() > area {
                area = n.length();
                best = Plane::from_points(a, vertices[i].pos, vertices[i + 1].pos);
            }
        }
        Some(Polygon { plane: best?, vertices })
    }

    fn flip(&mut self) {
        self.vertices.reverse();
        for v in &mut self.vertices {
            v.normal = -v.normal;
        }
        self.plane.flip();
    }
}

const COPLANAR: u8 = 0;
const FRONT: u8 = 1;
const BACK: u8 = 2;
const SPANNING: u8 = 3;

/// Sorts `poly` by `plane`: into the coplanar lists, or front or back,
/// cutting it in two when it crosses.
fn split(
    plane: &Plane,
    poly: Polygon,
    coplanar_front: &mut Vec<Polygon>,
    coplanar_back: &mut Vec<Polygon>,
    front: &mut Vec<Polygon>,
    back: &mut Vec<Polygon>,
) {
    let mut kind = 0u8;
    let types: Vec<u8> = poly
        .vertices
        .iter()
        .map(|v| {
            let t = plane.normal.dot(v.pos) - plane.w;
            let k = if t < -EPSILON {
                BACK
            } else if t > EPSILON {
                FRONT
            } else {
                COPLANAR
            };
            kind |= k;
            k
        })
        .collect();
    match kind {
        COPLANAR => {
            if plane.normal.dot(poly.plane.normal) > 0.0 {
                coplanar_front.push(poly);
            } else {
                coplanar_back.push(poly);
            }
        }
        FRONT => front.push(poly),
        BACK => back.push(poly),
        _ => {
            let (mut f, mut b) = (Vec::new(), Vec::new());
            let n = poly.vertices.len();
            for i in 0..n {
                let j = (i + 1) % n;
                let (ti, tj) = (types[i], types[j]);
                let (vi, vj) = (poly.vertices[i], poly.vertices[j]);
                if ti != BACK {
                    f.push(vi);
                }
                if ti != FRONT {
                    b.push(vi);
                }
                if (ti | tj) == SPANNING {
                    let t = (plane.w - plane.normal.dot(vi.pos)) / plane.normal.dot(vj.pos - vi.pos);
                    let v = Vertex { pos: vi.pos.lerp(vj.pos, t), normal: vi.normal.lerp(vj.normal, t) };
                    f.push(v);
                    b.push(v);
                }
            }
            if f.len() >= 3 {
                front.push(Polygon { vertices: f, plane: poly.plane });
            }
            if b.len() >= 3 {
                back.push(Polygon { vertices: b, plane: poly.plane });
            }
        }
    }
}

/// A plane that cuts few of `polys` and splits the rest evenly.
fn choose(polys: &[Polygon]) -> Plane {
    const TRIES: usize = 12;
    let step = (polys.len() / TRIES).max(1);
    let mut best = (usize::MAX, polys[0].plane);
    for cand in polys.iter().step_by(step).take(TRIES) {
        let (mut front, mut back, mut cut) = (0usize, 0usize, 0usize);
        for p in polys {
            let (mut f, mut b) = (false, false);
            for v in &p.vertices {
                let t = cand.plane.normal.dot(v.pos) - cand.plane.w;
                f |= t > EPSILON;
                b |= t < -EPSILON;
            }
            match (f, b) {
                (true, true) => cut += 1,
                (true, false) => front += 1,
                (false, true) => back += 1,
                _ => {}
            }
        }
        let score = cut * 8 + front.abs_diff(back);
        if score < best.0 {
            best = (score, cand.plane);
        }
    }
    best.1
}

#[derive(Default)]
struct Node {
    plane: Option<Plane>,
    front: Option<usize>,
    back: Option<usize>,
    polygons: Vec<Polygon>,
}

/// A BSP tree, its nodes kept in a list so deep trees need no deep calls.
struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    fn new(polygons: Vec<Polygon>) -> Tree {
        let mut t = Tree { nodes: vec![Node::default()] };
        t.build(polygons);
        t
    }

    fn build(&mut self, polygons: Vec<Polygon>) {
        let mut stack = vec![(0usize, polygons)];
        while let Some((n, polys)) = stack.pop() {
            if polys.is_empty() {
                continue;
            }
            let plane = *self.nodes[n].plane.get_or_insert_with(|| choose(&polys));
            let (mut cf, mut cb, mut front, mut back) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for p in polys {
                split(&plane, p, &mut cf, &mut cb, &mut front, &mut back);
            }
            self.nodes[n].polygons.append(&mut cf);
            self.nodes[n].polygons.append(&mut cb);
            if !front.is_empty() {
                let f = self.child(n, true);
                stack.push((f, front));
            }
            if !back.is_empty() {
                let b = self.child(n, false);
                stack.push((b, back));
            }
        }
    }

    fn child(&mut self, n: usize, front: bool) -> usize {
        let existing = if front { self.nodes[n].front } else { self.nodes[n].back };
        existing.unwrap_or_else(|| {
            self.nodes.push(Node::default());
            let id = self.nodes.len() - 1;
            if front {
                self.nodes[n].front = Some(id);
            } else {
                self.nodes[n].back = Some(id);
            }
            id
        })
    }

    fn invert(&mut self) {
        for node in &mut self.nodes {
            for p in &mut node.polygons {
                p.flip();
            }
            if let Some(p) = &mut node.plane {
                p.flip();
            }
            std::mem::swap(&mut node.front, &mut node.back);
        }
    }

    /// The parts of `polygons` outside this tree's solid.
    fn clip_polygons(&self, polygons: Vec<Polygon>) -> Vec<Polygon> {
        let mut out = Vec::new();
        let mut stack = vec![(0usize, polygons)];
        while let Some((n, polys)) = stack.pop() {
            let node = &self.nodes[n];
            let Some(plane) = node.plane else {
                out.extend(polys);
                continue;
            };
            let (mut front, mut back) = (Vec::new(), Vec::new());
            let (mut cf, mut cb) = (Vec::new(), Vec::new());
            for p in polys {
                split(&plane, p, &mut cf, &mut cb, &mut front, &mut back);
            }
            front.append(&mut cf);
            back.append(&mut cb);
            match node.front {
                Some(f) => stack.push((f, front)),
                None => out.extend(front),
            }
            if let Some(b) = node.back {
                stack.push((b, back));
            }
        }
        out
    }
}

/// What's inside `a` but none of `cuts`, cutting `a` only where a cut reaches.
pub fn subtract_all(a: Vec<Polygon>, cuts: &[Vec<Polygon>]) -> Vec<Polygon> {
    let Some(ab) = bounds(&a) else { return a };
    let cuts: Vec<((V3, V3), Tree, &Vec<Polygon>)> = cuts
        .iter()
        .filter_map(|c| Some((bounds(c)?, c)))
        .filter(|(b, _)| touch(*b, ab))
        .map(|(b, c)| (b, Tree::new(c.clone()), c))
        .collect();
    if cuts.is_empty() {
        return a;
    }
    let mut outside = Tree::new(a.clone());
    outside.invert();
    let mut out = Vec::new();
    for mut p in a {
        let pb = bounds(std::slice::from_ref(&p)).unwrap_or(ab);
        let mut pieces = vec![{
            p.flip();
            p
        }];
        for (cb, tree, _) in &cuts {
            if touch(pb, *cb) {
                pieces = tree.clip_polygons(pieces);
            }
        }
        for mut q in pieces {
            q.flip();
            out.push(q);
        }
    }
    for (i, (bi, _, c)) in cuts.iter().enumerate() {
        let mut inner = outside.clip_polygons((*c).clone());
        for q in &mut inner {
            q.flip();
        }
        inner = outside.clip_polygons(inner);
        for (j, (bj, tree, _)) in cuts.iter().enumerate() {
            if i != j && touch(*bi, *bj) {
                inner = tree.clip_polygons(inner);
            }
        }
        out.extend(inner);
    }
    out
}

fn touch(a: (V3, V3), b: (V3, V3)) -> bool {
    let e = EPSILON;
    a.0.x <= b.1.x + e
        && b.0.x <= a.1.x + e
        && a.0.y <= b.1.y + e
        && b.0.y <= a.1.y + e
        && a.0.z <= b.1.z + e
        && b.0.z <= a.1.z + e
}

pub fn bounds(polys: &[Polygon]) -> Option<(V3, V3)> {
    let mut it = polys.iter().flat_map(|p| p.vertices.iter().map(|v| v.pos));
    let first = it.next()?;
    Some(it.fold((first, first), |(lo, hi), p| {
        (
            V3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z)),
            V3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z)),
        )
    }))
}

/// A face from corners and the way it faces, wound to suit.
fn face(corners: &[V3], normal: V3, smooth: Option<&[V3]>) -> Option<Polygon> {
    let vertices = corners
        .iter()
        .enumerate()
        .map(|(i, &pos)| Vertex { pos, normal: smooth.map_or(normal, |s| s[i]) })
        .collect();
    let mut p = Polygon::new(vertices)?;
    if p.plane.normal.dot(normal) < 0.0 {
        p.vertices.reverse();
        p.plane.flip();
    }
    Some(p)
}

fn place(polys: Vec<Polygon>, frame: &Frame) -> Vec<Polygon> {
    polys
        .into_iter()
        .filter_map(|p| {
            let vertices = p
                .vertices
                .iter()
                .map(|v| Vertex { pos: frame.point(v.pos), normal: frame.turn(v.normal).normalize() })
                .collect();
            let mut out = Polygon::new(vertices)?;
            if out.plane.normal.dot(frame.turn(p.plane.normal)) < 0.0 {
                out.plane.flip();
            }
            Some(out)
        })
        .collect()
}

pub fn block(size: V3, frame: &Frame) -> Vec<Polygon> {
    let h = size * 0.5;
    let c = |x: f64, y: f64, z: f64| V3::new(x * h.x, y * h.y, z * h.z);
    let faces = [
        ([c(1., -1., -1.), c(1., 1., -1.), c(1., 1., 1.), c(1., -1., 1.)], V3::new(1., 0., 0.)),
        ([c(-1., -1., -1.), c(-1., -1., 1.), c(-1., 1., 1.), c(-1., 1., -1.)], V3::new(-1., 0., 0.)),
        ([c(-1., 1., -1.), c(-1., 1., 1.), c(1., 1., 1.), c(1., 1., -1.)], V3::new(0., 1., 0.)),
        ([c(-1., -1., -1.), c(1., -1., -1.), c(1., -1., 1.), c(-1., -1., 1.)], V3::new(0., -1., 0.)),
        ([c(-1., -1., 1.), c(1., -1., 1.), c(1., 1., 1.), c(-1., 1., 1.)], V3::new(0., 0., 1.)),
        ([c(-1., -1., -1.), c(-1., 1., -1.), c(1., 1., -1.), c(1., -1., -1.)], V3::new(0., 0., -1.)),
    ];
    place(faces.iter().filter_map(|(cs, n)| face(cs, *n, None)).collect(), frame)
}

/// Its slope rises from the front (-Z) bottom to the back (+Z) top.
pub fn wedge(size: V3, frame: &Frame) -> Vec<Polygon> {
    let h = size * 0.5;
    let c = |x: f64, y: f64, z: f64| V3::new(x * h.x, y * h.y, z * h.z);
    let slope = V3::new(0.0, size.z, -size.y).normalize();
    let faces: [(Vec<V3>, V3); 5] = [
        (vec![c(-1., -1., -1.), c(1., -1., -1.), c(1., -1., 1.), c(-1., -1., 1.)], V3::new(0., -1., 0.)),
        (vec![c(-1., -1., 1.), c(1., -1., 1.), c(1., 1., 1.), c(-1., 1., 1.)], V3::new(0., 0., 1.)),
        (vec![c(-1., -1., -1.), c(1., -1., -1.), c(1., 1., 1.), c(-1., 1., 1.)], slope),
        (vec![c(-1., -1., -1.), c(-1., -1., 1.), c(-1., 1., 1.)], V3::new(-1., 0., 0.)),
        (vec![c(1., -1., -1.), c(1., -1., 1.), c(1., 1., 1.)], V3::new(1., 0., 0.)),
    ];
    place(faces.iter().filter_map(|(cs, n)| face(cs, *n, None)).collect(), frame)
}

/// A square base with its top at the +X, -Z corner.
pub fn corner_wedge(size: V3, frame: &Frame) -> Vec<Polygon> {
    let h = size * 0.5;
    let c = |x: f64, y: f64, z: f64| V3::new(x * h.x, y * h.y, z * h.z);
    let top = c(1., 1., -1.);
    let base = [c(-1., -1., -1.), c(1., -1., -1.), c(1., -1., 1.), c(-1., -1., 1.)];
    let mut polys: Vec<Polygon> = face(&base, V3::new(0., -1., 0.), None).into_iter().collect();
    let centre = V3::new(h.x * 0.2, -h.y * 0.2, -h.z * 0.2);
    for i in 0..4 {
        let (a, b) = (base[i], base[(i + 1) % 4]);
        let n = (b - a).cross(top - a).normalize();
        let out = if n.dot(a - centre) >= 0.0 { n } else { -n };
        polys.extend(face(&[a, b, top], out, None));
    }
    place(polys, frame)
}

/// Round along X, as wide as the smaller of its other two sides.
pub fn cylinder(size: V3, frame: &Frame) -> Vec<Polygon> {
    const SIDES: usize = 32;
    let r = size.y.min(size.z) * 0.5;
    let hx = size.x * 0.5;
    let ring: Vec<V3> = (0..SIDES)
        .map(|i| {
            let a = i as f64 / SIDES as f64 * std::f64::consts::TAU;
            V3::new(0.0, a.cos(), a.sin())
        })
        .collect();
    let mut polys = Vec::new();
    let cap = |x: f64| ring.iter().map(|d| V3::new(x, d.y * r, d.z * r)).collect::<Vec<_>>();
    polys.extend(face(&cap(hx), V3::new(1., 0., 0.), None));
    polys.extend(face(&cap(-hx), V3::new(-1., 0., 0.), None));
    for i in 0..SIDES {
        let (d0, d1) = (ring[i], ring[(i + 1) % SIDES]);
        let corners = [
            V3::new(-hx, d0.y * r, d0.z * r),
            V3::new(hx, d0.y * r, d0.z * r),
            V3::new(hx, d1.y * r, d1.z * r),
            V3::new(-hx, d1.y * r, d1.z * r),
        ];
        let mid = (d0 + d1).normalize();
        polys.extend(face(&corners, mid, Some(&[d0, d0, d1, d1])));
    }
    place(polys, frame)
}

/// As wide as its smallest side.
pub fn ball(size: V3, frame: &Frame) -> Vec<Polygon> {
    const AROUND: usize = 24;
    const UP: usize = 12;
    let r = size.x.min(size.y).min(size.z) * 0.5;
    let dir = |i: usize, j: usize| {
        let a = i as f64 / AROUND as f64 * std::f64::consts::TAU;
        let b = j as f64 / UP as f64 * std::f64::consts::PI;
        V3::new(b.sin() * a.cos(), b.cos(), b.sin() * a.sin())
    };
    let mut polys = Vec::new();
    for j in 0..UP {
        for i in 0..AROUND {
            let mut ds = vec![dir(i, j), dir(i + 1, j), dir(i + 1, j + 1), dir(i, j + 1)];
            ds.dedup_by(|a, b| (*a - *b).length() < 1e-9);
            if (ds[0] - ds[ds.len() - 1]).length() < 1e-9 {
                ds.pop();
            }
            if ds.len() < 3 {
                continue;
            }
            let corners: Vec<V3> = ds.iter().map(|d| *d * r).collect();
            let mid = ds.iter().fold(V3::default(), |s, d| s + *d).normalize();
            polys.extend(face(&corners, mid, Some(&ds)));
        }
    }
    place(polys, frame)
}

/// Triangles from a model, stretched by `scale` then placed, facing out.
pub fn from_triangles(
    positions: &[V3],
    normals: Option<&[V3]>,
    corners: &[u32],
    scale: V3,
    frame: &Frame,
) -> Vec<Polygon> {
    let polys: Vec<Polygon> = corners
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|tri| {
            let vertices = tri
                .iter()
                .map(|&i| {
                    let pos = *positions.get(i as usize)?;
                    Some(Vertex {
                        pos,
                        normal: normals.and_then(|n| n.get(i as usize)).copied().unwrap_or_default(),
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            let mut p = Polygon::new(vertices)?;
            if normals.is_none() {
                for v in &mut p.vertices {
                    v.normal = p.plane.normal;
                }
            }
            Some(p)
        })
        .collect();
    let mut polys = transform(polys, scale, frame);
    if volume(&polys) < 0.0 {
        for p in &mut polys {
            p.vertices.reverse();
            p.plane.flip();
            if normals.is_none() {
                for v in &mut p.vertices {
                    v.normal = -v.normal;
                }
            }
        }
    }
    polys
}

/// Stretched by `scale`, then placed.
pub fn transform(polys: Vec<Polygon>, scale: V3, frame: &Frame) -> Vec<Polygon> {
    let inv = V3::new(1.0 / scale.x, 1.0 / scale.y, 1.0 / scale.z);
    polys
        .into_iter()
        .filter_map(|p| {
            let vertices = p
                .vertices
                .iter()
                .map(|v| Vertex {
                    pos: frame.point(v.pos.mul_each(scale)),
                    normal: frame.turn(v.normal.mul_each(inv)).normalize(),
                })
                .collect();
            let mut out = Polygon::new(vertices)?;
            if out.plane.normal.dot(frame.turn(p.plane.normal.mul_each(inv))) < 0.0 {
                out.plane.flip();
            }
            Some(out)
        })
        .collect()
}

pub fn volume(polys: &[Polygon]) -> f64 {
    polys
        .iter()
        .flat_map(|p| {
            let a = p.vertices[0].pos;
            (1..p.vertices.len() - 1)
                .map(move |i| a.dot(p.vertices[i].pos.cross(p.vertices[i + 1].pos)) / 6.0)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_cut_from_a_block() {
        let a = block(V3::new(2.0, 2.0, 2.0), &Frame::IDENTITY);
        assert!((volume(&a) - 8.0).abs() < 1e-9);
        let mut moved = Frame::IDENTITY;
        moved.at = V3::new(1.0, 0.0, 0.0);
        let b = block(V3::new(2.0, 2.0, 2.0), &moved);
        assert!((volume(&subtract_all(a.clone(), std::slice::from_ref(&b))) - 4.0).abs() < 1e-6);
    }

    #[test]
    fn shapes_are_closed_and_face_out() {
        let s = V3::new(2.0, 2.0, 2.0);
        let f = Frame::IDENTITY;
        assert!((volume(&wedge(s, &f)) - 4.0).abs() < 1e-9);
        assert!((volume(&corner_wedge(s, &f)) - 8.0 / 3.0).abs() < 1e-9);
        assert!((volume(&cylinder(s, &f)) - std::f64::consts::PI * 2.0).abs() < 0.1);
        assert!((volume(&ball(s, &f)) - 4.0 / 3.0 * std::f64::consts::PI).abs() < 0.2);
    }

    #[test]
    fn a_cut_flush_with_the_sides_leaves_two_slabs() {
        let a = block(V3::new(4.0, 16.0, 16.0), &Frame::IDENTITY);
        let b = block(V3::new(4.0, 12.0, 16.0), &Frame::IDENTITY);
        let out = subtract_all(a, &[b]);
        let (lo, hi) = bounds(&out).unwrap();
        assert!((hi - lo - V3::new(4.0, 16.0, 16.0)).length() < 1e-6, "{lo:?} {hi:?}");
        assert!((volume(&out) - 4.0 * 4.0 * 16.0).abs() < 1e-6, "{}", volume(&out));
    }

    #[test]
    fn a_ring_from_two_cylinders() {
        let a = cylinder(V3::new(4.0, 16.0, 16.0), &Frame::IDENTITY);
        let b = cylinder(V3::new(4.0, 12.0, 16.0), &Frame::IDENTITY);
        let (lo, hi) = bounds(&a).unwrap();
        assert!((hi - lo - V3::new(4.0, 16.0, 16.0)).length() < 1e-6, "{lo:?} {hi:?}");
        let out = subtract_all(a, &[b]);
        let (lo, hi) = bounds(&out).unwrap();
        assert!((hi - lo - V3::new(4.0, 16.0, 16.0)).length() < 1e-6, "{lo:?} {hi:?}");
        let ring = 16.0 * (2.0 * std::f64::consts::PI / 32.0).sin() * (64.0 - 36.0) * 4.0;
        assert!((volume(&out) - ring).abs() < 1e-6, "{}", volume(&out));
    }
}
