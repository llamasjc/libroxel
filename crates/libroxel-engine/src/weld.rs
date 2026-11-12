//! Welds: a `WeldConstraint` in the workspace holds its `Part0` and `Part1`
//! together. Parts joined by welds (directly or through other parts) make
//! an assembly that moves as one: one part, the root, is simulated, and the
//! others are carried along at the same offset from it.
//!
//! The root is the assembly's `HumanoidRootPart` if it has one, else an
//! anchored part, else the biggest. Carried parts stand still during the
//! physics step (things bump into them) and are placed afterwards. A script
//! that moves a carried part on its own changes where it sits on the root.

use std::collections::{HashMap, HashSet};

use libroxel_instance::{
    CFrame, ClassKind, InstanceId, InstanceTree, Property, PropertyValue, Rotation, Vec3,
};
use libroxel_script::names;

use crate::physics::Physics;

/// Where a carried part sits on its root, and where it was last put.
#[derive(Debug, Clone, Copy)]
struct Carry {
    root: InstanceId,
    offset: CFrame,
    placed: (Vec3, Vec3),
}

/// The engine's view of every weld.
#[derive(Debug, Default)]
pub struct Welds {
    carried: HashMap<InstanceId, Carry>,
}

fn pose(tree: &InstanceTree, part: InstanceId) -> Option<(Vec3, Vec3)> {
    Some((
        tree.get_vec3(part, Property::Position)?,
        tree.get_vec3(part, Property::Orientation).unwrap_or_default(),
    ))
}

fn cframe(pose: (Vec3, Vec3)) -> CFrame {
    CFrame::new(pose.0, Rotation::from_orientation(pose.1))
}

/// The groups of parts that enabled welds in the workspace join, each with
/// two or more parts.
pub fn assemblies(tree: &InstanceTree) -> Vec<Vec<InstanceId>> {
    let ws = tree.workspace();
    let in_world = |p: InstanceId| {
        tree.class(p).is_ok_and(|c| c.is_physical())
            && !tree.is_destroyed(p).unwrap_or(true)
            && tree.is_descendant_of(p, ws).unwrap_or(false)
    };
    // Union-find over the welded parts.
    let mut parent: HashMap<InstanceId, InstanceId> = HashMap::new();
    fn find(parent: &mut HashMap<InstanceId, InstanceId>, x: InstanceId) -> InstanceId {
        let p = *parent.entry(x).or_insert(x);
        if p == x {
            return x;
        }
        let r = find(parent, p);
        parent.insert(x, r);
        r
    }
    for weld in tree.descendants_of_class(ws, ClassKind::WeldConstraint) {
        if tree.get_bool(weld, Property::Enabled) != Some(true) {
            continue;
        }
        let (Some(a), Some(b)) = (tree.get_ref(weld, Property::Part0), tree.get_ref(weld, Property::Part1))
        else {
            continue;
        };
        if a == b || !in_world(a) || !in_world(b) {
            continue;
        }
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra != rb {
            parent.insert(ra, rb);
        }
    }
    let mut groups: HashMap<InstanceId, Vec<InstanceId>> = HashMap::new();
    let mut parts: Vec<InstanceId> = parent.keys().copied().collect();
    parts.sort();
    for p in parts {
        let r = find(&mut parent, p);
        groups.entry(r).or_default().push(p);
    }
    let mut out: Vec<Vec<InstanceId>> = groups.into_values().collect();
    out.sort();
    out
}

/// The part of an assembly that physics moves.
pub fn root_of(tree: &InstanceTree, parts: &[InstanceId]) -> InstanceId {
    let volume = |p: InstanceId| {
        let s = tree.get_vec3(p, Property::Size).unwrap_or_default();
        s.x * s.y * s.z
    };
    let rank = |p: InstanceId| {
        let named_root = tree.name(p) == Ok(names::character::ROOT_PART);
        let anchored = tree.get_bool(p, Property::Anchored) == Some(true);
        (named_root, anchored)
    };
    let mut best = parts[0];
    for &p in &parts[1..] {
        let (rp, rb) = (rank(p), rank(best));
        if rp > rb || (rp == rb && volume(p) > volume(best)) {
            best = p;
        }
    }
    best
}

impl Welds {
    /// Works out the assemblies, notes where each carried part sits on its
    /// root, and tells physics which parts to hold still and which pairs
    /// to let pass through each other.
    pub fn before_physics(&mut self, tree: &InstanceTree, physics: &mut Physics) {
        let groups = assemblies(tree);
        let mut carried = HashMap::new();
        let mut together = Vec::new();
        for parts in &groups {
            let root = root_of(tree, parts);
            let Some(root_pose) = pose(tree, root) else { continue };
            for &part in parts {
                if part == root {
                    continue;
                }
                let Some(now) = pose(tree, part) else { continue };
                // A new carry, a new root, or the part was moved by hand:
                // it stays where it is now, relative to the root.
                let carry = match self.carried.get(&part) {
                    Some(c) if c.root == root && c.placed == now => *c,
                    _ => Carry { root, offset: cframe(root_pose).inverse() * cframe(now), placed: now },
                };
                carried.insert(part, carry);
            }
            for (i, &a) in parts.iter().enumerate() {
                for &b in &parts[i + 1..] {
                    together.push((a, b));
                }
            }
        }
        self.carried = carried;
        let welded = self
            .carried
            .iter()
            .map(|(&part, c)| {
                let q = crate::physics::orientation_quat(c.offset.rot.to_orientation());
                (part, (c.root, (c.offset.pos, q)))
            })
            .collect();
        physics.set_welds(welded);
        physics.set_assemblies(self.carried.keys().copied().collect::<HashSet<_>>(), together);
    }

    /// Puts every carried part back at its place on its root.
    pub fn after_physics(&mut self, tree: &mut InstanceTree) {
        for carry in self.carried.values_mut() {
            let Some(root_pose) = pose(tree, carry.root) else { continue };
            let cf = cframe(root_pose) * carry.offset;
            carry.placed = (cf.pos, cf.rot.to_orientation());
        }
        for (&part, carry) in &self.carried {
            if pose(tree, part) == Some(carry.placed) {
                continue;
            }
            let _ = tree.set(part, Property::Position, PropertyValue::Vec3(carry.placed.0));
            let _ = tree.set(part, Property::Orientation, PropertyValue::Vec3(carry.placed.1));
        }
    }

    /// The part physics moves for `part`: its assembly's root, or itself.
    pub fn root_for(&self, part: InstanceId) -> InstanceId {
        self.carried.get(&part).map_or(part, |c| c.root)
    }

    /// Every part `root` carries, not counting itself.
    pub fn carried_by(&self, root: InstanceId) -> impl Iterator<Item = InstanceId> + '_ {
        self.carried.iter().filter(move |(_, c)| c.root == root).map(|(p, _)| *p)
    }

    /// Whether `part` is carried by a weld rather than moving on its own.
    pub fn is_carried(&self, part: InstanceId) -> bool {
        self.carried.contains_key(&part)
    }
}
