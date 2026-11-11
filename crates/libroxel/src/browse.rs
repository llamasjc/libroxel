//! The editor's side of the Hub: a built-in Hub, the Browse window, and
//! listing a hosted game so others can find it.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy_egui::egui::{self, Color32, RichText};
use libroxel_hub::{Announcer, HostKind, HubClient, Listing, RunningHub, ServerInfo};

use crate::server::data_dir;

type Fetched = Arc<Mutex<Option<Result<Vec<ServerInfo>, String>>>>;

/// Everything about the Hub the editor keeps between frames.
pub struct Platform {
    /// The Hub this editor runs for its network, if it could start one.
    pub own_hub: Option<RunningHub>,
    /// Why the built-in Hub is not running, if it is not.
    pub own_hub_note: Option<String>,
    /// The Hub that Browse lists and Host announces to.
    pub hub_address: String,
    pub open: bool,
    pub search: String,
    pub name: String,
    servers: Vec<ServerInfo>,
    error: Option<String>,
    fetched: Fetched,
    fetching: bool,
    last_fetch: Option<Instant>,
    /// Keeps a hosted game on the list.
    pub announcer: Option<Announcer>,
}

impl Default for Platform {
    fn default() -> Self {
        Self {
            own_hub: None,
            own_hub_note: None,
            hub_address: format!("127.0.0.1:{}", libroxel_hub::DEFAULT_PORT),
            open: false,
            search: String::new(),
            name: "Player".into(),
            servers: Vec::new(),
            error: None,
            fetched: Arc::default(),
            fetching: false,
            last_fetch: None,
            announcer: None,
        }
    }
}

impl Platform {
    /// Starts the built-in Hub, unless `LIBROXEL_HUB` names another one.
    pub fn start() -> Self {
        let mut platform = Self::default();
        if let Some(address) = std::env::var("LIBROXEL_HUB").ok().filter(|a| !a.trim().is_empty()) {
            platform.hub_address = address;
            return platform;
        }
        platform.start_hub();
        platform
    }

    /// Runs a Hub on this computer, if it isn't already.
    pub fn start_hub(&mut self) {
        if self.own_hub.is_some() {
            return;
        }
        let addr = SocketAddr::from(([0, 0, 0, 0], libroxel_hub::DEFAULT_PORT));
        match libroxel_hub::start(addr, Some(&data_dir().join("hub.sqlite"))) {
            Ok(hub) => {
                self.own_hub = Some(hub);
                self.own_hub_note = None;
            }
            // Usually another Libroxel on this computer already runs one,
            // and that Hub works just as well.
            Err(e) => self.own_hub_note = Some(e),
        }
    }

    /// Stops the Hub this app runs (games listed on it drop off).
    pub fn stop_hub(&mut self) {
        if self.own_hub.take().is_some() {
            self.own_hub_note = Some("You stopped it.".into());
        }
    }

    fn client(&self) -> HubClient {
        HubClient::new(&self.hub_address)
    }

    /// Lists a game this editor just started hosting.
    pub fn announce(&mut self, server_id: String, game: String) {
        let listing = Listing {
            name: format!("{}'s game", self.name.trim()),
            game,
            code: None,
            players: 1,
            max_players: 0,
            kind: HostKind::Player,
        };
        self.announcer = Some(Announcer::start(self.client(), server_id, listing));
    }

    /// Keeps the listing current; call once a frame.
    pub fn tick(&mut self, hosting: Option<(Option<String>, usize)>) {
        match (hosting, &self.announcer) {
            (Some((code, joined)), Some(announcer)) => announcer.update(|l| {
                if code.is_some() {
                    l.code = code;
                }
                l.players = joined as u32 + 1;
            }),
            (None, Some(_)) => self.announcer = None,
            _ => {}
        }
        if let Some(result) = self.fetched.lock().unwrap().take() {
            self.fetching = false;
            match result {
                Ok(servers) => {
                    self.servers = servers;
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
    }

    pub fn refresh(&mut self) {
        if self.fetching {
            return;
        }
        self.fetching = true;
        self.last_fetch = Some(Instant::now());
        let client = self.client();
        let search = self.search.clone();
        let out = self.fetched.clone();
        std::thread::spawn(move || {
            *out.lock().unwrap() = Some(client.servers(&search));
        });
    }

    /// The hosted games, as rows with a Join button each. Returns the join
    /// code and name to join with when one is pressed. Looks again every
    /// few seconds.
    pub fn list(&mut self, ui: &mut egui::Ui) -> Option<(String, String)> {
        if self.last_fetch.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            self.refresh();
        }
        let mut join = None;
        if let Some(error) = &self.error {
            ui.colored_label(Color32::from_rgb(255, 120, 110), error);
        } else if self.servers.is_empty() {
            ui.label(if self.fetching {
                "Looking…"
            } else {
                "No games are being hosted right now. Press Host to start one."
            });
        }
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            egui::Grid::new("browse-servers").num_columns(4).striped(true).spacing([16.0, 6.0]).show(
                ui,
                |ui| {
                    if !self.servers.is_empty() {
                        ui.strong("Server");
                        ui.strong("Game");
                        ui.strong("Players");
                        ui.label("");
                        ui.end_row();
                    }
                    for server in &self.servers {
                        ui.label(&server.name).on_hover_text(format!("{} host", server.kind.label()));
                        ui.label(&server.game);
                        let players = if server.max_players > 0 {
                            format!("{}/{}", server.players, server.max_players)
                        } else {
                            server.players.to_string()
                        };
                        ui.label(players);
                        let full = server.max_players > 0 && server.players >= server.max_players;
                        if ui.add_enabled(!full, egui::Button::new("Join")).clicked() {
                            join = Some((server.code.clone(), self.name.trim().to_owned()));
                        }
                        ui.end_row();
                    }
                },
            );
        });
        join
    }

    /// The Browse window. Returns the join code and name to join with when
    /// a Join button is pressed.
    pub fn window(&mut self, ctx: &egui::Context) -> Option<(String, String)> {
        if !self.open {
            return None;
        }
        if self.last_fetch.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            self.refresh();
        }
        let mut join = None;
        let mut open = self.open;
        egui::Window::new("🌍 Browse games").open(&mut open).default_width(560.0).show(ctx, |ui| {
            egui::Grid::new("browse-settings").num_columns(2).show(ui, |ui| {
                ui.label("Hub");
                let hub = ui.add(
                    egui::TextEdit::singleline(&mut self.hub_address).hint_text("127.0.0.1:7430 or a friend's Hub"),
                );
                if hub.lost_focus() {
                    self.servers.clear();
                    self.refresh();
                }
                ui.end_row();
                ui.label("Search");
                if ui.text_edit_singleline(&mut self.search).changed() {
                    self.last_fetch = None;
                }
                ui.end_row();
                ui.label("Your name");
                ui.add(egui::TextEdit::singleline(&mut self.name).char_limit(20));
                ui.end_row();
            });
            if let Some(hub) = &self.own_hub {
                ui.label(
                    RichText::new(format!(
                        "This computer runs a Hub on port {}. Friends on your network can use this computer's \
                         address as their Hub.",
                        hub.addr().port()
                    ))
                    .small(),
                );
            }
            ui.separator();
            if let Some(picked) = self.list(ui) {
                join = Some(picked);
            }
            ui.horizontal(|ui| {
                if ui.button("⟳ Refresh").clicked() {
                    self.refresh();
                }
                if let Some(status) = self.announcer.as_ref().and_then(Announcer::status) {
                    ui.label(RichText::new(format!("Your game: {status}")).small());
                }
            });
        });
        self.open = open && join.is_none();
        join
    }
}
