//! Starter worlds: the blank template, and the pieces (like karts) the
//! starter games in `starter_games` are built from.

use libroxel_instance::{
    ClassKind, Color3, EnumValue, InstanceId, InstanceTree, Material, PartShape, Property, PropertyValue,
    Vec3,
};

/// Builds a go-kart standing on the ground at `at`, facing `yaw` degrees
/// (0 faces -Z): a chassis with a vehicle seat, wheels and a bumper welded on.
pub fn kart(tree: &mut InstanceTree, parent: InstanceId, at: Vec3, yaw: f32, rgb: [u8; 3]) -> InstanceId {
    let turn = libroxel_instance::Rotation::about_y(yaw.to_radians());
    let model = add(tree, parent, ClassKind::Model, "Kart");
    let place = |tree: &mut InstanceTree, id: InstanceId, offset: [f32; 3]| {
        let p = at + turn.apply(Vec3::new(offset[0], offset[1], offset[2]));
        set(tree, id, Property::Position, PropertyValue::Vec3(p));
        orient(tree, id, [0.0, yaw, 0.0]);
        set(tree, id, Property::Anchored, PropertyValue::Bool(false));
    };
    let chassis = block(tree, model, "Chassis", [0.0; 3], [4.0, 1.0, 5.0], rgb);
    place(tree, chassis, [0.0, 0.5, 0.0]);
    let seat = add(tree, model, ClassKind::VehicleSeat, "VehicleSeat");
    set(tree, seat, Property::Size, PropertyValue::Vec3(Vec3::new(2.0, 1.0, 2.0)));
    set(tree, seat, Property::Color, PropertyValue::Color3(Color3::from_rgb8(40, 40, 45)));
    place(tree, seat, [0.0, 1.5, 0.8]);
    let mut parts = vec![seat];
    for (i, (x, z)) in [(-2.3, -1.7), (2.3, -1.7), (-2.3, 1.7), (2.3, 1.7)].into_iter().enumerate() {
        let wheel = block(tree, model, &format!("Wheel{}", i + 1), [0.0; 3], [0.6, 1.4, 1.4], [30, 30, 30]);
        shape(tree, wheel, PartShape::Cylinder);
        material(tree, wheel, Material::SmoothPlastic);
        set(tree, wheel, Property::CanCollide, PropertyValue::Bool(false));
        place(tree, wheel, [x, 0.5, z]);
        parts.push(wheel);
    }
    let bumper = block(tree, model, "Bumper", [0.0; 3], [4.0, 0.6, 0.4], [230, 230, 235]);
    material(tree, bumper, Material::Metal);
    place(tree, bumper, [0.0, 0.6, -2.7]);
    parts.push(bumper);
    for part in parts {
        let name = format!("{}Weld", tree.name(part).unwrap_or("Part"));
        let weld = add(tree, chassis, ClassKind::WeldConstraint, &name);
        set(tree, weld, Property::Part0, PropertyValue::Ref(Some(chassis)));
        set(tree, weld, Property::Part1, PropertyValue::Ref(Some(part)));
    }
    model
}

pub(crate) fn add(tree: &mut InstanceTree, parent: InstanceId, class: ClassKind, name: &str) -> InstanceId {
    let id = tree.create(class, name).expect("template classes are creatable");
    tree.set_parent(id, Some(parent)).expect("template parents are valid");
    id
}

pub(crate) fn block(
    tree: &mut InstanceTree,
    parent: InstanceId,
    name: &str,
    pos: [f32; 3],
    size: [f32; 3],
    rgb: [u8; 3],
) -> InstanceId {
    let id = add(tree, parent, ClassKind::Part, name);
    let set = |tree: &mut InstanceTree, p, v| tree.set(id, p, v).expect("template properties are valid");
    set(tree, Property::Position, PropertyValue::Vec3(Vec3::new(pos[0], pos[1], pos[2])));
    set(tree, Property::Size, PropertyValue::Vec3(Vec3::new(size[0], size[1], size[2])));
    set(tree, Property::Color, PropertyValue::Color3(Color3::from_rgb8(rgb[0], rgb[1], rgb[2])));
    set(tree, Property::Anchored, PropertyValue::Bool(true));
    id
}

pub(crate) fn set(tree: &mut InstanceTree, id: InstanceId, property: Property, value: PropertyValue) {
    tree.set(id, property, value).expect("template properties are valid");
}

pub(crate) fn material(tree: &mut InstanceTree, id: InstanceId, material: Material) {
    set(tree, id, Property::Material, PropertyValue::Enum(EnumValue::Material(material)));
}

pub(crate) fn shape(tree: &mut InstanceTree, id: InstanceId, shape: PartShape) {
    set(tree, id, Property::Shape, PropertyValue::Enum(EnumValue::Shape(shape)));
}

pub(crate) fn orient(tree: &mut InstanceTree, id: InstanceId, degrees: [f32; 3]) {
    set(tree, id, Property::Orientation, PropertyValue::Vec3(Vec3::new(degrees[0], degrees[1], degrees[2])));
}

pub(crate) fn script(tree: &mut InstanceTree, parent: InstanceId, name: &str, source: &str) -> InstanceId {
    code(tree, parent, ClassKind::Script, name, source)
}

pub(crate) fn code(
    tree: &mut InstanceTree,
    parent: InstanceId,
    class: ClassKind,
    name: &str,
    source: &str,
) -> InstanceId {
    let id = add(tree, parent, class, name);
    tree.set(id, Property::Source, PropertyValue::String(source.trim_start().to_owned()))
        .expect("scripts have Source");
    id
}

/// A baseplate and a spawn location.
pub fn blank() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    block(&mut tree, ws, "Baseplate", [0.0, -0.5, 0.0], [128.0, 1.0, 128.0], [99, 140, 88]);
    let spawn = add(&mut tree, ws, ClassKind::SpawnLocation, "SpawnLocation");
    tree.set(spawn, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, 0.5, 0.0))).unwrap();
    tree.set(spawn, Property::Size, PropertyValue::Vec3(Vec3::new(6.0, 1.0, 6.0))).unwrap();
    tree.drain_changes();
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use libroxel_engine::CharacterInput;
    use libroxel_engine::session::start_on;
    use libroxel_instance::place;

    #[test]
    fn templates_save_and_load() {
        let text = place::save(&blank()).unwrap();
        let (loaded, report) = place::load(&text).unwrap();
        assert!(report.warnings.is_empty());
        assert_eq!(place::save(&loaded).unwrap(), text);
    }

    #[test]
    fn karts_turn_tightly_brake_hard_and_stay_upright() {
        let frame = |s: &mut libroxel_engine::PlaySession, n: usize, input: CharacterInput| {
            for _ in 0..n {
                s.step(1.0 / 60.0, input);
            }
        };
        let idle = CharacterInput::default();
        let mut world = blank();
        let ws = world.workspace();
        kart(&mut world, ws, Vec3::new(30.0, 0.0, 0.0), 0.0, [200, 50, 50]);
        let mut session = start_on(world, true).unwrap();
        frame(&mut session, 30, idle);
        let tree = session.runtime().tree().clone();
        let (chassis, seat) = {
            let t = tree.borrow();
            let kart = t.find_first_child(t.workspace(), "Kart").unwrap().unwrap();
            (
                t.find_first_child(kart, "Chassis").unwrap().unwrap(),
                t.find_first_child(kart, "VehicleSeat").unwrap().unwrap(),
            )
        };
        let get = |id, p| tree.borrow().get_vec3(id, p).unwrap();
        let above = get(seat, Property::Position) + Vec3::new(0.0, 3.0, 0.0);
        let hop = format!(
            "workspace.Player1.HumanoidRootPart.Position = Vector3.new({}, {}, {})",
            above.x, above.y, above.z
        );
        session.runtime().run("hop", &hop).unwrap();
        frame(&mut session, 30, idle);
        frame(&mut session, 90, CharacterInput { throttle: 1.0, ..Default::default() });
        // A half turn at full speed takes under a second and a half.
        frame(&mut session, 85, CharacterInput { throttle: 1.0, steer: -1.0, ..Default::default() });
        let o = get(chassis, Property::Orientation);
        assert!(o.y.abs() > 150.0, "turned round: {o:?}");
        assert!(o.x.abs() < 1.0 && o.z.abs() < 1.0, "upright: {o:?}");
        // Pressing back stops it from full speed in well under a second.
        let before = get(chassis, Property::Position);
        frame(&mut session, 30, CharacterInput { throttle: -1.0, ..Default::default() });
        let mid = get(chassis, Property::Position);
        assert!((mid - before).length() < 8.0, "braked: {before:?} -> {mid:?}");
        // Then it backs up, at half its top speed.
        frame(&mut session, 60, CharacterInput { throttle: -1.0, ..Default::default() });
        let a = get(chassis, Property::Position);
        frame(&mut session, 30, CharacterInput { throttle: -1.0, ..Default::default() });
        let b = get(chassis, Property::Position);
        let speed = (b - a).length() * 2.0;
        assert!((10.0..=13.0).contains(&speed), "backing up at {speed}");
    }
}
