//! The editor's side of the Hub: a built-in Hub, the Browse window, and
//! listing a hosted game so others can find it.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy_egui::egui::{self, RichText};
use libroxel_hub::{
    AccountUpdate, Announcer, GameInfo, HostKind, HubClient, Item, Listing, NewItem, Profile, Publish,
    Report, RunningHub, ServerInfo,
};
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
    /// This player's account on the Hub: the name and words to claim, and
    /// what the Hub last said.
    pub account_name: String,
    pub account_about: String,
    /// The Hub the account was last looked up on.
    account_hub: Option<String>,
    account_found: Slot<Option<Profile>>,
    account_saved: Slot<String>,
    account_busy: bool,
    pub account: Option<Profile>,
    pub account_status: Option<Result<String, String>>,
    /// The catalog: things to wear that people made.
    pub items: Vec<Item>,
    fetched_items: Slot<Vec<Item>>,
    item_shared: Slot<String>,
    /// What the Hub said about the last report.
    reported: Slot<String>,
    pub report_status: Option<Result<String, String>>,
    /// The game or item whose ⚑ was pressed: its kind, id and name.
    reporting: Option<(&'static str, String, String)>,
    /// What the Hub said about the last item shared, for the editor's output.
    pub item_status: Option<Result<String, String>>,
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
            account_name: String::new(),
            account_about: String::new(),
            account_hub: None,
            account_found: Arc::default(),
            account_saved: Arc::default(),
            account_busy: false,
            account: None,
            account_status: None,
            items: Vec::new(),
            fetched_items: Arc::default(),
            item_shared: Arc::default(),
            reported: Arc::default(),
            report_status: None,
            reporting: None,
            item_status: None,
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
        if let Some(Ok(items)) = self.fetched_items.lock().unwrap().take() {
            self.items = items;
        }
        if let Some(result) = self.reported.lock().unwrap().take() {
            self.report_status = Some(result);
        }
        if let Some(result) = self.item_shared.lock().unwrap().take() {
            self.item_status = Some(result);
            self.last_fetch = None;
        }
        if let Some(result) = self.account_found.lock().unwrap().take() {
            self.account_busy = false;
            match result {
                Ok(Some(profile)) => {
                    self.account_name = profile.name.clone();
                    self.account_about = profile.about.clone();
                    self.account = Some(profile);
                }
                Ok(None) => self.account = None,
                Err(e) => self.account_status = Some(Err(e)),
            }
        }
        if let Some(result) = self.account_saved.lock().unwrap().take() {
            self.account_busy = false;
            if result.is_ok() {
                // Look again, for the games and the name as the Hub has it.
                self.account_hub = None;
            }
            self.account_status = Some(result);
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
        let items = self.fetched_items.clone();
        std::thread::spawn(move || {
            *out.lock().unwrap() = Some(client.servers(&search));
            *games.lock().unwrap() = Some(client.games(&search));
            *items.lock().unwrap() = Some(client.catalog(""));
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
            let me = crate::identity().ok();
            let signed = |game: &mut Publish| {
                // Signed, so the list shows our account name as the author.
                if let Some(me) = &me {
                    game.author_id = Some(me.player_id());
                    game.author_signature = Some(me.sign(&libroxel_hub::publish_message(&game.secret)));
                }
            };
            let mut game = Publish {
                game_id: mine.and_then(|m| m["game_id"].as_str()).map(str::to_owned),
                secret: mine
                    .and_then(|m| m["secret"].as_str())
                    .map_or_else(libroxel_hub::new_secret, str::to_owned),
                title,
                author,
                description,
                place,
                author_id: None,
                author_signature: None,
            };
            signed(&mut game);
            let mut result = client.publish(&game);
            if result.as_ref().is_err_and(|e| e.contains("no published game")) {
                // Taken down on the Hub since: publish it afresh.
                game.game_id = None;
                game.secret = libroxel_hub::new_secret();
                signed(&mut game);
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

    /// Reports a game or item (`kind`) to the Hub, signed as this player.
    fn report(&mut self, kind: &'static str, target: String, reason: &'static str) {
        let client = self.client();
        let out = self.reported.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let reporter_id = me.player_id();
                let time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs() as i64);
                let message = libroxel_hub::report_message(&reporter_id, kind, &target, reason, time);
                let report = Report {
                    signature: me.sign(&message),
                    reporter_id,
                    kind: kind.into(),
                    target,
                    reason: reason.into(),
                    time,
                };
                client.report(&report)?;
                Ok("Thanks, the Hub has your report. Enough reports hide it until someone looks.".to_owned())
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    /// Shares a hat made in the editor in the catalog, signed as this player.
    pub fn share_hat(&mut self, hat: &libroxel_engine::character::CustomHat) {
        let client = self.client();
        let (name, data) = (hat.name.trim().to_owned(), hat.to_json());
        let out = self.item_shared.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let creator_id = me.player_id();
                let time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs() as i64);
                let signature = me.sign(&libroxel_hub::item_message(&creator_id, &name, "hat", &data, time));
                let item = NewItem { creator_id, name, kind: "hat".into(), data, time, signature };
                let shared = client.share_item(&item)?;
                Ok(format!(
                    "{} is in the catalog on {}. Anyone can wear it from the Avatar tab.",
                    shared.name,
                    client.url()
                ))
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    /// The catalog part of the Avatar tab: hats to put on. Returns the one
    /// picked, or `Some(None)` to take a catalog hat off.
    #[allow(clippy::option_option)]
    pub fn catalog_ui(
        &mut self,
        ui: &mut egui::Ui,
        wearing: Option<&libroxel_engine::character::CustomHat>,
    ) -> Option<Option<libroxel_engine::character::CustomHat>> {
        if self.last_fetch.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            self.refresh();
        }
        let mut picked = None;
        ui.label(RichText::new("Catalog").font(crate::theme::display(17.0)));
        ui.label(
            RichText::new(
                "Hats people made and shared. To make one, build it from parts in the editor, select its \
                 model and use Model > Share as hat.",
            )
            .color(crate::theme::DUST),
        );
        if let Some(hat) = wearing {
            ui.horizontal(|ui| {
                ui.label(format!("Wearing {}", hat.name));
                if ui.button("Take it off").clicked() {
                    picked = Some(None);
                }
            });
        }
        if self.items.is_empty() {
            ui.label("Nothing in the catalog on this Hub yet.");
        }
        let mut worn = None;
        let mut reporting = None;
        ui.horizontal_wrapped(|ui| {
            for item in &self.items {
                let Some(hat) = libroxel_engine::character::CustomHat::from_json(&item.data) else {
                    continue;
                };
                crate::theme::panel().inner_margin(10.0).show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.strong(&item.name);
                        ui.label(RichText::new(format!("by {} · worn {}", item.creator, item.wears)).small());
                        let on = wearing.is_some_and(|w| *w == hat);
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(!on, egui::Button::new(if on { "Wearing" } else { "Wear" }))
                                .clicked()
                            {
                                worn = Some(item.item_id.clone());
                                picked = Some(Some(hat));
                            }
                            if report_button(ui, "hat") {
                                reporting = Some(("item", item.item_id.clone(), item.name.clone()));
                            }
                        });
                    });
                });
            }
        });
        if reporting.is_some() {
            self.reporting = reporting;
            self.report_status = None;
        }
        self.report_row(ui);
        if let Some(id) = worn {
            let client = self.client();
            std::thread::spawn(move || {
                let _ = client.wore(&id);
            });
        }
        picked
    }

    /// Asks why, when the ⚑ beside a game or hat was pressed, and shows
    /// what the Hub said about the report.
    fn report_row(&mut self, ui: &mut egui::Ui) {
        if let Some((kind, target, name)) = self.reporting.clone() {
            let mut done = false;
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{} Report {name} for:", crate::theme::icon::FLAG)).strong());
                for reason in REPORT_REASONS {
                    if ui.button(reason).clicked() {
                        self.report(kind, target.clone(), reason);
                        done = true;
                    }
                }
                if ui.button("Cancel").clicked() {
                    done = true;
                }
            });
            if done {
                self.reporting = None;
            }
        }
        self.show_report_status(ui);
    }

    fn show_report_status(&self, ui: &mut egui::Ui) {
        match &self.report_status {
            Some(Ok(said)) => {
                ui.colored_label(crate::theme::MINT, said);
            }
            Some(Err(why)) => {
                ui.colored_label(crate::theme::BRICK, why);
            }
            None => {}
        }
    }

    /// Looks up this player's account on the Hub, once for each Hub.
    pub fn look_up_account(&mut self) {
        if self.account_busy || self.account_hub.as_deref() == Some(self.hub_address.as_str()) {
            return;
        }
        self.account_hub = Some(self.hub_address.clone());
        self.account_busy = true;
        let client = self.client();
        let out = self.account_found.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| client.profile(&me.player_id()));
            *out.lock().unwrap() = Some(result);
        });
    }

    /// Claims the account name (or saves changes), with `look` as the avatar.
    fn save_account(&mut self, look: Json) {
        if self.account_busy {
            return;
        }
        self.account_busy = true;
        self.account_status = None;
        let client = self.client();
        let (name, about) = (self.account_name.trim().to_owned(), self.account_about.trim().to_owned());
        let out = self.account_saved.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let player_id = me.player_id();
                let time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs() as i64);
                let message = libroxel_hub::account_message(&player_id, &name, &about, &look, time);
                let account = AccountUpdate {
                    signature: me.sign(&message),
                    player_id,
                    name: name.clone(),
                    about,
                    look,
                    time,
                };
                client.update_account(&account)?;
                Ok(format!("Saved. You are {name} on {}.", client.url()))
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    /// The account part of the Avatar tab. `look` is the avatar as saved.
    pub fn account_ui(&mut self, ui: &mut egui::Ui, look: Json) {
        self.look_up_account();
        ui.label(RichText::new("Your account").font(crate::theme::display(17.0)));
        ui.label(
            RichText::new(format!(
                "Claim a name on the Hub at {}. Games you publish then show it, and others can find your page. \
                 It belongs to the identity key on this computer, so keep that file safe.",
                self.hub_address
            ))
            .color(crate::theme::DUST),
        );
        ui.add_space(6.0);
        egui::Grid::new("account").num_columns(2).show(ui, |ui| {
            ui.label("Name");
            ui.add(
                egui::TextEdit::singleline(&mut self.account_name)
                    .char_limit(20)
                    .hint_text("3 to 20 letters"),
            );
            ui.end_row();
            ui.label("About you");
            ui.add(egui::TextEdit::multiline(&mut self.account_about).desired_rows(2).char_limit(300));
            ui.end_row();
        });
        let name = self.account_name.trim().to_owned();
        let valid = libroxel_hub::valid_account_name(&name);
        ui.horizontal(|ui| {
            let label = if self.account.is_some() { "Save changes" } else { "Claim this name" };
            if ui.add_enabled(valid && !self.account_busy, egui::Button::new(label)).clicked() {
                self.save_account(look);
            }
            if self.account_busy {
                ui.spinner();
            }
        });
        if !name.is_empty() && !valid {
            ui.colored_label(crate::theme::SUN, "A name is 3 to 20 letters, digits or underscores.");
        }
        match &self.account_status {
            Some(Ok(said)) => {
                ui.colored_label(crate::theme::MINT, said);
            }
            Some(Err(why)) => {
                ui.colored_label(crate::theme::BRICK, why);
            }
            None => {}
        }
        if let Some(profile) = &self.account {
            let games = match profile.games.len() {
                0 => "no published games yet".to_owned(),
                1 => "1 published game".to_owned(),
                n => format!("{n} published games"),
            };
            ui.label(format!("Your page on this Hub: {} · {games}", profile.name));
            for game in &profile.games {
                ui.label(format!(
                    "  • {} (version {}, played {} times)",
                    game.title, game.version, game.plays
                ));
            }
        }
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
            ui.colored_label(crate::theme::BRICK, error);
        } else if self.games.is_empty() {
            ui.label(if self.fetching {
                "Looking…"
            } else {
                "No games are published on this Hub yet. Publish one from the editor's File menu."
            });
        }
        if let Some(error) = &self.download_error {
            ui.colored_label(crate::theme::BRICK, error);
        }
        if let Some(title) = &self.downloading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("Getting {title}…"));
            });
        }
        let mut chosen = None;
        let mut search = None;
        let mut reporting = None;
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
                        let by = ui.link(&game.author);
                        if by.clicked() {
                            search = Some(game.author.clone());
                        }
                        if game.author_id.is_some() {
                            by.on_hover_text("Their account on this Hub. Click for all their games");
                        }
                        ui.label(game.plays.to_string());
                        ui.horizontal(|ui| {
                            let free = self.downloading.is_none();
                            if ui
                                .add_enabled(
                                    free,
                                    egui::Button::new(format!("{} Play", crate::theme::icon::PLAY)),
                                )
                                .clicked()
                            {
                                chosen = Some((game.clone(), false));
                            }
                            if ui
                                .add_enabled(
                                    free,
                                    egui::Button::new(format!(
                                        "{} With friends",
                                        crate::theme::icon::USERS_THREE
                                    )),
                                )
                                .on_hover_text("Host it, so friends can join you")
                                .clicked()
                            {
                                chosen = Some((game.clone(), true));
                            }
                            if report_button(ui, "game") {
                                reporting = Some(("game", game.game_id.clone(), game.title.clone()));
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
        if reporting.is_some() {
            self.reporting = reporting;
            self.report_status = None;
        }
        self.report_row(ui);
        if let Some(author) = search {
            self.search = author;
            self.refresh();
        }
        if !self.search.is_empty() {
            ui.horizontal(|ui| {
                ui.label(format!("Showing games matching “{}”", self.search));
                if ui.small_button("Show all").clicked() {
                    self.search.clear();
                    self.refresh();
                }
            });
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
            ui.colored_label(crate::theme::BRICK, error);
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
        egui::Window::new(format!("{} Browse games", crate::theme::icon::GLOBE_HEMISPHERE_WEST)).open(&mut open).default_width(560.0).show(ctx, |ui| {
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
                if ui.button(format!("{} Refresh", crate::theme::icon::ARROW_CLOCKWISE)).clicked() {
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

/// The reasons a game or hat can be reported for.
const REPORT_REASONS: [&str; 4] = [
    "Rude, hateful or bullying",
    "Sexual or not OK for kids",
    "A scam or a cheat",
    "Something else that's wrong",
];

/// A small ⚑ button to report a `what` ("game" or "hat").
fn report_button(ui: &mut egui::Ui, what: &str) -> bool {
    ui.small_button(crate::theme::icon::FLAG).on_hover_text(format!("Report this {what}")).clicked()
}
