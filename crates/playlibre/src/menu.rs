//! The home screen: what PlayLibre shows when it starts, like a game
//! launcher. **Play** lists the games on this computer; **Online** finds
//! and joins games other people host; **Create** opens the editor on a new
//! or saved game; **Host** shares a game, runs a server in the background
//! and turns this computer's Hub on or off.

use std::path::PathBuf;

use bevy_egui::egui::{self, Color32, RichText};

use crate::browse::Platform;
use crate::i18n::{t, tf};
use crate::server::data_dir;
use crate::{games_dir, theme};
use playlibre_engine::character::{Hat, Look};
use playlibre_instance::Color3;

/// Which game the menu's buttons use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Game {
    /// One of the starter games, by name.
    Starter(&'static str),
    Blank,
    File(PathBuf),
}

impl Game {
    pub fn label(&self) -> String {
        match self {
            Game::Starter(name) => t(name).into(),
            Game::Blank => t("New empty world").into(),
            Game::File(path) => path
                .file_stem()
                .map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned()),
        }
    }

    /// What the game's picture is made from: the id the Hub lists a
    /// starter game under, or the file.
    pub fn id(&self) -> String {
        match self {
            Game::Starter(name) => crate::social::starter_id(name),
            Game::Blank => "blank".into(),
            Game::File(path) => path.display().to_string(),
        }
    }
}

/// What a menu button asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Edit(Game),
    Play(Game),
    /// Play it, and let others join.
    Host(Game),
    Join {
        code: String,
        name: String,
    },
    Browse,
}

/// The home screen's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Home,
    Search,
    Friends,
    Groups,
    Create,
    Avatar,
    Market,
    Messages,
    Charts,
    Profile,
}

/// What the home screen remembers.
pub struct Menu {
    pub open: bool,
    pub tab: Tab,
    /// The game picked on the Play and Host tabs.
    pub game: Game,
    /// Saved games found in the games folder, newest first.
    games: Vec<PathBuf>,
    pub scanned: bool,
    /// A path typed in by hand.
    pub path: String,
    pub name: String,
    pub error: Option<String>,
    /// How the player looks, read from the avatar file the first time it's needed.
    look: Option<Look>,
    /// The computer's "open a file" window, when it's showing.
    picker: crate::file_picker::Picker,
    /// What the Home and Friends tabs remember.
    pub home: crate::hub_home::HomeState,
    pub groups: crate::groups::Groups,
    my_games: crate::my_games::MyGames,
    shown: Option<(Tab, f64)>,
    donated: Option<f64>,
    part: Part,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Part {
    #[default]
    Body,
    Hats,
    Shirts,
    Trousers,
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            open: false,
            tab: Tab::Home,
            shown: None,
            donated: None,
            part: Part::Body,
            game: Game::Blank,
            games: Vec::new(),
            scanned: false,
            path: String::new(),
            name: "Player".into(),
            error: None,
            look: None,
            picker: crate::file_picker::Picker::default(),
            home: crate::hub_home::HomeState::default(),
            groups: crate::groups::Groups::default(),
            my_games: crate::my_games::MyGames::default(),
        }
    }
}

/// The saved games in the games folder (and one folder down), newest first.
pub fn saved_games() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let look = |dir: PathBuf, depth: u32, found: &mut Vec<(std::time::SystemTime, PathBuf)>| {
        let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
        let mut deeper = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && depth == 0 {
                deeper.push(path);
            } else if crate::open::is_game_file(&path) {
                let when = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                found.push((when, path));
            }
        }
        deeper
    };
    let deeper = look(games_dir(), 0, &mut found);
    for dir in deeper {
        look(dir, 1, &mut found);
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.into_iter().map(|(_, p)| p).collect()
}

const ACCENT: Color32 = theme::ACCENT;
const SOFT: Color32 = theme::MUTED;

/// A game's picture, worked out from its id: a cluster of cubes on a 4×4
/// patch of grass, seen from above a corner. Each column's height comes
/// from the id and the cluster is mirrored along the diagonal, so it looks
/// the same on the left and the right. Returns the heights (row by row)
/// and the cubes' one colour. The website makes the same picture for the
/// same id (`blocks` in web/app.js), so the two must stay in step.
pub(crate) fn blocks(id: &str) -> ([u8; 16], Color32) {
    let mut h = id.bytes().fold(2_166_136_261u32, |h, b| (h ^ b as u32).wrapping_mul(16_777_619));
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    let mut heights = [0u8; 16];
    let mut bit = 0;
    for y in 0..4 {
        for x in y..4 {
            let v = (h >> bit & 3) as usize;
            bit += 2;
            let inner = (1..3).contains(&x) && (1..3).contains(&y);
            let high = if inner { [1, 2, 2, 3][v] } else { [0, 0, 1, 2][v] };
            heights[y * 4 + x] = high;
            heights[x * 4 + y] = high;
        }
    }
    (heights, hsl((h >> 20) as f64 % 360.0, 0.6, 0.55))
}

/// The colour of a game's [`blocks`].
pub(crate) fn picture_colour(id: &str) -> Color32 {
    blocks(id).1
}

fn hsl(h: f64, s: f64, l: f64) -> Color32 {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let v = |n: f64| ((n + m) * 255.0).round() as u8;
    Color32::from_rgb(v(r), v(g), v(b))
}

/// A colour darkened to `k` of itself, for a cube's sides.
fn darker(c: Color32, k: f32) -> Color32 {
    let m = |v: u8| (v as f32 * k + 0.5).floor().min(255.0) as u8;
    Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
}

/// Paints a game's [`blocks`] over `rect`: a little sky, the grass and the
/// cubes, as big as fits in the middle.
pub(crate) fn paint_blocks(painter: &egui::Painter, rect: egui::Rect, id: &str) {
    let (heights, colour) = blocks(id);
    theme::paint_bubbles(&painter.with_clip_rect(rect.shrink(0.5)), rect);
    let unit = (rect.width() / 8.0).min(rect.height() / 7.5);
    let (dx, dy) = (unit * 0.866, unit * 0.5);
    let base = rect.center() + egui::vec2(0.0, unit * 0.6);
    let at =
        |x: f32, y: f32, z: f32| egui::pos2(base.x + (x - y) * dx, base.y + (x + y - 4.0) * dy - z * unit);
    let face = |points: [egui::Pos2; 4], fill: Color32| {
        painter.add(egui::Shape::convex_polygon(points.to_vec(), fill, egui::Stroke::NONE));
    };
    // The grass under the cubes.
    let (lo, hi) = (-0.25, 4.25);
    painter.add(egui::Shape::convex_polygon(
        vec![at(lo, lo, 0.0), at(hi, lo, 0.0), at(hi, hi, 0.0), at(lo, hi, 0.0)],
        Color32::from_rgb(146, 210, 104),
        egui::Stroke::new(1.0, Color32::from_rgb(92, 160, 64)),
    ));
    // Back to front, bottom to top.
    let (left, right) = (darker(colour, 0.78), darker(colour, 0.55));
    for sum in 0..7 {
        for y in 0..4 {
            let x = sum - y;
            if !(0..4).contains(&x) {
                continue;
            }
            let (fx, fy) = (x as f32, y as f32);
            for z in 0..heights[y as usize * 4 + x as usize] {
                let z = z as f32;
                face(
                    [
                        at(fx, fy + 1.0, z + 1.0),
                        at(fx + 1.0, fy + 1.0, z + 1.0),
                        at(fx + 1.0, fy + 1.0, z),
                        at(fx, fy + 1.0, z),
                    ],
                    left,
                );
                face(
                    [
                        at(fx + 1.0, fy, z + 1.0),
                        at(fx + 1.0, fy + 1.0, z + 1.0),
                        at(fx + 1.0, fy + 1.0, z),
                        at(fx + 1.0, fy, z),
                    ],
                    right,
                );
                face(
                    [
                        at(fx, fy, z + 1.0),
                        at(fx + 1.0, fy, z + 1.0),
                        at(fx + 1.0, fy + 1.0, z + 1.0),
                        at(fx, fy + 1.0, z + 1.0),
                    ],
                    colour,
                );
            }
        }
    }
}

/// A game's tile: its picture, its name and a line under it.
fn card(
    ui: &mut egui::Ui,
    symbol: &str,
    title: &str,
    id: &str,
    note: &str,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(196.0, 176.0), egui::Sense::click());
    let spoken_name = if note.is_empty() { title.to_owned() } else { format!("{title}. {note}") };
    theme::name_for_screen_readers(
        ui,
        &response,
        egui::WidgetType::SelectableLabel,
        &spoken_name,
        Some(selected),
    );
    let painter = ui.painter_at(rect.expand(20.0));
    let hovered = response.hovered();
    let lift = if hovered { -3.0 } else { 0.0 };
    // A frosted pane floating over the sky, lifting a little under the
    // pointer with its shadow spreading.
    let face = rect.translate(egui::vec2(0.0, lift));
    theme::soft_shadow(&painter, face, 8.0, if hovered { 80 } else { 50 });
    let colour = picture_colour(id);
    if hovered || selected {
        let glow = egui::Shadow {
            offset: [0, 0],
            blur: 14,
            spread: 1,
            color: theme::see_through(90, 180, 255, if selected { 200 } else { 130 }),
        };
        painter.add(glow.as_shape(face, 8));
    }
    theme::gradient(&painter, face, 8.0, &[(0.0, Color32::WHITE), (1.0, Color32::from_rgb(236, 244, 252))]);
    let picture = egui::Rect::from_min_size(face.min, egui::vec2(face.width(), 112.0)).shrink(4.0);
    paint_blocks(&painter, picture, id);
    painter.rect_stroke(
        picture,
        5,
        egui::Stroke::new(1.0, theme::see_through(30, 80, 140, 70)),
        egui::StrokeKind::Inside,
    );
    // The game's own icon on a glossy chip in a corner.
    let chip = egui::Rect::from_min_size(picture.min + egui::vec2(6.0, 6.0), egui::vec2(26.0, 26.0));
    theme::glass(&painter, chip, 6.0, colour);
    painter.text(
        chip.center(),
        egui::Align2::CENTER_CENTER,
        symbol,
        egui::FontId::proportional(16.0),
        Color32::WHITE,
    );
    let picture = picture.expand(4.0);
    let width = face.width() - 24.0;
    let line = |y: f32, text: &str, font: egui::FontId, colour: Color32| {
        theme::text_line(
            &painter,
            egui::pos2(face.min.x + 12.0, picture.max.y + y),
            text,
            font,
            colour,
            width,
        );
    };
    line(10.0, title, theme::display(16.0), theme::HEADING);
    line(34.0, note, egui::FontId::proportional(12.0), SOFT);
    let rim = if selected {
        egui::Stroke::new(2.0, ACCENT)
    } else if hovered {
        egui::Stroke::new(1.0, Color32::from_rgb(102, 167, 232))
    } else {
        egui::Stroke::new(1.0, Color32::from_rgb(170, 200, 230))
    };
    painter.rect_stroke(face, 8, rim, egui::StrokeKind::Inside);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

impl Menu {
    /// Draws the home screen over the whole window. Returns what was picked.
    pub fn show(&mut self, ctx: &egui::Context, platform: &mut Platform) -> Option<Pick> {
        if !self.scanned {
            self.games = saved_games();
            self.scanned = true;
        }
        // You always play as your account's name: it can't be changed
        // here (a new name is claimed on the Avatar tab).
        platform.look_up_account();
        self.name = platform.account.as_ref().map_or_else(|| "Player".to_owned(), |a| a.name.clone());
        let screen = ctx.content_rect();
        let mut ui = egui::Ui::new(
            ctx.clone(),
            "home".into(),
            egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(screen),
        );
        ui.painter().rect_filled(screen, 0.0, theme::BACK);
        theme::paint_sky(ui.painter(), screen);
        // The top bar, a sheet of sky-tinted glass: the mark (Home), the
        // tabs, and on the right Search and who you play as.
        let phone = theme::is_phone(screen);
        let bar = egui::Rect::from_min_size(
            screen.min,
            egui::vec2(screen.width(), if phone { 50.0 } else { 68.0 }),
        );
        theme::glass_bar(ui.painter(), bar);
        ui.painter().hline(
            bar.x_range(),
            bar.max.y,
            egui::Stroke::new(1.0, theme::see_through(20, 70, 130, 120)),
        );
        let inner = if phone { egui::vec2(8.0, 3.0) } else { egui::vec2(20.0, 12.0) };
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(inner)), |ui| {
            ui.horizontal_centered(|ui| self.top_bar(ui, platform, phone));
        });
        // The page sits on a big frosted pane over the sky, like a window
        // on a desktop, with the sky and hills showing round its edges.
        let pane = egui::Rect::from_min_max(egui::pos2(screen.min.x, bar.max.y), screen.max)
            .shrink2(if phone { egui::vec2(6.0, 6.0) } else { egui::vec2(22.0, 18.0) });
        theme::frosted_pane(ui.painter(), pane);
        if !phone {
            ui.painter().text(
                screen.right_bottom() - egui::vec2(10.0, 3.0),
                egui::Align2::RIGHT_BOTTOM,
                crate::update::tag(),
                egui::FontId::proportional(12.0),
                theme::MUTED,
            );
        }
        let page = pane.shrink2(if phone { egui::vec2(12.0, 8.0) } else { egui::vec2(22.0, 18.0) });
        let now = ui.input(|i| i.time);
        let since = match self.shown {
            Some((tab, since)) if tab == self.tab => since,
            _ => {
                self.shown = Some((self.tab, now));
                now
            }
        };
        let shown = ((now - since) as f32 / 0.24).clamp(0.0, 1.0);
        if shown < 1.0 {
            ui.ctx().request_repaint();
        }
        let shown = egui::emath::easing::cubic_out(shown);
        let mut pick = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(page), |ui| {
            ui.multiply_opacity(shown);
            if shown < 1.0 {
                ui.add_space(16.0 * (1.0 - shown));
            }
            egui::ScrollArea::vertical().id_salt("home-page").auto_shrink([false, false]).show(ui, |ui| {
                let name = self.name.trim().to_owned();
                if let Some(why) = platform.weak_password() {
                    platform.new_password_ui(ui, &why);
                    return;
                }
                if platform.needs_birthday() {
                    platform.birthday_page(ui);
                    return;
                }
                pick = match self.tab {
                    Tab::Home => {
                        crate::hub_home::home_tab(&mut self.home, ui, platform, &name, &mut self.tab)
                    }
                    Tab::Charts => crate::hub_home::charts_tab(&mut self.home, ui, platform, &name),
                    Tab::Search => {
                        crate::hub_home::search_tab(&mut self.home, ui, platform, &mut self.tab);
                        None
                    }
                    Tab::Friends => {
                        crate::hub_home::friends_tab(&mut self.home, ui, platform, &name, &mut self.tab)
                    }
                    Tab::Groups => {
                        crate::groups::groups_tab(
                            &mut self.groups,
                            &mut self.home,
                            ui,
                            platform,
                            &mut self.tab,
                        );
                        None
                    }
                    Tab::Create => self.create_tab(ui, platform),
                    Tab::Messages => {
                        crate::messages::messages_tab(ui, platform);
                        None
                    }
                    Tab::Avatar => {
                        egui::ScrollArea::vertical()
                            .id_salt("avatar")
                            .show(ui, |ui| self.avatar_tab(ui, platform));
                        None
                    }
                    Tab::Market => {
                        platform.market_ui(ui);
                        None
                    }
                    Tab::Profile => {
                        theme::heading(ui, t("Profile"));
                        ui.add_space(8.0);
                        platform.account_ui(ui);
                        ui.add_space(8.0);
                        crate::voice::toggle(ui);
                        None
                    }
                };
                if let Some(error) = &self.error {
                    ui.add_space(8.0);
                    crate::report::status(ui, error);
                }
            });
        });
        if let Some(at) = self.donated {
            let left = 4.0 - (now - at) as f32;
            if left <= 0.0 {
                self.donated = None;
            } else {
                ui.ctx().request_repaint();
                let fade = left.min(0.4) / 0.4;
                let text =
                    format!("{}  {}", theme::icon::BROWSER, t("The donate page opened in your browser"));
                let galley = ui.painter().layout_no_wrap(
                    text,
                    theme::display(15.0),
                    Color32::WHITE.gamma_multiply(fade),
                );
                let note = egui::Rect::from_center_size(
                    egui::pos2(screen.center().x, bar.max.y + 34.0),
                    galley.size() + egui::vec2(32.0, 18.0),
                );
                let painter =
                    ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, "donated".into()));
                painter.rect_filled(
                    note,
                    note.height() / 2.0,
                    theme::see_through(16, 40, 72, (220.0 * fade) as u8),
                );
                painter.galley(note.center() - galley.size() / 2.0, galley, Color32::WHITE);
            }
        }
        let credits = platform.balance().map(|(n, _)| n);
        let hub = platform.client();
        platform.buy_credits.ui(ctx, &hub, credits);
        platform.name = self.name.trim().to_owned();
        if pick.is_some() {
            self.error = None;
        }
        pick
    }

    /// Goes to a tab from the top bar.
    fn go(&mut self, tab: Tab) {
        self.tab = tab;
        self.error = None;
        if matches!(tab, Tab::Home | Tab::Charts) {
            self.home.view = crate::hub_home::View::Home;
        }
    }

    /// The top bar's contents: the mark, the tabs, the More menu, Search
    /// and who you play as. When the window is too narrow for every tab's
    /// name, the tabs show only their icons (with the name on hover).
    fn top_bar(&mut self, ui: &mut egui::Ui, platform: &mut Platform, phone: bool) {
        if theme::logo_button(ui, self.tab == Tab::Home)
            .on_hover_text(format!("PlayLibre {}", crate::update::tag()))
            .clicked()
        {
            self.go(Tab::Home);
        }
        ui.add_space(if phone { 4.0 } else { 10.0 });
        ui.spacing_mut().item_spacing.x = 2.0;
        let chip = player_chip_width(ui, &self.name, phone);
        let credits = platform.balance();
        let update = crate::update::ready();
        let gap = if phone { 4.0 } else { 8.0 };
        let icon_w = theme::tab_width(ui, theme::icon::LIST, "") + gap;
        let messages = if platform.account.is_some() { icon_w + 20.0 } else { 0.0 };
        let credits_w = credits
            .as_ref()
            .map_or(0.0, |(b, _)| theme::tab_width(ui, theme::icon::COIN, &b.to_string()) + gap);
        let donate_w = ui
            .painter()
            .layout_no_wrap(
                format!("{}  {}", theme::icon::HEART, t("Donate")),
                theme::display(13.0),
                Color32::WHITE,
            )
            .size()
            .x
            + 28.0
            + gap;
        let update_w = update
            .as_ref()
            .map_or(0.0, |_| theme::tab_width(ui, theme::icon::ARROW_CIRCLE_UP, t("Update")) + gap);
        let base = chip + gap + icon_w + messages + credits_w;
        let extras = donate_w + icon_w + update_w;
        let labels: f32 = TOP_TABS.iter().map(|(_, g, l)| theme::tab_width(ui, g, t(l)) + 2.0).sum();
        let icon_tab = |g: &str| theme::tab_width(ui, g, "") + 2.0;
        let icons: f32 = TOP_TABS.iter().map(|(_, g, _)| icon_tab(g)).sum();
        let mac = if cfg!(target_os = "macos") { icon_w } else { 0.0 };
        let room = ui.available_width() - mac - 8.0;
        let compact = labels + base + extras > room;
        let folded = icons + base + extras > room;
        let keep = if folded {
            let mut left = room - base - icon_w;
            TOP_TABS
                .iter()
                .take_while(|(_, g, _)| {
                    left -= icon_tab(g);
                    left >= 0.0
                })
                .count()
        } else {
            TOP_TABS.len()
        };
        for &(tab, glyph, label) in &TOP_TABS[..keep] {
            let label = t(label);
            let shown = if compact { "" } else { label };
            let response = theme::tab(ui, glyph, shown, self.tab == tab);
            let response = if compact { response.on_hover_text(label) } else { response };
            if response.clicked() {
                self.go(tab);
            }
        }
        #[cfg(target_os = "macos")]
        {
            let more = theme::tab(ui, theme::icon::CARET_DOWN, "", false).on_hover_text(t("More"));
            egui::Popup::menu(&more).show(|ui| {
                ui.set_min_width(190.0);
                if ui
                    .button(format!("{}  {}", theme::icon::TERMINAL_WINDOW, t("Add the playlibre command")))
                    .on_hover_text(t("Links /usr/local/bin/playlibre to this app, so typing playlibre in Terminal opens it. Asks for your password."))
                    .clicked()
                {
                    ui.close();
                    if let Err(why) = crate::command::link_for_everyone() {
                        self.error = Some(why);
                    }
                }
            });
        }
        // Search and who you play as, at the right of the bar.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = gap;
            if folded {
                let more = theme::tab(ui, theme::icon::LIST, "", false).on_hover_text(t("More"));
                egui::Popup::menu(&more).show(|ui| {
                    ui.set_min_width(200.0);
                    for &(tab, glyph, label) in &TOP_TABS[keep..] {
                        if ui.selectable_label(self.tab == tab, format!("{glyph}  {}", t(label))).clicked() {
                            ui.close();
                            self.go(tab);
                        }
                    }
                    if keep < TOP_TABS.len() {
                        ui.separator();
                    }
                    if let Some(version) = &update {
                        let label = format!("{}  {}", theme::icon::ARROW_CIRCLE_UP, t("Update"));
                        if ui
                            .button(label)
                            .on_hover_text(tf("Restart with {version}", &[("version", &version)]))
                            .clicked()
                            && let Err(why) = crate::update::restart()
                        {
                            self.error = Some(why);
                        }
                    }
                    if ui.button(format!("{}  {}", theme::icon::HEART, t("Donate"))).clicked() {
                        ui.close();
                        crate::report::open_url(DONATE);
                        self.donated = Some(ui.input(|i| i.time));
                    }
                    ui.menu_button(format!("{}  {}", theme::icon::GLOBE, t("Language")), |ui| {
                        languages(ui);
                    });
                });
            }
            if player_chip(ui, &self.name, platform.account.is_some(), phone).clicked() {
                self.go(Tab::Profile);
            }
            // The account's Credits, by its name.
            if let Some((balance, _)) = &credits {
                let shown = theme::tab(ui, theme::icon::COIN, &balance.to_string(), false);
                if shown.on_hover_text(t("Buy Credits")).clicked() {
                    if cfg!(target_os = "android") {
                        self.go(Tab::Avatar);
                    } else {
                        platform.buy_credits.open();
                    }
                }
            }
            if platform.account.is_some() {
                let unread = platform.chats.unread();
                let label = if unread > 0 { unread.min(99).to_string() } else { String::new() };
                let messages = theme::tab(ui, theme::icon::CHAT_CIRCLE, &label, self.tab == Tab::Messages);
                if messages.on_hover_text(t("Messages")).clicked() {
                    self.go(Tab::Messages);
                }
            }
            if !folded {
                if theme::block_button(ui, theme::Kind::Donate, theme::icon::HEART, t("Donate"), 28.0)
                    .on_hover_text(t("PlayLibre is open source. Donations are highly appreciated."))
                    .clicked()
                {
                    crate::report::open_url(DONATE);
                    self.donated = Some(ui.input(|i| i.time));
                }
                let globe = theme::tab(ui, theme::icon::GLOBE, "", false).on_hover_text(t("Language"));
                egui::Popup::menu(&globe).show(|ui| {
                    ui.set_min_width(180.0);
                    languages(ui);
                });
            }
            let search = theme::tab(ui, theme::icon::MAGNIFYING_GLASS, "", self.tab == Tab::Search);
            if search.on_hover_text(t("Search for players and games")).clicked() {
                self.go(Tab::Search);
            }
            if !folded && let Some(version) = &update {
                let update = theme::tab(ui, theme::icon::ARROW_CIRCLE_UP, t("Update"), false);
                if update.on_hover_text(tf("Restart with {version}", &[("version", &version)])).clicked()
                    && let Err(why) = crate::update::restart()
                {
                    self.error = Some(why);
                }
            }
        });
    }

    /// How the player looks, as saved on the Avatar tab.
    pub fn look(&mut self) -> Look {
        self.look
            .get_or_insert_with(|| {
                let usual = Look::for_player(0);
                std::fs::read_to_string(avatar_file())
                    .ok()
                    .and_then(|text| playlibre_instance::serde_json::from_str(&text).ok())
                    .map_or_else(|| usual.clone(), |json| Look::from_json(&json, usual.clone()))
            })
            .clone()
    }

    /// Takes the account's look from the Hub, as the Avatar page on the
    /// website and every other computer show it: all of it, so anything it
    /// leaves out (a hat taken off) is the usual for `name`, never what this
    /// computer had before. `Null` (a guest) goes back to the usual look.
    /// Returns the look to play with: `None` for the usual one.
    pub fn use_look(&mut self, json: &playlibre_instance::serde_json::Value, name: &str) -> Option<Look> {
        if json.is_null() {
            self.look = None;
            let _ = std::fs::remove_file(avatar_file());
            return None;
        }
        let look = playlibre_avatar::look_of(json, name);
        if self.look.as_ref() != Some(&look) {
            let _ = std::fs::create_dir_all(data_dir())
                .and_then(|_| std::fs::write(avatar_file(), look.to_json().to_string()));
            self.look = Some(look.clone());
        }
        Some(look)
    }

    /// The look to play a game with: the account's (kept on this computer
    /// for when the Hub can't be reached), or `None` for a guest, who has
    /// the usual look.
    pub fn playing_look(&mut self, platform: &Platform) -> Option<Look> {
        (!platform.is_guest()).then(|| self.look())
    }

    fn avatar_tab(&mut self, ui: &mut egui::Ui, platform: &mut Platform) {
        if platform.account.is_none() {
            // Picking a look needs an account: it's kept there, so it's the
            // same in every game, on the website and on every computer.
            theme::heading(ui, t("Your avatar"));
            if platform.account_known() {
                ui.label(
                    RichText::new(t(
                        "Make an account or sign in to choose how you look. Until then you play with the usual look.",
                    ))
                    .color(SOFT),
                );
                ui.add_space(12.0);
                if ui.button(t("Make an account or sign in")).clicked() {
                    self.go(Tab::Profile);
                }
            } else {
                ui.label(RichText::new(t("Looking up your account…")).color(SOFT));
            }
            return;
        }
        let mut look = self.look();
        theme::heading(ui, t("Your avatar"));
        ui.add_space(12.0);
        let owned = platform.inventory();
        ui.horizontal_top(|ui| {
            theme::panel().show(ui, |ui| {
                draw_avatar(ui, &look);
            });
            ui.add_space(20.0);
            ui.vertical(|ui| {
                ui.horizontal_wrapped(|ui| {
                    for (part, label) in [
                        (Part::Body, "Body"),
                        (Part::Hats, "Hats"),
                        (Part::Shirts, "Shirts"),
                        (Part::Trousers, "Trousers"),
                    ] {
                        if ui.selectable_label(self.part == part, t(label)).clicked() {
                            self.part = part;
                        }
                    }
                });
                ui.add_space(10.0);
                let kind = match self.part {
                    Part::Body => "",
                    Part::Hats => "hat",
                    Part::Shirts => "shirt",
                    Part::Trousers => "pants",
                };
                let mine: Vec<_> = owned.iter().filter(|i| i.kind == kind).collect();
                let mut worn = None;
                match self.part {
                    Part::Body => {
                        theme::label_caps(ui, t("Skin"));
                        swatches(ui, &mut look.colors.skin, "Skin");
                    }
                    Part::Hats => {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                            for hat in Hat::ALL {
                                let shown = Look { hat, item: None, ..Look::for_player(0) };
                                let on = look.hat == hat && look.item.is_none();
                                if tile(ui, &shown, true, t(hat.name()), on).clicked() {
                                    look.hat = hat;
                                    look.item = None;
                                }
                            }
                            for item in &mine {
                                let Some(shown) = crate::browse::item_look(item) else { continue };
                                let on = look.item.is_some() && look.item == shown.item;
                                if tile(ui, &shown, true, &item.name, on).clicked() {
                                    look.item = shown.item.clone();
                                    worn = Some(item.item_id.clone());
                                }
                            }
                        });
                        if look.hat != Hat::None && look.item.is_none() {
                            ui.add_space(8.0);
                            theme::label_caps(ui, t("Hat colour"));
                            swatches(ui, &mut look.hat_color, "Hat");
                        }
                    }
                    Part::Shirts | Part::Trousers => {
                        let shirts = self.part == Part::Shirts;
                        theme::label_caps(ui, t("Colour"));
                        swatches(
                            ui,
                            if shirts { &mut look.colors.shirt } else { &mut look.colors.pants },
                            "Shirt",
                        );
                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                            let bare = Look {
                                hat: Hat::None,
                                item: None,
                                shirt: if shirts { None } else { look.shirt.clone() },
                                pants: if shirts { look.pants.clone() } else { None },
                                ..look.clone()
                            };
                            let slot_empty = if shirts { look.shirt.is_none() } else { look.pants.is_none() };
                            if tile(ui, &bare, false, t("None"), slot_empty).clicked() {
                                if shirts {
                                    look.shirt = None;
                                } else {
                                    look.pants = None;
                                }
                            }
                            for item in &mine {
                                let Some(shown) = crate::browse::item_look(item) else { continue };
                                let (wearing, offered) = if shirts {
                                    (&look.shirt, &shown.shirt)
                                } else {
                                    (&look.pants, &shown.pants)
                                };
                                let on = wearing.is_some() && wearing == offered;
                                if tile(ui, &shown, false, &item.name, on).clicked() {
                                    let offered = offered.clone();
                                    if shirts {
                                        look.shirt = offered;
                                    } else {
                                        look.pants = offered;
                                    }
                                    worn = Some(item.item_id.clone());
                                }
                            }
                        });
                    }
                }
                if self.part != Part::Body && mine.is_empty() {
                    ui.add_space(6.0);
                    if platform.inventory_loading() {
                        ui.spinner();
                    } else if ui.link(t("Get more in the Marketplace")).clicked() {
                        self.go(Tab::Market);
                    }
                }
                if let Some(id) = worn {
                    platform.wore(&id);
                }
                ui.add_space(12.0);
                if ui.button(t("Back to the usual look")).clicked() {
                    look = Look::for_player(0);
                }
            });
        });
        if let Some(why) = &platform.look_error {
            ui.add_space(8.0);
            crate::report::error(ui, &tf("Your avatar was not saved: {error}", &[("error", why)]));
        }
        if Some(&look) != self.look.as_ref() {
            platform.save_look(look.to_json());
            let json = look.to_json().to_string();
            self.look = Some(look);
            let saved = std::fs::create_dir_all(data_dir()).and_then(|_| std::fs::write(avatar_file(), json));
            if let Err(e) = saved {
                self.error = Some(tf("Could not save your avatar: {error}", &[("error", &e)]));
            }
        }
    }

    /// The games on this computer, as tiles to pick from.
    fn game_tiles(&mut self, ui: &mut egui::Ui, with_blank: bool) -> Option<Game> {
        let mut chosen = None;
        let mut tiles: Vec<(Game, &str, String)> = Vec::new();
        if with_blank {
            tiles.insert(0, (Game::Blank, theme::icon::PLUS, t("Start from an empty baseplate").to_owned()));
        }
        for path in &self.games {
            tiles.push((
                Game::File(path.clone()),
                theme::icon::FLOPPY_DISK,
                t("Saved on this computer").into(),
            ));
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(16.0, 16.0);
            for (game, symbol, note) in tiles {
                let selected = !with_blank && self.game == game;
                let tile = card(ui, symbol, &game.label(), &game.id(), &note, selected);
                if tile.double_clicked() || (with_blank && tile.clicked()) {
                    chosen = Some(game.clone());
                }
                if tile.clicked() {
                    self.game = game;
                }
            }
        });
        chosen
    }

    fn create_tab(&mut self, ui: &mut egui::Ui, platform: &Platform) -> Option<Pick> {
        let mut pick = None;
        if self.picker.is_open() {
            // Keep looking for the choice while the window is up.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        match self.picker.poll() {
            Some(crate::file_picker::Picked::File(path)) => {
                self.path = path.display().to_string();
                pick = Some(Pick::Edit(Game::File(path)));
            }
            Some(crate::file_picker::Picked::Unavailable) => {
                self.error =
                    Some(t("No file picker could be opened here. Type the file's path instead.").into());
            }
            Some(crate::file_picker::Picked::Cancelled) | None => {}
        }
        theme::heading(ui, t("Make a game"));
        ui.label(RichText::new(t("Opens the editor: build with parts, scripts and terrain, press Play to test, and save it to play from Play.")).color(SOFT));
        ui.add_space(12.0);
        if let Some(game) = self.game_tiles(ui, true) {
            pick = Some(Pick::Edit(game));
        }
        ui.add_space(16.0);
        ui.label(tf("Games are saved in {folder}", &[("folder", &games_dir().display())]));
        ui.horizontal(|ui| {
            let typed = ui.add(
                egui::TextEdit::singleline(&mut self.path)
                    .hint_text(t("or open a .lrx, .rbxl or .rbxlx file by its path"))
                    .desired_width(320.0),
            );
            let enter = typed.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let typed_path = !self.path.trim().is_empty();
            let open = ui
                .button(t("Open"))
                .on_hover_text(t("Open the typed path, or pick a file from your computer"));
            let browse = ui
                .add_enabled(!self.picker.is_open(), egui::Button::new(t("Browse…")))
                .on_hover_text(t("Pick a game file from your computer"));
            if (open.clicked() || enter) && typed_path {
                pick = Some(Pick::Edit(Game::File(PathBuf::from(self.path.trim()))));
            } else if browse.clicked() || open.clicked() {
                // With nothing typed, Open browses too.
                self.picker.open(&games_dir());
            }
            if theme::tool_button(ui, theme::icon::ARROW_CLOCKWISE, "", false)
                .on_hover_text(t("Look for saved games again"))
                .clicked()
            {
                self.scanned = false;
            }
        });
        if let Some(game) = crate::my_games::my_games(&mut self.my_games, ui, platform) {
            pick = Some(Pick::Edit(game));
        }
        pick
    }
}

/// The tabs along the top bar, after the mark (which is Home).
const TOP_TABS: [(Tab, &str, &str); 6] = [
    (Tab::Charts, theme::icon::CHART_BAR, "Charts"),
    (Tab::Groups, theme::icon::SHIELD, "Groups"),
    (Tab::Avatar, theme::icon::T_SHIRT, "Avatar"),
    (Tab::Market, theme::icon::STOREFRONT, "Marketplace"),
    (Tab::Create, theme::icon::HAMMER, "Create"),
    (Tab::Profile, theme::icon::USER_CIRCLE, "Profile"),
];

const CHIP_CAPS: &str = "Playing as";
const DONATE: &str = "https://playlibre.net/donate";

fn languages(ui: &mut egui::Ui) {
    for language in crate::i18n::Language::ALL {
        let item = ui.add(egui::Button::selectable(crate::i18n::current() == language, language.name()));
        if item.clicked() {
            crate::i18n::set(language, true);
            theme::reload_fonts(ui.ctx());
            ui.close();
        }
    }
}

fn chip_name_width(phone: bool) -> f32 {
    if phone { 90.0 } else { 160.0 }
}

fn player_chip_width(ui: &egui::Ui, name: &str, phone: bool) -> f32 {
    let caps = if phone {
        -8.0
    } else {
        ui.painter().layout_no_wrap(t(CHIP_CAPS).into(), theme::display(12.0), theme::HEADING).size().x
    };
    let shown = ui.painter().layout_no_wrap(name.to_owned(), theme::display(15.0), theme::TEXT).size().x;
    caps + shown.min(chip_name_width(phone)) + 34.0
}

/// Who you play as, in a glass pill at the right of the top bar: your
/// account's name, which can't be typed over. Clicking it opens Avatar,
/// where the account is.
fn player_chip(ui: &mut egui::Ui, name: &str, signed_in: bool, phone: bool) -> egui::Response {
    let size = egui::vec2(player_chip_width(ui, name, phone), 36.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    theme::name_for_screen_readers(
        ui,
        &response,
        egui::WidgetType::Button,
        &tf("Playing as {name}", &[("name", &name)]),
        None,
    );
    let painter = ui.painter();
    let tint = if response.hovered() {
        Color32::from_rgba_unmultiplied(235, 247, 255, 200)
    } else {
        Color32::from_rgba_unmultiplied(235, 247, 255, 150)
    };
    theme::glass(painter, rect, 18.0, tint);
    let x = rect.min.x + 14.0;
    let mut name_x = x;
    if !phone {
        let caps = painter.layout_no_wrap(t(CHIP_CAPS).into(), theme::display(12.0), theme::HEADING);
        name_x += caps.size().x + 8.0;
        painter.galley(egui::pos2(x, rect.center().y - caps.size().y / 2.0), caps, theme::HEADING);
    }
    let mut job =
        egui::text::LayoutJob::simple_singleline(name.to_owned(), theme::display(15.0), theme::TEXT);
    job.wrap = egui::text::TextWrapping::truncate_at_width(chip_name_width(phone));
    let shown = painter.layout_job(job);
    painter.galley(egui::pos2(name_x, rect.center().y - shown.size().y / 2.0), shown, theme::TEXT);
    let hint = if signed_in {
        t("Your account's name. Change it on the Profile tab.")
    } else {
        t("Make an account on the Profile tab to play as yourself.")
    };
    response.on_hover_text(hint).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// An owned thing to put on, drawn on `look`.
fn tile(ui: &mut egui::Ui, look: &Look, head: bool, name: &str, on: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(96.0, 116.0), egui::Sense::click());
    let painter = ui.painter();
    let fill = if response.hovered() { theme::WELL } else { theme::CARD };
    painter.rect_filled(rect, 8.0, fill);
    let stroke = if on { egui::Stroke::new(2.0, theme::ACCENT) } else { egui::Stroke::new(1.0, theme::LINE) };
    painter.rect_stroke(rect, 8.0, stroke, egui::StrokeKind::Inside);
    let picture = egui::Rect::from_min_size(rect.min + egui::vec2(6.0, 6.0), egui::vec2(84.0, 84.0));
    crate::browse::look_picture(ui, picture, look, head);
    let mut job = egui::text::LayoutJob::simple_singleline(
        name.to_owned(),
        egui::FontId::proportional(12.0),
        theme::TEXT,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(86.0);
    let galley = ui.painter().layout_job(job);
    let at = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.max.y - 20.0);
    ui.painter().galley(at, galley, theme::TEXT);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Where the avatar is saved.
fn avatar_file() -> PathBuf {
    data_dir().join("avatar.json")
}

/// A row of colour squares; the chosen one is outlined. The last square
/// opens a picker for any colour.
fn swatches(ui: &mut egui::Ui, color: &mut Color3, label: &str) {
    let avatar = playlibre_engine::character::avatar();
    let choices = if label == "Skin" { &avatar.skins } else { &avatar.clothes };
    let rgb = |c: Color3| {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [byte(c.r), byte(c.g), byte(c.b)]
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for [r, g, b] in choices.iter().copied() {
            let (rect, response) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
            let chosen = rgb(*color) == [r, g, b];
            theme::name_for_screen_readers(
                ui,
                &response,
                egui::WidgetType::RadioButton,
                &tf(
                    "{part} colour {colour}",
                    &[("part", &t(label)), ("colour", &t(&crate::theme::colour_name([r, g, b])))],
                ),
                Some(chosen),
            );
            let face = if chosen { rect.translate(egui::vec2(0.0, -2.0)) } else { rect };
            ui.painter().rect_filled(
                rect.translate(egui::vec2(0.0, 2.0)),
                theme::CORNER,
                Color32::from_rgb(r, g, b).gamma_multiply(0.55),
            );
            ui.painter().rect_filled(face, theme::CORNER, Color32::from_rgb(r, g, b));
            if chosen {
                ui.painter().rect_stroke(
                    face,
                    theme::CORNER,
                    egui::Stroke::new(2.0, theme::TEXT),
                    egui::StrokeKind::Outside,
                );
            }
            if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                *color = Color3::from_rgb8(r, g, b);
            }
        }
        let [r, g, b] = rgb(*color);
        let mut picked = [r, g, b];
        if ui.color_edit_button_srgb(&mut picked).changed() {
            *color = Color3::from_rgb8(picked[0], picked[1], picked[2]);
        }
    });
}

/// A picture of the avatar's whole body.
fn draw_avatar(ui: &mut egui::Ui, look: &Look) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(220.0, 340.0), egui::Sense::hover());
    let picture = egui::Rect::from_min_size(rect.min, egui::vec2(220.0, 312.0));
    let texture = crate::hub_home::avatar_texture(ui.ctx(), look, playlibre_avatar::Shot::Body, (330, 468));
    ui.painter().image(
        texture.id(),
        picture,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    ui.painter().text(
        egui::pos2(rect.center().x, rect.max.y - 14.0),
        egui::Align2::CENTER_CENTER,
        look.item.as_ref().map_or(look.hat.name(), |item| item.name.as_str()),
        egui::FontId::proportional(13.0),
        SOFT,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same pictures the website makes for these ids.
    #[test]
    fn blocks_match_the_website() {
        for (id, heights, colour) in WEBSITE {
            let (got, c) = blocks(id);
            assert_eq!(got, heights, "{id}");
            assert_eq!([c.r(), c.g(), c.b()], colour, "{id}");
        }
    }

    /// Every picture has cubes in the middle and looks the same from the
    /// left and the right.
    #[test]
    fn blocks_are_mirrored() {
        for id in ["starter:Chess", "x", "", "a long game id 1234"] {
            let (h, _) = blocks(id);
            for y in 0..4 {
                for x in 0..4 {
                    assert_eq!(h[y * 4 + x], h[x * 4 + y]);
                }
            }
            assert!(h[5] > 0 && h[10] > 0);
        }
    }

    const WEBSITE: [(&str, [u8; 16], [u8; 3]); 3] = [
        ("starter:Chess", [2, 2, 2, 1, 2, 2, 2, 1, 2, 2, 3, 2, 1, 1, 2, 0], [71, 198, 209]),
        ("starter:Kart race", [0, 0, 0, 0, 0, 3, 2, 0, 0, 2, 2, 0, 0, 0, 0, 0], [71, 92, 209]),
        ("abc123", [0, 0, 0, 2, 0, 2, 3, 2, 0, 3, 1, 2, 2, 2, 2, 2], [209, 71, 87]),
    ];
}
