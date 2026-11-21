//! Libroxel's own chess pieces, drawn with shapes so they stay sharp at
//! any size: glossy pearl for White and smoky glass for Black.

use bevy_egui::egui::{self, Color32, Pos2, Rect, Shape, Stroke, Vec2};
use libroxel_chess::{Color, Kind, Piece};

/// The parts of a piece in a box from 0 to 1, each one convex.
enum Part {
    Poly(Vec<(f32, f32)>),
    Circle((f32, f32), f32),
}

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Part {
    Part::Poly(vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
}

fn trapezoid(top: f32, top_half: f32, bottom: f32, bottom_half: f32) -> Part {
    Part::Poly(vec![
        (0.5 - top_half, top),
        (0.5 + top_half, top),
        (0.5 + bottom_half, bottom),
        (0.5 - bottom_half, bottom),
    ])
}

/// The base every piece stands on.
fn base(parts: &mut Vec<Part>) {
    parts.push(trapezoid(0.74, 0.22, 0.82, 0.27));
    parts.push(rect(0.21, 0.81, 0.79, 0.89));
}

fn parts(kind: Kind) -> Vec<Part> {
    let mut p = Vec::new();
    match kind {
        Kind::Pawn => {
            p.push(trapezoid(0.44, 0.07, 0.76, 0.16));
            p.push(Part::Poly(vec![(0.35, 0.47), (0.65, 0.47), (0.62, 0.52), (0.38, 0.52)]));
            p.push(Part::Circle((0.5, 0.32), 0.13));
        }
        Kind::Rook => {
            p.push(trapezoid(0.36, 0.15, 0.76, 0.19));
            p.push(rect(0.27, 0.24, 0.73, 0.37));
            p.push(rect(0.27, 0.14, 0.37, 0.25));
            p.push(rect(0.45, 0.14, 0.55, 0.25));
            p.push(rect(0.63, 0.14, 0.73, 0.25));
        }
        Kind::Knight => {
            // Neck and chest.
            p.push(Part::Poly(vec![
                (0.30, 0.76),
                (0.36, 0.34),
                (0.52, 0.17),
                (0.68, 0.32),
                (0.72, 0.52),
                (0.70, 0.76),
            ]));
            // Head and muzzle, looking left.
            p.push(Part::Poly(vec![
                (0.52, 0.17),
                (0.36, 0.24),
                (0.21, 0.42),
                (0.19, 0.50),
                (0.27, 0.55),
                (0.46, 0.46),
            ]));
            // Ear.
            p.push(Part::Poly(vec![(0.45, 0.21), (0.52, 0.07), (0.58, 0.22)]));
        }
        Kind::Bishop => {
            p.push(trapezoid(0.50, 0.08, 0.76, 0.17));
            p.push(Part::Poly(vec![(0.36, 0.52), (0.64, 0.52), (0.61, 0.57), (0.39, 0.57)]));
            p.push(Part::Circle((0.5, 0.38), 0.15));
            p.push(Part::Poly(vec![(0.5, 0.14), (0.39, 0.31), (0.61, 0.31)]));
            p.push(Part::Circle((0.5, 0.13), 0.045));
        }
        Kind::Queen => {
            p.push(trapezoid(0.56, 0.12, 0.76, 0.21));
            p.push(trapezoid(0.30, 0.25, 0.58, 0.14));
            for (x, y) in [(0.22, 0.24), (0.36, 0.18), (0.5, 0.15), (0.64, 0.18), (0.78, 0.24)] {
                p.push(Part::Poly(vec![
                    (x, y),
                    (x - 0.06 + (0.5 - x) * 0.25, 0.42),
                    (x + 0.06 + (0.5 - x) * 0.25, 0.42),
                ]));
                p.push(Part::Circle((x, y), 0.045));
            }
        }
        Kind::King => {
            p.push(trapezoid(0.56, 0.12, 0.76, 0.21));
            p.push(trapezoid(0.36, 0.22, 0.58, 0.13));
            p.push(Part::Circle((0.5, 0.38), 0.15));
            p.push(rect(0.46, 0.05, 0.54, 0.27));
            p.push(rect(0.39, 0.11, 0.61, 0.18));
        }
    }
    base(&mut p);
    p
}

fn colours(color: Color) -> (Color32, Color32, Color32) {
    match color {
        // Body top, body bottom, outline.
        Color::White => (
            Color32::from_rgb(255, 255, 255),
            Color32::from_rgb(196, 222, 244),
            Color32::from_rgb(34, 62, 98),
        ),
        Color::Black => {
            (Color32::from_rgb(92, 112, 140), Color32::from_rgb(18, 26, 42), Color32::from_rgb(6, 10, 18))
        }
    }
}

/// Draws `piece` filling `rect`, see-through by `alpha` (0 to 1).
pub fn paint_piece(painter: &egui::Painter, rect: Rect, piece: Piece, alpha: f32) {
    let size = rect.width();
    let at = |(x, y): (f32, f32)| rect.min + Vec2::new(x * size, y * size);
    let (top, bottom, outline) = colours(piece.color);
    let fade = |c: Color32| c.gamma_multiply(alpha);
    let parts = parts(piece.kind);
    let rim = (size * 0.045).max(1.2);
    // A soft shadow on the square.
    painter.add(Shape::ellipse_filled(
        at((0.5, 0.87)),
        Vec2::new(size * 0.33, size * 0.06),
        Color32::from_black_alpha((50.0 * alpha) as u8),
    ));
    // First the outline, as every part a little bigger, so overlapping parts
    // merge into one silhouette.
    for part in &parts {
        match part {
            Part::Poly(points) => {
                painter.add(Shape::convex_polygon(
                    points.iter().map(|p| at(*p)).collect(),
                    fade(outline),
                    Stroke::new(rim * 2.0, fade(outline)),
                ));
            }
            Part::Circle(c, r) => {
                painter.circle_filled(at(*c), r * size + rim, fade(outline));
            }
        }
    }
    // Then the body, shaded from top to bottom like glass.
    let shade = |y: f32| {
        let t = ((y - 0.08) / 0.82).clamp(0.0, 1.0);
        fade(lerp(top, bottom, t))
    };
    for part in &parts {
        match part {
            Part::Poly(points) => {
                let mut mesh = egui::Mesh::default();
                for p in points {
                    mesh.colored_vertex(at(*p), shade(p.1));
                }
                for i in 1..points.len() as u32 - 1 {
                    mesh.add_triangle(0, i, i + 1);
                }
                painter.add(mesh);
            }
            Part::Circle(c, r) => {
                let mut mesh = egui::Mesh::default();
                mesh.colored_vertex(at(*c), shade(c.1 - r * 0.3));
                let steps = 28u32;
                for i in 0..steps {
                    let a = i as f32 / steps as f32 * std::f32::consts::TAU;
                    let p = (c.0 + r * a.cos(), c.1 + r * a.sin());
                    mesh.colored_vertex(at(p), shade(p.1));
                }
                for i in 0..steps {
                    mesh.add_triangle(0, 1 + i, 1 + (i + 1) % steps);
                }
                painter.add(mesh);
            }
        }
    }
    // Details drawn over the body.
    let line = Stroke::new(rim * 0.8, fade(outline));
    match piece.kind {
        Kind::Bishop => {
            painter.line_segment([at((0.55, 0.30)), at((0.47, 0.42))], line);
        }
        Kind::Knight => {
            let eye = if piece.color == Color::White { outline } else { Color32::from_rgb(170, 210, 245) };
            painter.circle_filled(at((0.40, 0.31)), size * 0.028, fade(eye));
            painter.line_segment([at((0.24, 0.48)), at((0.29, 0.47))], line);
        }
        Kind::Rook => {
            painter.line_segment([at((0.31, 0.37)), at((0.69, 0.37))], line);
        }
        Kind::Queen | Kind::King => {
            painter.line_segment([at((0.34, 0.57)), at((0.66, 0.57))], line);
        }
        Kind::Pawn => {}
    }
    painter.line_segment([at((0.24, 0.81)), at((0.76, 0.81))], line);
    // A gleam of light on the upper left.
    let gleam = Rect::from_center_size(at((0.42, 0.30)), Vec2::new(size * 0.10, size * 0.16));
    painter.add(Shape::ellipse_filled(
        gleam.center(),
        gleam.size() / 2.0,
        Color32::from_white_alpha((if piece.color == Color::White { 150.0 } else { 70.0 } * alpha) as u8),
    ));
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), m(a.a(), b.a()))
}

/// A small piece for the captured-pieces row.
pub fn paint_small(painter: &egui::Painter, centre: Pos2, size: f32, piece: Piece) {
    paint_piece(painter, Rect::from_center_size(centre, Vec2::splat(size)), piece, 1.0);
}
