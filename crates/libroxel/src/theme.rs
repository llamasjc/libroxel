//! Libroxel's look: one palette, one set of typefaces, one set of icons and
//! a few hand-drawn widgets, used by the home screen, the editor and the
//! in-game overlays alike.
//!
//! The idea is a bright, airy desktop from the late 2000s: a clear sky
//! fading down to fresh grass, soap bubbles and soft ribbons of light
//! drifting behind everything, and glass on top of it. Bars are tinted
//! glass with a gloss across their upper half, panels are frosted white,
//! and buttons are rounded and shiny like a boiled sweet, glowing blue
//! when the pointer is over them. Text is set in Selawik (an open
//! typeface with the proportions of a classic desktop UI font), code in
//! IBM Plex Mono. Icons are Phosphor's bold set, so they match each other
//! and can sit inside any line of text. Every picture here is drawn in
//! code, so nothing belongs to anyone else.

use bevy_egui::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Mesh, Pos2, Rect, Response,
    RichText, Sense, Shape, Stroke, TextStyle, Vec2,
};
pub use egui_phosphor::bold as icon;
use libroxel_instance::ClassKind;

// --- Palette -------------------------------------------------------------

/// Behind everything, where the sky isn't drawn.
pub const BACK: Color32 = Color32::from_rgb(214, 232, 247);
/// Panels and bars: a pale, cool white.
pub const PANEL: Color32 = Color32::from_rgb(241, 246, 252);
/// Cards and raised surfaces.
pub const CARD: Color32 = Color32::from_rgb(255, 255, 255);
/// Hovered surfaces: the pale blue a row lights up with.
pub const WELL: Color32 = Color32::from_rgb(229, 243, 252);
/// Hairlines between things.
pub const LINE: Color32 = Color32::from_rgb(180, 204, 228);
/// Text.
pub const TEXT: Color32 = Color32::from_rgb(28, 36, 48);
/// Headings: a deep, inky blue.
pub const HEADING: Color32 = Color32::from_rgb(30, 57, 91);
/// Quieter text: hints, notes, labels.
pub const MUTED: Color32 = Color32::from_rgb(92, 108, 128);
/// The one accent: what's chosen, what to press.
pub const ACCENT: Color32 = Color32::from_rgb(36, 128, 222);
/// The accent's edge and pressed face.
pub const ACCENT_DEEP: Color32 = Color32::from_rgb(18, 78, 158);
/// Going: play, joined, done.
pub const GREEN: Color32 = Color32::from_rgb(52, 158, 48);
pub const GREEN_DEEP: Color32 = Color32::from_rgb(28, 108, 30);
/// Links and GUI things.
pub const LINK: Color32 = Color32::from_rgb(0, 102, 204);
/// Stopping, deleting, errors.
pub const RED: Color32 = Color32::from_rgb(204, 46, 46);
/// Warnings, coins, lights: deep enough to read on white.
pub const AMBER: Color32 = Color32::from_rgb(196, 122, 0);
/// Things in the world (parts, models) in the explorer.
pub const ORANGE: Color32 = Color32::from_rgb(232, 118, 24);

/// The soft corner everything shares.
pub const CORNER: u8 = 5;

// --- Type ------------------------------------------------------------------

const DISPLAY: &str = "display";
const DISPLAY_BOLD: &str = "display-bold";
const LIGHT: &str = "light";

/// The semibold cut, for headings, tabs and buttons.
pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}

/// The bold cut, for the name and big numbers.
pub fn display_bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY_BOLD.into()))
}

/// The semilight cut, for big, airy page titles.
pub fn light(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(LIGHT.into()))
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.to_owned(), std::sync::Arc::new(FontData::from_static(bytes)));
    };
    add("selawik", include_bytes!("../assets/fonts/Selawik-Regular.ttf"));
    add("selawik-semibold", include_bytes!("../assets/fonts/Selawik-Semibold.ttf"));
    add("selawik-bold", include_bytes!("../assets/fonts/Selawik-Bold.ttf"));
    add("selawik-semilight", include_bytes!("../assets/fonts/Selawik-Semilight.ttf"));
    // Selawik covers Latin; Atkinson Hyperlegible fills in more of it.
    add("atkinson", include_bytes!("../assets/fonts/AtkinsonHyperlegible-Regular.ttf"));
    add("plex-mono", include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"));
    add("phosphor", egui_phosphor::variants::bytes::bold::FONT);
    // Each family falls back to the icons, then to egui's own fonts for
    // anything else (arrows, accents, other scripts).
    let fallback: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let family = |first: &str| {
        let mut list = vec![first.to_owned(), "atkinson".to_owned(), "phosphor".to_owned()];
        list.extend(fallback.iter().cloned());
        list
    };
    let proportional = family("selawik");
    let mut mono = vec!["plex-mono".to_owned(), "phosphor".to_owned()];
    mono.extend(fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default());
    fonts.families.insert(FontFamily::Proportional, proportional);
    fonts.families.insert(FontFamily::Monospace, mono);
    fonts.families.insert(FontFamily::Name(DISPLAY.into()), family("selawik-semibold"));
    fonts.families.insert(FontFamily::Name(DISPLAY_BOLD.into()), family("selawik-bold"));
    fonts.families.insert(FontFamily::Name(LIGHT.into()), family("selawik-semilight"));
    fonts
}

/// Puts Libroxel's fonts and colours on `ctx`, once. New fonts only take
/// effect from the next frame, so this says whether they can be used yet:
/// draw nothing until it does.
pub fn install(ctx: &egui::Context) -> bool {
    let id = egui::Id::new("libroxel theme installed");
    if ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false) {
        return ctx.fonts(|f| f.families().contains(&FontFamily::Name(LIGHT.into())));
    }
    ctx.data_mut(|d| d.insert_temp(id, true));
    ctx.set_fonts(fonts());
    ctx.all_styles_mut(style);
    false
}

fn style(style: &mut egui::Style) {
    use egui::style::{Selection, WidgetVisuals, Widgets};
    style.text_styles = [
        (TextStyle::Heading, light(23.0)),
        (TextStyle::Body, FontId::proportional(14.0)),
        (TextStyle::Button, FontId::proportional(14.0)),
        (TextStyle::Small, FontId::proportional(11.5)),
        (TextStyle::Monospace, FontId::monospace(13.0)),
    ]
    .into();
    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(8.0, 6.0);
    s.button_padding = Vec2::new(11.0, 4.0);
    s.menu_margin = egui::Margin::same(6);
    s.window_margin = egui::Margin::same(12);
    s.interact_size = Vec2::new(36.0, 24.0);
    s.indent = 16.0;
    s.scroll.bar_width = 9.0;
    s.scroll.floating = true;

    let v = &mut style.visuals;
    *v = egui::Visuals::light();
    let corner = CornerRadius::same(3);
    let look = |fill: Color32, edge: Color32, text: Color32| WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: Stroke::new(1.0, edge),
        corner_radius: corner,
        fg_stroke: Stroke::new(1.5, text),
        expansion: 0.0,
    };
    v.widgets = Widgets {
        noninteractive: WidgetVisuals {
            bg_fill: PANEL,
            weak_bg_fill: PANEL,
            bg_stroke: Stroke::new(1.0, LINE),
            corner_radius: corner,
            fg_stroke: Stroke::new(1.0, TEXT),
            expansion: 0.0,
        },
        // A plain desktop button: pale grey with a grey rim, lighting up
        // sky blue under the pointer and a deeper blue while pressed.
        inactive: look(Color32::from_rgb(238, 240, 243), Color32::from_rgb(160, 168, 178), TEXT),
        hovered: look(Color32::from_rgb(222, 241, 253), Color32::from_rgb(60, 127, 177), Color32::BLACK),
        active: look(Color32::from_rgb(196, 229, 246), Color32::from_rgb(44, 98, 139), Color32::BLACK),
        open: look(Color32::from_rgb(222, 241, 253), Color32::from_rgb(60, 127, 177), Color32::BLACK),
    };
    v.selection =
        Selection { bg_fill: Color32::from_rgb(170, 214, 250), stroke: Stroke::new(1.5, ACCENT_DEEP) };
    v.hyperlink_color = LINK;
    v.faint_bg_color = WELL;
    v.extreme_bg_color = Color32::WHITE;
    v.code_bg_color = Color32::from_rgb(246, 249, 252);
    v.warn_fg_color = AMBER;
    v.error_fg_color = RED;
    v.window_fill = Color32::from_rgb(248, 251, 254);
    v.window_stroke = Stroke::new(1.0, Color32::from_rgb(110, 150, 190));
    v.window_corner_radius = CornerRadius::same(8);
    // Soft, wide shadows, as if every window floats a little above the sky.
    v.window_shadow = egui::Shadow {
        offset: [0, 6],
        blur: 22,
        spread: 0,
        color: Color32::from_rgba_unmultiplied(10, 40, 80, 70),
    };
    v.popup_shadow = egui::Shadow {
        offset: [0, 3],
        blur: 10,
        spread: 0,
        color: Color32::from_rgba_unmultiplied(10, 40, 80, 60),
    };
    v.menu_corner_radius = CornerRadius::same(4);
    v.panel_fill = PANEL;
    v.striped = false;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.text_cursor.stroke = Stroke::new(1.5, Color32::BLACK);
}

// --- Painting helpers --------------------------------------------------------

/// egui's colours are blended after being turned to linear light, so a
/// pale colour laid thinly over another comes out greyer than it should:
/// thin white gloss looks like smoke. This gives the colour that blends
/// the way it would on paper, `a` of the way from what's under it to
/// `(r, g, b)`.
pub fn see_through(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    fix(Color32::from_rgba_unmultiplied(r, g, b, a))
}

/// [`see_through`] for a colour already made.
fn fix(colour: Color32) -> Color32 {
    let [r, g, b, a] = colour.to_srgba_unmultiplied();
    if a == 0 || a == 255 {
        return colour;
    }
    let alpha = a as f32 / 255.0;
    let channel =
        |c: u8| egui::ecolor::gamma_u8_from_linear_f32(egui::ecolor::linear_f32_from_gamma_u8(c) * alpha);
    Color32::from_rgba_premultiplied(channel(r), channel(g), channel(b), a)
}

/// A mesh as a shape, its see-through colours fixed as [`see_through`] does.
fn fixed(mut mesh: Mesh) -> Shape {
    for vertex in &mut mesh.vertices {
        vertex.color = fix(vertex.color);
    }
    Shape::mesh(mesh)
}

/// The outline of a rounded rectangle, as points going round it.
fn rounded_outline(rect: Rect, radius: f32) -> Vec<Pos2> {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0);
    if r < 0.5 {
        return vec![rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
    }
    let steps = ((r / 1.5) as usize).clamp(3, 10);
    let mut points = Vec::with_capacity(steps * 4 + 4);
    let corners = [
        (Pos2::new(rect.max.x - r, rect.min.y + r), -90.0f32),
        (Pos2::new(rect.max.x - r, rect.max.y - r), 0.0),
        (Pos2::new(rect.min.x + r, rect.max.y - r), 90.0),
        (Pos2::new(rect.min.x + r, rect.min.y + r), 180.0),
    ];
    for (centre, start) in corners {
        for i in 0..=steps {
            let a = (start + 90.0 * i as f32 / steps as f32).to_radians();
            points.push(centre + Vec2::new(a.cos(), a.sin()) * r);
        }
    }
    points
}

/// The colour `t` of the way (0 at the top, 1 at the bottom) down a list
/// of stops.
fn ramp(stops: &[(f32, Color32)], t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mut last = stops[0];
    for &stop in stops {
        if t <= stop.0 {
            let span = (stop.0 - last.0).max(1e-4);
            return lerp(last.1, stop.1, (t - last.0) / span);
        }
        last = stop;
    }
    last.1
}

/// Mixes two colours, alpha and all.
fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    let (a, b) = (a.to_srgba_unmultiplied(), b.to_srgba_unmultiplied());
    Color32::from_rgba_unmultiplied(m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2]), m(a[3], b[3]))
}

/// Fills `rect` (rounded by `radius`) with colours running from top to
/// bottom through `stops`, given as (how far down, colour).
pub fn gradient(painter: &egui::Painter, rect: Rect, radius: f32, stops: &[(f32, Color32)]) {
    painter.add(gradient_shape(rect, radius, stops));
}

/// The shape [`gradient`] paints, to paint later or under something else.
pub fn gradient_shape(rect: Rect, radius: f32, stops: &[(f32, Color32)]) -> Shape {
    if rect.height() <= 0.0 || rect.width() <= 0.0 {
        return Shape::Noop;
    }
    let outline = rounded_outline(rect, radius);
    let mut mesh = Mesh::default();
    let shade = |p: Pos2| ramp(stops, (p.y - rect.min.y) / rect.height());
    // A fan from the centre works for any convex outline; the stops in
    // between are kept by cutting the fan into rows where they fall.
    // See-through colours need many rows (see `fix`); opaque ones don't.
    let clear = stops.iter().any(|s| s.1.a() < 255);
    if stops.len() <= 2 && !clear {
        let centre = rect.center();
        mesh.colored_vertex(centre, shade(centre));
        for p in &outline {
            mesh.colored_vertex(*p, shade(*p));
        }
        let n = outline.len() as u32;
        for i in 0..n {
            mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
        }
    } else {
        // Horizontal bands between stops, each a trapezoid strip clipped
        // to the rounded outline by its width at that height.
        let width_at = |y: f32| -> (f32, f32) {
            let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0);
            let inset = if y < rect.min.y + r {
                let d = rect.min.y + r - y;
                r - (r * r - d * d).max(0.0).sqrt()
            } else if y > rect.max.y - r {
                let d = y - (rect.max.y - r);
                r - (r * r - d * d).max(0.0).sqrt()
            } else {
                0.0
            };
            (rect.min.x + inset, rect.max.x - inset)
        };
        let mut ys: Vec<f32> =
            stops.iter().map(|s| rect.min.y + s.0.clamp(0.0, 1.0) * rect.height()).collect();
        let r = radius.min(rect.height() / 2.0);
        for i in 0..=6 {
            ys.push(rect.min.y + r * i as f32 / 6.0);
            ys.push(rect.max.y - r * i as f32 / 6.0);
        }
        ys.push(rect.min.y);
        ys.push(rect.max.y);
        if clear {
            let rows = (rect.height() / 3.0).clamp(2.0, 64.0) as usize;
            ys.extend((1..rows).map(|i| rect.min.y + rect.height() * i as f32 / rows as f32));
        }
        ys.sort_by(|a, b| a.total_cmp(b));
        ys.dedup_by(|a, b| (*a - *b).abs() < 0.25);
        for y in &ys {
            let (l, r) = width_at(*y);
            let c = ramp(stops, (*y - rect.min.y) / rect.height());
            mesh.colored_vertex(Pos2::new(l, *y), c);
            mesh.colored_vertex(Pos2::new(r, *y), c);
        }
        for i in 0..ys.len().saturating_sub(1) as u32 {
            let a = i * 2;
            mesh.add_triangle(a, a + 1, a + 2);
            mesh.add_triangle(a + 1, a + 3, a + 2);
        }
    }
    fixed(mesh)
}

/// A disc fading from `inner` at its centre to `outer` at its rim.
pub fn radial(painter: &egui::Painter, centre: Pos2, radius: f32, inner: Color32, outer: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(centre, inner);
    // Several rings, so see-through colours fade evenly (see `fix`).
    let (n, rings) = (40u32, 10u32);
    for ring in 1..=rings {
        let t = ring as f32 / rings as f32;
        let colour = lerp(inner, outer, t);
        for i in 0..n {
            let a = std::f32::consts::TAU * i as f32 / n as f32;
            mesh.colored_vertex(centre + Vec2::new(a.cos(), a.sin()) * radius * t, colour);
        }
    }
    let at = |ring: u32, i: u32| 1 + (ring - 1) * n + i % n;
    for i in 0..n {
        mesh.add_triangle(0, at(1, i), at(1, i + 1));
        for ring in 1..rings {
            mesh.add_triangle(at(ring, i), at(ring + 1, i), at(ring, i + 1));
            mesh.add_triangle(at(ring, i + 1), at(ring + 1, i), at(ring + 1, i + 1));
        }
    }
    painter.add(fixed(mesh));
}

/// A soap bubble: clear in the middle, bright at its rim, with a gleam.
pub fn bubble(painter: &egui::Painter, centre: Pos2, radius: f32, strength: f32) {
    let a = |x: f32| (x * strength).clamp(0.0, 255.0) as u8;
    let mut mesh = Mesh::default();
    let n = 48;
    // A ring that is clear inside and glows towards the edge.
    const RINGS: [(f32, f32); 9] = [
        (0.0, 0.0),
        (0.4, 4.0),
        (0.6, 10.0),
        (0.72, 18.0),
        (0.8, 30.0),
        (0.87, 46.0),
        (0.94, 70.0),
        (0.97, 48.0),
        (1.0, 20.0),
    ];
    for (i, (r, alpha)) in RINGS.iter().enumerate() {
        let colour = Color32::from_rgba_unmultiplied(255, 255, 255, a(*alpha));
        if i == 0 {
            mesh.colored_vertex(centre, colour);
            continue;
        }
        for k in 0..n {
            let t = std::f32::consts::TAU * k as f32 / n as f32;
            mesh.colored_vertex(centre + Vec2::new(t.cos(), t.sin()) * radius * *r, colour);
        }
    }
    let ring = |r: u32, k: u32| 1 + (r - 1) * n + (k % n);
    for k in 0..n {
        mesh.add_triangle(0, ring(1, k), ring(1, k + 1));
        for r in 1..RINGS.len() as u32 - 1 {
            mesh.add_triangle(ring(r, k), ring(r + 1, k), ring(r, k + 1));
            mesh.add_triangle(ring(r, k + 1), ring(r + 1, k), ring(r + 1, k + 1));
        }
    }
    painter.add(fixed(mesh));
    // The gleam, up and to the left.
    let gleam = centre + Vec2::new(-radius * 0.38, -radius * 0.42);
    radial(
        painter,
        gleam,
        radius * 0.3,
        Color32::from_rgba_unmultiplied(255, 255, 255, a(150.0)),
        Color32::from_rgba_unmultiplied(255, 255, 255, 0),
    );
    painter.circle_stroke(centre, radius, Stroke::new(1.0, see_through(255, 255, 255, a(90.0))));
}

/// A ribbon of light: a soft band following a gentle wave across `rect`,
/// `thickness` tall, bright along its middle and fading at both edges.
fn light_ribbon(painter: &egui::Painter, rect: Rect, base: f32, thickness: f32, phase: f32, alpha: u8) {
    let steps = 48;
    let mut mesh = Mesh::default();
    let clear = Color32::from_rgba_unmultiplied(255, 255, 255, 0);
    let bright = Color32::from_rgba_unmultiplied(255, 255, 255, alpha);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = rect.min.x + t * rect.width();
        let wave = (t * 3.1 + phase).sin() * 0.55 + (t * 7.3 + phase * 2.0).sin() * 0.12;
        let y = rect.min.y + rect.height() * base + wave * rect.height() * 0.11;
        // Thinner at the ends, fuller in the middle.
        let h = thickness * (0.35 + 0.65 * (t * std::f32::consts::PI).sin());
        mesh.colored_vertex(Pos2::new(x, y - h), clear);
        mesh.colored_vertex(Pos2::new(x, y - h * 0.15), bright);
        mesh.colored_vertex(Pos2::new(x, y + h), clear);
    }
    for i in 0..steps as u32 {
        let a = i * 3;
        let b = a + 3;
        mesh.add_triangle(a, b, a + 1);
        mesh.add_triangle(a + 1, b, b + 1);
        mesh.add_triangle(a + 1, b + 1, a + 2);
        mesh.add_triangle(a + 2, b + 1, b + 2);
    }
    painter.add(fixed(mesh));
}

/// The sky behind the home screen: a clear blue fading down to the
/// horizon, a sunny glow, ribbons of light, bubbles, and soft green hills
/// along the bottom.
pub fn paint_sky(painter: &egui::Painter, rect: Rect) {
    gradient(
        painter,
        rect,
        0.0,
        &[
            (0.0, Color32::from_rgb(28, 112, 196)),
            (0.42, Color32::from_rgb(94, 172, 232)),
            (0.78, Color32::from_rgb(196, 230, 250)),
            (1.0, Color32::from_rgb(226, 244, 252)),
        ],
    );
    // The sun's glow, high on the right.
    let sun = Pos2::new(rect.min.x + rect.width() * 0.8, rect.min.y + rect.height() * 0.08);
    radial(
        painter,
        sun,
        rect.width().max(rect.height()) * 0.42,
        Color32::from_rgba_unmultiplied(255, 255, 255, 120),
        Color32::from_rgba_unmultiplied(255, 255, 255, 0),
    );
    light_ribbon(painter, rect, 0.38, rect.height() * 0.09, 0.4, 70);
    light_ribbon(painter, rect, 0.5, rect.height() * 0.05, 2.1, 60);
    light_ribbon(painter, rect, 0.3, rect.height() * 0.03, 4.0, 50);
    paint_hills(painter, rect);
    paint_bubbles_over(painter, rect, 1.0);
}

/// Two rolling green hills along the bottom of `rect`.
fn paint_hills(painter: &egui::Painter, rect: Rect) {
    let hill = |base: f32, height: f32, phase: f32, top: Color32, bottom: Color32| {
        let steps = 64;
        let mut mesh = Mesh::default();
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let x = rect.min.x + t * rect.width();
            let y = rect.max.y
                - rect.height() * base
                - ((t * 2.4 + phase).sin() * 0.5 + 0.5) * rect.height() * height;
            mesh.colored_vertex(Pos2::new(x, y), top);
            mesh.colored_vertex(Pos2::new(x, rect.max.y), bottom);
        }
        for i in 0..steps as u32 {
            let a = i * 2;
            mesh.add_triangle(a, a + 1, a + 2);
            mesh.add_triangle(a + 1, a + 3, a + 2);
        }
        painter.add(fixed(mesh));
    };
    hill(0.07, 0.06, 0.6, Color32::from_rgb(150, 214, 110), Color32::from_rgb(86, 168, 60));
    hill(0.0, 0.08, 2.6, Color32::from_rgb(120, 200, 80), Color32::from_rgb(54, 140, 40));
    // A gloss along the near hill's crest, as if it has just rained.
    let shine = Rect::from_min_max(Pos2::new(rect.min.x, rect.max.y - rect.height() * 0.1), rect.max);
    gradient(
        painter,
        shine,
        0.0,
        &[
            (0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 0)),
            (0.5, Color32::from_rgba_unmultiplied(255, 255, 255, 24)),
            (1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 0)),
        ],
    );
}

/// A few bubbles drifting over `rect`, the same ones every time.
fn paint_bubbles_over(painter: &egui::Painter, rect: Rect, strength: f32) {
    const BUBBLES: [(f32, f32, f32); 11] = [
        (0.06, 0.22, 0.05),
        (0.12, 0.62, 0.028),
        (0.22, 0.12, 0.018),
        (0.34, 0.8, 0.04),
        (0.5, 0.18, 0.024),
        (0.62, 0.66, 0.06),
        (0.72, 0.3, 0.02),
        (0.84, 0.56, 0.035),
        (0.92, 0.16, 0.045),
        (0.95, 0.82, 0.022),
        (0.44, 0.45, 0.014),
    ];
    let unit = rect.width().max(rect.height());
    for (x, y, r) in BUBBLES {
        let centre = Pos2::new(rect.min.x + rect.width() * x, rect.min.y + rect.height() * y);
        bubble(painter, centre, (r * unit).max(4.0), strength);
    }
}

/// A sheet of tinted glass over `rect`: the tint fading downwards, a
/// bright gloss over its upper half, a dark outer rim and a white inner
/// one, the way light catches the edge of a pane.
pub fn glass(painter: &egui::Painter, rect: Rect, radius: f32, tint: Color32) {
    if tint.a() == 255 {
        // Solid glass: the same shine as a glossy button.
        let rim = lerp(tint, Color32::BLACK, 0.45);
        glossy(
            painter,
            rect,
            radius,
            (lerp(tint, Color32::WHITE, 0.2), lerp(tint, Color32::BLACK, 0.12), rim),
            false,
            false,
        );
        return;
    }
    let [r, g, b, a] = tint.to_srgba_unmultiplied();
    let shade = |k: f32, alpha: f32| {
        let m = |c: u8| (c as f32 * k).clamp(0.0, 255.0) as u8;
        Color32::from_rgba_unmultiplied(m(r), m(g), m(b), (a as f32 * alpha).clamp(0.0, 255.0) as u8)
    };
    gradient(painter, rect, radius, &[(0.0, shade(1.08, 0.9)), (1.0, shade(0.86, 1.0))]);
    let gloss = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.center().y));
    let gloss_radius = CornerRadius { nw: radius as u8, ne: radius as u8, sw: 0, se: 0 };
    let _ = gloss_radius;
    gradient(
        painter,
        gloss.shrink2(Vec2::new(1.0, 0.0)).translate(Vec2::new(0.0, 1.0)),
        (radius - 1.0).max(0.0),
        &[
            (0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 150)),
            (1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 40)),
        ],
    );
    let corner = CornerRadius::same(radius as u8);
    painter.rect_stroke(
        rect.shrink(1.0),
        CornerRadius::same((radius - 1.0).max(0.0) as u8),
        Stroke::new(1.0, see_through(255, 255, 255, 120)),
        egui::StrokeKind::Inside,
    );
    painter.rect_stroke(rect, corner, Stroke::new(1.0, shade(0.45, 1.4)), egui::StrokeKind::Inside);
}

/// The glass bar along the top of the home screen: sky blue with a
/// bright gloss over its upper half, like a window's title bar.
pub fn glass_bar(painter: &egui::Painter, rect: Rect) {
    gradient(
        painter,
        rect,
        0.0,
        &[
            (0.0, Color32::from_rgb(214, 236, 252)),
            (0.48, Color32::from_rgb(170, 212, 246)),
            (0.5, Color32::from_rgb(128, 186, 238)),
            (1.0, Color32::from_rgb(150, 202, 244)),
        ],
    );
    // A faint ribbon of light through it, and a bright line along the top.
    light_ribbon(painter, rect, 0.5, rect.height() * 0.5, 1.2, 60);
    painter.hline(rect.x_range(), rect.min.y + 0.5, Stroke::new(1.0, Color32::WHITE));
}

/// Text with a soft white glow behind it, the way titles sit on glass.
pub fn glow_text(painter: &egui::Painter, at: Pos2, galley: std::sync::Arc<egui::Galley>, colour: Color32) {
    let halo = see_through(255, 255, 255, 70);
    for (dx, dy) in
        [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-1.5, -1.5), (1.5, 1.5), (1.5, -1.5), (-1.5, 1.5)]
    {
        painter.galley_with_override_text_color(at + Vec2::new(dx, dy), galley.clone(), halo);
    }
    painter.galley(at, galley, colour);
}

/// A soft shadow under `rect`, like a pane floating over the sky.
pub fn soft_shadow(painter: &egui::Painter, rect: Rect, radius: f32, strength: u8) {
    let shadow = egui::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_rgba_unmultiplied(10, 40, 80, strength),
    };
    painter.add(shadow.as_shape(rect, CornerRadius::same(radius as u8)));
}

/// A big frosted-glass pane over `rect`, for a page to sit on: nearly
/// white, a little brighter at the top, with a soft shadow, a gloss band
/// along its top edge and a glassy rim.
pub fn frosted_pane(painter: &egui::Painter, rect: Rect) {
    soft_shadow(painter, rect, 10.0, 70);
    // Frosted all the way through, so pages read clearly; the sky shows
    // round its edges instead.
    gradient(
        painter,
        rect,
        10.0,
        &[
            (0.0, Color32::from_rgb(255, 255, 255)),
            (0.06, Color32::from_rgb(250, 253, 255)),
            (1.0, Color32::from_rgb(238, 246, 253)),
        ],
    );
    painter.rect_stroke(
        rect.shrink(1.0),
        CornerRadius::same(9),
        Stroke::new(1.0, see_through(255, 255, 255, 220)),
        egui::StrokeKind::Inside,
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(10),
        Stroke::new(1.0, see_through(40, 100, 170, 140)),
        egui::StrokeKind::Inside,
    );
}

// --- Widgets ---------------------------------------------------------------

/// What a glossy button is for, which sets its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The one main thing to do here.
    Primary,
    /// Starting a game.
    Go,
    /// Everything else.
    Plain,
    /// Stopping or removing.
    Danger,
}

impl Kind {
    /// The face's top and bottom colours, its rim and its text.
    fn colours(self) -> (Color32, Color32, Color32, Color32) {
        match self {
            Kind::Primary => (
                Color32::from_rgb(98, 182, 247),
                Color32::from_rgb(22, 108, 206),
                Color32::from_rgb(16, 74, 150),
                Color32::WHITE,
            ),
            Kind::Go => (
                Color32::from_rgb(140, 220, 96),
                Color32::from_rgb(46, 150, 36),
                Color32::from_rgb(26, 100, 24),
                Color32::WHITE,
            ),
            Kind::Plain => (
                Color32::from_rgb(252, 252, 252),
                Color32::from_rgb(214, 220, 228),
                Color32::from_rgb(132, 142, 156),
                TEXT,
            ),
            Kind::Danger => (
                Color32::from_rgb(246, 124, 112),
                Color32::from_rgb(196, 40, 34),
                Color32::from_rgb(132, 24, 20),
                Color32::WHITE,
            ),
        }
    }
}

/// Room left under a glossy button for its glow.
const EDGE: f32 = 4.0;

/// Paints a glossy face over `rect`: `top` fading to `bottom`, a gloss
/// over the upper half and a rim. `lit` brightens it (pointer over it);
/// `pressed` darkens it and dims the gloss.
fn glossy(
    painter: &egui::Painter,
    rect: Rect,
    radius: f32,
    colours: (Color32, Color32, Color32),
    lit: bool,
    pressed: bool,
) {
    let (mut top, mut bottom, rim) = colours;
    if lit {
        top = lerp(top, Color32::WHITE, 0.18);
        bottom = lerp(bottom, Color32::WHITE, 0.12);
    }
    if pressed {
        top = lerp(top, bottom, 0.6).gamma_multiply(0.92).to_opaque();
        bottom = bottom.gamma_multiply(0.88).to_opaque();
    }
    // The gloss is folded into the face's own colours, all solid: a bright
    // upper half that ends in a sharp line, then the deeper colour, which
    // lightens again at the very bottom as light pools in a glass bead.
    let (shine, pool) = if pressed { (0.18, 0.05) } else { (0.62, 0.22) };
    let mid = lerp(top, bottom, 0.5);
    gradient(
        painter,
        rect,
        radius,
        &[
            (0.0, lerp(top, Color32::WHITE, shine)),
            (0.47, lerp(lerp(top, mid, 0.6), Color32::WHITE, shine * 0.45)),
            (0.5, lerp(mid, bottom, 0.25)),
            (0.82, bottom),
            (1.0, lerp(bottom, Color32::WHITE, pool)),
        ],
    );
    let corner = CornerRadius::same(radius as u8);
    painter.rect_stroke(
        rect.shrink(1.0),
        CornerRadius::same((radius - 1.0).max(0.0) as u8),
        Stroke::new(1.0, see_through(255, 255, 255, if pressed { 40 } else { 110 })),
        egui::StrokeKind::Inside,
    );
    painter.rect_stroke(rect, corner, Stroke::new(1.0, rim), egui::StrokeKind::Inside);
}

/// A glossy, rounded button, glowing under the pointer. `glyph` is an
/// icon from [`icon`] (or "").
pub fn block_button(ui: &mut egui::Ui, kind: Kind, glyph: &str, label: &str, height: f32) -> Response {
    let size = (height * 0.4).max(13.0);
    let font = display(size);
    let text = if glyph.is_empty() { label.to_owned() } else { format!("{glyph}  {label}") };
    let width = ui.painter().layout_no_wrap(text.clone(), font.clone(), Color32::WHITE).size().x
        + height.max(24.0) * 0.95;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height + EDGE), Sense::click());
    let enabled = ui.is_enabled();
    let (top, bottom, rim, ink) = kind.colours();
    let (top, bottom, rim, ink) = if enabled {
        (top, bottom, rim, ink)
    } else {
        (
            Color32::from_rgb(244, 245, 247),
            Color32::from_rgb(226, 229, 233),
            Color32::from_rgb(186, 192, 200),
            Color32::from_rgb(150, 156, 166),
        )
    };
    let pressed = response.is_pointer_button_down_on() && enabled;
    let hovered = response.hovered() && enabled;
    let face = Rect::from_min_size(rect.min + Vec2::new(0.0, 1.0), Vec2::new(width, height));
    let radius = (height * 0.22).clamp(4.0, 11.0);
    let painter = ui.painter();
    // A soft shadow under it, and a blue glow while the pointer is over it.
    soft_shadow(painter, face.translate(Vec2::new(0.0, -2.0)), radius, if pressed { 20 } else { 45 });
    if hovered {
        let glow = if kind == Kind::Plain { Color32::from_rgb(90, 180, 255) } else { top };
        let shadow = egui::Shadow { offset: [0, 0], blur: 14, spread: 1, color: glow.gamma_multiply(0.75) };
        painter.add(shadow.as_shape(face, CornerRadius::same(radius as u8)));
    }
    let (top, bottom, rim) = if hovered && kind == Kind::Plain {
        (Color32::from_rgb(234, 246, 253), Color32::from_rgb(178, 224, 250), Color32::from_rgb(60, 127, 177))
    } else {
        (top, bottom, rim)
    };
    glossy(painter, face, radius, (top, bottom, rim), hovered, pressed);
    let galley = painter.layout_no_wrap(text, font, ink);
    let at = face.center() - galley.size() / 2.0 + Vec2::new(0.0, if pressed { 1.0 } else { 0.0 });
    if ink == Color32::WHITE {
        // A faint dark shadow keeps white letters crisp on the shine.
        painter.galley_with_override_text_color(
            at + Vec2::new(0.0, 1.0),
            galley.clone(),
            Color32::from_black_alpha(70),
        );
    }
    painter.galley(at, galley, ink);
    if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }
}

/// A small flat button with an icon, for toolbars and rows. It lights up
/// with a pale blue gloss under the pointer, and stays lit while `on`.
pub fn tool_button(ui: &mut egui::Ui, glyph: &str, label: &str, on: bool) -> Response {
    let text = if label.is_empty() { glyph.to_owned() } else { format!("{glyph} {label}") };
    let font = FontId::proportional(14.0);
    let galley = ui.painter().layout_no_wrap(text, font, TEXT);
    let padding = ui.spacing().button_padding;
    let size = galley.size() + padding * 2.0;
    let size = Vec2::new(size.x, size.y.max(ui.spacing().interact_size.y));
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let hovered = response.hovered() && ui.is_enabled();
        let pressed = response.is_pointer_button_down_on();
        if on || hovered || pressed {
            hot_track(painter, rect, on || pressed);
        }
        let colour = if !ui.is_enabled() {
            MUTED.gamma_multiply(0.7)
        } else if on {
            ACCENT_DEEP
        } else {
            TEXT
        };
        painter.galley(rect.center() - galley.size() / 2.0, galley, colour);
    }
    response
}

/// The pale blue gloss a row or toolbar button shows under the pointer
/// (`strong` for a chosen one).
pub fn hot_track(painter: &egui::Painter, rect: Rect, strong: bool) {
    let (top, bottom, rim) = if strong {
        (Color32::from_rgb(220, 238, 252), Color32::from_rgb(178, 216, 246), Color32::from_rgb(102, 167, 232))
    } else {
        (Color32::from_rgb(244, 250, 255), Color32::from_rgb(216, 237, 252), Color32::from_rgb(158, 205, 242))
    };
    gradient(painter, rect, 3.0, &[(0.0, top), (1.0, bottom)]);
    painter.rect_stroke(
        rect.shrink(1.0),
        CornerRadius::same(2),
        Stroke::new(1.0, see_through(255, 255, 255, 150)),
        egui::StrokeKind::Inside,
    );
    painter.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0, rim), egui::StrokeKind::Inside);
}

/// A tab along the home screen's glass bar: a bright glass pill while
/// it's the open one, a faint one under the pointer.
pub fn tab(ui: &mut egui::Ui, glyph: &str, label: &str, on: bool) -> Response {
    let text = if label.is_empty() { glyph.to_owned() } else { format!("{glyph}  {label}") };
    tab_with(ui, &text, on, label.is_empty())
}

/// How wide [`tab`] draws a tab, so a bar can tell whether its tabs fit.
pub fn tab_width(ui: &egui::Ui, glyph: &str, label: &str) -> f32 {
    let text = if label.is_empty() { glyph.to_owned() } else { format!("{glyph}  {label}") };
    let (size, padding) = if label.is_empty() { (17.0, 22.0) } else { (15.0, 30.0) };
    (ui.painter().layout_no_wrap(text, display(size), TEXT).size().x + padding).max(40.0)
}

fn tab_with(ui: &mut egui::Ui, text: &str, on: bool, icon_only: bool) -> Response {
    let font = display(if icon_only { 17.0 } else { 15.0 });
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, TEXT);
    let padding = if icon_only { 22.0 } else { 30.0 };
    let size = Vec2::new((galley.size().x + padding).max(40.0), 40.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    let pill = rect.shrink2(Vec2::new(1.0, 3.0));
    if on {
        let shadow =
            egui::Shadow { offset: [0, 0], blur: 16, spread: 0, color: see_through(160, 220, 255, 200) };
        painter.add(shadow.as_shape(pill, CornerRadius::same(7)));
        glossy(
            painter,
            pill,
            7.0,
            (
                Color32::from_rgb(250, 253, 255),
                Color32::from_rgb(196, 228, 250),
                Color32::from_rgb(70, 130, 190),
            ),
            false,
            false,
        );
    } else if response.hovered() {
        glass(painter, pill, 7.0, Color32::from_rgba_unmultiplied(220, 240, 255, 110));
    }
    let colour = if on { HEADING } else { Color32::from_rgb(12, 34, 62) };
    glow_text(painter, rect.center() - galley.size() / 2.0, galley, colour);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A page heading: large, light letters in deep blue.
pub fn heading(ui: &mut egui::Ui, text: &str) {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), light(27.0), HEADING);
    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    glow_text(ui.painter(), rect.min, galley, HEADING);
}

/// A smaller heading inside a page, in the heading blue.
pub fn label_caps(ui: &mut egui::Ui, text: &str) -> Response {
    ui.label(RichText::new(text).font(display(13.0)).color(HEADING))
}

/// A frosted pane to group things on.
pub fn panel() -> egui::Frame {
    egui::Frame::new()
        .fill(see_through(255, 255, 255, 228))
        .stroke(Stroke::new(1.0, Color32::from_rgb(160, 196, 230)))
        .corner_radius(CornerRadius::same(8))
        .shadow(egui::Shadow {
            offset: [0, 3],
            blur: 14,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(10, 50, 100, 40),
        })
        .inner_margin(egui::Margin::same(14))
}

/// Libroxel's mark: a glassy cube seen from above a corner, its three
/// faces lit differently, drawn to fill a square `size` points across at
/// `at`.
pub fn paint_logo(painter: &egui::Painter, at: Pos2, size: f32) {
    let c = at + Vec2::splat(size / 2.0);
    let r = size / 2.0;
    let (dx, dy) = (r * 0.866, r * 0.5);
    let top = Pos2::new(c.x, c.y - r);
    let left = Pos2::new(c.x - dx, c.y - dy);
    let right = Pos2::new(c.x + dx, c.y - dy);
    let bottom = Pos2::new(c.x, c.y + r);
    let low_left = Pos2::new(c.x - dx, c.y + dy);
    let low_right = Pos2::new(c.x + dx, c.y + dy);
    // Each face fades from one colour at one edge to another, like light
    // through tinted glass.
    let face = |points: [Pos2; 4], a: Color32, b: Color32| {
        let mut mesh = Mesh::default();
        mesh.colored_vertex(points[0], a);
        mesh.colored_vertex(points[1], lerp(a, b, 0.5));
        mesh.colored_vertex(points[2], b);
        mesh.colored_vertex(points[3], lerp(a, b, 0.5));
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(fixed(mesh));
    };
    face([top, right, c, left], Color32::from_rgb(214, 246, 255), Color32::from_rgb(124, 214, 250));
    face([left, c, bottom, low_left], Color32::from_rgb(64, 190, 240), Color32::from_rgb(24, 128, 214));
    face([c, right, low_right, bottom], Color32::from_rgb(40, 150, 222), Color32::from_rgb(14, 82, 168));
    // A notch out of the top face: one voxel taken away, in leafy green.
    let q = |a: Pos2, b: Pos2, t: f32| a + (b - a) * t;
    face(
        [top, q(top, right, 0.5), q(top, c, 0.5), q(top, left, 0.5)],
        Color32::from_rgb(190, 246, 140),
        Color32::from_rgb(92, 196, 62),
    );
    // A gloss streak across the upper left, and a thin bright rim.
    let gleam = [left, top, q(top, right, 0.35), q(left, c, 0.6)];
    painter.add(Shape::convex_polygon(gleam.to_vec(), see_through(255, 255, 255, 60), Stroke::NONE));
    let rim = Stroke::new((size * 0.035).max(1.0), see_through(255, 255, 255, 170));
    painter.add(Shape::closed_line(vec![top, right, low_right, bottom, low_left, left], rim));
    painter.line_segment([left, c], Stroke::new(1.0, see_through(255, 255, 255, 120)));
    painter.line_segment([c, right], Stroke::new(1.0, see_through(255, 255, 255, 120)));
    painter.line_segment([c, bottom], Stroke::new(1.0, see_through(255, 255, 255, 90)));
}

/// The Libroxel mark as a button, for Home: it sits in a glossy pill
/// like a picked tab while `on`, and lights up under the pointer.
pub fn logo_button(ui: &mut egui::Ui, on: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(52.0, 44.0), Sense::click());
    let painter = ui.painter();
    let pill = rect.shrink2(Vec2::new(1.0, 2.0));
    if on {
        let shadow =
            egui::Shadow { offset: [0, 0], blur: 16, spread: 0, color: see_through(160, 220, 255, 200) };
        painter.add(shadow.as_shape(pill, CornerRadius::same(9)));
        glossy(
            painter,
            pill,
            9.0,
            (
                Color32::from_rgb(250, 253, 255),
                Color32::from_rgb(196, 228, 250),
                Color32::from_rgb(70, 130, 190),
            ),
            false,
            false,
        );
    } else if response.hovered() {
        glass(painter, pill, 9.0, Color32::from_rgba_unmultiplied(220, 240, 255, 110));
    }
    let size = 32.0;
    paint_logo(painter, rect.center() - Vec2::splat(size / 2.0), size);
    response.on_hover_text("Home").on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A box for a join code or an address. What's typed is hidden like a
/// password until Show is pressed, so a stream or a screenshot doesn't give
/// an IP away.
pub fn hidden_field(ui: &mut egui::Ui, text: &mut String, hint: &str, width: f32) -> Response {
    let id = ui.next_auto_id().with("shown");
    let mut shown = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let response = ui
        .horizontal(|ui| {
            let field = ui
                .add(egui::TextEdit::singleline(text).hint_text(hint).password(!shown).desired_width(width));
            let (glyph, tip) =
                if shown { (icon::EYE_SLASH, "Hide it again") } else { (icon::EYE, "Show IP") };
            if ui.small_button(glyph).on_hover_text(tip).clicked() {
                shown = !shown;
            }
            field
        })
        .inner;
    ui.data_mut(|d| d.insert_temp(id, shown));
    response
}

/// Shows `text` (a join code, or something with an IP in it) as dots
/// until Show IP is pressed, with a Copy button that works either way.
pub fn hidden_text(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    text: &str,
) {
    let id = ui.id().with(("hidden", id_salt));
    let mut shown = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    ui.horizontal(|ui| {
        ui.label(label);
        if shown {
            let short: String = text.chars().take(28).collect();
            let more = if text.chars().count() > 28 { "…" } else { "" };
            ui.label(RichText::new(format!("{short}{more}")).monospace());
        } else {
            ui.label(RichText::new("••••••••••••").monospace().color(MUTED));
        }
        let (word, tip) = if shown { ("Hide", "Hide it again") } else { ("Show IP", "Show it on screen") };
        if ui.small_button(word).on_hover_text(tip).clicked() {
            shown = !shown;
        }
        if ui.small_button(format!("{}  Copy", icon::COPY)).clicked() {
            ui.ctx().copy_text(text.to_owned());
        }
    });
    ui.data_mut(|d| d.insert_temp(id, shown));
}

/// A glossy little orb, for status lights and badges.
pub fn orb(painter: &egui::Painter, centre: Pos2, radius: f32, colour: Color32) {
    radial(
        painter,
        centre + Vec2::new(0.0, radius * 0.25),
        radius,
        lerp(colour, Color32::WHITE, 0.35),
        colour,
    );
    painter.circle_stroke(centre, radius, Stroke::new(1.0, colour.gamma_multiply(0.6).to_opaque()));
    let gleam = Rect::from_center_size(
        centre - Vec2::new(0.0, radius * 0.42),
        Vec2::new(radius * 1.2, radius * 0.75),
    );
    gradient(
        painter,
        gleam,
        radius * 0.38,
        &[
            (0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 200)),
            (1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 30)),
        ],
    );
}

/// A status light with a word beside it, for connection states.
pub fn status(ui: &mut egui::Ui, colour: Color32, text: &str) -> Response {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
        orb(ui.painter(), rect.center(), 5.5, colour);
        ui.label(RichText::new(text).color(lerp(colour, TEXT, 0.35)))
    })
    .inner
}

// --- Icons for classes -----------------------------------------------------

/// The icon and colour a class shows with in the explorer and menus. The
/// colour says what kind of thing it is: orange for things in the world,
/// green for code, blue for screens and text, amber for light and
/// effects, violet for physics, grey for containers and services.
pub fn class_glyph(class: ClassKind) -> (&'static str, Color32) {
    use ClassKind as C;
    const VIOLET: Color32 = Color32::from_rgb(128, 84, 210);
    let world = ORANGE;
    let code = GREEN;
    let screen = LINK;
    let glow = AMBER;
    let quiet = MUTED;
    match class {
        C::DataModel => (icon::GAME_CONTROLLER, HEADING),
        C::Workspace => (icon::GLOBE_HEMISPHERE_WEST, ACCENT),
        C::Part | C::SpawnLocation => (icon::CUBE, world),
        C::Model => (icon::CUBE_FOCUS, world),
        C::Terrain => (icon::MOUNTAINS, GREEN_DEEP),
        C::Camera => (icon::VIDEO_CAMERA, quiet),
        C::Folder | C::Configuration => (icon::FOLDER_SIMPLE, AMBER),
        C::Script => (icon::CODE, code),
        C::LocalScript => (icon::CODE_SIMPLE, code),
        C::ModuleScript => (icon::BRACKETS_CURLY, code),
        C::Players | C::Teams => (icon::USERS_THREE, quiet),
        C::Player => (icon::USER, TEXT),
        C::Team => (icon::FLAG_BANNER, quiet),
        C::Humanoid => (icon::PERSON_SIMPLE, world),
        C::Lighting => (icon::SUN, glow),
        C::ReplicatedStorage | C::ServerStorage => (icon::ARCHIVE, quiet),
        C::ServerScriptService => (icon::GEAR_SIX, quiet),
        C::StarterGui | C::PlayerGui => (icon::BROWSERS, screen),
        C::StarterPack | C::Backpack => (icon::BACKPACK, quiet),
        C::StarterPlayer => (icon::USER_CIRCLE, quiet),
        C::StarterPlayerScripts | C::PlayerScripts => (icon::FOLDER_USER, code),
        C::StarterCharacterScripts => (icon::FOLDER_SIMPLE_USER, code),
        C::ForceField => (icon::SHIELD, glow),
        C::ScreenGui | C::SurfaceGui => (icon::APP_WINDOW, screen),
        C::BillboardGui => (icon::SIGNPOST, screen),
        C::Frame | C::ScrollingFrame => (icon::SQUARE, screen),
        C::TextLabel => (icon::TEXT_T, screen),
        C::TextButton | C::ImageButton => (icon::CURSOR_CLICK, screen),
        C::TextBox => (icon::TEXTBOX, screen),
        C::ImageLabel | C::Decal | C::Texture => (icon::IMAGE, screen),
        C::Tool => (icon::HAMMER, world),
        C::ClickDetector => (icon::HAND_POINTING, world),
        C::ProximityPrompt => (icon::HAND_TAP, world),
        C::PointLight | C::SpotLight | C::SurfaceLight => (icon::LIGHTBULB, glow),
        C::Sound => (icon::SPEAKER_HIGH, glow),
        C::ParticleEmitter | C::Sparkles => (icon::SPARKLE, glow),
        C::Fire => (icon::FIRE, glow),
        C::Smoke => (icon::CLOUD, glow),
        C::Explosion => (icon::SEAL_WARNING, RED),
        C::Highlight | C::SelectionBox => (icon::SELECTION, glow),
        C::Beam | C::Trail => (icon::LIGHTNING, glow),
        C::Sky => (icon::MOON_STARS, glow),
        C::Atmosphere => (icon::CLOUD_FOG, glow),
        C::BloomEffect
        | C::ColorCorrectionEffect
        | C::BlurEffect
        | C::SunRaysEffect
        | C::DepthOfFieldEffect => (icon::APERTURE, glow),
        C::RemoteEvent | C::RemoteFunction => (icon::BROADCAST, code),
        C::BindableEvent | C::BindableFunction => (icon::PLUGS_CONNECTED, code),
        C::Attachment => (icon::CROSSHAIR_SIMPLE, VIOLET),
        C::Weld | C::WeldConstraint | C::Motor6D => (icon::LINK_SIMPLE, VIOLET),
        C::HingeConstraint => (icon::ARROWS_CLOCKWISE, VIOLET),
        C::RopeConstraint | C::SpringConstraint | C::RodConstraint => (icon::LINE_SEGMENT, VIOLET),
        C::NoCollisionConstraint => (icon::GHOST, VIOLET),
        C::UICorner
        | C::UIListLayout
        | C::UIGridLayout
        | C::UIPadding
        | C::UIStroke
        | C::UIScale
        | C::UIAspectRatioConstraint
        | C::UISizeConstraint
        | C::UITextSizeConstraint
        | C::UIGradient => (icon::RULER, screen),
        C::IntValue
        | C::NumberValue
        | C::StringValue
        | C::BoolValue
        | C::ObjectValue
        | C::Vector3Value
        | C::Color3Value
        | C::CFrameValue
        | C::BrickColorValue => (icon::HASH, code),
        C::Animation | C::Animator => (icon::FILM_STRIP, world),
        c if c.is_a(C::Part) || c.is_physical() => (icon::CUBE, world),
        _ if is_mover(class) => (icon::WIND, VIOLET),
        _ => (icon::DOT_OUTLINE, quiet),
    }
}

fn is_mover(class: ClassKind) -> bool {
    use ClassKind as C;
    matches!(
        class,
        C::LinearVelocity
            | C::VectorForce
            | C::AlignPosition
            | C::AlignOrientation
            | C::AngularVelocity
            | C::BodyVelocity
            | C::BodyPosition
            | C::BodyForce
            | C::BodyAngularVelocity
            | C::BodyGyro
            | C::Torque
            | C::PrismaticConstraint
            | C::BallSocketConstraint
            | C::CylindricalConstraint
    )
}

/// A class's icon in its colour, then `name`, as one line of text.
pub fn class_label(class: ClassKind, name: &str) -> egui::text::LayoutJob {
    let (glyph, colour) = class_glyph(class);
    let mut job = egui::text::LayoutJob::default();
    let format =
        |font: FontId, color: Color32| egui::TextFormat { font_id: font, color, ..Default::default() };
    job.append(glyph, 0.0, format(FontId::proportional(14.0), colour));
    job.append(name, 7.0, format(FontId::proportional(14.0), TEXT));
    job
}

/// Draws one line of `text` from `at`, cut short with "…" if it would run
/// past `width`.
pub fn text_line(painter: &egui::Painter, at: Pos2, text: &str, font: FontId, colour: Color32, width: f32) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, colour);
    job.wrap = egui::text::TextWrapping {
        max_width: width,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = painter.layout_job(job);
    painter.galley(at, galley, colour);
}

/// The title along the top of an editor panel: a pale blue band with its
/// icon and name, like the header of a pane in a file window.
pub fn panel_title(ui: &mut egui::Ui, glyph: &str, text: &str) {
    ui.add_space(2.0);
    let width = ui.available_width();
    let top = ui.cursor().min;
    let band = Rect::from_min_size(top - Vec2::new(4.0, 2.0), Vec2::new(width + 8.0, 26.0));
    gradient(
        ui.painter(),
        band,
        4.0,
        &[(0.0, Color32::from_rgb(250, 252, 255)), (1.0, Color32::from_rgb(220, 234, 248))],
    );
    ui.painter().rect_stroke(band, CornerRadius::same(4), Stroke::new(1.0, LINE), egui::StrokeKind::Inside);
    ui.horizontal(|ui| {
        ui.set_min_height(22.0);
        panel_name(ui, glyph, text)
    });
    ui.add_space(6.0);
}

/// A panel's icon and name on one line, for rows that hold more after it.
pub fn panel_name(ui: &mut egui::Ui, glyph: &str, text: &str) {
    ui.label(RichText::new(glyph).color(ACCENT).size(14.0));
    ui.label(RichText::new(text).font(display(14.0)).color(HEADING));
}

/// How tall a ribbon button is (icon over label).
pub const RIBBON_HEIGHT: f32 = 50.0;

/// The ribbon's pale blue backdrop over `rect`, as a shape to put under
/// its buttons.
pub fn ribbon_back(rect: Rect) -> Shape {
    let fill = gradient_shape(
        rect,
        4.0,
        &[
            (0.0, Color32::from_rgb(253, 254, 255)),
            (0.55, Color32::from_rgb(236, 243, 251)),
            (0.85, Color32::from_rgb(222, 234, 247)),
            (1.0, Color32::from_rgb(210, 226, 244)),
        ],
    );
    let rim = Shape::rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0, Color32::from_rgb(160, 188, 222)),
        egui::StrokeKind::Inside,
    );
    Shape::Vec(vec![fill, rim])
}

/// A big ribbon button: its icon over its name, lit while `on`.
pub fn ribbon_button(ui: &mut egui::Ui, glyph: &str, label: &str, on: bool) -> Response {
    let enabled = ui.is_enabled();
    let name = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(12.0), TEXT);
    let width = name.size().x.max(30.0) + 14.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, RIBBON_HEIGHT), Sense::click());
    let painter = ui.painter();
    let hovered = response.hovered() && enabled;
    if on || hovered || (response.is_pointer_button_down_on() && enabled) {
        hot_track(painter, rect, on || response.is_pointer_button_down_on());
    }
    let ink = if !enabled { MUTED.gamma_multiply(0.6) } else { TEXT };
    let glyph_colour = if !enabled {
        ink
    } else if on {
        ACCENT_DEEP
    } else {
        ACCENT
    };
    painter.text(
        Pos2::new(rect.center().x, rect.min.y + 17.0),
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(22.0),
        glyph_colour,
    );
    painter.text(
        Pos2::new(rect.center().x, rect.max.y - 9.0),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(12.0),
        ink,
    );
    if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }
}

/// A group of ribbon buttons with its name under them and a divider after.
pub fn ribbon_group<R>(ui: &mut egui::Ui, caption: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let inner = ui
        .vertical(|ui| {
            let r = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.set_min_height(RIBBON_HEIGHT);
                add(ui)
            });
            ui.add_space(1.0);
            let width = ui.min_rect().width();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 13.0), Sense::hover());
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                caption,
                FontId::proportional(11.0),
                Color32::from_rgb(62, 92, 130),
            );
            r.inner
        })
        .inner;
    let height = RIBBON_HEIGHT + 16.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(9.0, height), Sense::hover());
    // A groove: a blue-grey line beside a white one.
    let x = rect.center().x;
    let (y0, y1) = (rect.min.y + 4.0, rect.max.y - 2.0);
    ui.painter().line_segment(
        [Pos2::new(x, y0), Pos2::new(x, y1)],
        Stroke::new(1.0, Color32::from_rgb(170, 192, 218)),
    );
    ui.painter().line_segment(
        [Pos2::new(x + 1.0, y0), Pos2::new(x + 1.0, y1)],
        Stroke::new(1.0, see_through(255, 255, 255, 200)),
    );
    inner
}

/// A tab along the ribbon's top edge.
pub fn ribbon_tab(ui: &mut egui::Ui, label: &str, on: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(13.0), TEXT);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(galley.size().x + 22.0, 26.0), Sense::click());
    let painter = ui.painter();
    if on {
        let tab = rect.expand2(Vec2::new(0.0, 0.0));
        gradient(
            painter,
            tab,
            4.0,
            &[(0.0, Color32::from_rgb(255, 255, 255)), (1.0, Color32::from_rgb(244, 248, 254))],
        );
        painter.rect_stroke(
            tab,
            CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 },
            Stroke::new(1.0, Color32::from_rgb(150, 182, 220)),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        hot_track(painter, rect.shrink2(Vec2::new(0.0, 2.0)), false);
    }
    let colour = if on { HEADING } else { TEXT };
    painter.galley(rect.center() - galley.size() / 2.0, galley, colour);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A tile in the toolbox: an icon over a name.
pub fn toolbox_tile(ui: &mut egui::Ui, glyph: &str, colour: Color32, label: &str) -> Response {
    let size = Vec2::new(74.0, 66.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    if response.hovered() && ui.is_enabled() {
        hot_track(painter, rect, false);
    } else {
        gradient(painter, rect, 4.0, &[(0.0, Color32::WHITE), (1.0, Color32::from_rgb(240, 245, 251))]);
        painter.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, LINE), egui::StrokeKind::Inside);
    }
    painter.text(
        Pos2::new(rect.center().x, rect.min.y + 23.0),
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(24.0),
        if ui.is_enabled() { colour } else { MUTED },
    );
    text_line(
        painter,
        Pos2::new(rect.min.x + 5.0, rect.max.y - 19.0),
        label,
        FontId::proportional(12.0),
        TEXT,
        rect.width() - 10.0,
    );
    response
}
