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

use std::collections::{HashMap, HashSet};

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
    pub can_collide: bool,
    /// In water: `Some(true)` floats up.
    pub floats: Option<bool>,
    /// How heavy it is for its size (Roblox-like units).
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

/// The simulated world.
#[derive(Default)]
pub(crate) struct Rigid {
    world: PhysicsWorld,
    parts: HashMap<InstanceId, Entry>,
    terrain: Option<(u64, RigidBodyHandle)>,
    grounded: HashSet<InstanceId>,
    ready: bool,
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

fn pose_of(pos: Vec3, rot: Quat) -> Pose {
    Pose::from_parts(rv(pos), rq(rot))
}

fn id_bits(id: InstanceId) -> u128 {
    u128::from(id.to_bits())
}

/// Roblox cylinders lie along their X axis; the engine's stand up along Y.
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

fn collider_for(spec: &Spec, offset: Pose) -> Collider {
    let (shape, local) = shape_for(spec.size, spec.shape);
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
}

impl PhysicsHooks for Filter<'_> {
    fn filter_contact_pair(&self, context: &PairFilterContext) -> Option<SolverFlags> {
        let dynamic = |h: Option<RigidBodyHandle>| h.is_some_and(|h| context.bodies[h].is_dynamic());
        if !dynamic(context.rigid_body1) && !dynamic(context.rigid_body2) {
            return None;
        }
        if !self.ignored.is_empty() {
            let a = InstanceId::from_bits(context.colliders[context.collider1].user_data as u64);
            let b = InstanceId::from_bits(context.colliders[context.collider2].user_data as u64);
            let pair = if a <= b { (a, b) } else { (b, a) };
            if self.ignored.contains(&pair) {
                return None;
            }
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
                // Members on this body go with it; forget them so they are made again.
                self.world.remove_body_with_colliders(body, true);
                self.parts.retain(|_, e| e.parent != body);
            }
            None => {
                self.world.remove_collider(entry.collider);
            }
        }
    }

    /// Keeps terrain's solid cells as one fixed body, remade when they change.
    fn sync_terrain(&mut self, voxels: Option<&Voxels>) {
        let version = voxels.filter(|v| !v.is_empty()).map(|v| v.version());
        if self.terrain.map(|t| t.0) == version {
            return;
        }
        if let Some((_, body)) = self.terrain.take() {
            self.world.remove_body_with_colliders(body, true);
        }
        let (Some(version), Some(voxels)) = (version, voxels) else { return };
        let mut cells = Vec::new();
        for chunk in voxels.chunks() {
            for ((x, y, z), material) in voxels.cells_in_chunk(chunk) {
                if material != libroxel_instance::Material::Water {
                    cells.push(rapier3d::math::IVector::new(x, y, z));
                }
            }
        }
        let body = self.world.insert_body(RigidBodyBuilder::fixed());
        if !cells.is_empty() {
            let collider = ColliderBuilder::voxels(RVec::ONE, &cells).friction(0.5).user_data(u128::MAX);
            let bodies = &mut self.world.bodies;
            self.world.colliders.insert_with_parent(collider, body, bodies);
        }
        self.terrain = Some((version, body));
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
        velocities: &mut HashMap<InstanceId, Vec3>,
        spins: &mut HashMap<InstanceId, Vec3>,
        voxels: Option<&Voxels>,
        gravity: f32,
        dt: f32,
    ) -> Vec<Moved> {
        self.setup();
        self.sync_terrain(voxels);
        let live: HashSet<InstanceId> = specs.iter().map(|s| s.id).collect();
        let gone: Vec<InstanceId> = self.parts.keys().filter(|id| !live.contains(id)).copied().collect();
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

        // Run it, in slices of at most a sixtieth of a second, sliding
        // kinematic parts along to where they are going.
        self.world.gravity = RVec::new(0.0, -gravity, 0.0);
        let slices = ((dt / (1.0 / 60.0)).ceil() as usize).clamp(1, 8);
        self.world.integration_parameters.dt = dt / slices as f32;
        let filter = Filter { ignored };
        for k in 1..=slices {
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

    /// Whether a dynamic part ended the last step resting on something.
    pub(crate) fn is_grounded(&self, id: InstanceId) -> bool {
        self.grounded.contains(&id)
    }
}
