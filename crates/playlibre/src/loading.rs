use bevy_egui::egui::{self, Color32, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

use crate::i18n::{t, tf};
use crate::theme::{self, icon};

const FADE: f64 = 0.25;
const SHORTEST: f64 = 0.3;
const LONGEST_FILL: f64 = 5.0;
/// How long a join may go without anything new from the host before the
/// screen gives up and shows what came, rather than spinning forever.
const LONGEST_STALL: f64 = 30.0;

#[derive(Default)]
pub struct Loading {
    began: Option<f64>,
    came: Option<f64>,
    /// The join's progress, and when it last went up.
    moved: Option<(f32, f64)>,
    done: Option<f64>,
    /// What the loading screen waited on, once it is done, for the Output.
    finished: Option<String>,
    title: Option<String>,
    by: Option<String>,
}

impl Loading {
    pub fn start(&mut self, now: f64, title: Option<String>, by: Option<String>) {
        *self = Self { began: Some(now), title, by, ..Self::default() };
    }

    pub fn stop(&mut self) {
        *self = Self::default();
    }

    pub fn active(&self) -> bool {
        self.began.is_some()
    }

    /// Whether the screen is up and solid (not fading away).
    pub fn covering(&self) -> bool {
        self.began.is_some() && self.done.is_none()
    }

    /// Once, when the screen starts to go: how long it was up, and how
    /// much of that was waiting for the 3D view to fill after the world
    /// had arrived.
    pub fn take_finished(&mut self) -> Option<String> {
        self.finished.take()
    }

    /// Decides, from how loading is going, whether the screen should start
    /// to go.
    fn advance(&mut self, now: f64, progress: Option<f32>, filling: bool) {
        let Some(began) = self.began else { return };
        if self.done.is_none() {
            if let Some(p) = progress {
                self.came = None;
                if self.moved.is_none_or(|(was, _)| p > was) {
                    self.moved = Some((p, now));
                }
            } else if self.came.is_none() {
                self.came = Some(now);
            }
            let stalled = progress.is_some() && self.moved.is_some_and(|(_, at)| now - at > LONGEST_STALL);
            if stalled {
                self.done = Some(now);
                self.finished = Some(format!(
                    "⏱ Loading screen gone after {:.2} s: nothing new came from the host for {LONGEST_STALL:.0} s \
                     (at {:.0}% of the world)",
                    now - began,
                    self.moved.map_or(0.0, |(p, _)| p * 100.0),
                ));
            }
            let filled = !filling || self.came.is_some_and(|c| now - c > LONGEST_FILL);
            if self.done.is_none() && progress.is_none() && filled && now - began >= SHORTEST {
                self.done = Some(now);
                let came = self.came.unwrap_or(now);
                let why = if filling {
                    format!("gave up waiting for the 3D view after {LONGEST_FILL:.0} s, it was still filling")
                } else {
                    "the 3D view had filled".to_owned()
                };
                self.finished = Some(format!(
                    "⏱ Loading screen gone after {:.2} s: the world was in at {:.2} s, then {:.2} s waiting ({why})",
                    now - began,
                    came - began,
                    now - came,
                ));
            }
        }
    }

    /// Draws the loading screen over `view`. Returns true when the player
    /// taps its X to give up waiting (phones and tablets have no window to
    /// close).
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        view: Rect,
        now: f64,
        progress: Option<f32>,
        filling: bool,
    ) -> bool {
        let Some(began) = self.began else { return false };
        self.advance(now, progress, filling);
        let fade = self.done.map_or(1.0, |d| 1.0 - ((now - d) / FADE) as f32);
        if fade <= 0.0 {
            self.stop();
            return false;
        }
        ctx.request_repaint();
        let t = (now - began) as f32;
        let shown = progress.unwrap_or(if self.came.is_some() { 1.0 } else { 0.0 });
        let alpha = fade.clamp(0.0, 1.0);
        egui::Area::new("loading".into())
            .fixed_pos(view.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(view.size(), egui::Sense::click_and_drag());
                draw(ui.painter(), rect, t, shown, alpha, self.title.as_deref(), self.by.as_deref());
                // Only while it is still loading, not as it fades away.
                self.done.is_none() && close_button(ui, rect)
            })
            .inner
    }
}

/// The X in the top right corner, big enough for a finger.
fn close_button(ui: &mut egui::Ui, view: Rect) -> bool {
    const SIZE: f32 = 44.0;
    const MARGIN: f32 = 16.0;
    let at = Rect::from_min_size(pos2(view.max.x - MARGIN - SIZE, view.min.y + MARGIN), Vec2::splat(SIZE));
    let label = t("Stop loading");
    let button = egui::Button::new(egui::RichText::new(icon::X).size(22.0).color(Color32::WHITE))
        .fill(Color32::from_black_alpha(110))
        .corner_radius(SIZE / 2.0);
    let response = ui.put(at, button).on_hover_text(label);
    theme::name_for_screen_readers(ui, &response, egui::WidgetType::Button, label, None);
    response.clicked()
}

fn fade_color(c: Color32, a: f32) -> Color32 {
    c.gamma_multiply(a)
}

fn draw(
    painter: &egui::Painter,
    rect: Rect,
    t: f32,
    progress: f32,
    alpha: f32,
    title: Option<&str>,
    by: Option<&str>,
) {
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

    // The little cubes going round: the ones on the far side first, so the
    // big cube hides them, then the big cube, then the near ones over it.
    let orbit = |behind: bool| {
        for i in 0..3 {
            let a = t * 1.8 + i as f32 * std::f32::consts::TAU / 3.0;
            if (a.sin() < 0.0) != behind {
                continue;
            }
            let at = ring_center + vec2(a.cos() * unit * 2.4, a.sin() * unit * 0.7);
            let small = Cube { yaw: -t * 2.0 + i as f32, pitch: 0.55, size: unit * 0.22, stretch: 1.0 };
            small.faces(painter, at, alpha * if behind { 0.55 } else { 1.0 });
        }
    };
    orbit(true);
    let cube = Cube { yaw, pitch: 0.55, size: unit, stretch: squash };
    cube.faces(painter, center - vec2(0.0, lift), alpha);
    orbit(false);

    let mark = painter.layout(
        title.unwrap_or("PlayLibre").to_owned(),
        theme::light(30.0),
        fade_color(Color32::WHITE, alpha),
        rect.width() - 40.0,
    );
    let mark_at = pos2(rect.center().x - mark.size().x / 2.0, center.y + unit * 2.5);
    let mut below = mark_at.y + mark.size().y;
    painter.galley(mark_at, mark, Color32::WHITE);
    if let Some(by) = by {
        let line = painter.layout_no_wrap(
            tf("By {author}", &[("author", &by)]),
            theme::display(15.0),
            fade_color(Color32::from_white_alpha(170), alpha),
        );
        painter.galley(pos2(rect.center().x - line.size().x / 2.0, below + 4.0), line, Color32::WHITE);
        below += 24.0;
    }

    let bar = Rect::from_center_size(pos2(rect.center().x, below + 22.0), vec2(220.0, 4.0));
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
        // The faces go in as plain triangles and the edges as separate
        // lines, never as outlined polygons: as the cube turns, a face seen
        // almost side-on is a sliver with needle-sharp corners, and egui
        // joins a closed outline's corners with miters it never cuts short,
        // so those corners shot thin lines right across the screen.
        let mut mesh = egui::Mesh::default();
        let mut edges: Vec<(usize, usize)> = Vec::with_capacity(9);
        for (corners, normal) in FACES {
            let n = self.turn(normal);
            if n[2] <= 0.0 {
                continue;
            }
            let lit = 0.45 + 0.55 * (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]).max(0.0);
            let tint = |c: u8| ((c as f32 * lit) + 40.0 * lit * lit).min(255.0) as u8;
            let fill = fade_color(Color32::from_rgb(tint(base.r()), tint(base.g()), tint(base.b())), alpha);
            let first = mesh.vertices.len() as u32;
            for i in corners {
                mesh.colored_vertex(self.point(center, CORNERS[i]), fill);
            }
            mesh.add_triangle(first, first + 1, first + 2);
            mesh.add_triangle(first, first + 2, first + 3);
            for k in 0..4 {
                let (a, b) = (corners[k], corners[(k + 1) % 4]);
                let edge = (a.min(b), a.max(b));
                if !edges.contains(&edge) {
                    edges.push(edge);
                }
            }
        }
        painter.add(Shape::mesh(mesh));
        let stroke = Stroke::new(1.5, fade_color(Color32::from_white_alpha(90), alpha));
        for (a, b) in edges {
            painter.line_segment([self.point(center, CORNERS[a]), self.point(center, CORNERS[b])], stroke);
        }
    }

    fn edges(&self, painter: &egui::Painter, center: Pos2, stroke: Stroke) {
        for (a, b) in EDGES {
            painter.line_segment([self.point(center, CORNERS[a]), self.point(center, CORNERS[b])], stroke);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(loading: &mut Loading, now: f64, progress: Option<f32>) {
        loading.advance(now, progress, false);
    }

    #[test]
    fn a_join_that_stops_coming_lets_the_screen_go() {
        let mut loading = Loading::default();
        loading.start(0.0, None, None);
        for i in 0..100 {
            frame(&mut loading, f64::from(i) * 0.5, Some(0.4));
        }
        assert!(!loading.covering());
        assert!(loading.take_finished().is_some_and(|t| t.contains("nothing new came from the host")));
    }

    /// Every way the cubes turn, nothing they draw reaches past their own
    /// corners. Faces seen side-on used to send lines across the screen.
    #[test]
    fn turning_cubes_draw_nothing_past_their_corners() {
        let ctx = egui::Context::default();
        let center = pos2(400.0, 400.0);
        let size = 40.0;
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            for step in 0..3600 {
                let yaw = step as f32 * std::f32::consts::TAU / 3600.0;
                for stretch in [0.88, 1.0] {
                    Cube { yaw, pitch: 0.55, size, stretch }.faces(ui.painter(), center, 1.0);
                }
            }
        });
        // A cube's corners are at most size * sqrt(3) from its middle, a
        // little more when squashed, and its lines are 1.5 wide.
        let reach = size * 1.12 * 3f32.sqrt() + 3.0;
        for clipped in ctx.tessellate(output.shapes, 1.0) {
            let egui::epaint::Primitive::Mesh(mesh) = clipped.primitive else { continue };
            for vertex in &mesh.vertices {
                let far = (vertex.pos - center).length();
                assert!(far <= reach, "drew {far:.1} from the middle, reach is {reach:.1}");
            }
        }
    }

    #[test]
    fn a_join_that_keeps_coming_keeps_the_screen() {
        let mut loading = Loading::default();
        loading.start(0.0, None, None);
        for i in 0..100 {
            frame(&mut loading, f64::from(i) * 0.5, Some(i as f32 / 200.0));
        }
        assert!(loading.covering());
    }
}
