//! The home screen: what Libroxel shows when it starts, like a game
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
use libroxel_engine::character::{BodyColors, Hat, Look};
use libroxel_instance::Color3;

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
    Games,
    Search,
    Friends,
    Groups,
    Create,
    Avatar,
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
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            open: false,
            tab: Tab::Home,
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
        let bar = egui::Rect::from_min_size(screen.min, egui::vec2(screen.width(), 68.0));
        theme::glass_bar(ui.painter(), bar);
        ui.painter().hline(
            bar.x_range(),
            bar.max.y,
            egui::Stroke::new(1.0, theme::see_through(20, 70, 130, 120)),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(egui::vec2(20.0, 12.0))), |ui| {
            ui.horizontal_centered(|ui| self.top_bar(ui, platform));
        });
        // The page sits on a big frosted pane over the sky, like a window
        // on a desktop, with the sky and hills showing round its edges.
        let pane = egui::Rect::from_min_max(egui::pos2(screen.min.x, bar.max.y), screen.max)
            .shrink2(egui::vec2(22.0, 18.0));
        theme::frosted_pane(ui.painter(), pane);
        ui.painter().text(
            screen.right_bottom() - egui::vec2(10.0, 3.0),
            egui::Align2::RIGHT_BOTTOM,
            crate::update::tag(),
            egui::FontId::proportional(12.0),
            theme::MUTED,
        );
        let page = pane.shrink2(egui::vec2(22.0, 18.0));
        let mut pick = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(page), |ui| {
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
                    Tab::Games => crate::hub_home::games_tab(&mut self.home, ui, platform, &name),
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
                    Tab::Create => self.create_tab(ui),
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
                    Tab::Profile => {
                        let look = self.look().to_json();
                        theme::heading(ui, t("Profile"));
                        ui.add_space(8.0);
                        platform.account_ui(ui, look);
                        None
                    }
                };
                if let Some(error) = &self.error {
                    ui.add_space(8.0);
                    crate::report::status(ui, error);
                }
            });
        });
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
        if matches!(tab, Tab::Home | Tab::Games | Tab::Charts) {
            self.home.view = crate::hub_home::View::Home;
        }
    }

    /// The top bar's contents: the mark, the tabs, the More menu, Search
    /// and who you play as. When the window is too narrow for every tab's
    /// name, the tabs show only their icons (with the name on hover).
    fn top_bar(&mut self, ui: &mut egui::Ui, platform: &mut Platform) {
        if theme::logo_button(ui, self.tab == Tab::Home)
            .on_hover_text(format!("Libroxel {}", crate::update::tag()))
            .clicked()
        {
            self.go(Tab::Home);
        }
        ui.add_space(10.0);
        ui.spacing_mut().item_spacing.x = 2.0;
        let chip = player_chip_width(ui, &self.name);
        let right = chip + 148.0 + 24.0 + if crate::update::ready().is_some() { 110.0 } else { 0.0 };
        let wide: f32 =
            TOP_TABS.iter().map(|(_, glyph, label)| theme::tab_width(ui, glyph, t(label))).sum::<f32>()
                + 2.0 * TOP_TABS.len() as f32;
        let compact = wide > ui.available_width() - right;
        for (tab, glyph, label) in TOP_TABS {
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
                    .button(format!("{}  {}", theme::icon::TERMINAL_WINDOW, t("Add the libroxel command")))
                    .on_hover_text(t("Links /usr/local/bin/libroxel to this app, so typing libroxel in Terminal opens it. Asks for your password."))
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
            ui.spacing_mut().item_spacing.x = 8.0;
            if player_chip(ui, &self.name, platform.account.is_some()).clicked() {
                self.go(Tab::Profile);
            }
            if platform.account.is_some() {
                let unread = platform.chats.unread();
                let label = if unread > 0 { unread.min(99).to_string() } else { String::new() };
                let messages = theme::tab(ui, theme::icon::CHAT_CIRCLE, &label, self.tab == Tab::Messages);
                if messages.on_hover_text(t("Messages")).clicked() {
                    self.go(Tab::Messages);
                }
            }
            let globe = theme::tab(ui, theme::icon::GLOBE, "", false).on_hover_text(t("Language"));
            egui::Popup::menu(&globe).show(|ui| {
                ui.set_min_width(180.0);
                for language in crate::i18n::Language::ALL {
                    let missing_font =
                        language == crate::i18n::Language::Chinese && !theme::has_chinese_font();
                    let item = ui
                        .add_enabled(
                            !missing_font,
                            egui::Button::selectable(crate::i18n::current() == language, language.name()),
                        )
                        .on_disabled_hover_text(t("This computer has no font with Chinese characters."));
                    if item.clicked() {
                        crate::i18n::set(language, true);
                        theme::reload_fonts(ui.ctx());
                        ui.close();
                    }
                }
            });
            let search = theme::tab(ui, theme::icon::MAGNIFYING_GLASS, "", self.tab == Tab::Search);
            if search.on_hover_text(t("Search for players and games")).clicked() {
                self.go(Tab::Search);
            }
            if let Some(version) = crate::update::ready() {
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
                    .and_then(|text| libroxel_instance::serde_json::from_str(&text).ok())
                    .map_or_else(|| usual.clone(), |json| Look::from_json(&json, usual.clone()))
            })
            .clone()
    }

    pub fn use_look(&mut self, json: &libroxel_instance::serde_json::Value) {
        let look = Look::from_json(json, self.look());
        if self.look.as_ref() != Some(&look) {
            let _ = std::fs::create_dir_all(data_dir())
                .and_then(|_| std::fs::write(avatar_file(), look.to_json().to_string()));
            self.look = Some(look);
        }
    }

    fn avatar_tab(&mut self, ui: &mut egui::Ui, platform: &mut Platform) {
        let mut look = self.look();
        theme::heading(ui, t("Your avatar"));
        ui.label(
            RichText::new(t("How you look in every game you play or join. Other players see it too."))
                .color(SOFT),
        );
        ui.add_space(12.0);
        ui.horizontal_top(|ui| {
            theme::panel().show(ui, |ui| {
                draw_avatar(ui, &look);
            });
            ui.add_space(20.0);
            ui.vertical(|ui| {
                for (label, color) in [
                    ("Skin", &mut look.colors.skin),
                    ("Shirt", &mut look.colors.shirt),
                    ("Trousers", &mut look.colors.pants),
                ] {
                    theme::label_caps(ui, t(label));
                    swatches(ui, color, label);
                    ui.add_space(8.0);
                }
                theme::label_caps(ui, t("Hat"));
                ui.horizontal_wrapped(|ui| {
                    for hat in Hat::ALL {
                        let on = look.hat == hat && look.item.is_none();
                        if ui.selectable_label(on, t(hat.name())).clicked() {
                            look.hat = hat;
                            // A hat from here replaces one from the catalog.
                            look.item = None;
                        }
                    }
                });
                if look.hat != Hat::None {
                    ui.add_space(4.0);
                    swatches(ui, &mut look.hat_color, "Hat");
                }
                ui.add_space(12.0);
                if ui.button(t("Back to the usual look")).clicked() {
                    look = Look::for_player(0);
                }
            });
        });
        ui.add_space(24.0);
        if let Some(picked) = platform.catalog_ui(ui, look.item.as_deref()) {
            look.item = picked.map(std::sync::Arc::new);
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

    fn create_tab(&mut self, ui: &mut egui::Ui) -> Option<Pick> {
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
        pick
    }
}

/// The tabs along the top bar, after the mark (which is Home).
const TOP_TABS: [(Tab, &str, &str); 6] = [
    (Tab::Games, theme::icon::GAME_CONTROLLER, "Games"),
    (Tab::Charts, theme::icon::CHART_BAR, "Charts"),
    (Tab::Groups, theme::icon::SHIELD, "Groups"),
    (Tab::Avatar, theme::icon::T_SHIRT, "Avatar"),
    (Tab::Create, theme::icon::HAMMER, "Create"),
    (Tab::Profile, theme::icon::USER_CIRCLE, "Profile"),
];

const CHIP_CAPS: &str = "Playing as";

fn player_chip_width(ui: &egui::Ui, name: &str) -> f32 {
    let caps =
        ui.painter().layout_no_wrap(t(CHIP_CAPS).into(), theme::display(12.0), theme::HEADING).size().x;
    let shown = ui.painter().layout_no_wrap(name.to_owned(), theme::display(15.0), theme::TEXT).size().x;
    caps + shown.min(160.0) + 34.0
}

/// Who you play as, in a glass pill at the right of the top bar: your
/// account's name, which can't be typed over. Clicking it opens Avatar,
/// where the account is.
fn player_chip(ui: &mut egui::Ui, name: &str, signed_in: bool) -> egui::Response {
    let size = egui::vec2(player_chip_width(ui, name), 36.0);
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
    let caps = painter.layout_no_wrap(t(CHIP_CAPS).into(), theme::display(12.0), theme::HEADING);
    let x = rect.min.x + 14.0;
    painter.galley(egui::pos2(x, rect.center().y - caps.size().y / 2.0), caps.clone(), theme::HEADING);
    let mut job =
        egui::text::LayoutJob::simple_singleline(name.to_owned(), theme::display(15.0), theme::TEXT);
    job.wrap = egui::text::TextWrapping::truncate_at_width(160.0);
    let shown = painter.layout_job(job);
    painter.galley(
        egui::pos2(x + caps.size().x + 8.0, rect.center().y - shown.size().y / 2.0),
        shown,
        theme::TEXT,
    );
    let hint = if signed_in {
        t("Your account's name. Change it on the Profile tab.")
    } else {
        t("Make an account on the Profile tab to play as yourself.")
    };
    response.on_hover_text(hint).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Where the avatar is saved.
fn avatar_file() -> PathBuf {
    data_dir().join("avatar.json")
}

/// A row of colour squares; the chosen one is outlined. The last square
/// opens a picker for any colour.
fn swatches(ui: &mut egui::Ui, color: &mut Color3, label: &str) {
    let avatar = libroxel_engine::character::avatar();
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

/// Draws the avatar from the front, the way the character is built.
fn draw_avatar(ui: &mut egui::Ui, look: &Look) {
    let avatar = libroxel_engine::character::avatar();
    let front = &avatar.front;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(front.width, front.height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let scale = front.scale;
    let hips = egui::pos2(rect.center().x, rect.min.y + front.hips);
    let at = |x: f32, y: f32| egui::pos2(hips.x + x * scale, hips.y - y * scale);
    let colour = |c: Color3| {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color32::from_rgb(byte(c.r), byte(c.g), byte(c.b))
    };
    let rgb = |[r, g, b]: [u8; 3]| Color32::from_rgb(r, g, b);
    let block = |x: f32, y: f32, w: f32, h: f32, c: Color32| {
        let r = egui::Rect::from_center_size(at(x, y), egui::vec2(w * scale, h * scale));
        painter.rect_filled(r, 1.0, c);
        painter.rect_stroke(
            r,
            1.0,
            egui::Stroke::new(1.5, c.gamma_multiply(front.edge)),
            egui::StrokeKind::Inside,
        );
    };
    let BodyColors { skin, shirt, pants } = look.colors;
    for ([x, y], [w, h], part) in &front.blocks {
        let c = match part.as_str() {
            "skin" => colour(skin),
            "shirt" => colour(shirt),
            "pants" => colour(pants),
            _ => rgb(front.eye),
        };
        block(*x, *y, *w, *h, c);
    }
    if let Some(item) = &look.item {
        let mut parts: Vec<_> = item.parts.iter().collect();
        parts.sort_by(|a, b| b.offset.z.total_cmp(&a.offset.z));
        for part in parts {
            let sideways = (part.turn.z.abs() - 90.0).abs() < 1.0;
            let (w, h) = if sideways { (part.size.y, part.size.x) } else { (part.size.x, part.size.y) };
            let (x, y) = (part.offset.x, front.head + part.offset.y);
            if part.shape == libroxel_instance::PartShape::Ball {
                painter.circle_filled(at(x, y), w * scale / 2.0, colour(part.color));
            } else {
                block(x, y, w, h, colour(part.color));
            }
        }
    }
    let hat = if look.item.is_some() { libroxel_engine::character::Hat::None } else { look.hat };
    for part in hat.parts() {
        let c = if part.tinted { colour(look.hat_color) } else { rgb(avatar.dark) };
        let [w, h] =
            if part.turn != 0.0 { [part.size[1], part.size[0]] } else { [part.size[0], part.size[1]] };
        let (x, y) = (part.at[0], front.head + part.at[1]);
        if part.shape == libroxel_instance::PartShape::Ball {
            painter.circle_filled(at(x, y), w * scale / 2.0, c);
        } else {
            block(x, y, w, h, c);
        }
    }
    painter.text(
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
