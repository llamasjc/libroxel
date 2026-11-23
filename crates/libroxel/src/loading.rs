use bevy_egui::egui::{self, Color32, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

use crate::theme;

const FADE: f64 = 0.4;
const SHORTEST: f64 = 0.9;
const LONGEST_FILL: f64 = 10.0;

#[derive(Default)]
pub struct Loading {
    began: Option<f64>,
    came: Option<f64>,
    done: Option<f64>,
}

impl Loading {
    pub fn start(&mut self, now: f64) {
        *self = Self { began: Some(now), came: None, done: None };
    }

    pub fn stop(&mut self) {
        *self = Self::default();
    }

    pub fn active(&self) -> bool {
        self.began.is_some()
    }

    pub fn show(&mut self, ctx: &egui::Context, view: Rect, now: f64, progress: Option<f32>, filling: bool) {
        let Some(began) = self.began else { return };
        if self.done.is_none() {
            if progress.is_some() {
                self.came = None;
            } else if self.came.is_none() {
                self.came = Some(now);
            }
            let filled = !filling || self.came.is_some_and(|c| now - c > LONGEST_FILL);
            if progress.is_none() && filled && now - began >= SHORTEST {
                self.done = Some(now);
            }
        }
        let fade = self.done.map_or(1.0, |d| 1.0 - ((now - d) / FADE) as f32);
        if fade <= 0.0 {
            self.stop();
            return;
        }
        ctx.request_repaint();
        let t = (now - began) as f32;
        let shown = progress.unwrap_or(if self.came.is_some() { 1.0 } else { 0.0 });
        egui::Area::new("loading".into()).fixed_pos(view.min).order(egui::Order::Foreground).show(
            ctx,
            |ui| {
                let (rect, _) = ui.allocate_exact_size(view.size(), egui::Sense::click_and_drag());
                draw(ui.painter(), rect, t, shown, fade.clamp(0.0, 1.0));
            },
        );
    }
}

fn fade_color(c: Color32, a: f32) -> Color32 {
    c.gamma_multiply(a)
}

fn draw(painter: &egui::Painter, rect: Rect, t: f32, progress: f32, alpha: f32) {
    let top = Color32::from_rgb(14, 30, 58);
    let bottom = Color32::from_rgb(6, 12, 26);
    let mut back = egui::Mesh::default();
    back.colored_vertex(rect.left_top(), fade_color(top, alpha));
    back.colored_vertex(rect.right_top(), fade_color(top, alpha));
    back.colored_vertex(rect.right_bottom(), fade_color(bottom, alpha));
    back.colored_vertex(rect.left_bottom(), fade_color(bottom, alpha));
    back.add_triangle(0, 1, 2);
    back.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(back));

    let unit = (rect.width().min(rect.height()) * 0.075).clamp(26.0, 64.0);
    let center = rect.center() - vec2(0.0, unit * 0.9);

    let hop = (t * 2.6).sin().abs();
    let lift = hop * unit * 0.55;
    let squash = 1.0 - (1.0 - hop).powi(6) * 0.12;
    let flip = {
        let cycle = (t / 1.2).floor();
        let within = (t / 1.2).fract();
        let ease = 1.0 - (1.0 - (within / 0.45).min(1.0)).powi(3);
        cycle + ease
    };
    let yaw = t * 0.9 + flip * std::f32::consts::FRAC_PI_2;

    let shadow_w = unit * (1.5 - hop * 0.45);
    painter.add(Shape::ellipse_filled(
        center + vec2(0.0, unit * 1.9),
        vec2(shadow_w, shadow_w * 0.22),
        fade_color(Color32::from_black_alpha(120), alpha),
    ));

    let ring = Cube { yaw: -t * 0.7, pitch: 0.55, size: unit * 1.65, stretch: 1.0 };
    let ring_center = center - vec2(0.0, unit * 0.1);
    let glow = 0.5 + 0.5 * (t * 2.0).sin();
    ring.edges(
        painter,
        ring_center,
        Stroke::new(1.5, fade_color(theme::ACCENT, alpha * (0.35 + 0.35 * glow))),
    );

    let cube = Cube { yaw, pitch: 0.55, size: unit, stretch: squash };
    cube.faces(painter, center - vec2(0.0, lift), alpha);

    for i in 0..3 {
        let a = t * 1.8 + i as f32 * std::f32::consts::TAU / 3.0;
        let at = ring_center + vec2(a.cos() * unit * 2.4, a.sin() * unit * 0.7);
        let small = Cube { yaw: -t * 2.0 + i as f32, pitch: 0.55, size: unit * 0.22, stretch: 1.0 };
        let behind = a.sin() < 0.0;
        small.faces(painter, at, alpha * if behind { 0.55 } else { 1.0 });
    }

    let mark =
        painter.layout_no_wrap("Libroxel".to_owned(), theme::light(30.0), fade_color(Color32::WHITE, alpha));
    let mark_at = pos2(rect.center().x - mark.size().x / 2.0, center.y + unit * 2.5);
    painter.galley(mark_at, mark, Color32::WHITE);

    let bar = Rect::from_center_size(pos2(rect.center().x, mark_at.y + 58.0), vec2(220.0, 4.0));
    painter.rect_filled(bar, 2.0, fade_color(Color32::from_white_alpha(28), alpha));
    let shown = if progress > 0.0 {
        Rect::from_min_size(bar.min, vec2(bar.width() * progress.clamp(0.0, 1.0), bar.height()))
    } else {
        let x = (t * 0.8).fract();
        let w = bar.width() * 0.3;
        let left = bar.left() + (bar.width() + w) * x - w;
        Rect::from_min_max(
            pos2(left.max(bar.left()), bar.top()),
            pos2((left + w).min(bar.right()), bar.bottom()),
        )
    };
    painter.rect_filled(shown, 2.0, fade_color(theme::ACCENT, alpha));
}

struct Cube {
    yaw: f32,
    pitch: f32,
    size: f32,
    stretch: f32,
}

const CORNERS: [[f32; 3]; 8] = [
    [-1.0, -1.0, -1.0],
    [1.0, -1.0, -1.0],
    [1.0, 1.0, -1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, -1.0, 1.0],
    [1.0, -1.0, 1.0],
    [1.0, 1.0, 1.0],
    [-1.0, 1.0, 1.0],
];

const FACES: [([usize; 4], [f32; 3]); 6] = [
    ([4, 5, 6, 7], [0.0, 0.0, 1.0]),
    ([1, 0, 3, 2], [0.0, 0.0, -1.0]),
    ([5, 1, 2, 6], [1.0, 0.0, 0.0]),
    ([0, 4, 7, 3], [-1.0, 0.0, 0.0]),
    ([7, 6, 2, 3], [0.0, 1.0, 0.0]),
    ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
];

const EDGES: [(usize, usize); 12] =
    [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)];

impl Cube {
    fn turn(&self, v: [f32; 3]) -> [f32; 3] {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let x = v[0] * cy + v[2] * sy;
        let z = -v[0] * sy + v[2] * cy;
        let y = v[1] * cp - z * sp;
        let z = v[1] * sp + z * cp;
        [x, y, z]
    }

    fn point(&self, center: Pos2, v: [f32; 3]) -> Pos2 {
        let squash = [v[0] * (2.0 - self.stretch), v[1] * self.stretch, v[2] * (2.0 - self.stretch)];
        let [x, y, _] = self.turn(squash);
        center + Vec2::new(x, -y) * self.size
    }

    fn faces(&self, painter: &egui::Painter, center: Pos2, alpha: f32) {
        let light = {
            let l = [-0.45f32, 0.8, 0.55];
            let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
            [l[0] / n, l[1] / n, l[2] / n]
        };
        let base = theme::ACCENT;
        for (corners, normal) in FACES {
            let n = self.turn(normal);
            if n[2] <= 0.0 {
                continue;
            }
            let lit = 0.45 + 0.55 * (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]).max(0.0);
            let tint = |c: u8| ((c as f32 * lit) + 40.0 * lit * lit).min(255.0) as u8;
            let fill = Color32::from_rgb(tint(base.r()), tint(base.g()), tint(base.b()));
            let points: Vec<Pos2> = corners.iter().map(|i| self.point(center, CORNERS[*i])).collect();
            painter.add(Shape::convex_polygon(
                points,
                fade_color(fill, alpha),
                Stroke::new(1.5, fade_color(Color32::from_white_alpha(90), alpha)),
            ));
        }
    }

    fn edges(&self, painter: &egui::Painter, center: Pos2, stroke: Stroke) {
        for (a, b) in EDGES {
            painter.line_segment([self.point(center, CORNERS[a]), self.point(center, CORNERS[b])], stroke);
        }
    }
}
