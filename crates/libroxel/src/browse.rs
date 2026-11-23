//! The app's side of the Hub: the Browse window, and listing a hosted
//! game so others can find it.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy_egui::egui::{self, RichText};
use libroxel_hub::{
    AccountUpdate, Announcer, GameInfo, HostKind, HostSigner, HubClient, Item, Listing, NewItem,
    OfficialGame, Profile, Publish, Report, ServerInfo,
};
use libroxel_instance::serde_json::{self, Value as Json, json};

use crate::server::data_dir;

type Fetched = Arc<Mutex<Option<Result<Vec<ServerInfo>, String>>>>;
mod family;

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

/// A published game fetched to play: where it was saved, and whether to
/// host it for friends.
pub struct Downloaded {
    pub path: std::path::PathBuf,
    pub host: bool,
}

/// Everything about the Hub the editor keeps between frames.
pub struct Platform {
    /// The Hub's address ([`libroxel_hub::hub_address`]). It isn't
    /// something to change in the app: there is one Hub.
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
    official: Vec<OfficialGame>,
    fetched_official: Slot<Vec<OfficialGame>>,
    /// A published game being fetched to play, and its title.
    downloading: Option<String>,
    downloaded: Slot<Downloaded>,
    download_error: Option<String>,
    /// Play, asking the Hub for one of its own servers: the game's title,
    /// and the join code once there is one (`None`: the Hub runs none).
    finding: Option<String>,
    found: Slot<Option<String>>,
    stop_finding: Arc<std::sync::atomic::AtomicBool>,
    /// Why the Hub couldn't find a server, shown on the game's page.
    pub find_error: Option<String>,
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
    account_look: Option<Json>,
    account_checked: Option<Instant>,
    account_time: i64,
    look_to_save: Option<Json>,
    look_saving: bool,
    look_saved: Slot<()>,
    account_saved: Slot<String>,
    account_busy: bool,
    pub account: Option<Profile>,
    pub account_status: Option<Result<String, String>>,
    /// Accounts saved on this computer to switch between (read again when None).
    saved_accounts: Option<Vec<crate::accounts::Saved>>,
    /// A password being set for the account, typed twice.
    password: String,
    password_again: String,
    /// Signing in to an account with its name and password.
    sign_in_name: String,
    sign_in_password: String,
    password_busy: bool,
    password_done: Slot<String>,
    pub password_status: Option<Result<String, String>>,
    /// The Hub asked for a two-step code when signing in, typed here.
    sign_in_code: String,
    needs_code: bool,
    /// Why the account in use must set a new password before going on, if it
    /// signed in with one too easy to guess (read again when None).
    weak_password: Option<Option<String>>,
    /// How the account is kept safe, as the Hub says (only to its owner).
    security: Option<libroxel_hub::Security>,
    security_found: Slot<libroxel_hub::Security>,
    /// Turning two-step sign-in on: the new secret, and a code from the app.
    two_step_secret: Option<String>,
    two_step_code: String,
    two_step_done: Slot<String>,
    /// The email for a new account, or one being added; and the code mailed to it.
    account_email: String,
    email_code: String,
    email_busy: bool,
    email_done: Slot<String>,
    email_status: Option<Result<String, String>>,
    two_step_status: Option<Result<String, String>>,
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
    /// Friends, online status, pings and the game being played.
    pub social: crate::social::Social,
    family: family::FamilyUi,
    pub chats: crate::messages::Chats,
}

impl Default for Platform {
    fn default() -> Self {
        Self {
            hub_address: libroxel_hub::hub_address(),
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
            official: Vec::new(),
            fetched_official: Arc::default(),
            downloading: None,
            downloaded: Arc::default(),
            download_error: None,
            finding: None,
            found: Arc::default(),
            stop_finding: Arc::default(),
            find_error: None,
            publishing: false,
            published: Arc::default(),
            publish_status: None,
            account_name: String::new(),
            account_about: String::new(),
            account_hub: None,
            account_found: Arc::default(),
            account_look: None,
            account_checked: None,
            account_time: 0,
            look_to_save: None,
            look_saving: false,
            look_saved: Arc::default(),
            saved_accounts: None,
            account_saved: Arc::default(),
            account_busy: false,
            account: None,
            account_status: None,
            password: String::new(),
            password_again: String::new(),
            sign_in_name: String::new(),
            sign_in_password: String::new(),
            password_busy: false,
            password_done: Arc::default(),
            password_status: None,
            sign_in_code: String::new(),
            weak_password: None,
            needs_code: false,
            security: None,
            security_found: Arc::default(),
            two_step_secret: None,
            two_step_code: String::new(),
            two_step_done: Arc::default(),
            account_email: String::new(),
            email_code: String::new(),
            email_busy: false,
            email_done: Arc::default(),
            email_status: None,
            two_step_status: None,
            items: Vec::new(),
            fetched_items: Arc::default(),
            item_shared: Arc::default(),
            reported: Arc::default(),
            report_status: None,
            reporting: None,
            item_status: None,
            social: crate::social::Social::default(),
            family: family::FamilyUi::default(),
            chats: crate::messages::Chats::default(),
        }
    }
}

impl Platform {
    /// Who you play as: your account's name, or "Player" before you claim one.
    pub fn player_name(&self) -> String {
        self.account.as_ref().map_or_else(|| "Player".to_owned(), |a| a.name.clone())
    }

    /// Starts with the Hub ([`libroxel_hub::hub_address`]).
    pub fn start() -> Self {
        Self::default()
    }

    pub(crate) fn client(&self) -> HubClient {
        HubClient::new(&self.hub_address)
    }

    /// Lists a game this editor just started hosting.
    pub fn announce(&mut self, server_id: String, game: String) {
        // The game picked on the home screen, if that's what is hosted.
        let (game, game_id) = match self.social.next_game.clone() {
            Some(next) => (next.title, next.game_id),
            None => (game, None),
        };
        self.social.hosting = Some(server_id.clone());
        let listing = Listing {
            name: format!("{}'s game", self.name.trim()),
            game,
            code: None,
            players: 1,
            max_players: 0,
            kind: HostKind::Player,
            game_id,
        };
        // Signed, so the list shows this player's account as the host.
        let host = crate::identity().ok().map(|me| HostSigner {
            player_id: me.player_id(),
            sign: Arc::new(move |message: &[u8]| me.sign(message)),
        });
        self.announcer = Some(Announcer::start_signed(self.client(), server_id, listing, host));
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
            (None, Some(_)) => {
                self.announcer = None;
                self.social.hosting = None;
            }
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
        if let Some(Ok(official)) = self.fetched_official.lock().unwrap().take() {
            self.official = official;
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
        let taken = self.account_found.lock().unwrap().take();
        if let Some(result) = taken {
            self.account_busy = false;
            match result {
                Ok(Some(mut profile)) => {
                    let old = self.account.take().filter(|a| a.player_id == profile.player_id);
                    if let Some(old) = &old {
                        if self.account_name == old.name {
                            self.account_name = profile.name.clone();
                        }
                        if self.account_about == old.about {
                            self.account_about = profile.about.clone();
                        }
                        if let Some(look) = &self.look_to_save {
                            profile.look = look.clone();
                        }
                    } else {
                        // Kept on this computer, to switch back to after signing out.
                        if crate::accounts::remember(&profile.name).is_ok() {
                            self.saved_accounts = None;
                        }
                        self.account_name = profile.name.clone();
                        self.account_about = profile.about.clone();
                        self.look_to_save = None;
                        self.fetch_security();
                        self.family.fetch(self.client());
                    }
                    if old.is_none_or(|o| o.look != profile.look) && profile.look.is_object() {
                        self.account_look = Some(profile.look.clone());
                    }
                    self.account = Some(profile);
                }
                Ok(None) => {
                    self.account = None;
                    self.look_to_save = None;
                }
                Err(e) => self.account_status = Some(Err(e)),
            }
        }
        if let Some(result) = self.account_saved.lock().unwrap().take() {
            self.account_busy = false;
            if result.is_ok() {
                // Look again, for the games and the name as the Hub has it.
                self.account_hub = None;
                self.account_email.clear();
            }
            self.account_status = Some(result);
        }
        self.family.tick();
        if let Some(Ok(security)) = self.security_found.lock().unwrap().take() {
            self.security = Some(security);
        }
        let taken = self.two_step_done.lock().unwrap().take();
        if let Some(result) = taken {
            self.password_busy = false;
            if result.is_ok() {
                self.two_step_secret = None;
                self.two_step_code.clear();
                self.fetch_security();
            }
            self.two_step_status = Some(result);
        }
        let taken = self.email_done.lock().unwrap().take();
        if let Some(result) = taken {
            self.email_busy = false;
            if result.is_ok() {
                self.email_code.clear();
                self.fetch_security();
            }
            self.email_status = Some(result);
        }
        if let Some(result) = self.password_done.lock().unwrap().take() {
            self.password_busy = false;
            self.needs_code = matches!(&result, Err(e) if e == libroxel_hub::NEEDS_CODE);
            if result.is_ok() {
                self.password.clear();
                self.password_again.clear();
                self.sign_in_password.clear();
                self.sign_in_code.clear();
                // A sign-in may have changed whose account this is.
                self.account_hub = None;
            }
            self.weak_password = None;
            self.password_status = Some(result);
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

    /// Looks again when the lists are more than a few seconds old.
    pub fn refresh_if_old(&mut self) {
        if self.last_fetch.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            self.refresh();
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
        let official = self.fetched_official.clone();
        std::thread::spawn(move || {
            *out.lock().unwrap() = Some(client.servers(&search));
            *games.lock().unwrap() = Some(client.games(&search));
            *official.lock().unwrap() = Some(client.official());
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
            let place = match crate::hub_assets::upload(&client, &place) {
                Ok(place) => place,
                Err(e) => {
                    *out.lock().unwrap() = Some(Err(e));
                    return;
                }
            };
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
                    "Published. Anyone can find it Online.".to_owned()
                } else {
                    format!("Version {} is out.", done.version)
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
                Ok(format!("{} is in the catalog. Anyone can wear it from the Avatar tab.", shared.name))
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
            .color(crate::theme::MUTED),
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
            ui.label("Nothing in the catalog yet.");
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
                ui.colored_label(crate::theme::GREEN, said);
            }
            Some(Err(why)) => {
                crate::report::error(ui, why);
            }
            None => {}
        }
    }

    pub fn account_known(&self) -> bool {
        !self.account_busy && self.account_hub.as_deref() == Some(self.hub_address.as_str())
    }

    pub fn take_account_look(&mut self) -> Option<Json> {
        self.account_look.take()
    }

    pub fn save_look(&mut self, look: Json) {
        if let Some(account) = &mut self.account
            && account.look != look
        {
            account.look = look.clone();
            self.look_to_save = Some(look);
        }
    }

    fn signed_time(&mut self) -> i64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        self.account_time = now.max(self.account_time + 1);
        self.account_time
    }

    fn send_look(&mut self) {
        if self.look_saved.lock().unwrap().take().is_some() {
            self.look_saving = false;
        }
        if self.look_saving || self.account_busy {
            return;
        }
        let (Some(account), Some(look)) = (&self.account, self.look_to_save.take()) else { return };
        let (name, about) = (account.name.clone(), account.about.clone());
        let time = self.signed_time();
        self.look_saving = true;
        let client = self.client();
        let out = self.look_saved.clone();
        std::thread::spawn(move || {
            let _ = crate::identity().and_then(|me| {
                let player_id = me.player_id();
                let message = libroxel_hub::account_message(&player_id, &name, &about, &look, time);
                client.update_account(&AccountUpdate {
                    work: None,
                    email: None,
                    signature: me.sign(&message),
                    player_id,
                    name,
                    about,
                    look,
                    time,
                })
            });
            *out.lock().unwrap() = Some(Ok(()));
        });
    }

    /// Looks up this player's account on the Hub, once for each Hub.
    pub fn look_up_account(&mut self) {
        self.send_look();
        let same_hub = self.account_hub.as_deref() == Some(self.hub_address.as_str());
        let recent = self.account_checked.is_some_and(|at| at.elapsed() < Duration::from_secs(30));
        if self.account_busy || self.look_saving || self.look_to_save.is_some() || (same_hub && recent) {
            return;
        }
        self.account_checked = Some(Instant::now());
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
        let new = self.account.is_none();
        let email = Some(self.account_email.trim().to_owned()).filter(|e| new && !e.is_empty());
        let birthday = self.family.ready().filter(|_| new);
        let out = self.account_saved.clone();
        let time = self.signed_time();
        self.look_to_save = None;
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let player_id = me.player_id();
                let message = libroxel_hub::account_message(&player_id, &name, &about, &look, time);
                let account = AccountUpdate {
                    // Only a new account needs the bot check worked out.
                    work: new.then(|| libroxel_hub::account_work(&player_id, &name)),
                    email: email.clone(),
                    signature: me.sign(&message),
                    player_id,
                    name: name.clone(),
                    about,
                    look,
                    time,
                };
                client.update_account(&account)?;
                if let Some((born, parent)) = birthday {
                    family::set_birthday(&client, born, parent)?;
                }
                Ok(match &email {
                    Some(email) => format!(
                        "Welcome, {name}! We sent a code to {email}: type it below to prove the email is yours."
                    ),
                    None => format!("Saved. You are {name}."),
                })
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    /// Seals this computer's identity key with the password typed, and
    /// keeps it on the Hub for signing in elsewhere.
    fn set_password(&mut self) {
        if self.password_busy {
            return;
        }
        self.password_busy = true;
        self.password_status = None;
        let client = self.client();
        let password = self.password.clone();
        let out = self.password_done.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs() as i64);
                let set = libroxel_hub::seal_password(&me.secret_bytes(), &password, time)?;
                client.set_password(&set)?;
                crate::accounts::clear_weak_password();
                Ok("Password set. Sign in with your name and it on any computer.".to_owned())
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    /// Signs in with a name and password, and keeps that account's key on
    /// this computer in place of the one here (which is kept beside it).
    fn sign_in(&mut self) {
        if self.password_busy {
            return;
        }
        self.password_busy = true;
        self.password_status = None;
        let client = self.client();
        let (name, password) = (self.sign_in_name.trim().to_owned(), self.sign_in_password.clone());
        let code = Some(self.sign_in_code.trim().to_owned()).filter(|c| !c.is_empty());
        let out = self.password_done.clone();
        std::thread::spawn(move || {
            let result = client.sign_in(&name, &password, code.as_deref()).and_then(|key| {
                let me = libroxel_net::Identity::from_secret_bytes(&key);
                let kept = me
                    .save_replacing(&crate::identity_path())
                    .map_err(|e| format!("Signed in, but could not save the key here: {e}"))?;
                // The password typed is checked again here, so an account
                // with an easy one has to pick a better one to go on.
                if let Some(why) = libroxel_hub::password_problem(&password, &name) {
                    crate::accounts::mark_weak_password(&me.player_id(), &why)?;
                }
                Ok(match kept {
                    Some(old) => format!(
                        "Signed in as {name}. This computer's old key is kept at {}, in case you need it.",
                        old.display()
                    ),
                    None => format!("Signed in as {name}. This computer already had that account."),
                })
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    /// Gives the account the email typed (or sends a new code to it).
    fn send_email_code(&mut self, email: String) {
        if self.email_busy {
            return;
        }
        self.email_busy = true;
        self.email_status = None;
        let client = self.client();
        let out = self.email_done.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let time = libroxel_hub_now_ms();
                let set = libroxel_hub::EmailSet {
                    player_id: me.player_id(),
                    signature: me.sign(&libroxel_hub::email_message(&me.player_id(), &email, time)),
                    email: email.clone(),
                    time,
                };
                let sent = client.set_email(&set)?;
                Ok(if sent.verified {
                    format!("{email} is already proved.")
                } else if sent.sent {
                    format!("We sent a code to {email}. Type it below.")
                } else {
                    format!("Saved {email}. (The Hub can't send mail yet, so there's no code.)")
                })
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    /// Sends in the code from the mail.
    fn verify_email_code(&mut self) {
        if self.email_busy {
            return;
        }
        self.email_busy = true;
        self.email_status = None;
        let client = self.client();
        let code = self.email_code.trim().to_owned();
        let out = self.email_done.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let time = libroxel_hub_now_ms();
                client.verify_email(&libroxel_hub::EmailCode {
                    player_id: me.player_id(),
                    signature: me.sign(&libroxel_hub::email_code_message(&me.player_id(), &code, time)),
                    code,
                    time,
                })?;
                Ok("Your email is proved. Thanks!".to_owned())
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    /// The account's email: add one, prove it with the mailed code, or see it's done.
    fn email_ui(&mut self, ui: &mut egui::Ui) {
        let Some(security) = self.security.clone() else { return };
        ui.add_space(10.0);
        ui.label(RichText::new("Email").strong());
        match &security.email {
            Some(email) if security.email_verified => {
                ui.colored_label(crate::theme::GREEN, format!("{email} (proved)"));
            }
            Some(email) if security.sends_mail => {
                ui.colored_label(
                    crate::theme::AMBER,
                    format!("Not proved yet: type the code we mailed to {email}."),
                );
                ui.horizontal(|ui| {
                    ui.label("Code from the mail");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.email_code).char_limit(6).desired_width(70.0),
                    );
                    let ready = self.email_code.trim().len() == 6 && !self.email_busy;
                    if ui.add_enabled(ready, egui::Button::new("Prove it")).clicked() {
                        self.verify_email_code();
                    }
                    if ui.add_enabled(!self.email_busy, egui::Button::new("Send again")).clicked() {
                        self.send_email_code(email.clone());
                    }
                });
            }
            Some(email) => {
                ui.label(RichText::new(email).color(crate::theme::TEXT));
            }
            None => {
                ui.colored_label(
                    crate::theme::AMBER,
                    "Your account has no email yet. Add one, so you can prove the account is yours.",
                );
            }
        }
        // A new address, for an account without one or to change it.
        ui.horizontal(|ui| {
            let label = if security.email.is_some() { "New email" } else { "Your email" };
            ui.label(label);
            ui.add(
                egui::TextEdit::singleline(&mut self.account_email)
                    .char_limit(254)
                    .hint_text("you@example.com")
                    .desired_width(220.0),
            );
            let email = self.account_email.trim().to_owned();
            let ready = libroxel_hub::valid_email(&email) && !self.email_busy;
            let button = if security.sends_mail { "Send code" } else { "Save" };
            if ui.add_enabled(ready, egui::Button::new(button)).clicked() {
                self.send_email_code(email);
                self.account_email.clear();
            }
            if self.email_busy {
                ui.spinner();
            }
        });
        match &self.email_status {
            Some(Ok(said)) => {
                ui.colored_label(crate::theme::GREEN, said);
            }
            Some(Err(why)) => {
                crate::report::error(ui, why);
            }
            None => {}
        }
    }

    /// Asks the Hub how this account is kept safe (only its owner may).
    fn fetch_security(&mut self) {
        let client = self.client();
        let out = self.security_found.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let id = me.player_id();
                let time = libroxel_hub_now_ms();
                client.security(&id, time, &me.sign(&libroxel_hub::security_message(&id, time)))
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    /// Turns two-step sign-in on (with `secret`) or off, with a code from the app.
    fn change_two_step(&mut self, secret: Option<String>) {
        if self.password_busy {
            return;
        }
        self.password_busy = true;
        self.two_step_status = None;
        let client = self.client();
        let code = self.two_step_code.trim().to_owned();
        let out = self.two_step_done.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let turning_on = secret.is_some();
                let mut set = libroxel_hub::TwoStepSet {
                    player_id: me.player_id(),
                    secret,
                    code,
                    time: libroxel_hub_now_ms() / 1000,
                    signature: String::new(),
                };
                set.signature = me.sign(&libroxel_hub::two_step_message(&set));
                client.set_two_step(&set)?;
                Ok(if turning_on {
                    "Two-step sign-in is on. Signing in on another computer now needs a code from your app."
                        .to_owned()
                } else {
                    "Two-step sign-in is off.".to_owned()
                })
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    /// Two-step sign-in and recent sign-ins, for an account with a password.
    fn security_ui(&mut self, ui: &mut egui::Ui) {
        let Some(security) = self.security.clone() else { return };
        ui.add_space(10.0);
        ui.label(RichText::new("Two-step sign-in").strong());
        if !security.has_password {
            ui.label(RichText::new("Set a password first.").color(crate::theme::MUTED));
        } else if security.two_step {
            ui.colored_label(
                crate::theme::GREEN,
                "On: signing in on another computer needs a code from your app.",
            );
            ui.horizontal(|ui| {
                ui.label("Code from the app");
                ui.add(egui::TextEdit::singleline(&mut self.two_step_code).char_limit(7).desired_width(70.0));
                let ready = self.two_step_code.trim().len() >= 6 && !self.password_busy;
                if ui.add_enabled(ready, egui::Button::new("Turn off")).clicked() {
                    self.change_two_step(None);
                }
            });
        } else if let Some(secret) = self.two_step_secret.clone() {
            ui.label(
                RichText::new(
                    "Add Libroxel to an authenticator app (Google Authenticator, Authy, 1Password…) with this setup key, then type the 6-digit code it shows.",
                )
                .color(crate::theme::MUTED),
            );
            ui.horizontal(|ui| {
                ui.label(RichText::new(&secret).monospace().color(crate::theme::TEXT));
                if ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text(secret.clone());
                }
                if ui
                    .small_button("Copy link")
                    .on_hover_text("An otpauth:// link, for apps that take one")
                    .clicked()
                {
                    let link =
                        libroxel_hub::two_step_link(&secret, self.account_name.trim(), &self.hub_address);
                    ui.ctx().copy_text(link);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Code from the app");
                ui.add(egui::TextEdit::singleline(&mut self.two_step_code).char_limit(7).desired_width(70.0));
                let ready = self.two_step_code.trim().len() >= 6 && !self.password_busy;
                if ui.add_enabled(ready, egui::Button::new("Turn on")).clicked() {
                    self.change_two_step(Some(secret.clone()));
                }
                if ui.button("Cancel").clicked() {
                    self.two_step_secret = None;
                }
            });
        } else {
            ui.label(
                RichText::new(
                    "Off. Turn it on so a stolen password alone can't get anyone into your account.",
                )
                .color(crate::theme::MUTED),
            );
            if ui.button("Set up two-step sign-in").clicked() {
                self.two_step_secret = Some(libroxel_hub::new_two_step_secret());
                self.two_step_code.clear();
            }
        }
        match &self.two_step_status {
            Some(Ok(said)) => {
                ui.colored_label(crate::theme::GREEN, said);
            }
            Some(Err(why)) => {
                crate::report::error(ui, why);
            }
            None => {}
        }
        ui.add_space(6.0);
        ui.label(RichText::new("Sign-ins on other computers").strong());
        if security.sign_ins.is_empty() {
            ui.label(RichText::new("None yet.").color(crate::theme::MUTED));
        }
        for record in security.sign_ins.iter().take(5) {
            ui.label(format!("  • {} from {}", ago(record.secs_ago), record.from));
        }
        if !security.sign_ins.is_empty() {
            ui.label(
                RichText::new("Not you? Set a new password and turn on two-step sign-in.")
                    .small()
                    .color(crate::theme::MUTED),
            );
        }
    }

    /// Why the account in use has to set a new password before going on:
    /// it signed in with one too easy to guess.
    pub fn weak_password(&mut self) -> Option<String> {
        self.weak_password.get_or_insert_with(crate::accounts::weak_password).clone()
    }

    /// The whole page while the account must change its password: a new
    /// one, typed twice, and nothing else until it is set.
    pub fn new_password_ui(&mut self, ui: &mut egui::Ui, why: &str) {
        crate::theme::heading(ui, "Pick a new password");
        ui.colored_label(
            crate::theme::AMBER,
            format!("Your password is too easy to guess ({why}). Set a new one to keep playing."),
        );
        ui.label(
            RichText::new(
                "Use at least 10 characters that aren't a common password or your name. A few unrelated words work well. It is worked into a key on this computer and never sent to the Hub.",
            )
            .color(crate::theme::MUTED),
        );
        ui.add_space(8.0);
        let name = self
            .account
            .as_ref()
            .map(|a| a.name.clone())
            .unwrap_or_else(|| self.account_name.trim().to_owned());
        egui::Grid::new("new-password").num_columns(2).show(ui, |ui| {
            ui.label("New password");
            ui.add(egui::TextEdit::singleline(&mut self.password).password(true).char_limit(256));
            ui.end_row();
            ui.label("Again");
            ui.add(egui::TextEdit::singleline(&mut self.password_again).password(true).char_limit(256));
            ui.end_row();
        });
        let problem = libroxel_hub::password_problem(&self.password, &name)
            .or_else(|| (self.password != self.password_again).then(|| "the two don't match".to_owned()));
        ui.horizontal(|ui| {
            let ready = problem.is_none() && !self.password_busy;
            if ui.add_enabled(ready, egui::Button::new("Set password")).clicked() {
                self.set_password();
            }
            if self.password_busy {
                ui.spinner();
            }
        });
        if let Some(problem) = problem.filter(|_| !self.password.is_empty()) {
            ui.colored_label(crate::theme::AMBER, format!("Not yet: {problem}."));
        }
        if let Some(Err(why)) = &self.password_status {
            crate::report::error(ui, why);
        }
    }

    /// The password part of the Avatar tab: set one, or sign in with one.
    fn password_ui(&mut self, ui: &mut egui::Ui) {
        if self.account.is_some() {
            self.email_ui(ui);
        }
        ui.add_space(10.0);
        if self.account.is_some() {
            ui.label(RichText::new("Password").strong());
            ui.label(
                RichText::new(
                    "Set a password to sign in to this account on another computer, or to get it back if this one breaks. It is worked into a key on this computer and never sent to the Hub.",
                )
                .color(crate::theme::MUTED),
            );
            egui::Grid::new("account-password").num_columns(2).show(ui, |ui| {
                ui.label("New password");
                ui.add(egui::TextEdit::singleline(&mut self.password).password(true).char_limit(256));
                ui.end_row();
                ui.label("Again");
                ui.add(egui::TextEdit::singleline(&mut self.password_again).password(true).char_limit(256));
                ui.end_row();
            });
            let problem = libroxel_hub::password_problem(&self.password, self.account_name.trim())
                .or_else(|| (self.password != self.password_again).then(|| "the two don't match".to_owned()));
            ui.horizontal(|ui| {
                let ready = problem.is_none() && !self.password_busy;
                if ui.add_enabled(ready, egui::Button::new("Set password")).clicked() {
                    self.set_password();
                }
                if self.password_busy {
                    ui.spinner();
                }
            });
            if let Some(problem) = problem.filter(|_| !self.password.is_empty()) {
                ui.colored_label(crate::theme::AMBER, format!("Not yet: {problem}."));
            }
            self.security_ui(ui);
        }
        let header = if self.account.is_some() {
            "Sign in to a different account"
        } else {
            "Already have an account?"
        };
        egui::CollapsingHeader::new(RichText::new(header).strong())
            .id_salt("account-sign-in")
            .default_open(self.account.is_none())
            .show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Sign in with the name and password you set on another computer. This computer then uses that account; its own key is kept beside it, so nothing is lost.",
                    )
                    .color(crate::theme::MUTED),
                );
                egui::Grid::new("account-sign-in-grid").num_columns(2).show(ui, |ui| {
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut self.sign_in_name).char_limit(20));
                    ui.end_row();
                    ui.label("Password");
                    ui.add(egui::TextEdit::singleline(&mut self.sign_in_password).password(true).char_limit(256));
                    ui.end_row();
                    if self.needs_code {
                        ui.label("Code from your app");
                        ui.add(egui::TextEdit::singleline(&mut self.sign_in_code).char_limit(7));
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    let ready = libroxel_hub::valid_account_name(self.sign_in_name.trim())
                        && !self.sign_in_password.is_empty()
                        && !self.password_busy;
                    if ui.add_enabled(ready, egui::Button::new("Sign in")).clicked() {
                        self.sign_in();
                    }
                    if self.password_busy {
                        ui.spinner();
                    }
                });
            });
        match &self.password_status {
            Some(Ok(said)) => {
                ui.colored_label(crate::theme::GREEN, said);
            }
            Some(Err(why)) => {
                crate::report::error(ui, why);
            }
            None => {}
        }
    }

    /// Forgets everything about the account in use, after switching or
    /// signing out, so the new one is looked up afresh.
    fn account_changed(&mut self, said: Result<String, String>) {
        self.account = None;
        self.account_name.clear();
        self.account_about.clear();
        self.account_hub = None;
        self.account_status = None;
        self.security = None;
        self.two_step_secret = None;
        self.password_status = Some(said);
        self.saved_accounts = None;
        self.weak_password = None;
        self.family.forget();
    }

    /// The accounts saved on this computer: switch to one, or sign out.
    fn switch_ui(&mut self, ui: &mut egui::Ui) {
        let saved = self.saved_accounts.get_or_insert_with(crate::accounts::saved).clone();
        let current = self.account.as_ref().map(|a| a.player_id.clone());
        let others: Vec<_> = saved.iter().filter(|s| Some(&s.player_id) != current.as_ref()).collect();
        if others.is_empty() && self.account.is_none() {
            return;
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if let Some(account) = &self.account {
                ui.label(RichText::new(format!("Signed in as {}", account.name)).strong());
            } else {
                ui.label(RichText::new("Not signed in").strong());
            }
            let mut picked = None;
            for other in &others {
                if ui.button(format!("Switch to {}", other.name)).clicked() {
                    picked = Some((other.player_id.clone(), other.name.clone()));
                }
            }
            if let Some((id, name)) = picked {
                let said = crate::accounts::switch_to(&id).map(|()| format!("Switched to {name}."));
                self.account_changed(said);
            }
            if let Some(name) = self.account.as_ref().map(|a| a.name.clone())
                && ui
                    .button("Sign out")
                    .on_hover_text(
                        "Make another account or sign in to one. This account stays saved on this computer, \
                         so you can switch back to it.",
                    )
                    .clicked()
            {
                let said = crate::accounts::sign_out(Some(&name)).map(|()| {
                    format!("Signed out of {name}. Make a new account below, or sign in to another one.")
                });
                self.account_changed(said);
            }
        });
    }

    /// The account part of the Avatar tab. `look` is the avatar as saved.
    pub fn account_ui(&mut self, ui: &mut egui::Ui, look: Json) {
        self.look_up_account();
        ui.label(RichText::new("Your account").font(crate::theme::display(17.0)));
        let about = if self.account.is_some() {
            "You play, host and join games as this name, games you publish show it, and others can find \
             your page. Set a password below to use it on other computers too."
        } else {
            "Make an account: pick a name. You play as it, games you publish show it, and others can find \
             your page. It belongs to the identity key on this computer; set a password below to use it \
             elsewhere too."
        };
        ui.label(RichText::new(about).color(crate::theme::MUTED));
        self.switch_ui(ui);
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
            if self.account.is_none() {
                ui.label("Email");
                ui.add(
                    egui::TextEdit::singleline(&mut self.account_email)
                        .char_limit(254)
                        .hint_text("you@example.com, kept private"),
                );
                ui.end_row();
                self.family.birthday_rows(ui);
            }
        });
        let name = self.account_name.trim().to_owned();
        let email_ok = self.account.is_some() || libroxel_hub::valid_email(self.account_email.trim());
        let born_ok = self.account.is_some() || self.family.ready().is_some();
        let valid = libroxel_hub::valid_account_name(&name) && email_ok && born_ok;
        ui.horizontal(|ui| {
            let label = if self.account.is_some() { "Save changes" } else { "Make an account" };
            if ui.add_enabled(valid && !self.account_busy, egui::Button::new(label)).clicked() {
                self.save_account(look);
            }
            if self.account_busy {
                ui.spinner();
            }
        });
        if !name.is_empty() && !libroxel_hub::valid_account_name(&name) {
            ui.colored_label(
                crate::theme::AMBER,
                "A name is 3 to 20 letters, digits or underscores, with at least one letter.",
            );
        } else if self.account.is_none() && !name.is_empty() && !email_ok {
            ui.colored_label(
                crate::theme::AMBER,
                "Add your email too: the Hub mails it a code to prove it's yours. No one else sees it.",
            );
        }
        match &self.account_status {
            Some(Ok(said)) => {
                ui.colored_label(crate::theme::GREEN, said);
            }
            Some(Err(why)) => {
                crate::report::error(ui, why);
            }
            None => {}
        }
        if let Some(profile) = &self.account {
            let games = match profile.games.len() {
                0 => "no published games yet".to_owned(),
                1 => "1 published game".to_owned(),
                n => format!("{n} published games"),
            };
            ui.label(format!("Your page: {} (#{}) · {games}", profile.name, profile.number));
            if profile.admin {
                ui.colored_label(crate::theme::AMBER, "You are an admin of the Hub.");
            } else if profile.verified {
                ui.colored_label(crate::theme::LINK, "Your account is verified.");
            }
            for game in &profile.games {
                ui.label(format!(
                    "  • {} (version {}, played {} times)",
                    game.title, game.version, game.plays
                ));
            }
        }
        if self.account.is_some() {
            let client = self.client();
            self.family.parent_ui(ui, client);
        }
        self.password_ui(ui);
    }

    pub fn needs_birthday(&self) -> bool {
        self.account.is_some() && self.family.family.as_ref().is_some_and(|f| !f.has_birthday)
    }

    pub fn birthday_page(&mut self, ui: &mut egui::Ui) {
        let client = self.client();
        self.family.birthday_page(ui, client);
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

    /// Whether the lists have come in from the Hub at least once.
    pub fn listed(&self) -> bool {
        self.last_fetch.is_some() && !self.fetching
    }

    /// The live servers the Hub listed last.
    pub fn servers(&self) -> &[ServerInfo] {
        &self.servers
    }

    /// The published games the Hub listed last.
    pub fn games(&self) -> &[GameInfo] {
        &self.games
    }

    /// Whether a published game is being fetched, and its title.
    pub fn downloading(&self) -> Option<&str> {
        self.downloading.as_deref()
    }

    /// Play: asks the Hub for one of its own servers of `game_id` (it finds
    /// one with room or starts one), in the background. The answer comes
    /// from [`take_found`](Self::take_found).
    pub fn find_server(&mut self, game_id: &str, title: &str) {
        if self.finding.is_some() {
            return;
        }
        self.finding = Some(title.to_owned());
        self.find_error = None;
        *self.found.lock().unwrap() = None;
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.stop_finding = stop.clone();
        let client = self.client();
        let mut request = libroxel_hub::PlayRequest {
            game_id: game_id.to_owned(),
            ticket: libroxel_hub::new_secret(),
            ..Default::default()
        };
        if let Ok(me) = crate::identity()
            && self.account.is_some()
        {
            request.signature = Some(me.sign(&libroxel_hub::play_message(&me.player_id(), &request.ticket)));
            request.player_id = Some(me.player_id());
        }
        let out = self.found.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            let result = loop {
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                match client.play(&request) {
                    Ok(None) => break Ok(None),
                    Ok(Some(answer)) if answer.code.is_some() => break Ok(answer.code),
                    Ok(Some(_)) if started.elapsed() > Duration::from_secs(90) => {
                        break Err("The server took too long to start. Try again in a minute.".to_owned());
                    }
                    Ok(Some(_)) => std::thread::sleep(Duration::from_millis(1500)),
                    Err(e) => break Err(e),
                }
            };
            *out.lock().unwrap() = Some(result);
        });
    }

    /// The title of the game a server is being found for, while it is.
    pub fn finding_server(&self) -> Option<&str> {
        self.finding.as_deref()
    }

    /// Stops waiting for a server.
    pub fn stop_finding(&mut self) {
        self.stop_finding.store(true, std::sync::atomic::Ordering::Relaxed);
        self.finding = None;
    }

    /// What finding a server came to, once it has: `Some(Some(code))` to
    /// join, `Some(None)` when the Hub runs no servers of its own. Errors
    /// land in [`find_error`](Self::find_error).
    pub fn take_found(&mut self) -> Option<Option<String>> {
        self.finding.as_ref()?;
        let result = self.found.lock().unwrap().take()?;
        self.finding = None;
        match result {
            Ok(code) => Some(code),
            Err(e) => {
                self.find_error = Some(e);
                None
            }
        }
    }

    pub fn official(&self) -> &[OfficialGame] {
        &self.official
    }

    pub fn get_official(&mut self, game: &OfficialGame, host: bool) {
        if self.downloading.is_some() {
            return;
        }
        self.downloading = Some(game.name.clone());
        self.download_error = None;
        let client = self.client();
        let game = game.clone();
        let out = self.downloaded.clone();
        std::thread::spawn(move || {
            let result =
                crate::hub_assets::get_official(&client, &game).map(|path| Downloaded { path, host });
            *out.lock().unwrap() = Some(result);
        });
    }

    /// Fetches a published game to play (or host, with `host`).
    pub fn get_game(&mut self, game: &GameInfo, host: bool) {
        self.download(game, host);
    }

    fn download(&mut self, game: &GameInfo, host: bool) {
        if self.downloading.is_some() {
            return;
        }
        self.downloading = Some(game.title.clone());
        self.download_error = None;
        let client = self.client();
        let (id, title, version) = (game.game_id.clone(), game.title.clone(), game.version);
        let out = self.downloaded.clone();
        std::thread::spawn(move || {
            let dir = data_dir().join("downloads");
            let safe: String =
                title.chars().map(|c| if c.is_alphanumeric() || c == ' ' { c } else { '_' }).collect();
            let path = dir.join(format!("{} ({id}).lrx", safe.trim()));
            let stamp = dir.join(format!("{id}.version"));
            let cached = path.exists()
                && std::fs::read_to_string(&stamp).is_ok_and(|v| v.trim() == version.to_string());
            let result = if cached {
                Ok(Downloaded { path, host })
            } else {
                client.download(&id).and_then(|text| {
                    crate::hub_assets::write(&path, &text)?;
                    let _ = std::fs::write(&stamp, version.to_string());
                    Ok(Downloaded { path, host })
                })
            };
            *out.lock().unwrap() = Some(result);
        });
    }

    /// The published games, as rows with Play and Play with friends.
    pub fn published_list(&mut self, ui: &mut egui::Ui) {
        if self.last_fetch.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            self.refresh();
        }
        if let Some(error) = &self.games_error {
            crate::report::error(ui, error);
        } else if self.games.is_empty() {
            ui.label(if self.fetching {
                "Looking…"
            } else {
                "No games are published yet. Publish one from the editor's File menu."
            });
        }
        if let Some(error) = &self.download_error {
            crate::report::error(ui, error);
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
                            by.on_hover_text("Their account. Click for all their games");
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
            crate::report::error(ui, error);
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
                        let hover = match &server.host {
                            _ if server.admin_host => {
                                format!(
                                    "{} host · run by a Libroxel admin (checked by the Hub)",
                                    server.kind.label()
                                )
                            }
                            Some(host) => format!(
                                "{} host · hosted by {host} (checked by the Hub)",
                                server.kind.label()
                            ),
                            None => format!("{} host", server.kind.label()),
                        };
                        ui.label(&server.name).on_hover_text(hover);
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
        egui::Window::new(format!("{} Browse games", crate::theme::icon::GLOBE_HEMISPHERE_WEST))
            .open(&mut open)
            .default_width(560.0)
            .show(ctx, |ui| {
                egui::Grid::new("browse-settings").num_columns(2).show(ui, |ui| {
                    ui.label("Search");
                    if ui.text_edit_singleline(&mut self.search).changed() {
                        self.last_fetch = None;
                    }
                    ui.end_row();
                    // You play as your account's name; it can't be typed over here.
                    if let Some(account) = &self.account {
                        self.name = account.name.clone();
                    }
                    ui.label("Playing as");
                    ui.label(RichText::new(self.name.trim()).strong());
                    ui.end_row();
                });
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

fn libroxel_hub_now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// "3 minutes ago" and the like.
fn ago(secs: i64) -> String {
    let (n, unit) = match secs.max(0) {
        s if s < 60 => return "just now".into(),
        s if s < 3600 => (s / 60, "minute"),
        s if s < 86_400 => (s / 3600, "hour"),
        s => (s / 86_400, "day"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}
