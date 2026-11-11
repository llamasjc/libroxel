//! The main menu: what the app shows when it starts. From here you edit a
//! game, play it, host it, join someone else's, browse hosted games, run a
//! server in the background and turn this computer's Hub on or off.

use std::path::PathBuf;

use bevy_egui::egui::{self, Color32, RichText};
use libroxel_instance::place;

use crate::browse::Platform;
use crate::server::{BackgroundServer, HubUse, ServerSettings, data_dir};
use crate::{games_dir, templates};

/// Which game the menu's buttons use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Game {
    Sample,
    Blank,
    File(PathBuf),
}

impl Game {
    pub fn label(&self) -> String {
        match self {
            Game::Sample => "Sample obby".into(),
            Game::Blank => "New empty world".into(),
            Game::File(path) => path
                .file_stem()
                .map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned()),
        }
    }

    /// The game as a saved place, for a server.
    fn text(&self) -> Result<String, String> {
        match self {
            Game::Sample => place::save(&templates::sample_obby()).map_err(|e| e.to_string()),
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

/// What the menu remembers.
pub struct Menu {
    pub open: bool,
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
            game: Game::Sample,
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

fn big(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).size(18.0)).min_size(egui::vec2(220.0, 40.0))
}

impl Menu {
    /// Draws the menu over the whole window. Returns what was picked.
    pub fn show(&mut self, ctx: &egui::Context, platform: &mut Platform) -> Option<Pick> {
        if !self.scanned {
            self.games = saved_games();
            self.scanned = true;
        }
        self.servers.retain(|s| !s.finished() || !s.lines().is_empty());
        let screen = ctx.content_rect();
        let mut pick = None;
        let mut ui = egui::Ui::new(
            ctx.clone(),
            "main-menu".into(),
            egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(screen),
        );
        ui.painter().rect_filled(screen, 0.0, Color32::from_rgb(24, 28, 38));
        let width = (screen.width() - 64.0).min(1120.0);
        let area = egui::Rect::from_min_size(
            egui::pos2(screen.center().x - width / 2.0, screen.min.y + 32.0),
            egui::vec2(width, screen.height() - 64.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Libroxel").size(44.0).strong().color(Color32::from_rgb(255, 200, 80)),
                );
                ui.label(RichText::new("Build games, play them, and play with friends.").size(16.0));
            });
            ui.add_space(24.0);
            egui::ScrollArea::vertical().id_salt("menu-scroll").show(ui, |ui| {
                ui.columns(3, |cols| {
                    self.game_column(&mut cols[0]);
                    pick = pick.take().or(self.play_column(&mut cols[1]));
                    pick = pick.take().or(self.online_column(&mut cols[2], platform));
                });
            });
        });
        // Picking a game file and pressing Enter in the path box both work.
        if pick.is_some() {
            self.error = None;
        }
        pick
    }

    fn game_column(&mut self, ui: &mut egui::Ui) {
        ui.heading("Game");
        ui.label(RichText::new("Pick what the buttons open.").small());
        ui.add_space(4.0);
        ui.radio_value(&mut self.game, Game::Sample, "🏁 Sample obby");
        ui.radio_value(&mut self.game, Game::Blank, "⬜ New empty world");
        ui.add_space(6.0);
        ui.label(format!("Saved in {}", games_dir().display()));
        egui::ScrollArea::vertical().id_salt("menu-games").max_height(260.0).show(ui, |ui| {
            if self.games.is_empty() {
                ui.label(RichText::new("No saved games yet: Edit one and save it.").small().italics());
            }
            for path in self.games.clone() {
                let game = Game::File(path.clone());
                ui.radio_value(&mut self.game, game.clone(), format!("📄 {}", game.label()))
                    .on_hover_text(path.display().to_string());
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let typed = ui.add(
                egui::TextEdit::singleline(&mut self.path)
                    .hint_text("or a path to a .lrx file")
                    .desired_width(200.0),
            );
            if (ui.button("Use").clicked()
                || typed.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                && !self.path.trim().is_empty()
            {
                self.game = Game::File(PathBuf::from(self.path.trim()));
            }
            if ui.button("⟳").on_hover_text("Look for saved games again").clicked() {
                self.scanned = false;
            }
        });
    }

    fn play_column(&mut self, ui: &mut egui::Ui) -> Option<Pick> {
        let mut pick = None;
        ui.heading(self.game.label());
        ui.add_space(8.0);
        if ui.add(big("✏ Edit")).on_hover_text("Build it: add parts, scripts and terrain").clicked() {
            pick = Some(Pick::Edit(self.game.clone()));
        }
        ui.add_space(4.0);
        if ui.add(big("▶ Play")).on_hover_text("Play it on your own").clicked() {
            pick = Some(Pick::Play(self.game.clone()));
        }
        ui.add_space(4.0);
        if ui
            .add(big("🌐 Host and play"))
            .on_hover_text("Play it and let friends join, with a join code and in Browse")
            .clicked()
        {
            pick = Some(Pick::Host(self.game.clone()));
        }
        ui.add_space(14.0);
        ui.separator();
        ui.label(RichText::new("Server").strong());
        ui.label(
            RichText::new("Runs the game here with no player of your own, so others can join while you edit or play something else.")
                .small(),
        );
        ui.horizontal(|ui| {
            ui.label("Port");
            ui.add(egui::TextEdit::singleline(&mut self.server_port).desired_width(60.0));
            if ui.button("🖥 Start server").clicked() {
                match self.start_server() {
                    Ok(server) => self.servers.push(server),
                    Err(e) => self.error = Some(e),
                }
            }
        });
        let mut stop = None;
        for (i, server) in self.servers.iter().enumerate() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
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
                        let short: String = code.chars().take(18).collect();
                        ui.label(RichText::new(format!("Join code {short}…")).monospace().small())
                            .on_hover_text(code);
                        if ui.small_button("Copy").clicked() {
                            ui.ctx().copy_text(code.to_owned());
                        }
                    });
                }
                for line in lines.iter().rev().take(3).rev() {
                    ui.label(RichText::new(line.lines().next().unwrap_or("")).small());
                }
            });
        }
        if let Some(i) = stop {
            let server = self.servers.remove(i);
            server.stop();
        }
        if let Some(error) = &self.error {
            ui.colored_label(Color32::from_rgb(255, 120, 110), error);
        }
        pick
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

    fn online_column(&mut self, ui: &mut egui::Ui, platform: &mut Platform) -> Option<Pick> {
        let mut pick = None;
        ui.heading("Play with friends");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Your name");
            ui.add(egui::TextEdit::singleline(&mut self.name).char_limit(20).desired_width(140.0));
        });
        ui.add(
            egui::TextEdit::multiline(&mut self.code)
                .hint_text("Paste a join code (or an address like 192.168.1.5:7420)")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        let ready = !self.code.trim().is_empty();
        if ui.add_enabled(ready, big("🔗 Join")).clicked() {
            pick = Some(Pick::Join { code: self.code.trim().to_owned(), name: self.name.trim().to_owned() });
        }
        ui.add_space(4.0);
        if ui.add(big("🌍 Browse games")).on_hover_text("See the games people are hosting").clicked() {
            platform.name = self.name.trim().to_owned();
            pick = Some(Pick::Browse);
        }
        ui.add_space(14.0);
        ui.separator();
        ui.label(RichText::new("Hub").strong());
        ui.label(
            RichText::new("The Hub lists hosted games so people can find them in Browse. Friends on your network can use this computer's address as their Hub.")
                .small(),
        );
        match &platform.own_hub {
            Some(hub) => {
                ui.label(format!("✅ Running on this computer, port {}", hub.addr().port()));
                if ui.button("Stop the Hub").clicked() {
                    platform.stop_hub();
                }
            }
            None => {
                let note = platform.own_hub_note.clone().unwrap_or_default();
                ui.label(format!("⏸ Not running here. {note}"));
                if ui.button("Run a Hub").clicked() {
                    platform.start_hub();
                }
            }
        }
        ui.horizontal(|ui| {
            ui.label("Browse uses");
            ui.add(egui::TextEdit::singleline(&mut platform.hub_address).desired_width(150.0));
        });
        pick
    }
}
