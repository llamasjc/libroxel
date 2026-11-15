//! Viewport frames: the little 3D pictures GUIs show (item icons and the
//! like). The parts inside a ViewportFrame are drawn as its camera sees
//! them, lit by its Ambient, LightColor and LightDirection, into a picture
//! that is made again only when something in it changes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use bevy_egui::egui;
use libroxel_instance::{
    CFrame, ClassKind, Color3, EnumValue, InstanceId, InstanceTree, PartShape, Property, PropertyValue,
    Rotation, Vec3,
};

/// The most pixels a side of the picture has (bigger frames are scaled up).
const MAX_SIDE: usize = 192;
/// Frames whose picture went unused this many draws let it go.
const FORGET_AFTER: u64 = 600;

struct Made {
    signature: u64,
    handle: egui::TextureHandle,
    used: u64,
}

thread_local! {
    static MADE: RefCell<(u64, HashMap<InstanceId, Made>)> = RefCell::new((0, HashMap::new()));
}

/// Whether `id` is a ViewportFrame.
pub fn is_viewport(tree: &InstanceTree, id: InstanceId) -> bool {
    matches!(
        tree.get_extra(id, libroxel_script::names::STAND_IN),
        Ok(Some(PropertyValue::String(s))) if s == "ViewportFrame"
    )
}

/// One part to draw: where it is, its size, shape, colour and how see-through.
struct Solid {
    cf: CFrame,
    size: Vec3,
    shape: PartShape,
    colour: Color3,
    alpha: f32,
}

fn extra_color(tree: &InstanceTree, id: InstanceId, key: &str, fallback: Color3) -> Color3 {
    match tree.get_extra(id, key) {
        Ok(Some(PropertyValue::Color3(c))) => c,
        _ => fallback,
    }
}

fn extra_vec3(tree: &InstanceTree, id: InstanceId, key: &str, fallback: Vec3) -> Vec3 {
    match tree.get_extra(id, key) {
        Ok(Some(PropertyValue::Vec3(v))) => v,
        _ => fallback,
    }
}

/// Draws the viewport frame `id` into `rect`.
pub fn draw(painter: &egui::Painter, tree: &InstanceTree, id: InstanceId, rect: egui::Rect) {
    let ppp = painter.ctx().pixels_per_point();
    let w = ((rect.width() * ppp).round() as usize).clamp(1, MAX_SIDE);
    let h = ((rect.height() * ppp).round() as usize).clamp(1, MAX_SIDE);
    if rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }
    let Some(camera) = tree.find_first_child_of_class(id, ClassKind::Camera).ok().flatten() else { return };
    let cam_cf = CFrame::new(
        tree.get_vec3(camera, Property::Position).unwrap_or_default(),
        Rotation::from_orientation(tree.get_vec3(camera, Property::Orientation).unwrap_or_default()),
    );
    let fov = tree.get_number(camera, Property::FieldOfView).unwrap_or(70.0) as f32;
    let ambient = extra_color(tree, id, "Ambient", Color3::from_rgb8(200, 200, 200));
    let light = extra_color(tree, id, "LightColor", Color3::from_rgb8(140, 140, 140));
    let light_dir = extra_vec3(tree, id, "LightDirection", Vec3::new(-1.0, -1.0, -1.0)).normalize_or_zero();
    let tint = extra_color(tree, id, "ImageColor3", Color3::WHITE);
    let image_alpha = match tree.get_extra(id, "ImageTransparency") {
        Ok(Some(PropertyValue::Number(t))) => 1.0 - t as f32,
        _ => 1.0,
    };

    let mut solids = Vec::new();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (w, h).hash(&mut hasher);
    for v in [cam_cf.pos, tree.get_vec3(camera, Property::Orientation).unwrap_or_default(), light_dir] {
        [v.x, v.y, v.z].map(f32::to_bits).hash(&mut hasher);
    }
    for c in [ambient, light] {
        [c.r, c.g, c.b].map(f32::to_bits).hash(&mut hasher);
    }
    fov.to_bits().hash(&mut hasher);
    for part in tree.descendants(id).unwrap_or_default() {
        if !tree.class(part).is_ok_and(|c| c.is_physical()) {
            continue;
        }
        let alpha = 1.0 - tree.get_number(part, Property::Transparency).unwrap_or(0.0) as f32;
        if alpha <= 0.01 {
            continue;
        }
        let pos = tree.get_vec3(part, Property::Position).unwrap_or_default();
        let orientation = tree.get_vec3(part, Property::Orientation).unwrap_or_default();
        let size = tree.get_vec3(part, Property::Size).unwrap_or(Vec3::new(1.0, 1.0, 1.0));
        let colour = tree.get_color(part, Property::Color).unwrap_or(Color3::from_rgb8(160, 160, 160));
        let shape = match tree.get_enum(part, Property::Shape) {
            Some(EnumValue::Shape(s)) => s,
            _ => PartShape::Block,
        };
        for v in [pos, orientation, size] {
            [v.x, v.y, v.z].map(f32::to_bits).hash(&mut hasher);
        }
        [colour.r, colour.g, colour.b, alpha].map(f32::to_bits).hash(&mut hasher);
        (shape as u8).hash(&mut hasher);
        solids.push(Solid {
            cf: CFrame::new(pos, Rotation::from_orientation(orientation)),
            size,
            shape,
            colour,
            alpha,
        });
    }
    let signature = hasher.finish();

    let texture = MADE.with_borrow_mut(|(clock, made)| {
        *clock += 1;
        let now = *clock;
        // Pictures of frames that went away are let go now and then.
        if now.is_multiple_of(120) {
            made.retain(|_, m| now - m.used < FORGET_AFTER);
        }
        if let Some(m) = made.get_mut(&id) {
            m.used = now;
            if m.signature != signature {
                m.handle.set(
                    render(&solids, cam_cf, fov, w, h, ambient, light, light_dir),
                    egui::TextureOptions::LINEAR,
                );
                m.signature = signature;
            }
            return m.handle.id();
        }
        let image = render(&solids, cam_cf, fov, w, h, ambient, light, light_dir);
        let handle =
            painter.ctx().load_texture(format!("viewport:{id:?}"), image, egui::TextureOptions::LINEAR);
        let tex = handle.id();
        made.insert(id, Made { signature, handle, used: now });
        tex
    });
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
    let colour = egui::Color32::from_rgba_unmultiplied(to(tint.r), to(tint.g), to(tint.b), to(image_alpha));
    painter.with_clip_rect(rect.intersect(painter.clip_rect())).image(texture, rect, uv, colour);
}

/// The corners and faces of a part's shape, in its own space (±½ size).
fn mesh(shape: PartShape, size: Vec3) -> (Vec<Vec3>, Vec<[usize; 3]>) {
    let h = size * 0.5;
    match shape {
        PartShape::Block | PartShape::Wedge => {
            let mut v: Vec<Vec3> = (0..8)
                .map(|i| {
                    Vec3::new(
                        if i & 1 == 0 { -h.x } else { h.x },
                        if i & 2 == 0 { -h.y } else { h.y },
                        if i & 4 == 0 { -h.z } else { h.z },
                    )
                })
                .collect();
            if shape == PartShape::Wedge {
                // The top front edge comes down to the bottom: a ramp up to the back.
                v[2].y = -h.y;
                v[3].y = -h.y;
            }
            let quads = [[0, 1, 3, 2], [4, 6, 7, 5], [0, 4, 5, 1], [2, 3, 7, 6], [0, 2, 6, 4], [1, 5, 7, 3]];
            let tris = quads.iter().flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]).collect();
            (v, tris)
        }
        PartShape::Cylinder => {
            // Along X, round across Y and Z.
            let n = 16;
            let r = h.y.min(h.z);
            let mut v = Vec::new();
            for i in 0..n {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                let (s, c) = a.sin_cos();
                v.push(Vec3::new(-h.x, c * r, s * r));
                v.push(Vec3::new(h.x, c * r, s * r));
            }
            let (left, right) = (v.len(), v.len() + 1);
            v.push(Vec3::new(-h.x, 0.0, 0.0));
            v.push(Vec3::new(h.x, 0.0, 0.0));
            let mut t = Vec::new();
            for i in 0..n {
                let j = (i + 1) % n;
                let (a0, a1, b0, b1) = (2 * i, 2 * i + 1, 2 * j, 2 * j + 1);
                t.push([a0, b0, b1]);
                t.push([a0, b1, a1]);
                t.push([left, b0, a0]);
                t.push([right, a1, b1]);
            }
            (v, t)
        }
        PartShape::Ball => {
            let r = h.x.min(h.y).min(h.z);
            let (rings, segs) = (8, 12);
            let mut v = Vec::new();
            for i in 0..=rings {
                let phi = i as f32 / rings as f32 * std::f32::consts::PI;
                for j in 0..segs {
                    let th = j as f32 / segs as f32 * std::f32::consts::TAU;
                    v.push(Vec3::new(phi.sin() * th.cos() * r, phi.cos() * r, phi.sin() * th.sin() * r));
                }
            }
            let mut t = Vec::new();
            for i in 0..rings {
                for j in 0..segs {
                    let a = i * segs + j;
                    let b = i * segs + (j + 1) % segs;
                    t.push([a, b, b + segs]);
                    t.push([a, b + segs, a + segs]);
                }
            }
            (v, t)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render(
    solids: &[Solid],
    cam: CFrame,
    fov: f32,
    w: usize,
    h: usize,
    ambient: Color3,
    light: Color3,
    light_dir: Vec3,
) -> egui::ColorImage {
    let mut rgba = vec![0u8; w * h * 4];
    let mut depth = vec![f32::INFINITY; w * h];
    let view = cam.inverse();
    let focal = (h as f32 * 0.5) / (fov.to_radians() * 0.5).tan().max(0.01);
    let (cx, cy) = (w as f32 * 0.5, h as f32 * 0.5);
    for solid in solids {
        let (corners, tris) = mesh(solid.shape, solid.size);
        let world: Vec<Vec3> = corners.iter().map(|&c| solid.cf.point_to_world(c)).collect();
        let eye: Vec<Vec3> = world.iter().map(|&p| view.point_to_world(p)).collect();
        for t in &tris {
            let [a, b, c] = t.map(|i| eye[i]);
            // In front of the camera only (it looks along -Z).
            if a.z > -0.05 || b.z > -0.05 || c.z > -0.05 {
                continue;
            }
            let mut normal = (world[t[1]] - world[t[0]]).cross(world[t[2]] - world[t[0]]).normalize_or_zero();
            // Facing out of the part, whichever way round the corners go.
            let middle = (world[t[0]] + world[t[1]] + world[t[2]]) * (1.0 / 3.0);
            if normal.dot(middle - solid.cf.pos) < 0.0 {
                normal = normal * -1.0;
            }
            let lit = (-light_dir).dot(normal).max(0.0);
            let shade = |base: f32, amb: f32, l: f32| (base * (amb + l * lit)).clamp(0.0, 1.0);
            let colour = [
                shade(solid.colour.r, ambient.r, light.r),
                shade(solid.colour.g, ambient.g, light.g),
                shade(solid.colour.b, ambient.b, light.b),
            ];
            let project = |p: Vec3| (cx + p.x / -p.z * focal, cy - p.y / -p.z * focal, -p.z);
            let (p0, p1, p2) = (project(a), project(b), project(c));
            let area = (p1.0 - p0.0) * (p2.1 - p0.1) - (p1.1 - p0.1) * (p2.0 - p0.0);
            if area.abs() < 1e-6 {
                continue;
            }
            let x0 = p0.0.min(p1.0).min(p2.0).floor().max(0.0) as usize;
            let x1 = (p0.0.max(p1.0).max(p2.0).ceil() as isize).clamp(0, w as isize) as usize;
            let y0 = p0.1.min(p1.1).min(p2.1).floor().max(0.0) as usize;
            let y1 = (p0.1.max(p1.1).max(p2.1).ceil() as isize).clamp(0, h as isize) as usize;
            for y in y0..y1 {
                for x in x0..x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((p1.0 - px) * (p2.1 - py) - (p1.1 - py) * (p2.0 - px)) / area;
                    let w1 = ((p2.0 - px) * (p0.1 - py) - (p2.1 - py) * (p0.0 - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let z = w0 * p0.2 + w1 * p1.2 + w2 * p2.2;
                    let i = y * w + x;
                    if z >= depth[i] {
                        continue;
                    }
                    depth[i] = z;
                    let o = i * 4;
                    let old = [rgba[o], rgba[o + 1], rgba[o + 2], rgba[o + 3]].map(|v| f32::from(v) / 255.0);
                    let al = solid.alpha;
                    for k in 0..3 {
                        rgba[o + k] = ((colour[k] * al + old[k] * (1.0 - al)) * 255.0) as u8;
                    }
                    rgba[o + 3] = ((al + old[3] * (1.0 - al)).min(1.0) * 255.0) as u8;
                }
            }
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_in_front_of_the_camera_shows_lit_in_the_middle() {
        let solids = [Solid {
            cf: CFrame::new(Vec3::new(0.0, 0.0, -10.0), Rotation::IDENTITY),
            size: Vec3::new(2.0, 2.0, 2.0),
            shape: PartShape::Block,
            colour: Color3::new(1.0, 0.0, 0.0),
            alpha: 1.0,
        }];
        let light = Color3::new(0.5, 0.5, 0.5);
        let image = render(
            &solids,
            CFrame::IDENTITY,
            30.0,
            32,
            32,
            Color3::new(0.3, 0.3, 0.3),
            light,
            Vec3::new(0.0, 0.0, -1.0),
        );
        let middle = image.pixels[16 * 32 + 16];
        assert!(middle.a() == 255 && middle.r() > 150 && middle.g() == 0, "{middle:?}");
        assert_eq!(image.pixels[0].a(), 0, "the corner is empty");
    }
}
