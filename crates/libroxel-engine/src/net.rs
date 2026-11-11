//! Playing over a network.
//!
//! One computer **hosts**: it runs the game's scripts and physics for
//! everyone, exactly as a game on one computer does, with a player for each
//! computer that joins. Other computers **join**: they run only the
//! `LocalScript`s of their own player, send what their player does (the
//! movement keys, clicks, prompts, tools and `FireServer`), and show the
//! world the host sends them.
//!
//! The connection is TCP, carrying one JSON message per line. After every
//! step the host compares each instance it shares (everything under
//! Workspace, Players, ReplicatedStorage, StarterGui, Lighting and Teams,
//! except cameras and players' GUIs) with what it last sent, using the
//! instances' revisions to skip unchanged ones, and sends the differences:
//! instances created (`c`), updated (`u`) and removed (`d`). A computer that
//! has just joined gets everything as creations. Instances are named by the
//! host's ids; the joined computer keeps a map to its own.
//!
//! Characters move on the host, so a joined player sees their own
//! character's moves once the host has made them.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;

use libroxel_instance::serde_json::{self, Map, Value as Json, json};
use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue, Vec3, place};
use libroxel_script::{NetMessage, NetRole, SharedTree, map_instances};

use crate::character::{Character, CharacterInput};
use crate::session::{Avatar, Blast, PlaySession};

/// The port a hosted game listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 7420;
/// The version of the messages. Computers with different versions can't play together.
pub const PROTOCOL: u64 = 1;
/// How much unsent data a connection may build up before it is dropped.
const MAX_BACKLOG: usize = 64 * 1024 * 1024;

/// The services whose contents every computer sees.
const SHARED: [ClassKind; 6] = [
    ClassKind::Workspace,
    ClassKind::Players,
    ClassKind::ReplicatedStorage,
    ClassKind::StarterGui,
    ClassKind::Lighting,
    ClassKind::Teams,
];

/// Classes that stay on the computer they are on: each has its own camera
/// and its own player GUIs.
fn unshared(class: ClassKind) -> bool {
    matches!(class, ClassKind::Camera | ClassKind::PlayerGui)
}

// ---------------------------------------------------------------------------
// Connections
// ---------------------------------------------------------------------------

/// A connection that never blocks, carrying one JSON message per line.
pub(crate) struct Conn {
    stream: TcpStream,
    received: Vec<u8>,
    unsent: Vec<u8>,
    closed: bool,
}

impl Conn {
    fn new(stream: TcpStream) -> std::io::Result<Self> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        Ok(Self { stream, received: Vec::new(), unsent: Vec::new(), closed: false })
    }

    fn send(&mut self, message: &Json) {
        if self.closed {
            return;
        }
        if serde_json::to_writer(&mut self.unsent, message).is_err() {
            return;
        }
        self.unsent.push(b'\n');
        if self.unsent.len() > MAX_BACKLOG {
            self.closed = true;
        }
    }

    fn flush(&mut self) {
        while !self.closed && !self.unsent.is_empty() {
            match self.stream.write(&self.unsent) {
                Ok(0) => self.closed = true,
                Ok(n) => {
                    self.unsent.drain(..n);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => self.closed = true,
            }
        }
    }

    /// Every whole message that has arrived.
    fn receive(&mut self) -> Vec<Json> {
        let mut buf = [0u8; 64 * 1024];
        while !self.closed {
            match self.stream.read(&mut buf) {
                Ok(0) => self.closed = true,
                Ok(n) => self.received.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => self.closed = true,
            }
        }
        let mut messages = Vec::new();
        while let Some(end) = self.received.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.received.drain(..=end).collect();
            if let Ok(message) = serde_json::from_slice::<Json>(&line[..line.len() - 1]) {
                messages.push(message);
            }
        }
        messages
    }
}

/// The session's network side.
#[derive(Default)]
pub(crate) enum Link {
    /// A game on one computer.
    #[default]
    Alone,
    Host(Box<Hosting>),
    Client(Box<Joined>),
}

/// A hosted game: who has joined, and what they have been sent.
pub(crate) struct Hosting {
    listener: TcpListener,
    peers: Vec<Peer>,
    replicator: Replicator,
}

struct Peer {
    conn: Conn,
    /// Their player, once they have said hello.
    player: Option<InstanceId>,
    /// Whether they still need the whole world.
    fresh: bool,
}

/// A joined game: the connection and how the host's ids map to ours.
pub(crate) struct Joined {
    conn: Conn,
    /// The host's ids (as bits) to ours, and back.
    ids: HashMap<u64, InstanceId>,
    back: HashMap<InstanceId, u64>,
    /// Our player, in the host's ids.
    me: Option<u64>,
    /// Tools whose local scripts have been started.
    started: HashSet<InstanceId>,
    /// The input last sent.
    sent_input: Option<CharacterInput>,
    /// Whether the lost connection has been reported.
    reported: bool,
}

/// Something a joined player did, for the host to act on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ClientSays {
    /// Pressed a hotbar number (from 0).
    Tool(usize),
    /// Clicked a part.
    Click(InstanceId),
    /// Pressed (or let go of) the mouse button over the world.
    Activate(bool),
    /// Used (or let go of) a proximity prompt.
    Prompt { prompt: InstanceId, ended: bool },
}

// ---------------------------------------------------------------------------
// Replication
// ---------------------------------------------------------------------------

/// What was last sent about one instance.
#[derive(Debug, Clone)]
struct Sent {
    revision: u64,
    parent: Option<InstanceId>,
    name: String,
    props: Vec<(Property, PropertyValue)>,
    attributes: BTreeMap<String, PropertyValue>,
    tags: Vec<String>,
}

fn snapshot(tree: &InstanceTree, id: InstanceId) -> Option<Sent> {
    let class = tree.class(id).ok()?;
    let props = class
        .properties()
        .iter()
        .filter(|p| **p != Property::Name)
        .filter_map(|p| Some((*p, tree.get(id, *p).ok()?)))
        .collect();
    Some(Sent {
        revision: tree.revision(id).ok()?,
        parent: tree.parent(id).ok()?,
        name: tree.name(id).ok()?.to_owned(),
        props,
        attributes: tree.attributes(id).ok()?.clone(),
        tags: tree.tags(id).ok()?.to_vec(),
    })
}

fn bits(id: InstanceId) -> Json {
    json!(id.to_bits())
}

fn wire_value(class: ClassKind, property: Property, value: &PropertyValue) -> Json {
    match value {
        PropertyValue::Ref(Some(target)) => json!({ "$i": target.to_bits() }),
        PropertyValue::Ref(None) => Json::Null,
        // Server scripts' code stays on the host.
        PropertyValue::String(_) if class == ClassKind::Script && property == Property::Source => json!(""),
        other => place::value_to_json(other),
    }
}

/// Keeps track of what every joined computer has been sent.
#[derive(Default)]
pub(crate) struct Replicator {
    sent: HashMap<InstanceId, Sent>,
}

impl Replicator {
    /// What changed since the last call, as operations; updates the record.
    fn delta(&mut self, tree: &InstanceTree) -> Vec<Json> {
        let mut ops = Vec::new();
        let mut seen = HashSet::new();
        for class in SHARED {
            if let Some(service) = tree.service(class) {
                self.visit(tree, service, true, &mut ops, &mut seen);
            }
        }
        let gone: Vec<InstanceId> = self.sent.keys().copied().filter(|id| !seen.contains(id)).collect();
        for id in gone {
            self.sent.remove(&id);
            ops.push(json!({ "d": bits(id) }));
        }
        ops
    }

    /// Everything shared, as operations for a computer that has nothing.
    fn everything(tree: &InstanceTree) -> Vec<Json> {
        Replicator::default().delta(tree)
    }

    fn visit(
        &mut self,
        tree: &InstanceTree,
        id: InstanceId,
        service: bool,
        ops: &mut Vec<Json>,
        seen: &mut HashSet<InstanceId>,
    ) {
        let Ok(class) = tree.class(id) else { return };
        if unshared(class) || tree.is_destroyed(id).unwrap_or(true) {
            return;
        }
        seen.insert(id);
        let revision = tree.revision(id).unwrap_or(0);
        let parent = tree.parent(id).ok().flatten();
        let old = self.sent.get(&id);
        if old.is_none_or(|old| old.revision != revision || old.parent != parent)
            && let Some(now) = snapshot(tree, id)
        {
            if let Some(op) = Self::op(class, id, old, &now, service) {
                ops.push(op);
            }
            self.sent.insert(id, now);
        }
        for child in tree.children(id).unwrap_or_default() {
            self.visit(tree, *child, false, ops, seen);
        }
    }

    /// The operation that brings `old` (nothing, for a new instance) to `now`.
    fn op(class: ClassKind, id: InstanceId, old: Option<&Sent>, now: &Sent, service: bool) -> Option<Json> {
        let mut op = Map::new();
        let mut props = Map::new();
        for (property, value) in &now.props {
            let before = old.and_then(|o| o.props.iter().find(|(p, _)| p == property)).map(|(_, v)| v);
            if before != Some(value) {
                props.insert(property.file_name().to_owned(), wire_value(class, *property, value));
            }
        }
        let mut attributes = Map::new();
        for (name, value) in &now.attributes {
            if old.and_then(|o| o.attributes.get(name)) != Some(value) {
                attributes.insert(name.clone(), place::attribute_to_json(value));
            }
        }
        if let Some(old) = old {
            for name in old.attributes.keys().filter(|n| !now.attributes.contains_key(*n)) {
                attributes.insert(name.clone(), Json::Null);
            }
        }
        match old {
            None if !service => {
                op.insert("c".into(), bits(id));
                op.insert("k".into(), json!(class.file_name()));
                op.insert("n".into(), json!(now.name));
                op.insert("p".into(), now.parent.map_or(Json::Null, bits));
            }
            _ => {
                op.insert("u".into(), bits(id));
                if old.is_none_or(|o| o.name != now.name) && !service {
                    op.insert("n".into(), json!(now.name));
                }
                if old.is_some_and(|o| o.parent != now.parent) && !service {
                    op.insert("p".into(), now.parent.map_or(Json::Null, bits));
                }
            }
        }
        if !props.is_empty() {
            op.insert("v".into(), Json::Object(props));
        }
        if !attributes.is_empty() {
            op.insert("a".into(), Json::Object(attributes));
        }
        if old.map_or(!now.tags.is_empty(), |o| o.tags != now.tags) {
            op.insert("g".into(), json!(now.tags));
        }
        // An update that changes nothing isn't worth sending.
        (op.len() > 1 || op.contains_key("c")).then_some(Json::Object(op))
    }
}

impl Joined {
    fn local(&self, bits: u64) -> Option<InstanceId> {
        self.ids.get(&bits).copied()
    }

    /// Our id for one of the host's in a message.
    fn local_of(&self, json: &Json) -> Option<InstanceId> {
        json.as_u64().and_then(|b| self.local(b))
    }

    /// Applies the host's operations to our tree.
    fn apply(&mut self, tree: &mut InstanceTree, ops: &[Json]) {
        let mut links: Vec<(InstanceId, Property, Option<u64>)> = Vec::new();
        for op in ops {
            if let Some(host_id) = op.get("c").and_then(Json::as_u64) {
                let Some(class) = op.get("k").and_then(Json::as_str).and_then(ClassKind::from_file_name)
                else {
                    continue;
                };
                let name = op.get("n").and_then(Json::as_str).unwrap_or("Instance");
                let Ok(id) = tree.create(class, name) else { continue };
                self.ids.insert(host_id, id);
                self.back.insert(id, host_id);
                Self::fields(tree, id, op, &mut links);
                let parent = op.get("p").and_then(|p| self.local_of(p));
                let _ = tree.set_parent(id, parent);
            } else if let Some(id) = op.get("u").and_then(|u| self.local_of(u)) {
                if let Some(name) = op.get("n").and_then(Json::as_str) {
                    let _ = tree.set_name(id, name);
                }
                Self::fields(tree, id, op, &mut links);
                if let Some(parent) = op.get("p") {
                    let _ = tree.set_parent(id, self.local_of(parent));
                }
            } else if let Some(host_id) = op.get("d").and_then(Json::as_u64)
                && let Some(id) = self.ids.remove(&host_id)
            {
                self.back.remove(&id);
                let _ = tree.destroy(id);
            }
        }
        // Links last: they may point at instances made later in the batch.
        for (id, property, target) in links {
            let target = target.and_then(|t| self.local(t));
            let _ = tree.set(id, property, PropertyValue::Ref(target));
        }
    }

    fn fields(
        tree: &mut InstanceTree,
        id: InstanceId,
        op: &Json,
        links: &mut Vec<(InstanceId, Property, Option<u64>)>,
    ) {
        if let Some(props) = op.get("v").and_then(Json::as_object) {
            for (file_name, value) in props {
                let Some(property) = Property::from_file_name(file_name) else { continue };
                if property.expected_kind() == "ref" {
                    links.push((id, property, value.get("$i").and_then(Json::as_u64)));
                } else if let Some(value) = place::value_from_json(property, value) {
                    let _ = tree.set(id, property, value);
                }
            }
        }
        if let Some(attributes) = op.get("a").and_then(Json::as_object) {
            for (name, value) in attributes {
                let _ = tree.set_attribute(id, name, place::attribute_from_json(value));
            }
        }
        if let Some(tags) = op.get("g").and_then(Json::as_array) {
            let want: Vec<&str> = tags.iter().filter_map(Json::as_str).collect();
            let have: Vec<String> = tree.tags(id).map(|t| t.to_vec()).unwrap_or_default();
            for tag in have.iter().filter(|t| !want.contains(&t.as_str())) {
                let _ = tree.remove_tag(id, tag);
            }
            for tag in want {
                let _ = tree.add_tag(id, tag);
            }
        }
    }

    /// Rewrites instances in script values from the host's ids to ours.
    fn to_local(&self, json: &mut Json) {
        map_instances(json, &mut |id| self.local(id.to_bits()));
    }

    /// Rewrites instances in script values from our ids to the host's.
    fn to_host(&self, json: &mut Json) {
        map_instances(json, &mut |id| self.back.get(&id).map(|b| InstanceId::from_bits(*b)));
    }
}

fn move_json(input: CharacterInput) -> Json {
    json!({
        "t": "input",
        "move": [input.move_dir.x, input.move_dir.z],
        "jump": input.jump,
        "throttle": input.throttle,
        "steer": input.steer,
    })
}

fn input_from(message: &Json) -> CharacterInput {
    let f = |v: Option<&Json>| v.and_then(Json::as_f64).unwrap_or(0.0) as f32;
    let dir = message.get("move").and_then(Json::as_array);
    let at = |i: usize| f(dir.and_then(|d| d.get(i)));
    let mut move_dir = Vec3::new(at(0), 0.0, at(1));
    if !move_dir.x.is_finite() || !move_dir.z.is_finite() {
        move_dir = Vec3::ZERO;
    }
    CharacterInput {
        move_dir,
        jump: message.get("jump").and_then(Json::as_bool).unwrap_or(false),
        throttle: f(message.get("throttle")).clamp(-1.0, 1.0),
        steer: f(message.get("steer")).clamp(-1.0, 1.0),
    }
}

/// `host:port`, with the default port when none is given.
fn with_port(address: &str) -> String {
    let address = address.trim();
    let has_port =
        address.rsplit_once(':').is_some_and(|(_, p)| p.parse::<u16>().is_ok()) && !address.ends_with(']');
    if has_port { address.to_owned() } else { format!("{address}:{DEFAULT_PORT}") }
}

// ---------------------------------------------------------------------------
// The session's network side
// ---------------------------------------------------------------------------

impl PlaySession {
    /// Hosts the game on `port` (0 picks a free one): starts every script,
    /// adds a player for this computer when `play_here`, and lets other
    /// computers join with [`join`](Self::join).
    pub fn host(
        tree: SharedTree,
        port: u16,
        saved_data: Option<&str>,
        play_here: bool,
    ) -> Result<Self, String> {
        let listener = TcpListener::bind(("0.0.0.0", port))
            .map_err(|e| format!("could not host on port {port}: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let mut session = Self::start_inner(tree, play_here, saved_data, NetRole::Server)?;
        session.link =
            Link::Host(Box::new(Hosting { listener, peers: Vec::new(), replicator: Default::default() }));
        Ok(session)
    }

    /// Joins the game hosted at `address` (`host` or `host:port`) as `name`,
    /// showing it in `tree`, which is emptied first. The world arrives over
    /// the next few steps.
    pub fn join(tree: SharedTree, address: &str, name: &str) -> Result<Self, String> {
        let address = with_port(address);
        let target: SocketAddr = address
            .to_socket_addrs()
            .map_err(|e| format!("could not find {address}: {e}"))?
            .next()
            .ok_or_else(|| format!("could not find {address}"))?;
        let stream = TcpStream::connect_timeout(&target, Duration::from_secs(5))
            .map_err(|e| format!("could not reach {address}: {e}"))?;
        let mut conn = Conn::new(stream).map_err(|e| e.to_string())?;
        conn.send(&json!({ "t": "hello", "name": name, "version": PROTOCOL }));
        conn.flush();
        *tree.borrow_mut() = InstanceTree::new();
        let mut session = Self::empty(tree, NetRole::Client)?;
        session.link = Link::Client(Box::new(Joined {
            conn,
            ids: HashMap::new(),
            back: HashMap::new(),
            me: None,
            started: HashSet::new(),
            sent_input: None,
            reported: false,
        }));
        session.info(format!("🔗 Joining {address}"));
        Ok(session)
    }

    /// Where a hosted game listens.
    pub fn host_address(&self) -> Option<SocketAddr> {
        match &self.link {
            Link::Host(host) => host.listener.local_addr().ok(),
            _ => None,
        }
    }

    /// How many computers have joined this hosted game.
    pub fn joined_count(&self) -> usize {
        match &self.link {
            Link::Host(host) => host.peers.iter().filter(|p| p.player.is_some()).count(),
            _ => 0,
        }
    }

    /// Whether this is a joined game whose connection to the host is gone.
    pub fn disconnected(&self) -> bool {
        matches!(&self.link, Link::Client(joined) if joined.conn.closed)
    }

    /// Whether this computer has joined someone else's game.
    pub fn is_joined(&self) -> bool {
        matches!(self.link, Link::Client(_))
    }

    pub(crate) fn send_to_server(&mut self, says: ClientSays) {
        let Link::Client(joined) = &mut self.link else { return };
        let host_id = |id: InstanceId| joined.back.get(&id).copied();
        let message = match says {
            ClientSays::Tool(slot) => json!({ "t": "tool", "slot": slot }),
            ClientSays::Activate(down) => json!({ "t": "activate", "down": down }),
            ClientSays::Click(part) => match host_id(part) {
                Some(p) => json!({ "t": "click", "p": p }),
                None => return,
            },
            ClientSays::Prompt { prompt, ended } => match host_id(prompt) {
                Some(p) => json!({ "t": "prompt", "p": p, "ended": ended }),
                None => return,
            },
        };
        joined.conn.send(&message);
    }

    // --- Hosting -----------------------------------------------------------

    /// Lets new computers in and acts on what joined players sent.
    pub(crate) fn serve_incoming(&mut self) {
        let Link::Host(mut host) = std::mem::take(&mut self.link) else { return };
        while let Ok((stream, _)) = host.listener.accept() {
            if let Ok(conn) = Conn::new(stream) {
                host.peers.push(Peer { conn, player: None, fresh: true });
            }
        }
        let tree = self.runtime.tree().clone();
        let live = |id: InstanceId| {
            let t = tree.borrow();
            t.exists(id) && !t.is_destroyed(id).unwrap_or(true)
        };
        let id_in = |message: &Json, key: &str| {
            message.get(key).and_then(Json::as_u64).map(InstanceId::from_bits).filter(|id| live(*id))
        };
        for peer in &mut host.peers {
            for message in peer.conn.receive() {
                let kind = message.get("t").and_then(Json::as_str).unwrap_or("");
                let Some(player) = peer.player else {
                    if kind == "hello" {
                        if message.get("version").and_then(Json::as_u64) != Some(PROTOCOL) {
                            peer.conn
                                .send(&json!({ "t": "refused", "why": "a different version of Libroxel" }));
                            peer.conn.closed = true;
                            break;
                        }
                        let name: String = message
                            .get("name")
                            .and_then(Json::as_str)
                            .unwrap_or("Player")
                            .chars()
                            .filter(|c| c.is_alphanumeric() || *c == '_')
                            .take(20)
                            .collect();
                        let name = if name.is_empty() { "Player".to_owned() } else { name };
                        let player = self.add_player(&name, true);
                        peer.player = Some(player);
                        let shown = tree.borrow().name(player).unwrap_or("A player").to_owned();
                        self.info(format!("{shown} joined"));
                    }
                    continue;
                };
                let Some(index) = self.avatar_of(player) else { continue };
                match kind {
                    "input" => self.avatars[index].input = input_from(&message),
                    "fire" => {
                        if let Some(remote) = id_in(&message, "r") {
                            let args = message.get("a").cloned().unwrap_or(Json::Array(Vec::new()));
                            let errors = self.runtime.deliver_server_event(remote, player, &args);
                            self.collect(errors);
                        }
                    }
                    "invoke" => {
                        let call = message.get("call").and_then(Json::as_u64).unwrap_or(0);
                        if let Some(remote) = id_in(&message, "r") {
                            let args = message.get("a").cloned().unwrap_or(Json::Array(Vec::new()));
                            let errors = self.runtime.deliver_invoke(call, remote, player, &args);
                            self.collect(errors);
                        }
                    }
                    "click" => {
                        if let Some(part) = id_in(&message, "p") {
                            self.click_part_for(index, part);
                        }
                    }
                    "prompt" => {
                        if let Some(prompt) = id_in(&message, "p") {
                            let ended = message.get("ended").and_then(Json::as_bool).unwrap_or(false);
                            self.remote_prompt(index, prompt, ended);
                        }
                    }
                    "tool" => {
                        if let Some(slot) = message.get("slot").and_then(Json::as_u64) {
                            self.toggle_tool_for(index, slot as usize);
                        }
                    }
                    "activate" => {
                        let down = message.get("down").and_then(Json::as_bool).unwrap_or(false);
                        let tool = self.avatars[index].character.and_then(|c| {
                            tree.borrow().find_first_child_of_class(c.model, ClassKind::Tool).ok().flatten()
                        });
                        if let Some(tool) = tool {
                            let errors = self.runtime.activate_tool(tool, down);
                            self.collect(errors);
                        }
                    }
                    _ => {}
                }
            }
        }
        // Players whose computers went away leave the game.
        let (gone, kept): (Vec<Peer>, Vec<Peer>) =
            std::mem::take(&mut host.peers).into_iter().partition(|p| p.conn.closed);
        host.peers = kept;
        for peer in gone {
            if let Some(player) = peer.player {
                let shown = tree.borrow().name(player).unwrap_or("A player").to_owned();
                self.leave(player);
                self.info(format!("{shown} left"));
            }
        }
        self.link = Link::Host(host);
    }

    /// Sends joined computers what changed this step and what scripts sent them.
    pub(crate) fn serve_outgoing(&mut self) {
        let Link::Host(mut host) = std::mem::take(&mut self.link) else { return };
        let tree = self.runtime.tree().clone();
        let messages = self.runtime.take_net_messages();
        if host.peers.iter().all(|p| p.player.is_none()) {
            // No one to send to: forget what was sent, so the first to join gets everything.
            host.replicator = Replicator::default();
            for peer in &mut host.peers {
                peer.conn.flush();
            }
            self.link = Link::Host(host);
            return;
        }
        let delta = host.replicator.delta(&tree.borrow());
        let everything = host
            .peers
            .iter()
            .any(|p| p.fresh && p.player.is_some())
            .then(|| Replicator::everything(&tree.borrow()));
        for peer in &mut host.peers {
            let Some(player) = peer.player else { continue };
            if peer.fresh {
                let t = tree.borrow();
                let services: Map<String, Json> = SHARED
                    .iter()
                    .filter_map(|c| Some((c.file_name().to_owned(), bits(t.service(*c)?))))
                    .collect();
                peer.conn.send(&json!({ "t": "welcome", "services": services }));
                peer.conn.send(&json!({ "t": "rep", "ops": everything.clone().unwrap_or_default() }));
                peer.conn.send(&json!({ "t": "you", "player": bits(player) }));
                peer.fresh = false;
            } else if !delta.is_empty() {
                peer.conn.send(&json!({ "t": "rep", "ops": delta }));
            }
            for message in &messages {
                match message {
                    NetMessage::ClientEvent { remote, player: to, args }
                        if to.is_none_or(|p| p == player) =>
                    {
                        peer.conn.send(&json!({ "t": "event", "r": bits(*remote), "a": args }));
                    }
                    NetMessage::InvokeResult { call, player: to, ok, values } if *to == player => {
                        peer.conn.send(&json!({ "t": "result", "call": call, "ok": ok, "a": values }));
                    }
                    _ => {}
                }
            }
            peer.conn.flush();
        }
        self.link = Link::Host(host);
    }

    // --- Joined ------------------------------------------------------------

    /// One step of a joined game: take in what the host sent, run our local
    /// scripts, and send our input and what our scripts sent.
    pub(crate) fn client_step(&mut self, dt: f32, input: CharacterInput) {
        let Link::Client(mut joined) = std::mem::take(&mut self.link) else { return };
        let tree = self.runtime.tree().clone();
        for message in joined.conn.receive() {
            match message.get("t").and_then(Json::as_str).unwrap_or("") {
                "welcome" => {
                    let t = tree.borrow();
                    let services =
                        message.get("services").and_then(Json::as_object).cloned().unwrap_or_default();
                    for (file_name, host_id) in services {
                        let (Some(class), Some(host_id)) =
                            (ClassKind::from_file_name(&file_name), host_id.as_u64())
                        else {
                            continue;
                        };
                        if let Some(id) = t.service(class) {
                            joined.ids.insert(host_id, id);
                            joined.back.insert(id, host_id);
                        }
                    }
                }
                "rep" => {
                    let ops = message.get("ops").and_then(Json::as_array).cloned().unwrap_or_default();
                    joined.apply(&mut tree.borrow_mut(), &ops);
                }
                "you" => joined.me = message.get("player").and_then(Json::as_u64),
                "event" => {
                    if let Some(remote) = message.get("r").and_then(|r| joined.local_of(r)) {
                        let mut args = message.get("a").cloned().unwrap_or(Json::Array(Vec::new()));
                        joined.to_local(&mut args);
                        let errors = self.runtime.deliver_client_event(remote, &args);
                        self.collect(errors);
                    }
                }
                "result" => {
                    let call = message.get("call").and_then(Json::as_u64).unwrap_or(0);
                    let ok = message.get("ok").and_then(Json::as_bool).unwrap_or(false);
                    let mut values = message.get("a").cloned().unwrap_or(Json::Null);
                    joined.to_local(&mut values);
                    self.runtime.deliver_invoke_result(call, ok, values);
                }
                "refused" => {
                    let why = message.get("why").and_then(Json::as_str).unwrap_or("no reason given");
                    self.warn(format!("The game turned us away: {why}"));
                }
                _ => {}
            }
        }
        if joined.conn.closed && !joined.reported {
            joined.reported = true;
            self.warn("Lost the connection to the game");
        }

        self.adopt_player(&mut joined);

        let changes = tree.borrow_mut().drain_changes();
        let errors = self.runtime.fire_changes(&changes);
        self.collect(errors);
        let errors = self.runtime.step(dt as f64);
        self.collect(errors);
        if let Some(index) = self.avatars.iter().position(|a| !a.remote) {
            self.advance_hold(index, dt);
            self.refresh_hotbar(index);
        }
        // Explosions only flash here: the host does what they do.
        let fresh: Vec<(InstanceId, Vec3, f32)> = {
            let t = tree.borrow();
            t.descendants_of_class(t.workspace(), ClassKind::Explosion)
                .into_iter()
                .filter(|e| !self.exploded.contains(e))
                .map(|e| {
                    let at = t.get_vec3(e, Property::Position).unwrap_or_default();
                    (e, at, t.get_number(e, Property::BlastRadius).unwrap_or(4.0) as f32)
                })
                .collect()
        };
        for (explosion, position, radius) in fresh {
            self.exploded.insert(explosion);
            self.blasts.push(Blast { position, radius, age: 0.0 });
        }
        self.blasts.retain_mut(|b| {
            b.age += dt;
            b.age < crate::session::BLAST_TIME
        });
        self.emits.extend(self.runtime.take_emit_requests());

        if joined.sent_input != Some(input) {
            joined.sent_input = Some(input);
            joined.conn.send(&move_json(input));
        }
        for message in self.runtime.take_net_messages() {
            match message {
                NetMessage::ServerEvent { remote, mut args } => {
                    let Some(r) = joined.back.get(&remote).copied() else { continue };
                    joined.to_host(&mut args);
                    joined.conn.send(&json!({ "t": "fire", "r": r, "a": args }));
                }
                NetMessage::Invoke { call, remote, mut args } => {
                    let Some(r) = joined.back.get(&remote).copied() else { continue };
                    joined.to_host(&mut args);
                    joined.conn.send(&json!({ "t": "invoke", "call": call, "r": r, "a": args }));
                }
                _ => {}
            }
        }
        joined.conn.flush();
        self.link = Link::Client(joined);
    }

    /// Keeps our player, character and tools in step with what the host sent.
    fn adopt_player(&mut self, joined: &mut Joined) {
        let tree = self.runtime.tree().clone();
        let Some(player) = joined.me.and_then(|me| joined.local(me)) else { return };
        if !self.avatars.iter().any(|a| a.player == player) {
            let mut avatar = Avatar::new(player, false);
            {
                let mut t = tree.borrow_mut();
                let gui = t.create(ClassKind::PlayerGui, "PlayerGui").expect("player GUIs can be created");
                let _ = t.set_parent(gui, Some(player));
                avatar.gui = Some(gui);
            }
            self.avatars.push(avatar);
            self.runtime.set_local_player(Some(player));
            let index = self.avatars.len() - 1;
            self.copy_starter_gui(index, false);
            let name = tree.borrow().name(player).unwrap_or("").to_owned();
            self.info(format!("Joined as {name}"));
        }
        let Some(index) = self.avatars.iter().position(|a| a.player == player) else { return };
        if self.avatars[index].backpack.is_none() {
            let t = tree.borrow();
            self.avatars[index].backpack =
                t.find_first_child_of_class(player, ClassKind::Backpack).ok().flatten();
        }
        let model = tree.borrow().get_ref(player, Property::Character);
        if model != self.avatars[index].character.map(|c| c.model) {
            let character = model.and_then(|m| Character::from_model(&tree.borrow(), m));
            if character.is_some() || model.is_none() {
                let had = std::mem::replace(&mut self.avatars[index].character, character);
                if let Some(c) = character {
                    if had.is_some() {
                        self.copy_starter_gui(index, true);
                    }
                    let errors = self.runtime.character_added(player, c.model);
                    self.collect(errors);
                }
            }
        }
        // Start the local scripts of tools as they arrive.
        let avatar = &self.avatars[index];
        let tools: Vec<InstanceId> = {
            let t = tree.borrow();
            [avatar.backpack, avatar.character.map(|c| c.model)]
                .into_iter()
                .flatten()
                .flat_map(|c| t.children(c).unwrap_or_default().to_vec())
                .filter(|c| t.class(*c) == Ok(ClassKind::Tool))
                .collect()
        };
        for tool in tools {
            if joined.started.insert(tool) {
                let errors = self.runtime.run_local_scripts(tool);
                self.collect(errors);
            }
        }
    }
}
