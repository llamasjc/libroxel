//! Playing over a network.
//!
//! One computer **hosts**: it runs the game's scripts and physics for
//! everyone, exactly as a game on one computer does, with a player for each
//! computer that joins. Other computers **join**: they run only the
//! `LocalScript`s of their own player, send what their player does (the
//! movement keys, clicks, prompts, tools and `FireServer`), and show the
//! world the host sends them.
//!
//! The connection is TCP, encrypted (see [`Conn`]), carrying one JSON
//! message per line. After every
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

use std::collections::{BTreeMap, VecDeque};

// Fast-hashing maps: the host looks every instance up in these each step,
// and a joined computer every instance it is sent.
use bevy::platform::collections::{HashMap, HashSet};
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

use libroxel_instance::serde_json::{self, Map, Value as Json, json};
use libroxel_instance::{
    ClassKind, InstanceId, InstanceTree, ModelStreamingMode, Property, PropertyValue, StreamOutBehavior,
    Vec3, place,
};
use libroxel_script::{NetMessage, NetRole, SharedTree, map_instances};

use crate::character::{Character, CharacterInput};
use crate::session::{Avatar, Blast, PlaySession};

/// The port a hosted game listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 7420;
/// The version of the messages. Computers with different versions can't play together.
/// 2: long messages are compressed (see [`Conn::send`]).
/// 3: everything is encrypted (see [`Conn`]).
/// 4: streamed terrain comes in chunks (`xs`).
pub const PROTOCOL: u64 = 4;
/// Messages longer than this many bytes are sent compressed.
const COMPRESS_OVER: usize = 4096;
/// How much unsent data a connection may build up before it is dropped,
/// and the longest message it takes in.
const MAX_BACKLOG: usize = 64 * 1024 * 1024;
const MAX_QUEUED: usize = 100_000;
const SEND_AHEAD: usize = 2 * 1024 * 1024;
const READ_BUDGET: usize = 1024 * 1024;
const CHUNK: usize = 500;
const APPLY_BUDGET: Duration = Duration::from_millis(8);
/// The same while the app's loading screen is up: nothing to keep smooth
/// but its animation, so fewer, fuller frames bring the world in sooner on
/// a slow device (each frame has its own fixed cost).
const LOADING_APPLY_BUDGET: Duration = Duration::from_millis(30);
/// How many instances a streamed player gets when joining, and then each
/// step at most, nearest first.
const STREAM_FIRST: usize = 5_000;
const STREAM_STEP: usize = 2_000;
/// How many terrain chunks go each step at most, and how many steps apart
/// a streamed player's terrain is looked at when it hasn't changed.
const TERRAIN_STEP: usize = 256;
const TERRAIN_LOOK: u64 = 15;
/// A game with more parts than this streams, even without `StreamingEnabled`,
/// letting go of parts a little past the radius.
const BIG_MAP: usize = 5_000;
/// The longest message a host takes from a joined computer. What players
/// send (their moves, clicks and `FireServer` arguments) is small; anything
/// longer is someone trying to fill the host's memory.
pub(crate) const MAX_FROM_GUEST: usize = 4 * 1024 * 1024;
/// How many messages a second a joined computer may send, and how many it
/// may save up for a burst. A player's computer sends a few each frame.
const GUEST_MESSAGES_PER_SECOND: f32 = 600.0;
const GUEST_MESSAGE_BURST: f32 = 1200.0;
/// How many of those may be events, invokes, clicks, prompts and tool use,
/// which run the game's scripts.
const GUEST_ACTIONS_PER_SECOND: f32 = 120.0;
const GUEST_ACTION_BURST: f32 = 240.0;
/// How many computers may be connected without having said hello yet.
const MAX_WAITING: usize = 16;
/// How long a computer may stay connected without saying hello.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// Each side says something at least this often (a `ka`, "keep alive",
/// when it has nothing else to say), so the other can tell a quiet game
/// from a connection that went away without hanging up.
const KEEP_ALIVE: Duration = Duration::from_secs(1);
/// How long the other side may say nothing before the connection counts
/// as lost (Wi-Fi that dropped, a computer that went to sleep). Only for a
/// side that sends keep-alives: older copies don't.
pub const SILENCE_LIMIT: Duration = Duration::from_secs(20);

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

/// Whether a joined computer can know of `id`: it is in one of the
/// [`SHARED`] services and not inside a camera or someone's GUIs. Like
/// the reference platform, an instance a player's computer names in `FireServer` or
/// `InvokeServer` reaches the game's scripts only when it is one of these,
/// so a cheat can't hand a server script something from `ServerStorage`.
fn shared_with_guests(tree: &InstanceTree, id: InstanceId) -> bool {
    let mut current = id;
    loop {
        let Ok(class) = tree.class(current) else { return false };
        if unshared(class) || tree.is_destroyed(current).unwrap_or(true) {
            return false;
        }
        match tree.parent(current) {
            Ok(Some(parent)) if parent == tree.root() => {
                return SHARED.iter().any(|c| tree.service(*c) == Some(current));
            }
            Ok(Some(parent)) => current = parent,
            _ => return false,
        }
    }
}

/// `args` from a joined computer with every instance it couldn't have been
/// sent made `nil`: anything not [`shared_with_guests`], and, when the game
/// streams, anything not streamed to it.
fn guest_args(tree: &InstanceTree, message: &Json, stream: Option<&Replicator>) -> Json {
    let mut args = message.get("a").cloned().unwrap_or(Json::Array(Vec::new()));
    map_instances(&mut args, &mut |id| {
        let streamed = stream
            .is_none_or(|s| s.sent.contains_key(&id) || SHARED.iter().any(|c| tree.service(*c) == Some(id)));
        (streamed && shared_with_guests(tree, id)).then_some(id)
    });
    args
}

// ---------------------------------------------------------------------------
// Connections
// ---------------------------------------------------------------------------

/// Which end of a connection this is: the joining computer starts the
/// key exchange, the host answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Joining,
    Hosting,
}

/// The Noise handshake every connection starts with: fresh keys on both
/// sides, nothing known in advance. It keeps what's sent private and
/// unchanged on the way, but doesn't prove who the host is. Joining by code
/// does (the tunnel checks the host's key); joining by address, both sides
/// see a [safety code](Conn::safety_code) that someone in the middle can't
/// make match, the way Signal's safety numbers work.
const NOISE: &str = "Noise_NN_25519_ChaChaPoly_BLAKE2s";
/// The first bytes a joining computer sends, so a host can tell an
/// encrypted connection from an older copy of Libroxel talking plain JSON.
const MAGIC: &[u8; 4] = b"LRXE";
/// The most a Noise message may carry, and so each frame's plain part.
const NOISE_MAX: usize = 65535;
const FRAME_PLAIN: usize = NOISE_MAX - 16;
/// How long the key exchange may take before giving up.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

enum Noise {
    /// Exchanging keys; whatever is sent meanwhile waits.
    Handshake(Box<snow::HandshakeState>),
    Ready(Box<snow::TransportState>),
    Failed,
}

/// A connection that never blocks, carrying one JSON message per line,
/// encrypted. On the wire: [`MAGIC`] from the joining side, then frames of a
/// two-byte length and a Noise message, the first two of them the key
/// exchange.
pub(crate) struct Conn {
    stream: TcpStream,
    side: Side,
    noise: Noise,
    since: Instant,
    /// Whether the joining side's [`MAGIC`] has arrived (on the host).
    magic_seen: bool,
    /// Bytes from the wire not yet decrypted.
    wire_in: Vec<u8>,
    /// Decrypted bytes not yet split into messages.
    received: Vec<u8>,
    /// Messages waiting to be encrypted (until the keys are agreed).
    plain_out: Vec<u8>,
    /// Encrypted bytes waiting to go out.
    unsent: Vec<u8>,
    pub(crate) closed: bool,
    /// The longest message taken in; a longer one closes the connection.
    max_message: usize,
    /// Bytes read off the wire so far, for the speed reports.
    pub(crate) bytes_read: u64,
    /// A thread of its own for reading long messages (a newcomer's world),
    /// started when the first arrives.
    parser: Option<Parser>,
    /// The key exchange's [`safety code`](Self::safety_code), once done.
    safety: Option<String>,
    /// When something last came in, and when something last went out.
    heard: Instant,
    said: Instant,
    /// Whether the other side sends keep-alives, so its silence means
    /// something.
    keeps_alive: bool,
}

/// Eight digits from the key exchange: the same on both ends of one
/// connection, and different on each side when someone sits in the middle
/// (they can't make two exchanges come out alike).
fn safety_code(handshake_hash: &[u8]) -> String {
    let mut bytes = [0u8; 8];
    bytes[..5].copy_from_slice(&handshake_hash[..5]);
    let n = u64::from_le_bytes(bytes) % 100_000_000;
    format!("{:04} {:04}", n / 10_000, n % 10_000)
}

/// Messages longer than this are read on the [`Parser`]'s thread: reading
/// them was most of a joining computer's frame.
const PARSE_OFF_THREAD: usize = 16 * 1024;
/// How many messages may wait there at once (the rest wait their turn).
const PARSE_AHEAD: usize = 8;

/// Turns long lines into messages on another thread, in the order they
/// came, and lets go of messages already acted on there too (freeing a
/// big world's worth of them takes a while as well).
struct Parser {
    jobs: Sender<Job>,
    done: Receiver<Option<Unpacked>>,
    /// Lines sent off whose messages haven't come back yet.
    pending: usize,
}

enum Job {
    Parse(Vec<u8>),
    Drop(#[allow(dead_code)] Vec<Json>),
}

impl Parser {
    fn start(limit: usize) -> Self {
        let (jobs, inbox) = channel::<Job>();
        let (outbox, done) = channel();
        std::thread::spawn(move || {
            for job in inbox {
                let Job::Parse(line) = job else { continue };
                if outbox.send(parse_line(&line, limit)).is_err() {
                    break;
                }
            }
        });
        Self { jobs, done, pending: 0 }
    }
}

/// One line as a message: `None` for one that isn't one.
fn parse_line(line: &[u8], limit: usize) -> Option<Unpacked> {
    let message = serde_json::from_slice::<Json>(line).ok()?;
    if message.get("t").and_then(Json::as_str) != Some("z") {
        return Some(Unpacked::Message(message, 0));
    }
    // A compressed message: it mustn't unpack to more than one message
    // may be, so a small one can't fill this computer's memory.
    match unpack(&message, limit) {
        Unpacked::Broken => None,
        other => Some(other),
    }
}

impl Conn {
    pub(crate) fn new(stream: TcpStream, side: Side) -> std::io::Result<Self> {
        Self::with_limit(stream, side, MAX_BACKLOG)
    }

    /// A connection that closes when the other side sends a message longer
    /// than `max_message` bytes.
    pub(crate) fn with_limit(stream: TcpStream, side: Side, max_message: usize) -> std::io::Result<Self> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        let params = NOISE.parse().expect("a valid Noise pattern");
        let builder = snow::Builder::new(params);
        let handshake = match side {
            Side::Joining => builder.build_initiator(),
            Side::Hosting => builder.build_responder(),
        }
        .map_err(|e| std::io::Error::other(e.to_string()))?;
        let mut conn = Self {
            stream,
            side,
            noise: Noise::Handshake(Box::new(handshake)),
            since: Instant::now(),
            magic_seen: false,
            wire_in: Vec::new(),
            received: Vec::new(),
            plain_out: Vec::new(),
            unsent: Vec::new(),
            closed: false,
            max_message,
            bytes_read: 0,
            parser: None,
            safety: None,
            heard: Instant::now(),
            said: Instant::now(),
            keeps_alive: false,
        };
        if side == Side::Joining {
            conn.unsent.extend_from_slice(MAGIC);
            conn.handshake_step(None);
        }
        Ok(conn)
    }

    /// Whether the keys are agreed and messages flow.
    pub(crate) fn ready(&self) -> bool {
        matches!(self.noise, Noise::Ready(_))
    }

    /// Eight digits both ends of this connection see alike, once the keys
    /// are agreed. Someone listening in the middle makes them differ.
    pub(crate) fn safety_code(&self) -> Option<&str> {
        self.safety.as_deref()
    }

    /// How long the other side has said nothing, counting only from when
    /// everything it sent has been taken in (a computer busy with a big
    /// world isn't reading, but the host isn't quiet).
    fn silent_for(&self, now: Instant) -> Duration {
        let busy = !self.wire_in.is_empty()
            || !self.received.is_empty()
            || self.parser.as_ref().is_some_and(|p| p.pending > 0);
        if busy { Duration::ZERO } else { now.saturating_duration_since(self.heard) }
    }

    /// Whether the other side keeps alive and has been quiet for longer than `limit`.
    fn gone_quiet(&self, limit: Duration, now: Instant) -> bool {
        self.keeps_alive && self.ready() && self.silent_for(now) > limit
    }

    /// Whether it's time to say something so the other side knows we're here.
    fn due_keep_alive(&self, now: Instant) -> bool {
        self.ready() && !self.closed && now.saturating_duration_since(self.said) >= KEEP_ALIVE
    }

    /// Whether the other end is on this computer (as an internet tunnel is).
    fn is_local(&self) -> bool {
        self.stream.peer_addr().is_ok_and(|a| a.ip().is_loopback())
    }

    /// Queues `message` to go out. A long one goes compressed, as
    /// `{"t":"z","d":<base64 of the deflated message>}`: a newcomer's copy
    /// of the world is several times smaller that way.
    pub(crate) fn send(&mut self, message: &Json) {
        if self.closed {
            return;
        }
        let Some(body) = encode(message) else { return };
        self.send_encoded(&body);
    }

    /// A message [`encode`]d already: the same bytes can go to every
    /// player, so a step's changes are written out and compressed once,
    /// not once per player.
    fn send_encoded(&mut self, body: &[u8]) {
        if self.closed {
            return;
        }
        self.plain_out.extend_from_slice(body);
        self.plain_out.push(b'\n');
        self.said = Instant::now();
        self.seal();
    }

    /// A message already written out as a line (ending in a newline), as
    /// a newcomer's world is: compressed like any other, then encrypted.
    fn send_line(&mut self, line: &[u8]) {
        if self.closed {
            return;
        }
        let body = line.strip_suffix(b"\n").unwrap_or(line);
        match (body.len() > COMPRESS_OVER).then(|| pack(body)).flatten() {
            Some(packed) => self.plain_out.extend_from_slice(&packed),
            None => self.plain_out.extend_from_slice(body),
        }
        self.plain_out.push(b'\n');
        self.said = Instant::now();
        self.seal();
    }

    /// Lets go of messages already acted on, on the parser's thread if
    /// there is one: freeing a world's worth takes a while.
    pub(crate) fn discard(&self, messages: Vec<Json>) {
        if let Some(parser) = &self.parser {
            let _ = parser.jobs.send(Job::Drop(messages));
        }
    }

    /// How much is waiting to go out, so a newcomer's world can be paced.
    fn unsent_len(&self) -> usize {
        self.plain_out.len() + self.unsent.len()
    }

    /// Plain bytes straight into the outgoing stream, for tests that need
    /// to send what a well-behaved computer wouldn't.
    #[cfg(test)]
    pub(crate) fn send_raw(&mut self, bytes: &[u8]) {
        self.plain_out.extend_from_slice(bytes);
        self.seal();
    }

    /// Encrypts what's waiting, once the keys are agreed.
    fn seal(&mut self) {
        if let Noise::Ready(transport) = &mut self.noise {
            let mut frame = vec![0u8; NOISE_MAX];
            for chunk in self.plain_out.chunks(FRAME_PLAIN) {
                match transport.write_message(chunk, &mut frame) {
                    Ok(n) => {
                        self.unsent.extend_from_slice(&(n as u16).to_be_bytes());
                        self.unsent.extend_from_slice(&frame[..n]);
                    }
                    Err(_) => {
                        self.closed = true;
                        break;
                    }
                }
            }
            self.plain_out.clear();
        }
        if self.unsent.len() + self.plain_out.len() > MAX_BACKLOG {
            self.closed = true;
        }
    }

    /// One step of the key exchange: takes in the other side's message, if
    /// there is one, and sends ours when it's our turn.
    fn handshake_step(&mut self, incoming: Option<&[u8]>) {
        let Noise::Handshake(handshake) = &mut self.noise else { return };
        let mut buf = vec![0u8; NOISE_MAX];
        if let Some(message) = incoming
            && handshake.read_message(message, &mut buf).is_err()
        {
            self.noise = Noise::Failed;
            self.closed = true;
            return;
        }
        if !handshake.is_handshake_finished() && handshake.is_my_turn() {
            match handshake.write_message(&[], &mut buf) {
                Ok(n) => {
                    self.unsent.extend_from_slice(&(n as u16).to_be_bytes());
                    self.unsent.extend_from_slice(&buf[..n]);
                }
                Err(_) => {
                    self.noise = Noise::Failed;
                    self.closed = true;
                    return;
                }
            }
        }
        if handshake.is_handshake_finished() {
            self.safety = Some(safety_code(handshake.get_handshake_hash()));
            let Noise::Handshake(handshake) = std::mem::replace(&mut self.noise, Noise::Failed) else {
                unreachable!()
            };
            match handshake.into_transport_mode() {
                Ok(transport) => {
                    self.noise = Noise::Ready(Box::new(transport));
                    self.seal();
                }
                Err(_) => self.closed = true,
            }
        }
    }

    pub(crate) fn flush(&mut self) {
        self.seal();
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

    /// Takes the frames that have arrived off the wire: the key exchange,
    /// then decrypted messages into `received`.
    fn open_frames(&mut self) {
        if self.side == Side::Hosting && !self.magic_seen {
            if self.wire_in.first() == Some(&b'{') {
                // An older copy of Libroxel, talking plain JSON: say why,
                // in a way it understands, and hang up.
                let refusal = json!({ "t": "refused", "why": "a different version of Libroxel" });
                if let Ok(mut line) = serde_json::to_vec(&refusal) {
                    line.push(b'\n');
                    let _ = self.stream.write(&line);
                }
                self.closed = true;
                return;
            }
            if self.wire_in.len() < MAGIC.len() {
                return;
            }
            if &self.wire_in[..MAGIC.len()] != MAGIC {
                self.closed = true;
                return;
            }
            self.wire_in.drain(..MAGIC.len());
            self.magic_seen = true;
        }
        let mut start = 0;
        let mut plain = vec![0u8; NOISE_MAX];
        // What arrived before the other side hung up still counts (a
        // host's last words when it removes a player, say); a frame that
        // fails to open stops it.
        let closed_before = self.closed;
        while self.wire_in.len() >= start + 2 && (closed_before || !self.closed) {
            let length = usize::from(u16::from_be_bytes([self.wire_in[start], self.wire_in[start + 1]]));
            if self.wire_in.len() < start + 2 + length {
                break;
            }
            let frame = self.wire_in[start + 2..start + 2 + length].to_vec();
            start += 2 + length;
            match &mut self.noise {
                Noise::Handshake(_) => self.handshake_step(Some(&frame)),
                Noise::Ready(transport) => match transport.read_message(&frame, &mut plain) {
                    Ok(n) => self.received.extend_from_slice(&plain[..n]),
                    // Changed or out of order on the way: nothing after it can be trusted.
                    Err(_) => {
                        self.closed = true;
                        break;
                    }
                },
                Noise::Failed => {
                    self.closed = true;
                    break;
                }
            }
        }
        self.wire_in.drain(..start);
    }

    /// Messages the parser's thread has finished, in order.
    fn take_parsed(&mut self, messages: &mut Vec<Json>) {
        let Some(parser) = &mut self.parser else { return };
        // Once the other side has hung up, what it sent before still
        // counts (a host's last words when it removes a player): wait for it.
        let hung_up = self.closed;
        while parser.pending > 0 {
            let next = if hung_up {
                parser.done.recv().map_err(|_| TryRecvError::Disconnected)
            } else {
                parser.done.try_recv()
            };
            match next {
                Ok(Some(Unpacked::Message(message, _))) => {
                    parser.pending -= 1;
                    messages.push(message);
                }
                Ok(Some(Unpacked::TooBig)) => {
                    parser.pending -= 1;
                    self.closed = true;
                }
                Ok(_) => parser.pending -= 1,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    parser.pending = 0;
                    self.closed = true;
                }
            }
        }
    }

    /// Every whole message that has arrived.
    ///
    /// Takes in at most about one longest message's worth of bytes a call,
    /// so a computer sending without end can't hold up the frame, and closes
    /// the connection when a message runs longer than the limit, so it can't
    /// fill this computer's memory either.
    pub(crate) fn receive(&mut self) -> Vec<Json> {
        let mut buf = vec![0u8; 256 * 1024];
        let mut taken = 0;
        // Messages still waiting from last time come first: no more is read
        // until they're down to one step's worth.
        while !self.closed && taken < READ_BUDGET && self.received.len() < READ_BUDGET {
            match self.stream.read(&mut buf) {
                Ok(0) => self.closed = true,
                Ok(n) => {
                    taken += n;
                    self.bytes_read += n as u64;
                    self.heard = Instant::now();
                    self.wire_in.extend_from_slice(&buf[..n]);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => self.closed = true,
            }
        }
        self.open_frames();
        if !self.ready() && self.since.elapsed() > HANDSHAKE_TIMEOUT {
            // Not Libroxel, or an older or newer one that can't agree keys.
            self.closed = true;
        }
        // Our side of the key exchange (and anything it let out) goes now.
        if !self.unsent.is_empty() {
            self.flush();
        }
        let mut messages = Vec::new();
        let mut start = 0;
        let limit = self.max_message;
        // What the parser's thread has read, first: those lines came first.
        self.take_parsed(&mut messages);
        // A step takes in about READ_BUDGET bytes of messages, counted
        // unpacked, so a compressed world can't make one step take long.
        let mut parsed = 0;
        while parsed < READ_BUDGET
            && self.parser.as_ref().is_none_or(|p| p.pending < PARSE_AHEAD)
            && let Some(offset) = self.received[start..].iter().position(|b| *b == b'\n')
        {
            let line = &self.received[start..start + offset];
            start += offset + 1;
            parsed += line.len();
            if line.len() > limit {
                self.closed = true;
                break;
            }
            // Long lines, and every line while some wait there (so they
            // stay in order), go to the parser's thread.
            if line.len() > PARSE_OFF_THREAD || self.parser.as_ref().is_some_and(|p| p.pending > 0) {
                let parser = self.parser.get_or_insert_with(|| Parser::start(limit));
                if parser.jobs.send(Job::Parse(line.to_vec())).is_ok() {
                    parser.pending += 1;
                    continue;
                }
                // Its thread is gone: read it here.
                self.parser = None;
            }
            match parse_line(line, limit) {
                Some(Unpacked::Message(message, size)) => {
                    parsed += size;
                    messages.push(message);
                }
                Some(Unpacked::TooBig) => {
                    self.closed = true;
                    break;
                }
                _ => {}
            }
        }
        self.received.drain(..start);
        if self.closed {
            self.take_parsed(&mut messages);
        }
        if self.received.len() > self.max_message {
            self.closed = true;
        }
        if self.closed {
            self.received = Vec::new();
            self.wire_in = Vec::new();
        }
        messages
    }
}

/// `line` deflated and wrapped in a `z` message, if that's shorter.
/// The whole world written out once, on a thread of its own, for `count`
/// players who join in the same step: one copy of the tree and one pass
/// over it, however many join at once.
fn worlds_for(
    tree: InstanceTree,
    count: usize,
    primed: Option<Sender<Replicator>>,
) -> Vec<Receiver<Vec<Vec<u8>>>> {
    let (senders, receivers): (Vec<_>, Vec<_>) = (0..count).map(|_| channel()).unzip();
    std::thread::spawn(move || {
        let mut replicator = Replicator::default();
        let lines = world_lines(&replicator.delta(&tree));
        drop(tree);
        for tx in senders {
            let _ = tx.send(lines.clone());
        }
        if let Some(primed) = primed {
            let _ = primed.send(replicator);
        }
    });
    receivers
}

/// `message` as it goes on the wire, before encryption: JSON, compressed
/// when long (without the newline that ends it).
fn encode(message: &Json) -> Option<std::sync::Arc<[u8]>> {
    let line = serde_json::to_vec(message).ok()?;
    Some(match (line.len() > COMPRESS_OVER).then(|| pack(&line)).flatten() {
        Some(packed) => packed.into(),
        None => line.into(),
    })
}

fn pack(line: &[u8]) -> Option<Vec<u8>> {
    use flate2::{Compression, write::DeflateEncoder};
    let mut encoder = DeflateEncoder::new(Vec::with_capacity(line.len() / 4), Compression::fast());
    encoder.write_all(line).ok()?;
    let deflated = encoder.finish().ok()?;
    let packed =
        serde_json::to_vec(&json!({ "t": "z", "d": data_encoding::BASE64.encode(&deflated) })).ok()?;
    (packed.len() < line.len()).then_some(packed)
}

enum Unpacked {
    /// The message, and how long it was unpacked.
    Message(Json, usize),
    TooBig,
    Broken,
}

/// The message inside a `z` message, unless it would unpack to more than `limit` bytes.
fn unpack(message: &Json, limit: usize) -> Unpacked {
    use flate2::read::DeflateDecoder;
    let Some(deflated) =
        message.get("d").and_then(Json::as_str).and_then(|d| data_encoding::BASE64.decode(d.as_bytes()).ok())
    else {
        return Unpacked::Broken;
    };
    let mut line = Vec::new();
    if DeflateDecoder::new(deflated.as_slice()).take(limit as u64 + 1).read_to_end(&mut line).is_err() {
        return Unpacked::Broken;
    }
    if line.len() > limit {
        return Unpacked::TooBig;
    }
    match serde_json::from_slice::<Json>(&line) {
        // No compressed messages inside compressed messages.
        Ok(inner) if inner.get("t").and_then(Json::as_str) != Some("z") => {
            Unpacked::Message(inner, line.len())
        }
        _ => Unpacked::Broken,
    }
}

/// Lets through so many things a second, with room saved up for a burst.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Allowance {
    per_second: f32,
    most: f32,
    left: f32,
    at: Instant,
}

impl Allowance {
    pub(crate) fn new(per_second: f32, most: f32, now: Instant) -> Self {
        Self { per_second, most, left: most, at: now }
    }

    /// Spends `amount` if that much is left at `now`.
    pub(crate) fn spend(&mut self, amount: f32, now: Instant) -> bool {
        let seconds = now.saturating_duration_since(self.at).as_secs_f32();
        self.at = now;
        self.left = (self.left + seconds * self.per_second).min(self.most);
        if amount <= self.left {
            self.left -= amount;
            true
        } else {
            false
        }
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
    /// `Workspace.StreamingEnabled`, or a big map, when hosting began (it
    /// can't change while the game runs): each player is sent only what's
    /// near them.
    streaming: bool,
    /// Whether it streams because the map is big.
    big: bool,
    replicator: Option<Replicator>,
    priming: Option<Receiver<Replicator>>,
    /// Teleports found but not sent yet: player, number, where, facing.
    teleports: Vec<(InstanceId, u64, Vec3, f32)>,
    /// Players removed from this game, who can't join again.
    banned: Vec<Ban>,
    /// Who a connection really is, for connections through an internet
    /// tunnel (see [`PlaySession::set_guest_lookup`]).
    who: Option<GuestLookup>,
    /// Whether computers on the network may join by address while there is
    /// a tunnel (see [`PlaySession::allow_network_joins`]).
    network_joins: bool,
    /// How long a joined computer may say nothing before it is let go.
    silence: Duration,
}

impl Drop for Hosting {
    /// The game is over: everyone still here is told so, rather than just
    /// finding the connection gone (and trying to come back).
    fn drop(&mut self) {
        for peer in &mut self.peers {
            if peer.player.is_some() && !peer.conn.closed {
                peer.conn.send(&json!({ "t": "ended" }));
                peer.conn.flush();
            }
        }
    }
}

/// Says which player's computer a connection to the game comes from (by
/// the address it connects from), when something can vouch for it.
pub type GuestLookup = Box<dyn Fn(SocketAddr) -> Option<String> + Send + Sync>;

/// Someone removed from a hosted game.
#[derive(Debug, Clone, PartialEq)]
enum Ban {
    /// A player whose id was proved: renaming doesn't get them back in.
    Account(String),
    /// A player who joined without a proved id, by the name they gave and
    /// the address they came from.
    Unproved { name: String, ip: Option<std::net::IpAddr> },
}

struct Peer {
    conn: Conn,
    /// Their player, once they have said hello.
    player: Option<InstanceId>,
    /// Whether they still need the whole world.
    fresh: bool,
    /// The name they asked to play as.
    asked: String,
    /// Where they connected from, and their player id when it was proved.
    from: Option<SocketAddr>,
    account: Option<String>,
    queue: VecDeque<Out>,
    world: Option<Receiver<Vec<Vec<u8>>>>,
    /// When they connected.
    since: Instant,
    /// How many more messages, and script-running actions, they may send.
    messages: Allowance,
    actions: Allowance,
    /// Whether the host has been told they are sending too much.
    warned: bool,
    /// What their computer has been sent, when the game streams.
    stream: Replicator,
    /// Areas `RequestStreamAroundAsync` asked to send them, until when.
    stream_requests: Vec<(Vec3, Instant)>,
    /// `PersistentPerPlayer` models `AddPersistentPlayer` added them to.
    persistent: HashSet<InstanceId>,
}

enum Out {
    Message(Json),
    Line(Vec<u8>),
    /// A message [`encode`]d once for everyone.
    Encoded(std::sync::Arc<[u8]>),
    World,
}

impl Peer {
    fn new(conn: Conn, from: Option<SocketAddr>, now: Instant) -> Self {
        Self {
            conn,
            player: None,
            fresh: true,
            asked: String::new(),
            from,
            account: None,
            queue: VecDeque::new(),
            world: None,
            since: now,
            messages: Allowance::new(GUEST_MESSAGES_PER_SECOND, GUEST_MESSAGE_BURST, now),
            actions: Allowance::new(GUEST_ACTIONS_PER_SECOND, GUEST_ACTION_BURST, now),
            warned: false,
            stream: Replicator::default(),
            stream_requests: Vec::new(),
            persistent: HashSet::new(),
        }
    }

    fn send(&mut self, message: Json) {
        if self.queue.is_empty() && self.conn.unsent_len() < SEND_AHEAD {
            self.conn.send(&message);
        } else {
            self.queue.push_back(Out::Message(message));
            if self.queue.len() > MAX_QUEUED {
                self.conn.closed = true;
            }
        }
    }

    fn send_encoded(&mut self, body: std::sync::Arc<[u8]>) {
        if self.queue.is_empty() && self.conn.unsent_len() < SEND_AHEAD {
            self.conn.send_encoded(&body);
        } else {
            self.queue.push_back(Out::Encoded(body));
            if self.queue.len() > MAX_QUEUED {
                self.conn.closed = true;
            }
        }
    }

    /// Queues the whole world, as [`worlds_for`] writes it out.
    fn send_world(&mut self, world: Receiver<Vec<Vec<u8>>>) {
        self.world = Some(world);
        self.queue.push_back(Out::World);
    }

    fn pump(&mut self) {
        for _ in 0..2 {
            while self.conn.unsent_len() < SEND_AHEAD {
                match self.queue.front() {
                    None => break,
                    Some(Out::World) => {
                        let Some(world) = &self.world else {
                            self.queue.pop_front();
                            continue;
                        };
                        match world.try_recv() {
                            Ok(lines) => {
                                self.world = None;
                                self.queue.pop_front();
                                for line in lines.into_iter().rev() {
                                    self.queue.push_front(Out::Line(line));
                                }
                            }
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => {
                                self.world = None;
                                self.conn.closed = true;
                                break;
                            }
                        }
                    }
                    Some(_) => match self.queue.pop_front() {
                        Some(Out::Message(message)) => self.conn.send(&message),
                        Some(Out::Line(line)) => self.conn.send_line(&line),
                        Some(Out::Encoded(body)) => self.conn.send_encoded(&body),
                        _ => {}
                    },
                }
            }
            self.conn.flush();
        }
    }
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
    /// Whether we move our own character now (it's alive and not sitting).
    /// The host's word on where its parts are is ignored meanwhile.
    owning: bool,
    /// Our character's model, here.
    own_model: Option<InstanceId>,
    /// The latest of the host's teleports of our character we have taken.
    teleports: u64,
    /// Where the host last put each part that moves, for it to glide
    /// there: the host's updates come unevenly, and set straight away they
    /// stutter.
    glide: HashMap<InstanceId, (Vec3, Vec3)>,
    inbox: VecDeque<Json>,
    waiting: Vec<(InstanceId, Property, u64)>,
    total: usize,
    got: usize,
    wrong_version: bool,
    /// How the join is going, for the Output.
    clock: JoinClock,
    /// The workspace's loose parts, which our character's physics holds
    /// where the host put them.
    loose: LooseParts,
    /// Where the game is (`host:port`) and who we joined as, to come back.
    address: String,
    name: String,
    /// How we last asked to look, to ask again when we come back.
    look: Option<Json>,
    /// `InvokeServer` calls sent and not answered yet.
    calls: HashSet<u64>,
    /// Whether the host ended the game, or removed or turned us away:
    /// then there is nothing to come back to.
    ended: bool,
    turned_away: bool,
    /// How long the host may say nothing before the connection counts as lost.
    silence: Duration,
}

/// The unanchored parts in the workspace, kept up from the tree's log of
/// what moved, came or went: looking for them among every part each frame
/// cost a big map's joined computers a pass over all its parts a frame.
#[derive(Default)]
struct LooseParts {
    ready: bool,
    epoch: u64,
    read: usize,
    parts: std::collections::HashSet<InstanceId>,
}

impl LooseParts {
    fn update(&mut self, t: &InstanceTree) -> &std::collections::HashSet<InstanceId> {
        let ws = t.workspace();
        let (epoch, entries) = t.moved_since(self.read);
        if !self.ready || epoch != self.epoch {
            self.parts = t
                .descendants_of_class(ws, ClassKind::Part)
                .into_iter()
                .filter(|p| t.get_bool(*p, Property::Anchored) == Some(false))
                .collect();
        } else {
            for id in entries {
                let mut ids = t.descendants(*id).unwrap_or_default();
                ids.push(*id);
                for id in ids {
                    let loose = t.class(id).is_ok_and(|c| c.is_a(ClassKind::Part))
                        && !t.is_destroyed(id).unwrap_or(true)
                        && t.get_bool(id, Property::Anchored) == Some(false)
                        && t.is_descendant_of(id, ws).unwrap_or(false);
                    if loose {
                        self.parts.insert(id);
                    } else {
                        self.parts.remove(&id);
                    }
                }
            }
        }
        self.ready = true;
        self.epoch = t.moved_epoch();
        self.read = t.moved_len();
        &self.parts
    }
}

/// When a join got where, and what took its time, put in the Output as
/// it goes: a report from a slow join then says whether the network, the
/// world arriving or this computer held it up.
struct JoinClock {
    began: Instant,
    frames: u64,
    ready: bool,
    welcomed: bool,
    first: bool,
    done: bool,
    receive: f64,
    apply: f64,
    slowest: f64,
    bytes: u64,
}

impl JoinClock {
    fn new(began: Instant) -> Self {
        Self {
            began,
            frames: 0,
            ready: false,
            welcomed: false,
            first: false,
            done: false,
            receive: 0.0,
            apply: 0.0,
            slowest: 0.0,
            bytes: 0,
        }
    }

    /// `what` happened, with the time since joining began and the frame.
    fn mark(&self, what: &str) -> String {
        format!("⏱ Join: {what} after {:.2} s (frame {})", self.began.elapsed().as_secs_f64(), self.frames)
    }
}

/// How quickly a part glides to where the host put it, a second.
const GLIDE_RATE: f32 = 18.0;
/// A part put further than this away jumps there.
const GLIDE_JUMP: f32 = 40.0;

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
    Prompt {
        prompt: InstanceId,
        ended: bool,
    },
    Reset,
}

// ---------------------------------------------------------------------------
// Replication
// ---------------------------------------------------------------------------

/// What was last sent about one instance.
#[derive(Debug, Clone)]
pub(crate) struct Sent {
    revision: u64,
    parent: Option<InstanceId>,
    name: String,
    props: Vec<(Property, PropertyValue)>,
    attributes: BTreeMap<String, PropertyValue>,
    extras: BTreeMap<String, PropertyValue>,
    tags: Vec<String>,
    /// The terrain cells' version (0 for anything without cells).
    voxels: u64,
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
        extras: tree.extras(id).ok()?.clone(),
        tags: tree.tags(id).ok()?.to_vec(),
        voxels: tree.voxels(id).ok()?.version(),
    })
}

fn world_lines(ops: &[Json]) -> Vec<Vec<u8>> {
    ops.chunks(CHUNK)
        .filter_map(|ops| {
            let mut line = serde_json::to_vec(&json!({ "t": "rep", "ops": ops })).ok()?;
            line.push(b'\n');
            Some(line)
        })
        .collect()
}

pub(crate) fn bits(id: InstanceId) -> Json {
    json!(id.to_bits())
}

fn wire_value(class: ClassKind, property: Property, value: &PropertyValue, edits: bool) -> Json {
    match value {
        PropertyValue::Ref(Some(target)) => json!({ "$i": target.to_bits() }),
        PropertyValue::Ref(None) => Json::Null,
        // Server scripts' code stays on the host, except for people editing it.
        PropertyValue::String(_) if class == ClassKind::Script && property == Property::Source && !edits => {
            json!("")
        }
        other => place::value_to_json(other),
    }
}

/// Keeps track of what every joined computer has been sent.
#[derive(Default)]
pub(crate) struct Replicator {
    sent: HashMap<InstanceId, Sent>,
    /// Whether this shares a game being edited: every service, and the
    /// code of server scripts too.
    edits: bool,
    /// Steps sent, for parts further away to be updated less often.
    tick: u64,
    /// Terrain chunks sent, when streamed, and their fingerprints.
    chunks: HashMap<(i32, i32, i32), u64>,
    /// The terrain's version when its chunks were last looked at, and
    /// whether some near ones are still to come.
    chunks_version: u64,
    chunks_waiting: bool,
    /// What the last full look found to send, and whether some was left
    /// for later; between full looks only what changed is looked at.
    wanted: Option<HashMap<InstanceId, u64>>,
    waiting: bool,
    /// Where the tree's touched log was read up to, and its structure then.
    touched: (u64, usize),
    structure: u64,
    /// Changes held back until their turn.
    later: HashSet<InstanceId>,
}

/// How many steps apart a streamed player's whole share is looked at again.
const FULL_LOOK: u64 = 6;

impl Replicator {
    /// A replicator for a game being edited together.
    pub(crate) fn edits() -> Self {
        Self { edits: true, ..Self::default() }
    }

    /// What changed since the last call, as operations; updates the record.
    pub(crate) fn delta(&mut self, tree: &InstanceTree) -> Vec<Json> {
        self.delta_within(tree, None)
    }

    /// [`delta`](Self::delta) for a player whose computer is streamed to:
    /// only the parts of the workspace `focus` says are near enough.
    /// Parts that stop being near are sent as removed (streamed out), and
    /// come back as new ones when near again.
    pub(crate) fn delta_streamed(&mut self, tree: &InstanceTree, focus: &Focus) -> Vec<Json> {
        self.tick += 1;
        let (epoch, touched) = tree.touched_since(self.touched.1);
        let full = self.wanted.is_none()
            || self.waiting
            || epoch != self.touched.0
            || tree.game_structure() != self.structure
            || self.tick.is_multiple_of(FULL_LOOK);
        let mut ops = if full {
            // A full look finds again what's still held back; without this,
            // a game whose structure changes every step (anything made or
            // destroyed) only ever added to it.
            self.later.clear();
            let (wanted, waiting) = focus.wanted(tree, &self.sent);
            self.waiting = waiting;
            let ops = self.delta_within(tree, Some(&wanted));
            self.wanted = Some(wanted);
            ops
        } else {
            let mut ids: Vec<InstanceId> = touched.to_vec();
            ids.extend(self.later.drain());
            let mut done = HashSet::new();
            let mut ops = Vec::new();
            for id in ids {
                if done.insert(id) {
                    self.changed(tree, id, &mut ops);
                }
            }
            ops
        };
        self.touched = (epoch, tree.touched_len());
        self.structure = tree.game_structure();
        if let Some(terrain) = tree.terrain().filter(|t| self.sent.contains_key(t))
            && let Some(op) = self.terrain_chunks(tree, terrain, focus)
        {
            ops.push(op);
        }
        ops
    }

    /// The terrain chunks near enough that are new or changed, nearest
    /// first, and those gone or too far now, as air.
    fn terrain_chunks(&mut self, tree: &InstanceTree, terrain: InstanceId, focus: &Focus) -> Option<Json> {
        let voxels = tree.voxels(terrain).ok()?;
        if voxels.version() == self.chunks_version
            && !self.chunks_waiting
            && !self.tick.is_multiple_of(TERRAIN_LOOK)
        {
            return None;
        }
        self.chunks_version = voxels.version();
        let size = libroxel_instance::voxels::CHUNK as f32;
        let half = size * 0.75f32.sqrt();
        let distance = |(x, y, z): (i32, i32, i32)| {
            let centre = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5) * size;
            focus.points.iter().map(|p| (centre - *p).length() - half).fold(f32::INFINITY, f32::min)
        };
        let mut send: Vec<(f32, (i32, i32, i32), u64)> = Vec::new();
        let mut gone = Vec::new();
        for chunk in voxels.chunks() {
            let print = voxels.chunk_fingerprint(chunk);
            let d = distance(chunk);
            match self.chunks.get(&chunk) {
                Some(_) if d > focus.keep => gone.push(chunk),
                Some(old) if *old == print => {}
                Some(_) => send.push((d, chunk, print)),
                None if d <= focus.radius => send.push((d, chunk, print)),
                None => {}
            }
        }
        gone.extend(self.chunks.keys().copied().filter(|c| voxels.chunk_fingerprint(*c) == 0));
        send.sort_by(|a, b| a.0.total_cmp(&b.0));
        self.chunks_waiting = send.len() > TERRAIN_STEP;
        send.truncate(TERRAIN_STEP);
        for chunk in &gone {
            self.chunks.remove(chunk);
        }
        for (_, chunk, print) in &send {
            self.chunks.insert(*chunk, *print);
        }
        if gone.is_empty() && send.is_empty() {
            return None;
        }
        let air = libroxel_instance::voxels::Voxels::new().encode_chunks(gone);
        let near = voxels.encode_chunks(send.into_iter().map(|(_, c, _)| c));
        let text = [air, near].into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join(";");
        Some(json!({ "u": bits(terrain), "xs": text }))
    }

    /// The update for one instance already sent, between full looks.
    fn changed(&mut self, tree: &InstanceTree, id: InstanceId, ops: &mut Vec<Json>) {
        let Some(old) = self.sent.get(&id) else { return };
        let Ok(class) = tree.class(id) else { return };
        if tree.is_destroyed(id).unwrap_or(true) || tree.parent(id).ok().flatten() != old.parent {
            return;
        }
        let every = self.wanted.as_ref().and_then(|w| w.get(&id)).copied().unwrap_or(1);
        if every > 1 && !self.tick.wrapping_add(id.to_bits()).is_multiple_of(every) {
            self.later.insert(id);
            return;
        }
        if tree.revision(id).unwrap_or(0) == old.revision {
            return;
        }
        let Some(now) = snapshot(tree, id) else { return };
        let service = SHARED.iter().any(|c| tree.service(*c) == Some(id));
        if let Some(op) = Self::op(class, id, Some(old), &now, service, None, self.edits) {
            ops.push(op);
        }
        self.sent.insert(id, now);
    }

    fn delta_within(
        &mut self,
        tree: &InstanceTree,
        workspace_filter: Option<&HashMap<InstanceId, u64>>,
    ) -> Vec<Json> {
        let mut ops = Vec::new();
        let mut seen = HashSet::new();
        let services: &[ClassKind] = if self.edits { ClassKind::SERVICES } else { &SHARED };
        for &class in services {
            if let Some(service) = tree.service(class) {
                let filter = if class == ClassKind::Workspace { workspace_filter } else { None };
                self.visit(tree, service, true, filter, &mut ops, &mut seen);
            }
        }
        let gone: Vec<InstanceId> = self.sent.keys().copied().filter(|id| !seen.contains(id)).collect();
        for id in gone {
            self.sent.remove(&id);
            ops.push(json!({ "d": bits(id) }));
        }
        ops
    }

    fn visit(
        &mut self,
        tree: &InstanceTree,
        id: InstanceId,
        service: bool,
        filter: Option<&HashMap<InstanceId, u64>>,
        ops: &mut Vec<Json>,
        seen: &mut HashSet<InstanceId>,
    ) {
        let Ok(class) = tree.class(id) else { return };
        if unshared(class) || tree.is_destroyed(id).unwrap_or(true) {
            return;
        }
        let every = match filter {
            Some(wanted) if !service => match wanted.get(&id) {
                Some(every) => *every,
                None => return,
            },
            _ => 1,
        };
        seen.insert(id);
        let revision = tree.revision(id).unwrap_or(0);
        let parent = tree.parent(id).ok().flatten();
        let old = self.sent.get(&id);
        let due = old.is_none() || every <= 1 || self.tick.wrapping_add(id.to_bits()).is_multiple_of(every);
        let stale = old.is_none_or(|old| old.revision != revision || old.parent != parent);
        if stale && !due {
            self.later.insert(id);
        } else if stale && let Some(now) = snapshot(tree, id) {
            let voxels = (class == ClassKind::Terrain && filter.is_none())
                .then(|| tree.voxels(id).map(|v| v.encode()).unwrap_or_default());
            if let Some(op) = Self::op(class, id, old, &now, service, voxels, self.edits) {
                ops.push(op);
            }
            self.sent.insert(id, now);
        }
        for child in tree.children(id).unwrap_or_default() {
            self.visit(tree, *child, false, filter, ops, seen);
        }
    }

    /// The operation that brings `old` (nothing, for a new instance) to `now`.
    fn op(
        class: ClassKind,
        id: InstanceId,
        old: Option<&Sent>,
        now: &Sent,
        service: bool,
        voxels: Option<String>,
        edits: bool,
    ) -> Option<Json> {
        let mut op = Map::new();
        let mut props = Map::new();
        for (property, value) in &now.props {
            let before = old.and_then(|o| o.props.iter().find(|(p, _)| p == property)).map(|(_, v)| v);
            if before != Some(value) {
                props.insert(property.file_name().to_owned(), wire_value(class, *property, value, edits));
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
        let mut extras = Map::new();
        for (name, value) in &now.extras {
            if old.and_then(|o| o.extras.get(name)) != Some(value) {
                extras.insert(name.clone(), place::attribute_to_json(value));
            }
        }
        if let Some(old) = old {
            for name in old.extras.keys().filter(|n| !now.extras.contains_key(*n)) {
                extras.insert(name.clone(), Json::Null);
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
        if !extras.is_empty() {
            op.insert("e".into(), Json::Object(extras));
        }
        if old.map_or(!now.tags.is_empty(), |o| o.tags != now.tags) {
            op.insert("g".into(), json!(now.tags));
        }
        if let Some(text) = voxels.filter(|_| old.map_or(now.voxels != 0, |o| o.voxels != now.voxels)) {
            op.insert("x".into(), json!(text));
        }
        // An update that changes nothing isn't worth sending.
        (op.len() > 1 || op.contains_key("c")).then_some(Json::Object(op))
    }
}

/// Where a streamed player's computer wants parts, and how far out, as
/// `Workspace.StreamingEnabled` games do it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Focus {
    /// The places parts are sent around: their `ReplicationFocus` or
    /// character (or the spawn, before it), and any
    /// `RequestStreamAroundAsync` areas.
    pub(crate) points: Vec<Vec3>,
    /// `StreamingTargetRadius`: parts this near are sent.
    pub(crate) radius: f32,
    /// How far parts already sent may go before they're let go:
    /// `StreamOutBehavior.Opportunistic` lets go of them a little past the
    /// radius; otherwise they stay (they'd only go when memory ran low).
    pub(crate) keep: f32,
    /// Characters, always sent.
    pub(crate) always: HashSet<InstanceId>,
    /// `PersistentPerPlayer` models this player was added to, always sent too.
    pub(crate) persistent: HashSet<InstanceId>,
    /// How many instances not sent yet may go this step, nearest first.
    pub(crate) budget: usize,
}

impl Focus {
    /// The workspace's descendants to send, each with how many steps apart
    /// its changes go: every step near, less often further out. Models,
    /// folders and anything not inside a part always go (as
    /// `ModelStreamingMode.Default` and `Nonatomic`); a part goes, with
    /// everything in it, when it's near; an `Atomic` model goes whole when
    /// any of its parts is near; a `Persistent` model, terrain and
    /// characters always go whole.
    /// Also says whether some was left for later.
    pub(crate) fn wanted(
        &self,
        tree: &InstanceTree,
        sent: &HashMap<InstanceId, Sent>,
    ) -> (HashMap<InstanceId, u64>, bool) {
        let mut wanted = HashMap::new();
        let mut waiting = Vec::new();
        self.walk(tree, tree.workspace(), sent, &mut wanted, &mut waiting);
        waiting.sort_by(|a: &(f32, InstanceId), b| a.0.total_cmp(&b.0));
        let mut left = self.budget;
        for (distance, id) in waiting {
            if left == 0 {
                return (wanted, true);
            }
            let before = wanted.len();
            Self::all(tree, id, self.every(distance), &mut wanted);
            left = left.saturating_sub(wanted.len() - before);
        }
        (wanted, false)
    }

    /// How many steps apart changes go to something this far away.
    fn every(&self, distance: f32) -> u64 {
        if distance <= self.radius * 0.25 {
            1
        } else if distance <= self.radius * 0.6 {
            3
        } else if distance <= self.radius {
            8
        } else {
            20
        }
    }

    fn distance(&self, tree: &InstanceTree, part: InstanceId) -> f32 {
        let (Some(at), size) = (
            tree.get_vec3(part, Property::Position),
            tree.get_vec3(part, Property::Size).unwrap_or_default(),
        ) else {
            return 0.0;
        };
        let half = size.length() * 0.5;
        self.points.iter().map(|p| (at - *p).length() - half).fold(f32::INFINITY, f32::min)
    }

    fn reach(&self, id: InstanceId, sent: &HashMap<InstanceId, Sent>) -> f32 {
        if sent.contains_key(&id) { self.keep } else { self.radius }
    }

    fn all(tree: &InstanceTree, id: InstanceId, every: u64, wanted: &mut HashMap<InstanceId, u64>) {
        wanted.insert(id, every);
        for child in tree.children(id).unwrap_or_default() {
            Self::all(tree, *child, every, wanted);
        }
    }

    /// How near the nearest part in a model is, and how near one that may
    /// be sent is.
    fn nearest_part(
        &self,
        tree: &InstanceTree,
        id: InstanceId,
        sent: &HashMap<InstanceId, Sent>,
    ) -> (f32, f32) {
        let mut nearest = (f32::INFINITY, f32::INFINITY);
        for &child in tree.children(id).unwrap_or_default() {
            let (d, ok) = if tree.class(child).is_ok_and(ClassKind::is_physical) {
                let d = self.distance(tree, child);
                (d, if d <= self.reach(child, sent) { d } else { f32::INFINITY })
            } else {
                (f32::INFINITY, f32::INFINITY)
            };
            let inner = self.nearest_part(tree, child, sent);
            nearest = (nearest.0.min(d).min(inner.0), nearest.1.min(ok).min(inner.1));
        }
        nearest
    }

    /// Sends `id` with everything in it: now, when it's been sent before,
    /// or else in turn, nearest first.
    fn whole(
        &self,
        tree: &InstanceTree,
        id: InstanceId,
        distance: f32,
        sent: &HashMap<InstanceId, Sent>,
        wanted: &mut HashMap<InstanceId, u64>,
        waiting: &mut Vec<(f32, InstanceId)>,
    ) {
        if sent.contains_key(&id) {
            Self::all(tree, id, self.every(distance), wanted);
        } else {
            waiting.push((distance, id));
        }
    }

    fn walk(
        &self,
        tree: &InstanceTree,
        id: InstanceId,
        sent: &HashMap<InstanceId, Sent>,
        wanted: &mut HashMap<InstanceId, u64>,
        waiting: &mut Vec<(f32, InstanceId)>,
    ) {
        for &child in tree.children(id).unwrap_or_default() {
            let Ok(class) = tree.class(child) else { continue };
            if class.is_physical() {
                let d = self.distance(tree, child);
                if d <= self.reach(child, sent) {
                    self.whole(tree, child, d, sent, wanted, waiting);
                }
            } else if class == ClassKind::Terrain {
                Self::all(tree, child, 1, wanted);
            } else if self.always.contains(&child) {
                Self::all(tree, child, self.every(self.nearest_part(tree, child, sent).0), wanted);
            } else if class == ClassKind::Model {
                let mode = match tree.get_enum(child, Property::ModelStreamingMode) {
                    Some(libroxel_instance::EnumValue::ModelStreamingMode(m)) => m,
                    _ => ModelStreamingMode::Default,
                };
                match mode {
                    ModelStreamingMode::Persistent => {
                        Self::all(tree, child, self.every(self.nearest_part(tree, child, sent).0), wanted)
                    }
                    ModelStreamingMode::PersistentPerPlayer if self.persistent.contains(&child) => {
                        Self::all(tree, child, self.every(self.nearest_part(tree, child, sent).0), wanted)
                    }
                    ModelStreamingMode::Atomic => {
                        let (_, near) = self.nearest_part(tree, child, sent);
                        if near.is_finite() {
                            self.whole(tree, child, near, sent, wanted, waiting);
                        }
                    }
                    // PersistentPerPlayer, for players not added to it, goes like Default.
                    _ => {
                        wanted.insert(child, 1);
                        self.walk(tree, child, sent, wanted, waiting);
                    }
                }
            } else {
                wanted.insert(child, 1);
                self.walk(tree, child, sent, wanted, waiting);
            }
        }
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
            if let Some(host_id) = op.get("c").and_then(Json::as_u64)
                && !self.ids.contains_key(&host_id)
            {
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
            } else if let Some(id) = op.get("u").or_else(|| op.get("c")).and_then(|u| self.local_of(u)) {
                if let Some(name) = op.get("n").and_then(Json::as_str) {
                    let _ = tree.set_name(id, name);
                }
                // While we move our character, its parts are where we say.
                let ours = self.owning
                    && self
                        .own_model
                        .is_some_and(|m| id == m || tree.is_descendant_of(id, m).unwrap_or(false));
                if ours {
                    let mut op = op.clone();
                    if let Some(props) = op.get_mut("v").and_then(Json::as_object_mut) {
                        props.remove(Property::Position.file_name());
                        props.remove(Property::Orientation.file_name());
                    }
                    Self::fields(tree, id, &op, &mut links);
                } else {
                    let moves = op.get("v").and_then(Json::as_object).is_some_and(|v| {
                        v.contains_key(Property::Position.file_name())
                            || v.contains_key(Property::Orientation.file_name())
                    });
                    let was = moves.then(|| {
                        (tree.get_vec3(id, Property::Position), tree.get_vec3(id, Property::Orientation))
                    });
                    Self::fields(tree, id, op, &mut links);
                    if let Some((Some(from), Some(turn))) = was {
                        let to = tree.get_vec3(id, Property::Position).unwrap_or(from);
                        let to_turn = tree.get_vec3(id, Property::Orientation).unwrap_or(turn);
                        if (to - from).length() < GLIDE_JUMP {
                            let _ = tree.set(id, Property::Position, PropertyValue::Vec3(from));
                            let _ = tree.set(id, Property::Orientation, PropertyValue::Vec3(turn));
                            self.glide.insert(id, (to, to_turn));
                        } else {
                            self.glide.remove(&id);
                        }
                    }
                }
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
            match target.map(|t| (t, self.local(t))) {
                Some((t, None)) if self.me.is_none() => self.waiting.push((id, property, t)),
                Some((_, local)) => {
                    let _ = tree.set(id, property, PropertyValue::Ref(local));
                }
                None => {
                    let _ = tree.set(id, property, PropertyValue::Ref(None));
                }
            }
        }
        self.link_waiting(tree);
    }

    fn link_waiting(&mut self, tree: &mut InstanceTree) {
        if self.waiting.is_empty() {
            return;
        }
        let done = self.me.is_some();
        let waiting = std::mem::take(&mut self.waiting);
        for (id, property, target) in waiting {
            match self.local(target) {
                Some(local) => {
                    let _ = tree.set(id, property, PropertyValue::Ref(Some(local)));
                }
                None if !done && tree.exists(id) => self.waiting.push((id, property, target)),
                None => {
                    let _ = tree.set(id, property, PropertyValue::Ref(None));
                }
            }
        }
    }

    fn fields(
        tree: &mut InstanceTree,
        id: InstanceId,
        op: &Json,
        links: &mut Vec<(InstanceId, Property, Option<u64>)>,
    ) {
        let mut found = Vec::new();
        set_fields(tree, id, op, &mut found);
        links.extend(found.into_iter().map(|(id, property, to)| (id, property, to.as_u64())));
    }

    /// Moves parts on toward where the host last put them.
    fn glide(&mut self, tree: &mut InstanceTree, dt: f32) {
        if self.glide.is_empty() {
            return;
        }
        let mut models = HashSet::new();
        for id in self.glide.keys() {
            let mut up = tree.parent(*id).ok().flatten();
            if up.is_some_and(|p| tree.class(p) == Ok(ClassKind::Accessory)) {
                up = up.and_then(|p| tree.parent(p).ok().flatten());
            }
            models.extend(up);
        }
        let step = 1.0 - (-GLIDE_RATE * dt).exp();
        self.glide.retain(|id, (to, to_turn)| {
            let (Some(at), Some(turn)) =
                (tree.get_vec3(*id, Property::Position), tree.get_vec3(*id, Property::Orientation))
            else {
                return false;
            };
            let there = (*to - at).length() < 1e-3 && (*to_turn - turn).length() < 1e-2;
            let (pos, orientation) = if there {
                (*to, *to_turn)
            } else {
                let from = libroxel_instance::Rotation::from_orientation(turn);
                let goal = libroxel_instance::Rotation::from_orientation(*to_turn);
                (at.lerp(*to, step), from.slerp(goal, step).to_orientation())
            };
            let _ = tree.set(*id, Property::Position, PropertyValue::Vec3(pos));
            let _ = tree.set(*id, Property::Orientation, PropertyValue::Vec3(orientation));
            !there
        });
        for model in models {
            crate::character::stick_to_head(tree, model);
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

/// Sets the name-less fields of an operation (properties, attributes, cells
/// and tags) on `id`. Links to other instances are left in `links`, each
/// with the target as sent (`null` for none), to be set once every instance
/// in the batch exists.
pub(crate) fn set_fields(
    tree: &mut InstanceTree,
    id: InstanceId,
    op: &Json,
    links: &mut Vec<(InstanceId, Property, Json)>,
) {
    if let Some(props) = op.get("v").and_then(Json::as_object) {
        for (file_name, value) in props {
            let Some(property) = Property::from_file_name(file_name) else { continue };
            if property.expected_kind() == "ref" {
                links.push((id, property, value.get("$i").cloned().unwrap_or(Json::Null)));
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
    if let Some(extras) = op.get("e").and_then(Json::as_object) {
        for (name, value) in extras {
            let _ = tree.set_extra(id, name, place::attribute_from_json(value));
        }
    }
    if let Some(text) = op.get("x").and_then(Json::as_str) {
        let _ = tree.edit_voxels(id, |v| *v = libroxel_instance::voxels::Voxels::decode(text));
    }
    if let Some(text) = op.get("xs").and_then(Json::as_str) {
        let _ = tree.edit_voxels(id, |v| v.patch(text));
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

fn move_json(input: CharacterInput) -> Json {
    json!({
        "t": "input",
        "move": [input.move_dir.x, input.move_dir.z],
        "jump": input.jump,
        "throttle": input.throttle,
        "steer": input.steer,
    })
}

/// Whether a solid wall stands between where a character was and where its
/// computer says it went: a cheat walking through walls ("noclip"). Two
/// rays, at the middle of the character and a stud higher, must both be
/// stopped, so steps, slopes and low ledges never count. Terrain and parts
/// that don't collide don't count either.
pub(crate) fn walls_between(tree: &InstanceTree, model: InstanceId, from: Vec3, to: Vec3) -> bool {
    let step = Vec3::new(to.x - from.x, to.y - from.y, to.z - from.z);
    let length = step.length();
    if !length.is_finite() || length < 0.25 {
        return false;
    }
    let solid = |part: InstanceId| {
        tree.class(part).is_ok_and(|c| c != ClassKind::Terrain)
            && tree.get_bool(part, Property::CanCollide) != Some(false)
            && !tree.is_descendant_of(part, model).unwrap_or(false)
    };
    [0.0, 1.0].iter().all(|up| {
        let start = Vec3::new(from.x, from.y + up, from.z);
        libroxel_instance::geometry::raycast(tree, start, step, solid)
            .is_some_and(|hit| hit.distance < length - 0.1)
    })
}

/// How far a joined computer may say its character went. Generous, so
/// lag, knocks and launch pads still get through, but nowhere near what a
/// teleport or a speed hack needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MoveLimits {
    /// Studs a second, sideways and up, kept up for as long as they like.
    pub(crate) speed: f32,
    /// Studs that may be saved up, for a burst after a lag spike.
    pub(crate) burst: f32,
}

impl MoveLimits {
    pub(crate) fn for_character(walk_speed: f32, jump_power: f32) -> Self {
        let fastest = walk_speed.max(16.0).max(jump_power.max(50.0) * 0.5);
        let speed = fastest * 4.0 + 100.0;
        Self { speed, burst: speed * 2.0 + 20.0 }
    }

    pub(crate) fn allowance(self, now: Instant) -> Allowance {
        Allowance::new(self.speed, self.burst, now)
    }

    /// How much of the allowance a move uses: falling is free, so a
    /// long fall never counts against anyone.
    pub(crate) fn distance(self, from: Vec3, to: Vec3) -> f32 {
        let sideways = Vec3::new(to.x - from.x, 0.0, to.z - from.z).length();
        let up = (to.y - from.y).max(0.0);
        let d = sideways + up;
        if d.is_finite() { d } else { f32::INFINITY }
    }

    /// A reported velocity, no faster than allowed (falling as fast as anything falls).
    pub(crate) fn clamp_velocity(self, v: Vec3) -> Vec3 {
        let sideways = Vec3::new(v.x, 0.0, v.z);
        let sideways =
            if sideways.length() > self.speed { sideways.normalize_or_zero() * self.speed } else { sideways };
        Vec3::new(sideways.x, v.y.clamp(-1000.0, self.speed), sideways.z)
    }
}

fn vec3_from(json: Option<&Json>) -> Option<Vec3> {
    let items = json?.as_array().filter(|a| a.len() == 3)?;
    let f = |i: usize| items[i].as_f64().map(|v| v as f32).filter(|v| v.is_finite());
    Some(Vec3::new(f(0)?, f(1)?, f(2)?))
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
        face: None,
    }
}

/// `host:port`, with the default port when none is given.
pub(crate) fn with_port(address: &str) -> String {
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
        let (streaming, big) = {
            let t = session.runtime.tree().borrow();
            let ws = t.workspace();
            let parts = t
                .descendants(ws)
                .map_or(0, |d| d.iter().filter(|i| t.class(**i).is_ok_and(ClassKind::is_physical)).count());
            let big = parts > BIG_MAP;
            (big || t.get_bool(ws, Property::StreamingEnabled).unwrap_or(false), big)
        };
        session.link = Link::Host(Box::new(Hosting {
            listener,
            peers: Vec::new(),
            streaming,
            big,
            replicator: None,
            priming: None,
            teleports: Vec::new(),
            banned: Vec::new(),
            who: None,
            network_joins: true,
            silence: SILENCE_LIMIT,
        }));
        Ok(session)
    }

    /// Joins the game hosted at `address` (`host` or `host:port`) as `name`,
    /// showing it in `tree`, which is emptied first. The world arrives over
    /// the next few steps.
    pub fn join(tree: SharedTree, address: &str, name: &str) -> Result<Self, String> {
        let began = Instant::now();
        let address = with_port(address);
        let target: SocketAddr = address
            .to_socket_addrs()
            .map_err(|e| format!("could not find that address: {e}"))?
            .next()
            .ok_or_else(|| "could not find that address".to_owned())?;
        let stream = TcpStream::connect_timeout(&target, Duration::from_secs(5))
            .map_err(|e| format!("could not reach the host: {e}"))?;
        let reached = began.elapsed().as_secs_f64();
        let mut conn = Conn::new(stream, Side::Joining).map_err(|e| e.to_string())?;
        conn.send(&json!({ "t": "hello", "name": name, "version": PROTOCOL, "ka": true }));
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
            owning: false,
            own_model: None,
            teleports: 0,
            glide: HashMap::new(),
            inbox: VecDeque::new(),
            waiting: Vec::new(),
            total: 0,
            got: 0,
            wrong_version: false,
            clock: JoinClock::new(began),
            loose: LooseParts::default(),
            address: address.clone(),
            name: name.to_owned(),
            look: None,
            calls: HashSet::new(),
            ended: false,
            turned_away: false,
            silence: SILENCE_LIMIT,
        }));
        session.info("🔗 Joining the game (its address stays hidden here)".to_owned());
        session.info(format!("⏱ Join: reached the host after {reached:.2} s"));
        session.info(format!(
            "⏱ Join: this computer is {} ({}-bit), {}",
            std::env::consts::ARCH,
            usize::BITS,
            std::env::consts::OS,
        ));
        Ok(session)
    }

    /// Tells the host how this computer's player looks, when joined.
    pub(crate) fn send_look(&mut self, look: crate::character::Look) {
        if let Link::Client(joined) = &mut self.link {
            let look = look.to_json();
            joined.conn.send(&json!({ "t": "look", "look": look }));
            joined.look = Some(look);
        }
    }

    /// How long the other side may say nothing before the connection
    /// counts as lost ([`SILENCE_LIMIT`] unless changed): for a host, each
    /// joined computer; for a joined computer, the host.
    pub fn set_silence_limit(&mut self, limit: Duration) {
        match &mut self.link {
            Link::Host(host) => host.silence = limit,
            Link::Client(joined) => joined.silence = limit,
            Link::Alone => {}
        }
    }

    /// Lets go of what the network side remembers about instances freed
    /// since (see [`tidy`](Self::tidy)).
    pub(crate) fn tidy_link(&mut self) {
        let Link::Client(joined) = &mut self.link else { return };
        let tree = self.runtime.tree().borrow();
        let alive = |id: &InstanceId| tree.is_destroyed(*id) == Ok(false);
        joined.started.retain(alive);
        joined.glide.retain(|id, _| alive(id));
    }

    /// Whether this joined game lost its connection in a way worth trying
    /// again: not because the host ended the game, removed us or turned us
    /// away.
    pub fn can_reconnect(&self) -> bool {
        matches!(&self.link, Link::Client(j)
            if j.conn.closed && !j.ended && !j.turned_away && !j.wrong_version)
    }

    /// Where this joined game is (`host:port`), to see whether it answers
    /// again before [`reconnect`](Self::reconnect)ing.
    pub fn joined_address(&self) -> Option<String> {
        match &self.link {
            Link::Client(joined) => Some(joined.address.clone()),
            _ => None,
        }
    }

    /// Joins the same game again after the connection was lost (see
    /// [`can_reconnect`](Self::can_reconnect)), as the same name and look.
    /// The host sees a player leave and come back; this computer gets the
    /// world as it is now, and its local scripts start over.
    pub fn reconnect(&mut self) -> Result<(), String> {
        let Link::Client(joined) = &self.link else { return Err("not a joined game".to_owned()) };
        let (address, name, look, silence) =
            (joined.address.clone(), joined.name.clone(), joined.look.clone(), joined.silence);
        let mut again = Self::join(self.runtime.tree().clone(), &address, &name)?;
        if let Link::Client(joined) = &mut again.link {
            joined.silence = silence;
            if let Some(look) = look {
                joined.conn.send(&json!({ "t": "look", "look": look }));
                joined.look = Some(look);
            }
        }
        let mut log = std::mem::take(&mut self.log);
        log.append(&mut again.log);
        again.log = log;
        self.stop();
        *self = again;
        Ok(())
    }

    /// How many instances the session keeps records about besides the
    /// world itself (what was sent, ids matched up, effects seen): for
    /// checking that a long game doesn't keep growing.
    pub fn bookkeeping_len(&self) -> usize {
        let own = self.exploded.len() + self.died.len();
        own + match &self.link {
            Link::Alone => 0,
            Link::Host(host) => {
                host.replicator.as_ref().map_or(0, |r| r.sent.len() + r.later.len())
                    + host.peers.iter().map(|p| p.stream.sent.len() + p.stream.later.len()).sum::<usize>()
            }
            Link::Client(j) => j.ids.len() + j.back.len() + j.glide.len() + j.started.len() + j.waiting.len(),
        }
    }

    /// Removes `player` from the game, telling them `why`: a joined
    /// computer is sent the reason and let go (and can't come back this
    /// game); the player on this computer just leaves.
    pub fn kick(&mut self, player: InstanceId, why: &str) {
        let shown = self.runtime.tree().borrow().name(player).unwrap_or("A player").to_owned();
        if let Link::Host(host) = &mut self.link
            && let Some(peer) = host.peers.iter_mut().find(|p| p.player == Some(player))
        {
            peer.conn.send(&json!({ "t": "kicked", "why": why }));
            peer.conn.flush();
            peer.conn.closed = true;
            host.banned.push(match &peer.account {
                Some(id) => Ban::Account(id.clone()),
                None => Ban::Unproved {
                    name: peer.asked.clone(),
                    ip: peer.from.map(|f| f.ip()).filter(|ip| !ip.is_loopback()),
                },
            });
            self.info(format!("{shown} was removed: {why}"));
            return;
        }
        if self.avatar_of(player).is_some() {
            self.warn(format!("{shown} was removed from the game: {why}"));
            self.leave(player);
        }
    }

    /// The players who joined from other computers, for the host to see.
    pub fn joined_players(&self) -> Vec<(InstanceId, String)> {
        let Link::Host(host) = &self.link else { return Vec::new() };
        let tree = self.runtime.tree().borrow();
        host.peers
            .iter()
            .filter_map(|p| p.player)
            .map(|p| (p, tree.name(p).unwrap_or("Player").to_owned()))
            .collect()
    }

    /// The proved player id of a player who joined from another computer,
    /// when their connection came through a tunnel that vouches for it
    /// (see [`set_guest_lookup`](Self::set_guest_lookup)). A player's name
    /// is whatever they typed; this is who they really are.
    pub fn account_of(&self, player: InstanceId) -> Option<String> {
        let Link::Host(host) = &self.link else { return None };
        host.peers.iter().find(|p| p.player == Some(player)).and_then(|p| p.account.clone())
    }

    /// While hosting: how to tell who a connection really is, such as
    /// `libroxel_net::HostTunnel::guest_lookup`. Players removed from the
    /// game are then kept out by who they are, not by the name they give.
    ///
    /// From then on only connections from this computer (the tunnel's) are
    /// taken, so nobody on the network gets in without the join code; see
    /// [`allow_network_joins`](Self::allow_network_joins).
    pub fn set_guest_lookup(&mut self, lookup: GuestLookup) {
        if let Link::Host(host) = &mut self.link {
            host.who = Some(lookup);
            host.network_joins = false;
        }
    }

    /// Lets computers on the network join by address too, after
    /// [`set_guest_lookup`](Self::set_guest_lookup). They have no proved id
    /// and see a safety code instead.
    pub fn allow_network_joins(&mut self) {
        if let Link::Host(host) = &mut self.link {
            host.network_joins = true;
        }
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

    pub fn joining(&self) -> Option<f32> {
        let Link::Client(joined) = &self.link else { return None };
        if joined.conn.closed || joined.me.is_some() {
            return None;
        }
        let part = if joined.total == 0 { 0.0 } else { joined.got as f32 / joined.total as f32 };
        Some(part.clamp(0.0, 1.0))
    }

    pub fn wrong_version(&self) -> bool {
        matches!(&self.link, Link::Client(joined) if joined.wrong_version)
    }

    /// Whether this computer has joined someone else's game.
    pub fn is_joined(&self) -> bool {
        matches!(self.link, Link::Client(_))
    }

    /// Whether this game is hosted or joined.
    pub fn online(&self) -> bool {
        !matches!(self.link, Link::Alone)
    }

    /// Sends one frame of this computer's voice ([`crate::voice::FRAME`] samples at [`crate::voice::RATE`]).
    pub fn speak(&mut self, samples: &[i16]) {
        if self.online() && !samples.is_empty() {
            let frame = &samples[..samples.len().min(crate::voice::FRAME)];
            self.voice.say(crate::voice::to_wire(frame));
        }
    }

    /// The voice heard since the last call: who spoke, and what.
    pub fn take_heard(&mut self) -> Vec<(InstanceId, Vec<i16>)> {
        std::mem::take(&mut self.voice.heard)
    }

    pub(crate) fn send_to_server(&mut self, says: ClientSays) {
        let Link::Client(joined) = &mut self.link else { return };
        let host_id = |id: InstanceId| joined.back.get(&id).copied();
        let message = match says {
            ClientSays::Tool(slot) => json!({ "t": "tool", "slot": slot }),
            ClientSays::Activate(down) => json!({ "t": "activate", "down": down }),
            ClientSays::Reset => json!({ "t": "reset" }),
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
        // Something may have moved a character since the last step (a
        // command, say); catch it before the player's moves overwrite it.
        host.teleports.extend(self.find_teleports());
        let now = Instant::now();
        while let Ok((stream, from)) = host.listener.accept() {
            // With a tunnel, everyone comes through it (and so has a proved
            // id and needs the code), unless the host asked otherwise.
            if host.who.is_some() && !host.network_joins && !from.ip().is_loopback() {
                continue;
            }
            // Computers that connect and never say hello are hung up on, and
            // only a few may wait at once, so they can't pile up.
            let waiting = host.peers.iter().filter(|p| p.player.is_none() && !p.conn.closed).count();
            if waiting >= MAX_WAITING {
                continue;
            }
            if let Ok(conn) = Conn::with_limit(stream, Side::Hosting, MAX_FROM_GUEST) {
                host.peers.push(Peer::new(conn, Some(from), now));
            }
        }
        let mut quiet = Vec::new();
        for peer in &mut host.peers {
            if peer.player.is_none() && now.saturating_duration_since(peer.since) > HELLO_TIMEOUT {
                peer.conn.closed = true;
            }
            // A computer that went away without hanging up (its Wi-Fi
            // dropped, it went to sleep) is let go, not kept as a player
            // standing still forever with the world piling up for it.
            if peer.player.is_some() && !peer.conn.closed && peer.conn.gone_quiet(host.silence, now) {
                peer.conn.closed = true;
                quiet.extend(peer.player);
            }
        }
        let mut too_much = Vec::new();
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
                let action = matches!(kind, "fire" | "invoke" | "click" | "prompt" | "tool" | "activate");
                if !peer.messages.spend(1.0, now) || (action && !peer.actions.spend(1.0, now)) {
                    // Too much at once: the rest is dropped, and the host told
                    // once. A call is answered, so the script that made it
                    // doesn't wait for ever.
                    if kind == "invoke"
                        && let Some(call) = message.get("call").and_then(Json::as_u64)
                    {
                        peer.send(json!({ "t": "result", "call": call, "ok": false, "a": "too many calls to the server at once" }));
                    }
                    if !peer.warned {
                        peer.warned = true;
                        too_much.push(peer.player);
                    }
                    continue;
                }
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
                        // No bad words in names.
                        let name = if name.is_empty() || libroxel_script::filter::filter(&name) != name {
                            "Player".to_owned()
                        } else {
                            name
                        };
                        peer.account = peer.from.and_then(|from| host.who.as_ref().and_then(|who| who(from)));
                        let ip = peer.from.map(|f| f.ip()).filter(|ip| !ip.is_loopback());
                        let banned = host.banned.iter().any(|ban| match (ban, &peer.account) {
                            (Ban::Account(id), Some(account)) => id == account,
                            (Ban::Unproved { name: n, ip: i }, None) => {
                                n == &name || (i.is_some() && *i == ip)
                            }
                            _ => false,
                        });
                        if banned {
                            peer.conn.send(
                                &json!({ "t": "refused", "why": "the host removed you from this game" }),
                            );
                            peer.conn.flush();
                            peer.conn.closed = true;
                            break;
                        }
                        peer.asked = name.clone();
                        peer.conn.keeps_alive = message.get("ka").and_then(Json::as_bool) == Some(true);
                        let player = self.add_player(&name, true);
                        peer.player = Some(player);
                        let shown = tree.borrow().name(player).unwrap_or("A player").to_owned();
                        match peer.conn.safety_code() {
                            // By address: the host can check the player sees the same digits.
                            Some(code) if peer.account.is_none() && !peer.conn.is_local() => {
                                self.info(format!("{shown} joined (safety code {code})"));
                            }
                            _ => self.info(format!("{shown} joined")),
                        }
                    }
                    continue;
                };
                if kind == "ka" {
                    peer.conn.keeps_alive = true;
                    continue;
                }
                let Some(index) = self.avatar_of(player) else { continue };
                match kind {
                    "input" => self.avatars[index].input = input_from(&message),
                    "look" => {
                        let usual = crate::character::Look::for_player(index);
                        let look = message.get("look").map_or_else(
                            || usual.clone(),
                            |l| crate::character::Look::from_json(l, usual.clone()),
                        );
                        self.set_look(player, look);
                    }
                    "move" => {
                        if let Some(back) = self.remote_move(index, &message, now) {
                            host.teleports.push(back);
                        }
                    }
                    "fire" => {
                        if let Some(remote) = id_in(&message, "r") {
                            let stream = host.streaming.then_some(&peer.stream);
                            let args = guest_args(&tree.borrow(), &message, stream);
                            let errors = self.runtime.deliver_server_event(remote, player, &args);
                            self.collect(errors);
                        }
                    }
                    "invoke" => {
                        let call = message.get("call").and_then(Json::as_u64).unwrap_or(0);
                        let remote = id_in(&message, "r")
                            .filter(|r| tree.borrow().class(*r) == Ok(ClassKind::RemoteFunction));
                        if let Some(remote) = remote {
                            let stream = host.streaming.then_some(&peer.stream);
                            let args = guest_args(&tree.borrow(), &message, stream);
                            let errors = self.runtime.deliver_invoke(call, remote, player, &args);
                            self.collect(errors);
                        } else {
                            // Gone from the server since (or never there): say
                            // so, or the call would wait for ever.
                            peer.send(json!({ "t": "result", "call": call, "ok": false, "a": "that RemoteFunction isn't on the server" }));
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
                    "reset" => self.reset_character_for(index),
                    "voice" => {
                        if let Some(text) = message.get("d").and_then(Json::as_str)
                            && let Some(samples) = crate::voice::from_wire(text)
                        {
                            if self.avatars.iter().any(|a| !a.remote) {
                                self.voice.hear(player, samples);
                            }
                            self.voice.relay.push((player, text.to_owned()));
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
        for player in too_much {
            let shown = player
                .and_then(|p| tree.borrow().name(p).ok().map(str::to_owned))
                .unwrap_or_else(|| "A computer that hasn't joined".to_owned());
            self.info(format!("{shown} is sending too much; some of it was ignored"));
        }
        // Players whose computers went away leave the game.
        let (gone, kept): (Vec<Peer>, Vec<Peer>) =
            std::mem::take(&mut host.peers).into_iter().partition(|p| p.conn.closed);
        host.peers = kept;
        for peer in gone {
            if let Some(player) = peer.player {
                let shown = tree.borrow().name(player).unwrap_or("A player").to_owned();
                self.leave(player);
                if quiet.contains(&player) {
                    self.info(format!("{shown} left (their connection went quiet)"));
                } else {
                    self.info(format!("{shown} left"));
                }
            }
        }
        self.link = Link::Host(host);
    }

    /// Sends joined computers what changed this step and what scripts sent them.
    pub(crate) fn serve_outgoing(&mut self) {
        let Link::Host(mut host) = std::mem::take(&mut self.link) else { return };
        let tree = self.runtime.tree().clone();
        let messages = self.runtime.take_net_messages();
        let mut said = std::mem::take(&mut self.voice.relay);
        let out = std::mem::take(&mut self.voice.out);
        if let Some(me) = self.avatars.iter().find(|a| !a.remote).map(|a| a.player) {
            said.extend(out.into_iter().map(|d| (me, d)));
        }
        for peer in &mut host.peers {
            let Some(player) = peer.player else { continue };
            if peer.conn.due_keep_alive(Instant::now()) {
                peer.send(json!({ "t": "ka" }));
            }
            for (from, d) in &said {
                if *from != player
                    && !peer.fresh
                    && peer.queue.is_empty()
                    && peer.conn.unsent_len() < SEND_AHEAD
                {
                    peer.conn.send(&json!({ "t": "voice", "p": bits(*from), "d": d }));
                }
            }
        }
        if host.peers.iter().all(|p| p.player.is_none()) {
            // No one to send to: forget what was sent, so the first to join gets everything.
            host.replicator = None;
            host.priming = None;
            host.teleports.clear();
            for peer in &mut host.peers {
                peer.pump();
            }
            self.link = Link::Host(host);
            return;
        }
        let mut teleports = std::mem::take(&mut host.teleports);
        teleports.extend(self.find_teleports());
        if host.streaming {
            self.serve_streamed(&mut host, teleports, messages);
            self.link = Link::Host(host);
            return;
        }
        let fresh = host.peers.iter().any(|p| p.fresh && p.player.is_some());
        let mut primed = None;
        if fresh && host.replicator.is_none() && host.priming.is_none() {
            let (tx, rx) = channel();
            host.priming = Some(rx);
            primed = Some(tx);
        }
        if host.replicator.is_none() && host.priming.is_none() && !fresh {
            host.replicator = Some(Replicator::default());
        }
        if let Some(priming) = &host.priming {
            match priming.try_recv() {
                Ok(replicator) => {
                    host.replicator = Some(replicator);
                    host.priming = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => host.priming = None,
            }
        }
        let delta = host.replicator.as_mut().map(|r| r.delta(&tree.borrow())).unwrap_or_default();
        // Everyone joining this step shares one copy of the world.
        let joining = host.peers.iter().filter(|p| p.fresh && p.player.is_some()).count();
        let mut worlds =
            if joining > 0 { worlds_for(tree.borrow().clone(), joining, primed.take()) } else { Vec::new() };
        // Written out and compressed once, then sent to every player as is.
        let delta =
            (!delta.is_empty()).then(|| json!({ "t": "rep", "ops": delta })).as_ref().and_then(encode);
        for peer in &mut host.peers {
            let Some(player) = peer.player else { continue };
            for (who, seq, pos, yaw) in &teleports {
                if *who == player {
                    peer.send(
                        json!({ "t": "teleport", "seq": seq, "pos": [pos.x, pos.y, pos.z], "yaw": yaw }),
                    );
                }
            }
            if peer.fresh {
                let t = tree.borrow();
                let services: Map<String, Json> = SHARED
                    .iter()
                    .filter_map(|c| Some((c.file_name().to_owned(), bits(t.service(*c)?))))
                    .collect();
                let total: usize = SHARED
                    .iter()
                    .filter_map(|c| t.service(*c))
                    .map(|s| t.descendants(s).map_or(0, |d| d.len()))
                    .sum();
                peer.send(json!({ "t": "welcome", "services": services, "total": total, "ka": true }));
                drop(t);
                if let Some(world) = worlds.pop() {
                    peer.send_world(world);
                }
                peer.send(json!({ "t": "you", "player": bits(player) }));
                peer.fresh = false;
            } else if let Some(delta) = &delta {
                peer.send_encoded(delta.clone());
            }
            for message in &messages {
                match message {
                    NetMessage::ClientEvent { remote, player: to, args }
                        if to.is_none_or(|p| p == player) =>
                    {
                        peer.send(json!({ "t": "event", "r": bits(*remote), "a": args }));
                    }
                    NetMessage::InvokeResult { call, player: to, ok, values } if *to == player => {
                        peer.send(json!({ "t": "result", "call": call, "ok": ok, "a": values }));
                    }
                    _ => {}
                }
            }
            peer.pump();
        }
        self.link = Link::Host(host);
    }

    /// [`serve_outgoing`](Self::serve_outgoing) for a game whose workspace
    /// streams: each player's computer gets its own share of the workspace.
    fn serve_streamed(
        &mut self,
        host: &mut Hosting,
        teleports: Vec<(InstanceId, u64, Vec3, f32)>,
        messages: Vec<NetMessage>,
    ) {
        let tree = self.runtime.tree().clone();
        let now = Instant::now();
        for (model, player, add) in self.runtime.take_persistent_players() {
            if let Some(peer) = host.peers.iter_mut().find(|p| p.player == Some(player)) {
                if add {
                    peer.persistent.insert(model);
                } else {
                    peer.persistent.remove(&model);
                }
            }
        }
        for (player, at, seconds) in self.runtime.take_stream_requests() {
            if let Some(peer) = host.peers.iter_mut().find(|p| p.player == Some(player)) {
                peer.stream_requests.push((at, now + Duration::from_secs_f32(seconds)));
            }
        }
        let t = tree.borrow();
        let ws = t.workspace();
        let radius = t
            .get_number(ws, Property::StreamingTargetRadius)
            .unwrap_or(1024.0)
            .max(t.get_number(ws, Property::StreamingMinRadius).unwrap_or(64.0)) as f32;
        let opportunistic = t.get_enum(ws, Property::StreamOutBehavior)
            == Some(libroxel_instance::EnumValue::StreamOutBehavior(StreamOutBehavior::Opportunistic));
        let keep = if opportunistic || host.big { radius * 1.5 } else { f32::INFINITY };
        let players = t
            .service(ClassKind::Players)
            .map(|p| t.children(p).unwrap_or_default().to_vec())
            .unwrap_or_default();
        let always: HashSet<InstanceId> =
            players.iter().filter_map(|p| t.get_ref(*p, Property::Character)).collect();
        let spawn = t
            .descendants_of_class(ws, ClassKind::SpawnLocation)
            .first()
            .and_then(|s| t.get_vec3(*s, Property::Position))
            .unwrap_or_default();
        let position_of = |id: InstanceId| -> Option<Vec3> {
            if let Some(at) = t.get_vec3(id, Property::Position) {
                return Some(at);
            }
            // A model: its primary part, or its first part.
            let part = t.get_ref(id, Property::PrimaryPart).or_else(|| {
                t.descendants(id).ok()?.into_iter().find(|d| t.class(*d).is_ok_and(ClassKind::is_physical))
            })?;
            t.get_vec3(part, Property::Position)
        };
        for peer in &mut host.peers {
            let Some(player) = peer.player else { continue };
            peer.stream_requests.retain(|(_, until)| *until > now);
            let mut points: Vec<Vec3> = peer.stream_requests.iter().map(|(at, _)| *at).collect();
            let centre = t
                .get_ref(player, Property::ReplicationFocus)
                .and_then(position_of)
                .or_else(|| t.get_ref(player, Property::Character).and_then(position_of))
                .unwrap_or(spawn);
            points.push(centre);
            let focus = Focus {
                points,
                radius,
                keep,
                always: always.clone(),
                persistent: peer.persistent.clone(),
                budget: if peer.fresh { STREAM_FIRST } else { STREAM_STEP },
            };
            for (who, seq, pos, yaw) in &teleports {
                if *who == player {
                    peer.send(
                        json!({ "t": "teleport", "seq": seq, "pos": [pos.x, pos.y, pos.z], "yaw": yaw }),
                    );
                }
            }
            let ops = peer.stream.delta_streamed(&t, &focus);
            if peer.fresh {
                let services: Map<String, Json> = SHARED
                    .iter()
                    .filter_map(|c| Some((c.file_name().to_owned(), bits(t.service(*c)?))))
                    .collect();
                peer.send(json!({ "t": "welcome", "services": services, "total": ops.len(), "ka": true }));
                for line in world_lines(&ops) {
                    peer.queue.push_back(Out::Line(line));
                }
                peer.send(json!({ "t": "you", "player": bits(player) }));
                peer.fresh = false;
            } else if !ops.is_empty() {
                peer.send(json!({ "t": "rep", "ops": ops }));
            }
            for message in &messages {
                match message {
                    NetMessage::ClientEvent { remote, player: to, args }
                        if to.is_none_or(|p| p == player) =>
                    {
                        peer.send(json!({ "t": "event", "r": bits(*remote), "a": args }));
                    }
                    NetMessage::InvokeResult { call, player: to, ok, values } if *to == player => {
                        peer.send(json!({ "t": "result", "call": call, "ok": ok, "a": values }));
                    }
                    _ => {}
                }
            }
            peer.pump();
        }
    }

    // --- Joined ------------------------------------------------------------

    /// One step of a joined game: take in what the host sent, run our local
    /// scripts, and send our input and what our scripts sent.
    pub(crate) fn client_step(&mut self, dt: f32, input: CharacterInput) {
        let Link::Client(mut joined) = std::mem::take(&mut self.link) else { return };
        let tree = self.runtime.tree().clone();
        let started = Instant::now();
        let read_before = joined.conn.bytes_read;
        let incoming = joined.conn.receive();
        let received = started.elapsed().as_secs_f64();
        let bytes_in = joined.conn.bytes_read - read_before;
        joined.inbox.extend(incoming);
        let mut applying = 0.0;
        let mut slowest_apply = 0.0f64;
        let mut ops_applied = 0u64;
        let mut marks: Vec<String> = Vec::new();
        let mut infos: Vec<String> = Vec::new();
        joined.clock.frames += 1;
        if !joined.clock.ready && joined.conn.ready() {
            joined.clock.ready = true;
            marks.push(joined.clock.mark("keys agreed with the host"));
        }
        let budget = if crate::view::LOADING_SCREEN.load(std::sync::atomic::Ordering::Relaxed) {
            LOADING_APPLY_BUDGET
        } else {
            APPLY_BUDGET
        };
        let began = std::time::Instant::now();
        while began.elapsed() < budget
            && let Some(mut message) = joined.inbox.pop_front()
        {
            match message.get("t").and_then(Json::as_str).unwrap_or("") {
                "ka" => joined.conn.keeps_alive = true,
                "ended" => {
                    self.warn("The host ended the game".to_owned());
                    joined.reported = true;
                    joined.ended = true;
                    joined.conn.closed = true;
                }
                "welcome" => {
                    joined.conn.keeps_alive |= message.get("ka").and_then(Json::as_bool) == Some(true);
                    joined.total = message.get("total").and_then(Json::as_u64).unwrap_or(0) as usize;
                    joined.got = 0;
                    if !joined.clock.welcomed {
                        if let Some(code) = joined.conn.safety_code().filter(|_| !joined.conn.is_local()) {
                            infos.push(format!(
                                "Connected. Safety code {code}: the host sees the same one unless someone is listening in"
                            ));
                        }
                        joined.clock.welcomed = true;
                        marks.push(joined.clock.mark(&format!(
                            "the host said hello, {} instances in the first batch,",
                            joined.total
                        )));
                    }
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
                    let ops = match message.get_mut("ops").map(Json::take) {
                        Some(Json::Array(ops)) => ops,
                        _ => Vec::new(),
                    };
                    if joined.me.is_none() {
                        joined.got += ops.len();
                    }
                    let at = Instant::now();
                    joined.apply(&mut tree.borrow_mut(), &ops);
                    let took = at.elapsed().as_secs_f64();
                    let count = ops.len() as u64;
                    joined.conn.discard(ops);
                    applying += took;
                    slowest_apply = slowest_apply.max(took);
                    ops_applied += count;
                    if !joined.clock.first {
                        joined.clock.first = true;
                        marks.push(joined.clock.mark("the first instances arrived"));
                    }
                }
                "you" => {
                    joined.me = message.get("player").and_then(Json::as_u64);
                    joined.link_waiting(&mut tree.borrow_mut());
                    if !joined.clock.done {
                        joined.clock.done = true;
                        // What this frame has spent so far counts too.
                        let c = &joined.clock;
                        let receive = c.receive + received;
                        let apply = c.apply + applying;
                        marks.push(c.mark(&format!("the first batch is in ({} instances)", joined.got)));
                        marks.push(format!(
                            "⏱ Join: so far {:.1} MB from the host, {receive:.2} s taking it in, {apply:.2} s putting it in the world (slowest message {:.0} ms)",
                            (c.bytes + bytes_in) as f64 / 1_000_000.0,
                            c.slowest.max(slowest_apply) * 1000.0,
                        ));
                    }
                }
                "kicked" => {
                    let why =
                        message.get("why").and_then(Json::as_str).unwrap_or("You were removed from the game");
                    self.warn(format!("The host removed you: {why}"));
                    joined.reported = true;
                    joined.turned_away = true;
                    joined.conn.closed = true;
                }
                "teleport" => {
                    let seq = message.get("seq").and_then(Json::as_u64).unwrap_or(0);
                    let pos = vec3_from(message.get("pos"));
                    let yaw = message.get("yaw").and_then(Json::as_f64).unwrap_or(0.0) as f32;
                    if let (Some(pos), Some(c)) = (pos, self.character()) {
                        let mut t = tree.borrow_mut();
                        let _ = t.set(c.root, Property::Position, PropertyValue::Vec3(pos));
                        let _ = t.set(
                            c.root,
                            Property::Orientation,
                            PropertyValue::Vec3(Vec3::new(0.0, yaw, 0.0)),
                        );
                        c.follow(&mut t);
                        self.physics.set_velocity(c.root, Vec3::ZERO);
                    }
                    joined.teleports = joined.teleports.max(seq);
                }
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
                    joined.calls.remove(&call);
                    let ok = message.get("ok").and_then(Json::as_bool).unwrap_or(false);
                    let mut values = message.get("a").cloned().unwrap_or(Json::Null);
                    joined.to_local(&mut values);
                    self.runtime.deliver_invoke_result(call, ok, values);
                }
                "voice" => {
                    let from = message.get("p").and_then(|p| joined.local_of(p));
                    let samples = message.get("d").and_then(Json::as_str).and_then(crate::voice::from_wire);
                    if let (Some(from), Some(samples)) = (from, samples) {
                        self.voice.hear(from, samples);
                    }
                }
                "refused" => {
                    let why = message.get("why").and_then(Json::as_str).unwrap_or("no reason given");
                    joined.wrong_version |= why.contains("version");
                    joined.turned_away = true;
                    self.warn(format!("The game turned us away: {why}"));
                }
                _ => {}
            }
        }
        let waiting = joined.inbox.len();
        if !joined.clock.done {
            let c = &mut joined.clock;
            c.receive += received;
            c.apply += applying;
            c.slowest = c.slowest.max(slowest_apply);
            c.bytes += bytes_in;
        }
        for line in infos {
            self.info(line);
        }
        for mark in marks {
            self.info(mark);
        }
        if joined.conn.closed && !joined.conn.ready() {
            joined.wrong_version = true;
        }
        if !joined.conn.closed && joined.conn.gone_quiet(joined.silence, Instant::now()) {
            // The host went away without hanging up (or the network
            // between did): don't wait for ever.
            joined.conn.closed = true;
            joined.reported = true;
            self.warn("Lost the connection to the game: the host stopped answering".to_owned());
        }
        if joined.conn.closed && joined.inbox.is_empty() && !joined.reported {
            joined.reported = true;
            self.warn("Lost the connection to the game");
        }
        if joined.conn.closed && joined.inbox.is_empty() {
            // No answer is coming: calls waiting for one fail, and the
            // scripts that made them go on.
            for call in joined.calls.drain() {
                self.runtime.deliver_invoke_result(call, false, json!("lost the connection to the server"));
            }
        }

        self.adopt_player(&mut joined);
        joined.glide(&mut tree.borrow_mut(), dt);

        let at = Instant::now();
        let changes = tree.borrow_mut().drain_changes();
        let errors = self.runtime.fire_changes(&changes);
        self.collect(errors);
        let mut change_events = at.elapsed().as_secs_f64();
        let at = Instant::now();
        let errors = self.runtime.step(dt as f64);
        self.collect(errors);
        let scripts = at.elapsed().as_secs_f64();
        let at = Instant::now();
        self.settle_changes();
        change_events += at.elapsed().as_secs_f64();
        let mut physics = 0.0;
        if let Some(index) = self.avatars.iter().position(|a| !a.remote) {
            self.advance_hold(index, dt);
            let at = Instant::now();
            self.move_own_character(&mut joined, index, input, dt);
            physics = at.elapsed().as_secs_f64();
            self.refresh_hotbar(index);
        }
        // Explosions only flash here: the host does what they do.
        let at = Instant::now();
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
        let explosion_scan = at.elapsed().as_secs_f64();
        for (explosion, position, radius) in fresh {
            self.exploded.insert(explosion);
            self.blasts.push(Blast { position, radius, age: 0.0 });
        }
        self.blasts.retain_mut(|b| {
            b.age += dt;
            b.age < crate::session::BLAST_TIME
        });
        self.emits.extend(self.runtime.take_emit_requests());

        let t = &mut self.frame_times;
        t.frames += 1;
        t.receive += received;
        t.apply += applying;
        t.change_events += change_events;
        t.scripts += scripts;
        t.physics += physics;
        t.explosion_scan += explosion_scan;
        t.other += (started.elapsed().as_secs_f64()
            - received
            - applying
            - change_events
            - scripts
            - physics
            - explosion_scan)
            .max(0.0);
        t.bytes_in += bytes_in;
        t.ops_applied += ops_applied;
        t.slowest_apply = t.slowest_apply.max(slowest_apply);
        t.most_waiting = t.most_waiting.max(waiting);

        let input = CharacterInput { face: None, ..input };
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
                    let Some(r) = joined.back.get(&remote).copied() else {
                        // Made here, so the server has never heard of it.
                        self.runtime.deliver_invoke_result(
                            call,
                            false,
                            json!("that RemoteFunction isn't on the server"),
                        );
                        continue;
                    };
                    if joined.conn.closed {
                        self.runtime.deliver_invoke_result(
                            call,
                            false,
                            json!("lost the connection to the server"),
                        );
                        continue;
                    }
                    joined.to_host(&mut args);
                    joined.conn.send(&json!({ "t": "invoke", "call": call, "r": r, "a": args }));
                    joined.calls.insert(call);
                }
                _ => {}
            }
        }
        let out = std::mem::take(&mut self.voice.out);
        if joined.me.is_some() {
            for d in out {
                joined.conn.send(&json!({ "t": "voice", "d": d }));
            }
        }
        if joined.conn.due_keep_alive(Instant::now()) {
            joined.conn.send(&json!({ "t": "ka" }));
        }
        joined.conn.flush();
        self.link = Link::Client(joined);
    }

    /// On the host: a joined player's computer says where its character is.
    /// A joined computer saying where its character went. A move further
    /// than the character could have gone is refused: the character stays,
    /// and the teleport returned puts it back on that computer too.
    fn remote_move(
        &mut self,
        index: usize,
        message: &Json,
        now: Instant,
    ) -> Option<(InstanceId, u64, Vec3, f32)> {
        let tree = self.runtime.tree().clone();
        let avatar = &self.avatars[index];
        let c = avatar.character?;
        let current = message.get("m").and_then(Json::as_u64) == Some(c.model.to_bits());
        let seq = message.get("seq").and_then(Json::as_u64).unwrap_or(0);
        if !current || seq != avatar.teleports || !avatar.moves_itself(&tree.borrow()) {
            return None;
        }
        let pos = vec3_from(message.get("pos"))?;
        let vel = vec3_from(message.get("vel"));
        let yaw = message.get("yaw").and_then(Json::as_f64).filter(|y| y.is_finite()).unwrap_or(0.0) as f32;
        let (from, limits) = {
            let t = tree.borrow();
            let from = t.get_vec3(c.root, Property::Position).unwrap_or(pos);
            let walk = t.get_number(c.humanoid, Property::WalkSpeed).unwrap_or(16.0) as f32;
            let jump = t.get_number(c.humanoid, Property::JumpPower).unwrap_or(50.0) as f32;
            (from, MoveLimits::for_character(walk, jump))
        };
        let through_wall = walls_between(&tree.borrow(), c.model, from, pos);
        let avatar = &mut self.avatars[index];
        let travel = avatar.travel.get_or_insert_with(|| limits.allowance(now));
        travel.per_second = limits.speed;
        travel.most = limits.burst;
        if !travel.spend(limits.distance(from, pos), now) || through_wall {
            avatar.teleports += 1;
            avatar.moved_to = Some(from);
            let facing = tree.borrow().get_vec3(c.root, Property::Orientation).unwrap_or_default().y;
            self.physics.set_velocity(c.root, Vec3::ZERO);
            return Some((avatar.player, avatar.teleports, from, facing));
        }
        {
            let mut t = tree.borrow_mut();
            let _ = t.set(c.root, Property::Position, PropertyValue::Vec3(pos));
            let _ = t.set(c.root, Property::Orientation, PropertyValue::Vec3(Vec3::new(0.0, yaw, 0.0)));
        }
        self.physics.set_velocity(c.root, limits.clamp_velocity(vel.unwrap_or_default()));
        self.avatars[index].moved_to = Some(pos);
        None
    }

    /// On the host: joined players' characters that something here moved
    /// (a script, a respawn) since their computer last said where they
    /// were. Each gets a new teleport number, and later moves from before it
    /// are ignored.
    fn find_teleports(&mut self) -> Vec<(InstanceId, u64, Vec3, f32)> {
        let tree = self.runtime.tree().clone();
        let t = tree.borrow();
        let mut found = Vec::new();
        for avatar in &mut self.avatars {
            if !avatar.moves_itself(&t) {
                avatar.moved_to = None;
                continue;
            }
            let Some(c) = avatar.character else { continue };
            let Some(pos) = t.get_vec3(c.root, Property::Position) else { continue };
            match avatar.moved_to {
                Some(at) if (at - pos).length() > 0.05 => {
                    avatar.teleports += 1;
                    avatar.moved_to = Some(pos);
                    let yaw = t.get_vec3(c.root, Property::Orientation).unwrap_or_default().y;
                    found.push((avatar.player, avatar.teleports, pos, yaw));
                }
                Some(_) => {}
                None => avatar.moved_to = Some(pos),
            }
        }
        found
    }

    /// On a joined computer: walks our own character here, straight away,
    /// and tells the host where it got to. Everything else stays where the
    /// host puts it.
    fn move_own_character(&mut self, joined: &mut Joined, index: usize, input: CharacterInput, dt: f32) {
        let tree = self.runtime.tree().clone();
        let Some(c) = self.avatars[index].character else {
            joined.owning = false;
            return;
        };
        joined.own_model = Some(c.model);
        let mut input = input;
        joined.owning = {
            let mut t = tree.borrow_mut();
            let owning = !c.is_dead(&t) && t.get_ref(c.humanoid, Property::SeatPart).is_none();
            if owning {
                if t.get_bool(c.humanoid, Property::Jump) == Some(true) {
                    input.jump = true;
                    let _ = t.set(c.humanoid, Property::Jump, PropertyValue::Bool(false));
                }
                let mut others = joined.loose.update(&t).clone();
                others.remove(&c.root);
                // Other players' characters are where the host last put
                // them. Bumping into them here as well as on the host made
                // two players on one spot push each other up into the sky,
                // so here our character passes through them and the host
                // alone sorts out who stands where.
                let players = t.service(ClassKind::Players);
                let theirs: Vec<(InstanceId, InstanceId)> = players
                    .map(|players| t.children(players).unwrap_or_default().to_vec())
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|player| t.get_ref(player, Property::Character))
                    .filter(|model| *model != c.model)
                    .flat_map(|model| t.descendants_of_class(model, ClassKind::Part))
                    .map(|part| (c.root, part))
                    .collect();
                self.physics.set_assemblies(others, theirs);
                c.drive(&t, &mut self.physics, input, dt);
                c.share_moving(&mut t, &self.physics);
                self.physics.step(&mut t, dt);
                c.follow(&mut t);
            }
            owning
        };
        self.pose_character(index, input, dt);
        if !joined.owning {
            return;
        }
        let (Some(model), Some(pos)) =
            (joined.back.get(&c.model).copied(), tree.borrow().get_vec3(c.root, Property::Position))
        else {
            return;
        };
        let yaw = tree.borrow().get_vec3(c.root, Property::Orientation).unwrap_or_default().y;
        let v = self.physics.velocity(c.root);
        joined.conn.send(&json!({
            "t": "move",
            "m": model,
            "seq": joined.teleports,
            "pos": [pos.x, pos.y, pos.z],
            "yaw": yaw,
            "vel": [v.x, v.y, v.z],
        }));
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
        // Start the local scripts of tools, of the player's scripts and of
        // the character's scripts as they arrive.
        let avatar = &self.avatars[index];
        let arrived: Vec<InstanceId> = {
            let t = tree.borrow();
            let scripts = t.find_first_child_of_class(player, ClassKind::PlayerScripts).ok().flatten();
            [avatar.backpack, avatar.character.map(|c| c.model), scripts]
                .into_iter()
                .flatten()
                .flat_map(|c| t.children(c).unwrap_or_default().to_vec())
                .collect()
        };
        for id in arrived {
            if joined.started.insert(id) {
                let errors = self.runtime.run_local_scripts(id);
                self.collect(errors);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host's end of a connection on this machine, taking messages up to
    /// `limit` long, and the joining end, plain.
    fn raw_pair(limit: usize) -> (Conn, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let other = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (stream, _) = listener.accept().unwrap();
        (Conn::with_limit(stream, Side::Hosting, limit).unwrap(), other)
    }

    /// A host's end and a joining end, both encrypted.
    fn pair(limit: usize) -> (Conn, Conn) {
        let (host, other) = raw_pair(limit);
        (host, Conn::new(other, Side::Joining).unwrap())
    }

    #[test]
    fn both_ends_see_the_same_safety_code() {
        let (mut host, mut guest) = pair(MAX_BACKLOG);
        connect(&mut host, &mut guest);
        let code = host.safety_code().unwrap().to_owned();
        assert_eq!(guest.safety_code(), Some(code.as_str()));
        assert_eq!(code.len(), 9, "{code}");
        // Another connection gets other digits.
        let (mut host2, mut guest2) = pair(MAX_BACKLOG);
        connect(&mut host2, &mut guest2);
        assert_ne!(host2.safety_code(), Some(code.as_str()));
    }

    #[test]
    fn walking_through_a_wall_is_caught_but_not_walking_past_one() {
        let mut tree = InstanceTree::new();
        let ws = tree.workspace();
        let wall = tree.create(ClassKind::Part, "Wall").unwrap();
        tree.set_parent(wall, Some(ws)).unwrap();
        tree.set(wall, Property::Size, PropertyValue::Vec3(Vec3::new(1.0, 10.0, 20.0))).unwrap();
        tree.set(wall, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, 5.0, 0.0))).unwrap();
        tree.set(wall, Property::Anchored, PropertyValue::Bool(true)).unwrap();
        let model = tree.create(ClassKind::Model, "Sam").unwrap();
        tree.set_parent(model, Some(ws)).unwrap();
        let at = |x: f32, z: f32| Vec3::new(x, 3.0, z);
        assert!(walls_between(&tree, model, at(-2.0, 0.0), at(2.0, 0.0)), "straight through");
        assert!(!walls_between(&tree, model, at(-2.0, 0.0), at(-0.8, 0.0)), "up to it");
        assert!(!walls_between(&tree, model, at(-2.0, 15.0), at(2.0, 15.0)), "round the end");
        // A wall players walk through on purpose.
        tree.set(wall, Property::CanCollide, PropertyValue::Bool(false)).unwrap();
        assert!(!walls_between(&tree, model, at(-2.0, 0.0), at(2.0, 0.0)));
    }

    #[test]
    fn guests_can_only_name_what_they_were_sent() {
        let mut tree = InstanceTree::new();
        let ws = tree.workspace();
        let shared = tree.create(ClassKind::Part, "Coin").unwrap();
        tree.set_parent(shared, Some(ws)).unwrap();
        let storage = tree.service(ClassKind::ServerStorage).unwrap();
        let sword = tree.create(ClassKind::Tool, "AdminSword").unwrap();
        tree.set_parent(sword, Some(storage)).unwrap();
        let message = json!({ "t": "fire", "a": [
            { "$i": shared.to_bits() },
            { "$i": sword.to_bits() },
            { "list": [{ "$i": sword.to_bits() }] },
            { "$i": storage.to_bits() },
            { "$i": ws.to_bits() },
        ]});
        let args = guest_args(&tree, &message, None);
        assert_eq!(
            args,
            json!([{ "$i": shared.to_bits() }, null, { "list": [null] }, null, { "$i": ws.to_bits() }])
        );
    }

    /// Steps both ends until their keys are agreed.
    fn connect(host: &mut Conn, guest: &mut Conn) {
        let start = Instant::now();
        while !(host.ready() && guest.ready()) && start.elapsed() < Duration::from_secs(1) {
            guest.flush();
            let _ = host.receive();
            let _ = guest.receive();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(host.ready() && guest.ready(), "keys agreed");
    }

    /// Lets `guest` send and `host` take in until `done` says so, or a
    /// second has gone by.
    fn deliver(host: &mut Conn, guest: &mut Conn, done: impl Fn(&Conn, &[Json]) -> bool) -> Vec<Json> {
        let mut all = Vec::new();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(1) {
            guest.flush();
            let _ = guest.receive();
            all.extend(host.receive());
            if done(host, &all) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        all
    }

    #[test]
    fn messages_within_the_limit_arrive() {
        let (mut host, mut guest) = pair(1024);
        guest.send_raw(b"{\"t\":\"a\"}\n{\"t\":\"b\"}\nnot json\n{\"t\":");
        let got = deliver(&mut host, &mut guest, |_, m| m.len() >= 2);
        assert_eq!(got, vec![json!({"t": "a"}), json!({"t": "b"})]);
        guest.send_raw(b"\"c\"}\n");
        let got = deliver(&mut host, &mut guest, |_, m| !m.is_empty());
        assert_eq!(got, vec![json!({"t": "c"})]);
        assert!(host.ready() && guest.ready() && !host.closed);
        // Back the other way too.
        host.send(&json!({ "t": "welcome" }));
        host.flush();
        let mut back = Vec::new();
        let start = Instant::now();
        while back.is_empty() && start.elapsed() < Duration::from_secs(1) {
            back = guest.receive();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(back, vec![json!({ "t": "welcome" })]);
    }

    #[test]
    fn nothing_readable_goes_over_the_wire() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut host =
            Conn::new(TcpStream::connect(listener.local_addr().unwrap()).unwrap(), Side::Hosting).unwrap();
        let (stream, _) = listener.accept().unwrap();
        // Listen in on what the joining side sends.
        let mut guest = Conn::new(stream, Side::Joining).unwrap();
        guest.send(&json!({ "t": "hello", "name": "SecretName", "password": "hunter2" }));
        connect(&mut host, &mut guest);
        guest.send(&json!({ "t": "fire", "a": ["SecretName says hunter2"] }));
        guest.seal();
        let wire = String::from_utf8_lossy(&guest.unsent).into_owned();
        assert!(
            !wire.contains("SecretName") && !wire.contains("hunter2") && !wire.contains("fire"),
            "{wire}"
        );
        let got = deliver(&mut host, &mut guest, |_, m| m.len() >= 2);
        assert_eq!(got[1]["a"][0], "SecretName says hunter2");
    }

    #[test]
    fn a_changed_byte_on_the_way_closes_the_connection() {
        let (mut host, mut guest) = pair(1024);
        connect(&mut host, &mut guest);
        guest.send(&json!({ "t": "input" }));
        guest.seal();
        let last = guest.unsent.len() - 1;
        guest.unsent[last] ^= 1;
        let got = deliver(&mut host, &mut guest, |c, _| c.closed);
        assert!(got.is_empty() && host.closed);
    }

    #[test]
    fn an_older_copy_talking_plain_json_is_told_why_it_cant_join() {
        let (mut host, mut old) = raw_pair(1024);
        old.write_all(b"{\"t\":\"hello\",\"version\":2,\"name\":\"Bea\"}\n").unwrap();
        let start = Instant::now();
        while !host.closed && start.elapsed() < Duration::from_secs(1) {
            let _ = host.receive();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(host.closed);
        old.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let mut answer = String::new();
        let _ = old.read_to_string(&mut answer);
        assert!(answer.contains("refused") && answer.contains("a different version"), "{answer}");
    }

    #[test]
    fn a_message_longer_than_the_limit_closes_the_connection() {
        let (mut host, mut guest) = pair(1024);
        // No newline ever comes: before, this grew without end.
        guest.send_raw(&[b'x'; 4096]);
        deliver(&mut host, &mut guest, |c, _| c.closed);
        assert!(host.closed);
        assert!(host.received.is_empty(), "what came in is let go");

        let (mut host, mut guest) = pair(1024);
        let mut long = vec![b'"'; 1];
        long.extend([b'y'; 2000]);
        long.extend(b"\"\n");
        guest.send_raw(&long);
        let got = deliver(&mut host, &mut guest, |c, _| c.closed);
        assert!(got.is_empty() && host.closed);
    }

    #[test]
    fn long_messages_travel_compressed_and_arrive_whole() {
        let (mut host, mut guest) = pair(MAX_FROM_GUEST);
        connect(&mut host, &mut guest);
        // A world's worth of parts: long, and much the same over and over.
        let parts: Vec<Json> = (0..2000)
            .map(|i| json!({ "class": "Part", "name": format!("Brick{i}"), "size": [4, 1, 2], "anchored": true }))
            .collect();
        let big = json!({ "t": "rep", "ops": parts });
        let small = json!({ "t": "input", "move": [1.0, 0.0] });
        let plain_length = serde_json::to_vec(&big).unwrap().len();
        guest.send(&big);
        guest.send(&small);
        assert!(guest.unsent.len() * 4 < plain_length, "{} vs {plain_length}", guest.unsent.len());
        let got = deliver(&mut host, &mut guest, |_, m| m.len() >= 2);
        assert_eq!(got, vec![big, small]);
        assert!(!host.closed);
    }

    #[test]
    fn a_compressed_message_that_unpacks_too_big_closes_the_connection() {
        let (mut host, mut guest) = pair(64 * 1024);
        // A few kilobytes that unpack to megabytes.
        let bomb = format!("\"{}\"", "a".repeat(8 * 1024 * 1024));
        let mut packed = pack(bomb.as_bytes()).unwrap();
        assert!(packed.len() < 64 * 1024);
        packed.push(b'\n');
        guest.send_raw(&packed);
        let got = deliver(&mut host, &mut guest, |c, _| c.closed);
        assert!(got.is_empty() && host.closed);
        // Rubbish inside a z message is skipped, not taken in.
        let (mut host, mut guest) = pair(64 * 1024);
        guest.send_raw(b"{\"t\":\"z\",\"d\":\"not base64!\"}\n{\"t\":\"ok\"}\n");
        let got = deliver(&mut host, &mut guest, |_, m| !m.is_empty());
        assert_eq!(got, vec![json!({ "t": "ok" })]);
    }

    #[test]
    fn an_allowance_refills_over_time_up_to_its_most() {
        let start = Instant::now();
        let mut a = Allowance::new(10.0, 20.0, start);
        assert!(a.spend(20.0, start));
        assert!(!a.spend(1.0, start));
        assert!(a.spend(5.0, start + Duration::from_millis(500)));
        assert!(!a.spend(1.0, start + Duration::from_millis(500)));
        // Waiting a long time saves up no more than the most.
        assert!(!a.spend(21.0, start + Duration::from_secs(60)));
        assert!(a.spend(20.0, start + Duration::from_secs(60)));
    }

    #[test]
    fn walking_and_falling_are_fine_but_teleports_and_speed_hacks_are_not() {
        let limits = MoveLimits::for_character(16.0, 50.0);
        let start = Instant::now();
        let mut travel = limits.allowance(start);
        let mut at = Vec3::new(0.0, 3.0, 0.0);
        // A minute of running at full speed, 60 moves a second.
        for frame in 1..=3600 {
            let next = at + Vec3::new(16.0 / 60.0, 0.0, 0.0);
            let now = start + Duration::from_secs_f32(frame as f32 / 60.0);
            assert!(travel.spend(limits.distance(at, next), now), "frame {frame}");
            at = next;
        }
        // A long fall costs nothing.
        assert_eq!(limits.distance(at, at - Vec3::new(0.0, 500.0, 0.0)), 0.0);
        // Half a second of lag, then everything at once, still gets through.
        let later = start + Duration::from_secs_f32(60.5);
        assert!(travel.spend(limits.distance(at, at + Vec3::new(8.0, 0.0, 0.0)), later));

        // A jump across the map is refused.
        let mut travel = limits.allowance(start);
        assert!(!travel.spend(limits.distance(at, at + Vec3::new(2000.0, 0.0, 0.0)), start));
        // So is running ten times too fast for long.
        let mut travel = limits.allowance(start);
        let mut refused = false;
        for frame in 1..=600 {
            let now = start + Duration::from_secs_f32(frame as f32 / 60.0);
            if !travel.spend(160.0 * 10.0 / 60.0, now) {
                refused = true;
                break;
            }
        }
        assert!(refused);
        // Numbers that aren't numbers never pass.
        assert!(!limits.allowance(start).spend(limits.distance(at, Vec3::new(f32::NAN, 0.0, 0.0)), start));
    }

    #[test]
    fn faster_characters_may_move_faster() {
        let slow = MoveLimits::for_character(16.0, 50.0);
        let fast = MoveLimits::for_character(100.0, 50.0);
        assert!(fast.speed > slow.speed && fast.speed >= 400.0);
        let v = slow.clamp_velocity(Vec3::new(1e6, -1e6, 0.0));
        assert!((v.x - slow.speed).abs() < 0.01 && v.y == -1000.0);
    }

    #[test]
    fn nearer_parts_stream_in_first_and_update_more_often() {
        let mut tree = InstanceTree::new();
        let ws = tree.workspace();
        let part = |tree: &mut InstanceTree, x: f32| {
            let id = tree.create(ClassKind::Part, "P").unwrap();
            tree.set_parent(id, Some(ws)).unwrap();
            tree.set(id, Property::Position, PropertyValue::Vec3(Vec3::new(x, 0.0, 0.0))).unwrap();
            tree.set(id, Property::Size, PropertyValue::Vec3(Vec3::new(1.0, 1.0, 1.0))).unwrap();
            id
        };
        let (near, mid, far, gone) =
            (part(&mut tree, 10.0), part(&mut tree, 500.0), part(&mut tree, 900.0), part(&mut tree, 5000.0));
        let mut focus = Focus {
            points: vec![Vec3::new(0.0, 0.0, 0.0)],
            radius: 1024.0,
            keep: 1536.0,
            always: HashSet::new(),
            persistent: HashSet::new(),
            budget: 2,
        };
        let mut replicator = Replicator::default();
        let (first, more) = focus.wanted(&tree, &replicator.sent);
        assert!(more);
        assert!(first.contains_key(&near) && first.contains_key(&mid));
        assert!(!first.contains_key(&far) && !first.contains_key(&gone));
        replicator.delta_streamed(&tree, &focus);
        focus.budget = 100;
        let (next, _) = focus.wanted(&tree, &replicator.sent);
        assert_eq!((next[&near], next[&mid], next[&far]), (1, 3, 8));
        assert!(!next.contains_key(&gone));
    }
}
