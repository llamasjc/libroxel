//! The app's side of friends and presence: keeping the friends list fresh,
//! telling the Hub where this player is, pinging servers for the lists, and
//! remembering which game the home screen started.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use libroxel_hub::{FriendAction, FriendChange, FriendState, Friends, Person, PresenceUpdate, ServerInfo};
use libroxel_net::{Network, Pinger};

use crate::browse::Platform;
use crate::menu::{Game, Pick};

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

/// How often the friends list is fetched while the home screen shows it.
const FRIENDS_EVERY: Duration = Duration::from_secs(8);

/// How often the Hub hears this player is still online.
const PRESENCE_EVERY: Duration = Duration::from_secs(15);

/// How long a ping stays good before a list pings again.
const PING_EVERY: Duration = Duration::from_secs(20);

const PICKS_EVERY: Duration = Duration::from_secs(30);

/// A game by title, with its id when it has one: a published game's id, or
/// `starter:<name>` for a starter game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameRef {
    pub title: String,
    pub game_id: Option<String>,
}

/// The id a starter game's servers share.
pub fn starter_id(name: &str) -> String {
    format!("starter:{name}")
}

/// What a home screen game is, for server lists.
pub fn game_ref(game: &Game) -> GameRef {
    match game {
        Game::Starter(name) => GameRef { title: (*name).to_owned(), game_id: Some(starter_id(name)) },
        Game::Blank => GameRef { title: game.label(), game_id: None },
        Game::File(path) if path.parent().is_some_and(|dir| dir.ends_with("official")) => {
            let name = game.label();
            GameRef { game_id: Some(starter_id(&name)), title: name }
        }
        Game::File(path) => {
            let stem = game.label();
            // Published games are kept as "Title (id).lrx" in downloads.
            let downloaded = path.parent().is_some_and(|dir| dir.ends_with("downloads"));
            match stem.rsplit_once(" (").filter(|_| downloaded) {
                Some((title, id)) if id.ends_with(')') => {
                    GameRef { title: title.to_owned(), game_id: Some(id.trim_end_matches(')').to_owned()) }
                }
                _ => GameRef { title: stem, game_id: None },
            }
        }
    }
}

/// Friends and presence, kept between frames.
#[derive(Default)]
pub struct Social {
    /// The game the home screen last started, so hosting lists it under
    /// that game.
    pub next_game: Option<GameRef>,
    /// This player's server id while they host.
    pub hosting: Option<String>,
    /// The listed server this player joined, while they are in it.
    pub joined: Option<ServerInfo>,
    was_playing: bool,
    pub friends: Friends,
    pub friends_error: Option<String>,
    friends_slot: Slot<Friends>,
    friends_busy: bool,
    friends_at: Option<Instant>,
    /// Whether the friends list has come back at least once.
    pub friends_loaded: bool,
    /// People found by name.
    pub people: Vec<Person>,
    pub people_search: String,
    people_slot: Slot<Vec<Person>>,
    /// The search the shown people came from.
    pub people_for: String,
    changed: Slot<(String, String, FriendState)>,
    /// Players whose friendship is being changed right now.
    pub changing: Vec<String>,
    /// What the Hub said to the last friend change.
    pub change_status: Option<Result<String, String>>,
    presence_at: Option<Instant>,
    presence_sent: Option<(bool, Option<String>, Option<String>, String)>,
    pinger: Option<Pinger>,
    pinged: HashMap<String, Instant>,
    /// The last signed time, so every one is newer.
    clock: i64,
    pub charts: libroxel_hub::Charts,
    pub for_you: Option<libroxel_hub::ForYou>,
    picks_slot: Slot<(libroxel_hub::Charts, Option<libroxel_hub::ForYou>)>,
    picks_busy: bool,
    picks_at: Option<Instant>,
}

impl Social {
    /// A time to sign, in milliseconds, newer than the last.
    fn stamp(&mut self) -> i64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        self.clock = now.max(self.clock + 1);
        self.clock
    }

    /// The round trip to a server's host in milliseconds, once measured.
    /// Asks for a new measure when the last one is old.
    pub fn ping(&mut self, code: &str) -> Option<Result<u32, String>> {
        if self.pinger.is_none() {
            self.pinger = Pinger::start(Network::Internet).ok();
        }
        let pinger = self.pinger.as_ref()?;
        if self.pinged.get(code).is_none_or(|at| at.elapsed() > PING_EVERY) {
            self.pinged.insert(code.to_owned(), Instant::now());
            pinger.ping(code);
        }
        pinger.result(code).map(|r| r.map(|d| d.as_millis().max(1) as u32))
    }

    /// Whether `player_id` is a friend.
    pub fn is_friend(&self, player_id: &str) -> bool {
        self.friends.friends.iter().any(|f| f.player_id == player_id)
    }

    /// Whether this player asked `player_id` and is waiting.
    pub fn asked(&self, player_id: &str) -> bool {
        self.friends.outgoing.iter().any(|p| p.player_id == player_id)
    }

    /// Whether `player_id` asked this player.
    pub fn asked_by(&self, player_id: &str) -> bool {
        self.friends.incoming.iter().any(|p| p.player_id == player_id)
    }

    /// Friends' names in the server `server_id`.
    pub fn friends_in(&self, server_id: &str) -> Vec<&libroxel_hub::Friend> {
        self.friends
            .friends
            .iter()
            .filter(|f| f.server.as_ref().is_some_and(|s| s.server_id == server_id))
            .collect()
    }
}

impl Platform {
    /// Keeps friends and presence up to date; call once a frame.
    /// `playing` is whether a game is running.
    pub fn social_tick(&mut self, playing: bool) {
        let social = &mut self.social;
        if let Some(result) = social.friends_slot.lock().unwrap().take() {
            social.friends_busy = false;
            match result {
                Ok(friends) => {
                    social.friends = friends;
                    social.friends_error = None;
                    social.friends_loaded = true;
                }
                Err(e) => social.friends_error = Some(e),
            }
        }
        if let Some(result) = social.people_slot.lock().unwrap().take() {
            social.people = result.unwrap_or_default();
        }
        if let Some(result) = social.changed.lock().unwrap().take() {
            match result {
                Ok((id, name, state)) => {
                    social.changing.retain(|c| *c != id);
                    social.change_status = Some(Ok(match state {
                        FriendState::Requested => format!("Friend request sent to {name}."),
                        FriendState::Friends => format!("You and {name} are friends now."),
                        FriendState::None => format!("{name} is not on your friends list any more."),
                    }));
                }
                Err(e) => {
                    social.changing.clear();
                    social.change_status = Some(Err(e));
                }
            }
            social.friends_at = None;
        }
        if let Some(result) = social.picks_slot.lock().unwrap().take() {
            social.picks_busy = false;
            if let Ok((charts, for_you)) = result {
                social.charts = charts;
                social.for_you = for_you;
            }
        }
        if social.was_playing && !playing {
            social.joined = None;
            social.next_game = None;
        }
        social.was_playing = playing;
        if self.account.is_some()
            && !self.social.friends_busy
            && self.social.friends_at.is_none_or(|at| at.elapsed() > FRIENDS_EVERY)
        {
            self.refresh_friends();
        }
        if !playing
            && !self.social.picks_busy
            && self.social.picks_at.is_none_or(|at| at.elapsed() > PICKS_EVERY)
        {
            self.refresh_picks();
        }
        self.send_presence(playing);
    }

    pub fn refresh_picks(&mut self) {
        let time = self.social.stamp();
        self.social.picks_busy = true;
        self.social.picks_at = Some(Instant::now());
        let client = self.client();
        let signed_in = self.account.is_some();
        let out = self.social.picks_slot.clone();
        std::thread::spawn(move || {
            let result = client.charts().map(|charts| {
                let for_you = signed_in.then(|| crate::identity().ok()).flatten().and_then(|me| {
                    let id = me.player_id();
                    client.for_you(&id, time, &me.sign(&libroxel_hub::for_you_message(&id, time))).ok()
                });
                (charts, for_you)
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    /// Fetches the friends list again now.
    pub fn refresh_friends(&mut self) {
        let time = self.social.stamp();
        self.social.friends_busy = true;
        self.social.friends_at = Some(Instant::now());
        let client = self.client();
        let out = self.social.friends_slot.clone();
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let id = me.player_id();
                let signature = me.sign(&libroxel_hub::friends_list_message(&id, time));
                client.friends(&id, time, &signature)
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    /// Tells the Hub this player is online and where, when that changes
    /// and every little while.
    fn send_presence(&mut self, playing: bool) {
        let social = &self.social;
        let (game, game_id, server_id) = if !playing {
            (None, None, None)
        } else if let Some(server) = &social.joined {
            (Some(server.game.clone()), server.game_id.clone(), Some(server.server_id.clone()))
        } else {
            let next = social.next_game.clone();
            (
                Some(next.as_ref().map_or_else(|| "a game".to_owned(), |g| g.title.clone())),
                next.and_then(|g| g.game_id),
                social.hosting.clone(),
            )
        };
        let state = (true, game.clone(), server_id.clone(), self.hub_address.clone());
        let due = self.social.presence_at.is_none_or(|at| at.elapsed() > PRESENCE_EVERY);
        if !due && self.social.presence_sent.as_ref() == Some(&state) {
            return;
        }
        if self.social.presence_at.is_some_and(|at| at.elapsed() < Duration::from_secs(1)) {
            // Changes settle before they're sent.
            return;
        }
        self.social.presence_at = Some(Instant::now());
        self.social.presence_sent = Some(state);
        let time = self.social.stamp();
        let client = self.client();
        std::thread::spawn(move || {
            let Ok(me) = crate::identity() else { return };
            let mut update = PresenceUpdate {
                player_id: me.player_id(),
                online: true,
                game,
                game_id,
                server_id,
                time,
                signature: String::new(),
            };
            update.signature = me.sign(&libroxel_hub::presence_message(&update));
            let _ = client.presence(&update);
        });
    }

    /// Sends a friend request (or accepts theirs) with `Add`; unfriends,
    /// declines or takes a request back with `Remove`.
    pub fn change_friend(&mut self, other_id: &str, name: &str, action: FriendAction) {
        let time = self.social.stamp();
        self.social.changing.push(other_id.to_owned());
        self.social.change_status = None;
        let client = self.client();
        let out = self.social.changed.clone();
        let (other_id, name) = (other_id.to_owned(), name.to_owned());
        std::thread::spawn(move || {
            let result = crate::identity().and_then(|me| {
                let player_id = me.player_id();
                let signature = me.sign(&libroxel_hub::friend_message(&player_id, &other_id, action, time));
                let change = FriendChange { player_id, other_id: other_id.clone(), action, time, signature };
                client.change_friend(&change).map(|state| (other_id, name, state))
            });
            *out.lock().unwrap() = Some(result);
        });
    }

    /// Looks for people by name (this player too, shown as them).
    pub fn find_people(&mut self) {
        let search = self.social.people_search.trim().to_owned();
        self.social.people_for = search.clone();
        if search.is_empty() {
            self.social.people.clear();
            return;
        }
        let client = self.client();
        let out = self.social.people_slot.clone();
        std::thread::spawn(move || {
            let result = client.people(&search);
            *out.lock().unwrap() = Some(result);
        });
    }

    /// Notes what the home screen is about to start, so presence and
    /// hosting can say which game and server it is.
    pub fn picked(&mut self, pick: &Pick) {
        match pick {
            Pick::Play(game) | Pick::Host(game) | Pick::Edit(game) => {
                self.social.next_game = Some(game_ref(game));
                self.social.joined = None;
            }
            Pick::Join { code, .. } => {
                let code = code.trim();
                let listed = self
                    .servers()
                    .iter()
                    .chain(self.social.friends.friends.iter().filter_map(|f| f.server.as_ref()))
                    .find(|s| s.code.trim() == code)
                    .cloned();
                self.social.next_game =
                    listed.as_ref().map(|s| GameRef { title: s.game.clone(), game_id: s.game_id.clone() });
                self.social.joined = listed;
            }
            Pick::Browse => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn games_know_their_ids() {
        assert_eq!(game_ref(&Game::Starter("Kart race")).game_id.as_deref(), Some("starter:Kart race"));
        let downloaded = std::path::PathBuf::from("/data/downloads/Tower of fun (abc123).lrx");
        let got = game_ref(&Game::File(downloaded));
        assert_eq!(got.title, "Tower of fun");
        assert_eq!(got.game_id.as_deref(), Some("abc123"));
        let mine = game_ref(&Game::File("/games/Mine (old).lrx".into()));
        assert_eq!((mine.title.as_str(), mine.game_id), ("Mine (old)", None));
    }
}
