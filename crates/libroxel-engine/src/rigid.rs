//! Loose parts as real rigid bodies: they tumble, roll, stack and knock
//! each other over, whichever way they are turned. Anchored parts and
//! terrain are fixed bodies; parts moved by something else (characters,
//! hinge arms, parts held still for a step) are kinematic bodies that
//! shove loose things as they go. Parts welded to a loose root are extra
//! shapes on the root's body, so a welded model moves as one solid thing.
//!
//! The [`Physics`](crate::physics::Physics) step decides what each part is
//! and hands it over as a [`Spec`]; this module keeps the simulated world
//! in step with them and runs it.

// A fast fixed hash: these maps are read thousands of times a step.
use bevy::platform::collections::{HashMap, HashSet};

use bevy::math::{Quat, Vec3 as GVec3};
use libroxel_instance::{InstanceId, PartShape, Vec3, voxels::Voxels};
use rapier3d::glamx::{Quat as RQuat, Vec3 as RVec};
use rapier3d::prelude::*;

/// How far ahead (studs) moving parts look for things they are about to hit.
const SOFT_CCD: f32 = 6.0;

/// What a part is in the simulated world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Role {
    /// Never moves (anchored).
    Fixed,
    /// Moved by something else to where the tree says; shoves loose things.
    Kinematic,
    /// Moved by the simulation.
    Dynamic,
    /// Welded to a dynamic root, at this place on it.
    Member { root: InstanceId, offset: (Vec3, Quat) },
}

/// How a joint holds its two parts together.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JointKind {
    /// Part 1 turns only about frame 0's X axis, through frame 0. With a
    /// motor, it is driven at a speed (radians a second) with up to a torque.
    /// A servo's motor also holds its part up against gravity.
    /// With limits, the hinge angle (radians) stays between them.
    Hinge { motor: Option<(f32, f32)>, servo: bool, limits: Option<(f32, f32)> },
    /// Part 1 slides along frame 0's X axis without turning, between the
    /// limits (studs from frame 0) if there are any.
    Slider { limits: Option<(f32, f32)> },
    /// The two frames' origins held together; part 1 turns any way.
    Ball,
    /// The two frames' origins stay at most `length` apart.
    Rope { length: f32 },
    /// The two frames' origins stay exactly `length` apart.
    Rod { length: f32 },
    /// The two frames' origins pulled towards `length` apart, like a spring.
    Spring { length: f32, stiffness: f32, damping: f32 },
}

/// A joint between two parts. Each frame is in its part's own space (an
/// attachment's position and rotation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointSpec {
    pub id: InstanceId,
    pub part0: InstanceId,
    pub part1: InstanceId,
    pub frame0: (Vec3, Quat),
    pub frame1: (Vec3, Quat),
    pub kind: JointKind,
}

/// One part, as the step sees it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Spec {
    pub id: InstanceId,
    pub role: Role,
    pub pos: Vec3,
    pub rot: Quat,
    /// The orientation as the tree has it (degrees), to notice when it changes.
    pub orientation: Vec3,
    pub size: Vec3,
    pub shape: PartShape,
    /// The collision shape of its model, if it has one (see [`crate::collision_shapes`]).
    pub mesh: Option<u32>,
    pub can_collide: bool,
    /// In water: `Some(true)` floats up.
    pub floats: Option<bool>,
    /// How heavy it is for its size (reference-like units).
    pub density: f32,
    pub friction: f32,
    pub elasticity: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Key {
    kind: u8,
    root: Option<InstanceId>,
    size: [u32; 3],
    shape: PartShape,
    mesh: Option<u32>,
    can_collide: bool,
    density: u32,
    elasticity: u32,
}

impl Key {
    fn of(spec: &Spec) -> Self {
        let (kind, root) = match spec.role {
            Role::Fixed => (0, None),
            Role::Kinematic => (1, None),
            Role::Dynamic => (2, None),
            Role::Member { root, .. } => (3, Some(root)),
        };
        Key {
            kind,
            root,
            size: [spec.size.x.to_bits(), spec.size.y.to_bits(), spec.size.z.to_bits()],
            shape: spec.shape,
            mesh: spec.mesh,
            can_collide: spec.can_collide,
            density: spec.density.to_bits(),
            elasticity: spec.elasticity.to_bits(),
        }
    }
}

struct Entry {
    /// The part's own body (members ride on their root's).
    body: Option<RigidBodyHandle>,
    collider: ColliderHandle,
    parent: RigidBodyHandle,
    key: Key,
    /// Where it was last put, or (for dynamic parts) last left by the
    /// simulation, as the tree has it (position, orientation in degrees).
    pose: (Vec3, Vec3),
    offset: (Vec3, Quat),
    /// How a dynamic part was moving when the last step ended.
    velocity: (RVec, RVec),
}

/// A terrain chunk's position, in chunks.
type Chunk = (i32, i32, i32);

/// The simulated world.
#[derive(Default)]
pub(crate) struct Rigid {
    world: PhysicsWorld,
    parts: HashMap<InstanceId, Entry>,
    /// Terrain's fixed body, and a collider for each chunk of cells near
    /// something loose (with the chunk's fingerprint when it was made).
    terrain: Option<RigidBodyHandle>,
    terrain_chunks: HashMap<(i32, i32, i32), (u64, Option<ColliderHandle>)>,
    /// The terrain version and the chunks wanted when they were last brought in line.
    terrain_seen: Option<(u64, Vec<Chunk>)>,
    grounded: HashSet<InstanceId>,
    ready: bool,
    /// Each joint, the spec it was made from, and the bodies it joins.
    joints: HashMap<InstanceId, (ImpulseJointHandle, JointSpec, RigidBodyHandle, RigidBodyHandle)>,
    /// Bodies given extra turning weight for being in a joint.
    steadied: HashSet<RigidBodyHandle>,
}

impl std::fmt::Debug for Rigid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rigid").field("parts", &self.parts.len()).finish()
    }
}

pub(crate) fn rv(v: Vec3) -> RVec {
    RVec::new(v.x, v.y, v.z)
}

fn lv(v: RVec) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn rq(q: Quat) -> RQuat {
    RQuat::from_xyzw(q.x, q.y, q.z, q.w)
}

fn lq(q: RQuat) -> Quat {
    Quat::from_xyzw(q.x, q.y, q.z, q.w)
}

/// Turning weight added to every jointed part (studs squared times mass).
const JOINT_INERTIA: f32 = 0.3;

fn pose_of(pos: Vec3, rot: Quat) -> Pose {
    Pose::from_parts(rv(pos), rq(rot))
}

fn id_bits(id: InstanceId) -> u128 {
    u128::from(id.to_bits())
}

/// The reference platform's cylinders lie along their X axis; the engine's stand up along Y.
fn shape_for(size: Vec3, shape: PartShape) -> (SharedShape, Pose) {
    let h = GVec3::new(size.x, size.y, size.z) * 0.5;
    let h = h.max(GVec3::splat(0.01));
    match shape {
        PartShape::Ball => (SharedShape::ball(h.min_element()), Pose::IDENTITY),
        PartShape::Cylinder => {
            let radius = h.y.min(h.z);
            let turn = RQuat::from_rotation_z(std::f32::consts::FRAC_PI_2);
            (SharedShape::cylinder(h.x, radius), Pose::from_parts(RVec::ZERO, turn))
        }
        PartShape::Wedge => {
            // A wedge's slope rises from the front (-Z) bottom to the back (+Z) top.
            let points = [
                RVec::new(-h.x, -h.y, -h.z),
                RVec::new(h.x, -h.y, -h.z),
                RVec::new(-h.x, -h.y, h.z),
                RVec::new(h.x, -h.y, h.z),
                RVec::new(-h.x, h.y, h.z),
                RVec::new(h.x, h.y, h.z),
            ];
            let hull =
                SharedShape::convex_hull(&points).unwrap_or_else(|| SharedShape::cuboid(h.x, h.y, h.z));
            (hull, Pose::IDENTITY)
        }
        _ => (SharedShape::cuboid(h.x, h.y, h.z), Pose::IDENTITY),
    }
}

/// A part with a model: one convex shape per piece, stretched to its size.
fn mesh_shape_for(size: Vec3, mesh: u32) -> Option<SharedShape> {
    let found = crate::collision_shapes::shape(mesh)?;
    let size = GVec3::new(size.x, size.y, size.z).max(GVec3::splat(0.02));
    let hull = |piece: &crate::collision_shapes::Piece| {
        let points: Vec<RVec> =
            piece.points.iter().map(|p| *p * size).map(|p| RVec::new(p.x, p.y, p.z)).collect();
        SharedShape::convex_hull(&points)
    };
    let parts: Vec<(Pose, SharedShape)> =
        found.pieces.iter().filter_map(hull).map(|shape| (Pose::IDENTITY, shape)).collect();
    match parts.len() {
        0 => hull(&found.whole),
        1 => parts.into_iter().next().map(|(_, s)| s),
        _ => Some(SharedShape::compound(parts)),
    }
}

fn collider_for(spec: &Spec, offset: Pose) -> Collider {
    let (shape, local) = match spec.mesh.and_then(|m| mesh_shape_for(spec.size, m)) {
        Some(shape) => (shape, Pose::IDENTITY),
        None => shape_for(spec.size, spec.shape),
    };
    let mut builder = ColliderBuilder::new(shape)
        .position(offset * local)
        .density(spec.density.max(0.01))
        .friction(spec.friction)
        // Something slippery slides on anything.
        .friction_combine_rule(CoefficientCombineRule::Min)
        .restitution(spec.elasticity)
        .user_data(id_bits(spec.id))
        .active_hooks(ActiveHooks::FILTER_CONTACT_PAIRS);
    if !spec.can_collide {
        builder = builder.sensor(true);
    }
    builder.build()
}

/// Lets pairs that belong together (welded, or a hinge arm and its car)
/// pass through each other.
struct Filter<'a> {
    ignored: &'a HashSet<(InstanceId, InstanceId)>,
    groups: &'a crate::physics::Groups,
    /// Touching pairs let through so far this step.
    pairs: std::sync::atomic::AtomicUsize,
}

/// The most touching pairs solved in one step. A pile of thousands of loose
/// parts made in one spot (a script can make one in a line) touches every
/// part with every other, millions of pairs, which would take gigabytes and
/// minutes to solve; past this, the rest pass through each other until the
/// pile spreads out.
const MOST_CONTACT_PAIRS: usize = 100_000;

impl PhysicsHooks for Filter<'_> {
    fn filter_contact_pair(&self, context: &PairFilterContext) -> Option<SolverFlags> {
        let dynamic = |h: Option<RigidBodyHandle>| h.is_some_and(|h| context.bodies[h].is_dynamic());
        if !dynamic(context.rigid_body1) && !dynamic(context.rigid_body2) {
            return None;
        }
        let a = InstanceId::from_bits(context.colliders[context.collider1].user_data as u64);
        let b = InstanceId::from_bits(context.colliders[context.collider2].user_data as u64);
        if !self.ignored.is_empty() {
            let pair = if a <= b { (a, b) } else { (b, a) };
            if self.ignored.contains(&pair) {
                return None;
            }
        }
        if self.groups.apart(a, b) {
            return None;
        }
        if self.pairs.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= MOST_CONTACT_PAIRS {
            return None;
        }
        Some(SolverFlags::COMPUTE_RIGID_IMPULSES)
    }
}

/// Where a dynamic part ended a step, and how it moves.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Moved {
    pub id: InstanceId,
    pub pos: Vec3,
    pub orientation: Vec3,
}

impl Rigid {
    fn setup(&mut self) {
        if self.ready {
            return;
        }
        self.ready = true;
        let p = &mut self.world.integration_parameters;
        // Lengths are in studs: about three and a half to a metre.
        p.length_unit = 3.5;
        p.num_solver_iterations = 6;
        // Fast things are caught by looking ahead along how they move
        // (soft CCD), not by stopping them dead where they would hit.
        p.max_ccd_substeps = 0;
    }

    fn remove_entry(&mut self, id: InstanceId) {
        let Some(entry) = self.parts.remove(&id) else { return };
        match entry.body {
            Some(body) => {
                // Members on this body go with it; forget them so they are made
                // again. (Most bodies have only their own collider, and looking
                // through every part for members each time is slow when many go.)
                let members = self.world.bodies.get(body).is_some_and(|b| b.colliders().len() > 1);
                self.world.remove_body_with_colliders(body, true);
                if members {
                    self.parts.retain(|_, e| e.parent != body);
                }
            }
            None => {
                self.world.remove_collider(entry.collider);
            }
        }
    }

    /// Keeps terrain's solid cells near loose parts as colliders on one
    /// fixed body, one collider per chunk, remade when that chunk changes.
    /// Terrain far from anything loose can't be hit this step, and a big
    /// world's terrain changes too often to rebuild all of it each time.
    fn sync_terrain(&mut self, voxels: Option<&Voxels>, specs: &[Spec]) {
        use libroxel_instance::voxels::CHUNK;
        const REACH: i32 = 2;
        let voxels = voxels.filter(|v| !v.is_empty());
        let mut wanted: HashSet<(i32, i32, i32)> = HashSet::new();
        if voxels.is_some() {
            for spec in specs.iter().filter(|s| matches!(s.role, Role::Dynamic | Role::Member { .. })) {
                let reach = spec.size.length() as i32 / CHUNK + REACH;
                let c = |v: f32| (v.floor() as i32).div_euclid(CHUNK);
                let (cx, cy, cz) = (c(spec.pos.x), c(spec.pos.y), c(spec.pos.z));
                for x in cx - reach..=cx + reach {
                    for y in cy - reach..=cy + reach {
                        for z in cz - reach..=cz + reach {
                            wanted.insert((x, y, z));
                        }
                    }
                }
            }
        }
        let mut wanted_list: Vec<_> = wanted.iter().copied().collect();
        wanted_list.sort_unstable();
        let seen = (voxels.map_or(0, |v| v.version()), wanted_list);
        if self.terrain_seen.as_ref() == Some(&seen) {
            return;
        }
        self.terrain_seen = Some(seen);
        // Chunks no longer near anything loose go.
        let gone: Vec<_> = self.terrain_chunks.keys().filter(|k| !wanted.contains(*k)).copied().collect();
        for key in gone {
            if let Some((_, Some(collider))) = self.terrain_chunks.remove(&key) {
                self.world.remove_collider(collider);
            }
        }
        let Some(voxels) = voxels else { return };
        let body = *self.terrain.get_or_insert_with(|| self.world.insert_body(RigidBodyBuilder::fixed()));
        for key in wanted {
            let print = voxels.chunk_fingerprint(key);
            if self.terrain_chunks.get(&key).is_some_and(|(p, _)| *p == print) {
                continue;
            }
            if let Some((_, Some(old))) = self.terrain_chunks.remove(&key) {
                self.world.remove_collider(old);
            }
            let cells: Vec<_> = voxels
                .cells_in_chunk(key)
                .into_iter()
                .filter(|(_, m)| *m != libroxel_instance::Material::Water)
                .map(|((x, y, z), _)| rapier3d::math::IVector::new(x, y, z))
                .collect();
            let collider = (!cells.is_empty()).then(|| {
                let collider = ColliderBuilder::voxels(RVec::ONE, &cells).friction(0.5).user_data(u128::MAX);
                let bodies = &mut self.world.bodies;
                self.world.colliders.insert_with_parent(collider, body, bodies)
            });
            self.terrain_chunks.insert(key, (print, collider));
        }
    }

    /// Brings the world in line with `specs`, runs it for `dt` seconds,
    /// and returns where every dynamic part ended up. `velocities` and
    /// `spins` are read for dynamic parts before the step (scripts and
    /// movers set them) and written back after.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn step(
        &mut self,
        specs: &[Spec],
        ignored: &HashSet<(InstanceId, InstanceId)>,
        groups: &crate::physics::Groups,
        velocities: &mut HashMap<InstanceId, Vec3>,
        spins: &mut HashMap<InstanceId, Vec3>,
        voxels: Option<&Voxels>,
        joints: &[JointSpec],
        gravity: f32,
        dt: f32,
    ) -> Vec<Moved> {
        self.setup();
        self.sync_terrain(voxels, specs);
        let live: HashSet<InstanceId> = specs.iter().map(|s| s.id).collect();
        let gone: Vec<InstanceId> = self.parts.keys().filter(|id| !live.contains(*id)).copied().collect();
        for id in gone {
            self.remove_entry(id);
        }

        // Bodies first, then the members riding on them.
        let mut kinematic: Vec<(RigidBodyHandle, Pose, Pose)> = Vec::new();
        let (members, owners): (Vec<&Spec>, Vec<&Spec>) =
            specs.iter().partition(|s| matches!(s.role, Role::Member { .. }));
        for spec in owners {
            let key = Key::of(spec);
            if self.parts.get(&spec.id).is_some_and(|e| e.key != key) {
                self.remove_entry(spec.id);
            }
            let pose = pose_of(spec.pos, spec.rot);
            if !self.parts.contains_key(&spec.id) {
                let builder = match spec.role {
                    Role::Fixed => RigidBodyBuilder::fixed(),
                    Role::Kinematic => RigidBodyBuilder::kinematic_position_based(),
                    _ => RigidBodyBuilder::dynamic()
                        .linvel(rv(velocities.get(&spec.id).copied().unwrap_or_default()))
                        .angvel(rv(spins.get(&spec.id).copied().unwrap_or_default()))
                        .soft_ccd_prediction(SOFT_CCD),
                };
                let body = self.world.insert_body(builder.pose(pose).user_data(id_bits(spec.id)));
                let bodies = &mut self.world.bodies;
                let collider =
                    self.world.colliders.insert_with_parent(collider_for(spec, Pose::IDENTITY), body, bodies);
                self.parts.insert(
                    spec.id,
                    Entry {
                        body: Some(body),
                        collider,
                        parent: body,
                        key,
                        pose: (spec.pos, spec.orientation),
                        offset: (Vec3::ZERO, Quat::IDENTITY),
                        velocity: (RVec::NAN, RVec::NAN),
                    },
                );
                if spec.role != Role::Dynamic {
                    continue;
                }
            }
            let Some(entry) = self.parts.get_mut(&spec.id) else { continue };
            if let Some(c) = self.world.colliders.get_mut(entry.collider)
                && c.friction() != spec.friction
            {
                c.set_friction(spec.friction);
            }
            let Some(handle) = entry.body else { continue };
            let moved = entry.pose != (spec.pos, spec.orientation);
            let body = &mut self.world.bodies[handle];
            match spec.role {
                Role::Fixed => {
                    if moved {
                        body.set_position(pose, true);
                    }
                }
                Role::Kinematic => {
                    let from = *body.position();
                    let far = (lv(from.translation) - spec.pos).length() > 20.0;
                    if far {
                        body.set_position(pose, true);
                    }
                    kinematic.push((handle, if far { pose } else { from }, pose));
                }
                _ => {
                    // Moved by a script since the last step: put it there.
                    if moved {
                        body.set_position(pose, true);
                    }
                    let v = rv(velocities.get(&spec.id).copied().unwrap_or_default());
                    let w = rv(spins.get(&spec.id).copied().unwrap_or_default());
                    let (scale, damping) = match spec.floats {
                        Some(true) => (-0.3, 3.0),
                        Some(false) => (0.0, 3.0),
                        None => (1.0, 0.0),
                    };
                    if body.gravity_scale() != scale {
                        body.set_gravity_scale(scale, true);
                    }
                    body.reset_forces(false);
                    // Something (a script, a mover) changed how it moves
                    // since the last step: push it that much over this step,
                    // so a push that cancels gravity holds it exactly still.
                    let (was_v, was_w) = entry.velocity;
                    if was_v.is_nan() {
                        body.set_linvel(v, true);
                        body.set_angvel(w, true);
                    } else {
                        let dv = v - was_v;
                        if dv.length_squared() > 1e-10 && dt > 0.0 {
                            let mass = body.mass().max(1e-3);
                            body.add_force(dv * (mass / dt), true);
                        }
                        if (w - was_w).length_squared() > 1e-10 {
                            body.set_angvel(w, true);
                        }
                    }
                    if body.linear_damping() != damping {
                        body.set_linear_damping(damping);
                        body.set_angular_damping(damping);
                    }
                }
            }
            entry.pose = (spec.pos, spec.orientation);
        }
        for spec in members {
            let Role::Member { root, offset } = spec.role else { continue };
            let Some(parent) = self.parts.get(&root).and_then(|e| e.body) else { continue };
            let key = Key::of(spec);
            if self.parts.get(&spec.id).is_some_and(|e| e.key != key || e.parent != parent) {
                self.remove_entry(spec.id);
            }
            let local = pose_of(offset.0, offset.1);
            match self.parts.get_mut(&spec.id) {
                Some(entry) => {
                    if let Some(c) = self.world.colliders.get_mut(entry.collider)
                        && c.friction() != spec.friction
                    {
                        c.set_friction(spec.friction);
                    }
                    if entry.offset != offset {
                        let (_, shape_local) = shape_for(spec.size, spec.shape);
                        if let Some(c) = self.world.colliders.get_mut(entry.collider) {
                            c.set_position_wrt_parent(local * shape_local);
                        }
                        entry.offset = offset;
                    }
                    entry.pose = (spec.pos, spec.orientation);
                }
                None => {
                    let bodies = &mut self.world.bodies;
                    let collider =
                        self.world.colliders.insert_with_parent(collider_for(spec, local), parent, bodies);
                    self.parts.insert(
                        spec.id,
                        Entry {
                            body: None,
                            collider,
                            parent,
                            key,
                            pose: (spec.pos, spec.orientation),
                            offset,
                            velocity: (RVec::ZERO, RVec::ZERO),
                        },
                    );
                }
            }
        }

        self.sync_joints(joints);
        self.drive_servos(gravity, dt);

        // Run it in slices (a sixtieth of a second at a good frame rate,
        // longer when frames are slow, at most four: more would make slow
        // frames slower still), sliding kinematic parts along to where they
        // are going.
        self.world.gravity = RVec::new(0.0, -gravity, 0.0);
        let slices = ((dt * 45.0).ceil() as usize).clamp(1, 4);
        self.world.integration_parameters.dt = dt / slices as f32;
        let filter = Filter { ignored, groups, pairs: Default::default() };
        for k in 1..=slices {
            filter.pairs.store(0, std::sync::atomic::Ordering::Relaxed);
            let t = k as f32 / slices as f32;
            for (handle, from, to) in &kinematic {
                let at = from.lerp(to, t);
                self.world.bodies[*handle].set_next_kinematic_position(at);
            }
            self.world.step_with_events(&filter, &());
        }

        // Read back the dynamic parts.
        let mut moved = Vec::new();
        self.grounded.clear();
        for spec in specs.iter().filter(|s| s.role == Role::Dynamic) {
            let Some(entry) = self.parts.get_mut(&spec.id) else { continue };
            let Some(handle) = entry.body else { continue };
            let body = &self.world.bodies[handle];
            let pos = lv(body.position().translation);
            let rot = lq(body.position().rotation).normalize();
            entry.velocity = (body.linvel(), body.angvel());
            let (velocity, spin) = (lv(body.linvel()), lv(body.angvel()));
            let orientation = crate::physics::degrees_of(rot);
            entry.pose = (pos, orientation);
            velocities.insert(spec.id, velocity);
            if spin.length() > 1e-5 {
                spins.insert(spec.id, spin);
            } else {
                spins.remove(&spec.id);
            }
            // Resting on something: a contact pushing it up.
            let on_ground = body.colliders().iter().any(|&c| {
                self.world.narrow_phase.contact_pairs_with(c).any(|pair| {
                    let PairContacts::Rigid(contacts) = &pair.contacts else { return false };
                    let first = pair.collider1 == c;
                    contacts.manifolds.iter().any(|m| {
                        let up = if first { -m.data.normal.y } else { m.data.normal.y };
                        up > 0.5 && m.points.iter().any(|p| p.dist < 0.1)
                    })
                })
            });
            if on_ground {
                self.grounded.insert(spec.id);
            }
            moved.push(Moved { id: spec.id, pos, orientation });
        }
        moved
    }

    /// The body a part moves with, and where the part sits on it.
    fn body_of(&self, part: InstanceId) -> Option<(RigidBodyHandle, Pose)> {
        let entry = self.parts.get(&part)?;
        match entry.body {
            Some(body) => Some((body, Pose::IDENTITY)),
            None => Some((entry.parent, pose_of(entry.offset.0, entry.offset.1))),
        }
    }

    /// Makes, changes and removes joints to match `specs`.
    fn sync_joints(&mut self, specs: &[JointSpec]) {
        let live: HashSet<InstanceId> = specs.iter().map(|s| s.id).collect();
        let gone: Vec<InstanceId> = self.joints.keys().filter(|id| !live.contains(*id)).copied().collect();
        for id in gone {
            if let Some((handle, ..)) = self.joints.remove(&id) {
                self.world.remove_impulse_joint(handle);
            }
        }
        for spec in specs {
            let (Some((b0, at0)), Some((b1, at1))) = (self.body_of(spec.part0), self.body_of(spec.part1))
            else {
                continue;
            };
            if b0 == b1 {
                continue;
            }
            // Joints go when their bodies do; and a joint whose ends changed is made again.
            let stale = match self.joints.get(&spec.id) {
                Some((handle, old, o0, o1)) => {
                    self.world.impulse_joints.get(*handle).is_none()
                        || (*o0, *o1) != (b0, b1)
                        || old.frame0 != spec.frame0
                        || old.frame1 != spec.frame1
                        || std::mem::discriminant(&old.kind) != std::mem::discriminant(&spec.kind)
                }
                None => true,
            };
            if stale {
                if let Some((handle, ..)) = self.joints.remove(&spec.id) {
                    self.world.remove_impulse_joint(handle);
                }
                let f0 = at0 * pose_of(spec.frame0.0, spec.frame0.1);
                let f1 = at1 * pose_of(spec.frame1.0, spec.frame1.1);
                let mut joint = match spec.kind {
                    JointKind::Hinge { .. } => GenericJoint::new(JointAxesMask::LOCKED_REVOLUTE_AXES),
                    JointKind::Slider { .. } => GenericJoint::new(JointAxesMask::LOCKED_PRISMATIC_AXES),
                    JointKind::Ball => GenericJoint::new(JointAxesMask::LOCKED_SPHERICAL_AXES),
                    JointKind::Rope { length } | JointKind::Rod { length } => {
                        RopeJoint::new(length.max(0.01)).data
                    }
                    JointKind::Spring { length, stiffness, damping } => {
                        SpringJoint::new(length, stiffness, damping).data
                    }
                };
                joint.set_local_frame1(f0).set_local_frame2(f1).set_contacts_enabled(false);
                // Tiny parts in a chain of joints (a steering knuckle
                // between car and wheel) are too easily twisted for the
                // solver to keep the joints together: give them some heft.
                for b in [b0, b1] {
                    if let Some(body) = self.world.bodies.get_mut(b)
                        && body.is_dynamic()
                        && self.steadied.insert(b)
                    {
                        body.set_additional_mass_properties(
                            MassProperties::new(RVec::ZERO, 0.0, RVec::splat(JOINT_INERTIA)),
                            true,
                        );
                    }
                }
                let handle = self.world.insert_impulse_joint(b0, b1, joint);
                self.joints.insert(spec.id, (handle, *spec, b0, b1));
            }
            let Some((handle, old, ..)) = self.joints.get_mut(&spec.id) else { continue };
            let changed = stale || old.kind != spec.kind;
            *old = *spec;
            if !changed {
                continue;
            }
            let Some(joint) = self.world.impulse_joints.get_mut(*handle, true) else { continue };
            match spec.kind {
                JointKind::Hinge { motor, limits, .. } => {
                    match limits {
                        Some((lo, hi)) => {
                            joint.data.set_limits(JointAxis::AngX, [lo, hi]);
                        }
                        None => joint.data.limit_axes.remove(JointAxesMask::ANG_X),
                    }
                    // Servos are also pushed round by `drive_servos`.
                    let (speed, torque) = motor.unwrap_or((0.0, 0.0));
                    joint.data.set_motor_model(JointAxis::AngX, MotorModel::ForceBased);
                    joint.data.set_motor_velocity(
                        JointAxis::AngX,
                        speed,
                        if motor.is_some() { 1e6 } else { 0.0 },
                    );
                    joint.data.set_motor_max_force(JointAxis::AngX, torque);
                }
                JointKind::Slider { limits } => {
                    let [lo, hi] = limits.map_or([-1e6, 1e6], |(lo, hi)| [lo, hi]);
                    joint.data.set_limits(JointAxis::LinX, [lo, hi]);
                }
                JointKind::Ball => {}
                JointKind::Rope { length } => {
                    joint.data.set_limits(JointAxis::LinX, [0.0, length.max(0.0)]);
                }
                JointKind::Rod { length } => {
                    joint.data.set_limits(JointAxis::LinX, [length.max(0.0), length.max(0.0)]);
                }
                JointKind::Spring { length, stiffness, damping } => {
                    joint.data.set_motor_position(JointAxis::LinX, length, stiffness, damping);
                }
            }
        }
    }

    /// Turns each servo's part with a torque about the hinge: enough to
    /// reach its wanted speed over `dt` and to hold the part up against
    /// gravity, but no more than the servo's torque. (The solver's own
    /// motor, which servos keep too, only sees a part's turning weight
    /// about its middle, so alone it is far too weak for an arm held out
    /// from its hinge; it does hold the hinge firm against knocks.)
    fn drive_servos(&mut self, gravity: f32, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        let links: Vec<(RigidBodyHandle, RigidBodyHandle)> =
            self.joints.values().map(|j| (j.2, j.3)).collect();
        for (_, spec, b0, b1) in self.joints.values() {
            let JointKind::Hinge { motor: Some((speed, most)), servo: true, .. } = spec.kind else {
                continue;
            };
            let (Some(body0), Some(body1)) = (self.world.bodies.get(*b0), self.world.bodies.get(*b1)) else {
                continue;
            };
            let frame = *body0.position() * pose_of(spec.frame0.0, spec.frame0.1);
            let axis = frame.rotation * RVec::X;
            let pivot = frame.translation;
            let spin = (body1.angvel() - body0.angvel()).dot(axis);
            // Everything it turns: its part, and what hangs from that on
            // other joints (a wheel on a steering knuckle).
            let mut carried = vec![*b1];
            let mut i = 0;
            while i < carried.len() && carried.len() < 16 {
                let from = carried[i];
                for &(a, b) in &links {
                    if a == from && b != *b0 && !carried.contains(&b) {
                        carried.push(b);
                    }
                }
                i += 1;
            }
            // How hard each one is to turn about the hinge: by spinning
            // it about its own middle, and by swinging its middle round.
            let (mut shares, mut inertia, mut sag) = (Vec::new(), 0.0, 0.0);
            for handle in &carried {
                let Some(body) = self.world.bodies.get(*handle).filter(|b| b.is_dynamic()) else { continue };
                let turn = (body.mass_properties().effective_world_inv_inertia * axis).dot(axis);
                let own = if turn > 0.0 { 1.0 / turn } else { 0.0 };
                let mass = body.mass();
                let arm = body.center_of_mass() - pivot;
                let lever = axis.cross(arm);
                let swing = mass * lever.length_squared();
                inertia += own + swing;
                let weight = RVec::new(0.0, -gravity * mass * body.gravity_scale(), 0.0);
                sag += arm.cross(weight).dot(axis);
                shares.push((*handle, own, swing, lever, body.center_of_mass()));
            }
            if inertia <= 0.0 {
                continue;
            }
            let torque = crate::clamp_lenient(inertia * (speed - spin) / dt - sag, -most, most);
            // Share the torque out by how much each takes to turn. A part
            // held out from the hinge has its middle pushed round (as
            // gravity does): twisting it there instead, the solver turns
            // that into swinging about the hinge only slowly.
            let dynamic0 = body0.is_dynamic();
            for (handle, own, swing, lever, at) in shares {
                let twist = axis * (torque * own / inertia);
                let reach = lever.length_squared();
                let push = if reach > 1e-6 { lever * (torque * swing / inertia / reach) } else { RVec::ZERO };
                let body = &mut self.world.bodies[handle];
                body.add_torque(twist, true);
                body.add_force(push, true);
                if dynamic0 {
                    let base = &mut self.world.bodies[*b0];
                    base.add_torque(-twist, true);
                    base.add_force_at_point(-push, at, true);
                }
            }
        }
    }

    /// Whether a dynamic part ended the last step resting on something.
    pub(crate) fn is_grounded(&self, id: InstanceId) -> bool {
        self.grounded.contains(&id)
    }
}
