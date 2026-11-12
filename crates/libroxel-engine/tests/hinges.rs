//! Hinges, motors, servos and spinning parts.

use libroxel_engine::CharacterInput;
use libroxel_engine::session::start_on;
use libroxel_instance::{
    ActuatorType, ClassKind, EnumValue, InstanceId, InstanceTree, Property, PropertyValue, Vec3,
};

const DT: f32 = 1.0 / 60.0;

fn part(tree: &mut InstanceTree, name: &str, pos: Vec3, size: Vec3, anchored: bool) -> InstanceId {
    let id = tree.create(ClassKind::Part, name).unwrap();
    tree.set_parent(id, Some(tree.workspace())).unwrap();
    tree.set(id, Property::Position, PropertyValue::Vec3(pos)).unwrap();
    tree.set(id, Property::Size, PropertyValue::Vec3(size)).unwrap();
    tree.set(id, Property::Anchored, PropertyValue::Bool(anchored)).unwrap();
    id
}

/// An attachment whose X axis points along world Z (when its part isn't turned).
fn attach(tree: &mut InstanceTree, part: InstanceId, offset: Vec3) -> InstanceId {
    let a = tree.create(ClassKind::Attachment, "Attachment").unwrap();
    tree.set(a, Property::Position, PropertyValue::Vec3(offset)).unwrap();
    tree.set(a, Property::Orientation, PropertyValue::Vec3(Vec3::new(0.0, -90.0, 0.0))).unwrap();
    tree.set_parent(a, Some(part)).unwrap();
    a
}

fn hinge(
    tree: &mut InstanceTree,
    a0: InstanceId,
    a1: InstanceId,
    props: &[(Property, PropertyValue)],
) -> InstanceId {
    let id = tree.create(ClassKind::HingeConstraint, "Hinge").unwrap();
    tree.set(id, Property::Attachment0, PropertyValue::Ref(Some(a0))).unwrap();
    tree.set(id, Property::Attachment1, PropertyValue::Ref(Some(a1))).unwrap();
    for (p, v) in props {
        tree.set(id, *p, v.clone()).unwrap();
    }
    tree.set_parent(id, Some(tree.workspace())).unwrap();
    id
}

fn actuator(kind: ActuatorType) -> (Property, PropertyValue) {
    (Property::ActuatorType, PropertyValue::Enum(EnumValue::ActuatorType(kind)))
}

fn world() -> InstanceTree {
    let mut tree = InstanceTree::new();
    part(&mut tree, "Floor", Vec3::new(0.0, -0.5, 0.0), Vec3::new(400.0, 1.0, 400.0), true);
    tree
}

fn run(tree: InstanceTree, seconds: f32) -> libroxel_script::SharedTree {
    let mut session = start_on(tree, false).unwrap();
    let shared = session.runtime().tree().clone();
    for _ in 0..(seconds / DT).round() as usize {
        session.step(DT, CharacterInput::default());
    }
    let log = session.take_log();
    assert!(log.iter().all(|l| l.level != libroxel_engine::LogLevel::Error), "{log:?}");
    shared
}

fn vec(tree: &libroxel_script::SharedTree, id: InstanceId, p: Property) -> Vec3 {
    tree.borrow().get_vec3(id, p).unwrap()
}

fn angle(tree: &libroxel_script::SharedTree, hinge: InstanceId) -> f64 {
    tree.borrow().get_number(hinge, Property::CurrentAngle).unwrap()
}

/// An anchored post at (0, 10, 0) and a loose arm 3 studs to its right,
/// hinged about the Z axis through the post.
fn pendulum(tree: &mut InstanceTree) -> (InstanceId, InstanceId, InstanceId) {
    let post = part(tree, "Post", Vec3::new(0.0, 10.0, 0.0), Vec3::new(1.0, 1.0, 1.0), true);
    let arm = part(tree, "Arm", Vec3::new(3.0, 10.0, 0.0), Vec3::new(1.0, 1.0, 1.0), false);
    let a0 = attach(tree, post, Vec3::ZERO);
    let a1 = attach(tree, arm, Vec3::new(-3.0, 0.0, 0.0));
    (arm, a0, a1)
}

#[test]
fn a_free_hinge_swings_down_and_keeps_its_length() {
    let mut tree = world();
    let (arm, a0, a1) = pendulum(&mut tree);
    let h = hinge(&mut tree, a0, a1, &[]);
    let t = run(tree, 0.25);
    let at = vec(&t, arm, Property::Position);
    assert!(at.y < 9.0, "swung down: {at:?}");
    assert!((at.z).abs() < 1e-3, "stays in its plane: {at:?}");
    assert!(((at - Vec3::new(0.0, 10.0, 0.0)).length() - 3.0).abs() < 0.01, "{at:?}");
    // Swinging down to the right turns it clockwise, seen from +Z.
    assert!(angle(&t, h) < -10.0, "{}", angle(&t, h));
}

#[test]
fn a_servo_turns_to_its_target_angle_and_holds_it() {
    let mut tree = world();
    let (arm, a0, a1) = pendulum(&mut tree);
    let h = hinge(
        &mut tree,
        a0,
        a1,
        &[
            actuator(ActuatorType::Servo),
            (Property::TargetAngle, PropertyValue::Number(90.0)),
            (Property::AngularSpeed, PropertyValue::Number(4.0)),
        ],
    );
    let t = run(tree, 1.5);
    assert!((angle(&t, h) - 90.0).abs() < 1.0, "{}", angle(&t, h));
    // A quarter turn anticlockwise (seen from +Z) lifts the arm straight up.
    let at = vec(&t, arm, Property::Position);
    assert!((at - Vec3::new(0.0, 13.0, 0.0)).length() < 0.1, "{at:?}");
}

#[test]
fn a_weak_servo_sags_under_the_arm() {
    let mut tree = world();
    let (_, a0, a1) = pendulum(&mut tree);
    let h = hinge(
        &mut tree,
        a0,
        a1,
        &[
            actuator(ActuatorType::Servo),
            (Property::TargetAngle, PropertyValue::Number(0.0)),
            (Property::ServoMaxTorque, PropertyValue::Number(1.0)),
        ],
    );
    let t = run(tree, 0.5);
    assert!(angle(&t, h) < -20.0, "{}", angle(&t, h));
}

#[test]
fn a_motor_spins_a_fan_blade_about_its_middle() {
    let mut tree = world();
    let hub = part(&mut tree, "Hub", Vec3::new(0.0, 10.0, 0.0), Vec3::new(1.0, 1.0, 1.0), true);
    let blade = part(&mut tree, "Blade", Vec3::new(0.0, 10.0, -1.0), Vec3::new(8.0, 1.0, 0.5), false);
    let a0 = attach(&mut tree, hub, Vec3::new(0.0, 0.0, -1.0));
    let a1 = attach(&mut tree, blade, Vec3::ZERO);
    let speed = std::f32::consts::PI; // half a turn a second
    let h = hinge(
        &mut tree,
        a0,
        a1,
        &[actuator(ActuatorType::Motor), (Property::MotorVelocity, PropertyValue::Number(speed as f64))],
    );
    let t = run(tree, 0.5);
    assert!((angle(&t, h) - 90.0).abs() < 3.0, "{}", angle(&t, h));
    assert!((vec(&t, blade, Property::Position) - Vec3::new(0.0, 10.0, -1.0)).length() < 1e-3);
    let turned = vec(&t, blade, Property::Orientation);
    assert!((turned.z.abs() - 90.0).abs() < 3.0, "{turned:?}");
    // It keeps going round, past half a turn.
    let t2 = {
        let tree = t.borrow().clone();
        run(tree, 0.75)
    };
    let a = angle(&t2, h);
    assert!(a < -120.0 || a > 170.0, "{a}");
}

#[test]
fn welded_parts_ride_on_a_hinge_arm() {
    let mut tree = world();
    let (arm, a0, a1) = pendulum(&mut tree);
    let tip = part(&mut tree, "Tip", Vec3::new(4.0, 10.0, 0.0), Vec3::new(1.0, 1.0, 1.0), false);
    let weld = tree.create(ClassKind::WeldConstraint, "Weld").unwrap();
    tree.set(weld, Property::Part0, PropertyValue::Ref(Some(arm))).unwrap();
    tree.set(weld, Property::Part1, PropertyValue::Ref(Some(tip))).unwrap();
    tree.set_parent(weld, Some(arm)).unwrap();
    hinge(
        &mut tree,
        a0,
        a1,
        &[
            actuator(ActuatorType::Servo),
            (Property::TargetAngle, PropertyValue::Number(-90.0)),
            (Property::AngularSpeed, PropertyValue::Number(6.0)),
        ],
    );
    let t = run(tree, 1.0);
    assert!((vec(&t, arm, Property::Position) - Vec3::new(0.0, 7.0, 0.0)).length() < 0.1);
    assert!((vec(&t, tip, Property::Position) - Vec3::new(0.0, 6.0, 0.0)).length() < 0.1);
}

#[test]
fn spinners_and_scripts_turn_loose_parts() {
    let mut tree = world();
    // A top on the floor, kept spinning by an AngularVelocity.
    let top = part(&mut tree, "Top", Vec3::new(0.0, 1.0, 0.0), Vec3::new(2.0, 2.0, 2.0), false);
    let a = attach(&mut tree, top, Vec3::ZERO);
    let spinner = tree.create(ClassKind::AngularVelocity, "Spin").unwrap();
    tree.set(spinner, Property::Attachment0, PropertyValue::Ref(Some(a))).unwrap();
    tree.set(spinner, Property::AngularVelocity, PropertyValue::Vec3(Vec3::new(0.0, 3.0, 0.0))).unwrap();
    tree.set_parent(spinner, Some(top)).unwrap();
    // A crate with a body spinner.
    let crate_ = part(&mut tree, "Crate", Vec3::new(10.0, 1.0, 0.0), Vec3::new(2.0, 2.0, 2.0), false);
    let body = tree.create(ClassKind::BodyAngularVelocity, "Spin").unwrap();
    tree.set(body, Property::AngularVelocity, PropertyValue::Vec3(Vec3::new(0.0, -3.0, 0.0))).unwrap();
    tree.set_parent(body, Some(crate_)).unwrap();
    // A ball a script sets spinning.
    let ball = part(&mut tree, "Ball", Vec3::new(-10.0, 1.0, 0.0), Vec3::new(2.0, 2.0, 2.0), false);
    let script = tree.create(ClassKind::Script, "Spin").unwrap();
    let source = r#"
        workspace.Ball.AssemblyAngularVelocity = Vector3.new(0, 2, 0)
        print(workspace.Ball.AssemblyAngularVelocity.Y)
    "#;
    tree.set(script, Property::Source, PropertyValue::String(source.into())).unwrap();
    tree.set_parent(script, Some(tree.workspace())).unwrap();

    let mut session = start_on(tree, false).unwrap();
    for _ in 0..30 {
        session.step(DT, CharacterInput::default());
    }
    let printed: Vec<_> = session.take_log().iter().map(|l| l.text.clone()).collect();
    assert!(printed.contains(&"2".to_owned()), "{printed:?}");
    let t = session.runtime().tree().clone();
    // Half a second at 3 radians a second is about 86 degrees.
    let yaw = vec(&t, top, Property::Orientation).y;
    assert!((yaw - 86.0).abs() < 6.0, "{yaw}");
    assert!((vec(&t, crate_, Property::Orientation).y + 86.0).abs() < 10.0);
    // The ball rubs on the floor, so it soon stops.
    let ball_yaw = vec(&t, ball, Property::Orientation).y;
    assert!((0.0..20.0).contains(&ball_yaw), "{ball_yaw}");
    // Spinning parts stay on the floor.
    assert!((vec(&t, top, Property::Position).y - 1.0).abs() < 0.5);
}

/// An attachment at `offset` whose X axis is the world's `x_to` direction.
fn attach_facing(tree: &mut InstanceTree, part: InstanceId, offset: Vec3, orientation: Vec3) -> InstanceId {
    let a = tree.create(ClassKind::Attachment, "Attachment").unwrap();
    tree.set(a, Property::Position, PropertyValue::Vec3(offset)).unwrap();
    tree.set(a, Property::Orientation, PropertyValue::Vec3(orientation)).unwrap();
    tree.set_parent(a, Some(part)).unwrap();
    a
}

fn motor(speed: f64) -> [(Property, PropertyValue); 3] {
    [
        actuator(ActuatorType::Motor),
        (Property::MotorVelocity, PropertyValue::Number(speed)),
        (Property::MotorMaxTorque, PropertyValue::Number(1e9)),
    ]
}

#[test]
fn a_turning_arm_shoves_what_it_meets() {
    let mut tree = world();
    // A post with a bar sweeping round it, about the vertical.
    let post = part(&mut tree, "Post", Vec3::new(0.0, 1.5, 0.0), Vec3::new(1.0, 3.0, 1.0), true);
    let bar = part(&mut tree, "Bar", Vec3::new(4.5, 1.5, 0.0), Vec3::new(8.0, 1.0, 1.0), false);
    let up = Vec3::new(0.0, 0.0, 90.0);
    let a0 = attach_facing(&mut tree, post, Vec3::ZERO, up);
    let a1 = attach_facing(&mut tree, bar, Vec3::new(-4.5, 0.0, 0.0), up);
    hinge(&mut tree, a0, a1, &motor(2.0));
    // A crate in the bar's way (turning about +Y takes +X towards -Z).
    let crate_ = part(&mut tree, "Crate", Vec3::new(0.0, 1.0, -5.0), Vec3::new(2.0, 2.0, 2.0), false);
    let shared = run(tree, 1.5);
    let at = vec(&shared, crate_, Property::Position);
    let moved = ((at.x).powi(2) + (at.z + 5.0).powi(2)).sqrt();
    assert!(moved > 3.0, "the crate was shoved {moved} studs, to {at:?}");
    // It wasn't pushed into the floor or through the bar.
    assert!(at.y > 0.9, "{at:?}");
}

#[test]
fn motor_wheels_drive_a_car() {
    let mut tree = world();
    let chassis = part(&mut tree, "Chassis", Vec3::new(0.0, 2.0, 0.0), Vec3::new(6.0, 1.0, 4.0), false);
    let mut hinges = Vec::new();
    for (x, z) in [(3.5, 1.5), (-3.5, 1.5), (3.5, -1.5), (-3.5, -1.5)] {
        let wheel = part(&mut tree, "Wheel", Vec3::new(x, 1.0, z), Vec3::new(1.0, 2.0, 2.0), false);
        tree.set(
            wheel,
            Property::Shape,
            PropertyValue::Enum(libroxel_instance::EnumValue::Shape(libroxel_instance::PartShape::Cylinder)),
        )
        .unwrap();
        // Left and right hinges face opposite ways, as people often build them.
        let (facing, speed) = if x > 0.0 { (Vec3::ZERO, 5.0) } else { (Vec3::new(0.0, 180.0, 0.0), -5.0) };
        let a0 = attach_facing(&mut tree, chassis, Vec3::new(x, -1.0, z), facing);
        let a1 = attach_facing(&mut tree, wheel, Vec3::ZERO, facing);
        hinges.push(hinge(&mut tree, a0, a1, &motor(speed)));
    }
    let shared = run(tree, 2.0);
    let at = vec(&shared, chassis, Property::Position);
    // Wheels turning forwards about +X roll it towards +Z, about 5 studs a second.
    assert!(at.z > 6.0, "it drove to {at:?}");
    assert!(at.x.abs() < 0.5, "it went straight: {at:?}");
    assert!((at.y - 2.0).abs() < 0.2, "the wheels hold it up: {at:?}");
    assert!(angle(&shared, hinges[0]).abs() > 0.0);
}

#[test]
fn wheels_at_different_speeds_turn_a_car() {
    let mut tree = world();
    let chassis = part(&mut tree, "Chassis", Vec3::new(0.0, 2.0, 0.0), Vec3::new(6.0, 1.0, 4.0), false);
    for (x, z) in [(3.5, 1.5), (-3.5, 1.5), (3.5, -1.5), (-3.5, -1.5)] {
        let wheel = part(&mut tree, "Wheel", Vec3::new(x, 1.0, z), Vec3::new(1.0, 2.0, 2.0), false);
        tree.set(
            wheel,
            Property::Shape,
            PropertyValue::Enum(libroxel_instance::EnumValue::Shape(libroxel_instance::PartShape::Cylinder)),
        )
        .unwrap();
        // Only the right-hand wheels drive; the left ones hold still.
        let speed = if x > 0.0 { 5.0 } else { 0.0 };
        let a0 = attach_facing(&mut tree, chassis, Vec3::new(x, -1.0, z), Vec3::ZERO);
        let a1 = attach_facing(&mut tree, wheel, Vec3::ZERO, Vec3::ZERO);
        hinge(&mut tree, a0, a1, &motor(speed));
    }
    let shared = run(tree, 1.5);
    let turned = vec(&shared, chassis, Property::Orientation).y;
    let at = vec(&shared, chassis, Property::Position);
    // Pushed forward on the right only, it swings round to the left.
    assert!(turned.abs() > 30.0, "it turned {turned} degrees, to {at:?}");
    assert!((at.y - 2.0).abs() < 0.3, "still held up: {at:?}");
}

#[test]
fn front_wheels_on_steering_hinges_turn_a_car() {
    let mut tree = world();
    let chassis = part(&mut tree, "Chassis", Vec3::new(0.0, 2.0, 0.0), Vec3::new(4.0, 1.0, 6.0), false);
    let cylinder =
        PropertyValue::Enum(libroxel_instance::EnumValue::Shape(libroxel_instance::PartShape::Cylinder));
    let mut steering = Vec::new();
    for (x, z) in [(2.5, -2.5), (-2.5, -2.5), (2.5, 2.5), (-2.5, 2.5)] {
        let wheel = part(&mut tree, "Wheel", Vec3::new(x, 1.0, z), Vec3::new(1.0, 2.0, 2.0), false);
        tree.set(wheel, Property::Shape, cylinder.clone()).unwrap();
        let hub = attach_facing(&mut tree, wheel, Vec3::ZERO, Vec3::ZERO);
        // Wheels turning about +X roll the car towards +Z... so these spin
        // the other way to drive towards -Z, the front.
        let drive = motor(-5.0);
        if z < 0.0 {
            // A front wheel hangs from a knuckle that a servo turns about the vertical.
            let knuckle = part(&mut tree, "Knuckle", Vec3::new(x, 1.0, z), Vec3::new(0.4, 0.4, 0.4), false);
            tree.set(knuckle, Property::CanCollide, PropertyValue::Bool(false)).unwrap();
            let up = Vec3::new(0.0, 0.0, 90.0);
            let a0 = attach_facing(&mut tree, chassis, Vec3::new(x, -1.0, z), up);
            let a1 = attach_facing(&mut tree, knuckle, Vec3::ZERO, up);
            steering.push(hinge(
                &mut tree,
                a0,
                a1,
                &[
                    actuator(ActuatorType::Servo),
                    (Property::TargetAngle, PropertyValue::Number(25.0)),
                    (Property::AngularSpeed, PropertyValue::Number(4.0)),
                    (Property::ServoMaxTorque, PropertyValue::Number(1e9)),
                ],
            ));
            let axle = attach_facing(&mut tree, knuckle, Vec3::ZERO, Vec3::ZERO);
            hinge(&mut tree, axle, hub, &drive);
        } else {
            let axle = attach_facing(&mut tree, chassis, Vec3::new(x, -1.0, z), Vec3::ZERO);
            hinge(&mut tree, axle, hub, &drive);
        }
    }
    let shared = run(tree, 2.0);
    let at = vec(&shared, chassis, Property::Position);
    let turned = vec(&shared, chassis, Property::Orientation).y;
    assert!(at.z < -4.0, "it drove forwards: {at:?}");
    assert!(turned.abs() > 15.0, "and turned {turned} degrees");
    assert!((at.y - 2.0).abs() < 0.3, "{at:?}");
    assert!((angle(&shared, steering[0]) - 25.0).abs() < 1.0);
}
