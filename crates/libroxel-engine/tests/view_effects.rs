//! The look classes: what bloom, colour correction, atmosphere, selection
//! boxes and beams ask the renderer for.

use libroxel_engine::view::atmosphere_fog;
use libroxel_engine::view_effects::{beams, bloom_look, grading, selection_boxes};
use libroxel_instance::{ClassKind, Color3, InstanceId, InstanceTree, Property, PropertyValue, Vec3};

fn add(tree: &mut InstanceTree, class: ClassKind, parent: InstanceId) -> InstanceId {
    let id = tree.create(class, "x").unwrap();
    tree.set_parent(id, Some(parent)).unwrap();
    id
}

#[test]
fn bloom_and_colour_correction_come_from_lighting() {
    let mut tree = InstanceTree::new();
    assert_eq!(bloom_look(&tree), None);
    assert_eq!(grading(&tree), Default::default());
    let lighting = tree.service(ClassKind::Lighting).unwrap();
    let bloom = add(&mut tree, ClassKind::BloomEffect, lighting);
    let look = bloom_look(&tree).expect("a bloom");
    assert!(look.intensity > 0.0 && look.threshold > 0.0);
    tree.set(bloom, Property::Enabled, PropertyValue::Bool(false)).unwrap();
    assert_eq!(bloom_look(&tree), None, "a turned-off bloom does nothing");

    let cc = add(&mut tree, ClassKind::ColorCorrectionEffect, lighting);
    assert_eq!(grading(&tree), Default::default(), "the defaults change nothing");
    tree.set(cc, Property::Saturation, PropertyValue::Number(-1.0)).unwrap();
    tree.set(cc, Property::Contrast, PropertyValue::Number(0.5)).unwrap();
    let g = grading(&tree);
    assert_eq!(g.global.post_saturation, 0.0);
    assert_eq!(g.midtones.contrast, 1.5);
    tree.set(cc, Property::TintColor, PropertyValue::Color3(Color3::new(1.0, 0.8, 0.6))).unwrap();
    assert!(grading(&tree).global.temperature > 0.0, "an orange tint warms the picture");
}

#[test]
fn atmosphere_thickens_with_density_and_haze() {
    let mut tree = InstanceTree::new();
    let lighting = tree.service(ClassKind::Lighting).unwrap();
    let air = add(&mut tree, ClassKind::Atmosphere, lighting);
    let (_, normal) = atmosphere_fog(&tree, air);
    tree.set(air, Property::Haze, PropertyValue::Number(5.0)).unwrap();
    let (colour, hazy) = atmosphere_fog(&tree, air);
    assert!(normal > 0.0 && hazy > normal);
    assert_eq!(colour, Color3::from_rgb8(199, 199, 199));
}

#[test]
fn selection_boxes_outline_parts_and_models_and_beams_join_attachments() {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let model = add(&mut tree, ClassKind::Model, ws);
    let a = add(&mut tree, ClassKind::Part, model);
    tree.set(a, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, 0.0, 0.0))).unwrap();
    tree.set(a, Property::Size, PropertyValue::Vec3(Vec3::new(2.0, 2.0, 2.0))).unwrap();
    let b = add(&mut tree, ClassKind::Part, model);
    tree.set(b, Property::Position, PropertyValue::Vec3(Vec3::new(10.0, 0.0, 0.0))).unwrap();
    tree.set(b, Property::Size, PropertyValue::Vec3(Vec3::new(2.0, 2.0, 2.0))).unwrap();
    let outline = add(&mut tree, ClassKind::SelectionBox, ws);
    tree.set(outline, Property::Adornee, PropertyValue::Ref(Some(model))).unwrap();
    tree.set(outline, Property::LineThickness, PropertyValue::Number(0.0)).unwrap();
    let boxes = selection_boxes(&tree);
    assert_eq!(boxes.len(), 1);
    let (_, center, size) = boxes[0];
    assert!((center.x - 5.0).abs() < 1e-4 && (size.x - 12.0).abs() < 1e-4, "{center} {size}");
    tree.set(outline, Property::Visible, PropertyValue::Bool(false)).unwrap();
    assert!(selection_boxes(&tree).is_empty());

    let start = add(&mut tree, ClassKind::Attachment, a);
    let end = add(&mut tree, ClassKind::Attachment, b);
    let beam = add(&mut tree, ClassKind::Beam, a);
    tree.set(beam, Property::Attachment0, PropertyValue::Ref(Some(start))).unwrap();
    tree.set(beam, Property::Attachment1, PropertyValue::Ref(Some(end))).unwrap();
    tree.set(beam, Property::Width1, PropertyValue::Number(3.0)).unwrap();
    let drawn = beams(&tree);
    assert_eq!(drawn.len(), 1);
    assert!((drawn[0].to.x - drawn[0].from.x - 10.0).abs() < 1e-4);
    assert_eq!(drawn[0].width, 2.0);
}
