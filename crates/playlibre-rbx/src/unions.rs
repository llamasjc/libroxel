//! A union's shape rebuilt from what the file keeps of it: its drawn
//! shape, the parts it was made from, or its collision pieces.

use rbx_dom_weak::Instance;
use rbx_dom_weak::types::Variant;

use crate::csg::{self, Frame, Polygon, V3};
use crate::meshes::{self, Triangles};

const DEEPEST: u32 = 8;
const MOST: usize = 200_000;

/// Closed pieces that together make a shape.
type Solid = Vec<Vec<Polygon>>;

fn bytes<'a>(inst: &'a Instance, names: &[&str]) -> Option<&'a [u8]> {
    names.iter().find_map(|n| match inst.properties.get(&(*n).into()) {
        Some(Variant::BinaryString(b)) => Some(<_ as AsRef<[u8]>>::as_ref(b)).filter(|b| !b.is_empty()),
        Some(Variant::SharedString(s)) => Some(s.data()).filter(|b| !b.is_empty()),
        _ => None,
    })
}

fn vec3(inst: &Instance, name: &str) -> Option<V3> {
    match inst.properties.get(&name.into()) {
        Some(Variant::Vector3(v)) => Some(V3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z))),
        _ => None,
    }
}

fn frame(inst: &Instance) -> Frame {
    match inst.properties.get(&"CFrame".into()) {
        Some(Variant::CFrame(cf)) => {
            let m = cf.orientation;
            let v = |x: f32, y: f32, z: f32| V3::new(f64::from(x), f64::from(y), f64::from(z));
            Frame {
                x: v(m.x.x, m.y.x, m.z.x),
                y: v(m.x.y, m.y.y, m.z.y),
                z: v(m.x.z, m.y.z, m.z.z),
                at: v(cf.position.x, cf.position.y, cf.position.z),
            }
        }
        _ => Frame::IDENTITY,
    }
}

/// How much `inst` was stretched since it was made.
fn stretched(inst: &Instance, size: V3) -> V3 {
    let initial = vec3(inst, "InitialSize").filter(|s| s.x > 0.0 && s.y > 0.0 && s.z > 0.0).unwrap_or(size);
    V3::new(size.x / initial.x, size.y / initial.y, size.z / initial.z)
}

/// The drawn shape kept in the file, for a union made in an older version.
pub fn graphics(inst: &Instance) -> Option<Triangles> {
    meshes::union_graphics(bytes(inst, &["MeshData", "MeshData2"])?)
}

/// The collision pieces kept in the file.
pub fn physics_bytes(inst: &Instance) -> Option<&[u8]> {
    bytes(inst, &["PhysicsData", "PhysicalConfigData"])
}

/// The shape rebuilt from the parts the union was made from, in its own
/// space at its current size.
pub fn from_parts(inst: &Instance) -> Option<Triangles> {
    let size = vec3(inst, "Size")?;
    let pieces = from_child_data(bytes(inst, &["ChildData2", "ChildData"])?, 0)?;
    let scale = stretched(inst, size);
    let polys: Vec<Polygon> =
        pieces.into_iter().flat_map(|p| csg::transform(p, scale, &Frame::IDENTITY)).collect();
    to_triangles(&polys)
}

/// `inst`'s shape in its own space at its current size.
fn solid(inst: &Instance, depth: u32) -> Option<Solid> {
    if depth > DEEPEST {
        return None;
    }
    let size = vec3(inst, "Size")?;
    let scale = stretched(inst, size);
    let made = graphics(inst)
        .map(|t| vec![triangles(&t, V3::new(1.0, 1.0, 1.0))])
        .or_else(|| bytes(inst, &["ChildData2", "ChildData"]).and_then(|b| from_child_data(b, depth)))
        .or_else(|| {
            let pieces = meshes::union_pieces(physics_bytes(inst)?)?;
            Some(pieces.iter().map(|t| triangles(t, V3::new(1.0, 1.0, 1.0))).collect())
        });
    match made {
        Some(pieces) => {
            Some(pieces.into_iter().map(|p| csg::transform(p, scale, &Frame::IDENTITY)).collect())
        }
        None if inst.class.as_str() == "NegateOperation" => Some(vec![csg::block(size, &Frame::IDENTITY)]),
        None => None,
    }
}

fn from_child_data(data: &[u8], depth: u32) -> Option<Solid> {
    let dom = crate::read_dom(data).ok()?;
    let children: Vec<&Instance> = dom.root().children().iter().filter_map(|r| dom.get_by_ref(*r)).collect();
    let negative = |i: &Instance| i.class.as_str() == "NegateOperation";
    let keep: Solid =
        children.iter().filter(|i| !negative(i)).filter_map(|i| piece(i, depth)).flatten().collect();
    if keep.is_empty() {
        return None;
    }
    let cut: Solid =
        children.iter().filter(|i| negative(i)).filter_map(|i| piece(i, depth)).flatten().collect();
    let mut out = Vec::new();
    let mut count = 0;
    for p in keep {
        let p = csg::subtract_all(p, &cut);
        count += p.len();
        if count > MOST {
            return None;
        }
        if !p.is_empty() {
            out.push(p);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// One of a union's parts, placed in the union's space.
fn piece(inst: &Instance, depth: u32) -> Option<Solid> {
    let class = inst.class.as_str();
    let at = frame(inst);
    if matches!(class, "UnionOperation" | "NegateOperation" | "PartOperation") {
        let local = solid(inst, depth + 1)?;
        return Some(local.into_iter().map(|p| csg::transform(p, V3::new(1.0, 1.0, 1.0), &at)).collect());
    }
    let size = vec3(inst, "Size")?;
    if class == "MeshPart" {
        let scale = stretched(inst, size);
        return Some(match physics_bytes(inst).and_then(meshes::union_pieces) {
            Some(pieces) => pieces
                .iter()
                .map(|t| triangles(t, scale))
                .map(|p| csg::transform(p, V3::new(1.0, 1.0, 1.0), &at))
                .collect(),
            None => vec![csg::block(size, &at)],
        });
    }
    let shape = match class {
        "WedgePart" => 3,
        "CornerWedgePart" => 4,
        _ => match inst.properties.get(&"Shape".into()) {
            Some(Variant::Enum(e)) => e.to_u32(),
            _ => 1,
        },
    };
    Some(vec![match shape {
        0 => csg::ball(size, &at),
        2 => csg::cylinder(size, &at),
        3 => csg::wedge(size, &at),
        4 => csg::corner_wedge(size, &at),
        _ => csg::block(size, &at),
    }])
}

fn triangles(t: &Triangles, scale: V3) -> Vec<Polygon> {
    let positions: Vec<V3> = t.positions.iter().map(|p| V3::from_f32(*p)).collect();
    let normals: Vec<V3> = t.normals.iter().map(|n| V3::from_f32(*n)).collect();
    let normals = (normals.len() == positions.len()).then_some(&normals[..]);
    csg::from_triangles(&positions, normals, &t.corners, scale, &Frame::IDENTITY)
}

fn to_triangles(polys: &[Polygon]) -> Option<Triangles> {
    let mut t = Triangles::default();
    for p in polys {
        let start = t.positions.len() as u32;
        for v in &p.vertices {
            t.positions.push(v.pos.to_f32());
            t.normals.push(v.normal.to_f32());
        }
        for i in 1..p.vertices.len() as u32 - 1 {
            t.corners.extend([start, start + i, start + i + 1]);
        }
    }
    (!t.is_empty()).then_some(t)
}
