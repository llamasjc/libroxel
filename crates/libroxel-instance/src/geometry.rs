//! Ray tests against the parts and terrain in a tree, for picking and
//! script raycasts.

use crate::{ClassKind, EnumValue, InstanceId, InstanceTree, PartShape, Property, Rotation, Vec3};

/// Where a ray met a part.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    pub part: InstanceId,
    pub position: Vec3,
    /// The outward surface normal at the hit, in world space.
    pub normal: Vec3,
    pub distance: f32,
}

/// Casts a ray from `origin` along `direction` (whose length is the ray's
/// reach) against every part in the workspace that `keep` accepts, and
/// returns the nearest hit. Terrain is hit too (its `part` is the
/// terrain) if `keep` accepts it. Parts the ray starts inside are not hit.
pub fn raycast(
    tree: &InstanceTree,
    origin: Vec3,
    direction: Vec3,
    keep: impl Fn(InstanceId) -> bool,
) -> Option<RayHit> {
    let reach = direction.length();
    if reach <= 0.0 {
        return None;
    }
    let dir = direction / reach;
    let mut best: Option<RayHit> = None;
    for part in tree.descendants_of_class(tree.workspace(), ClassKind::Part) {
        if !keep(part) {
            continue;
        }
        let (Some(pos), Some(size)) =
            (tree.get_vec3(part, Property::Position), tree.get_vec3(part, Property::Size))
        else {
            continue;
        };
        let shape = match tree.get_enum(part, Property::Shape) {
            Some(EnumValue::Shape(s)) => s,
            _ => PartShape::Block,
        };
        let rot = Rotation::from_orientation(tree.get_vec3(part, Property::Orientation).unwrap_or_default());
        let inv = rot.transpose();
        let local_origin = inv.apply(origin - pos);
        let local_dir = inv.apply(dir);
        let Some((t, local_normal)) = ray_shape(shape, size / 2.0, local_origin, local_dir) else { continue };
        if t > reach || best.is_some_and(|b| b.distance <= t) {
            continue;
        }
        best =
            Some(RayHit { part, position: origin + dir * t, normal: rot.apply(local_normal), distance: t });
    }
    if let Some(terrain) = tree.terrain().filter(|t| keep(*t))
        && let Ok(voxels) = tree.voxels(terrain)
        && let Some((t, normal, _)) = voxels.raycast(origin, dir, best.map_or(reach, |b| b.distance))
    {
        best = Some(RayHit { part: terrain, position: origin + dir * t, normal, distance: t });
    }
    best
}

/// A ray against a shape centred on the origin: the distance and the normal.
fn ray_shape(shape: PartShape, half: Vec3, o: Vec3, d: Vec3) -> Option<(f32, Vec3)> {
    match shape {
        PartShape::Ball => ray_sphere(half.x.min(half.y).min(half.z), o, d),
        PartShape::Cylinder => ray_cylinder(half.x, half.y.min(half.z), o, d),
        PartShape::Block => ray_planes(&box_planes(half), o, d),
        PartShape::Wedge => {
            let mut planes = box_planes(half).to_vec();
            // Drop the top and front faces; the slope replaces both.
            planes.retain(|(n, _)| *n != Vec3::new(0.0, 1.0, 0.0) && *n != Vec3::new(0.0, 0.0, -1.0));
            let slope = Vec3::new(0.0, half.z, -half.y).normalize_or_zero();
            planes.push((slope, 0.0));
            ray_planes(&planes, o, d)
        }
    }
}

/// The six faces of a box as (outward normal, offset) with `n · p <= offset` inside.
fn box_planes(half: Vec3) -> [(Vec3, f32); 6] {
    [
        (Vec3::new(1.0, 0.0, 0.0), half.x),
        (Vec3::new(-1.0, 0.0, 0.0), half.x),
        (Vec3::new(0.0, 1.0, 0.0), half.y),
        (Vec3::new(0.0, -1.0, 0.0), half.y),
        (Vec3::new(0.0, 0.0, 1.0), half.z),
        (Vec3::new(0.0, 0.0, -1.0), half.z),
    ]
}

/// A ray against the convex shape where every `n · p <= offset`.
fn ray_planes(planes: &[(Vec3, f32)], o: Vec3, d: Vec3) -> Option<(f32, Vec3)> {
    let (mut near, mut far) = (f32::NEG_INFINITY, f32::INFINITY);
    let mut normal = Vec3::ZERO;
    for &(n, offset) in planes {
        let denom = n.dot(d);
        let dist = n.dot(o) - offset;
        if denom.abs() < 1e-9 {
            if dist > 0.0 {
                return None;
            }
            continue;
        }
        let t = -dist / denom;
        if denom < 0.0 {
            if t > near {
                near = t;
                normal = n;
            }
        } else {
            far = far.min(t);
        }
    }
    (near <= far && near >= 0.0).then_some((near, normal))
}

fn ray_sphere(radius: f32, o: Vec3, d: Vec3) -> Option<(f32, Vec3)> {
    let b = o.dot(d);
    let c = o.dot(o) - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 || c < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    (t >= 0.0).then(|| (t, (o + d * t).normalize_or_zero()))
}

/// A cylinder along X with half-length `half_len`.
fn ray_cylinder(half_len: f32, radius: f32, o: Vec3, d: Vec3) -> Option<(f32, Vec3)> {
    let inside_round = o.y * o.y + o.z * o.z <= radius * radius;
    if inside_round && o.x.abs() <= half_len {
        return None;
    }
    let mut best: Option<(f32, Vec3)> = None;
    let mut consider = |t: f32, n: Vec3| {
        if t >= 0.0 && best.is_none_or(|(bt, _)| t < bt) {
            best = Some((t, n));
        }
    };
    // The round side.
    let a = d.y * d.y + d.z * d.z;
    if a > 1e-12 {
        let b = o.y * d.y + o.z * d.z;
        let c = o.y * o.y + o.z * o.z - radius * radius;
        let disc = b * b - a * c;
        if disc >= 0.0 {
            let t = (-b - disc.sqrt()) / a;
            let p = o + d * t;
            if p.x.abs() <= half_len {
                consider(t, Vec3::new(0.0, p.y, p.z).normalize_or_zero());
            }
        }
    }
    // The flat ends.
    if d.x.abs() > 1e-12 {
        for side in [-1.0f32, 1.0] {
            let t = (side * half_len - o.x) / d.x;
            let p = o + d * t;
            if p.y * p.y + p.z * p.z <= radius * radius {
                consider(t, Vec3::new(side, 0.0, 0.0));
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PropertyValue;

    fn part(
        tree: &mut InstanceTree,
        pos: Vec3,
        size: Vec3,
        shape: PartShape,
        orientation: Vec3,
    ) -> InstanceId {
        let id = tree.create(ClassKind::Part, "P").unwrap();
        tree.set_parent(id, Some(tree.workspace())).unwrap();
        tree.set(id, Property::Position, PropertyValue::Vec3(pos)).unwrap();
        tree.set(id, Property::Size, PropertyValue::Vec3(size)).unwrap();
        tree.set(id, Property::Shape, PropertyValue::Enum(EnumValue::Shape(shape))).unwrap();
        tree.set(id, Property::Orientation, PropertyValue::Vec3(orientation)).unwrap();
        id
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-3
    }

    #[test]
    fn rays_hit_each_shape_with_the_right_normal() {
        let mut tree = InstanceTree::new();
        let floor = part(&mut tree, Vec3::ZERO, Vec3::new(10.0, 2.0, 10.0), PartShape::Block, Vec3::ZERO);
        let down = Vec3::new(0.0, -50.0, 0.0);
        let hit = raycast(&tree, Vec3::new(1.0, 10.0, 1.0), down, |_| true).unwrap();
        assert_eq!(hit.part, floor);
        assert!(near(hit.position, Vec3::new(1.0, 1.0, 1.0)), "{hit:?}");
        assert!(near(hit.normal, Vec3::new(0.0, 1.0, 0.0)));
        assert!((hit.distance - 9.0).abs() < 1e-4);
        // Too short to reach, or filtered out.
        assert!(raycast(&tree, Vec3::new(1.0, 10.0, 1.0), Vec3::new(0.0, -5.0, 0.0), |_| true).is_none());
        assert!(raycast(&tree, Vec3::new(1.0, 10.0, 1.0), down, |p| p != floor).is_none());
        // Starting inside a part doesn't hit it.
        assert!(raycast(&tree, Vec3::ZERO, down, |_| true).is_none());

        let ball =
            part(&mut tree, Vec3::new(20.0, 0.0, 0.0), Vec3::new(4.0, 4.0, 4.0), PartShape::Ball, Vec3::ZERO);
        let hit = raycast(&tree, Vec3::new(20.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -20.0), |_| true).unwrap();
        assert_eq!(hit.part, ball);
        assert!(near(hit.position, Vec3::new(20.0, 0.0, 2.0)) && near(hit.normal, Vec3::new(0.0, 0.0, 1.0)));

        // A wedge: the slope faces up and towards -Z.
        let wedge = part(
            &mut tree,
            Vec3::new(40.0, 0.0, 0.0),
            Vec3::new(2.0, 2.0, 2.0),
            PartShape::Wedge,
            Vec3::ZERO,
        );
        let hit = raycast(&tree, Vec3::new(40.0, 5.0, 0.0), down, |_| true).unwrap();
        assert_eq!(hit.part, wedge);
        assert!(near(hit.position, Vec3::new(40.0, 0.0, 0.0)), "{hit:?}");
        let s = std::f32::consts::FRAC_1_SQRT_2;
        assert!(near(hit.normal, Vec3::new(0.0, s, -s)), "{hit:?}");

        // A cylinder along X, turned to stand upright.
        let cyl = part(
            &mut tree,
            Vec3::new(60.0, 0.0, 0.0),
            Vec3::new(4.0, 2.0, 2.0),
            PartShape::Cylinder,
            Vec3::new(0.0, 0.0, 90.0),
        );
        let hit = raycast(&tree, Vec3::new(60.0, 10.0, 0.0), down, |_| true).unwrap();
        assert_eq!(hit.part, cyl);
        assert!(near(hit.position, Vec3::new(60.0, 2.0, 0.0)), "{hit:?}");
        assert!(near(hit.normal, Vec3::new(0.0, 1.0, 0.0)), "{hit:?}");
    }
}
