//! Editing one game together from several computers.
//!
//! One computer **shares** the game it has open; others **join** it and see
//! the same game in their editor, scripts and all. Anyone let in can
//! change anything, and everyone sees the change a moment later. No server
//! is needed: the sharing computer keeps the true copy.
//!
//! Since a guest sees every script and can change any of them, nobody gets
//! in by just reaching the port: each share has a random edit password
//! ([`CoEdit::password`]) that a guest must send, and then the sharing
//! computer's person lets each guest in by name ([`CoEdit::asking`],
//! [`CoEdit::answer`]). A guest who is turned away, or gives the wrong
//! password, is sent nothing.
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
use std::time::{Duration, Instant};

use playlibre_instance::serde_json::{Map, Value as Json, json};
use playlibre_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue};

use crate::net::{Conn, Replicator, Side as End, bits, set_fields, with_port};

/// The port a shared game listens on unless told otherwise (one above
/// [`crate::net::DEFAULT_PORT`], so a game can be shared and played at once).
pub const EDIT_PORT: u16 = 7421;
/// The version of the messages. Computers with different versions can't edit together.
/// 2: long messages are compressed, as in a played game.
/// 3: everything is encrypted, as in a played game.
/// 4: guests give the share's password and wait to be let in.
pub const EDIT_PROTOCOL: u64 = 4;

/// What separates an edit password from the address or code before it:
/// `192.168.1.20#abcde-fghij`.
pub const PASSWORD_MARK: char = '#';

/// How long a computer may stay connected without giving the password.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

/// How many guests may wait to be let in at once.
const MAX_ASKING: usize = 8;

/// `text` split into the address (or code) and the edit password after
/// [`PASSWORD_MARK`], if there is one.
pub fn split_password(text: &str) -> (&str, Option<&str>) {
    match text.trim().rsplit_once(PASSWORD_MARK) {
        Some((address, password)) => (address.trim(), Some(password.trim())),
        None => (text.trim(), None),
    }
}

/// A new edit password: ten letters and digits (about 50 bits), in two
/// groups of five so it reads aloud.
fn new_password() -> String {
    const LETTERS: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut letters = String::new();
    while letters.len() < 10 {
        let mut byte = [0u8; 1];
        getrandom::fill(&mut byte).expect("the system has a random number source");
        // 248 is the largest multiple of 31 under 256: no letter comes up more often.
        if byte[0] < 248 {
            letters.push(LETTERS[byte[0] as usize % LETTERS.len()] as char);
        }
    }
    format!("{}-{}", &letters[..5], &letters[5..])
}

fn tidy_password(password: &str) -> String {
    password.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase()
}

/// Compares without taking longer the more letters match.
fn same_password(a: &str, b: &str) -> bool {
    let (a, b) = (tidy_password(a), tidy_password(b));
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

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
    /// What guests must send to be asked about.
    password: String,
    /// The number the next guest who asks gets.
    next_ticket: u64,
    peers: Vec<Peer>,
    /// The services' ids, as sent to guests. When they change (another game
    /// was opened), every guest starts over.
    services: Vec<InstanceId>,
}

struct Peer {
    conn: Conn,
    /// Their name, once they have said hello.
    name: Option<String>,
    /// Their number while they wait to be let in, and whether they have been.
    ticket: u64,
    allowed: bool,
    /// When they connected.
    since: Instant,
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
    /// Whether the sharing computer has the password and is deciding.
    waiting: bool,
    reported: bool,
}

impl CoEdit {
    /// Shares the game open on this computer on `port` (0 picks a free one),
    /// to this computer's network as well. Guests still need
    /// [`password`](Self::password) and to be let in.
    pub fn host(port: u16) -> Result<Self, String> {
        Self::host_on("0.0.0.0", port)
    }

    /// [`host`](Self::host), reachable only from this computer (and so only
    /// through an internet tunnel running here).
    pub fn host_here_only(port: u16) -> Result<Self, String> {
        Self::host_on("127.0.0.1", port)
    }

    fn host_on(ip: &str, port: u16) -> Result<Self, String> {
        let listener =
            TcpListener::bind((ip, port)).map_err(|e| format!("could not share on port {port}: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        Ok(Self {
            side: Side::Host(Box::new(Sharing {
                listener,
                password: new_password(),
                next_ticket: 1,
                peers: Vec::new(),
                services: Vec::new(),
            })),
            log: Vec::new(),
        })
    }

    /// The password guests give to ask to join a shared game.
    pub fn password(&self) -> Option<&str> {
        match &self.side {
            Side::Host(host) => Some(&host.password),
            Side::Guest(_) => None,
        }
    }

    /// Guests who gave the password and wait to be let in: a number for
    /// [`answer`](Self::answer), and the name they gave.
    pub fn asking(&self) -> Vec<(u64, String)> {
        match &self.side {
            Side::Host(host) => host
                .peers
                .iter()
                .filter(|p| !p.allowed && !p.conn.closed)
                .filter_map(|p| Some((p.ticket, p.name.clone()?)))
                .collect(),
            Side::Guest(_) => Vec::new(),
        }
    }

    /// Lets the guest numbered `ticket` in, or turns them away.
    pub fn answer(&mut self, ticket: u64, allow: bool) {
        let Side::Host(host) = &mut self.side else { return };
        let Some(peer) = host.peers.iter_mut().find(|p| p.ticket == ticket && !p.allowed) else { return };
        let name = peer.name.clone().unwrap_or_default();
        if allow {
            peer.allowed = true;
            peer.fresh = true;
            self.log.push(format!("👥 {name} joined to edit"));
        } else {
            peer.conn.send(&json!({ "t": "bye", "why": "the person sharing this game didn't let you in" }));
            peer.conn.flush();
            peer.conn.closed = true;
            self.log.push(format!("{name} was not let in"));
        }
    }

    /// Joins the game shared at `address` (`host` or `host:port`, then
    /// [`PASSWORD_MARK`] and the edit password) as `name`. The game replaces
    /// this computer's once it arrives, once the sharing computer lets this
    /// one in.
    pub fn join(address: &str, name: &str) -> Result<Self, String> {
        let (address, password) = split_password(address);
        let password = password.filter(|p| !p.is_empty()).ok_or_else(|| {
            format!("add the edit password after the address, like 192.168.1.20{PASSWORD_MARK}abcde-fghij")
        })?;
        let address = with_port_for_edits(address);
        let target: SocketAddr = address
            .to_socket_addrs()
            .map_err(|e| format!("could not find that address: {e}"))?
            .next()
            .ok_or_else(|| "could not find that address".to_owned())?;
        let stream = TcpStream::connect_timeout(&target, Duration::from_secs(5))
            .map_err(|e| format!("could not reach the host: {e}"))?;
        let mut conn = Conn::new(stream, End::Joining).map_err(|e| e.to_string())?;
        conn.send(&json!({ "t": "hello", "name": name, "version": EDIT_PROTOCOL, "password": password }));
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
                waiting: false,
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
            Side::Host(host) => {
                host.peers.iter().filter(|p| p.allowed).filter_map(|p| p.name.clone()).collect()
            }
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

    /// Whether this guest gave the right password and waits to be let in.
    pub fn waiting(&self) -> bool {
        matches!(&self.side, Side::Guest(guest) if guest.waiting && !guest.welcomed)
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
                    ticket: 0,
                    allowed: false,
                    since: Instant::now(),
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
                peer.fresh |= peer.allowed;
            }
        }
        let mut asking =
            self.peers.iter().filter(|p| p.name.is_some() && !p.allowed && !p.conn.closed).count();
        for peer in &mut self.peers {
            if peer.name.is_none() && peer.since.elapsed() > HELLO_TIMEOUT {
                peer.conn.closed = true;
            }
            for message in peer.conn.receive() {
                match message.get("t").and_then(Json::as_str) {
                    Some("hello") if peer.name.is_none() => {
                        if message.get("version").and_then(Json::as_u64) != Some(EDIT_PROTOCOL) {
                            peer.conn.send(&json!({ "t": "bye", "why": "a different version of playlibre" }));
                            peer.conn.flush();
                            peer.conn.closed = true;
                            continue;
                        }
                        let given = message.get("password").and_then(Json::as_str).unwrap_or("");
                        if !same_password(given, &self.password) {
                            peer.conn
                                .send(&json!({ "t": "bye", "why": "that isn't this game's edit password" }));
                            peer.conn.flush();
                            peer.conn.closed = true;
                            continue;
                        }
                        if asking >= MAX_ASKING {
                            peer.conn
                                .send(&json!({ "t": "bye", "why": "too many people are waiting to join" }));
                            peer.conn.flush();
                            peer.conn.closed = true;
                            continue;
                        }
                        let name: String = message
                            .get("name")
                            .and_then(Json::as_str)
                            .unwrap_or("")
                            .trim()
                            .chars()
                            .filter(|c| !c.is_control())
                            .take(40)
                            .collect();
                        let name = if name.is_empty() { "Someone".to_owned() } else { name };
                        log.push(format!("👥 {name} asks to edit this game"));
                        peer.name = Some(name);
                        peer.ticket = self.next_ticket;
                        self.next_ticket += 1;
                        asking += 1;
                        peer.conn.send(&json!({ "t": "waiting" }));
                    }
                    // Changes made before they had this game's ids are dropped.
                    Some("ops") if peer.allowed && !peer.fresh => {
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
            if !peer.allowed {
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
            if let Some(name) = peer.name.filter(|_| peer.allowed) {
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
                    self.waiting = false;
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
                Some("waiting") if !self.welcomed => {
                    log.push("👥 Waiting for the person sharing the game to let you in".to_owned());
                    self.waiting = true;
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
