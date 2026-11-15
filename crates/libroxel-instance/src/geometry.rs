//! Ray tests against the parts and terrain in a tree, for picking and
//! script raycasts, and overlap tests for finding the parts in a box, a
//! ball or another part.

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
    tree.visit_ray(origin, dir, reach, |part, entered| {
        nearest_hit(tree, part, entered, origin, dir, reach, &keep, &mut best)
    });
    if let Some(terrain) = tree.terrain().filter(|t| keep(*t))
        && let Ok(voxels) = tree.voxels(terrain)
        && let Some((t, normal, _)) = voxels.raycast(origin, dir, best.map_or(reach, |b| b.distance))
    {
        best = Some(RayHit { part: terrain, position: origin + dir * t, normal, distance: t });
    }
    best
}

/// A ray from `origin` along the unit vector `dir` against one part: how
/// far along it hits, and the outward normal there.
pub fn ray_part(tree: &InstanceTree, part: InstanceId, origin: Vec3, dir: Vec3) -> Option<(f32, Vec3)> {
    let pos = tree.get_vec3(part, Property::Position)?;
    let size = tree.get_vec3(part, Property::Size)?;
    let shape = match tree.get_enum(part, Property::Shape) {
        Some(EnumValue::Shape(s)) => s,
        _ => PartShape::Block,
    };
    let rot = Rotation::from_orientation(tree.get_vec3(part, Property::Orientation).unwrap_or_default());
    let inv = rot.transpose();
    let (t, local_normal) = ray_shape(shape, size / 2.0, inv.apply(origin - pos), inv.apply(dir))?;
    Some((t, rot.apply(local_normal)))
}

/// A region to look for parts in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Region {
    /// A box: its middle, how it's turned, and half its size.
    Box { center: Vec3, rot: Rotation, half: Vec3 },
    /// A ball: its middle and radius.
    Ball { center: Vec3, radius: f32 },
}

impl Region {
    /// The smallest upright box around it: its lowest and highest corners.
    pub fn bounds(self) -> (Vec3, Vec3) {
        let reach = match self {
            Region::Box { rot, half, .. } => {
                let abs = |v: Vec3| Vec3::new(v.x.abs(), v.y.abs(), v.z.abs());
                abs(rot.x) * half.x + abs(rot.y) * half.y + abs(rot.z) * half.z
            }
            Region::Ball { radius, .. } => Vec3::new(radius, radius, radius),
        };
        let center = match self {
            Region::Box { center, .. } | Region::Ball { center, .. } => center,
        };
        (center - reach, center + reach)
    }

    /// The space `part` takes up: a ball for a ball part, else its box.
    pub fn of_part(tree: &InstanceTree, part: InstanceId) -> Option<Region> {
        let center = tree.get_vec3(part, Property::Position)?;
        let size = tree.get_vec3(part, Property::Size)?;
        if matches!(tree.get_enum(part, Property::Shape), Some(EnumValue::Shape(PartShape::Ball))) {
            return Some(Region::Ball { center, radius: size.x.min(size.y).min(size.z) / 2.0 });
        }
        let rot = Rotation::from_orientation(tree.get_vec3(part, Property::Orientation).unwrap_or_default());
        Some(Region::Box { center, rot, half: size / 2.0 })
    }

    /// The same region grown by `margin` on every side.
    pub fn grown(self, margin: f32) -> Region {
        match self {
            Region::Box { center, rot, half } => {
                Region::Box { center, rot, half: half + Vec3::new(margin, margin, margin) }
            }
            Region::Ball { center, radius } => Region::Ball { center, radius: radius + margin },
        }
    }

    /// Whether the two regions overlap (touching counts).
    pub fn overlaps(self, other: Region) -> bool {
        match (self, other) {
            (Region::Ball { center: a, radius: ra }, Region::Ball { center: b, radius: rb }) => {
                (a - b).length() <= ra + rb
            }
            (Region::Box { center, rot, half }, Region::Ball { center: c, radius })
            | (Region::Ball { center: c, radius }, Region::Box { center, rot, half }) => {
                // The nearest point of the box to the ball's middle.
                let local = rot.transpose().apply(c - center);
                let nearest = Vec3::new(
                    local.x.clamp(-half.x, half.x),
                    local.y.clamp(-half.y, half.y),
                    local.z.clamp(-half.z, half.z),
                );
                (local - nearest).length() <= radius
            }
            (
                Region::Box { center: ca, rot: ra, half: ha },
                Region::Box { center: cb, rot: rb, half: hb },
            ) => boxes_overlap(ca, ra, ha, cb, rb, hb),
        }
    }
}

/// Two turned boxes overlap unless some axis separates them: one of either
/// box's three axes, or a cross product of one axis from each.
fn boxes_overlap(ca: Vec3, ra: Rotation, ha: Vec3, cb: Vec3, rb: Rotation, hb: Vec3) -> bool {
    let a = [ra.x, ra.y, ra.z];
    let b = [rb.x, rb.y, rb.z];
    let (ea, eb) = ([ha.x, ha.y, ha.z], [hb.x, hb.y, hb.z]);
    let gap = cb - ca;
    let separates = |axis: Vec3| {
        let length = axis.length();
        if length < 1e-6 {
            return false;
        }
        let axis = axis / length;
        let reach =
            |axes: &[Vec3; 3], e: &[f32; 3]| (0..3).map(|i| e[i] * axes[i].dot(axis).abs()).sum::<f32>();
        gap.dot(axis).abs() > reach(&a, &ea) + reach(&b, &eb) + 1e-4
    };
    let mut axes = a.to_vec();
    axes.extend(b);
    for i in a {
        for j in b {
            axes.push(i.cross(j));
        }
    }
    !axes.into_iter().any(separates)
}

/// Every part in the workspace that `keep` accepts and that overlaps
/// `region`, judged by its box (or ball), at most `limit` of them.
pub fn parts_in(
    tree: &InstanceTree,
    region: Region,
    limit: usize,
    keep: impl Fn(InstanceId) -> bool,
) -> Vec<InstanceId> {
    let mut found = Vec::new();
    for part in tree.descendants_of_class(tree.workspace(), ClassKind::Part) {
        if found.len() >= limit {
            break;
        }
        if keep(part) && Region::of_part(tree, part).is_some_and(|r| r.overlaps(region)) {
            found.push(part);
        }
    }
    found
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
    fn the_part_index_finds_what_a_full_search_finds_as_parts_move() {
        let mut tree = InstanceTree::new();
        let mut parts = Vec::new();
        for i in 0..400 {
            let f = i as f32;
            let pos = Vec3::new((f * 37.0) % 300.0 - 150.0, (f * 13.0) % 60.0, (f * 71.0) % 300.0 - 150.0);
            let size = Vec3::new(1.0 + f % 7.0, 1.0 + f % 3.0, 2.0 + f % 5.0);
            parts.push(part(&mut tree, pos, size, PartShape::Block, Vec3::new(0.0, f % 90.0, 0.0)));
        }
        let floor = part(
            &mut tree,
            Vec3::new(0.0, -5.0, 0.0),
            Vec3::new(5000.0, 2.0, 5000.0),
            PartShape::Block,
            Vec3::ZERO,
        );
        let mut index = PartIndex::new();
        let check = |tree: &InstanceTree, index: &mut PartIndex| {
            index.refresh(tree);
            for i in 0..200 {
                let f = i as f32;
                let origin = Vec3::new((f * 17.0) % 320.0 - 160.0, 80.0, (f * 29.0) % 320.0 - 160.0);
                let dir = Vec3::new((f * 0.37).sin() * 40.0, -100.0, (f * 0.53).cos() * 40.0);
                let full = raycast(tree, origin, dir, |_| true).map(|h| h.part);
                let fast = raycast_indexed(tree, index, origin, dir, |_| true).map(|h| h.part);
                assert_eq!(full, fast, "ray {i}");
                let region = Region::Ball { center: origin - Vec3::new(0.0, 60.0, 0.0), radius: 20.0 };
                let mut a = parts_in(tree, region, usize::MAX, |_| true);
                let mut b = parts_in_indexed(tree, index, region, usize::MAX, |_| true);
                a.sort_by_key(|p| p.to_bits());
                b.sort_by_key(|p| p.to_bits());
                assert_eq!(a, b, "region {i}");
            }
        };
        check(&tree, &mut index);
        // Move half the parts, destroy some, and add a model of new ones.
        for (i, p) in parts.iter().enumerate().step_by(2) {
            let f = i as f32;
            tree.set(
                *p,
                Property::Position,
                PropertyValue::Vec3(Vec3::new(f % 200.0 - 100.0, 5.0, (f * 3.0) % 200.0 - 100.0)),
            )
            .unwrap();
        }
        for p in parts.iter().skip(1).step_by(5) {
            tree.destroy(*p).unwrap();
        }
        let model = tree.create(ClassKind::Model, "M").unwrap();
        for i in 0..20 {
            let p = tree.create(ClassKind::Part, "Q").unwrap();
            tree.set(p, Property::Position, PropertyValue::Vec3(Vec3::new(i as f32 * 9.0 - 90.0, 10.0, 0.0)))
                .unwrap();
            tree.set(p, Property::Size, PropertyValue::Vec3(Vec3::new(4.0, 4.0, 4.0))).unwrap();
            tree.set_parent(p, Some(model)).unwrap();
        }
        tree.set_parent(model, Some(tree.workspace())).unwrap();
        check(&tree, &mut index);
        tree.set_parent(model, None).unwrap();
        tree.destroy(floor).unwrap();
        check(&tree, &mut index);
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

/// Where the workspace's parts are, in a grid of cells, so a ray or a box
/// only looks at the parts near it instead of every part in a big world.
/// It catches up with the tree's moved log each time it is used.
#[derive(Debug, Default, Clone)]
pub struct PartIndex {
    built: bool,
    epoch: u64,
    read: usize,
    cells: foldhash::HashMap<(i32, i32, i32), Vec<InstanceId>>,
    /// Parts too big for the grid, checked by every query.
    huge: foldhash::HashSet<InstanceId>,
    /// The cells each part was put in (none for a huge part).
    placed: foldhash::HashMap<InstanceId, Option<CellRange>>,
    /// The parts that aren't anchored (the ones that can move by themselves).
    loose: foldhash::HashSet<InstanceId>,
}

/// A box of grid cells: its lowest and highest corners.
type CellRange = ((i32, i32, i32), (i32, i32, i32));

const INDEX_CELL: f32 = 16.0;
/// A part over more cells than this goes in the huge list instead.
const INDEX_MAX_CELLS: i64 = 512;

fn index_cell(v: Vec3) -> (i32, i32, i32) {
    let c = |x: f32| (x / INDEX_CELL).floor().clamp(-1.0e9, 1.0e9) as i32;
    (c(v.x), c(v.y), c(v.z))
}

impl PartIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Brings the index in line with the tree.
    pub fn refresh(&mut self, tree: &InstanceTree) {
        let (epoch, _) = tree.moved_since(self.read);
        if !self.built || epoch != self.epoch {
            self.cells.clear();
            self.huge.clear();
            self.placed.clear();
            self.loose.clear();
            for part in tree.descendants_of_class(tree.workspace(), ClassKind::Part) {
                self.place(tree, part);
            }
            self.built = true;
            self.epoch = tree.moved_epoch();
            self.read = tree.moved_len();
            return;
        }
        let (_, moved) = tree.moved_since(self.read);
        let moved = moved.to_vec();
        self.read = tree.moved_len();
        let mut seen = foldhash::HashSet::default();
        for id in moved {
            if !seen.insert(id) {
                continue;
            }
            self.update(tree, id);
            // A model moved in or out of the world takes its parts along.
            if tree.class(id).is_ok_and(|c| !c.is_a(ClassKind::Part))
                || !tree.children(id).is_ok_and(|c| c.is_empty())
            {
                for d in tree.descendants(id).unwrap_or_default() {
                    if seen.insert(d) {
                        self.update(tree, d);
                    }
                }
            }
        }
    }

    fn update(&mut self, tree: &InstanceTree, id: InstanceId) {
        let live = tree.exists(id) && !tree.is_destroyed(id).unwrap_or(true);
        if live
            && tree.class(id).is_ok_and(|c| c.is_a(ClassKind::Part))
            && tree.is_descendant_of(id, tree.workspace()).unwrap_or(false)
        {
            // A part that moved a little usually stays in the same cells:
            // nothing to do then (animated machines move thousands a frame).
            let loose = tree.get_bool(id, Property::Anchored) == Some(false);
            if let Some(&placed) = self.placed.get(&id)
                && placed.is_some()
                && placed == self.cells_for(tree, id)
                && self.loose.contains(&id) == loose
            {
                return;
            }
            self.remove(id);
            self.place(tree, id);
        } else {
            self.remove(id);
        }
    }

    /// The cells a part covers, or None for one too big (or far) for cells.
    fn cells_for(&self, tree: &InstanceTree, part: InstanceId) -> Option<CellRange> {
        let (pos, size) = (tree.get_vec3(part, Property::Position)?, tree.get_vec3(part, Property::Size)?);
        let r = size.length() * 0.5;
        if !(pos.x.is_finite() && pos.y.is_finite() && pos.z.is_finite() && r.is_finite()) {
            return None;
        }
        let reach = Vec3::new(r, r, r);
        let (lo, hi) = (index_cell(pos - reach), index_cell(pos + reach));
        let count = (i64::from(hi.0) - i64::from(lo.0) + 1)
            * (i64::from(hi.1) - i64::from(lo.1) + 1)
            * (i64::from(hi.2) - i64::from(lo.2) + 1);
        (count <= INDEX_MAX_CELLS).then_some((lo, hi))
    }

    fn remove(&mut self, id: InstanceId) {
        self.loose.remove(&id);
        match self.placed.remove(&id) {
            Some(Some((lo, hi))) => {
                for x in lo.0..=hi.0 {
                    for y in lo.1..=hi.1 {
                        for z in lo.2..=hi.2 {
                            if let Some(list) = self.cells.get_mut(&(x, y, z)) {
                                list.retain(|p| *p != id);
                                if list.is_empty() {
                                    self.cells.remove(&(x, y, z));
                                }
                            }
                        }
                    }
                }
            }
            Some(None) => {
                self.huge.remove(&id);
            }
            None => {}
        }
    }

    fn place(&mut self, tree: &InstanceTree, part: InstanceId) {
        let (Some(pos), Some(size)) =
            (tree.get_vec3(part, Property::Position), tree.get_vec3(part, Property::Size))
        else {
            return;
        };
        if tree.get_bool(part, Property::Anchored) == Some(false) {
            self.loose.insert(part);
        }
        let r = size.length() * 0.5;
        if !(pos.x.is_finite() && pos.y.is_finite() && pos.z.is_finite() && r.is_finite()) {
            self.huge.insert(part);
            self.placed.insert(part, None);
            return;
        }
        let reach = Vec3::new(r, r, r);
        let (lo, hi) = (index_cell(pos - reach), index_cell(pos + reach));
        let count = (i64::from(hi.0) - i64::from(lo.0) + 1)
            * (i64::from(hi.1) - i64::from(lo.1) + 1)
            * (i64::from(hi.2) - i64::from(lo.2) + 1);
        if count > INDEX_MAX_CELLS {
            self.huge.insert(part);
            self.placed.insert(part, None);
            return;
        }
        for x in lo.0..=hi.0 {
            for y in lo.1..=hi.1 {
                for z in lo.2..=hi.2 {
                    self.cells.entry((x, y, z)).or_default().push(part);
                }
            }
        }
        self.placed.insert(part, Some((lo, hi)));
    }

    /// How many parts the index holds.
    pub fn len(&self) -> usize {
        self.placed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.placed.is_empty()
    }

    /// The parts that aren't anchored.
    pub fn loose(&self) -> impl Iterator<Item = InstanceId> + '_ {
        self.loose.iter().copied()
    }

    /// The parts that might be in the box from `lo` to `hi`.
    pub fn near_box(&self, lo: Vec3, hi: Vec3) -> Vec<InstanceId> {
        let (a, b) = (index_cell(lo), index_cell(hi));
        let count = (i64::from(b.0) - i64::from(a.0) + 1)
            * (i64::from(b.1) - i64::from(a.1) + 1)
            * (i64::from(b.2) - i64::from(a.2) + 1);
        let mut out: Vec<InstanceId> = self.huge.iter().copied().collect();
        if count > 4096 {
            // A very big box: every part is a candidate.
            out.extend(self.placed.iter().filter(|(_, p)| p.is_some()).map(|(id, _)| *id));
            return out;
        }
        let mut seen = std::collections::HashSet::new();
        for x in a.0..=b.0 {
            for y in a.1..=b.1 {
                for z in a.2..=b.2 {
                    if let Some(list) = self.cells.get(&(x, y, z)) {
                        out.extend(list.iter().copied().filter(|p| seen.insert(*p)));
                    }
                }
            }
        }
        out
    }

    /// The parts that might be in any of the boxes (each `(lo, hi)`), each
    /// part once. Boxes that overlap (a character's parts, a crowd) look
    /// each cell up only once.
    pub fn near_boxes(&self, boxes: &[(Vec3, Vec3)]) -> Vec<InstanceId> {
        let mut cells = std::collections::HashSet::new();
        for (lo, hi) in boxes {
            let (a, b) = (index_cell(*lo), index_cell(*hi));
            let count = (i64::from(b.0) - i64::from(a.0) + 1)
                * (i64::from(b.1) - i64::from(a.1) + 1)
                * (i64::from(b.2) - i64::from(a.2) + 1);
            if count > 4096 {
                // A very big box: every part is a candidate anyway.
                return self.near_box(*lo, *hi);
            }
            for x in a.0..=b.0 {
                for y in a.1..=b.1 {
                    for z in a.2..=b.2 {
                        cells.insert((x, y, z));
                    }
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<InstanceId> = self.huge.iter().copied().filter(|p| seen.insert(*p)).collect();
        for cell in cells {
            if let Some(list) = self.cells.get(&cell) {
                out.extend(list.iter().copied().filter(|p| seen.insert(*p)));
            }
        }
        out
    }

    /// The parts that might be along the ray from `origin` along the unit
    /// vector `dir` for `reach` studs, roughly nearest first.
    pub fn along_ray(&self, origin: Vec3, dir: Vec3, reach: f32) -> Vec<InstanceId> {
        let mut out = Vec::new();
        self.visit_ray(origin, dir, reach, |part, _| {
            out.push(part);
            true
        });
        out
    }

    /// Hands `visit` each part that might be along the ray (as
    /// [`along_ray`](Self::along_ray)), with how far along the ray the cell
    /// it was found in starts, nearest cells first. Stops once `visit`
    /// returns false. A part is in every cell it overlaps, so once a hit is
    /// nearer than where a cell starts, nothing further on can be nearer.
    pub fn visit_ray(
        &self,
        origin: Vec3,
        dir: Vec3,
        reach: f32,
        mut visit: impl FnMut(InstanceId, f32) -> bool,
    ) {
        for part in &self.huge {
            if !visit(*part, 0.0) {
                return;
            }
        }
        let mut seen = std::collections::HashSet::new();
        let reach = reach.min(1.0e6);
        // Walk the cells the ray passes through, in order (a voxel walk).
        let mut cell = index_cell(origin);
        let step = |d: f32| {
            if d > 0.0 {
                1
            } else if d < 0.0 {
                -1
            } else {
                0
            }
        };
        let (sx, sy, sz) = (step(dir.x), step(dir.y), step(dir.z));
        let next_edge = |c: i32, s: i32| (c + i32::from(s > 0)) as f32 * INDEX_CELL;
        let first =
            |o: f32, d: f32, c: i32, s: i32| if s == 0 { f32::INFINITY } else { (next_edge(c, s) - o) / d };
        let delta = |d: f32| if d == 0.0 { f32::INFINITY } else { INDEX_CELL / d.abs() };
        let (mut tx, mut ty, mut tz) = (
            first(origin.x, dir.x, cell.0, sx),
            first(origin.y, dir.y, cell.1, sy),
            first(origin.z, dir.z, cell.2, sz),
        );
        let (dx, dy, dz) = (delta(dir.x), delta(dir.y), delta(dir.z));
        let max_cells = (reach / INDEX_CELL) as usize * 3 + 4;
        if max_cells > 60000 {
            for (id, _) in self.placed.iter().filter(|(_, p)| p.is_some()) {
                if !visit(*id, 0.0) {
                    return;
                }
            }
            return;
        }
        let mut entered = 0.0;
        for _ in 0..max_cells {
            if let Some(list) = self.cells.get(&cell) {
                for part in list {
                    if seen.insert(*part) && !visit(*part, entered) {
                        return;
                    }
                }
            }
            let t = tx.min(ty).min(tz);
            if t > reach {
                break;
            }
            entered = t;
            if t == tx {
                cell.0 += sx;
                tx += dx;
            } else if t == ty {
                cell.1 += sy;
                ty += dy;
            } else {
                cell.2 += sz;
                tz += dz;
            }
        }
    }
}

/// One step of a ray cast: tries `part` (found in a cell that starts
/// `entered` along the ray) against the nearest hit so far. False once
/// nothing further along can be nearer.
#[allow(clippy::too_many_arguments)]
fn nearest_hit(
    tree: &InstanceTree,
    part: InstanceId,
    entered: f32,
    origin: Vec3,
    dir: Vec3,
    reach: f32,
    keep: &impl Fn(InstanceId) -> bool,
    best: &mut Option<RayHit>,
) -> bool {
    if best.is_some_and(|b| b.distance < entered) {
        return false;
    }
    if !tree.exists(part) || tree.is_destroyed(part).unwrap_or(true) || !keep(part) {
        return true;
    }
    let Some((t, normal)) = ray_part(tree, part, origin, dir) else { return true };
    if t > reach || best.is_some_and(|b| b.distance <= t) {
        return true;
    }
    *best = Some(RayHit { part, position: origin + dir * t, normal, distance: t });
    true
}

/// [`raycast`] with an index of where the parts are.
pub fn raycast_indexed(
    tree: &InstanceTree,
    index: &PartIndex,
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
    index.visit_ray(origin, dir, reach, |part, entered| {
        nearest_hit(tree, part, entered, origin, dir, reach, &keep, &mut best)
    });
    if let Some(terrain) = tree.terrain().filter(|t| keep(*t))
        && let Ok(voxels) = tree.voxels(terrain)
        && let Some((t, normal, _)) = voxels.raycast(origin, dir, best.map_or(reach, |b| b.distance))
    {
        best = Some(RayHit { part: terrain, position: origin + dir * t, normal, distance: t });
    }
    best
}

/// [`parts_in`] with an index of where the parts are.
pub fn parts_in_indexed(
    tree: &InstanceTree,
    index: &PartIndex,
    region: Region,
    limit: usize,
    keep: impl Fn(InstanceId) -> bool,
) -> Vec<InstanceId> {
    let (lo, hi) = region.bounds();
    let mut candidates = index.near_box(lo, hi);
    // In the tree's order, as without the index.
    candidates.sort_by_key(|id| id.to_bits() as u32);
    let mut found = Vec::new();
    for part in candidates {
        if found.len() >= limit {
            break;
        }
        if tree.exists(part)
            && !tree.is_destroyed(part).unwrap_or(true)
            && keep(part)
            && Region::of_part(tree, part).is_some_and(|r| r.overlaps(region))
        {
            found.push(part);
        }
    }
    found
}
