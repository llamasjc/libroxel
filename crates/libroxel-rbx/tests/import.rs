//! Opening files saved by Roblox Studio: real sample files (from the
//! rbx-test-files collection, MIT licensed, in `tests/files`), and a small
//! game built here and saved in both formats.

use std::path::{Path, PathBuf};

use libroxel_engine::session::PlaySession;
use libroxel_engine::{CharacterInput, LogLevel};
use libroxel_instance::{ClassKind, EnumValue, InstanceTree, Material, PartShape, Property, PropertyValue};
use rbx_dom_weak::types::{
    Attributes, CFrame, Color3uint8, Enum, Matrix3, Tags, UDim, UDim2, Variant, Vector3,
};
use rbx_dom_weak::{InstanceBuilder, WeakDom};

fn file(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/files").join(name)
}

fn find(tree: &InstanceTree, name: &str) -> libroxel_instance::InstanceId {
    tree.descendants(tree.root())
        .unwrap()
        .into_iter()
        .find(|id| tree.name(*id) == Ok(name))
        .unwrap_or_else(|| panic!("no {name}"))
}

#[test]
fn every_sample_file_opens() {
    for name in [
        "baseplate-566-binary.rbxl",
        "baseplate-566-xml.rbxlx",
        "all-instances-415-binary.rbxl",
        "three-unique-parts-binary.rbxm",
        "unions-xml.rbxmx",
        "three-screengui-binary.rbxm",
        "weldconstraint-xml.rbxmx",
        "attributes-binary.rbxm",
        "tags-xml.rbxmx",
    ] {
        let (tree, report) = libroxel_rbx::open(&file(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(report.instances > 0, "{name}: {report:?}");
        assert!(tree.descendants(tree.root()).unwrap().len() > report.instances / 2);
    }
}

#[test]
fn a_baseplate_place_brings_its_baseplate_and_spawn() {
    for name in ["baseplate-566-binary.rbxl", "baseplate-566-xml.rbxlx"] {
        let (tree, _) = libroxel_rbx::open(&file(name)).unwrap();
        let plate = find(&tree, "Baseplate");
        assert_eq!(tree.parent(plate).unwrap(), Some(tree.workspace()));
        let size = tree.get_vec3(plate, Property::Size).unwrap();
        assert!(size.x >= 512.0 && size.y > 1.0, "{name}: {size:?}");
        assert_eq!(tree.get_bool(plate, Property::Anchored), Some(true));
        let spawn = tree.descendants_of_class(tree.workspace(), ClassKind::SpawnLocation);
        assert_eq!(spawn.len(), 1, "{name}");
    }
}

#[test]
fn the_all_instances_place_keeps_scripts_parts_and_gui() {
    let (tree, report) = libroxel_rbx::open(&file("all-instances-415-binary.rbxl")).unwrap();
    assert!(report.instances > 150, "{report:?}");
    assert_eq!(report.scripts, 3, "{report:?}");
    // Wedges keep their shape.
    let wedge = find(&tree, "Wedge");
    assert_eq!(
        tree.get(wedge, Property::Shape).unwrap(),
        PropertyValue::Enum(EnumValue::Shape(PartShape::Wedge))
    );
    for class in
        [ClassKind::ScreenGui, ClassKind::Frame, ClassKind::TextLabel, ClassKind::Humanoid, ClassKind::Tool]
    {
        assert!(!tree.descendants_of_class(tree.root(), class).is_empty(), "{class:?}");
    }
}

#[test]
fn links_attributes_and_tags_come_across() {
    let (tree, _) = libroxel_rbx::open(&file("weldconstraint-xml.rbxmx")).unwrap();
    let weld = tree.descendants_of_class(tree.root(), ClassKind::WeldConstraint)[0];
    let (a, b) = (find(&tree, "A"), find(&tree, "B"));
    assert_eq!(tree.get_ref(weld, Property::Part0), Some(a));
    assert_eq!(tree.get_ref(weld, Property::Part1), Some(b));

    let (tree, _) = libroxel_rbx::open(&file("tags-xml.rbxmx")).unwrap();
    let folder = tree.descendants_of_class(tree.workspace(), ClassKind::Folder)[0];
    for tag in ["My", "Cool", "Tags"] {
        assert!(tree.has_tag(folder, tag).unwrap(), "{tag}");
    }

    let (tree, _) = libroxel_rbx::open(&file("attributes-binary.rbxm")).unwrap();
    let folder = tree.descendants_of_class(tree.workspace(), ClassKind::Folder)[0];
    assert_eq!(
        tree.get_attribute(folder, "String").unwrap(),
        Some(PropertyValue::String("Hello, world!".into()))
    );
    assert_eq!(tree.get_attribute(folder, "Number").unwrap(), Some(PropertyValue::Number(12345.0)));
    assert_eq!(tree.get_attribute(folder, "Boolean").unwrap(), Some(PropertyValue::Bool(true)));
}

/// A rotation of `degrees` about Y, as the file stores it (by rows).
fn turned_y(degrees: f32) -> Matrix3 {
    let (s, c) = degrees.to_radians().sin_cos();
    Matrix3::new(Vector3::new(c, 0.0, s), Vector3::new(0.0, 1.0, 0.0), Vector3::new(-s, 0.0, c))
}

/// A small game, as Studio would save it.
fn game() -> WeakDom {
    let part = |name: &str, x: f32| {
        InstanceBuilder::new("Part")
            .with_name(name)
            .with_property("CFrame", CFrame::new(Vector3::new(x, 4.0, 0.0), turned_y(90.0)))
            .with_property("Size", Vector3::new(4.0, 1.0, 2.0))
            .with_property("Anchored", true)
            .with_property("Color", Color3uint8::new(255, 0, 0))
            .with_property("Material", Enum::from_u32(288)) // Neon
            .with_property("Transparency", 0.25f32)
    };
    let model = InstanceBuilder::new("Model").with_name("Car").with_child(part("Body", 10.0));
    let body_ref = model.referent();
    let _ = body_ref;
    let mut attributes = Attributes::new();
    attributes.insert("Speed".into(), Variant::Float64(42.0));
    let mut tags = Tags::new();
    tags.push("Kart");
    let model = model.with_property("Attributes", attributes).with_property("Tags", tags);
    let server = InstanceBuilder::new("Script").with_name("Main").with_property(
        "Source",
        "local car = workspace:WaitForChild('Car')\n\
         print('car speed', car:GetAttribute('Speed'))\n\
         print('body at', car.Body.Position.X, car.Body.Orientation.Y)\n\
         print('neon', car.Body.Material == Enum.Material.Neon)",
    );
    let client = InstanceBuilder::new("LocalScript")
        .with_name("Hello")
        .with_property("Source", "print('hello from ' .. script.Parent.ClassName)");
    let gui = InstanceBuilder::new("ScreenGui").with_name("HUD").with_child(
        InstanceBuilder::new("TextLabel")
            .with_name("Coins")
            .with_property("Text", "Coins: 0")
            .with_property("Size", UDim2::new(UDim::new(0.0, 200), UDim::new(0.0, 50)))
            .with_property("Position", UDim2::new(UDim::new(0.5, -100), UDim::new(0.0, 10))),
    );
    WeakDom::new(
        InstanceBuilder::new("DataModel")
            .with_child(InstanceBuilder::new("Workspace").with_child(model).with_child(part("Plate", 0.0)))
            .with_child(InstanceBuilder::new("Lighting").with_property("ClockTime", 20.0f32))
            .with_child(InstanceBuilder::new("ServerScriptService").with_child(server))
            .with_child(InstanceBuilder::new("StarterGui").with_child(gui))
            .with_child(
                InstanceBuilder::new("StarterPlayer")
                    .with_child(InstanceBuilder::new("StarterPlayerScripts").with_child(client)),
            )
            .with_child(InstanceBuilder::new("SoundService"))
            .with_child(InstanceBuilder::new("Chat").with_child(InstanceBuilder::new("Folder"))),
    )
}

fn saved(binary: bool) -> Vec<u8> {
    let dom = game();
    let mut out = Vec::new();
    let roots = dom.root().children().to_vec();
    if binary {
        rbx_binary::to_writer(&mut out, &dom, &roots).unwrap();
    } else {
        rbx_xml::to_writer_default(&mut out, &dom, &roots).unwrap();
    }
    out
}

#[test]
fn a_game_saved_both_ways_comes_across_and_plays() {
    for binary in [true, false] {
        let dom = libroxel_rbx::read_dom(&saved(binary)).unwrap();
        let mut tree = InstanceTree::new();
        let report = libroxel_rbx::import_place(&mut tree, &dom);
        assert_eq!(report.skipped_services, ["Chat"], "{report:?}");

        let body = find(&tree, "Body");
        assert_eq!(tree.get_vec3(body, Property::Position).unwrap().x, 10.0);
        assert!((tree.get_vec3(body, Property::Orientation).unwrap().y - 90.0).abs() < 0.01);
        assert_eq!(
            tree.get(body, Property::Material).unwrap(),
            PropertyValue::Enum(EnumValue::Material(Material::Neon))
        );
        let red = tree.get_color(body, Property::Color).unwrap();
        assert!(red.r > 0.99 && red.g < 0.01);
        assert_eq!(tree.get_number(body, Property::Transparency), Some(0.25));
        let lighting = tree.service(ClassKind::Lighting).unwrap();
        assert_eq!(tree.get_number(lighting, Property::ClockTime), Some(20.0));
        let label = find(&tree, "Coins");
        assert_eq!(
            tree.get(label, Property::GuiPosition).unwrap(),
            PropertyValue::UDim2(libroxel_instance::UDim2::new(0.5, -100.0, 0.0, 10.0))
        );
        let car = find(&tree, "Car");
        assert!(tree.has_tag(car, "Kart").unwrap());
        // The local script landed in the starter player's own folder.
        let folder = tree.starter_scripts(ClassKind::StarterPlayerScripts).unwrap();
        assert_eq!(tree.children(folder).unwrap().len(), 1);

        let shared = std::rc::Rc::new(std::cell::RefCell::new(tree));
        let mut session = PlaySession::start(shared, true).unwrap();
        for _ in 0..10 {
            session.step(1.0 / 60.0, CharacterInput::default());
        }
        let log = session.take_log();
        assert!(log.iter().all(|l| l.level != LogLevel::Error), "{log:?}");
        let text: Vec<String> = log.into_iter().map(|l| l.text).collect();
        for line in ["car speed 42", "body at 10 90", "neon true", "hello from PlayerScripts"] {
            assert!(text.iter().any(|t| t == line), "missing {line:?} in {text:?}");
        }
    }
}

#[test]
fn unions_keep_their_carved_shape() {
    let (tree, report) = libroxel_rbx::open(&file("unions-xml.rbxmx")).unwrap();
    assert!(!report.files.is_empty(), "{report:?}");
    let parts = tree.descendants_of_class(tree.workspace(), ClassKind::Part);
    let with_shapes: Vec<_> = parts
        .iter()
        .filter_map(|p| tree.get_string(*p, Property::MeshId).filter(|m| m.starts_with("union-")))
        .collect();
    assert!(!with_shapes.is_empty());
    for name in with_shapes {
        let (_, text) = report.files.iter().find(|(n, _)| *n == name).expect("its model is made");
        let model = libroxel_engine::mesh_files::parse(text).expect("the model reads back");
        assert!(model.positions.len() >= 12 * 3, "more than a box's worth of triangles");
    }
}

/// A terrain blob: one chunk with a full grass voxel, a half-full rock
/// voxel on top of it, then air.
fn terrain_blob() -> Vec<u8> {
    let mut blob = vec![0x01, 0x05];
    blob.extend([0u8; 12]);
    // Voxel (0,0,0) grass; air up to (0,1,0); half rock; then air to the end.
    blob.extend([0x02]);
    let air = |mut n: u32, blob: &mut Vec<u8>| {
        while n > 0 {
            let run = n.min(256);
            blob.extend([0x80, (run - 1) as u8]);
            n -= run;
        }
    };
    air(32 * 32 - 1, &mut blob);
    blob.extend([0x48, 0x80]);
    air(32 * 32 * 32 - 32 * 32 - 1, &mut blob);
    blob
}

#[test]
fn terrain_meshes_and_old_style_meshes_come_across() {
    let dom = WeakDom::new(
        InstanceBuilder::new("DataModel").with_child(
            InstanceBuilder::new("Workspace")
                .with_child(
                    InstanceBuilder::new("Terrain")
                        .with_property("SmoothGrid", Variant::BinaryString(terrain_blob().into())),
                )
                .with_child(InstanceBuilder::new("Part").with_name("Ball").with_child(
                    InstanceBuilder::new("SpecialMesh").with_property("MeshType", Enum::from_u32(3)),
                ))
                .with_child(
                    InstanceBuilder::new("Part").with_name("Rock").with_child(
                        InstanceBuilder::new("SpecialMesh")
                            .with_property("MeshType", Enum::from_u32(5))
                            .with_property("MeshId", rbx_dom_weak::types::ContentId::from("rbxassetid://42")),
                    ),
                )
                .with_child(
                    InstanceBuilder::new("MeshPart")
                        .with_name("Statue")
                        .with_property("MeshId", rbx_dom_weak::types::ContentId::from("rbxassetid://77")),
                )
                .with_child(InstanceBuilder::new("CornerWedgePart").with_name("Corner"))
                .with_child(InstanceBuilder::new("ViewportFrame").with_name("View")),
        ),
    );
    let mut out = Vec::new();
    let roots = dom.root().children().to_vec();
    rbx_binary::to_writer(&mut out, &dom, &roots).unwrap();
    let dom = libroxel_rbx::read_dom(&out).unwrap();
    let mut tree = InstanceTree::new();
    let report = libroxel_rbx::import_place(&mut tree, &dom);

    // Terrain: a full 4x4x4 block of grass with two layers of rock above.
    let terrain = tree.terrain().expect("terrain");
    let voxels = tree.voxels(terrain).unwrap();
    assert_eq!(voxels.get(0, 0, 0), Some(Material::Grass));
    assert_eq!(voxels.get(3, 3, 3), Some(Material::Grass));
    assert_eq!(voxels.get(0, 4, 0), Some(Material::Rock));
    assert_eq!(voxels.get(3, 5, 3), Some(Material::Rock));
    assert_eq!(voxels.get(0, 6, 0), None);
    assert_eq!(report.terrain_cells, 64 + 32);

    let shape = |name| tree.get(find(&tree, name), Property::Shape).unwrap();
    assert_eq!(shape("Ball"), PropertyValue::Enum(EnumValue::Shape(PartShape::Ball)));
    let mesh = |name| tree.get_string(find(&tree, name), Property::MeshId).unwrap();
    assert_eq!(mesh("Rock"), "rbxassetid://42");
    assert_eq!(mesh("Statue"), "rbxassetid://77");
    assert_eq!(mesh("Corner"), "cornerwedge.obj");
    assert!(report.files.iter().any(|(n, _)| n == "cornerwedge.obj"));
    assert_eq!(tree.class(find(&tree, "View")), Ok(ClassKind::Frame));
}

#[test]
fn expanded_terrain_is_named_as_left_out() {
    let (_, report) = libroxel_rbx::open(&file("expanded-terrain-binary.rbxm")).unwrap();
    assert_eq!(report.terrain_cells, 0);
    assert!(report.summary().iter().any(|l| l.contains("expanded terrain")), "{:?}", report.summary());
}
