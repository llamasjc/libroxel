//! Libroxel's look: one palette, one set of typefaces, one set of icons and
//! a few hand-drawn widgets, used by the home screen, the editor and the
//! in-game overlays alike.
//!
//! The idea is a builder's workbench at night: warm graphite surfaces,
//! chalk-coloured text, and a single tangerine accent, with near-square
//! corners and buttons that sit on a hard edge like a toy brick and press
//! down into it. Headings and buttons are set in Chakra Petch (squared,
//! technical), reading text in Atkinson Hyperlegible, code in IBM Plex
//! Mono. Icons are Phosphor's bold set, so they match each other and can
//! sit inside any line of text.

use bevy_egui::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Pos2, Rect, Response,
    RichText, Sense, Stroke, TextStyle, Vec2,
};
pub use egui_phosphor::bold as icon;
use libroxel_instance::ClassKind;

// --- Palette -------------------------------------------------------------

/// The window behind everything.
pub const INK0: Color32 = Color32::from_rgb(18, 17, 15);
/// Panels and bars.
pub const INK1: Color32 = Color32::from_rgb(28, 26, 23);
/// Cards and raised surfaces.
pub const INK2: Color32 = Color32::from_rgb(38, 35, 31);
/// Hovered surfaces and the wells text is typed into.
pub const INK3: Color32 = Color32::from_rgb(52, 48, 42);
/// Hairlines between things.
pub const LINE: Color32 = Color32::from_rgb(63, 58, 51);
/// Text.
pub const CHALK: Color32 = Color32::from_rgb(241, 235, 221);
/// Quieter text: hints, notes, labels.
pub const DUST: Color32 = Color32::from_rgb(163, 155, 139);
/// The one accent: what's chosen, what to press.
pub const TANGERINE: Color32 = Color32::from_rgb(255, 106, 43);
/// The accent's edge and pressed face.
pub const EMBER: Color32 = Color32::from_rgb(176, 64, 22);
/// Going: play, joined, done.
pub const MINT: Color32 = Color32::from_rgb(47, 212, 154);
pub const MINT_EDGE: Color32 = Color32::from_rgb(22, 128, 92);
/// Links and GUI things.
pub const SKY: Color32 = Color32::from_rgb(89, 168, 255);
/// Stopping, deleting, errors.
pub const BRICK: Color32 = Color32::from_rgb(242, 84, 91);
/// Warnings, coins, lights.
pub const SUN: Color32 = Color32::from_rgb(255, 201, 60);

/// The squared corner everything shares.
pub const CORNER: u8 = 3;

// --- Type ------------------------------------------------------------------

const DISPLAY: &str = "display";
const DISPLAY_BOLD: &str = "display-bold";

/// The squared typeface for headings, tabs and buttons.
pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}

/// The heaviest cut of the display face, for the name and big numbers.
pub fn display_bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY_BOLD.into()))
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.to_owned(), std::sync::Arc::new(FontData::from_static(bytes)));
    };
    add("chakra-semibold", include_bytes!("../assets/fonts/ChakraPetch-SemiBold.ttf"));
    add("chakra-bold", include_bytes!("../assets/fonts/ChakraPetch-Bold.ttf"));
    add("atkinson", include_bytes!("../assets/fonts/AtkinsonHyperlegible-Regular.ttf"));
    add("plex-mono", include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"));
    add("phosphor", egui_phosphor::variants::bytes::bold::FONT);
    // Each family falls back to the icons, then to egui's own fonts for
    // anything else (arrows, accents, other scripts).
    let fallback: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let family = |first: &str| {
        let mut list = vec![first.to_owned(), "phosphor".to_owned()];
        list.extend(fallback.iter().cloned());
        list
    };
    let proportional = family("atkinson");
    let mut mono = vec!["plex-mono".to_owned(), "phosphor".to_owned()];
    mono.extend(fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default());
    fonts.families.insert(FontFamily::Proportional, proportional);
    fonts.families.insert(FontFamily::Monospace, mono);
    fonts.families.insert(FontFamily::Name(DISPLAY.into()), family("chakra-semibold"));
    fonts.families.insert(FontFamily::Name(DISPLAY_BOLD.into()), family("chakra-bold"));
    fonts
}

/// Puts Libroxel's fonts and colours on `ctx`, once. New fonts only take
/// effect from the next frame, so this says whether they can be used yet:
/// draw nothing until it does.
pub fn install(ctx: &egui::Context) -> bool {
    let id = egui::Id::new("libroxel theme installed");
    if ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false) {
        return ctx.fonts(|f| f.families().contains(&FontFamily::Name(DISPLAY_BOLD.into())));
    }
    ctx.data_mut(|d| d.insert_temp(id, true));
    ctx.set_fonts(fonts());
    ctx.all_styles_mut(style);
    false
}

fn style(style: &mut egui::Style) {
    use egui::style::{Selection, WidgetVisuals, Widgets};
    style.text_styles = [
        (TextStyle::Heading, display(22.0)),
        (TextStyle::Body, FontId::proportional(14.0)),
        (TextStyle::Button, display(14.0)),
        (TextStyle::Small, FontId::proportional(11.5)),
        (TextStyle::Monospace, FontId::monospace(13.0)),
    ]
    .into();
    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(8.0, 6.0);
    s.button_padding = Vec2::new(10.0, 4.0);
    s.menu_margin = egui::Margin::same(6);
    s.window_margin = egui::Margin::same(12);
    s.interact_size = Vec2::new(36.0, 24.0);
    s.indent = 16.0;
    s.scroll.bar_width = 8.0;
    s.scroll.floating = true;

    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    let corner = CornerRadius::same(CORNER);
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
            bg_fill: INK1,
            weak_bg_fill: INK1,
            bg_stroke: Stroke::new(1.0, LINE),
            corner_radius: corner,
            fg_stroke: Stroke::new(1.0, CHALK),
            expansion: 0.0,
        },
        inactive: look(INK2, LINE, CHALK),
        hovered: look(INK3, DUST, Color32::WHITE),
        active: look(EMBER, TANGERINE, Color32::WHITE),
        open: look(INK3, TANGERINE, Color32::WHITE),
    };
    v.selection = Selection { bg_fill: TANGERINE.gamma_multiply(0.35), stroke: Stroke::new(1.5, TANGERINE) };
    v.hyperlink_color = SKY;
    v.faint_bg_color = INK2;
    v.extreme_bg_color = INK0;
    v.code_bg_color = INK0;
    v.warn_fg_color = SUN;
    v.error_fg_color = BRICK;
    v.window_fill = INK1;
    v.window_stroke = Stroke::new(1.0, LINE);
    v.window_corner_radius = CornerRadius::same(CORNER + 1);
    // A hard shadow, offset like a block sitting on a table.
    v.window_shadow =
        egui::Shadow { offset: [5, 5], blur: 0, spread: 0, color: Color32::from_black_alpha(110) };
    v.popup_shadow =
        egui::Shadow { offset: [3, 3], blur: 0, spread: 0, color: Color32::from_black_alpha(110) };
    v.menu_corner_radius = CornerRadius::same(CORNER);
    v.panel_fill = INK1;
    v.striped = false;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.6 };
    v.text_cursor.stroke = Stroke::new(2.0, TANGERINE);
}

// --- Widgets ---------------------------------------------------------------

/// What a block button is for, which sets its colour.
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
    fn colours(self) -> (Color32, Color32, Color32) {
        match self {
            Kind::Primary => (TANGERINE, EMBER, INK0),
            Kind::Go => (MINT, MINT_EDGE, INK0),
            Kind::Plain => (INK3, Color32::from_rgb(24, 22, 19), CHALK),
            Kind::Danger => (BRICK, Color32::from_rgb(140, 36, 42), INK0),
        }
    }
}

/// How far a block button stands on its edge.
const EDGE: f32 = 4.0;

/// A button shaped like a brick: a face standing on a darker edge, which
/// presses down when held. `glyph` is an icon from [`icon`] (or "").
pub fn block_button(ui: &mut egui::Ui, kind: Kind, glyph: &str, label: &str, height: f32) -> Response {
    let size = height * 0.42;
    let font = display(size);
    let text = if glyph.is_empty() { label.to_owned() } else { format!("{glyph}  {label}") };
    let width =
        ui.painter().layout_no_wrap(text.clone(), font.clone(), Color32::WHITE).size().x + height * 0.9;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height + EDGE), Sense::click());
    let enabled = ui.is_enabled();
    let (face, edge, ink) = kind.colours();
    let (face, edge, ink) = if enabled {
        (face, edge, ink)
    } else {
        (INK2, Color32::from_rgb(24, 22, 19), DUST.gamma_multiply(0.7))
    };
    let pressed = response.is_pointer_button_down_on() && enabled;
    let sink = if pressed { EDGE - 1.0 } else { 0.0 };
    let face_rect = Rect::from_min_size(rect.min + Vec2::new(0.0, sink), Vec2::new(width, height));
    let painter = ui.painter();
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(rect.min.x, rect.min.y + EDGE), rect.max),
        CornerRadius::same(CORNER),
        edge,
    );
    let lit = if response.hovered() && enabled && !pressed {
        face.lerp_to_gamma(Color32::WHITE, 0.12)
    } else {
        face
    };
    painter.rect_filled(face_rect, CornerRadius::same(CORNER), lit);
    // A thin highlight along the top, as light catches a brick's edge.
    painter.line_segment(
        [face_rect.left_top() + Vec2::new(3.0, 1.0), face_rect.right_top() + Vec2::new(-3.0, 1.0)],
        Stroke::new(1.0, Color32::from_white_alpha(if kind == Kind::Plain { 18 } else { 60 })),
    );
    let galley = painter.layout_no_wrap(text, font, ink);
    painter.galley(face_rect.center() - galley.size() / 2.0, galley, ink);
    if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }
}

/// A small flat button with an icon, for toolbars and rows.
pub fn tool_button(ui: &mut egui::Ui, glyph: &str, label: &str, on: bool) -> Response {
    let text = if label.is_empty() { glyph.to_owned() } else { format!("{glyph} {label}") };
    let colour = if on { TANGERINE } else { CHALK };
    let button = egui::Button::new(RichText::new(text).color(colour))
        .fill(if on { TANGERINE.gamma_multiply(0.16) } else { Color32::TRANSPARENT })
        .stroke(if on { Stroke::new(1.0, TANGERINE.gamma_multiply(0.7)) } else { Stroke::NONE })
        .corner_radius(CornerRadius::same(CORNER));
    ui.add(button)
}

/// A tab along the home screen's bar: its name, with an accent block
/// under it while it's the open one.
pub fn tab(ui: &mut egui::Ui, glyph: &str, label: &str, on: bool) -> Response {
    let font = display(16.0);
    let galley = ui.painter().layout_no_wrap(format!("{glyph}  {}", label.to_uppercase()), font, CHALK);
    let size = Vec2::new(galley.size().x + 28.0, 40.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let colour = if on {
        CHALK
    } else if response.hovered() {
        Color32::WHITE
    } else {
        DUST
    };
    if response.hovered() && !on {
        ui.painter().rect_filled(rect, CornerRadius::same(CORNER), INK2);
    }
    ui.painter().galley(rect.center() - galley.size() / 2.0 - Vec2::new(0.0, 2.0), galley, colour);
    if on {
        let bar = Rect::from_min_size(
            Pos2::new(rect.min.x + 10.0, rect.max.y - 4.0),
            Vec2::new(rect.width() - 20.0, 4.0),
        );
        ui.painter().rect_filled(bar, CornerRadius::ZERO, TANGERINE);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A page heading: big squared letters, with a short accent bar before them.
pub fn heading(ui: &mut egui::Ui, text: &str) {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), display(24.0), CHALK);
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(galley.size().x + 18.0, galley.size().y), Sense::hover());
    let mark = Rect::from_min_size(Pos2::new(rect.min.x, rect.center().y - 7.0), Vec2::new(6.0, 14.0));
    ui.painter().rect_filled(mark, CornerRadius::ZERO, TANGERINE);
    ui.painter().galley(Pos2::new(rect.min.x + 18.0, rect.min.y), galley, CHALK);
}

/// A smaller heading inside a page, in spaced capitals.
pub fn label_caps(ui: &mut egui::Ui, text: &str) -> Response {
    ui.label(RichText::new(spaced(&text.to_uppercase())).font(display(12.5)).color(DUST))
}

/// Letters with thin gaps between them, for small capitals.
fn spaced(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for (i, c) in text.chars().enumerate() {
        if i > 0 && c != ' ' {
            out.push('\u{200A}');
        }
        out.push(c);
    }
    out
}

/// A raised panel to group things on.
pub fn panel() -> egui::Frame {
    egui::Frame::new()
        .fill(INK2)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(CORNER))
        .inner_margin(egui::Margin::same(14))
}

/// Libroxel's mark: a cube seen from above a corner, its three faces lit
/// differently, drawn to fill a square `size` points across at `at`.
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
    let face = |points: Vec<Pos2>, colour: Color32| {
        painter.add(egui::Shape::convex_polygon(points, colour, Stroke::NONE));
    };
    face(vec![top, right, c, left], Color32::from_rgb(255, 160, 110));
    face(vec![left, c, bottom, low_left], TANGERINE);
    face(vec![c, right, low_right, bottom], EMBER);
    // A notch out of the top face: one voxel taken away.
    let q = |a: Pos2, b: Pos2, t: f32| a + (b - a) * t;
    let notch = vec![top, q(top, right, 0.5), q(top, c, 0.5), q(top, left, 0.5)];
    face(notch, INK0.gamma_multiply(0.0).lerp_to_gamma(EMBER, 1.0));
}

/// The mark and the name side by side.
pub fn wordmark(ui: &mut egui::Ui, height: f32) -> Response {
    let galley = ui.painter().layout_no_wrap(spaced("LIBROXEL"), display_bold(height * 0.62), CHALK);
    let size = Vec2::new(height + 10.0 + galley.size().x, height);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    paint_logo(ui.painter(), rect.min, height);
    ui.painter().galley(
        Pos2::new(rect.min.x + height + 10.0, rect.center().y - galley.size().y / 2.0),
        galley,
        CHALK,
    );
    response
}

/// A faint grid of dots over `rect`, offset every other row like the
/// corners of cubes seen from above.
pub fn paint_dot_grid(painter: &egui::Painter, rect: Rect) {
    let step = 26.0;
    let colour = Color32::from_rgba_unmultiplied(241, 235, 221, 10);
    let mut row = 0;
    let mut y = rect.min.y + step / 2.0;
    while y < rect.max.y {
        let shift = if row % 2 == 0 { 0.0 } else { step / 2.0 };
        let mut x = rect.min.x + step / 2.0 + shift;
        while x < rect.max.x {
            painter.rect_filled(Rect::from_center_size(Pos2::new(x, y), Vec2::splat(2.0)), 0.0, colour);
            x += step;
        }
        y += step * 0.58;
        row += 1;
    }
}

/// A status dot with a word beside it, for connection states.
pub fn status(ui: &mut egui::Ui, colour: Color32, text: &str) -> Response {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
        ui.painter().rect_filled(rect.shrink(1.0), CornerRadius::same(1), colour);
        ui.label(RichText::new(text).color(colour.lerp_to_gamma(CHALK, 0.4)))
    })
    .inner
}

// --- Icons for classes -----------------------------------------------------

/// The icon and colour a class shows with in the explorer and menus. The
/// colour says what kind of thing it is: tangerine for things in the
/// world, mint for code, sky for screens and text, sun for light and
/// effects, lilac for physics, dust for containers and services.
pub fn class_glyph(class: ClassKind) -> (&'static str, Color32) {
    use ClassKind as C;
    const LILAC: Color32 = Color32::from_rgb(178, 146, 255);
    let world = TANGERINE;
    let code = MINT;
    let screen = SKY;
    let glow = SUN;
    let quiet = DUST;
    match class {
        C::DataModel => (icon::GAME_CONTROLLER, CHALK),
        C::Workspace => (icon::GLOBE_HEMISPHERE_WEST, world),
        C::Part | C::SpawnLocation => (icon::CUBE, world),
        C::Model => (icon::CUBE_FOCUS, world),
        C::Terrain => (icon::MOUNTAINS, world),
        C::Camera => (icon::VIDEO_CAMERA, quiet),
        C::Folder | C::Configuration => (icon::FOLDER_SIMPLE, quiet),
        C::Script => (icon::CODE, code),
        C::LocalScript => (icon::CODE_SIMPLE, code),
        C::ModuleScript => (icon::BRACKETS_CURLY, code),
        C::Players | C::Teams => (icon::USERS_THREE, quiet),
        C::Player => (icon::USER, CHALK),
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
        C::Explosion => (icon::SEAL_WARNING, BRICK),
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
        C::Attachment => (icon::CROSSHAIR_SIMPLE, LILAC),
        C::Weld | C::WeldConstraint | C::Motor6D => (icon::LINK_SIMPLE, LILAC),
        C::HingeConstraint => (icon::ARROWS_CLOCKWISE, LILAC),
        C::RopeConstraint | C::SpringConstraint | C::RodConstraint => (icon::LINE_SEGMENT, LILAC),
        C::NoCollisionConstraint => (icon::GHOST, LILAC),
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
        _ if is_mover(class) => (icon::WIND, LILAC),
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
    job.append(name, 7.0, format(FontId::proportional(14.0), CHALK));
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

/// The title along the top of an editor panel: its icon in the accent
/// colour and its name in small capitals, over a hairline.
pub fn panel_title(ui: &mut egui::Ui, glyph: &str, text: &str) {
    ui.add_space(2.0);
    ui.horizontal(|ui| panel_name(ui, glyph, text));
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, LINE);
    ui.add_space(4.0);
}

/// A panel's icon and name on one line, for rows that hold more after it.
pub fn panel_name(ui: &mut egui::Ui, glyph: &str, text: &str) {
    ui.label(RichText::new(glyph).color(TANGERINE).size(14.0));
    ui.label(RichText::new(spaced(&text.to_uppercase())).font(display(13.5)).color(CHALK));
}
