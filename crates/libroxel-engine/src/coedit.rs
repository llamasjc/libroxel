//! Editing one game together from several computers.
//!
//! One computer **shares** the game it has open; others **join** it and see
//! the same game in their editor, scripts and all. Anyone can change
//! anything, and everyone sees the change a moment later. No server is
//! needed: the sharing computer keeps the true copy.
//!
//! This uses the same connection and the same instance operations as
//! playing over a network ([`crate::net`]), but in both directions. After
//! every frame each computer compares its game with what it last sent and
//! sends the differences. The sharing computer applies what guests send and
//! passes the result on to everyone.
//!
//! Instances are named by the sharing computer's ids. A guest names an
//! instance it has just made, before it has been told the sharing
//! computer's id for it, as `{"g": <the guest's id>}`.
//!
//! Two people changing the same thing at once: the last change to reach the
//! sharing computer wins. A guest's messages are numbered, and the sharing
//! computer says which it has applied; until then the guest keeps its own
//! values for what it changed, so a part someone drags doesn't jump back.

use std::collections::{HashMap, HashSet};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;

use libroxel_instance::serde_json::{Map, Value as Json, json};
use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue};

use crate::net::{Conn, Replicator, Side as End, bits, set_fields, with_port};

/// The port a shared game listens on unless told otherwise (one above
/// [`crate::net::DEFAULT_PORT`], so a game can be shared and played at once).
pub const EDIT_PORT: u16 = 7421;
/// The version of the messages. Computers with different versions can't edit together.
/// 2: long messages are compressed, as in a played game.
/// 3: everything is encrypted, as in a played game.
pub const EDIT_PROTOCOL: u64 = 3;

/// A game being edited together.
pub struct CoEdit {
    side: Side,
    log: Vec<String>,
}

enum Side {
    Host(Box<Sharing>),
    Guest(Box<Guest>),
}

struct Sharing {
    listener: TcpListener,
    peers: Vec<Peer>,
    /// The services' ids, as sent to guests. When they change (another game
    /// was opened), every guest starts over.
    services: Vec<InstanceId>,
}

struct Peer {
    conn: Conn,
    /// Their name, once they have said hello.
    name: Option<String>,
    /// What they have been sent.
    replicator: Replicator,
    /// The instances they made: their ids (as bits) to ours.
    theirs: HashMap<u64, InstanceId>,
    /// Their latest message applied, and the latest we have told them of.
    applied: u64,
    told: u64,
    /// Whether they still need the whole game.
    fresh: bool,
}

struct Guest {
    conn: Conn,
    /// The sharing computer's ids (as bits) to ours, and back.
    ids: HashMap<u64, InstanceId>,
    back: HashMap<InstanceId, u64>,
    /// What we have sent.
    replicator: Replicator,
    /// The number of our latest message.
    seq: u64,
    /// What we changed and the sharing computer hasn't applied yet: the
    /// instance, the field, and the message it went in.
    pending: HashMap<(InstanceId, String), u64>,
    /// Whether the game has arrived.
    welcomed: bool,
    reported: bool,
}

impl CoEdit {
    /// Shares the game open on this computer on `port` (0 picks a free one).
    pub fn host(port: u16) -> Result<Self, String> {
        let listener = TcpListener::bind(("0.0.0.0", port))
            .map_err(|e| format!("could not share on port {port}: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        Ok(Self {
            side: Side::Host(Box::new(Sharing { listener, peers: Vec::new(), services: Vec::new() })),
            log: Vec::new(),
        })
    }

    /// Joins the game shared at `address` (`host` or `host:port`) as `name`.
    /// The game replaces this computer's once it arrives.
    pub fn join(address: &str, name: &str) -> Result<Self, String> {
        let address = with_port_for_edits(address);
        let target: SocketAddr = address
            .to_socket_addrs()
            .map_err(|e| format!("could not find that address: {e}"))?
            .next()
            .ok_or_else(|| "could not find that address".to_owned())?;
        let stream = TcpStream::connect_timeout(&target, Duration::from_secs(5))
            .map_err(|e| format!("could not reach the host: {e}"))?;
        let mut conn = Conn::new(stream, End::Joining).map_err(|e| e.to_string())?;
        conn.send(&json!({ "t": "hello", "name": name, "version": EDIT_PROTOCOL }));
        conn.flush();
        Ok(Self {
            side: Side::Guest(Box::new(Guest {
                conn,
                ids: HashMap::new(),
                back: HashMap::new(),
                replicator: Replicator::edits(),
                seq: 0,
                pending: HashMap::new(),
                welcomed: false,
                reported: false,
            })),
            log: vec!["👥 Joining to edit together".to_owned()],
        })
    }

    /// Sends this frame's changes and takes in everyone else's. Call once a
    /// frame while editing (not while playing). Returns whether someone
    /// else's changes came in.
    pub fn step(&mut self, tree: &mut InstanceTree) -> bool {
        match &mut self.side {
            Side::Host(host) => host.step(tree, &mut self.log),
            Side::Guest(guest) => guest.step(tree, &mut self.log),
        }
    }

    /// What happened since the last call (who joined or left), for the output.
    pub fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }

    /// Where a shared game listens.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        match &self.side {
            Side::Host(host) => host.listener.local_addr().ok(),
            Side::Guest(_) => None,
        }
    }

    /// The names of the people who have joined a shared game.
    pub fn people(&self) -> Vec<String> {
        match &self.side {
            Side::Host(host) => host.peers.iter().filter_map(|p| p.name.clone()).collect(),
            Side::Guest(_) => Vec::new(),
        }
    }

    /// Whether this computer joined someone else's game.
    pub fn is_guest(&self) -> bool {
        matches!(self.side, Side::Guest(_))
    }

    /// Whether the game has arrived (always, for the sharing computer).
    pub fn ready(&self) -> bool {
        match &self.side {
            Side::Host(_) => true,
            Side::Guest(guest) => guest.welcomed,
        }
    }

    /// Whether a joined game's connection is gone.
    pub fn disconnected(&self) -> bool {
        matches!(&self.side, Side::Guest(guest) if guest.conn.closed)
    }
}

fn with_port_for_edits(address: &str) -> String {
    let address = address.trim();
    let full = with_port(address);
    if full.len() == address.len() { full } else { format!("{address}:{EDIT_PORT}") }
}

/// The starter player's script folders, which every tree has, like the services.
const STARTER_FOLDERS: &[ClassKind] = &[ClassKind::StarterPlayerScripts, ClassKind::StarterCharacterScripts];

/// A service, or one of the [`STARTER_FOLDERS`].
fn fixed(tree: &InstanceTree, class: ClassKind) -> Option<InstanceId> {
    tree.service(class).or_else(|| tree.starter_scripts(class))
}

fn service_ids(tree: &InstanceTree) -> Vec<InstanceId> {
    ClassKind::SERVICES.iter().filter_map(|c| tree.service(*c)).collect()
}

// ---------------------------------------------------------------------------
// The sharing computer
// ---------------------------------------------------------------------------

impl Sharing {
    fn step(&mut self, tree: &mut InstanceTree, log: &mut Vec<String>) -> bool {
        let mut changed = false;
        while let Ok((stream, _)) = self.listener.accept() {
            if let Ok(conn) = Conn::new(stream, End::Hosting) {
                self.peers.push(Peer {
                    conn,
                    name: None,
                    replicator: Replicator::edits(),
                    theirs: HashMap::new(),
                    applied: 0,
                    told: 0,
                    fresh: false,
                });
            }
        }
        let services = service_ids(tree);
        if services != self.services {
            self.services = services;
            for peer in &mut self.peers {
                peer.fresh |= peer.name.is_some();
            }
        }
        for peer in &mut self.peers {
            for message in peer.conn.receive() {
                match message.get("t").and_then(Json::as_str) {
                    Some("hello") if peer.name.is_none() => {
                        if message.get("version").and_then(Json::as_u64) != Some(EDIT_PROTOCOL) {
                            peer.conn.send(&json!({ "t": "bye", "why": "a different version of libroxel" }));
                            peer.conn.flush();
                            peer.conn.closed = true;
                            continue;
                        }
                        let name = message.get("name").and_then(Json::as_str).unwrap_or("").trim();
                        let name: String = name.chars().take(40).collect();
                        let name = if name.is_empty() { "Someone".to_owned() } else { name };
                        log.push(format!("👥 {name} joined to edit"));
                        peer.name = Some(name);
                        peer.fresh = true;
                    }
                    // Changes made before they had this game's ids are dropped.
                    Some("ops") if peer.name.is_some() && !peer.fresh => {
                        let ops = message.get("ops").and_then(Json::as_array).cloned().unwrap_or_default();
                        let mut ids = PeerIds { theirs: &mut peer.theirs, made: Vec::new() };
                        changed |= !ops.is_empty();
                        apply(tree, &ops, &mut ids);
                        for (theirs, ours) in ids.made {
                            peer.conn.send(&json!({ "t": "made", "g": theirs, "h": bits(ours) }));
                        }
                        if let Some(seq) = message.get("seq").and_then(Json::as_u64) {
                            peer.applied = peer.applied.max(seq);
                        }
                    }
                    _ => {}
                }
            }
        }
        for peer in &mut self.peers {
            if peer.name.is_none() {
                peer.conn.flush();
                continue;
            }
            if peer.fresh {
                peer.fresh = false;
                peer.replicator = Replicator::edits();
                peer.theirs.clear();
                let services: Map<String, Json> = ClassKind::SERVICES
                    .iter()
                    .chain(STARTER_FOLDERS)
                    .filter_map(|c| Some((c.file_name().to_owned(), bits(fixed(tree, *c)?))))
                    .collect();
                peer.conn.send(&json!({ "t": "welcome", "services": services }));
                let ops = peer.replicator.delta(tree);
                peer.conn.send(&json!({ "t": "rep", "ops": ops, "ack": peer.applied }));
                peer.told = peer.applied;
            } else {
                let ops = peer.replicator.delta(tree);
                if !ops.is_empty() || peer.told != peer.applied {
                    peer.conn.send(&json!({ "t": "rep", "ops": ops, "ack": peer.applied }));
                    peer.told = peer.applied;
                }
            }
            peer.conn.flush();
        }
        let (gone, kept): (Vec<Peer>, Vec<Peer>) =
            std::mem::take(&mut self.peers).into_iter().partition(|p| p.conn.closed);
        self.peers = kept;
        for peer in gone {
            if let Some(name) = peer.name {
                log.push(format!("{name} stopped editing"));
            }
        }
        changed
    }
}

/// How the sharing computer reads a guest's ids.
struct PeerIds<'a> {
    theirs: &'a mut HashMap<u64, InstanceId>,
    made: Vec<(u64, InstanceId)>,
}

impl Ids for PeerIds<'_> {
    fn find(&self, json: &Json) -> Option<InstanceId> {
        match json {
            Json::Number(n) => n.as_u64().map(InstanceId::from_bits),
            Json::Object(o) => o.get("g").and_then(Json::as_u64).and_then(|g| self.theirs.get(&g).copied()),
            _ => None,
        }
    }

    fn made(&mut self, json: &Json, id: InstanceId) {
        if let Some(g) = json.get("g").and_then(Json::as_u64) {
            self.theirs.insert(g, id);
            self.made.push((g, id));
        }
    }

    fn gone(&mut self, id: InstanceId) {
        self.theirs.retain(|_, ours| *ours != id);
    }
}

// ---------------------------------------------------------------------------
// A guest
// ---------------------------------------------------------------------------

/// The fields of an operation, as `pending` names them.
fn fields_of(op: &Json) -> Vec<String> {
    let mut fields = Vec::new();
    for key in ["n", "p", "g", "x"] {
        if op.get(key).is_some() {
            fields.push(key.to_owned());
        }
    }
    for (key, prefix) in [("v", "v:"), ("a", "a:"), ("e", "e:")] {
        if let Some(map) = op.get(key).and_then(Json::as_object) {
            fields.extend(map.keys().map(|k| format!("{prefix}{k}")));
        }
    }
    fields
}

impl Guest {
    fn step(&mut self, tree: &mut InstanceTree, log: &mut Vec<String>) -> bool {
        if self.welcomed {
            let mut ops = self.replicator.delta(tree);
            if !ops.is_empty() {
                self.seq += 1;
                for op in &mut ops {
                    let id = ["c", "u", "d"]
                        .iter()
                        .find_map(|k| op.get(*k).and_then(Json::as_u64))
                        .map(InstanceId::from_bits);
                    if let Some(id) = id {
                        for field in fields_of(op) {
                            self.pending.insert((id, field), self.seq);
                        }
                    }
                    self.outgoing(op);
                }
                self.conn.send(&json!({ "t": "ops", "seq": self.seq, "ops": ops }));
            }
        }
        let mut changed = false;
        for message in self.conn.receive() {
            match message.get("t").and_then(Json::as_str) {
                Some("welcome") => {
                    *tree = InstanceTree::new();
                    self.ids.clear();
                    self.back.clear();
                    self.pending.clear();
                    let services = message.get("services").and_then(Json::as_object);
                    for &class in ClassKind::SERVICES.iter().chain(STARTER_FOLDERS) {
                        if let (Some(theirs), Some(ours)) = (
                            services.and_then(|s| s.get(class.file_name())).and_then(Json::as_u64),
                            fixed(tree, class),
                        ) {
                            self.ids.insert(theirs, ours);
                            self.back.insert(ours, theirs);
                        }
                    }
                    self.replicator = Replicator::edits();
                    if !self.welcomed {
                        log.push("👥 Editing together".to_owned());
                    }
                    self.welcomed = true;
                    changed = true;
                }
                Some("made") if self.welcomed => {
                    if let (Some(ours), Some(theirs)) = (
                        message.get("g").and_then(Json::as_u64).map(InstanceId::from_bits),
                        message.get("h").and_then(Json::as_u64),
                    ) {
                        self.ids.insert(theirs, ours);
                        self.back.insert(ours, theirs);
                    }
                }
                Some("rep") if self.welcomed => {
                    let ack = message.get("ack").and_then(Json::as_u64).unwrap_or(0);
                    self.pending.retain(|_, seq| *seq > ack);
                    let ops = message.get("ops").and_then(Json::as_array).cloned().unwrap_or_default();
                    let ops: Vec<Json> = ops.into_iter().map(|op| self.without_pending(op)).collect();
                    apply(tree, &ops, &mut GuestIds { ids: &mut self.ids, back: &mut self.back });
                    changed = true;
                }
                Some("bye") => {
                    let why = message.get("why").and_then(Json::as_str).unwrap_or("no reason given");
                    log.push(format!("The shared game turned us away: {why}"));
                    self.reported = true;
                }
                _ => {}
            }
        }
        if changed {
            // What we were just sent isn't ours to send back.
            self.replicator.delta(tree);
        }
        self.conn.flush();
        if self.conn.closed && !self.reported {
            self.reported = true;
            log.push("👥 Lost the connection to the shared game".to_owned());
        }
        changed
    }

    /// The sharing computer's name for one of our instances.
    fn name_of(&self, id: u64) -> Json {
        match self.back.get(&InstanceId::from_bits(id)) {
            Some(theirs) => json!(theirs),
            None => json!({ "g": id }),
        }
    }

    /// Rewrites one of our operations into the sharing computer's ids.
    fn outgoing(&self, op: &mut Json) {
        let Some(map) = op.as_object_mut() else { return };
        for key in ["c", "u", "d", "p"] {
            if let Some(id) = map.get(key).and_then(Json::as_u64) {
                map.insert(key.to_owned(), self.name_of(id));
            }
        }
        if let Some(values) = map.get_mut("v").and_then(Json::as_object_mut) {
            for value in values.values_mut() {
                if let Some(id) = value.get("$i").and_then(Json::as_u64) {
                    *value = json!({ "$i": self.name_of(id) });
                }
            }
        }
    }

    /// An incoming operation, less what we changed and haven't had applied.
    fn without_pending(&self, mut op: Json) -> Json {
        if self.pending.is_empty() {
            return op;
        }
        let Some(id) =
            ["c", "u"].iter().find_map(|k| op.get(*k).and_then(Json::as_u64)).and_then(|b| self.ids.get(&b))
        else {
            return op;
        };
        let mine: HashSet<&str> =
            self.pending.keys().filter(|(i, _)| i == id).map(|(_, field)| field.as_str()).collect();
        if mine.is_empty() {
            return op;
        }
        let Some(map) = op.as_object_mut() else { return op };
        for key in ["n", "p", "g", "x"] {
            if mine.contains(key) {
                map.remove(key);
            }
        }
        for (key, prefix) in [("v", "v:"), ("a", "a:"), ("e", "e:")] {
            if let Some(values) = map.get_mut(key).and_then(Json::as_object_mut) {
                values.retain(|k, _| !mine.contains(format!("{prefix}{k}").as_str()));
            }
        }
        op
    }
}

/// How a guest reads the sharing computer's ids.
struct GuestIds<'a> {
    ids: &'a mut HashMap<u64, InstanceId>,
    back: &'a mut HashMap<InstanceId, u64>,
}

impl Ids for GuestIds<'_> {
    fn find(&self, json: &Json) -> Option<InstanceId> {
        json.as_u64().and_then(|b| self.ids.get(&b).copied())
    }

    fn made(&mut self, json: &Json, id: InstanceId) {
        if let Some(theirs) = json.as_u64() {
            self.ids.insert(theirs, id);
            self.back.insert(id, theirs);
        }
    }

    fn gone(&mut self, id: InstanceId) {
        if let Some(theirs) = self.back.remove(&id) {
            self.ids.remove(&theirs);
        }
    }
}

// ---------------------------------------------------------------------------
// Applying operations
// ---------------------------------------------------------------------------

/// How one side names the other's instances.
trait Ids {
    /// Ours, for one of theirs.
    fn find(&self, json: &Json) -> Option<InstanceId>;
    /// We made `id` for one of theirs.
    fn made(&mut self, json: &Json, id: InstanceId);
    /// `id` was removed.
    fn gone(&mut self, id: InstanceId);
}

fn alive(tree: &InstanceTree, id: InstanceId) -> bool {
    tree.is_destroyed(id).is_ok_and(|gone| !gone)
}

/// Applies the other side's operations to `tree`.
fn apply(tree: &mut InstanceTree, ops: &[Json], ids: &mut impl Ids) {
    let mut links: Vec<(InstanceId, Property, Json)> = Vec::new();
    for op in ops {
        let made = op.get("c");
        let existing = made.or_else(|| op.get("u")).and_then(|k| ids.find(k)).filter(|id| alive(tree, *id));
        let id = match (made, existing) {
            (_, Some(id)) => {
                if let Some(name) = op.get("n").and_then(Json::as_str) {
                    let _ = tree.set_name(id, name);
                }
                id
            }
            (Some(key), None) => {
                let Some(class) = op.get("k").and_then(Json::as_str).and_then(ClassKind::from_file_name)
                else {
                    continue;
                };
                let name = op.get("n").and_then(Json::as_str).unwrap_or("Instance");
                let Ok(id) = tree.create(class, name) else { continue };
                ids.made(key, id);
                id
            }
            (None, None) => {
                if let Some(id) = op.get("d").and_then(|d| ids.find(d)).filter(|id| alive(tree, *id))
                    && tree.class(id).is_ok_and(|c| !c.is_service())
                    && tree.root() != id
                {
                    let _ = tree.destroy(id);
                    ids.gone(id);
                }
                continue;
            }
        };
        set_fields(tree, id, op, &mut links);
        match op.get("p") {
            Some(Json::Null) => {
                let _ = tree.set_parent(id, None);
            }
            Some(parent) => {
                if let Some(parent) = ids.find(parent).filter(|p| alive(tree, *p)) {
                    let _ = tree.set_parent(id, Some(parent));
                }
            }
            None => {}
        }
    }
    // Links last: they may point at instances made later in the batch. A
    // link to something we don't have (their camera, say) is left alone.
    for (id, property, target) in links {
        let target = match target {
            Json::Null => None,
            other => match ids.find(&other) {
                Some(found) => Some(found),
                None => continue,
            },
        };
        let _ = tree.set(id, property, PropertyValue::Ref(target));
    }
}
