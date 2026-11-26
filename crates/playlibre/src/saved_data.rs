//! Keeping a server's saved data (data stores) on the Hub, so players keep
//! their progress from one server of a game to the next.
//!
//! The server starts with everything the Hub has for the game. Then, every
//! few seconds, it sends the keys its scripts changed and takes in what
//! other servers of the game changed, on a background thread so a slow
//! Hub never holds up the game. When it stops, it sends the last changes
//! before it quits.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use playlibre_engine::PlaySession;
use playlibre_hub::{DataChange, DataTrade, DataTraded, HubClient, MAX_TRADE, data_message};
use playlibre_instance::serde_json::{self, Value};
use playlibre_net::Identity;
use playlibre_script::SavedData;

/// How often a running server trades with the Hub.
pub const TRADE_EVERY: Duration = Duration::from_secs(5);
/// After the Hub can't be reached, wait this long before trying again.
const RETRY_AFTER: Duration = Duration::from_secs(15);

type Slot = Arc<Mutex<Option<Result<DataTraded, String>>>>;

/// One game's data kept on the Hub.
pub struct SavedOnHub {
    hub: HubClient,
    identity: Identity,
    game_id: String,
    /// The number of the last change the Hub gave us.
    seq: i64,
    /// What the Hub holds, as far as this server knows.
    known: SavedData,
    /// The trade on its way: what we sent, what the game held when we sent it.
    sending: Option<(Vec<DataChange>, SavedData, Slot)>,
    /// When the next trade is due.
    next_at: Instant,
    clock: i64,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// The keys that differ between `old` and `new`.
pub fn changes(old: &SavedData, new: &SavedData) -> Vec<DataChange> {
    let mut out = Vec::new();
    for (store, keys) in new {
        let before = old.get(store);
        for (key, value) in keys {
            if before.and_then(|b| b.get(key)) != Some(value) {
                out.push(DataChange { store: store.clone(), key: key.clone(), value: Some(value.clone()) });
            }
        }
    }
    for (store, keys) in old {
        let after = new.get(store);
        for key in keys.keys() {
            if after.is_none_or(|a| !a.contains_key(key)) {
                out.push(DataChange { store: store.clone(), key: key.clone(), value: None });
            }
        }
    }
    out
}

/// Puts `change` into `data`.
pub fn apply(data: &mut SavedData, change: &DataChange) {
    match &change.value {
        Some(v) => {
            data.entry(change.store.clone()).or_default().insert(change.key.clone(), v.clone());
        }
        None => {
            if let Some(s) = data.get_mut(&change.store) {
                s.remove(&change.key);
                if s.is_empty() {
                    data.remove(&change.store);
                }
            }
        }
    }
}

fn value_of<'a>(data: &'a SavedData, store: &str, key: &str) -> Option<&'a Value> {
    data.get(store).and_then(|s| s.get(key))
}

/// As many of `changes` as fit in one trade.
fn first_batch(changes: &mut Vec<DataChange>) -> Vec<DataChange> {
    let mut size = 0;
    let mut n = 0;
    for c in changes.iter() {
        let len = c.value.as_ref().map_or(0, |v| serde_json::to_string(v).map_or(0, |s| s.len()))
            + c.store.len()
            + c.key.len();
        if n > 0 && size + len > MAX_TRADE / 2 {
            break;
        }
        size += len;
        n += 1;
    }
    changes.drain(..n).collect()
}

impl SavedOnHub {
    /// Fetches everything the Hub keeps for `game_id`. `Err` when this
    /// server may not keep the game's data there (or the Hub can't be
    /// reached), with why.
    pub fn start(hub: HubClient, identity: Identity, game_id: &str) -> Result<Self, String> {
        let mut me = Self {
            hub,
            identity,
            game_id: game_id.to_owned(),
            seq: 0,
            known: SavedData::new(),
            sending: None,
            next_at: Instant::now() + TRADE_EVERY,
            clock: 0,
        };
        loop {
            let traded = me.trade_now(Vec::new())?;
            for c in &traded.changes {
                apply(&mut me.known, c);
            }
            me.seq = traded.seq;
            if !traded.more {
                break;
            }
        }
        Ok(me)
    }

    /// Everything the Hub had at the start, as JSON for
    /// [`PlaySession::host`].
    pub fn start_json(&self) -> String {
        serde_json::to_string(&self.known).unwrap_or_else(|_| "{}".into())
    }

    fn signed(&mut self, changes: Vec<DataChange>) -> DataTrade {
        self.clock = now_ms().max(self.clock + 1);
        let server_id = self.identity.player_id();
        let signature =
            self.identity.sign(&data_message(&server_id, &self.game_id, self.clock, self.seq, &changes));
        DataTrade { server_id, time: self.clock, since: self.seq, changes, signature }
    }

    fn trade_now(&mut self, changes: Vec<DataChange>) -> Result<DataTraded, String> {
        let trade = self.signed(changes);
        self.hub.trade_data(&self.game_id, &trade)
    }

    /// Run every frame: starts a trade when one is due and takes in the
    /// answer to the last one.
    pub fn tick(&mut self, session: &PlaySession) -> Option<String> {
        let mut said = None;
        if let Some((_, _, slot)) = &self.sending {
            let done = slot.lock().unwrap().take();
            if let Some(result) = done {
                let (sent, held, _) = self.sending.take().expect("sending");
                match result {
                    Ok(traded) => self.took(session, &sent, &held, traded),
                    Err(e) => {
                        said = Some(format!("Could not save data on the Hub (will try again): {e}"));
                        self.next_at = Instant::now() + RETRY_AFTER;
                    }
                }
            }
        }
        if self.sending.is_none() && Instant::now() >= self.next_at {
            let held = session.data_snapshot();
            let mut all = changes(&self.known, &held);
            let sent = first_batch(&mut all);
            let trade = self.signed(sent.clone());
            let slot: Slot = Arc::default();
            let (hub, game_id, put) = (self.hub.clone(), self.game_id.clone(), slot.clone());
            std::thread::spawn(move || *put.lock().unwrap() = Some(hub.trade_data(&game_id, &trade)));
            self.sending = Some((sent, held, slot));
            // Straight away again if there is more to send.
            self.next_at = Instant::now() + if all.is_empty() { TRADE_EVERY } else { Duration::ZERO };
        }
        said
    }

    /// Takes in a trade's answer: what we sent is now on the Hub, and what
    /// others changed goes into the game unless a script has changed that
    /// key since.
    fn took(&mut self, session: &PlaySession, sent: &[DataChange], held: &SavedData, traded: DataTraded) {
        for c in sent {
            apply(&mut self.known, c);
        }
        let ours: HashSet<(&str, &str)> = sent.iter().map(|c| (c.store.as_str(), c.key.as_str())).collect();
        let now = session.data_snapshot();
        for c in &traded.changes {
            apply(&mut self.known, c);
            if ours.contains(&(c.store.as_str(), c.key.as_str())) {
                continue;
            }
            // Left alone by scripts since the trade was sent?
            if value_of(&now, &c.store, &c.key) == value_of(held, &c.store, &c.key) {
                session.set_saved_value(&c.store, &c.key, c.value.clone());
            }
        }
        self.seq = traded.seq;
        if traded.more {
            self.next_at = Instant::now();
        }
    }

    /// Sends whatever is left, waiting for the Hub, before the server quits.
    pub fn finish(&mut self, session: &PlaySession) -> Result<(), String> {
        if let Some((sent, held, slot)) = self.sending.take() {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                let done = slot.lock().unwrap().take();
                if let Some(result) = done {
                    if let Ok(traded) = result {
                        self.took(session, &sent, &held, traded);
                    }
                    break;
                }
                if Instant::now() > deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        let held = session.data_snapshot();
        let mut all = changes(&self.known, &held);
        while !all.is_empty() {
            let sent = first_batch(&mut all);
            let traded = self.trade_now(sent.clone())?;
            for c in &sent {
                apply(&mut self.known, c);
            }
            self.seq = traded.seq;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(pairs: &[(&str, &str, i64)]) -> SavedData {
        let mut d = SavedData::new();
        for (s, k, v) in pairs {
            d.entry((*s).into()).or_default().insert((*k).into(), Value::from(*v));
        }
        d
    }

    #[test]
    fn changes_find_new_changed_and_removed_keys() {
        let old = data(&[("coins", "1", 5), ("coins", "2", 7), ("xp", "1", 1)]);
        let new = data(&[("coins", "1", 6), ("coins", "2", 7), ("coins", "3", 1)]);
        let c = changes(&old, &new);
        assert_eq!(c.len(), 3);
        assert!(c.contains(&DataChange { store: "coins".into(), key: "1".into(), value: Some(6.into()) }));
        assert!(c.contains(&DataChange { store: "xp".into(), key: "1".into(), value: None }));
        let mut back = old.clone();
        for change in &c {
            apply(&mut back, change);
        }
        assert_eq!(back, new);
    }

    #[test]
    fn a_big_change_goes_alone() {
        let big = "x".repeat(MAX_TRADE / 2 + 10);
        let mut all = vec![
            DataChange { store: "a".into(), key: "1".into(), value: Some(Value::from(big.clone())) },
            DataChange { store: "a".into(), key: "2".into(), value: Some(Value::from(big)) },
        ];
        assert_eq!(first_batch(&mut all).len(), 1);
        assert_eq!(all.len(), 1);
    }
}
