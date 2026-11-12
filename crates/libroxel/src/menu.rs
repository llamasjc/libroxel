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
use crate::{games_dir, templates};

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
    Play,
    Online,
    Create,
    Host,
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
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            open: false,
            tab: Tab::Play,
            game: Game::Starter(STARTER_GAMES[0].name),
            games: Vec::new(),
            scanned: false,
            path: String::new(),
            code: String::new(),
            name: "Player".into(),
            server_port: (libroxel_engine::net::DEFAULT_PORT + 1).to_string(),
            servers: Vec::new(),
            error: None,
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
            } else if path.extension().is_some_and(|e| e == "lrx") {
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

const BACKGROUND: Color32 = Color32::from_rgb(20, 23, 32);
const PANEL: Color32 = Color32::from_rgb(31, 36, 50);
const ACCENT: Color32 = Color32::from_rgb(255, 196, 72);
const SOFT: Color32 = Color32::from_rgb(160, 168, 190);

fn big(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).size(18.0).strong()).min_size(egui::vec2(200.0, 44.0))
}

fn play_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).size(20.0).strong().color(Color32::from_rgb(20, 23, 32)))
        .fill(ACCENT)
        .min_size(egui::vec2(200.0, 48.0))
}

/// A colour for a game's picture, the same every time for the same name.
fn tint(name: &str) -> Color32 {
    let hash = name.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    let hue = (hash % 360) as f32 / 360.0;
    egui::ecolor::Hsva::new(hue, 0.55, 0.75, 1.0).into()
}

/// A game's tile: a picture with a big symbol, its name and a line under it.
fn card(ui: &mut egui::Ui, symbol: &str, title: &str, note: &str, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(184.0, 150.0), egui::Sense::click());
    let painter = ui.painter_at(rect.expand(3.0));
    let lift = if response.hovered() { 6 } else { 0 };
    painter.rect_filled(rect, 10.0, Color32::from_rgb(38 + lift, 44 + lift, 60 + lift));
    let picture = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 100.0));
    let colour = tint(title);
    painter.rect_filled(picture, egui::CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 }, colour);
    // A few blocks along the bottom of the picture, for a bit of world.
    for i in 0..6 {
        let x = picture.min.x + 8.0 + i as f32 * 29.0;
        let h = 10.0 + ((title.len() * 7 + i * 13) % 22) as f32;
        let block =
            egui::Rect::from_min_max(egui::pos2(x, picture.max.y - h), egui::pos2(x + 26.0, picture.max.y));
        painter.rect_filled(block, 2.0, colour.gamma_multiply(0.6));
    }
    painter.text(
        picture.center(),
        egui::Align2::CENTER_CENTER,
        symbol,
        egui::FontId::proportional(40.0),
        Color32::WHITE,
    );
    painter.text(
        egui::pos2(rect.min.x + 10.0, picture.max.y + 8.0),
        egui::Align2::LEFT_TOP,
        title,
        egui::FontId::proportional(15.0),
        Color32::WHITE,
    );
    painter.text(
        egui::pos2(rect.min.x + 10.0, picture.max.y + 28.0),
        egui::Align2::LEFT_TOP,
        note,
        egui::FontId::proportional(11.0),
        SOFT,
    );
    if selected || response.hovered() {
        let stroke =
            egui::Stroke::new(if selected { 3.0 } else { 1.5 }, if selected { ACCENT } else { SOFT });
        painter.rect_stroke(rect, 10.0, stroke, egui::StrokeKind::Outside);
    }
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
        let screen = ctx.content_rect();
        let mut ui = egui::Ui::new(
            ctx.clone(),
            "home".into(),
            egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(screen),
        );
        ui.painter().rect_filled(screen, 0.0, BACKGROUND);
        // The top bar: the name, the tabs and who you play as.
        let bar = egui::Rect::from_min_size(screen.min, egui::vec2(screen.width(), 64.0));
        ui.painter().rect_filled(bar, 0.0, PANEL);
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(egui::vec2(24.0, 12.0))), |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new("▣ Libroxel").size(28.0).strong().color(ACCENT));
                ui.add_space(28.0);
                for (tab, label) in [
                    (Tab::Play, "▶ Play"),
                    (Tab::Online, "🌍 Online"),
                    (Tab::Create, "✏ Create"),
                    (Tab::Host, "🌐 Host"),
                ] {
                    let on = self.tab == tab;
                    let text =
                        RichText::new(label).size(17.0).color(if on { ACCENT } else { Color32::WHITE });
                    if ui.add(egui::Button::new(text).frame(on).min_size(egui::vec2(96.0, 36.0))).clicked() {
                        self.tab = tab;
                        self.error = None;
                    }
                }
                // Who you play as, at the right of the bar.
                ui.add_space((ui.available_width() - 200.0).max(12.0));
                ui.label(RichText::new("Playing as").color(SOFT));
                ui.add(egui::TextEdit::singleline(&mut self.name).char_limit(20).desired_width(120.0));
            });
        });
        let page = egui::Rect::from_min_max(egui::pos2(screen.min.x, bar.max.y), screen.max).shrink(28.0);
        let mut pick = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(page), |ui| {
            egui::ScrollArea::vertical().id_salt("home-page").auto_shrink([false, false]).show(ui, |ui| {
                pick = match self.tab {
                    Tab::Play => self.play_tab(ui),
                    Tab::Online => self.online_tab(ui, platform),
                    Tab::Create => self.create_tab(ui),
                    Tab::Host => self.host_tab(ui, platform),
                };
                if let Some(error) = &self.error {
                    ui.add_space(8.0);
                    ui.colored_label(Color32::from_rgb(255, 120, 110), error);
                }
            });
        });
        platform.name = self.name.trim().to_owned();
        if pick.is_some() {
            self.error = None;
        }
        pick
    }

    /// The games on this computer, as tiles to pick from.
    fn game_tiles(&mut self, ui: &mut egui::Ui, with_blank: bool) -> Option<Game> {
        let mut chosen = None;
        let mut tiles: Vec<_> =
            STARTER_GAMES.iter().map(|g| (Game::Starter(g.name), g.symbol, g.about.to_owned())).collect();
        if with_blank {
            tiles.insert(0, (Game::Blank, "➕", "Start from an empty baseplate".to_owned()));
        }
        for path in &self.games {
            tiles.push((Game::File(path.clone()), "🎮", "Saved on this computer".into()));
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
        ui.label(RichText::new("Your games").size(22.0).strong());
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
            if ui.add(play_button(&format!("▶  Play {}", self.game.label()))).clicked() {
                pick = Some(Pick::Play(self.game.clone()));
            }
            ui.add_space(12.0);
            if ui
                .add(big("🌐 Play with friends"))
                .on_hover_text("Host it, so friends can join with a code or find it Online")
                .clicked()
            {
                pick = Some(Pick::Host(self.game.clone()));
            }
        });
        pick
    }

    fn online_tab(&mut self, ui: &mut egui::Ui, platform: &mut Platform) -> Option<Pick> {
        let mut pick = None;
        ui.label(RichText::new("Games being played right now").size(22.0).strong());
        ui.label(RichText::new("Hosted games appear here. Press Join to play in one.").color(SOFT));
        ui.add_space(10.0);
        egui::Frame::new().fill(PANEL).corner_radius(10.0).inner_margin(14.0).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if let Some((code, _)) = platform.list(ui) {
                pick = Some(Pick::Join { code, name: self.name.trim().to_owned() });
            }
            ui.horizontal(|ui| {
                if ui.button("⟳ Refresh").clicked() {
                    platform.refresh();
                }
                ui.label(RichText::new("Listing from the Hub at").color(SOFT));
                ui.add(egui::TextEdit::singleline(&mut platform.hub_address).desired_width(160.0));
            });
        });
        ui.add_space(20.0);
        ui.label(RichText::new("Join with a code").size(18.0).strong());
        ui.label(RichText::new("A friend who hosts can send you their join code. On your own network an address like 192.168.1.5:7420 works too.").color(SOFT));
        ui.add_space(6.0);
        ui.add(
            egui::TextEdit::multiline(&mut self.code)
                .hint_text("Paste the code here")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        ui.add_space(6.0);
        if ui.add_enabled(!self.code.trim().is_empty(), play_button("🔗  Join")).clicked() {
            pick = Some(Pick::Join { code: self.code.trim().to_owned(), name: self.name.trim().to_owned() });
        }
        pick
    }

    fn create_tab(&mut self, ui: &mut egui::Ui) -> Option<Pick> {
        let mut pick = None;
        ui.label(RichText::new("Make a game").size(22.0).strong());
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
                    .hint_text("or open a .lrx file by its path")
                    .desired_width(320.0),
            );
            let enter = typed.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Open").clicked() || enter) && !self.path.trim().is_empty() {
                pick = Some(Pick::Edit(Game::File(PathBuf::from(self.path.trim()))));
            }
            if ui.button("⟳").on_hover_text("Look for saved games again").clicked() {
                self.scanned = false;
            }
        });
        pick
    }

    fn host_tab(&mut self, ui: &mut egui::Ui, platform: &mut Platform) -> Option<Pick> {
        let mut pick = None;
        ui.label(RichText::new("Share a game").size(22.0).strong());
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
        egui::Frame::new().fill(PANEL).corner_radius(10.0).inner_margin(14.0).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new("Host and play").size(17.0).strong());
            ui.label(
                RichText::new("You play it and friends join you, with a join code or from Online.")
                    .color(SOFT),
            );
            ui.add_space(6.0);
            if ui.add(play_button("🌐  Host and play")).clicked() {
                pick = Some(Pick::Host(self.game.clone()));
            }
        });
        ui.add_space(12.0);
        egui::Frame::new().fill(PANEL).corner_radius(10.0).inner_margin(14.0).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            self.server_section(ui);
        });
        ui.add_space(12.0);
        egui::Frame::new().fill(PANEL).corner_radius(10.0).inner_margin(14.0).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            hub_section(ui, platform);
        });
        pick
    }

    fn server_section(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Run a server").size(17.0).strong());
        ui.label(
            RichText::new("Runs the game in the background with no player of your own, so others can join while you play something else.")
                .color(SOFT),
        );
        ui.horizontal(|ui| {
            ui.label("Port");
            ui.add(egui::TextEdit::singleline(&mut self.server_port).desired_width(60.0));
            if ui.add(big("🖥 Start server")).clicked() {
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
            hub: HubUse::Own,
            official: false,
            max_players: 0,
        }))
    }
}

fn hub_section(ui: &mut egui::Ui, platform: &mut Platform) {
    ui.label(RichText::new("This computer's Hub").size(17.0).strong());
    ui.label(
        RichText::new("The Hub is the list Online shows. Friends on your network can point their Online tab at this computer's address to see the games hosted here.")
            .color(SOFT),
    );
    ui.add_space(4.0);
    match platform.own_hub.as_ref().map(|hub| hub.addr().port()) {
        Some(port) => {
            ui.horizontal(|ui| {
                ui.label(format!("✅ Running on port {port}"));
                if ui.button("Stop the Hub").clicked() {
                    platform.stop_hub();
                }
            });
        }
        None => {
            ui.horizontal(|ui| {
                let note = platform.own_hub_note.clone().unwrap_or_default();
                ui.label(format!("⏸ Not running here. {note}"));
                if ui.button("Run a Hub").clicked() {
                    platform.start_hub();
                }
            });
        }
    }
}
