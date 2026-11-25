//! A pretend network between a joining computer and a host, for testing
//! games over connections that are slow, uneven, lossy or that go away.
//!
//! [`Proxy`] sits between the two on this machine and passes the bytes
//! along as a real network would with the [`Weather`] it is given: each
//! piece arrives after a delay (plus jitter), no faster than the bandwidth
//! allows, and in order, since games go over TCP and TCP puts pieces back
//! in order before the game sees them. A lost packet therefore shows up as
//! a stall: everything behind it waits for it to be sent again. Pieces
//! are cut at random sizes, so messages arrive split anywhere.
//!
//! The network can also stall for a while ([`Proxy::hold`]), go silent
//! for good without saying so, as Wi-Fi that drops does
//! ([`Proxy::vanish`]), or hang up at once ([`Proxy::cut`]).

#![allow(dead_code)]

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How a connection behaves, the same both ways.
#[derive(Debug, Clone, Copy)]
pub struct Weather {
    /// One way, before jitter.
    pub latency: Duration,
    /// Up to this much more, at random, for each piece.
    pub jitter: Duration,
    /// The chance a piece is lost and has to be sent again, holding up
    /// everything behind it by `resend`.
    pub loss: f64,
    pub resend: Duration,
    /// Bytes a second, or none for as fast as this machine goes.
    pub bandwidth: Option<u64>,
    /// The largest piece; each is cut at a random size up to this.
    pub largest_piece: usize,
}

impl Weather {
    /// Straight through.
    pub const CLEAR: Self = Self {
        latency: Duration::ZERO,
        jitter: Duration::ZERO,
        loss: 0.0,
        resend: Duration::ZERO,
        bandwidth: None,
        largest_piece: 64 * 1024,
    };

    /// A poor connection: across the world on busy Wi-Fi.
    pub const STORMY: Self = Self {
        latency: Duration::from_millis(120),
        jitter: Duration::from_millis(60),
        loss: 0.03,
        resend: Duration::from_millis(250),
        bandwidth: Some(2_000_000),
        largest_piece: 1400,
    };

    /// A phone on a weak signal.
    pub const AWFUL: Self = Self {
        latency: Duration::from_millis(250),
        jitter: Duration::from_millis(150),
        loss: 0.08,
        resend: Duration::from_millis(400),
        bandwidth: Some(400_000),
        largest_piece: 600,
    };
}

#[derive(Default)]
struct Control {
    /// Pieces wait until then, whatever the weather.
    held_until: Mutex<Option<Instant>>,
    /// Nothing more gets through, and nobody is told.
    vanished: AtomicBool,
    /// Every connection is hung up.
    cut: AtomicBool,
    bytes_up: AtomicU64,
    bytes_down: AtomicU64,
}

/// A pretend network in front of a host.
pub struct Proxy {
    address: SocketAddr,
    control: Arc<Control>,
    weather: Arc<Mutex<Weather>>,
}

impl Proxy {
    /// Starts passing connections made to [`address`](Self::address) on to `host`.
    pub fn start(host: SocketAddr, weather: Weather, seed: u64) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
        let address = listener.local_addr().unwrap();
        let control = Arc::new(Control::default());
        let weather = Arc::new(Mutex::new(weather));
        let (c, w) = (control.clone(), weather.clone());
        std::thread::spawn(move || {
            let mut seed = seed;
            for guest in listener.incoming() {
                let Ok(guest) = guest else { continue };
                if c.cut.load(Ordering::Relaxed) {
                    continue;
                }
                let Ok(upstream) = TcpStream::connect(host) else { continue };
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                pipe(
                    guest.try_clone().unwrap(),
                    upstream.try_clone().unwrap(),
                    c.clone(),
                    w.clone(),
                    seed,
                    true,
                );
                pipe(upstream, guest, c.clone(), w.clone(), seed ^ 0x9e37_79b9, false);
            }
        });
        Self { address, control, weather }
    }

    /// Where joining computers connect, as `127.0.0.1:port`.
    pub fn address(&self) -> String {
        self.address.to_string()
    }

    /// Changes the weather from now on.
    pub fn set_weather(&self, weather: Weather) {
        *self.weather.lock().unwrap() = weather;
    }

    /// Nothing gets through for `how_long` (a lag spike); then everything
    /// held arrives, in order.
    pub fn hold(&self, how_long: Duration) {
        *self.control.held_until.lock().unwrap() = Some(Instant::now() + how_long);
    }

    /// The network goes quiet for good: nothing more gets through either
    /// way, and neither side is told.
    pub fn vanish(&self) {
        self.control.vanished.store(true, Ordering::Relaxed);
    }

    /// Hangs up every connection at once, and takes no more.
    pub fn cut(&self) {
        self.control.cut.store(true, Ordering::Relaxed);
    }

    /// Lets connections through again after [`cut`](Self::cut) or
    /// [`vanish`](Self::vanish) (new ones; the old ones stay gone).
    pub fn heal(&self) {
        self.control.cut.store(false, Ordering::Relaxed);
        self.control.vanished.store(false, Ordering::Relaxed);
        *self.control.held_until.lock().unwrap() = None;
    }

    /// Bytes passed from joining computers to the host, and back.
    pub fn bytes(&self) -> (u64, u64) {
        (self.control.bytes_up.load(Ordering::Relaxed), self.control.bytes_down.load(Ordering::Relaxed))
    }
}

/// A small, repeatable random number source.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Passes bytes from `from` to `to` in the weather, on a thread of its own.
fn pipe(
    mut from: TcpStream,
    mut to: TcpStream,
    control: Arc<Control>,
    weather: Arc<Mutex<Weather>>,
    seed: u64,
    up: bool,
) {
    std::thread::spawn(move || {
        let mut dice = Dice(seed | 1);
        from.set_read_timeout(Some(Duration::from_millis(1))).unwrap();
        let mut waiting: VecDeque<(Instant, Vec<u8>)> = VecDeque::new();
        // When the last piece is due: pieces never overtake each other.
        let mut last_due = Instant::now();
        let mut buf = vec![0u8; 64 * 1024];
        let mut open = true;
        loop {
            if control.cut.load(Ordering::Relaxed) {
                let _ = from.shutdown(Shutdown::Both);
                let _ = to.shutdown(Shutdown::Both);
                return;
            }
            if open {
                match from.read(&mut buf) {
                    Ok(0) => open = false,
                    Ok(n) => {
                        let w = *weather.lock().unwrap();
                        let now = Instant::now();
                        let mut rest = &buf[..n];
                        while !rest.is_empty() {
                            let size = 1 + dice.below(w.largest_piece.min(rest.len()));
                            let (piece, after) = rest.split_at(size);
                            rest = after;
                            let jitter = w.jitter.mul_f64(dice.unit());
                            let mut due = now + w.latency + jitter;
                            if dice.unit() < w.loss {
                                due += w.resend;
                            }
                            if let Some(bandwidth) = w.bandwidth {
                                due = due
                                    .max(last_due + Duration::from_secs_f64(size as f64 / bandwidth as f64));
                            }
                            due = due.max(last_due);
                            last_due = due;
                            waiting.push_back((due, piece.to_vec()));
                        }
                    }
                    Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(_) => open = false,
                }
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
            if control.vanished.load(Ordering::Relaxed) {
                // Gone quiet: what was on the way is lost, and nobody hangs up.
                waiting.clear();
                continue;
            }
            let held = control.held_until.lock().unwrap().is_some_and(|until| Instant::now() < until);
            if held {
                continue;
            }
            let now = Instant::now();
            while waiting.front().is_some_and(|(due, _)| *due <= now) {
                let (_, piece) = waiting.pop_front().unwrap();
                if to.write_all(&piece).is_err() {
                    let _ = from.shutdown(Shutdown::Both);
                    return;
                }
                let counter = if up { &control.bytes_up } else { &control.bytes_down };
                counter.fetch_add(piece.len() as u64, Ordering::Relaxed);
            }
            if !open && waiting.is_empty() {
                let _ = to.shutdown(Shutdown::Write);
                return;
            }
        }
    });
}
