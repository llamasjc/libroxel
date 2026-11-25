use playlibre_instance::{Color3, PartShape, Vec3};

use crate::{Look, Piece, pieces};

/// Which picture of an avatar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shot {
    Head,
    Body,
}

impl Shot {
    pub fn named(name: &str) -> Option<Shot> {
        match name {
            "head" => Some(Shot::Head),
            "body" => Some(Shot::Body),
            _ => None,
        }
    }
}

/// An RGBA picture, row by row, not premultiplied.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

const SAMPLES: usize = 3;

struct Tri {
    p: [Vec3; 3],
    n: [Vec3; 3],
    color: Color3,
    neon: bool,
    piece: u16,
}

/// A picture of `look` standing in daylight, on a clear background.
pub fn render(look: &Look, shot: Shot, width: u32, height: u32) -> Picture {
    let parts = pieces(look);
    let (yaw, rise): (f32, f32) = match shot {
        Shot::Head => (0.38, 0.12),
        Shot::Body => (0.42, 0.16),
    };
    let back = Vec3::new(yaw.sin() * rise.cos(), rise.sin(), -yaw.cos() * rise.cos());
    let right = Vec3::new(0.0, 1.0, 0.0).cross(back).normalize_or_zero();
    let up = back.cross(right);
    let distance = 40.0;
    let mut tris = Vec::new();
    for (i, piece) in parts.iter().enumerate() {
        mesh(piece, i as u16, &mut tris);
    }
    let framed: Vec<Vec3> = parts
        .iter()
        .filter(|p| shot == Shot::Body || (p.on_head && p.pos == crate::HEAD_AT))
        .flat_map(corners)
        .collect();
    let target = match shot {
        Shot::Head => crate::HEAD_AT,
        Shot::Body => Vec3::new(0.0, 0.6, 0.0),
    };
    let eye = target + back * distance;
    let view = |p: Vec3| {
        let d = p - eye;
        let z = -d.dot(back);
        (d.dot(right) / z * distance, d.dot(up) / z * distance, z)
    };
    let bounds = |points: &[Vec3]| {
        let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in points {
            let (x, y, _) = view(*p);
            (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
        }
        (x0, x1, y0, y1)
    };
    let (x0, x1, y0, mut y1) = bounds(&framed);
    if shot == Shot::Head {
        let worn: Vec<Vec3> = parts.iter().filter(|p| p.on_head).flat_map(corners).collect();
        let top = bounds(&worn).3;
        y1 = top.min(y1 + (y1 - y0) * 0.6).max(y1);
    }
    let aspect = width as f32 / height as f32;
    let (cx, cy, span) = match shot {
        Shot::Head => {
            let span = (x1 - x0).max(y1 - y0) * 1.45;
            (0.5 * (x0 + x1), 0.5 * (y0 + y1) - span * 0.1, span)
        }
        Shot::Body => {
            let span = ((x1 - x0) / aspect).max(y1 - y0) * 1.08;
            (0.5 * (x0 + x1), 0.5 * (y0 + y1), span)
        }
    };
    let (w, h) = (width as usize * SAMPLES, height as usize * SAMPLES);
    let scale = h as f32 / span;
    let to_screen = |p: Vec3| {
        let (x, y, z) = view(p);
        ((x - cx) * scale + w as f32 * 0.5, (cy - y) * scale + h as f32 * 0.5, z)
    };
    let mut depth = vec![f32::MAX; w * h];
    let mut colour = vec![[0f32; 3]; w * h];
    let mut owner = vec![u16::MAX; w * h];
    let light = Vec3::new(0.3, 0.7, -0.65).normalize_or_zero();
    let towards_eye = back;
    for tri in &tris {
        let s = tri.p.map(to_screen);
        let area = (s[1].0 - s[0].0) * (s[2].1 - s[0].1) - (s[2].0 - s[0].0) * (s[1].1 - s[0].1);
        if area.abs() < 1e-6 {
            continue;
        }
        let face = (tri.p[1] - tri.p[0]).cross(tri.p[2] - tri.p[0]);
        if face.dot(eye - tri.p[0]) <= 0.0 {
            continue;
        }
        let min_x = s.iter().map(|v| v.0).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
        let max_x = (s.iter().map(|v| v.0).fold(f32::MIN, f32::max).ceil() as usize).min(w);
        let min_y = s.iter().map(|v| v.1).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
        let max_y = (s.iter().map(|v| v.1).fold(f32::MIN, f32::max).ceil() as usize).min(h);
        for py in min_y..max_y {
            for px in min_x..max_x {
                let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
                let e = |a: (f32, f32, f32), b: (f32, f32, f32)| {
                    (b.0 - a.0) * (y - a.1) - (x - a.0) * (b.1 - a.1)
                };
                let (w0, w1, w2) = (e(s[1], s[2]) / area, e(s[2], s[0]) / area, e(s[0], s[1]) / area);
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * s[0].2 + w1 * s[1].2 + w2 * s[2].2;
                let i = py * w + px;
                if z >= depth[i] {
                    continue;
                }
                depth[i] = z;
                owner[i] = tri.piece;
                let n = (tri.n[0] * w0 + tri.n[1] * w1 + tri.n[2] * w2).normalize_or_zero();
                colour[i] = shade(tri, n, light, towards_eye);
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if owner[i] == u16::MAX {
                continue;
            }
            let edge = [(1, 0), (0, 1), (-1, 0), (0, -1)].iter().any(|(dx, dy)| {
                let (nx, ny) = (x as i64 + dx * 2, y as i64 + dy * 2);
                if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                    return false;
                }
                let j = ny as usize * w + nx as usize;
                owner[j] != owner[i] && depth[j] > depth[i]
            });
            if edge {
                colour[i] = colour[i].map(|c| c * 0.72);
            }
        }
    }
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let (mut sum, mut covered) = ([0f32; 3], 0usize);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let i = (y * SAMPLES + sy) * w + x * SAMPLES + sx;
                    if owner[i] != u16::MAX {
                        covered += 1;
                        for k in 0..3 {
                            sum[k] += colour[i][k];
                        }
                    }
                }
            }
            if covered == 0 {
                rgba.extend([0, 0, 0, 0]);
                continue;
            }
            let c = sum.map(|v| to_srgb(v / covered as f32));
            rgba.extend([c[0], c[1], c[2], (covered * 255 / (SAMPLES * SAMPLES)) as u8]);
        }
    }
    Picture { width, height, rgba }
}

fn shade(tri: &Tri, n: Vec3, light: Vec3, eye: Vec3) -> [f32; 3] {
    let base = [tri.color.r, tri.color.g, tri.color.b].map(to_linear);
    if tri.neon {
        return base.map(|c| (c * 1.25 + 0.08).min(1.0));
    }
    let sky = 0.5 + 0.5 * n.y;
    let ambient = [0.30 + 0.18 * sky, 0.30 + 0.22 * sky, 0.32 + 0.30 * sky];
    let sun = n.dot(light).max(0.0);
    let sun_colour = [1.0, 0.95, 0.86];
    let half = (light + eye).normalize_or_zero();
    let gloss = n.dot(half).max(0.0).powi(28) * 0.18;
    let rim = (1.0 - n.dot(eye).max(0.0)).powi(3) * 0.12;
    [0, 1, 2].map(|k| base[k] * (ambient[k] + sun_colour[k] * sun * 0.85) + gloss + rim * base[k])
}

fn to_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

fn corners(p: &Piece) -> Vec<Vec3> {
    let s = scaled(p);
    (0..8)
        .map(|i| {
            let pick = |bit: u32, v: f32| if i & (1 << bit) == 0 { -v * 0.5 } else { v * 0.5 };
            p.pos + p.rot.apply(Vec3::new(pick(0, s.x), pick(1, s.y), pick(2, s.z)))
        })
        .collect()
}

/// The size a part is drawn at: balls use their smallest side, and
/// cylinders (along X) the smaller of their other two.
fn scaled(p: &Piece) -> Vec3 {
    let s = p.size;
    match p.shape {
        PartShape::Ball => {
            let d = s.x.min(s.y).min(s.z);
            Vec3::new(d, d, d)
        }
        PartShape::Cylinder => {
            let d = s.y.min(s.z);
            Vec3::new(s.x, d, d)
        }
        _ => s,
    }
}

fn mesh(p: &Piece, piece: u16, out: &mut Vec<Tri>) {
    let size = scaled(p);
    let world = |v: Vec3| p.pos + p.rot.apply(v * size);
    let normal =
        |n: Vec3| p.rot.apply(Vec3::new(n.x / size.x, n.y / size.y, n.z / size.z)).normalize_or_zero();
    let mut tri = |a: (Vec3, Vec3), b: (Vec3, Vec3), c: (Vec3, Vec3)| {
        let t = Tri {
            p: [world(a.0), world(b.0), world(c.0)],
            n: [normal(a.1), normal(b.1), normal(c.1)],
            color: p.color,
            neon: p.neon,
            piece,
        };
        let face = (t.p[1] - t.p[0]).cross(t.p[2] - t.p[0]);
        let out_dir = t.n[0] + t.n[1] + t.n[2];
        if face.dot(out_dir) < 0.0 {
            out.push(Tri { p: [t.p[0], t.p[2], t.p[1]], n: [t.n[0], t.n[2], t.n[1]], ..t });
        } else {
            out.push(t);
        }
    };
    let v = Vec3::new;
    let h = 0.5;
    let mut flat = |corners: &[Vec3], n: Vec3| {
        for k in 1..corners.len() - 1 {
            tri((corners[0], n), (corners[k], n), (corners[k + 1], n));
        }
    };
    match p.shape {
        PartShape::Block => {
            for axis in 0..3 {
                for sign in [-h, h] {
                    let at = |a: f32, b: f32| match axis {
                        0 => v(sign, a, b),
                        1 => v(a, sign, b),
                        _ => v(a, b, sign),
                    };
                    let n = at(0.0, 0.0) * 2.0;
                    flat(&[at(-h, -h), at(h, -h), at(h, h), at(-h, h)], n);
                }
            }
        }
        PartShape::Wedge => {
            let slope = v(0.0, 1.0, -1.0).normalize_or_zero();
            flat(&[v(-h, -h, -h), v(h, -h, -h), v(h, -h, h), v(-h, -h, h)], v(0.0, -1.0, 0.0));
            flat(&[v(-h, -h, h), v(h, -h, h), v(h, h, h), v(-h, h, h)], v(0.0, 0.0, 1.0));
            flat(&[v(-h, -h, -h), v(h, -h, -h), v(h, h, h), v(-h, h, h)], slope);
            flat(&[v(-h, -h, -h), v(-h, -h, h), v(-h, h, h)], v(-1.0, 0.0, 0.0));
            flat(&[v(h, -h, -h), v(h, -h, h), v(h, h, h)], v(1.0, 0.0, 0.0));
        }
        PartShape::Ball => {
            let (rings, around) = (14, 24);
            let at = |r: usize, a: usize| {
                let (t, f) = (
                    std::f32::consts::PI * r as f32 / rings as f32,
                    std::f32::consts::TAU * a as f32 / around as f32,
                );
                let n = v(t.sin() * f.cos(), t.cos(), t.sin() * f.sin());
                (n * h, n)
            };
            for r in 0..rings {
                for a in 0..around {
                    let (p00, p01, p10, p11) = (at(r, a), at(r, a + 1), at(r + 1, a), at(r + 1, a + 1));
                    if r > 0 {
                        tri(p00, p01, p11);
                    }
                    if r < rings - 1 {
                        tri(p00, p11, p10);
                    }
                }
            }
        }
        PartShape::Cylinder => {
            let around = 28;
            let ring = |a: usize| {
                let f = std::f32::consts::TAU * a as f32 / around as f32;
                (f.cos() * h, f.sin() * h)
            };
            for a in 0..around {
                let ((y0, z0), (y1, z1)) = (ring(a), ring(a + 1));
                let (n0, n1) = (v(0.0, y0, z0) * 2.0, v(0.0, y1, z1) * 2.0);
                tri((v(-h, y0, z0), n0), (v(h, y0, z0), n0), (v(h, y1, z1), n1));
                tri((v(-h, y0, z0), n0), (v(h, y1, z1), n1), (v(-h, y1, z1), n1));
                for x in [-h, h] {
                    let n = v(x * 2.0, 0.0, 0.0);
                    tri((v(x, 0.0, 0.0), n), (v(x, y0, z0), n), (v(x, y1, z1), n));
                }
            }
        }
    }
}

impl Picture {
    pub fn png(&self) -> Vec<u8> {
        use std::io::Write;
        let mut raw = Vec::with_capacity(self.rgba.len() + self.height as usize);
        for row in self.rgba.chunks(self.width as usize * 4) {
            raw.push(0);
            raw.extend_from_slice(row);
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&raw).expect("writing to memory works");
        let data = z.finish().expect("writing to memory works");
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut chunk = |kind: &[u8; 4], body: &[u8]| {
            out.extend((body.len() as u32).to_be_bytes());
            let start = out.len();
            out.extend(kind);
            out.extend(body);
            let crc = crc32(&out[start..]);
            out.extend(crc.to_be_bytes());
        };
        let mut header = Vec::new();
        header.extend(self.width.to_be_bytes());
        header.extend(self.height.to_be_bytes());
        header.extend([8, 6, 0, 0, 0]);
        chunk(b"IHDR", &header);
        chunk(b"IDAT", &data);
        chunk(b"IEND", &[]);
        out
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Hat, Look};

    #[test]
    fn pictures_show_the_avatar_on_a_clear_background() {
        let mut look = Look::for_player(1);
        look.hat = Hat::Crown;
        for (shot, w, h) in [(Shot::Head, 64, 64), (Shot::Body, 60, 80)] {
            let p = render(&look, shot, w, h);
            assert_eq!(p.rgba.len(), (w * h * 4) as usize);
            assert_eq!(p.rgba[3], 0);
            let solid = p.rgba.chunks(4).filter(|c| c[3] == 255).count();
            assert!(solid > (w * h / 5) as usize, "{shot:?} {solid}");
            assert!(p.png().starts_with(b"\x89PNG"));
        }
    }

    #[test]
    fn shirt_colour_shows_in_the_picture() {
        let mut look = Look::for_player(0);
        look.colors.shirt = Color3::from_rgb8(255, 0, 0);
        let p = render(&look, Shot::Body, 60, 80);
        let red = p.rgba.chunks(4).filter(|c| c[3] == 255 && c[0] > 120 && c[1] < 40 && c[2] < 40).count();
        assert!(red > 100, "{red}");
    }
}
