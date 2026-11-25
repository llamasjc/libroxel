//! Rate limits, so one computer can't flood a Hub, grab every good name or
//! guess passwords.
//!
//! Each limit counts requests in a fixed window per key (usually the
//! caller's IP address, or an account name for failed sign-ins).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// What is being counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Limit {
    /// Any request at all, per IP.
    Any,
    /// New accounts made, per IP.
    NewAccount,
    /// Sign-in attempts (and the salt lookups before them), per IP.
    SignIn,
    /// Sign-ins that got the password wrong, per account name and IP (see
    /// [`Limits::wrong_password`]).
    WrongPassword,
    /// The same per account name from everyone together: much higher, so a
    /// stranger can't lock someone out by guessing wrong on purpose.
    WrongPasswordAnyone,
    /// Games published and items shared, per IP.
    Share,
    /// Reports, per IP.
    Report,
    Comment,
    /// Emails given (each sends a code), per IP.
    Email,
    Asset,
    /// Messages sent, per IP and per sender.
    Message,
    Download,
    AssetGet,
    Picture,
}

impl Limit {
    /// How many in how long.
    fn rule(self) -> (u32, Duration) {
        const MINUTE: Duration = Duration::from_secs(60);
        match self {
            // A busy player polls friends, presence and lists every few
            // seconds; a whole LAN party behind one address stays well under.
            Limit::Any => (1200, MINUTE),
            Limit::NewAccount => (5, 60 * MINUTE),
            Limit::SignIn => (30, 15 * MINUTE),
            Limit::WrongPassword => (10, 15 * MINUTE),
            Limit::WrongPasswordAnyone => (200, 15 * MINUTE),
            Limit::Share => (30, 60 * MINUTE),
            Limit::Report => (30, 60 * MINUTE),
            Limit::Comment => (20, 10 * MINUTE),
            Limit::Email => (10, 60 * MINUTE),
            Limit::Asset => (2000, 60 * MINUTE),
            Limit::Message => (30, MINUTE),
            Limit::Download => (200, 60 * MINUTE),
            Limit::AssetGet => (20_000, 60 * MINUTE),
            Limit::Picture => (600, 10 * MINUTE),
        }
    }
}

pub(crate) struct Window {
    start: Instant,
    count: u32,
    noted: bool,
}

/// Every count the Hub keeps.
#[derive(Default)]
pub(crate) struct Limits {
    pub(crate) windows: Mutex<HashMap<(Limit, String), Window>>,
    /// Whether a web server on this computer passes callers on, saying who
    /// they are in `X-Forwarded-For`.
    pub(crate) trust_proxy: bool,
    /// Every limit switched off (see [`crate::HubOptions::no_limits`]).
    pub(crate) off: bool,
}

impl Limits {
    /// Counts one more for `who`, and whether that is still allowed.
    pub(crate) fn take(&self, limit: Limit, who: &str) -> bool {
        if self.off {
            return true;
        }
        let (most, every) = limit.rule();
        let now = Instant::now();
        let mut windows = self.windows.lock().unwrap();
        if windows.len() > 50_000 {
            windows.retain(|(limit, _), w| now.duration_since(w.start) < limit.rule().1);
        }
        let window = windows.entry((limit, who.to_ascii_lowercase())).or_insert(Window {
            start: now,
            count: 0,
            noted: false,
        });
        if now.duration_since(window.start) >= every {
            *window = Window { start: now, count: 0, noted: false };
        }
        if window.count >= most {
            return false;
        }
        window.count += 1;
        true
    }

    pub(crate) fn take_logged(&self, limit: Limit, who: &str) -> bool {
        if self.take(limit, who) {
            return true;
        }
        let mut windows = self.windows.lock().unwrap();
        if let Some(window) = windows.get_mut(&(limit, who.to_ascii_lowercase()))
            && !window.noted
        {
            window.noted = true;
            eprintln!("{who} hit the {limit:?} limit");
        }
        false
    }

    /// Whether `who` is over `limit` right now, without counting anything.
    /// Counts a wrong password for the account `name` from `caller`.
    pub(crate) fn wrong_password(&self, name: &str, caller: &str) {
        self.take(Limit::WrongPassword, &format!("{name}\n{caller}"));
        self.take(Limit::WrongPasswordAnyone, name);
    }

    /// Whether `caller` has guessed wrong too often for `name`, or everyone
    /// together has.
    pub(crate) fn too_many_wrong(&self, name: &str, caller: &str) -> bool {
        self.over(Limit::WrongPassword, &format!("{name}\n{caller}"))
            || self.over(Limit::WrongPasswordAnyone, name)
    }

    pub(crate) fn over(&self, limit: Limit, who: &str) -> bool {
        if self.off {
            return false;
        }
        let (most, every) = limit.rule();
        let windows = self.windows.lock().unwrap();
        windows
            .get(&(limit, who.to_ascii_lowercase()))
            .is_some_and(|w| w.start.elapsed() < every && w.count >= most)
    }
}

/// Who is calling, as the Hub saw the connection.
#[derive(Debug, Clone)]
pub(crate) struct Caller(pub String);

impl Caller {
    pub(crate) fn local(&self) -> bool {
        self.0.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
    }
}

/// The answer when a limit is reached.
pub(crate) fn too_many(what: &str) -> (StatusCode, String) {
    (StatusCode::TOO_MANY_REQUESTS, format!("too many {what} from here; wait a while and try again"))
}

/// Who is calling, from the connection's address and, when `trust_proxy`,
/// the `X-Forwarded-For` a web server on this computer adds.
fn caller_ip(peer: Option<std::net::IpAddr>, forwarded: Option<&str>, trust_proxy: bool) -> String {
    match peer {
        // Only a web server on this computer is believed, and only the
        // address it added itself (the last one): callers can put anything
        // before it.
        Some(peer) if trust_proxy && peer.is_loopback() => forwarded
            .and_then(|v| v.rsplit(',').next())
            .and_then(|v| v.trim().parse::<std::net::IpAddr>().ok())
            .unwrap_or(peer)
            .to_string(),
        Some(peer) => peer.to_string(),
        None => "unknown".to_owned(),
    }
}

/// Notes who is calling and turns them away past [`Limit::Any`].
pub(crate) async fn every_request(
    State(limits): State<Arc<Limits>>,
    mut request: Request,
    next: Next,
) -> Response {
    let peer = request.extensions().get::<ConnectInfo<SocketAddr>>().map(|info| info.0.ip());
    let forwarded = request.headers().get("x-forwarded-for").and_then(|v| v.to_str().ok());
    let ip = caller_ip(peer, forwarded, limits.trust_proxy);
    if !limits.take(Limit::Any, &ip) {
        return too_many("requests").into_response();
    }
    request.extensions_mut().insert(Caller(ip));
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_limit_stops_at_its_count_for_that_caller_only() {
        let limits = Limits::default();
        for _ in 0..5 {
            assert!(limits.take(Limit::NewAccount, "1.2.3.4"));
        }
        assert!(!limits.take(Limit::NewAccount, "1.2.3.4"));
        assert!(limits.over(Limit::NewAccount, "1.2.3.4"));
        assert!(limits.take(Limit::NewAccount, "5.6.7.8"));
        // Names are counted whatever their case, and per caller: someone
        // guessing wrong doesn't lock Alice out where she is.
        for _ in 0..10 {
            limits.wrong_password("Alice2", "6.6.6.6");
        }
        assert!(limits.too_many_wrong("alice2", "6.6.6.6"));
        assert!(!limits.too_many_wrong("alice2", "1.2.3.4"));
        // But everyone together can't guess for ever either.
        for i in 0..200 {
            limits.wrong_password("Alice2", &format!("10.0.{}.{}", i / 250, i % 250));
        }
        assert!(limits.too_many_wrong("alice2", "1.2.3.4"));
        for _ in 0..200 {
            assert!(limits.take_logged(Limit::Download, "9.9.9.9"));
        }
        assert!(!limits.take_logged(Limit::Download, "9.9.9.9"));
        assert!(!limits.take_logged(Limit::Download, "9.9.9.9"));
    }

    #[test]
    fn only_a_web_server_on_this_computer_is_believed_about_who_calls() {
        let local = Some("127.0.0.1".parse().unwrap());
        let far = Some("203.0.113.9".parse().unwrap());
        // Behind Caddy: the address Caddy added (the last one), not what the caller made up.
        assert_eq!(caller_ip(local, Some("1.1.1.1, 198.51.100.7"), true), "198.51.100.7");
        // Not told to trust a proxy, or the header came from far away: the connection counts.
        assert_eq!(caller_ip(local, Some("198.51.100.7"), false), "127.0.0.1");
        assert_eq!(caller_ip(far, Some("198.51.100.7"), true), "203.0.113.9");
        assert_eq!(caller_ip(local, Some("nonsense"), true), "127.0.0.1");
    }
}
