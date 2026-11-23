//! The look classes: what bloom, colour correction, atmosphere, selection
//! boxes and beams ask the renderer for.

use libroxel_engine::view::atmosphere_fog;
use libroxel_engine::view_effects::{base_grading, beams, bloom_look, grading, selection_boxes};
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
    assert_eq!(grading(&tree), base_grading());
    let lighting = tree.service(ClassKind::Lighting).unwrap();
    let bloom = add(&mut tree, ClassKind::BloomEffect, lighting);
    let look = bloom_look(&tree).expect("a bloom");
    assert!(look.intensity > 0.0 && look.threshold > 0.0);
    tree.set(bloom, Property::Enabled, PropertyValue::Bool(false)).unwrap();
    assert_eq!(bloom_look(&tree), None, "a turned-off bloom does nothing");

    let cc = add(&mut tree, ClassKind::ColorCorrectionEffect, lighting);
    assert_eq!(grading(&tree), base_grading(), "the defaults change nothing");
    tree.set(cc, Property::Saturation, PropertyValue::Number(-1.0)).unwrap();
    tree.set(cc, Property::Contrast, PropertyValue::Number(0.5)).unwrap();
    let g = grading(&tree);
    assert_eq!(g.global.post_saturation, 0.0);
    assert_eq!(g.midtones.contrast, base_grading().midtones.contrast * 1.5);
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

#[test]
fn a_sky_shows_the_sun_by_day_and_stars_at_night() {
    use libroxel_engine::view_effects::{sky_look, star_directions};
    let mut tree = InstanceTree::new();
    assert_eq!(sky_look(&tree), None, "no sky, nothing extra drawn");
    let lighting = tree.service(ClassKind::Lighting).unwrap();
    let sky = add(&mut tree, ClassKind::Sky, lighting);
    tree.set(lighting, Property::ClockTime, PropertyValue::Number(12.0)).unwrap();
    let noon = sky_look(&tree).unwrap();
    let (sun, size) = noon.sun.expect("the sun is up at noon");
    assert!(sun.y > 0.9 && size == 21.0);
    assert!(noon.moon.is_none(), "the moon is below the ground");
    assert_eq!(noon.stars, (3000, 0.0));
    tree.set(lighting, Property::ClockTime, PropertyValue::Number(0.0)).unwrap();
    let midnight = sky_look(&tree).unwrap();
    assert!(midnight.sun.is_none() && midnight.moon.unwrap().0.y > 0.9);
    assert_eq!(midnight.stars.1, 1.0);
    tree.set(sky, Property::CelestialBodiesShown, PropertyValue::Bool(false)).unwrap();
    tree.set(sky, Property::StarCount, PropertyValue::Number(10.0)).unwrap();
    let hidden = sky_look(&tree).unwrap();
    assert!(hidden.moon.is_none() && hidden.stars.0 == 10);
    let stars = star_directions(500);
    assert_eq!(stars.len(), 500);
    assert!(stars.iter().all(|s| (s.length() - 1.0).abs() < 1e-4 && s.y > -0.15));
}

#[test]
fn a_trail_remembers_where_it_was_and_forgets_it_in_time() {
    use bevy::math::Vec3 as V;
    use libroxel_engine::view_effects::TrailHistory;
    let mut trail = TrailHistory::default();
    for i in 0..10 {
        let x = i as f32;
        trail.record(i as f32 * 0.1, V::new(x, 0.0, 0.0), V::new(x, 1.0, 0.0), 0.5);
    }
    let strip = trail.strip(0.9, 0.5);
    // Only the last half second is left, newest last and least faded.
    assert_eq!(strip.len(), 6, "{strip:?}");
    assert_eq!(strip.last().unwrap().0, V::new(9.0, 0.0, 0.0));
    assert!(strip[0].2 > 0.9 && strip.last().unwrap().2 == 0.0);
    // Standing still adds nothing new, and the old part fades away.
    for i in 10..20 {
        trail.record(i as f32 * 0.1, V::new(9.0, 0.0, 0.0), V::new(9.0, 1.0, 0.0), 0.5);
    }
    assert_eq!(trail.strip(1.9, 0.5).len(), 1);
}
