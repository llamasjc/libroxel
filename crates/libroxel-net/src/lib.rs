//! Player identity and join codes for Libroxel.
//!
//! A hosted game listens on a local TCP port (see `libroxel_engine::net`).
//! This crate carries that game between computers over [iroh], which
//! connects two computers directly even when both sit behind home routers,
//! and falls back to a relay server when a direct path cannot be found.
//!
//! - [`Identity`] is a player's keypair, made once and kept on disk. Its
//!   public half is the player's permanent ID and the iroh endpoint ID.
//! - [`HostTunnel`] runs next to a hosted game and hands out a join code.
//! - [`JoinTunnel`] takes a join code and gives back a local address that
//!   `PlaySession::join` connects to as if the host were on this computer.
//!
//! - [`Pinger`] measures how far away hosts are, for server lists.
//!
//! Every connection is encrypted end to end, relayed or not.

use std::fmt;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use data_encoding::BASE32_NOPAD;
use iroh::endpoint::{Connection, presets};
use iroh::{Endpoint, EndpointAddr, RelayMode, SecretKey};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

/// The protocol name both sides agree on. Bump the number when the game's
/// wire protocol changes in a way old versions cannot read.
pub const ALPN: &[u8] = b"libroxel/game/0";

/// Every join code starts with this, so the Join box can tell a code from an
/// address.
pub const CODE_PREFIX: &str = "lrx1";

/// The protocol server lists use to time a round trip to a host. A host
/// echoes back whatever arrives on each stream.
pub const PING_ALPN: &[u8] = b"libroxel/ping/0";

/// How long a host waits to reach a relay before handing out a code that
/// only holds its direct addresses.
const ONLINE_WAIT: Duration = Duration::from_secs(8);

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// A player's keypair. The secret half never leaves this computer.
#[derive(Clone)]
pub struct Identity {
    key: SecretKey,
}

impl Identity {
    /// A fresh identity that is not saved anywhere.
    pub fn generate() -> Self {
        Self { key: SecretKey::generate() }
    }

    /// The identity saved at `path`, making and saving a new one the first
    /// time. The file holds the 32-byte secret key and nothing else.
    pub fn load_or_create(path: &Path) -> std::io::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => {
                let bytes: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("{} is not a Libroxel identity", path.display()),
                    )
                })?;
                Ok(Self { key: SecretKey::from_bytes(&bytes) })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let identity = Self::generate();
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                write_private(path, &identity.key.to_bytes())?;
                Ok(identity)
            }
            Err(e) => Err(e),
        }
    }

    /// The identity whose secret key is `bytes`, such as one signed in to
    /// with a password.
    pub fn from_secret_bytes(bytes: &[u8; 32]) -> Self {
        Self { key: SecretKey::from_bytes(bytes) }
    }

    /// The secret key itself. Only for sealing it with a password; never
    /// send it anywhere.
    pub fn secret_bytes(&self) -> [u8; 32] {
        self.key.to_bytes()
    }

    /// Saves this identity at `path` in place of whatever is there. A
    /// different identity already there is kept beside it as
    /// `<name>.old-<seconds since 1970>`, whose path comes back, so no
    /// account is ever lost by signing in to another.
    pub fn save_replacing(&self, path: &Path) -> std::io::Result<Option<std::path::PathBuf>> {
        let mut kept = None;
        match std::fs::read(path) {
            Ok(old) if old == self.key.to_bytes() => return Ok(None),
            Ok(_) => {
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                let mut name = path.file_name().unwrap_or_default().to_os_string();
                name.push(format!(".old-{secs}"));
                let old_path = path.with_file_name(name);
                std::fs::rename(path, &old_path)?;
                kept = Some(old_path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
            }
            Err(e) => return Err(e),
        }
        write_private(path, &self.key.to_bytes())?;
        Ok(kept)
    }

    /// The player's permanent ID: their public key, as text.
    pub fn player_id(&self) -> String {
        self.key.public().to_string()
    }

    /// `message` signed with this identity's secret key, as hex, for
    /// anyone with the player id to check.
    pub fn sign(&self, message: &[u8]) -> String {
        data_encoding::HEXLOWER.encode(&self.key.sign(message).to_bytes())
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Identity").field("player_id", &self.player_id()).finish_non_exhaustive()
    }
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    file.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

// ---------------------------------------------------------------------------
// Join codes
// ---------------------------------------------------------------------------

/// Whether `text` looks like a join code rather than an address.
pub fn is_join_code(text: &str) -> bool {
    text.trim().to_ascii_lowercase().starts_with(CODE_PREFIX)
}

/// The join code for a host reachable at `addr`.
fn encode_code(addr: &EndpointAddr) -> String {
    let json = serde_json::to_vec(addr).expect("an endpoint address always serializes");
    format!("{CODE_PREFIX}{}", BASE32_NOPAD.encode(&json).to_ascii_lowercase())
}

/// The host a join code points at.
fn decode_code(code: &str) -> Result<EndpointAddr, String> {
    let code = code.trim().to_ascii_lowercase();
    let body = code.strip_prefix(CODE_PREFIX).ok_or("that is not a join code")?;
    let body: String = body.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    let bytes = BASE32_NOPAD
        .decode(body.to_ascii_uppercase().as_bytes())
        .map_err(|_| "that join code is damaged; copy it again".to_owned())?;
    serde_json::from_slice(&bytes).map_err(|_| "that join code is damaged; copy it again".to_owned())
}

// ---------------------------------------------------------------------------
// Networks
// ---------------------------------------------------------------------------

/// Which network a tunnel uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Network {
    /// The internet: direct connections where possible, iroh's public relays
    /// and address lookup otherwise.
    #[default]
    Internet,
    /// Only direct connections, with no relay or lookup servers. For tests
    /// and games on one home network.
    LocalOnly,
}

async fn bind(identity: &Identity, network: Network, alpns: Vec<Vec<u8>>) -> Result<Endpoint, String> {
    let builder = match network {
        Network::Internet => Endpoint::builder(presets::N0),
        Network::LocalOnly => Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Disabled),
    };
    builder
        .secret_key(identity.key.clone())
        .alpns(alpns)
        .bind()
        .await
        .map_err(|e| format!("could not start networking: {e}"))
}

/// Shared between a tunnel and its background thread.
#[derive(Default)]
struct Status {
    code: Mutex<Option<String>>,
    error: Mutex<Option<String>>,
    connections: AtomicUsize,
    /// The endpoint, once bound, so stopping can close it politely and the
    /// other side hears about it at once.
    endpoint: Mutex<Option<Endpoint>>,
}

impl Status {
    fn fail(&self, error: impl Into<String>) {
        *self.error.lock().unwrap() = Some(error.into());
    }
}

/// Runs `task` on its own small runtime in a background thread until it
/// ends or `stop` fires.
fn spawn<F>(name: &str, status: Arc<Status>, task: F) -> Result<oneshot::Sender<()>, String>
where
    F: Future<Output = ()> + Send + 'static,
{
    let (stop, stopped) = oneshot::channel::<()>();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name(name)
        .enable_all()
        .build()
        .map_err(|e| format!("could not start networking: {e}"))?;
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            runtime.block_on(async move {
                tokio::select! {
                    () = task => {}
                    _ = stopped => {}
                }
                let endpoint = status.endpoint.lock().unwrap().take();
                if let Some(endpoint) = endpoint {
                    let _ = tokio::time::timeout(Duration::from_secs(1), endpoint.close()).await;
                }
            });
            // Dropping the runtime cancels every connection still open.
            runtime.shutdown_timeout(Duration::from_millis(500));
        })
        .map_err(|e| format!("could not start networking: {e}"))?;
    Ok(stop)
}

/// Copies bytes both ways between a TCP connection and an iroh stream until
/// both sides have finished, passing each end-of-stream along so the game on
/// either side sees the other leave.
async fn pipe(tcp: TcpStream, connection: Connection, opened: bool) {
    use tokio::io::AsyncWriteExt;
    let streams = if opened { connection.open_bi().await } else { connection.accept_bi().await };
    let Ok((mut send, mut recv)) = streams else { return };
    let _ = tcp.set_nodelay(true);
    let (mut tcp_read, mut tcp_write) = tcp.into_split();
    let up = async {
        let _ = tokio::io::copy(&mut tcp_read, &mut send).await;
        let _ = send.finish();
    };
    let down = async {
        let _ = tokio::io::copy(&mut recv, &mut tcp_write).await;
        let _ = tcp_write.shutdown().await;
    };
    tokio::join!(up, down);
    if opened {
        // Everything the host sent has arrived, so the joiner can hang up.
        connection.close(0u32.into(), b"bye");
    } else {
        // Give the joiner a moment to read the last bytes, then hang up.
        let _ = tokio::time::timeout(Duration::from_secs(5), connection.closed()).await;
    }
}

// ---------------------------------------------------------------------------
// Hosting
// ---------------------------------------------------------------------------

/// Lets players with the join code reach a game hosted on `game_port`.
/// Dropping it stops new and current players from reaching the game.
pub struct HostTunnel {
    status: Arc<Status>,
    _stop: oneshot::Sender<()>,
}

impl HostTunnel {
    /// Starts handing out a join code for the game on local `game_port`.
    /// The code appears in [`code`](Self::code) within a few seconds.
    pub fn start(identity: &Identity, network: Network, game_port: u16) -> Result<Self, String> {
        let status = Arc::new(Status::default());
        let identity = identity.clone();
        let shared = status.clone();
        let stop = spawn("libroxel-host-net", status.clone(), async move {
            let endpoint = match bind(&identity, network, vec![ALPN.to_vec(), PING_ALPN.to_vec()]).await {
                Ok(endpoint) => endpoint,
                Err(e) => return shared.fail(e),
            };
            *shared.endpoint.lock().unwrap() = Some(endpoint.clone());
            if network == Network::Internet {
                // Wait for a relay so the code works from anywhere; a code
                // with only direct addresses still works nearby.
                let _ = tokio::time::timeout(ONLINE_WAIT, endpoint.online()).await;
            }
            *shared.code.lock().unwrap() = Some(encode_code(&endpoint.addr()));
            while let Some(incoming) = endpoint.accept().await {
                let shared = shared.clone();
                tokio::spawn(async move {
                    let Ok(connection) = incoming.await else { return };
                    if connection.alpn() == PING_ALPN {
                        return answer_pings(connection).await;
                    }
                    let Ok(tcp) = TcpStream::connect((Ipv4Addr::LOCALHOST, game_port)).await else {
                        connection.close(1u32.into(), b"game not running");
                        return;
                    };
                    shared.connections.fetch_add(1, Ordering::Relaxed);
                    pipe(tcp, connection, false).await;
                    shared.connections.fetch_sub(1, Ordering::Relaxed);
                });
            }
        })?;
        Ok(Self { status, _stop: stop })
    }

    /// The code others paste into Join, once networking is ready.
    pub fn code(&self) -> Option<String> {
        self.status.code.lock().unwrap().clone()
    }

    /// Why hosting over the internet stopped working, if it did.
    pub fn error(&self) -> Option<String> {
        self.status.error.lock().unwrap().clone()
    }

    /// How many players are connected through the tunnel right now.
    pub fn connections(&self) -> usize {
        self.status.connections.load(Ordering::Relaxed)
    }
}

/// Echoes each ping stream back until the pinger hangs up.
async fn answer_pings(connection: Connection) {
    while let Ok(Ok((mut send, mut recv))) =
        tokio::time::timeout(Duration::from_secs(10), connection.accept_bi()).await
    {
        tokio::spawn(async move {
            if let Ok(bytes) = recv.read_to_end(64).await {
                let _ = send.write_all(&bytes).await;
                let _ = send.finish();
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Pinging
// ---------------------------------------------------------------------------

/// How long a ping may take before the host counts as unreachable.
const PING_TIMEOUT: Duration = Duration::from_secs(8);

/// What a ping found.
pub type PingResult = Result<Duration, String>;

/// Times round trips to hosts from a background thread, like a server
/// browser does. Ask with [`ping`](Self::ping); the answer shows up in
/// [`result`](Self::result) a moment later.
pub struct Pinger {
    asks: tokio::sync::mpsc::UnboundedSender<String>,
    results: Arc<Mutex<std::collections::HashMap<String, PingResult>>>,
    _stop: oneshot::Sender<()>,
}

impl Pinger {
    /// Starts pinging with a throwaway identity.
    pub fn start(network: Network) -> Result<Self, String> {
        let (asks, mut asked) = tokio::sync::mpsc::unbounded_channel::<String>();
        let results: Arc<Mutex<std::collections::HashMap<String, PingResult>>> = Arc::default();
        let shared = results.clone();
        let status = Arc::new(Status::default());
        let keep = status.clone();
        let stop = spawn("libroxel-ping", status, async move {
            let endpoint = match bind(&Identity::generate(), network, Vec::new()).await {
                Ok(endpoint) => endpoint,
                Err(e) => return keep.fail(e),
            };
            *keep.endpoint.lock().unwrap() = Some(endpoint.clone());
            while let Some(code) = asked.recv().await {
                let (endpoint, shared) = (endpoint.clone(), shared.clone());
                tokio::spawn(async move {
                    let result = match tokio::time::timeout(PING_TIMEOUT, ping_once(&endpoint, &code)).await {
                        Ok(result) => result,
                        Err(_) => Err("no answer".to_owned()),
                    };
                    shared.lock().unwrap().insert(code, result);
                });
            }
        })?;
        Ok(Self { asks, results, _stop: stop })
    }

    /// Pings the host behind `code` (again, if it was pinged before).
    pub fn ping(&self, code: &str) {
        let _ = self.asks.send(code.trim().to_owned());
    }

    /// The last ping to `code`'s host, once it has come back.
    pub fn result(&self, code: &str) -> Option<PingResult> {
        self.results.lock().unwrap().get(code.trim()).cloned()
    }
}

/// Connects to a host and returns the fastest of a few round trips.
async fn ping_once(endpoint: &Endpoint, code: &str) -> PingResult {
    let host = decode_code(code)?;
    let connection =
        endpoint.connect(host, PING_ALPN).await.map_err(|e| format!("could not reach it: {e}"))?;
    let mut best = None::<Duration>;
    for i in 0..3u8 {
        let started = std::time::Instant::now();
        let (mut send, mut recv) = connection.open_bi().await.map_err(|e| e.to_string())?;
        send.write_all(&[i; 8]).await.map_err(|e| e.to_string())?;
        let _ = send.finish();
        let back = recv.read_to_end(64).await.map_err(|e| e.to_string())?;
        if back != [i; 8] {
            return Err("the host answered oddly".into());
        }
        let took = started.elapsed();
        best = Some(best.map_or(took, |b| b.min(took)));
    }
    connection.close(0u32.into(), b"pinged");
    best.ok_or_else(|| "no answer".into())
}

// ---------------------------------------------------------------------------
// Joining
// ---------------------------------------------------------------------------

/// A local address that leads to the host a join code points at.
/// Dropping it ends the connection.
pub struct JoinTunnel {
    local: SocketAddr,
    status: Arc<Status>,
    _stop: oneshot::Sender<()>,
}

impl JoinTunnel {
    /// Starts connecting to the host behind `code`. Connect to
    /// [`local_addr`](Self::local_addr) straight away: what is sent waits
    /// until the host is reached.
    pub fn start(identity: &Identity, network: Network, code: &str) -> Result<Self, String> {
        let host = decode_code(code)?;
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|e| format!("could not start networking: {e}"))?;
        let local = listener.local_addr().map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let status = Arc::new(Status::default());
        let identity = identity.clone();
        let shared = status.clone();
        let stop = spawn("libroxel-join-net", status.clone(), async move {
            let Ok(listener) = TcpListener::from_std(listener) else {
                return shared.fail("could not start networking");
            };
            // The game connects once, so one TCP connection is all we serve.
            let Ok((tcp, _)) = listener.accept().await else { return };
            drop(listener);
            let endpoint = match bind(&identity, network, Vec::new()).await {
                Ok(endpoint) => endpoint,
                Err(e) => return shared.fail(e),
            };
            *shared.endpoint.lock().unwrap() = Some(endpoint.clone());
            let connection = match endpoint.connect(host, ALPN).await {
                Ok(connection) => connection,
                Err(e) => return shared.fail(format!("could not reach the host: {e}")),
            };
            shared.connections.store(1, Ordering::Relaxed);
            pipe(tcp, connection, true).await;
            shared.connections.store(0, Ordering::Relaxed);
            endpoint.close().await;
        })?;
        Ok(Self { local, status, _stop: stop })
    }

    /// Where `PlaySession::join` should connect.
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// Why the host could not be reached, if it could not.
    pub fn error(&self) -> Option<String> {
        self.status.error.lock().unwrap().clone()
    }

    /// Whether the host has been reached and the connection is open.
    pub fn connected(&self) -> bool {
        self.status.connections.load(Ordering::Relaxed) > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        let id = Identity::generate();
        let addr = EndpointAddr::new(id.key.public());
        let code = encode_code(&addr);
        assert!(is_join_code(&code));
        assert!(is_join_code(&format!("  {}  ", code.to_ascii_uppercase())));
        assert!(!is_join_code("192.168.1.20"));
        assert_eq!(decode_code(&code).unwrap(), addr);
        assert!(decode_code("lrx1notacode!").is_err());
    }

    #[test]
    fn hosts_answer_pings() {
        // A host with nothing behind it still answers pings.
        let host = HostTunnel::start(&Identity::generate(), Network::LocalOnly, 1).unwrap();
        let started = std::time::Instant::now();
        let code = loop {
            if let Some(code) = host.code() {
                break code;
            }
            assert!(started.elapsed() < Duration::from_secs(10), "no code");
            std::thread::sleep(Duration::from_millis(20));
        };
        let pinger = Pinger::start(Network::LocalOnly).unwrap();
        pinger.ping(&code);
        let result = loop {
            if let Some(result) = pinger.result(&code) {
                break result;
            }
            assert!(started.elapsed() < Duration::from_secs(15), "no answer");
            std::thread::sleep(Duration::from_millis(20));
        };
        let took = result.unwrap();
        assert!(took < Duration::from_secs(1), "{took:?}");
        // Nobody was counted as a player.
        assert_eq!(host.connections(), 0);
        pinger.ping("lrx1notacode");
        std::thread::sleep(Duration::from_millis(300));
        assert!(pinger.result("lrx1notacode").unwrap().is_err());
    }

    #[test]
    fn identity_is_kept() {
        let dir = std::env::temp_dir().join(format!("libroxel-net-test-{}", std::process::id()));
        let path = dir.join("identity.key");
        let _ = std::fs::remove_file(&path);
        let first = Identity::load_or_create(&path).unwrap();
        let again = Identity::load_or_create(&path).unwrap();
        assert_eq!(first.player_id(), again.player_id());
        std::fs::write(&path, b"short").unwrap();
        assert!(Identity::load_or_create(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
