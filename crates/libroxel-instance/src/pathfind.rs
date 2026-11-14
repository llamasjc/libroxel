//! Finding a way for a character to walk from one place to another,
//! around walls and up steps, jumping where it has to.
//!
//! The ground is searched on a grid of one-stud cells. A cell can be stood
//! on when there is ground under it and room above it for the agent. From
//! a cell the agent can step to any of its eight neighbours whose ground is
//! at most a step up (or a jump up, if it can jump), or any distance down
//! (up to [`MAX_DROP`]). Several floors work: each cell's ground is found
//! from the height of the cell it was reached from.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::geometry::{Region, ray_part};
use crate::{ClassKind, InstanceId, InstanceTree, Property, Rotation, Vec3};

/// The size of a grid cell, in studs.
pub const CELL: f32 = 1.0;
/// The furthest an agent will drop off a ledge.
pub const MAX_DROP: f32 = 20.0;
/// How many cells the search looks at before giving up.
const MAX_VISITS: usize = 40_000;
/// How far past the start and the goal the search may wander, in cells.
const MARGIN: i32 = 40;

/// What is walking: how wide and tall it is, and how high it can get up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Agent {
    /// Half its width: it needs this much room on every side.
    pub radius: f32,
    pub height: f32,
    /// The highest it can walk up without jumping.
    pub step: f32,
    /// The highest it can jump up, or 0 if it can't.
    pub jump: f32,
}

impl Default for Agent {
    /// A character's size: two studs wide, five tall, stepping 0.6 and
    /// jumping about 4.5 studs up.
    fn default() -> Self {
        Self { radius: 1.0, height: 5.0, step: 0.6, jump: 4.5 }
    }
}

/// A point on a path: where, and whether to jump to get there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Waypoint {
    pub position: Vec3,
    pub jump: bool,
}

/// The solid things the search has to walk on and around.
struct World<'a> {
    tree: &'a InstanceTree,
    parts: Vec<(InstanceId, Region, Vec3, Vec3)>,
    /// Which parts are over each 8 by 8 stud patch of ground.
    buckets: HashMap<(i32, i32), Vec<usize>>,
    /// Parts too big to bucket (a map-sized floor), checked everywhere.
    huge: Vec<usize>,
    terrain: Option<&'a crate::voxels::Voxels>,
}

const BUCKET: f32 = 8.0;

fn bucket_of(x: f32, z: f32) -> (i32, i32) {
    ((x / BUCKET).floor() as i32, (z / BUCKET).floor() as i32)
}

impl<'a> World<'a> {
    fn new(tree: &'a InstanceTree, keep: &impl Fn(InstanceId) -> bool) -> Self {
        let mut parts = Vec::new();
        let mut buckets: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        let mut huge = Vec::new();
        for part in tree.descendants_of_class(tree.workspace(), ClassKind::Part) {
            if tree.get_bool(part, Property::CanCollide) == Some(false) || !keep(part) {
                continue;
            }
            let Some(region) = Region::of_part(tree, part) else { continue };
            let (low, high) = region.bounds();
            let index = parts.len();
            parts.push((part, region, low, high));
            let (x0, z0) = bucket_of(low.x, low.z);
            let (x1, z1) = bucket_of(high.x, high.z);
            // A part over more than a few thousand patches (a map-sized
            // floor) is checked everywhere instead.
            let patches = (i64::from(x1) - i64::from(x0) + 1) * (i64::from(z1) - i64::from(z0) + 1);
            if patches > 4096 {
                huge.push(index);
                continue;
            }
            for bx in x0..=x1 {
                for bz in z0..=z1 {
                    buckets.entry((bx, bz)).or_default().push(index);
                }
            }
        }
        let terrain =
            tree.terrain().filter(|t| keep(*t)).and_then(|t| tree.voxels(t).ok()).filter(|v| !v.is_empty());
        Self { tree, parts, buckets, huge, terrain }
    }

    fn near(&self, x: f32, z: f32) -> impl Iterator<Item = &usize> {
        self.buckets.get(&bucket_of(x, z)).map_or(&[][..], Vec::as_slice).iter().chain(&self.huge)
    }

    /// The height of the highest ground at (x, z) at or below `from`, no
    /// more than `depth` below it.
    fn ground(&self, x: f32, z: f32, from: f32, depth: f32) -> Option<f32> {
        let origin = Vec3::new(x, from, z);
        let down = Vec3::new(0.0, -1.0, 0.0);
        let mut best = depth;
        for &i in self.near(x, z) {
            let (part, _, low, high) = self.parts[i];
            if low.y > from || x < low.x || x > high.x || z < low.z || z > high.z {
                continue;
            }
            if let Some((t, normal)) = ray_part(self.tree, part, origin, down)
                && t <= best
                && normal.y > 0.5
            {
                best = t;
            }
        }
        if let Some(voxels) = self.terrain
            && let Some((t, normal, _)) = voxels.raycast(origin, down, best)
            && normal.y > 0.5
        {
            best = best.min(t);
        }
        (best < depth).then_some(from - best)
    }

    /// Whether an agent standing at (x, ground, z) has room.
    fn room(&self, x: f32, ground: f32, z: f32, agent: &Agent) -> bool {
        let tall = (agent.height - agent.step).max(0.5);
        let center = Vec3::new(x, ground + agent.step + 0.05 + tall / 2.0, z);
        let half = Vec3::new(agent.radius - 0.05, tall / 2.0, agent.radius - 0.05);
        let space = Region::Box { center, rot: Rotation::IDENTITY, half };
        let (low, high) = space.bounds();
        let mut seen = Vec::new();
        for corner_x in [low.x, high.x] {
            for corner_z in [low.z, high.z] {
                for &i in self.near(corner_x, corner_z) {
                    if seen.contains(&i) {
                        continue;
                    }
                    seen.push(i);
                    let (_, region, plow, phigh) = self.parts[i];
                    let apart = plow.x > high.x
                        || phigh.x < low.x
                        || plow.y > high.y
                        || phigh.y < low.y
                        || plow.z > high.z
                        || phigh.z < low.z;
                    if !apart && region.overlaps(space) {
                        return false;
                    }
                }
            }
        }
        if let Some(voxels) = self.terrain {
            let (x0, y0, z0) = crate::voxels::cell_at(low);
            let (x1, y1, z1) = crate::voxels::cell_at(high);
            for cx in x0..=x1 {
                for cy in y0..=y1 {
                    for cz in z0..=z1 {
                        if voxels.get(cx, cy, cz).is_some() {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }
}

/// A cell and the height of the ground in it (in quarter studs, so floors
/// at different heights over the same cell are different places).
type Node = (i32, i32, i32);

#[derive(PartialEq)]
struct Open {
    guess: f32,
    node: Node,
}

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other.guess.partial_cmp(&self.guess).unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn cell(v: f32) -> i32 {
    (v / CELL).floor() as i32
}

fn middle(c: i32) -> f32 {
    (c as f32 + 0.5) * CELL
}

fn level(y: f32) -> i32 {
    (y * 4.0).round() as i32
}

/// Finds a way from `start` to `goal` (each a point at or a little above
/// the ground, like a character's middle) for `agent`, among the parts
/// `keep` accepts. Returns the waypoints from the start to the goal, each
/// on the ground, or `None` if there is no way.
pub fn find_path(
    tree: &InstanceTree,
    start: Vec3,
    goal: Vec3,
    agent: Agent,
    keep: impl Fn(InstanceId) -> bool,
) -> Option<Vec<Waypoint>> {
    let world = World::new(tree, &keep);
    let reach = agent.height + MAX_DROP;
    let start_ground = world.ground(start.x, start.z, start.y + agent.step, reach)?;
    let goal_ground = world.ground(goal.x, goal.z, goal.y + agent.step, reach)?;
    let (sx, sz, gx, gz) = (cell(start.x), cell(start.z), cell(goal.x), cell(goal.z));
    let (min_x, max_x) = (sx.min(gx) - MARGIN, sx.max(gx) + MARGIN);
    let (min_z, max_z) = (sz.min(gz) - MARGIN, sz.max(gz) + MARGIN);

    let first: Node = (sx, sz, level(start_ground));
    let guess = |n: Node| {
        let (dx, dz) = ((n.0 - gx) as f32 * CELL, (n.1 - gz) as f32 * CELL);
        (dx * dx + dz * dz).sqrt() + (n.2 as f32 / 4.0 - goal_ground).abs()
    };
    let mut open = BinaryHeap::new();
    let mut cost: HashMap<Node, f32> = HashMap::new();
    let mut came: HashMap<Node, (Node, bool)> = HashMap::new();
    // Where a step from one cell to a neighbour lands, worked out once.
    let mut landing: HashMap<(Node, i32, i32), Option<(Node, bool)>> = HashMap::new();
    cost.insert(first, 0.0);
    open.push(Open { guess: guess(first), node: first });
    let mut visits = 0;
    let mut end = None;
    while let Some(Open { node, .. }) = open.pop() {
        if node.0 == gx && node.1 == gz && (node.2 as f32 / 4.0 - goal_ground).abs() <= agent.step + 0.3 {
            end = Some(node);
            break;
        }
        visits += 1;
        if visits > MAX_VISITS {
            return None;
        }
        let here = cost[&node];
        let height = node.2 as f32 / 4.0;
        let mut step_to = |dx: i32, dz: i32| -> Option<(Node, bool)> {
            *landing.entry((node, dx, dz)).or_insert_with(|| {
                let (nx, nz) = (node.0 + dx, node.1 + dz);
                if nx < min_x || nx > max_x || nz < min_z || nz > max_z {
                    return None;
                }
                let (x, z) = (middle(nx), middle(nz));
                let top = height + agent.step.max(agent.jump) + 0.05;
                let ground = world.ground(x, z, top, top - height + MAX_DROP)?;
                let rise = ground - height;
                let jump = rise > agent.step + 0.01;
                if jump && rise > agent.jump {
                    return None;
                }
                if !world.room(x, ground, z, &agent) {
                    return None;
                }
                let leap = dx.abs().max(dz.abs());
                if leap > 1 && !jump {
                    // Leaps are only for getting up onto ledges.
                    return None;
                }
                // A jump needs room to get up there, all the way from here.
                if jump {
                    let (ux, uz) = (dx.signum(), dz.signum());
                    for i in 0..leap {
                        let (cx, cz) = (middle(node.0 + ux * i), middle(node.1 + uz * i));
                        if !world.room(cx, ground, cz, &agent) {
                            return None;
                        }
                    }
                }
                Some(((nx, nz, level(ground)), jump))
            })
        };
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            // No cutting corners: a diagonal needs both sides clear.
            if dx != 0 && dz != 0 && (step_to(dx, 0).is_none() || step_to(0, dz).is_none()) {
                continue;
            }
            // Next to a ledge there's no room to stand right under its
            // edge, so a jump up it can start a cell or two back.
            let mut found = step_to(dx, dz);
            let mut leap = 1;
            if found.is_none() && (dx == 0 || dz == 0) {
                for k in 2..=3 {
                    found = step_to(dx * k, dz * k);
                    if found.is_some() {
                        leap = k;
                        break;
                    }
                }
            }
            let Some((next, jump)) = found else { continue };
            let length = if dx != 0 && dz != 0 { std::f32::consts::SQRT_2 } else { leap as f32 } * CELL;
            let climb = (next.2 - node.2).max(0) as f32 / 4.0;
            let total = here + length + climb + if jump { 2.0 } else { 0.0 };
            if cost.get(&next).is_none_or(|c| total < *c) {
                cost.insert(next, total);
                came.insert(next, (node, jump));
                open.push(Open { guess: total + guess(next), node: next });
            }
        }
    }
    let end = end?;
    // Walk back from the goal, then keep only the corners and the jumps.
    let mut nodes = vec![(end, false)];
    let mut at = end;
    while let Some(&(before, jump)) = came.get(&at) {
        nodes.last_mut().expect("there is a node").1 = jump;
        nodes.push((before, false));
        at = before;
    }
    nodes.reverse();
    let point = |n: Node| Vec3::new(middle(n.0), n.2 as f32 / 4.0, middle(n.1));
    let mut waypoints = vec![Waypoint { position: Vec3::new(start.x, start_ground, start.z), jump: false }];
    for i in 1..nodes.len() {
        let (node, jump) = nodes[i];
        let last = i + 1 == nodes.len();
        let turns = !last && {
            let (a, b, c) = (nodes[i - 1].0, node, nodes[i + 1].0);
            (b.0 - a.0, b.1 - a.1) != (c.0 - b.0, c.1 - b.1) || nodes[i + 1].1 || (c.2 != b.2)
        };
        if last {
            waypoints.push(Waypoint { position: Vec3::new(goal.x, goal_ground, goal.z), jump });
        } else if jump || turns {
            waypoints.push(Waypoint { position: point(node), jump });
        }
    }
    Some(waypoints)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PropertyValue;

    fn block(tree: &mut InstanceTree, pos: [f32; 3], size: [f32; 3]) -> InstanceId {
        let id = tree.create(ClassKind::Part, "Block").unwrap();
        tree.set_parent(id, Some(tree.workspace())).unwrap();
        tree.set(id, Property::Position, PropertyValue::Vec3(Vec3::new(pos[0], pos[1], pos[2]))).unwrap();
        tree.set(id, Property::Size, PropertyValue::Vec3(Vec3::new(size[0], size[1], size[2]))).unwrap();
        id
    }

    fn floor() -> InstanceTree {
        let mut tree = InstanceTree::new();
        block(&mut tree, [0.0, -0.5, 0.0], [100.0, 1.0, 100.0]);
        tree
    }

    fn path(tree: &InstanceTree, from: [f32; 3], to: [f32; 3]) -> Option<Vec<Waypoint>> {
        let v = |p: [f32; 3]| Vec3::new(p[0], p[1], p[2]);
        find_path(tree, v(from), v(to), Agent::default(), |_| true)
    }

    #[test]
    fn walks_straight_across_open_ground() {
        let tree = floor();
        let way = path(&tree, [0.0, 2.0, 0.0], [20.0, 2.0, 0.0]).unwrap();
        assert_eq!(way.len(), 2, "{way:?}");
        assert_eq!(way[1].position, Vec3::new(20.0, 0.0, 0.0));
        assert!(way.iter().all(|w| !w.jump));
    }

    #[test]
    fn goes_round_a_wall() {
        let mut tree = floor();
        // A wall across the way, with a gap at its far end.
        block(&mut tree, [10.0, 5.0, -4.0], [1.0, 10.0, 30.0]);
        let way = path(&tree, [0.0, 2.0, 0.0], [20.0, 2.0, 0.0]).unwrap();
        assert!(way.len() > 2, "{way:?}");
        // It goes round the end of the wall (z beyond 11).
        assert!(way.iter().any(|w| w.position.z > 11.0), "{way:?}");
    }

    #[test]
    fn jumps_up_ledges_but_not_cliffs() {
        let mut tree = floor();
        // A 3-stud ledge: a jump up.
        block(&mut tree, [15.0, 1.5, 0.0], [10.0, 3.0, 100.0]);
        let way = path(&tree, [0.0, 2.0, 0.0], [15.0, 5.0, 0.0]).unwrap();
        assert!(way.iter().any(|w| w.jump), "{way:?}");
        assert!((way.last().unwrap().position.y - 3.0).abs() < 0.01);
        // A 10-stud cliff all the way across: no way up.
        let mut tree = floor();
        block(&mut tree, [15.0, 5.0, 0.0], [10.0, 10.0, 100.0]);
        assert!(path(&tree, [0.0, 2.0, 0.0], [15.0, 12.0, 0.0]).is_none());
    }

    #[test]
    fn climbs_stairs_without_jumping() {
        let mut tree = floor();
        for i in 0..8 {
            let h = 0.5 * (i + 1) as f32;
            block(&mut tree, [5.0 + i as f32, h / 2.0, 0.0], [1.0, h, 6.0]);
        }
        block(&mut tree, [20.0, 2.0, 0.0], [14.0, 4.0, 6.0]);
        // Walls on both sides, so the stairs are the way.
        let way = path(&tree, [0.0, 2.0, 0.0], [20.0, 6.0, 0.0]).unwrap();
        assert!(!way.iter().any(|w| w.jump), "{way:?}");
        assert!((way.last().unwrap().position.y - 4.0).abs() < 0.01);
    }

    #[test]
    fn a_boxed_in_goal_has_no_path() {
        let mut tree = floor();
        for (pos, size) in [
            ([20.0, 5.0, 4.0], [10.0, 10.0, 1.0]),
            ([20.0, 5.0, -4.0], [10.0, 10.0, 1.0]),
            ([24.0, 5.0, 0.0], [1.0, 10.0, 8.0]),
            ([16.0, 5.0, 0.0], [1.0, 10.0, 8.0]),
        ] {
            block(&mut tree, pos, size);
        }
        assert!(path(&tree, [0.0, 2.0, 0.0], [20.0, 2.0, 0.0]).is_none());
    }
}
