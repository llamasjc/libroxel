//! The home screen: what Libroxel shows when it starts, like a game
//! launcher. **Play** lists the games on this computer; **Online** finds
//! and joins games other people host; **Create** opens the editor on a new
//! or saved game; **Host** shares a game, runs a server in the background
//! and turns this computer's Hub on or off.

use std::path::PathBuf;

use bevy_egui::egui::{self, Color32, RichText};
use libroxel_instance::place;

use crate::browse::Platform;
use crate::server::{BackgroundServer, HubUse, ServerSettings, data_dir};
use crate::starter_games::{self, STARTER_GAMES};
use crate::{games_dir, templates, theme};
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
            Game::Starter(name) => (*name).into(),
            Game::Blank => "New empty world".into(),
            Game::File(path) => path
                .file_stem()
                .map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned()),
        }
    }

    /// The game as a saved place, for a server.
    fn text(&self) -> Result<String, String> {
        match self {
            Game::Starter(name) => {
                let game =
                    starter_games::starter(name).ok_or_else(|| format!("there is no starter game {name}"))?;
                place::save(&(game.build)()).map_err(|e| e.to_string())
            }
            Game::Blank => place::save(&templates::blank()).map_err(|e| e.to_string()),
            // A file from Roblox Studio is brought across first.
            Game::File(path) if libroxel_rbx::is_roblox_file(path) => {
                let opened = crate::open::open(path)?;
                place::save(&opened.tree).map_err(|e| e.to_string())
            }
            Game::File(path) => {
                std::fs::read_to_string(path).map_err(|e| format!("could not read {}: {e}", path.display()))
            }
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
    Friends,
    Play,
    Online,
    Create,
    Host,
    Avatar,
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
    pub code: String,
    pub name: String,
    pub server_port: String,
    pub servers: Vec<BackgroundServer>,
    pub error: Option<String>,
    /// How the player looks, read from the avatar file the first time it's needed.
    look: Option<Look>,
    /// The computer's "open a file" window, when it's showing.
    picker: crate::file_picker::Picker,
    /// What the Home and Friends tabs remember.
    pub home: crate::hub_home::HomeState,
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            open: false,
            tab: Tab::Home,
            game: Game::Starter(STARTER_GAMES[0].name),
            games: Vec::new(),
            scanned: false,
            path: String::new(),
            code: String::new(),
            name: "Player".into(),
            server_port: (libroxel_engine::net::DEFAULT_PORT + 1).to_string(),
            servers: Vec::new(),
            error: None,
            look: None,
            picker: crate::file_picker::Picker::default(),
            home: crate::hub_home::HomeState::default(),
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

/// A secondary block button.
fn big(ui: &mut egui::Ui, glyph: &str, text: &str) -> egui::Response {
    theme::block_button(ui, theme::Kind::Plain, glyph, text, 42.0)
}

/// The block button for the main thing to do on a page.
fn play_button(ui: &mut egui::Ui, kind: theme::Kind, glyph: &str, text: &str) -> egui::Response {
    theme::block_button(ui, kind, glyph, text, 46.0)
}

/// A section's title inside a page.
fn subheading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(theme::display(17.0)).color(theme::TEXT));
}

/// A colour for a game's picture, the same every time for the same name,
/// picked from a few bright ones that sit well on the sky.
pub(crate) fn tint(name: &str) -> Color32 {
    const TINTS: [Color32; 6] = [
        Color32::from_rgb(255, 140, 40),
        Color32::from_rgb(92, 196, 62),
        Color32::from_rgb(30, 170, 230),
        Color32::from_rgb(255, 196, 40),
        Color32::from_rgb(160, 110, 240),
        Color32::from_rgb(240, 84, 140),
    ];
    let hash = name.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    TINTS[(hash % TINTS.len() as u32) as usize]
}

/// A little stack of cubes seen from above a corner, made up from `seed`,
/// standing on a ground tile: each game's tile gets its own.
pub(crate) fn paint_diorama(
    painter: &egui::Painter,
    centre: egui::Pos2,
    unit: f32,
    seed: &str,
    colour: Color32,
) {
    let hash = seed.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    let (dx, dy) = (unit * 0.866, unit * 0.5);
    let at = |x: f32, y: f32, z: f32| egui::pos2(centre.x + (x - y) * dx, centre.y + (x + y) * dy - z * unit);
    let cube = |x: f32, y: f32, z: f32, c: Color32| {
        let top = [
            at(x, y, z + 1.0),
            at(x + 1.0, y, z + 1.0),
            at(x + 1.0, y + 1.0, z + 1.0),
            at(x, y + 1.0, z + 1.0),
        ];
        let left = [
            at(x, y + 1.0, z + 1.0),
            at(x + 1.0, y + 1.0, z + 1.0),
            at(x + 1.0, y + 1.0, z),
            at(x, y + 1.0, z),
        ];
        let right = [
            at(x + 1.0, y, z + 1.0),
            at(x + 1.0, y + 1.0, z + 1.0),
            at(x + 1.0, y + 1.0, z),
            at(x + 1.0, y, z),
        ];
        for (face, shade) in [(left, 0.78), (right, 0.55), (top, 1.0)] {
            let fill = c.gamma_multiply(shade);
            painter.add(egui::Shape::convex_polygon(face.to_vec(), fill.to_opaque(), egui::Stroke::NONE));
        }
    };
    // The ground: a flat slab.
    let ground = |x: f32, y: f32| at(x, y, 0.6);
    let slab = [ground(-1.7, -1.7), ground(1.7, -1.7), ground(1.7, 1.7), ground(-1.7, 1.7)];
    painter.add(egui::Shape::convex_polygon(
        slab.to_vec(),
        Color32::from_rgb(146, 210, 104),
        egui::Stroke::new(1.0, Color32::from_rgb(92, 160, 64)),
    ));
    // Columns of cubes drawn back to front, their heights from the seed.
    let mut cells: Vec<(i32, i32)> = (0..3).flat_map(|x| (0..3).map(move |y| (x, y))).collect();
    cells.sort_by_key(|(x, y)| x + y);
    for (gx, gy) in cells {
        {
            let bits = (hash >> ((gx * 3 + gy) * 2) as u32) & 3;
            let high = if gx == 1 && gy == 1 { 2 } else { bits.saturating_sub(1) };
            for z in 0..high {
                let shade = if z + 1 == high { colour } else { colour.lerp_to_gamma(theme::CARD, 0.35) };
                cube(gx as f32 - 1.5, gy as f32 - 1.5, 0.6 + z as f32, shade);
            }
        }
    }
}

/// A game's tile: a little scene, its name and a line under it.
fn card(ui: &mut egui::Ui, symbol: &str, title: &str, note: &str, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(196.0, 176.0), egui::Sense::click());
    let painter = ui.painter_at(rect.expand(20.0));
    let hovered = response.hovered();
    let lift = if hovered { -3.0 } else { 0.0 };
    // A frosted pane floating over the sky, lifting a little under the
    // pointer with its shadow spreading.
    let face = rect.translate(egui::vec2(0.0, lift));
    theme::soft_shadow(&painter, face, 8.0, if hovered { 80 } else { 50 });
    let colour = tint(title);
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
    theme::paint_bubbles(&painter.with_clip_rect(picture), picture);
    paint_diorama(&painter, picture.center() + egui::vec2(0.0, 8.0), 13.0, title, colour);
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
        self.servers.retain(|s| !s.finished() || !s.lines().is_empty());
        // Play as your account's name unless you picked another.
        platform.look_up_account();
        if self.name == "Player"
            && let Some(account) = &platform.account
        {
            self.name = account.name.clone();
        }
        let screen = ctx.content_rect();
        let mut ui = egui::Ui::new(
            ctx.clone(),
            "home".into(),
            egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(screen),
        );
        ui.painter().rect_filled(screen, 0.0, theme::BACK);
        theme::paint_sky(ui.painter(), screen);
        // The top bar, a sheet of sky-tinted glass: the name, the tabs and
        // who you play as.
        let bar = egui::Rect::from_min_size(screen.min, egui::vec2(screen.width(), 68.0));
        theme::glass_bar(ui.painter(), bar);
        ui.painter().hline(
            bar.x_range(),
            bar.max.y,
            egui::Stroke::new(1.0, theme::see_through(20, 70, 130, 120)),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(egui::vec2(28.0, 14.0))), |ui| {
            ui.horizontal_centered(|ui| {
                theme::wordmark(ui, 30.0);
                ui.add_space(36.0);
                ui.spacing_mut().item_spacing.x = 2.0;
                for (tab, glyph, label) in [
                    (Tab::Home, theme::icon::HOUSE, "Home"),
                    (Tab::Friends, theme::icon::USERS, "Friends"),
                    (Tab::Play, theme::icon::PLAY, "Play"),
                    (Tab::Online, theme::icon::GLOBE_HEMISPHERE_WEST, "Online"),
                    (Tab::Create, theme::icon::HAMMER, "Create"),
                    (Tab::Host, theme::icon::BROADCAST, "Host"),
                    (Tab::Avatar, theme::icon::T_SHIRT, "Avatar"),
                ] {
                    if theme::tab(ui, glyph, label, self.tab == tab).clicked() {
                        self.tab = tab;
                        self.error = None;
                        if tab == Tab::Home {
                            self.home.view = crate::hub_home::View::Home;
                        }
                    }
                }
                // Who you play as, at the right of the bar.
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_space((ui.available_width() - 236.0).max(12.0));
                theme::label_caps(ui, "Playing as");
                ui.add(
                    egui::TextEdit::singleline(&mut self.name)
                        .char_limit(20)
                        .desired_width(130.0)
                        .font(theme::display(15.0))
                        .margin(egui::vec2(8.0, 5.0)),
                );
            });
        });
        // The page sits on a big frosted pane over the sky, like a window
        // on a desktop, with the sky and hills showing round its edges.
        let pane = egui::Rect::from_min_max(egui::pos2(screen.min.x, bar.max.y), screen.max)
            .shrink2(egui::vec2(22.0, 18.0));
        theme::frosted_pane(ui.painter(), pane);
        let page = pane.shrink2(egui::vec2(22.0, 18.0));
        let mut pick = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(page), |ui| {
            egui::ScrollArea::vertical().id_salt("home-page").auto_shrink([false, false]).show(ui, |ui| {
                let name = self.name.trim().to_owned();
                pick = match self.tab {
                    Tab::Home => {
                        crate::hub_home::home_tab(&mut self.home, ui, platform, &name, &mut self.tab)
                    }
                    Tab::Friends => {
                        crate::hub_home::friends_tab(&mut self.home, ui, platform, &name, &mut self.tab)
                    }
                    Tab::Play => self.play_tab(ui),
                    Tab::Online => self.online_tab(ui, platform),
                    Tab::Create => self.create_tab(ui),
                    Tab::Host => self.host_tab(ui, platform),
                    Tab::Avatar => {
                        egui::ScrollArea::vertical()
                            .id_salt("avatar")
                            .show(ui, |ui| self.avatar_tab(ui, platform));
                        None
                    }
                };
                if let Some(error) = &self.error {
                    ui.add_space(8.0);
                    theme::status(ui, theme::RED, error);
                }
            });
        });
        platform.name = self.name.trim().to_owned();
        if pick.is_some() {
            self.error = None;
        }
        pick
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

    fn avatar_tab(&mut self, ui: &mut egui::Ui, platform: &mut Platform) {
        let mut look = self.look();
        theme::heading(ui, "Your avatar");
        ui.label(
            RichText::new("How you look in every game you play or join. Other players see it too.")
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
                    theme::label_caps(ui, label);
                    swatches(ui, color, label);
                    ui.add_space(8.0);
                }
                theme::label_caps(ui, "Hat");
                ui.horizontal_wrapped(|ui| {
                    for hat in Hat::ALL {
                        let on = look.hat == hat && look.item.is_none();
                        if ui.selectable_label(on, hat.name()).clicked() {
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
                if ui.button("Back to the usual look").clicked() {
                    look = Look::for_player(0);
                }
            });
        });
        ui.add_space(24.0);
        if let Some(picked) = platform.catalog_ui(ui, look.item.as_deref()) {
            look.item = picked.map(std::sync::Arc::new);
        }
        ui.add_space(24.0);
        platform.account_ui(ui, look.to_json());
        if Some(&look) != self.look.as_ref() {
            let json = look.to_json().to_string();
            self.look = Some(look);
            let saved = std::fs::create_dir_all(data_dir()).and_then(|_| std::fs::write(avatar_file(), json));
            if let Err(e) = saved {
                self.error = Some(format!("Could not save your avatar: {e}"));
            }
        }
    }

    /// The games on this computer, as tiles to pick from.
    fn game_tiles(&mut self, ui: &mut egui::Ui, with_blank: bool) -> Option<Game> {
        let mut chosen = None;
        let mut tiles: Vec<_> =
            STARTER_GAMES.iter().map(|g| (Game::Starter(g.name), g.symbol, g.about.to_owned())).collect();
        if with_blank {
            tiles.insert(0, (Game::Blank, theme::icon::PLUS, "Start from an empty baseplate".to_owned()));
        }
        for path in &self.games {
            tiles.push((Game::File(path.clone()), theme::icon::FLOPPY_DISK, "Saved on this computer".into()));
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(16.0, 16.0);
            for (game, symbol, note) in tiles {
                let selected = !with_blank && self.game == game;
                let tile = card(ui, symbol, &game.label(), &note, selected);
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

    fn play_tab(&mut self, ui: &mut egui::Ui) -> Option<Pick> {
        let mut pick = None;
        theme::heading(ui, "Your games");
        ui.label(RichText::new("Pick one and press Play. Double-click to jump straight in.").color(SOFT));
        ui.add_space(12.0);
        if let Some(game) = self.game_tiles(ui, false) {
            pick = Some(Pick::Play(game));
        }
        if self.games.is_empty() {
            ui.add_space(8.0);
            ui.label(
                RichText::new("Games you make in Create show up here once you save them.")
                    .color(SOFT)
                    .italics(),
            );
        }
        ui.add_space(20.0);
        ui.horizontal(|ui| {
            let label = format!("Play {}", self.game.label());
            if play_button(ui, theme::Kind::Go, theme::icon::PLAY, &label).clicked() {
                pick = Some(Pick::Play(self.game.clone()));
            }
            ui.add_space(12.0);
            if big(ui, theme::icon::USERS_THREE, "Play with friends")
                .on_hover_text("Host it, so friends can join with a code or find it Online")
                .clicked()
            {
                pick = Some(Pick::Host(self.game.clone()));
            }
        });
        pick
    }

    fn online_tab(&mut self, ui: &mut egui::Ui, platform: &mut Platform) -> Option<Pick> {
        egui::ScrollArea::vertical().id_salt("online").show(ui, |ui| self.online_inner(ui, platform)).inner
    }

    fn online_inner(&mut self, ui: &mut egui::Ui, platform: &mut Platform) -> Option<Pick> {
        let mut pick = None;
        if let Some(done) = platform.take_download() {
            let game = Game::File(done.path);
            pick = Some(if done.host { Pick::Host(game) } else { Pick::Play(game) });
        }
        theme::heading(ui, "Published games");
        ui.label(
            RichText::new("Games people have published. They play without anyone hosting; press With friends to host one yourself.")
                .color(SOFT),
        );
        ui.add_space(10.0);
        theme::panel().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            platform.published_list(ui);
        });
        ui.add_space(20.0);
        theme::heading(ui, "Games being played right now");
        ui.label(RichText::new("Hosted games appear here. Press Join to play in one.").color(SOFT));
        ui.add_space(10.0);
        theme::panel().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if let Some((code, _)) = platform.list(ui) {
                pick = Some(Pick::Join { code, name: self.name.trim().to_owned() });
            }
            ui.horizontal(|ui| {
                if theme::tool_button(ui, theme::icon::ARROW_CLOCKWISE, "Refresh", false).clicked() {
                    platform.refresh();
                }
                ui.label(RichText::new("Listing from the Hub at").color(SOFT));
                ui.add(egui::TextEdit::singleline(&mut platform.hub_address).desired_width(160.0));
            });
        });
        ui.add_space(20.0);
        subheading(ui, "Join with a code");
        ui.label(RichText::new("A friend who hosts can send you their join code. On your own network an address like 192.168.1.5:7420 works too.").color(SOFT));
        ui.add_space(6.0);
        ui.add(
            egui::TextEdit::multiline(&mut self.code)
                .hint_text("Paste the code here")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        ui.add_space(6.0);
        let ready = !self.code.trim().is_empty();
        if ui
            .add_enabled_ui(ready, |ui| play_button(ui, theme::Kind::Primary, theme::icon::SIGN_IN, "Join"))
            .inner
            .clicked()
        {
            pick = Some(Pick::Join { code: self.code.trim().to_owned(), name: self.name.trim().to_owned() });
        }
        pick
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
                    Some("No file picker could be opened here. Type the file's path instead.".into());
            }
            Some(crate::file_picker::Picked::Cancelled) | None => {}
        }
        theme::heading(ui, "Make a game");
        ui.label(RichText::new("Opens the editor: build with parts, scripts and terrain, press Play to test, and save it to play from Play.").color(SOFT));
        ui.add_space(12.0);
        if let Some(game) = self.game_tiles(ui, true) {
            pick = Some(Pick::Edit(game));
        }
        ui.add_space(16.0);
        ui.label(format!("Games are saved in {}", games_dir().display()));
        ui.horizontal(|ui| {
            let typed = ui.add(
                egui::TextEdit::singleline(&mut self.path)
                    .hint_text("or open a .lrx, .rbxl or .rbxlx file by its path")
                    .desired_width(320.0),
            );
            let enter = typed.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let typed_path = !self.path.trim().is_empty();
            let open =
                ui.button("Open").on_hover_text("Open the typed path, or pick a file from your computer");
            let browse = ui
                .add_enabled(!self.picker.is_open(), egui::Button::new("Browse…"))
                .on_hover_text("Pick a game file from your computer");
            if (open.clicked() || enter) && typed_path {
                pick = Some(Pick::Edit(Game::File(PathBuf::from(self.path.trim()))));
            } else if browse.clicked() || open.clicked() {
                // With nothing typed, Open browses too.
                self.picker.open(&games_dir());
            }
            if theme::tool_button(ui, theme::icon::ARROW_CLOCKWISE, "", false)
                .on_hover_text("Look for saved games again")
                .clicked()
            {
                self.scanned = false;
            }
        });
        pick
    }

    fn host_tab(&mut self, ui: &mut egui::Ui, platform: &mut Platform) -> Option<Pick> {
        let mut pick = None;
        theme::heading(ui, "Share a game");
        ui.horizontal(|ui| {
            ui.label("Game");
            egui::ComboBox::from_id_salt("host-game").selected_text(self.game.label()).show_ui(ui, |ui| {
                for game in STARTER_GAMES {
                    ui.selectable_value(&mut self.game, Game::Starter(game.name), game.name);
                }
                for path in self.games.clone() {
                    let game = Game::File(path);
                    let label = game.label();
                    ui.selectable_value(&mut self.game, game, label);
                }
            });
        });
        ui.add_space(10.0);
        theme::panel().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            subheading(ui, "Host and play");
            ui.label(
                RichText::new("You play it and friends join you, with a join code or from Online.")
                    .color(SOFT),
            );
            ui.add_space(6.0);
            if play_button(ui, theme::Kind::Primary, theme::icon::BROADCAST, "Host and play").clicked() {
                pick = Some(Pick::Host(self.game.clone()));
            }
        });
        ui.add_space(12.0);
        theme::panel().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            self.server_section(ui);
        });
        ui.add_space(12.0);
        theme::panel().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            hub_section(ui, platform);
        });
        pick
    }

    fn server_section(&mut self, ui: &mut egui::Ui) {
        subheading(ui, "Run a server");
        ui.label(
            RichText::new("Runs the game in the background with no player of your own, so others can join while you play something else.")
                .color(SOFT),
        );
        ui.horizontal(|ui| {
            ui.label("Port");
            ui.add(egui::TextEdit::singleline(&mut self.server_port).desired_width(60.0));
            if big(ui, theme::icon::HARD_DRIVES, "Start server").clicked() {
                match self.start_server() {
                    Ok(server) => self.servers.push(server),
                    Err(e) => self.error = Some(e),
                }
            }
        });
        let mut stop = None;
        for (i, server) in self.servers.iter().enumerate() {
            ui.add_space(6.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    let state = if server.finished() { "stopped" } else { "running" };
                    ui.label(
                        RichText::new(format!("{} on port {} ({state})", server.game, server.port)).strong(),
                    );
                    let label = if server.finished() { "Dismiss" } else { "Stop" };
                    if ui.small_button(label).clicked() {
                        stop = Some(i);
                    }
                });
                let lines = server.lines();
                let code = lines.iter().position(|l| l.starts_with("Join code"));
                if let Some(code) = code.and_then(|i| lines[i].split("\n\n").nth(1)).map(str::trim) {
                    ui.horizontal(|ui| {
                        let short: String = code.chars().take(24).collect();
                        ui.label(RichText::new(format!("Join code {short}…")).monospace())
                            .on_hover_text(code);
                        if ui.small_button("Copy").clicked() {
                            ui.ctx().copy_text(code.to_owned());
                        }
                    });
                }
                for line in lines.iter().rev().take(3).rev() {
                    ui.label(RichText::new(line.lines().next().unwrap_or("")).small().color(SOFT));
                }
            });
        }
        if let Some(i) = stop {
            let server = self.servers.remove(i);
            server.stop();
        }
    }

    fn start_server(&self) -> Result<BackgroundServer, String> {
        let port: u16 =
            self.server_port.trim().parse().map_err(|_| "The port needs to be a number.".to_owned())?;
        if self.servers.iter().any(|s| s.port == port && !s.finished()) {
            return Err(format!("A server already uses port {port}."));
        }
        let key = match &self.game {
            Game::File(path) => path.with_file_name("server-identity.key"),
            _ => data_dir().join("server-identity.key"),
        };
        let game = self.game.label();
        Ok(BackgroundServer::start(ServerSettings {
            name: format!("{game} server"),
            game,
            text: self.game.text()?,
            key,
            port,
            hub: HubUse::usual(),
            official: false,
            max_players: 0,
            game_id: crate::social::game_ref(&self.game).game_id,
        }))
    }
}

fn hub_section(ui: &mut egui::Ui, platform: &mut Platform) {
    subheading(ui, "This computer's Hub");
    ui.label(
        RichText::new("The Hub is the list Online shows. Friends on your network can point their Online tab at this computer's address to see the games hosted here.")
            .color(SOFT),
    );
    ui.add_space(4.0);
    match platform.own_hub.as_ref().map(|hub| hub.addr().port()) {
        Some(port) => {
            ui.horizontal(|ui| {
                theme::status(ui, theme::GREEN, &format!("Running on port {port}"));
                if ui.button("Stop the Hub").clicked() {
                    platform.stop_hub();
                }
            });
        }
        None => {
            ui.horizontal(|ui| {
                let note = platform.own_hub_note.clone().unwrap_or_default();
                theme::status(ui, theme::MUTED, &format!("Not running here. {note}"));
                if ui.button("Run a Hub").clicked() {
                    platform.start_hub();
                }
            });
        }
    }
}

/// Where the avatar is saved.
fn avatar_file() -> PathBuf {
    data_dir().join("avatar.json")
}

/// Colours to pick from on the Avatar tab, for skin and for clothes.
const SKINS: [[u8; 3]; 8] = [
    [255, 224, 196],
    [238, 196, 160],
    [222, 172, 128],
    [196, 140, 96],
    [160, 108, 72],
    [118, 78, 52],
    [86, 56, 38],
    [120, 200, 120],
];
const CLOTHES: [[u8; 3]; 16] = [
    [236, 98, 74],
    [242, 170, 40],
    [250, 220, 70],
    [120, 200, 80],
    [32, 168, 150],
    [70, 150, 230],
    [50, 56, 84],
    [124, 92, 214],
    [226, 86, 150],
    [240, 240, 240],
    [150, 150, 160],
    [40, 40, 46],
    [120, 80, 50],
    [180, 40, 50],
    [20, 90, 60],
    [250, 200, 60],
];

/// A row of colour squares; the chosen one is outlined. The last square
/// opens a picker for any colour.
fn swatches(ui: &mut egui::Ui, color: &mut Color3, label: &str) {
    let choices: &[[u8; 3]] = if label == "Skin" { &SKINS } else { &CLOTHES };
    let rgb = |c: Color3| {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [byte(c.r), byte(c.g), byte(c.b)]
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for [r, g, b] in choices.iter().copied() {
            let (rect, response) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
            let chosen = rgb(*color) == [r, g, b];
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
    let (rect, _) = ui.allocate_exact_size(egui::vec2(220.0, 300.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let scale = 42.0;
    // The hips are the middle of the character; up is up the screen.
    let hips = egui::pos2(rect.center().x, rect.min.y + 175.0);
    let at = |x: f32, y: f32| egui::pos2(hips.x + x * scale, hips.y - y * scale);
    let colour = |c: Color3| {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color32::from_rgb(byte(c.r), byte(c.g), byte(c.b))
    };
    let shade = |c: Color32| c.gamma_multiply(0.82);
    let block = |x: f32, y: f32, w: f32, h: f32, c: Color32| {
        let r = egui::Rect::from_center_size(at(x, y), egui::vec2(w * scale, h * scale));
        painter.rect_filled(r, 1.0, c);
        painter.rect_stroke(r, 1.0, egui::Stroke::new(1.5, shade(c)), egui::StrokeKind::Inside);
    };
    let BodyColors { skin, shirt, pants } = look.colors;
    let (skin, shirt, pants) = (colour(skin), colour(shirt), colour(pants));
    block(-0.5, -1.0, 0.95, 2.0, pants);
    block(0.5, -1.0, 0.95, 2.0, pants);
    block(-1.5, 0.85, 0.8, 1.9, skin);
    block(1.5, 0.85, 0.8, 1.9, skin);
    block(0.0, 0.9, 2.2, 1.8, shirt);
    block(0.0, 2.55, 1.6, 1.4, skin);
    let eye = Color32::from_rgb(30, 30, 40);
    block(-0.35, 2.65, 0.25, 0.35, eye);
    block(0.35, 2.65, 0.25, 0.35, eye);
    if let Some(item) = &look.item {
        // Seen from the front, sorted back to front so nearer parts cover farther ones.
        let mut parts: Vec<_> = item.parts.iter().collect();
        parts.sort_by(|a, b| b.offset.z.total_cmp(&a.offset.z));
        for part in parts {
            let sideways = (part.turn.z.abs() - 90.0).abs() < 1.0;
            let (w, h) = if sideways { (part.size.y, part.size.x) } else { (part.size.x, part.size.y) };
            let (x, y) = (part.offset.x, 2.55 + part.offset.y);
            if part.shape == libroxel_instance::PartShape::Ball {
                painter.circle_filled(at(x, y), w * scale / 2.0, colour(part.color));
            } else {
                block(x, y, w, h, colour(part.color));
            }
        }
    }
    let hat = if look.item.is_some() { libroxel_engine::character::Hat::None } else { look.hat };
    for (_, size, offset, turn, shape, _, tinted) in hat.parts() {
        let c = if *tinted { colour(look.hat_color) } else { Color32::from_rgb(35, 35, 42) };
        // A part turned on its side shows its other face.
        let (w, h) = if *turn != 0.0 { (size[1], size[0]) } else { (size[0], size[1]) };
        let (x, y) = (offset[0], 2.55 + offset[1]);
        if *shape == libroxel_instance::PartShape::Ball {
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
