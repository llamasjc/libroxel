//! Teleports: a game's script sends this player to another of the game's
//! places, or to a server of its own (`TeleportService`). The app asks
//! the Hub for that server, as Play does, and joins it, bringing the
//! teleport data along for `Player:GetJoinData()` there.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use playlibre_engine::PlaySession;
use playlibre_hub::{HubClient, PlayRequest};
use playlibre_instance::serde_json::{Value, json};

type Slot = Arc<Mutex<Option<Result<String, String>>>>;

/// A teleport on its way.
struct Going {
    game_id: String,
    join: Value,
    found: Slot,
}

#[derive(Default)]
pub struct Teleporter {
    going: Option<Going>,
}

/// Where a teleport goes: the server to join and the game it is of.
pub struct Arrive {
    pub code: String,
    pub game_id: String,
}

impl Teleporter {
    /// Run every frame. Answers the server to join once a teleport's has
    /// been found; `Err` says why one could not be.
    pub fn tick(
        &mut self,
        session: Option<&mut PlaySession>,
        hub: &HubClient,
        game_id: Option<&str>,
        signed_in: bool,
    ) -> Option<Result<Arrive, String>> {
        let Some(session) = session else {
            self.going = None;
            return None;
        };
        let mut others = Vec::new();
        let mut failed = None;
        for ask in session.take_host_asks() {
            if ask.get("kind").and_then(Value::as_str) != Some("teleport") {
                others.push(ask);
                continue;
            }
            if self.going.is_some() {
                continue;
            }
            match game_id.filter(|g| !g.starts_with("starter:")) {
                Some(game_id) => self.going = Some(start(hub, game_id, &ask, signed_in)),
                None => failed = Some(Err("teleports only work in games published on the Hub".into())),
            }
        }
        session.put_back_asks(others);
        if failed.is_some() {
            return failed;
        }
        let going = self.going.as_ref()?;
        let result = going.found.lock().unwrap().take()?;
        let going = self.going.take()?;
        Some(result.map(|code| {
            playlibre_engine::session::set_next_join_data(Some(going.join));
            Arrive { code, game_id: going.game_id }
        }))
    }
}

fn start(hub: &HubClient, game_id: &str, ask: &Value, signed_in: bool) -> Going {
    let mut request = PlayRequest {
        game_id: game_id.to_owned(),
        ticket: playlibre_hub::new_secret(),
        place_id: ask.get("place_id").and_then(Value::as_i64),
        access_code: ask.get("access_code").and_then(Value::as_str).map(str::to_owned),
        ..Default::default()
    };
    // Signed, so private servers know who is coming.
    if let Ok(me) = crate::identity()
        && signed_in
    {
        request.signature = Some(me.sign(&playlibre_hub::play_message(&me.player_id(), &request.ticket)));
        request.player_id = Some(me.player_id());
    }
    let found: Slot = Arc::default();
    let (hub, out) = (hub.clone(), found.clone());
    std::thread::spawn(move || {
        let started = Instant::now();
        let result = loop {
            match hub.play(&request) {
                Ok(None) => break Err("this Hub doesn't run game servers".to_owned()),
                Ok(Some(answer)) if answer.code.is_some() => break Ok(answer.code.unwrap_or_default()),
                Ok(Some(_)) if started.elapsed() > Duration::from_secs(90) => {
                    break Err("the server took too long to start".to_owned());
                }
                Ok(Some(_)) => std::thread::sleep(Duration::from_millis(1500)),
                Err(e) => break Err(e),
            }
        };
        *out.lock().unwrap() = Some(result);
    });
    Going {
        game_id: game_id.to_owned(),
        join: json!({ "from": ask.get("from").cloned().unwrap_or(Value::Null), "data": ask.get("data").cloned().unwrap_or(Value::Null) }),
        found,
    }
}
