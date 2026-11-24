//! Viewport frames: the little 3D pictures GUIs show (item icons and the
//! like). The parts inside a ViewportFrame are drawn as its camera sees
//! them, lit by its Ambient, LightColor and LightDirection, into a picture
//! that is made again only when something in it changes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use bevy_egui::egui;
use libroxel_engine::mesh_files::{self, Model};
use libroxel_engine::pictures;
use libroxel_instance::{
    CFrame, ClassKind, Color3, EnumValue, InstanceId, InstanceTree, PartShape, Property, PropertyValue,
    Rotation, Vec3,
};

/// The most pixels a side of the picture has (bigger frames are scaled up).
const MAX_SIDE: usize = 256;
/// Samples across each pixel, for smooth edges.
const SAMPLES: usize = 3;
/// Frames whose picture went unused this many draws let it go.
const FORGET_AFTER: u64 = 600;

struct Made {
    signature: u64,
    handle: egui::TextureHandle,
    used: u64,
}

struct Picture {
    size: [usize; 2],
    pixels: Vec<u8>,
}

#[derive(Default)]
struct Files {
    generation: u64,
    models: HashMap<String, Option<Rc<Model>>>,
    pictures: HashMap<String, Option<Rc<Picture>>>,
}

thread_local! {
    static MADE: RefCell<(u64, HashMap<InstanceId, Made>)> = RefCell::new((0, HashMap::new()));
    static FILES: RefCell<Files> = RefCell::new(Files::default());
}

fn files<T>(f: impl FnOnce(&mut Files) -> T) -> T {
    FILES.with_borrow_mut(|files| {
        let generation = pictures::generation();
        if files.generation != generation {
            *files = Files { generation, ..Files::default() };
        }
        f(files)
    })
}

fn model(name: &str) -> Option<Rc<Model>> {
    files(|f| f.models.entry(name.to_owned()).or_insert_with(|| mesh_files::load(name).map(Rc::new)).clone())
}

fn picture(name: &str) -> Option<Rc<Picture>> {
    files(|f| {
        f.pictures
            .entry(name.to_owned())
            .or_insert_with(|| {
                let (size, pixels) = pictures::decode_png(&pictures::bytes_for(name)?)?;
                Some(Rc::new(Picture { size, pixels }))
            })
            .clone()
    })
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
    model: Option<Rc<Model>>,
    picture: Option<Rc<Picture>>,
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
    (w, h, pictures::generation()).hash(&mut hasher);
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
        let mesh = tree.get_string(part, Property::MeshId).unwrap_or_default().trim().to_owned();
        let texture = tree.get_string(part, Property::MeshTexture).unwrap_or_default().trim().to_owned();
        for v in [pos, orientation, size] {
            [v.x, v.y, v.z].map(f32::to_bits).hash(&mut hasher);
        }
        [colour.r, colour.g, colour.b, alpha].map(f32::to_bits).hash(&mut hasher);
        (shape as u8, &mesh, &texture).hash(&mut hasher);
        let model = (!mesh.is_empty()).then(|| model(&mesh)).flatten();
        let picture = (model.is_some() && !texture.is_empty()).then(|| picture(&texture)).flatten();
        solids.push(Solid {
            cf: CFrame::new(pos, Rotation::from_orientation(orientation)),
            size,
            shape,
            colour,
            alpha,
            model,
            picture,
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

/// Triangles in a part's own space: corners, their normals and texture spots.
#[derive(Default)]
struct Shape {
    points: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<[f32; 2]>,
}

impl Shape {
    fn flat(&mut self, tri: [Vec3; 3], centre: Vec3) {
        let mut n = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
        let middle = (tri[0] + tri[1] + tri[2]) * (1.0 / 3.0);
        if n.dot(middle - centre) < 0.0 {
            n = n * -1.0;
        }
        for p in tri {
            self.points.push(p);
            self.normals.push(n);
            self.uvs.push([0.0, 0.0]);
        }
    }

    fn smooth(&mut self, tri: [(Vec3, Vec3); 3]) {
        for (p, n) in tri {
            self.points.push(p);
            self.normals.push(n);
            self.uvs.push([0.0, 0.0]);
        }
    }
}

fn v3(a: [f32; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn shape_of(solid: &Solid) -> Shape {
    let size = solid.size;
    let h = size * 0.5;
    let mut out = Shape::default();
    if let Some(model) = &solid.model {
        let safe = Vec3::new(size.x.max(1e-4), size.y.max(1e-4), size.z.max(1e-4));
        for ((p, n), uv) in model.positions.iter().zip(&model.normals).zip(&model.uvs) {
            out.points.push(v3(*p) * size);
            out.normals.push((v3(*n) / safe).normalize_or_zero());
            out.uvs.push(*uv);
        }
        return out;
    }
    match solid.shape {
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
            if solid.shape == PartShape::Wedge {
                v[2].y = -h.y;
                v[3].y = -h.y;
            }
            let quads = [[0, 1, 3, 2], [4, 6, 7, 5], [0, 4, 5, 1], [2, 3, 7, 6], [0, 2, 6, 4], [1, 5, 7, 3]];
            for q in quads {
                for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
                    let tri = t.map(|i| v[i]);
                    if (tri[1] - tri[0]).cross(tri[2] - tri[0]).length() > 1e-6 {
                        out.flat(tri, Vec3::ZERO);
                    }
                }
            }
        }
        PartShape::Cylinder => {
            let n = 64;
            let r = h.y.min(h.z);
            let ring = |i: usize| {
                let (s, c) = (i as f32 / n as f32 * std::f32::consts::TAU).sin_cos();
                Vec3::new(0.0, c, s)
            };
            for i in 0..n {
                let (a, b) = (ring(i), ring(i + 1));
                let at = |x: f32, d: Vec3| Vec3::new(x, d.y * r, d.z * r);
                out.smooth([(at(-h.x, a), a), (at(-h.x, b), b), (at(h.x, b), b)]);
                out.smooth([(at(-h.x, a), a), (at(h.x, b), b), (at(h.x, a), a)]);
                for x in [-h.x, h.x] {
                    out.flat([Vec3::new(x, 0.0, 0.0), at(x, a), at(x, b)], Vec3::ZERO);
                }
            }
        }
        PartShape::Ball => {
            let r = h.x.min(h.y).min(h.z);
            let (rings, segs) = (32, 64);
            let dir = |i: usize, j: usize| {
                let phi = i as f32 / rings as f32 * std::f32::consts::PI;
                let th = j as f32 / segs as f32 * std::f32::consts::TAU;
                Vec3::new(phi.sin() * th.cos(), phi.cos(), phi.sin() * th.sin())
            };
            for i in 0..rings {
                for j in 0..segs {
                    let [a, b, c, d] = [dir(i, j), dir(i, j + 1), dir(i + 1, j + 1), dir(i + 1, j)];
                    out.smooth([(a * r, a), (b * r, b), (c * r, c)]);
                    out.smooth([(a * r, a), (c * r, c), (d * r, d)]);
                }
            }
        }
    }
    out
}

fn sample(picture: &Picture, uv: [f32; 2]) -> [f32; 4] {
    let [w, h] = picture.size;
    let x = (uv[0] - uv[0].floor()) * w as f32 - 0.5;
    let y = (uv[1] - uv[1].floor()) * h as f32 - 0.5;
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |x: f32, y: f32| {
        let x = (x as isize).rem_euclid(w as isize) as usize;
        let y = (y as isize).rem_euclid(h as isize) as usize;
        let o = (y * w + x) * 4;
        [0, 1, 2, 3].map(|k| f32::from(picture.pixels[o + k]) / 255.0)
    };
    let (a, b, c, d) = (at(x0, y0), at(x0 + 1.0, y0), at(x0, y0 + 1.0), at(x0 + 1.0, y0 + 1.0));
    [0, 1, 2, 3].map(|k| {
        let top = a[k] + (b[k] - a[k]) * fx;
        let bottom = c[k] + (d[k] - c[k]) * fx;
        top + (bottom - top) * fy
    })
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
    let (sw, sh) = (w * SAMPLES, h * SAMPLES);
    let mut colour = vec![[0.0f32; 4]; sw * sh];
    let mut depth = vec![f32::INFINITY; sw * sh];
    let view = cam.inverse();
    let focal = (sh as f32 * 0.5) / (fov.to_radians() * 0.5).tan().max(0.01);
    let (cx, cy) = (sw as f32 * 0.5, sh as f32 * 0.5);
    let to_light = view.rot.apply(light_dir) * -1.0;
    let mut order: Vec<&Solid> = solids.iter().filter(|s| s.alpha >= 0.99).collect();
    let mut clear: Vec<(f32, &Solid)> =
        solids.iter().filter(|s| s.alpha < 0.99).map(|s| (view.point_to_world(s.cf.pos).z, s)).collect();
    clear.sort_by(|a, b| a.0.total_cmp(&b.0));
    order.extend(clear.into_iter().map(|(_, s)| s));
    for solid in order {
        let shape = shape_of(solid);
        let rot = view.rot * solid.cf.rot;
        let eye: Vec<Vec3> =
            shape.points.iter().map(|&p| view.point_to_world(solid.cf.point_to_world(p))).collect();
        let normals: Vec<Vec3> = shape.normals.iter().map(|&n| rot.apply(n)).collect();
        let opaque = solid.alpha >= 0.99;
        for t in 0..eye.len() / 3 {
            let i = [t * 3, t * 3 + 1, t * 3 + 2];
            let [a, b, c] = i.map(|i| eye[i]);
            if a.z > -0.05 || b.z > -0.05 || c.z > -0.05 {
                continue;
            }
            let project = |p: Vec3| (cx + p.x / -p.z * focal, cy - p.y / -p.z * focal, 1.0 / -p.z);
            let (p0, p1, p2) = (project(a), project(b), project(c));
            let area = (p1.0 - p0.0) * (p2.1 - p0.1) - (p1.1 - p0.1) * (p2.0 - p0.0);
            if area.abs() < 1e-9 {
                continue;
            }
            let x0 = p0.0.min(p1.0).min(p2.0).floor().max(0.0) as usize;
            let x1 = (p0.0.max(p1.0).max(p2.0).ceil() as isize).clamp(0, sw as isize) as usize;
            let y0 = p0.1.min(p1.1).min(p2.1).floor().max(0.0) as usize;
            let y1 = (p0.1.max(p1.1).max(p2.1).ceil() as isize).clamp(0, sh as isize) as usize;
            let [n0, n1, n2] = i.map(|i| normals[i]);
            let [t0, t1, t2] = i.map(|i| shape.uvs[i]);
            for y in y0..y1 {
                for x in x0..x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((p1.0 - px) * (p2.1 - py) - (p1.1 - py) * (p2.0 - px)) / area;
                    let w1 = ((p2.0 - px) * (p0.1 - py) - (p2.1 - py) * (p0.0 - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let inv = w0 * p0.2 + w1 * p1.2 + w2 * p2.2;
                    let z = 1.0 / inv;
                    let k = y * sw + x;
                    if z >= depth[k] {
                        continue;
                    }
                    let (b0, b1, b2) = (w0 * p0.2 * z, w1 * p1.2 * z, w2 * p2.2 * z);
                    let point = a * b0 + b * b1 + c * b2;
                    let mut normal = (n0 * b0 + n1 * b1 + n2 * b2).normalize_or_zero();
                    if normal.dot(point) > 0.0 {
                        normal = normal * -1.0;
                    }
                    let lit = to_light.dot(normal).max(0.0);
                    let base = match &solid.picture {
                        Some(p) => {
                            let uv = [0, 1].map(|j| t0[j] * b0 + t1[j] * b1 + t2[j] * b2);
                            sample(p, uv)
                        }
                        None => [solid.colour.r, solid.colour.g, solid.colour.b, 1.0],
                    };
                    let al = base[3] * solid.alpha;
                    if al <= 0.01 {
                        continue;
                    }
                    if opaque && al >= 0.99 {
                        depth[k] = z;
                    }
                    let lit = [
                        (base[0] * (ambient.r + light.r * lit)).clamp(0.0, 1.0),
                        (base[1] * (ambient.g + light.g * lit)).clamp(0.0, 1.0),
                        (base[2] * (ambient.b + light.b * lit)).clamp(0.0, 1.0),
                    ];
                    let old = colour[k];
                    colour[k] = [
                        lit[0] * al + old[0] * (1.0 - al),
                        lit[1] * al + old[1] * (1.0 - al),
                        lit[2] * al + old[2] * (1.0 - al),
                        al + old[3] * (1.0 - al),
                    ];
                }
            }
        }
    }
    let mut rgba = vec![0u8; w * h * 4];
    let share = 1.0 / (SAMPLES * SAMPLES) as f32;
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0.0f32; 4];
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let c = colour[(y * SAMPLES + sy) * sw + x * SAMPLES + sx];
                    for k in 0..4 {
                        sum[k] += c[k];
                    }
                }
            }
            let o = (y * w + x) * 4;
            for k in 0..4 {
                rgba[o + k] = (sum[k] * share * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    egui::ColorImage::from_rgba_premultiplied([w, h], &rgba)
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
            model: None,
            picture: None,
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

    #[test]
    fn a_model_is_drawn_in_its_own_shape_with_its_picture() {
        let pyramid = "v -1 -1 -1\nv 1 -1 -1\nv 1 -1 1\nv -1 -1 1\nv 0 1 0\n\
            vt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nvt 0.5 0.5\n\
            f 1/1 2/2 3/3 4/4\nf 1/1 2/2 5/5\nf 2/2 3/3 5/5\nf 3/3 4/4 5/5\nf 4/4 1/1 5/5\n";
        let model = Rc::new(mesh_files::parse_obj(pyramid).unwrap());
        let green = Rc::new(Picture { size: [2, 2], pixels: [0, 255, 0, 255].repeat(4) });
        let solids = [Solid {
            cf: CFrame::new(Vec3::new(0.0, 0.0, -10.0), Rotation::IDENTITY),
            size: Vec3::new(4.0, 4.0, 4.0),
            shape: PartShape::Block,
            colour: Color3::new(1.0, 0.0, 0.0),
            alpha: 1.0,
            model: Some(model),
            picture: Some(green),
        }];
        let image = render(
            &solids,
            CFrame::IDENTITY,
            30.0,
            64,
            64,
            Color3::new(0.5, 0.5, 0.5),
            Color3::new(0.5, 0.5, 0.5),
            Vec3::new(0.0, 0.0, -1.0),
        );
        let at = |x: usize, y: usize| image.pixels[y * 64 + x];
        let middle = at(32, 36);
        assert!(middle.a() == 255 && middle.g() > 100 && middle.r() < 20, "{middle:?}");
        assert_eq!(at(22, 20).a(), 0, "a block would fill the top corner");
        let edge = (0..64).map(|x| at(x, 32).a()).find(|a| *a > 0 && *a < 255);
        assert!(edge.is_some(), "edges are smoothed");
    }
}
