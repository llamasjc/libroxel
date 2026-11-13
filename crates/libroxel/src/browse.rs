//! The editor's side of the Hub: a built-in Hub, the Browse window, and
//! listing a hosted game so others can find it.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy_egui::egui::{self, Color32, RichText};
use libroxel_hub::{Announcer, GameInfo, HostKind, HubClient, Listing, Publish, RunningHub, ServerInfo};
use libroxel_instance::serde_json::{self, Value as Json, json};

use crate::server::data_dir;

type Fetched = Arc<Mutex<Option<Result<Vec<ServerInfo>, String>>>>;
type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

/// A published game fetched to play: where it was saved, and whether to
/// host it for friends.
pub struct Downloaded {
    pub path: std::path::PathBuf,
    pub host: bool,
}

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
    /// Published games on the Hub.
    games: Vec<GameInfo>,
    games_error: Option<String>,
    fetched_games: Slot<Vec<GameInfo>>,
    /// A published game being fetched to play, and its title.
    downloading: Option<String>,
    downloaded: Slot<Downloaded>,
    download_error: Option<String>,
    /// Publishing this editor's game: what the Hub said, once it has.
    publishing: bool,
    published: Slot<String>,
    pub publish_status: Option<Result<String, String>>,
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
            games: Vec::new(),
            games_error: None,
            fetched_games: Arc::default(),
            downloading: None,
            downloaded: Arc::default(),
            download_error: None,
            publishing: false,
            published: Arc::default(),
            publish_status: None,
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
        if let Some(result) = self.fetched_games.lock().unwrap().take() {
            match result {
                Ok(games) => {
                    self.games = games;
                    self.games_error = None;
                }
                Err(e) => self.games_error = Some(e),
            }
        }
        if let Some(result) = self.published.lock().unwrap().take() {
            self.publishing = false;
            self.publish_status = Some(result);
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
        let games = self.fetched_games.clone();
        std::thread::spawn(move || {
            *out.lock().unwrap() = Some(client.servers(&search));
            *games.lock().unwrap() = Some(client.games(&search));
        });
    }

    /// Publishes `place` (a saved game) to the Hub as `title`, or puts out
    /// a new version if this computer published it before under `key`.
    pub fn publish(&mut self, key: String, title: String, description: String, place: String) {
        if self.publishing {
            return;
        }
        self.publishing = true;
        self.publish_status = None;
        let client = self.client();
        let author = self.name.trim().to_owned();
        let out = self.published.clone();
        std::thread::spawn(move || {
            let hub = client.url().to_owned();
            let mut known = read_published();
            let mine = known.get(&key).filter(|m| m["hub"] == hub.as_str());
            let mut game = Publish {
                game_id: mine.and_then(|m| m["game_id"].as_str()).map(str::to_owned),
                secret: mine
                    .and_then(|m| m["secret"].as_str())
                    .map_or_else(libroxel_hub::new_secret, str::to_owned),
                title,
                author,
                description,
                place,
            };
            let mut result = client.publish(&game);
            if result.as_ref().is_err_and(|e| e.contains("no published game")) {
                // Taken down on the Hub since: publish it afresh.
                game.game_id = None;
                game.secret = libroxel_hub::new_secret();
                result = client.publish(&game);
            }
            let said = result.map(|done| {
                known.insert(key, json!({ "hub": hub, "game_id": done.game_id, "secret": game.secret }));
                let _ = std::fs::create_dir_all(data_dir())
                    .and_then(|_| std::fs::write(published_file(), Json::Object(known).to_string()));
                if done.version == 1 {
                    format!("Published on {hub}. Anyone using that Hub can find it Online.")
                } else {
                    format!("Version {} is out on {hub}.", done.version)
                }
            });
            *out.lock().unwrap() = Some(said);
        });
    }

    /// Whether a publish is on its way.
    pub fn is_publishing(&self) -> bool {
        self.publishing
    }

    /// A published game that has finished downloading, to play.
    pub fn take_download(&mut self) -> Option<Downloaded> {
        let result = self.downloaded.lock().unwrap().take()?;
        self.downloading = None;
        match result {
            Ok(done) => Some(done),
            Err(e) => {
                self.download_error = Some(e);
                None
            }
        }
    }

    fn download(&mut self, game: &GameInfo, host: bool) {
        if self.downloading.is_some() {
            return;
        }
        self.downloading = Some(game.title.clone());
        self.download_error = None;
        let client = self.client();
        let (id, title) = (game.game_id.clone(), game.title.clone());
        let out = self.downloaded.clone();
        std::thread::spawn(move || {
            let result = client.download(&id).and_then(|text| {
                let dir = data_dir().join("downloads");
                let safe: String =
                    title.chars().map(|c| if c.is_alphanumeric() || c == ' ' { c } else { '_' }).collect();
                let path = dir.join(format!("{} ({id}).lrx", safe.trim()));
                std::fs::create_dir_all(&dir)
                    .and_then(|_| std::fs::write(&path, text))
                    .map_err(|e| format!("could not keep the game: {e}"))?;
                Ok(Downloaded { path, host })
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    /// The published games, as rows with Play and Play with friends.
    pub fn published_list(&mut self, ui: &mut egui::Ui) {
        if self.last_fetch.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            self.refresh();
        }
        if let Some(error) = &self.games_error {
            ui.colored_label(Color32::from_rgb(255, 120, 110), error);
        } else if self.games.is_empty() {
            ui.label(if self.fetching {
                "Looking…"
            } else {
                "No games are published on this Hub yet. Publish one from the editor's File menu."
            });
        }
        if let Some(error) = &self.download_error {
            ui.colored_label(Color32::from_rgb(255, 120, 110), error);
        }
        if let Some(title) = &self.downloading {
            ui.label(format!("⏳ Getting {title}…"));
        }
        let mut chosen = None;
        egui::ScrollArea::vertical().id_salt("published").max_height(300.0).show(ui, |ui| {
            egui::Grid::new("published-games").num_columns(4).striped(true).spacing([16.0, 6.0]).show(
                ui,
                |ui| {
                    if !self.games.is_empty() {
                        ui.strong("Game");
                        ui.strong("By");
                        ui.strong("Plays");
                        ui.label("");
                        ui.end_row();
                    }
                    for game in &self.games {
                        let about = if game.description.is_empty() {
                            format!("Version {}", game.version)
                        } else {
                            format!("{}\n\nVersion {}", game.description, game.version)
                        };
                        ui.label(&game.title).on_hover_text(about);
                        ui.label(&game.author);
                        ui.label(game.plays.to_string());
                        ui.horizontal(|ui| {
                            let free = self.downloading.is_none();
                            if ui.add_enabled(free, egui::Button::new("▶ Play")).clicked() {
                                chosen = Some((game.clone(), false));
                            }
                            if ui
                                .add_enabled(free, egui::Button::new("🌐 With friends"))
                                .on_hover_text("Host it, so friends can join you")
                                .clicked()
                            {
                                chosen = Some((game.clone(), true));
                            }
                        });
                        ui.end_row();
                    }
                },
            );
        });
        if let Some((game, host)) = chosen {
            self.download(&game, host);
        }
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

/// Where this computer remembers the games it published (and the secrets
/// that let it put out new versions).
fn published_file() -> std::path::PathBuf {
    data_dir().join("published.json")
}

fn read_published() -> serde_json::Map<String, Json> {
    std::fs::read_to_string(published_file())
        .ok()
        .and_then(|t| serde_json::from_str::<Json>(&t).ok())
        .and_then(|j| j.as_object().cloned())
        .unwrap_or_default()
}
