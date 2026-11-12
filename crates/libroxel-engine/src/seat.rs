//! Seats and vehicles. A character that touches a `Seat` or `VehicleSeat`
//! sits in it: it is held on the seat (moving with it) until it jumps, a
//! script sets `Humanoid.Sit` to false, or the seat goes away. While the
//! player sits in a vehicle seat, the movement keys set its `Throttle` and
//! `Steer`, and every vehicle seat drives the assembly it is welded into.

use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue, Rotation, Vec3};

use crate::character::{BODY_SIZE, Character, CharacterInput};
use crate::physics::Physics;
use crate::weld;

/// How long after getting up before a character can sit again, in seconds.
pub const SIT_COOLDOWN: f32 = 1.0;
/// How far the character's middle sits above the seat's top.
const SIT_HEIGHT: f32 = BODY_SIZE.y / 2.0 - 2.0;

/// Who sits where.
#[derive(Debug, Default)]
pub struct Seating {
    seat: Option<InstanceId>,
    cooldown: f32,
}

/// A change in who sits where, for the `Seated` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeatChange {
    pub humanoid: InstanceId,
    pub seat: Option<InstanceId>,
}

fn usable(tree: &InstanceTree, seat: InstanceId) -> bool {
    tree.class(seat).is_ok_and(|c| c.is_seat())
        && !tree.is_destroyed(seat).unwrap_or(true)
        && tree.is_descendant_of(seat, tree.workspace()).unwrap_or(false)
        && tree.get_bool(seat, Property::Disabled) != Some(true)
}

/// The seat's forward direction, flat on the ground.
fn forward(tree: &InstanceTree, seat: InstanceId) -> Vec3 {
    let o = tree.get_vec3(seat, Property::Orientation).unwrap_or_default();
    let f = Rotation::from_orientation(o).apply(Vec3::new(0.0, 0.0, -1.0));
    let flat = Vec3::new(f.x, 0.0, f.z).normalize_or_zero();
    if flat == Vec3::ZERO { Vec3::new(0.0, 0.0, -1.0) } else { flat }
}

/// The parts a seat moves with: its weld assembly, or just itself.
fn assembly(tree: &InstanceTree, seat: InstanceId) -> (InstanceId, Vec<InstanceId>) {
    match weld::assemblies(tree).into_iter().find(|g| g.contains(&seat)) {
        Some(parts) => (weld::root_of(tree, &parts), parts),
        None => (seat, vec![seat]),
    }
}

impl Seating {
    /// The seat the player's character is in.
    pub fn seat(&self) -> Option<InstanceId> {
        self.seat
    }

    /// Sits `character` in `seat` if it's free.
    pub fn sit(
        &mut self,
        tree: &mut InstanceTree,
        character: Character,
        seat: InstanceId,
    ) -> Option<SeatChange> {
        if self.seat.is_some() || !usable(tree, seat) || tree.get_ref(seat, Property::Occupant).is_some() {
            return None;
        }
        self.seat = Some(seat);
        let _ = tree.set(seat, Property::Occupant, PropertyValue::Ref(Some(character.humanoid)));
        let _ = tree.set(character.humanoid, Property::Sit, PropertyValue::Bool(true));
        let _ = tree.set(character.humanoid, Property::SeatPart, PropertyValue::Ref(Some(seat)));
        Some(SeatChange { humanoid: character.humanoid, seat: Some(seat) })
    }

    /// Gets the character up, if it's sitting.
    pub fn stand(&mut self, tree: &mut InstanceTree, character: Character) -> Option<SeatChange> {
        let seat = self.seat.take()?;
        self.cooldown = SIT_COOLDOWN;
        if tree.exists(seat) && !tree.is_destroyed(seat).unwrap_or(true) {
            let _ = tree.set(seat, Property::Occupant, PropertyValue::Ref(None));
            if tree.class(seat) == Ok(ClassKind::VehicleSeat) {
                let _ = tree.set(seat, Property::Throttle, PropertyValue::Number(0.0));
                let _ = tree.set(seat, Property::Steer, PropertyValue::Number(0.0));
            }
        }
        if tree.exists(character.humanoid) && !tree.is_destroyed(character.humanoid).unwrap_or(true) {
            let _ = tree.set(character.humanoid, Property::Sit, PropertyValue::Bool(false));
            let _ = tree.set(character.humanoid, Property::SeatPart, PropertyValue::Ref(None));
        }
        Some(SeatChange { humanoid: character.humanoid, seat: None })
    }

    /// Before physics: gets up when the character jumps or a script says
    /// so, turns the movement keys into the vehicle's throttle and steer,
    /// and returns the input the character itself should get (none while
    /// sitting) plus any change in seating.
    pub fn before_physics(
        &mut self,
        tree: &mut InstanceTree,
        character: Option<Character>,
        input: CharacterInput,
        dt: f32,
    ) -> (CharacterInput, Option<SeatChange>) {
        self.cooldown = (self.cooldown - dt).max(0.0);
        let (Some(character), Some(seat)) = (character, self.seat) else { return (input, None) };
        let alive = !character.is_dead(tree);
        let told_to_stand = tree.get_bool(character.humanoid, Property::Sit) == Some(false);
        if !alive || !usable(tree, seat) || told_to_stand || input.jump {
            let change = self.stand(tree, character);
            // Jump clear of the seat.
            let up = if alive && input.jump {
                if let Some(pos) = tree.get_vec3(character.root, Property::Position) {
                    let _ = tree.set(
                        character.root,
                        Property::Position,
                        PropertyValue::Vec3(pos + Vec3::new(0.0, 1.0, 0.0)),
                    );
                }
                CharacterInput { jump: true, ..Default::default() }
            } else {
                CharacterInput::default()
            };
            return (up, change);
        }
        if tree.class(seat) == Ok(ClassKind::VehicleSeat) {
            let (mut throttle, mut steer) = (input.throttle, input.steer);
            if throttle == 0.0 && steer == 0.0 && input.move_dir != Vec3::ZERO {
                // Only a direction: drive toward it, relative to the seat.
                let f = forward(tree, seat);
                let right = Vec3::new(-f.z, 0.0, f.x);
                let along = |v: f32| {
                    if v > 0.3 {
                        1.0
                    } else if v < -0.3 {
                        -1.0
                    } else {
                        0.0
                    }
                };
                throttle = along(input.move_dir.dot(f));
                steer = along(input.move_dir.dot(right));
            }
            for (property, value) in [(Property::Throttle, throttle), (Property::Steer, steer)] {
                if tree.get_number(seat, property) != Some(value as f64) {
                    let _ = tree.set(seat, property, PropertyValue::Number(value as f64));
                }
            }
        }
        (CharacterInput::default(), None)
    }

    /// Holds the sitting character still during the physics step.
    pub fn hold(&self, tree: &InstanceTree, physics: &mut Physics, character: Option<Character>) {
        let (Some(character), Some(seat)) = (character, self.seat) else { return };
        let (_, parts) = assembly(tree, seat);
        physics.carry(character.root, &parts);
    }

    /// After physics: puts the sitting character on its seat.
    pub fn after_physics(&self, tree: &mut InstanceTree, character: Option<Character>) {
        let (Some(character), Some(seat)) = (character, self.seat) else { return };
        let (Some(pos), Some(size)) =
            (tree.get_vec3(seat, Property::Position), tree.get_vec3(seat, Property::Size))
        else {
            return;
        };
        let o = tree.get_vec3(seat, Property::Orientation).unwrap_or_default();
        let up = Rotation::from_orientation(o).apply(Vec3::new(0.0, size.y / 2.0 + SIT_HEIGHT, 0.0));
        let _ = tree.set(character.root, Property::Position, PropertyValue::Vec3(pos + up));
        let facing = PropertyValue::Vec3(Vec3::new(0.0, o.y, 0.0));
        let _ = tree.set(character.root, Property::Orientation, facing);
        character.follow(tree);
    }

    /// Whether the character may sit down now (it isn't sitting and didn't just get up).
    pub fn can_sit(&self) -> bool {
        self.seat.is_none() && self.cooldown <= 0.0
    }
}

/// Drives every vehicle seat's assembly by its `Throttle` and `Steer`:
/// speeds it up along the seat's front toward `MaxSpeed` (as fast as its
/// `Torque` allows), grips the ground so it doesn't slide sideways, and
/// turns it at up to `TurnSpeed` radians a second while it moves.
pub fn drive_vehicles(tree: &mut InstanceTree, physics: &mut Physics, dt: f32) {
    for seat in tree.descendants_of_class(tree.workspace(), ClassKind::VehicleSeat) {
        let throttle = tree.get_number(seat, Property::Throttle).unwrap_or(0.0) as f32;
        let steer = tree.get_number(seat, Property::Steer).unwrap_or(0.0) as f32;
        let (root, _) = assembly(tree, seat);
        if tree.get_bool(root, Property::Anchored) != Some(false) || !physics.is_grounded(root) {
            continue;
        }
        physics.glide(root);
        let max_speed = tree.get_number(seat, Property::MaxSpeed).unwrap_or(25.0) as f32;
        let torque = tree.get_number(seat, Property::Torque).unwrap_or(10.0) as f32;
        let turn_speed = tree.get_number(seat, Property::TurnSpeed).unwrap_or(1.0) as f32;
        let f = forward(tree, seat);
        let v = physics.velocity(root);
        let along = v.dot(f);
        let side = Vec3::new(v.x, 0.0, v.z) - f * along;
        let target = throttle * max_speed;
        let step = torque * 5.0 * dt;
        // It glides over the ground (see `Physics::glide`): the seat does
        // the speeding up, and slowing down when the throttle is let go.
        let along = if throttle != 0.0 {
            along + (target - along).clamp(-step, step)
        } else {
            along * (1.0 - 2.0 * dt).max(0.0)
        };
        let grip = (1.0 - 10.0 * dt).max(0.0);
        let new_v = f * along + side * grip + Vec3::new(0.0, v.y, 0.0);
        if new_v != v {
            physics.set_velocity(root, new_v);
        }
        if steer != 0.0 && along.abs() > 0.1 {
            let turn = -steer * turn_speed * (along / 8.0).clamp(-1.0, 1.0) * dt;
            let o = tree.get_vec3(root, Property::Orientation).unwrap_or_default();
            let turned = Vec3::new(o.x, o.y + turn.to_degrees(), o.z);
            let _ = tree.set(root, Property::Orientation, PropertyValue::Vec3(turned));
        }
    }
}

/// The seat among `touches` that the character's root just touched, if any.
pub fn touched_seat(
    tree: &InstanceTree,
    root: InstanceId,
    touches: &[(InstanceId, InstanceId)],
) -> Option<InstanceId> {
    touches.iter().find_map(|&(a, b)| {
        let other = if a == root {
            b
        } else if b == root {
            a
        } else {
            return None;
        };
        (usable(tree, other) && tree.get_ref(other, Property::Occupant).is_none()).then_some(other)
    })
}
