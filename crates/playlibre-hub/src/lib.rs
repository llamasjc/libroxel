//! The PlayLibre Hub: one small server that lists the games being played
//! right now, so players can find one and join it with its join code.
//!
//! The Hub never runs a game or carries its traffic: hosts announce
//! themselves every few seconds with a [`Heartbeat`], and a host that stops
//! announcing drops off the list. There is one Hub, run on a server with
//! the `playlibre-hub` program; it keeps its data in one SQLite file.
//!
//! A Hub also keeps **published** games: whole saved places that players
//! can play with no one needing to be online. On a Hub that runs its own
//! game servers, only a game's creator and those servers get its place. Whoever publishes a game gets a secret back;
//! publishing again with the same id and secret puts out a new version.
//!
//! Players can also claim an **account**: a name (unique on that Hub), a
//! few words about themselves and how their avatar looks, tied to their
//! player id (the public half of the identity key on their computer).
//! Every change is signed with that key, so no one else can change it, and
//! games published with a signature show their author's account name.
//! Setting a **password** keeps a sealed copy of that key on the Hub, so the
//! player can sign in on another computer (see [`seal_password`]); the
//! password itself never leaves their computer.
//!
//! Hosts can sign their listing too, so the server list shows which
//! account really hosts each server. A Hub limits how fast any one address
//! can make accounts, guess passwords, publish or report, and keeps names
//! that look official, or that someone gave up recently, from being taken.
//!
//! Friends and **presence** live here too: players send and accept friend
//! requests (each one signed), and the app tells the Hub every few seconds
//! whether its player is online and which server they are in, so their
//! friends (and only their friends) can see it and join them. Servers say
//! which game they run, so each game has its own server list.
//!
//! [`HubClient`] is the blocking client the editor and servers use.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::{Extension, FromRef, Path as UrlPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use rusqlite::{Connection, OptionalExtension, params};
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};

mod analytics;
pub use analytics::{Creator, DayStats, GameStats, KEEP_DAYS, creator_message, creator_stats, record};
mod assets;
mod charts;
pub use assets::{
    ASSET_PREFIX, MAX_ASSET, OfficialGame, StarterPlace, asset_hash, official, official_from, set_official,
    valid_hash,
};
pub use charts::{Chart, ChartStats, Charts, ForYou, Played, for_you_message, guess_genre, wilson};
mod badges;
mod cli;
pub use badges::{
    Awarded, BADGE_COLORS, BADGE_CREDITS, BADGE_ICONS, Badge, BadgeAction, BadgeAward, BadgeChange,
    MAX_BADGE_ABOUT, MAX_BADGE_NAME, MAX_BADGES, PLATFORM_BADGES, PlatformBadge, award_message,
    badge_message, platform_badge,
};
mod copies;
mod earnings;
pub use earnings::{
    BuyPass, CREATOR_SHARE, CashOut, EarnedDay, Earnings, EarningsSettings, GameEarnings, HOLD_DAYS,
    PAYOUT_AGE, Pass, PassAction, PassChange, Payout, SaleLine, buy_pass_message, cash_out_message,
    earnings_message, finish_payout, pass_message, payouts, set_settings as set_earnings_settings,
    settings as earnings_settings, settle,
};
mod email;
mod family;
pub use family::{
    Birthday, CONSENT_AGE, Chat, ChatRule, Consent, Family, FamilyChange, FamilySettings, Messages,
    ParentEmail, age_on, birthday_message, chat_level, consent_message, family_message, family_read_message,
    may_message, may_spend, parent_email_message, parse_date, playtime_up, strict_filter,
};
mod filter;
mod fleet;
pub use fleet::{FleetOptions, PlayAnswer, PlayRequest, play_message};
mod groups;
pub use groups::{
    GROUP_COLORS, GroupAction, GroupBadge, GroupChange, GroupChanged, GroupInfo, GroupJoin, GroupMember,
    GroupPage, GroupPost, GroupRole, MAX_TAG, group_color, group_invites_message, group_message,
    group_view_message, valid_group_name, valid_group_tag,
};
mod limits;
mod manage;
mod market;
pub use manage::{Change, KEEP_VERSIONS, MOST_PLAYERS, ManageGame, Version, manage_message};
pub use market::{
    Asset, AssetData, AssetKind, MAX_MODEL, MAX_PLUGIN, NewAsset, PERMISSIONS, Unmarket, asset_message,
    unmarket_message,
};
mod messages;
mod moderate;
pub use cli::{HUB_HELP, cli};
pub use messages::{
    Conversation, MAX_MESSAGE, Message, Moderation, NewMessage, message_message, read_messages_message,
};
pub use moderate::{
    Appeal, AppealAsk, ChatLine, Dashboard, HatMaker, LogEntry, MAX_APPEAL, ModAction, ModChange, OpenAppeal,
    Penalty, Queued, ReportReason, Standing, Warning, WarningsSeen, appeal_message, moderate_message,
    moderation_message, warnings_seen_message,
};
mod owner;
mod parties;
pub use parties::{
    PARTY_SIZE, Party, PartyAction, PartyChange, PartyInvite, PartyMember, PartyView, party_message,
    party_view_message,
};
mod password;
mod pictures;
mod ratings;
mod recovery;
pub use recovery::{
    RECOVERY_CODES, RecoveryAsk, RecoveryCodes, ResetAsk, ResetFinish, SIGNED_OUT, Session, SessionInfo,
    SessionStart, SignOutEverywhere, recovery_message, session_message, sign_out_message,
};
pub mod scrub;
mod seal;
mod security;
mod social;
mod stats;
mod wallet;
use limits::{Caller, Limit, Limits, too_many};
pub use owner::{
    AccountRow, accounts, add_owner, ban, ban_until, hat_makers, owners, remove_owner, rename, set_hat_maker,
    unban,
};
pub use password::{
    MIN_PASSWORD, PASSWORD_MEMORY_KIB, PASSWORD_PASSES, PasswordSet, SignIn, SignInSalt, SignedIn,
    password_message, password_problem, seal_password, seal_password_with, sign_in_message, weak_password,
};
pub use ratings::{
    Comment, MAX_COMMENT, NewComment, Rate, Ratings, RemoveComment, Vote, comment_message, rate_message,
    remove_comment_message,
};
pub use security::{
    NEEDS_CODE, Security, SignInRecord, TwoStepSet, WORK_BITS, account_work, new_two_step_secret,
    security_message, set_verified, two_step_code, two_step_link, two_step_message,
};
pub use social::{
    Friend, FriendAction, FriendChange, FriendState, Friends, PresenceUpdate, WebPresence, friend_message,
    friends_list_message, presence_message, web_presence_message,
};
pub use stats::{Crash, CrashGroup, PING_EVERY_SECS, Ping, Share, Usage, UsageDay, admin_message};
pub use wallet::{
    Buy, CURRENCY, Entry, MAX_PRICE, Refused, Sale, SetPrice, Wallet, balance, buy, buy_message, credit,
    debit, owns, price_message, spent, top_up, wallet_message,
};

/// The port a Hub listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 7430;

/// The Hub: the one place every copy of the app keeps its accounts,
/// friends, published games and server lists. There is only one.
///
/// A fork sets its own here, or builds with `PLAYLIBRE_OFFICIAL_HUB=<url>`.
pub const OFFICIAL_HUB: &str = match option_env!("PLAYLIBRE_OFFICIAL_HUB") {
    Some(hub) if !hub.is_empty() => hub,
    _ => "https://hub.playlibre.net",
};

pub use email::{
    EmailCode, EmailSent, EmailSet, Mail, SmtpSettings, email_code_message, email_message, valid_email,
};

/// Where the app finds the Hub: [`OFFICIAL_HUB`], unless `PLAYLIBRE_HUB`
/// names another address (for trying a Hub out on one computer, such as
/// the demo Hub or tests).
pub fn hub_address() -> String {
    setting("PLAYLIBRE_HUB")
        .map(|a| a.trim().to_owned())
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| OFFICIAL_HUB.to_owned())
}

/// A `PLAYLIBRE_…` setting from the environment, or the same setting under
/// its name from before the rename (`LIBROXEL_…`), so a server or computer
/// set up back then keeps its settings. `None` when it is unset or not text.
pub fn setting(name: &str) -> Option<String> {
    std::env::var(name).ok().or_else(|| {
        let rest = name.strip_prefix("PLAYLIBRE_")?;
        std::env::var(format!("LIBROXEL_{rest}")).ok()
    })
}

/// How a Hub is run.
#[derive(Debug, Clone, Default)]
pub struct HubOptions {
    /// It sits behind a web server on this computer (such as Caddy, for
    /// HTTPS), so trust that server's `X-Forwarded-For` for who is calling.
    /// Without it, every caller would share one rate limit.
    pub behind_proxy: bool,
    /// No rate limits at all: for a demo or tests on one computer, where
    /// every pretend player calls from the same address. Never for a Hub
    /// others use.
    pub no_limits: bool,
    /// How it mails codes to prove emails ([`Mail::from_env`] for the
    /// `PLAYLIBRE_SMTP_*` settings).
    pub mail: Mail,
    /// Game servers it runs itself, so Play sends players to this Hub's
    /// computer (see [`FleetOptions`]). `None`: only players host.
    pub fleet: Option<FleetOptions>,
    /// The playlibre app to ask for the official games, if not the fleet's.
    pub starters: Option<PathBuf>,
    /// A moderation service to check messages with ([`Moderation::from_env`]).
    pub moderation: Option<Moderation>,
}

/// How often hosts should announce themselves.
pub const HEARTBEAT_EVERY: Duration = Duration::from_secs(15);

/// A host missing for this long is taken off the list.
const EXPIRES_AFTER_SECS: i64 = 45;

/// The longest name, game title or code the Hub keeps.
const MAX_TEXT: usize = 600;

/// The longest description of a published game.
const MAX_DESCRIPTION: usize = 2000;

/// The biggest saved place the Hub takes, in bytes.
pub const MAX_PLACE: usize = 100 * 1024 * 1024;

// ---------------------------------------------------------------------------
// What goes over the wire
// ---------------------------------------------------------------------------

/// What kind of computer hosts a game. The list shows community and player
/// hosts first; official servers are the last resort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HostKind {
    /// A player hosting from the editor while they play.
    #[default]
    Player,
    /// A windowless server someone runs for their community.
    Community,
    /// A server run by whoever runs this Hub.
    Official,
}

impl HostKind {
    fn rank(self) -> i64 {
        match self {
            HostKind::Community => 0,
            HostKind::Player => 1,
            HostKind::Official => 2,
        }
    }

    fn from_rank(rank: i64) -> Self {
        match rank {
            0 => HostKind::Community,
            2 => HostKind::Official,
            _ => HostKind::Player,
        }
    }

    /// A short word for lists.
    pub fn label(self) -> &'static str {
        match self {
            HostKind::Player => "Player",
            HostKind::Community => "Community",
            HostKind::Official => "Official",
        }
    }
}

/// A host announcing itself. The first heartbeat for a `server_id` claims
/// it; later ones must carry the same `secret`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Heartbeat {
    pub server_id: String,
    pub secret: String,
    /// The name shown in the list, such as "Alice's obby".
    pub name: String,
    /// The game's title.
    pub game: String,
    /// The join code players paste into Join.
    pub code: String,
    pub players: u32,
    /// 0 means no limit.
    pub max_players: u32,
    pub kind: HostKind,
    /// Which game it runs: a published game's id, or `starter:<name>` for
    /// a starter game. Servers that say put the game on its own list.
    #[serde(default)]
    pub game_id: Option<String>,
    /// The player hosting it, to show their account on the list.
    #[serde(default)]
    pub host_id: Option<String>,
    /// [`host_message`] signed with that player's identity key.
    #[serde(default)]
    pub host_signature: Option<String>,
}

/// What a host signs to show a listed server is theirs: its id and its
/// secret, which only they know.
pub fn host_message(server_id: &str, secret: &str) -> Vec<u8> {
    format!("libroxel host\n{server_id}\n{secret}").into_bytes()
}

/// The start of every join code (as `playlibre_net` makes them).
const CODE_PREFIX: &str = "lrx1";

/// The player id of the computer a join code reaches, if it is a join code.
/// A join code is the host's key and addresses, so this can't be faked: a
/// connection made with the code only reaches the computer with that key.
pub fn code_host(code: &str) -> Option<String> {
    let code = code.trim().to_ascii_lowercase();
    let body: String =
        code.strip_prefix(CODE_PREFIX)?.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    let bytes = data_encoding::BASE32_NOPAD.decode(body.to_ascii_uppercase().as_bytes()).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let id = json.get("id")?.as_str()?.parse::<iroh_base::PublicKey>().ok()?;
    Some(id.to_string())
}

/// A join code for `player_id` that has no addresses in it, for tests and
/// tools (a real code also says where the host is).
pub fn code_for(player_id: &str) -> String {
    let json = serde_json::json!({ "id": player_id, "addrs": [] });
    let body = data_encoding::BASE32_NOPAD.encode(json.to_string().as_bytes()).to_ascii_lowercase();
    format!("{CODE_PREFIX}{body}")
}

/// `beat` signed as hosted by the player whose key is `key`, with a join
/// code that reaches them when it has none yet.
pub fn signed_heartbeat(key: &[u8; 32], mut beat: Heartbeat) -> Heartbeat {
    let id = player_id_of(key);
    if beat.code.trim().is_empty() {
        beat.code = code_for(&id);
    }
    beat.host_signature = Some(sign(key, &host_message(&beat.server_id, &beat.secret)));
    beat.host_id = Some(id);
    beat
}

/// A live server as the list shows it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerInfo {
    pub server_id: String,
    pub name: String,
    pub game: String,
    pub code: String,
    pub players: u32,
    pub max_players: u32,
    pub kind: HostKind,
    /// Seconds since the host last announced itself.
    pub seen_secs_ago: i64,
    /// Which game it runs, if it said (see [`Heartbeat::game_id`]).
    #[serde(default)]
    pub game_id: Option<String>,
    /// The account name of whoever hosts it, when they signed the listing
    /// and have an account here. The server's own name is whatever the host
    /// typed; this is checked.
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub host_id: Option<String>,
    /// Whether one of the Hub's owners (its admins) hosts it.
    #[serde(default)]
    pub admin_host: bool,
    /// Whether whoever hosts it has a verified badge.
    #[serde(default)]
    pub host_verified: bool,
    /// Whether the Hub runs it on its own computer (see [`FleetOptions`]).
    #[serde(default)]
    pub hub_server: bool,
}

/// What `GET /` answers, so a client can check it reached a Hub.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HubStatus {
    pub hub: String,
    pub version: String,
    pub servers: u32,
    pub players: u32,
    /// Published games (older Hubs don't say).
    #[serde(default)]
    pub games: u32,
}

/// Publishing a game, or a new version of one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Publish {
    /// `None` for a new game; the game's id to put out a new version.
    pub game_id: Option<String>,
    /// Proves a new version comes from whoever published the game.
    pub secret: String,
    pub title: String,
    /// Who made it, as the list shows.
    pub author: String,
    pub description: String,
    /// The saved place (`.lrx` text).
    pub place: String,
    /// The publisher's player id, to show their account name as the author.
    #[serde(default)]
    pub author_id: Option<String>,
    /// [`publish_message`] signed with that player's identity key.
    #[serde(default)]
    pub author_signature: Option<String>,
}

/// What a publisher signs to show a game is theirs: its secret, which only
/// they know.
pub fn publish_message(secret: &str) -> Vec<u8> {
    format!("libroxel publish\n{secret}").into_bytes()
}

/// Claiming or changing an account.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountUpdate {
    pub player_id: String,
    /// 3 to 20 letters, digits or underscores, unique on the Hub (whatever the case).
    pub name: String,
    pub about: String,
    /// How their avatar looks, as the app saves it.
    pub look: serde_json::Value,
    /// When it was signed, in seconds since 1970. Each change must be newer
    /// than the last.
    pub time: i64,
    /// [`account_message`] signed with the player's identity key.
    pub signature: String,
    /// The answer to the bot check ([`account_work`]), needed to make a new account.
    #[serde(default)]
    pub work: Option<u64>,
    /// An email for a new account, kept private; a Hub that can send mail
    /// needs one, and mails it a code to prove it ([`EmailCode`]).
    #[serde(default)]
    pub email: Option<String>,
}

/// What a player signs to change their account.
pub fn account_message(
    player_id: &str,
    name: &str,
    about: &str,
    look: &serde_json::Value,
    time: i64,
) -> Vec<u8> {
    format!("libroxel account\n{player_id}\n{name}\n{about}\n{look}\n{time}").into_bytes()
}

/// Whether `name` can be an account's name: 3 to 20 letters, digits or
/// underscores, with at least one letter (a bare number is an account number).
pub fn valid_account_name(name: &str) -> bool {
    (3..=20).contains(&name.len())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.chars().any(|c| c.is_ascii_alphabetic())
}

/// Gives every account without a number the next ones, in the order they joined.
fn number_accounts(db: &Connection) -> rusqlite::Result<()> {
    db.execute("CREATE UNIQUE INDEX IF NOT EXISTS accounts_number ON accounts(number)", [])?;
    let next: i64 = db.query_row("SELECT COALESCE(MAX(number), -1) + 1 FROM accounts", [], |r| r.get(0))?;
    let unnumbered: Vec<String> = db
        .prepare("SELECT player_id FROM accounts WHERE number IS NULL ORDER BY created, rowid")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for (i, id) in unnumbered.iter().enumerate() {
        db.execute("UPDATE accounts SET number = ?2 WHERE player_id = ?1", params![id, next + i as i64])?;
    }
    Ok(())
}

/// Gives every game a number for its page on the website
/// (`/games/<number>/<name>`): the starter games and published games a Hub
/// had before numbers came in get the next ones (starters first, then games
/// in the order they came out), and from then on each new one gets the next
/// number as it is added. A number stays with its game, and is never given
/// to another one, even after the game is taken down.
fn number_games(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS game_numbers (
            game_id     TEXT PRIMARY KEY,
            number      INTEGER NOT NULL UNIQUE
        );
        -- No conflict is possible here (NOT EXISTS), so the INSERT OR REPLACE
        -- that refreshes the starter games can't hand a game a new number.
        CREATE TRIGGER IF NOT EXISTS games_get_numbers AFTER INSERT ON games BEGIN
            INSERT INTO game_numbers (game_id, number)
            SELECT NEW.game_id, (SELECT COALESCE(MAX(number), 0) + 1 FROM game_numbers)
            WHERE NOT EXISTS (SELECT 1 FROM game_numbers WHERE game_id = NEW.game_id);
        END;
        CREATE TRIGGER IF NOT EXISTS starters_get_numbers AFTER INSERT ON official BEGIN
            INSERT INTO game_numbers (game_id, number)
            SELECT 'starter:' || NEW.name, (SELECT COALESCE(MAX(number), 0) + 1 FROM game_numbers)
            WHERE NOT EXISTS (SELECT 1 FROM game_numbers WHERE game_id = 'starter:' || NEW.name);
        END;",
    )?;
    let tx = db.unchecked_transaction()?;
    let unnumbered: Vec<String> = tx
        .prepare(
            "SELECT 'starter:' || name FROM official
             WHERE 'starter:' || name NOT IN (SELECT game_id FROM game_numbers)
             ORDER BY position, name",
        )?
        .query_map([], |r| r.get(0))?
        .chain(
            tx.prepare(
                "SELECT game_id FROM games WHERE game_id NOT IN (SELECT game_id FROM game_numbers)
                 ORDER BY created, rowid",
            )?
            .query_map([], |r| r.get(0))?,
        )
        .collect::<rusqlite::Result<_>>()?;
    let next: i64 =
        tx.query_row("SELECT COALESCE(MAX(number), 0) + 1 FROM game_numbers", [], |r| r.get(0))?;
    for (i, id) in unnumbered.iter().enumerate() {
        tx.execute(
            "INSERT INTO game_numbers (game_id, number) VALUES (?1, ?2)",
            params![id, next + i as i64],
        )?;
    }
    tx.commit()
}

/// The end of a game's page address on the website: its title in small
/// letters, with a dash between words (`My Cool Game!` is `my-cool-game`).
/// The website makes the same one (`slug` in web/app.js).
pub fn game_slug(title: &str) -> String {
    let mut slug = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let mut slug = slug.trim_end_matches('-').to_owned();
    if slug.len() > 60 {
        slug.truncate(60);
        slug = slug.trim_end_matches('-').to_owned();
    }
    slug
}

/// A game's page on the website at `site` (such as `https://playlibre.net`),
/// from its number and title.
pub fn game_page(site: &str, number: u64, title: &str) -> String {
    let slug = game_slug(title);
    if slug.is_empty() { format!("{site}/games/{number}") } else { format!("{site}/games/{number}/{slug}") }
}

/// How long a name someone gave up stays theirs to take back, in seconds.
pub const NAME_HELD_FOR_SECS: i64 = 30 * 24 * 60 * 60;

/// Sharing something to wear in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewItem {
    /// Who made it: items always belong to someone.
    pub creator_id: String,
    pub name: String,
    /// What kind of thing it is; only "hat" for now.
    pub kind: String,
    /// The item itself, as the app saves it.
    pub data: serde_json::Value,
    pub time: i64,
    /// [`item_message`] signed with the creator's identity key.
    pub signature: String,
}

/// What a creator signs to share an item.
pub fn item_message(
    creator_id: &str,
    name: &str,
    kind: &str,
    data: &serde_json::Value,
    time: i64,
) -> Vec<u8> {
    format!("libroxel item\n{creator_id}\n{name}\n{kind}\n{data}\n{time}").into_bytes()
}

/// What a creator signs to take an item out of the catalog.
pub fn unlist_message(item_id: &str) -> Vec<u8> {
    format!("libroxel unlist\n{item_id}").into_bytes()
}

/// The biggest item the catalog takes, in bytes of JSON.
pub const MAX_ITEM: usize = 16 * 1024;

/// Something to wear, as the catalog lists it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Item {
    pub item_id: String,
    pub name: String,
    pub kind: String,
    /// The creator's account name, or "Someone" if they have none here.
    pub creator: String,
    pub creator_id: String,
    pub data: serde_json::Value,
    /// How many times someone has put it on.
    pub wears: u32,
    #[serde(default)]
    pub price: u32,
}

/// Reporting a published game or a catalog item as not OK.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Report {
    pub reporter_id: String,
    /// "game", "item", "comment", "message", "group", "post" or "player".
    pub kind: String,
    /// The thing's id; for a player, their player id or name.
    pub target: String,
    pub reason: String,
    /// What the reporter saw, such as the game and what was said there.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub evidence: String,
    pub time: i64,
    /// [`report_message`] signed with the reporter's identity key.
    pub signature: String,
}

/// What a reporter signs.
pub fn report_message(
    reporter_id: &str,
    kind: &str,
    target: &str,
    reason: &str,
    evidence: &str,
    time: i64,
) -> Vec<u8> {
    let mut out = format!("libroxel report\n{reporter_id}\n{kind}\n{target}\n{reason}\n{time}");
    if !evidence.is_empty() {
        out.push('\n');
        out.push_str(evidence);
    }
    out.into_bytes()
}

pub const MAX_EVIDENCE: usize = 2000;

/// How many different players must report a game or item before the Hub
/// stops listing it (until whoever runs the Hub looks at it).
pub const HIDE_AFTER_REPORTS: i64 = 3;

/// Something reported, as whoever runs the Hub sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Reported {
    pub kind: String,
    pub target: String,
    /// The game's title or the item's name, if it's still there.
    pub name: Option<String>,
    pub reasons: Vec<String>,
    pub hidden: bool,
    /// Whether whoever runs the Hub has said it's fine.
    pub cleared: bool,
}

/// A player's public page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub player_id: String,
    /// Their account number: 0 for the first account made on this Hub, 1
    /// for the next, and so on. Profile links use it.
    #[serde(default)]
    pub number: u64,
    pub name: String,
    pub about: String,
    pub look: serde_json::Value,
    /// Seconds since they made the account.
    pub joined_secs_ago: i64,
    /// The games they published.
    pub games: Vec<GameInfo>,
    /// Whether they are one of the Hub's owners (its admins).
    #[serde(default)]
    pub admin: bool,
    /// Whether they have a verified badge (whoever runs the Hub gives them out).
    #[serde(default)]
    pub verified: bool,
    /// Whether they may add hats to the catalog (owners give it out).
    #[serde(default)]
    pub hat_maker: bool,
    #[serde(default)]
    pub groups: Vec<GroupBadge>,
    #[serde(default)]
    pub badge: Option<GroupBadge>,
}

/// A player found by name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Person {
    pub player_id: String,
    /// Their account number (see [`Profile::number`]).
    #[serde(default)]
    pub number: u64,
    pub name: String,
    pub about: String,
}

/// Signs `message` with an identity's secret key (32 bytes), as text.
pub fn sign(secret_key: &[u8; 32], message: &[u8]) -> String {
    let key = iroh_base::SecretKey::from_bytes(secret_key);
    data_encoding::HEXLOWER.encode(&key.sign(message).to_bytes())
}

/// The player id of an identity's secret key (32 bytes).
pub fn player_id_of(secret_key: &[u8; 32]) -> String {
    iroh_base::SecretKey::from_bytes(secret_key).public().to_string()
}

/// Whether `signature` is `message` signed by the player `player_id`.
pub fn verify(player_id: &str, message: &[u8], signature: &str) -> bool {
    let Ok(key) = player_id.parse::<iroh_base::PublicKey>() else { return false };
    let Ok(bytes) = data_encoding::HEXLOWER.decode(signature.as_bytes()) else { return false };
    let Ok(bytes) = <[u8; 64]>::try_from(bytes.as_slice()) else { return false };
    key.verify(message, &iroh_base::Signature::from_bytes(&bytes)).is_ok()
}

/// What publishing answers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Published {
    pub game_id: String,
    /// 1 for the first version.
    pub version: u32,
}

/// A published game as the list shows it (without the place itself).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameInfo {
    pub game_id: String,
    pub title: String,
    /// Their account name if the game is signed by someone with one.
    pub author: String,
    /// The author's player id, when the game is signed.
    #[serde(default)]
    pub author_id: Option<String>,
    pub description: String,
    pub version: u32,
    /// How many times it has been downloaded.
    pub plays: u32,
    /// The place's size in bytes.
    pub size: u64,
    /// Seconds since the latest version came out.
    pub updated_secs_ago: i64,
    /// Players in its live servers right now (older Hubs don't say).
    #[serde(default)]
    pub playing: u32,
    /// How many servers run it right now.
    #[serde(default)]
    pub servers: u32,
    #[serde(default)]
    pub likes: u32,
    #[serde(default)]
    pub dislikes: u32,
    #[serde(default)]
    pub favorites: u32,
    #[serde(default)]
    pub group: Option<GroupBadge>,
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub max_players: u32,
    #[serde(default)]
    pub closed: bool,
    /// Its number, for its page on the website (`/games/<number>/<name>`);
    /// older Hubs don't say.
    #[serde(default)]
    pub number: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    q: Option<String>,
    /// Only the servers running this game.
    game_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Secret {
    secret: String,
}

// ---------------------------------------------------------------------------
// The server
// ---------------------------------------------------------------------------

type Db = Arc<Mutex<Connection>>;

/// A list search: what was typed, and which game.
type ListKey = (String, Option<String>);
/// A list as last sent: database version, when, and the JSON.
type KeptList = (u64, std::time::Instant, axum::body::Bytes);

/// Server lists already answered, so a crowd of players looking at the
/// list (each asking every few seconds) doesn't run the same search and
/// build the same reply over and over. A list is kept for at most a second
/// and only while nothing in the database changed (the database tells
/// [`ServerLists::changed`] about every write, and `PRAGMA data_version`
/// covers writes from other connections).
#[derive(Default)]
struct ServerLists {
    changed: Arc<std::sync::atomic::AtomicU64>,
    kept: Mutex<HashMap<ListKey, KeptList>>,
}

/// How long a server list is kept at most.
const LIST_KEPT_FOR: Duration = Duration::from_secs(1);

/// Everything the Hub's routes share.
#[derive(Clone)]
struct Hub {
    db: Db,
    server_lists: Arc<ServerLists>,
    limits: Arc<Limits>,
    mail: Arc<Mail>,
    fleet: Option<Arc<fleet::Fleet>>,
    moderation: Option<Arc<Moderation>>,
}

impl FromRef<Hub> for Option<Arc<Moderation>> {
    fn from_ref(hub: &Hub) -> Option<Arc<Moderation>> {
        hub.moderation.clone()
    }
}

impl FromRef<Hub> for Option<Arc<fleet::Fleet>> {
    fn from_ref(hub: &Hub) -> Option<Arc<fleet::Fleet>> {
        hub.fleet.clone()
    }
}

impl FromRef<Hub> for Arc<Mail> {
    fn from_ref(hub: &Hub) -> Arc<Mail> {
        hub.mail.clone()
    }
}

impl FromRef<Hub> for Arc<ServerLists> {
    fn from_ref(hub: &Hub) -> Arc<ServerLists> {
        hub.server_lists.clone()
    }
}

impl FromRef<Hub> for Db {
    fn from_ref(hub: &Hub) -> Db {
        hub.db.clone()
    }
}

impl FromRef<Hub> for Arc<Limits> {
    fn from_ref(hub: &Hub) -> Arc<Limits> {
        hub.limits.clone()
    }
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn clip(text: &str) -> String {
    text.trim().chars().take(MAX_TEXT).collect()
}

/// Opens (or makes) the Hub's database. `None` keeps it in memory.
pub fn open_db(path: Option<&Path>) -> Result<Connection, String> {
    let db = match path {
        Some(path) => {
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir).map_err(|e| format!("could not make {}: {e}", dir.display()))?;
            }
            Connection::open(path)
        }
        None => Connection::open_in_memory(),
    }
    .map_err(|e| format!("could not open the Hub database: {e}"))?;
    if path.is_some() {
        // Readers don't wait for writers, and a write is one append rather
        // than two copies: much faster with many players at once.
        db.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA busy_timeout = 5000;",
        )
        .map_err(|e| format!("could not set up the Hub database: {e}"))?;
    }
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS servers (
            server_id   TEXT PRIMARY KEY,
            secret      TEXT NOT NULL,
            name        TEXT NOT NULL,
            game        TEXT NOT NULL,
            code        TEXT NOT NULL,
            players     INTEGER NOT NULL,
            max_players INTEGER NOT NULL,
            kind        INTEGER NOT NULL,
            first_seen  INTEGER NOT NULL,
            last_seen   INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS servers_last_seen ON servers(last_seen);
        CREATE TABLE IF NOT EXISTS games (
            game_id     TEXT PRIMARY KEY,
            secret      TEXT NOT NULL,
            title       TEXT NOT NULL,
            author      TEXT NOT NULL,
            description TEXT NOT NULL,
            place       TEXT NOT NULL,
            version     INTEGER NOT NULL,
            plays       INTEGER NOT NULL,
            created     INTEGER NOT NULL,
            updated     INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS items (
            item_id     TEXT PRIMARY KEY,
            creator_id  TEXT NOT NULL,
            name        TEXT NOT NULL,
            kind        TEXT NOT NULL,
            data        TEXT NOT NULL,
            wears       INTEGER NOT NULL,
            created     INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS reports (
            kind        TEXT NOT NULL,
            target      TEXT NOT NULL,
            reporter_id TEXT NOT NULL,
            reason      TEXT NOT NULL,
            time        INTEGER NOT NULL,
            PRIMARY KEY (kind, target, reporter_id)
        );
        CREATE TABLE IF NOT EXISTS cleared (
            kind        TEXT NOT NULL,
            target      TEXT NOT NULL,
            PRIMARY KEY (kind, target)
        );
        CREATE TABLE IF NOT EXISTS accounts (
            player_id   TEXT PRIMARY KEY,
            name        TEXT NOT NULL UNIQUE COLLATE NOCASE,
            about       TEXT NOT NULL,
            look        TEXT NOT NULL,
            signed_at   INTEGER NOT NULL,
            created     INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS released_names (
            name        TEXT PRIMARY KEY COLLATE NOCASE,
            player_id   TEXT NOT NULL,
            released    INTEGER NOT NULL
        );",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))?;
    // Hubs from before accounts: games get an author id (this fails, harmlessly, once it has one).
    let _ = db.execute("ALTER TABLE games ADD COLUMN author_id TEXT", []);
    // Hubs from before per-game server lists.
    let _ = db.execute("ALTER TABLE servers ADD COLUMN game_id TEXT", []);
    // Hubs from before signed hosts.
    let _ = db.execute("ALTER TABLE servers ADD COLUMN host_id TEXT", []);
    // A game's own server list is asked for often.
    let _ = db.execute("CREATE INDEX IF NOT EXISTS servers_game ON servers(game_id, last_seen)", []);
    // Hubs from before account numbers: number everyone in the order they joined.
    let _ = db.execute("ALTER TABLE accounts ADD COLUMN number INTEGER", []);
    number_accounts(&db).map_err(|e| format!("could not number the Hub's accounts: {e}"))?;
    social::create_tables(&db)?;
    parties::create_tables(&db)?;
    badges::create_tables(&db)?;
    messages::create_tables(&db)?;
    groups::create_tables(&db)?;
    password::create_tables(&db)?;
    owner::create_tables(&db)?;
    security::create_tables(&db)?;
    email::create_tables(&db)?;
    family::create_tables(&db)?;
    fleet::create_tables(&db)?;
    assets::create_tables(&db)?;
    analytics::create_tables(&db)?;
    stats::create_tables(&db)?;
    ratings::create_tables(&db)?;
    charts::create_tables(&db)?;
    moderate::create_tables(&db)?;
    recovery::create_tables(&db)?;
    manage::create_tables(&db)?;
    market::create_tables(&db)?;
    copies::create_tables(&db)?;
    pictures::create_tables(&db)?;
    wallet::create_tables(&db)?;
    earnings::create_tables(&db)?;
    // Hubs from before game numbers: number every game there is.
    number_games(&db).map_err(|e| format!("could not number the Hub's games: {e}"))?;
    seal::install(&db, path)?;
    closed_games(&db, false).map_err(|e| format!("could not set up the Hub database: {e}"))?;
    Ok(db)
}

fn closed_games(db: &Connection, closed: bool) -> rusqlite::Result<()> {
    db.create_scalar_function("closed_games", 0, rusqlite::functions::FunctionFlags::SQLITE_UTF8, move |_| {
        Ok(closed)
    })
}

/// The Hub's routes over `db`.
///
/// Serve it with `into_make_service_with_connect_info::<SocketAddr>()` so
/// rate limits count each caller on their own; otherwise every caller
/// shares one count.
pub fn router(db: Connection) -> Router {
    router_with(db, HubOptions::default())
}

/// [`router`], run the way `options` says.
pub fn router_with(db: Connection, options: HubOptions) -> Router {
    let limits =
        Arc::new(Limits { trust_proxy: options.behind_proxy, off: options.no_limits, ..Limits::default() });
    let fleet = options.fleet.map(|o| Arc::new(fleet::Fleet::new(o)));
    closed_games(&db, fleet.is_some()).expect("the database takes functions");
    let mut db = db;
    let server_lists = Arc::new(ServerLists::default());
    let changed = server_lists.changed.clone();
    // Any write may change what a list shows (bans, names, servers), so any
    // write drops the kept lists. Reads between writes are served from memory.
    db.update_hook(Some(move |_: rusqlite::hooks::Action, _: &str, _: &str, _: i64| {
        changed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }))
    .expect("the database takes hooks");
    let hub = Hub {
        db: Arc::new(Mutex::new(db)),
        server_lists,
        limits,
        mail: Arc::new(options.mail),
        fleet,
        moderation: options.moderation.map(Arc::new),
    };
    // Look after the Hub's own game servers every few seconds.
    if let (Some(fleet), Ok(runtime)) = (hub.fleet.clone(), tokio::runtime::Handle::try_current()) {
        let db = hub.db.clone();
        runtime.spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(3)).await;
                fleet.tidy(&db.lock().unwrap());
            }
        });
    }
    // Servers that stopped saying they're up are forgotten here, every so
    // often, rather than on every look at the list (a write each time).
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        let db = hub.db.clone();
        runtime.spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let _ = db
                    .lock()
                    .unwrap()
                    .execute("DELETE FROM servers WHERE last_seen < ?1", params![now() - EXPIRES_AFTER_SECS]);
            }
        });
    }
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        let db = hub.db.clone();
        runtime.spawn(async move {
            loop {
                let _ = earnings::settle(&db.lock().unwrap(), now());
                tokio::time::sleep(Duration::from_secs(600)).await;
            }
        });
    }
    // Only publishing carries a whole place; everything else is small.
    let big = (
        tower_http::decompression::RequestDecompressionLayer::new(),
        axum::extract::DefaultBodyLimit::max(MAX_PLACE * 2 + 64 * 1024),
    );
    let shared = axum::extract::DefaultBodyLimit::max(MAX_MODEL * 3);
    Router::new()
        .route("/", get(status))
        .route("/servers", get(list_servers))
        .route("/servers/heartbeat", post(heartbeat))
        .route("/servers/{id}", delete(remove_server))
        .route("/play", post(play))
        .route("/games", get(list_games).post(publish).layer(big))
        .route("/games/{id}", get(game_info).delete(unpublish))
        .route("/games/{id}/ratings", get(ratings::get_ratings).post(ratings::rate))
        .route("/games/{id}/comments", get(ratings::list_comments).post(ratings::add_comment))
        .route("/comments/{id}", delete(ratings::remove_comment))
        .route("/accounts/{id}/favorites", get(ratings::favorites))
        .route("/accounts/{id}/badges", get(badges::player_badges))
        .route("/games/{id}/badges", get(badges::game_badges).post(badges::change_badge))
        .route("/badges/award", post(badges::award))
        .route("/games/{id}/place", get(download).layer(tower_http::compression::CompressionLayer::new()))
        .route("/assets/missing", post(assets::missing_assets))
        .route(
            "/assets/{hash}",
            get(assets::get_asset)
                .post(assets::put_asset)
                .layer(axum::extract::DefaultBodyLimit::max(MAX_ASSET + 1024)),
        )
        .route("/official", get(assets::list_official))
        .route("/official/{name}", get(assets::official_place))
        .route("/accounts", get(find_people).post(update_account))
        .route("/accounts/{id}", get(profile))
        .route("/accounts/{id}/pictures/{file}", get(pictures::picture))
        .route("/avatar/{file}", get(pictures::preview))
        .route("/accounts/password", post(password::set_password))
        .route("/accounts/sign-in", post(password::sign_in))
        .route("/accounts/sign-in/{name}", get(password::sign_in_salt))
        .route("/accounts/two-step", post(security::set_two_step))
        .route("/accounts/recovery-codes", post(recovery::new_codes))
        .route("/accounts/session", post(recovery::new_session))
        .route("/accounts/sign-out-everywhere", post(recovery::sign_out_everywhere))
        .route("/accounts/reset", post(recovery::ask_reset))
        .route("/accounts/reset/finish", post(recovery::finish_reset))
        .route("/accounts/email", post(email::set_email))
        .route("/accounts/email/verify", post(email::verify_email))
        .route("/accounts/{id}/security", get(security::security))
        .route("/accounts/birthday", post(family::set_birthday))
        .route("/accounts/family", post(family::change))
        .route("/accounts/family/parent", post(family::ask_parent))
        .route("/accounts/family/consent", post(family::consent))
        .route("/accounts/{id}/family", get(family::family))
        .route("/accounts/{id}/chat", get(family::chat_rule))
        .route("/market", get(market::list).post(market::publish).layer(shared))
        .route("/market/{id}", get(market::info).delete(market::remove))
        .route("/market/{id}/data", get(market::data))
        .route("/catalog", get(list_items).post(share_item))
        .route("/catalog/{id}", delete(unlist_item))
        .route("/catalog/{id}/wear", post(wear_item))
        .route("/catalog/{id}/price", post(wallet::set_price))
        .route("/shop/buy", post(wallet::buy_item))
        .route("/wallet/daily", post(wallet::no_daily))
        .route("/wallet/{id}", get(wallet::get_wallet))
        .route("/earnings/{id}", get(earnings::get_earnings))
        .route("/earnings/cash-out", post(earnings::cash_out))
        .route("/games/{id}/passes", get(earnings::list_passes).post(earnings::change_pass))
        .route("/passes/buy", post(earnings::buy_pass))
        .route("/passes/{id}/owners/{player}", get(earnings::owns))
        .route("/reports", post(report))
        .route("/moderation", post(moderate::moderate))
        .route("/moderation/{id}", get(moderate::get_dashboard))
        .route("/accounts/warnings/seen", post(moderate::warnings_seen))
        .route("/appeals", post(moderate::appeal))
        .route("/charts", get(charts::get_charts))
        .route("/for-you/{id}", get(charts::for_you))
        .route("/groups", get(groups::list_groups).post(groups::change_group))
        .route("/groups/{id}", get(groups::group_page))
        .route("/groups/invites/{id}", get(groups::group_invites))
        .route("/friends", post(social::change_friend))
        .route("/friends/{id}", get(social::list_friends))
        .route("/presence", post(social::update_presence))
        .route("/presence/web", post(social::update_web_presence))
        .route("/parties", post(parties::change_party))
        .route("/parties/{id}", get(parties::get_party))
        .route("/creator/{id}", get(analytics::creator))
        .route("/creator/{id}/games", get(manage::my_games))
        .route("/games/{id}/versions", get(manage::versions))
        .route("/games/{id}/manage", post(manage::manage))
        .route("/stats/ping", post(stats::ping))
        .route("/stats/crash", post(stats::crash))
        .route("/admin/{id}", get(stats::admin))
        .route("/messages", post(messages::send_message))
        .route("/messages/{id}", get(messages::conversations))
        .route("/messages/{id}/{other}", get(messages::conversation))
        .fallback(|| async { (StatusCode::NOT_FOUND, NO_ROUTE) })
        .layer(axum::extract::DefaultBodyLimit::max(256 * 1024))
        .layer(axum::middleware::from_fn_with_state(hub.limits.clone(), limits::every_request))
        // Any web page may call the Hub, as the app does: everything that
        // changes something is signed, and nothing rides on cookies.
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::DELETE,
                ])
                .allow_headers([axum::http::header::CONTENT_TYPE]),
        )
        .with_state(hub)
}

async fn status(State(db): State<Db>) -> Json<HubStatus> {
    let db = db.lock().unwrap();
    let (servers, players): (i64, i64) = db
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(players), 0) FROM servers WHERE last_seen >= ?1",
            params![now() - EXPIRES_AFTER_SECS],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap_or((0, 0));
    let games: i64 = db.query_row("SELECT COUNT(*) FROM games", [], |r| r.get(0)).unwrap_or(0);
    Json(HubStatus {
        hub: "playlibre".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        servers: servers as u32,
        players: players as u32,
        games: games as u32,
    })
}

pub const NO_ROUTE: &str = "this Hub is older than the site";

fn clip_to(text: &str, most: usize) -> String {
    text.trim().chars().take(most).collect()
}

/// A short random id for a newly published game.
fn new_game_id() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the system has a random number source");
    data_encoding::BASE32_NOPAD.encode(&bytes).to_ascii_lowercase()
}

const GAME_COLUMNS: &str = concat!(
    "g.game_id, g.title, COALESCE(a.name, g.author), g.description, g.version, g.plays,
    LENGTH(g.place), g.updated, g.author_id,
    (SELECT COALESCE(SUM(s.players), 0) FROM servers s WHERE s.game_id = g.game_id AND s.last_seen >= ",
    "CAST(strftime('%s', 'now') AS INTEGER) - 45),
    (SELECT COUNT(*) FROM servers s WHERE s.game_id = g.game_id AND s.last_seen >= ",
    "CAST(strftime('%s', 'now') AS INTEGER) - 45),
    (SELECT COUNT(*) FROM game_votes v WHERE v.game_id = g.game_id AND v.vote = 1),
    (SELECT COUNT(*) FROM game_votes v WHERE v.game_id = g.game_id AND v.vote = -1),
    (SELECT COUNT(*) FROM game_favorites f WHERE f.game_id = g.game_id),
    gr.group_id, gr.name, gr.tag, gr.color, g.private, g.max_players, closed_games(),
    (SELECT n.number FROM game_numbers n WHERE n.game_id = g.game_id)"
);
const GAMES: &str = "games g LEFT JOIN accounts a ON a.player_id = g.author_id
    LEFT JOIN group_games gg ON gg.game_id = g.game_id LEFT JOIN groups gr ON gr.group_id = gg.group_id";

/// SQL that holds for a `kind` of thing whose id is `column` when enough
/// players reported it and whoever runs the Hub hasn't cleared it.
fn hidden_sql(kind: &str, column: &str) -> String {
    format!(
        "((SELECT COUNT(*) FROM reports r WHERE r.kind = '{kind}' AND r.target = {column}) >= {HIDE_AFTER_REPORTS}
          AND NOT EXISTS (SELECT 1 FROM cleared c WHERE c.kind = '{kind}' AND c.target = {column}))"
    )
}

fn game_row(r: &rusqlite::Row, now: i64) -> rusqlite::Result<GameInfo> {
    Ok(GameInfo {
        game_id: r.get(0)?,
        title: r.get(1)?,
        author: r.get(2)?,
        description: r.get(3)?,
        version: r.get::<_, i64>(4)? as u32,
        plays: r.get::<_, i64>(5)? as u32,
        size: r.get::<_, i64>(6)? as u64,
        updated_secs_ago: now - r.get::<_, i64>(7)?,
        author_id: r.get(8)?,
        playing: r.get::<_, i64>(9)? as u32,
        servers: r.get::<_, i64>(10)? as u32,
        likes: r.get::<_, i64>(11)? as u32,
        dislikes: r.get::<_, i64>(12)? as u32,
        favorites: r.get::<_, i64>(13)? as u32,
        group: match r.get::<_, Option<String>>(14)? {
            Some(group_id) => {
                Some(GroupBadge { group_id, name: r.get(15)?, tag: r.get(16)?, color: r.get(17)? })
            }
            None => None,
        },
        private: r.get(18)?,
        max_players: r.get::<_, i64>(19)? as u32,
        closed: r.get(20)?,
        number: r.get::<_, Option<i64>>(21)?.map(|n| n as u64),
    })
}

async fn list_games(State(db): State<Db>, Query(query): Query<ListQuery>) -> Json<Vec<GameInfo>> {
    let db = db.lock().unwrap();
    let pattern = format!("%{}%", query.q.unwrap_or_default().trim().replace(['%', '_'], ""));
    let now = now();
    let mut statement = db
        .prepare(&format!(
            "SELECT {GAME_COLUMNS} FROM {GAMES}
             WHERE (g.title LIKE ?1 OR COALESCE(a.name, g.author) LIKE ?1 OR g.description LIKE ?1)
                AND g.private = 0 AND NOT {hidden} AND NOT {banned}
             ORDER BY g.plays DESC, g.updated DESC LIMIT 200",
            hidden = hidden_sql("game", "g.game_id"),
            banned = owner::banned_sql("g.author_id")
        ))
        .expect("the games query is valid");
    let rows = statement
        .query_map(params![pattern], |r| game_row(r, now))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default();
    Json(rows)
}

async fn game_info(State(db): State<Db>, UrlPath(id): UrlPath<String>) -> Result<Json<GameInfo>, StatusCode> {
    let db = db.lock().unwrap();
    let now = now();
    // Its id, or its number (as its page on the website has it).
    let number: Option<i64> = id.parse().ok();
    db.query_row(
        &format!(
            "SELECT {GAME_COLUMNS} FROM {GAMES}
             WHERE g.game_id = ?1 OR g.game_id = (SELECT game_id FROM game_numbers WHERE number = ?2)
             ORDER BY g.game_id = ?1 DESC LIMIT 1"
        ),
        params![id, number],
        |r| game_row(r, now),
    )
    .optional()
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map(Json)
    .ok_or(StatusCode::NOT_FOUND)
}

pub fn download_message(game_id: &str, player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel download\n{game_id}\n{player_id}\n{time}").into_bytes()
}

#[derive(Debug, Clone)]
pub struct DownloadProof {
    pub player_id: String,
    pub time: i64,
    pub signature: String,
}

impl DownloadProof {
    pub fn new(game_id: &str, player_id: String, sign: impl FnOnce(&[u8]) -> String) -> Self {
        let time = now();
        let signature = sign(&download_message(game_id, &player_id, time));
        Self { player_id, time, signature }
    }
}

const DOWNLOAD_SIGNED_FOR: i64 = 300;

// The `x-libroxel-…` download headers, like the `libroxel …` prefix of
// every signed message, keep the app's name from before the rename to
// PlayLibre: copies from before it send them, and signatures cover them.

fn download_signer(headers: &HeaderMap, game_id: &str) -> Option<String> {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let player_id = header("x-libroxel-player")?;
    let time: i64 = header("x-libroxel-time")?.parse().ok()?;
    let signature = header("x-libroxel-signature")?;
    ((now() - time).abs() <= DOWNLOAD_SIGNED_FOR
        && verify(player_id, &download_message(game_id, player_id, time), signature))
    .then(|| player_id.to_owned())
}

async fn download(
    State(db): State<Db>,
    State(limits): State<Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    UrlPath(id): UrlPath<String>,
    headers: HeaderMap,
) -> Result<String, (StatusCode, String)> {
    let db = db.lock().unwrap();
    let game: Option<(String, Option<String>)> = db
        .query_row(
            &format!(
                "SELECT place, author_id FROM games WHERE game_id = ?1 AND private = 0 AND NOT {}",
                hidden_sql("game", "game_id")
            ),
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let (place, author_id) =
        game.ok_or((StatusCode::NOT_FOUND, "there is no published game with that id".to_owned()))?;
    let trusted = download_signer(&headers, &id).is_some_and(|signer| {
        author_id.as_deref() == Some(signer.as_str()) || fleet::runs(&db, &signer, &id)
    });
    // A place holds the game's server scripts and storage, which players'
    // computers never get, so only its creator and the Hub's own servers
    // may have it, whether or not this Hub runs servers. Everyone else
    // plays it by joining a server.
    if !trusted {
        if !limits.take_logged(Limit::Download, &caller.0) {
            return Err(too_many("games downloaded"));
        }
        eprintln!("{} was refused the place of {id}", caller.0);
        return Err((
            StatusCode::FORBIDDEN,
            "only its creator can download or host this game; join one of its servers to play".into(),
        ));
    }
    let _ = db.execute("UPDATE games SET plays = plays + 1 WHERE game_id = ?1", params![id]);
    Ok(place)
}

async fn publish(
    State(db): State<Db>,
    State(limits): State<Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    Json(game): Json<Publish>,
) -> Result<Json<Published>, (StatusCode, String)> {
    if !limits.take(Limit::Share, &caller.0) {
        return Err(too_many("games and items shared"));
    }
    let bad = |why: &str| Err((StatusCode::BAD_REQUEST, why.to_owned()));
    if game.secret.len() < 16 {
        return bad("publishing needs a secret of 16 or more characters");
    }
    if game.title.trim().is_empty() {
        return bad("a published game needs a title");
    }
    if game.place.len() > MAX_PLACE {
        return bad("that game is too big to publish here");
    }
    // Only saved places: a JSON object, as `.lrx` files are.
    if serde_json::from_str::<HashMap<String, IgnoredAny>>(&game.place).is_err() {
        return bad("that is not a saved game");
    }
    let author_id = match (&game.author_id, &game.author_signature) {
        (Some(id), Some(signature)) => {
            if !verify(id, &publish_message(&game.secret), signature) {
                return Err((StatusCode::FORBIDDEN, "the author's signature does not match".into()));
            }
            Some(id.clone())
        }
        _ => None,
    };
    let prints = copies::prints(&game.place);
    let db = db.lock().unwrap();
    if author_id.as_deref().is_some_and(|id| crate::owner::is_banned(&db, id)) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from PlayLibre".into()));
    }
    let now = now();
    let title = clip(&game.title);
    let author = if game.author.trim().is_empty() { "Someone".to_owned() } else { clip(&game.author) };
    let description = clip_to(&game.description, MAX_DESCRIPTION);
    let internal = |e: rusqlite::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    match game.game_id.as_deref().map(str::trim).filter(|id| !id.is_empty()) {
        Some(id) => {
            let owner: Option<(String, i64)> = db
                .query_row("SELECT secret, version FROM games WHERE game_id = ?1", params![id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional()
                .map_err(internal)?;
            let Some((secret, version)) = owner else {
                return Err((StatusCode::NOT_FOUND, "there is no published game with that id".into()));
            };
            if secret != seal::pepper(&db, &game.secret) {
                return Err((StatusCode::FORBIDDEN, "someone else published that game".into()));
            }
            let owner = author_id.clone().unwrap_or_else(|| id.to_owned());
            not_copied(&db, id, &owner, &prints)?;
            db.execute(
                "UPDATE games SET title = ?2, author = ?3, description = ?4, place = ?5, version = ?6,
                     updated = ?7, author_id = ?8 WHERE game_id = ?1",
                params![id, title, author, description, game.place, version + 1, now, author_id],
            )
            .map_err(internal)?;
            manage::keep_version(&db, id, version + 1, &title, &game.place, now).map_err(internal)?;
            copies::keep(&db, id, &owner, &prints).map_err(internal)?;
            Ok(Json(Published { game_id: id.to_owned(), version: version as u32 + 1 }))
        }
        None => {
            let id = new_game_id();
            let owner = author_id.clone().unwrap_or_else(|| id.clone());
            not_copied(&db, &id, &owner, &prints)?;
            db.execute(
                "INSERT INTO games (game_id, secret, title, author, description, place, version, plays, created,
                     updated, author_id)
                 VALUES (?1, pepper(?2), ?3, ?4, ?5, ?6, 1, 0, ?7, ?7, ?8)",
                params![id, game.secret, title, author, description, game.place, now, author_id],
            )
            .map_err(internal)?;
            manage::keep_version(&db, &id, 1, &title, &game.place, now).map_err(internal)?;
            copies::keep(&db, &id, &owner, &prints).map_err(internal)?;
            Ok(Json(Published { game_id: id, version: 1 }))
        }
    }
}

fn not_copied(
    db: &Connection,
    game_id: &str,
    owner: &str,
    prints: &std::collections::HashSet<String>,
) -> Result<(), (StatusCode, String)> {
    match copies::copied_from(db, game_id, owner, prints) {
        Some(title) => {
            eprintln!("{owner} tried to publish a copy of {title}");
            Err((
                StatusCode::CONFLICT,
                format!("this game looks copied from “{title}”, which someone else made"),
            ))
        }
        None => Ok(()),
    }
}

async fn update_account(
    State(db): State<Db>,
    State(limits): State<Arc<Limits>>,
    State(mail): State<Arc<Mail>>,
    Extension(caller): Extension<Caller>,
    Json(account): Json<AccountUpdate>,
) -> Result<StatusCode, (StatusCode, String)> {
    let outgoing = save_account(&db, &limits, &mail, &caller, &account)?;
    if let Err(e) = email::post(mail, outgoing).await {
        eprintln!("could not mail {}'s code: {e}", account.name);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Checks and keeps an account change; a new account's email code to mail comes back.
fn save_account(
    db: &Db,
    limits: &Limits,
    mail: &Mail,
    caller: &Caller,
    account: &AccountUpdate,
) -> Result<Option<email::Outgoing>, (StatusCode, String)> {
    let bad = |why: &str| Err((StatusCode::BAD_REQUEST, why.to_owned()));
    if !valid_account_name(&account.name) {
        return bad("a name is 3 to 20 letters, digits or underscores");
    }
    if account.about.chars().count() > MAX_DESCRIPTION {
        return bad("that is too much to say about yourself");
    }
    let about = account.about.clone();
    let look = account.look.to_string();
    if look.len() > 8 * 1024 {
        return bad("that look is too big");
    }
    let message =
        account_message(&account.player_id, &account.name, &account.about, &account.look, account.time);
    if !verify(&account.player_id, &message, &account.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    let now = now();
    if (account.time - now).abs() > 24 * 60 * 60 {
        return bad("this computer's clock is too far off");
    }
    let db = db.lock().unwrap();
    if crate::owner::is_banned(&db, &account.player_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from PlayLibre".into()));
    }
    if recovery::was_moved(&db, &account.player_id) {
        return Err((StatusCode::GONE, SIGNED_OUT.into()));
    }
    if !wallet::may_wear(&db, &account.player_id, &account.look) {
        return Err((StatusCode::PAYMENT_REQUIRED, "buy that hat in the shop first".into()));
    }
    let internal = |e: rusqlite::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    let last: Option<(i64, String)> = db
        .query_row(
            "SELECT signed_at, name FROM accounts WHERE player_id = ?1",
            params![account.player_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(internal)?;
    if last.as_ref().is_some_and(|(last, _)| account.time <= *last) {
        return Err((StatusCode::CONFLICT, "a newer change to that account is already here".into()));
    }
    let old_name = last.map(|(_, name)| name);
    let new_account = old_name.is_none();
    let renaming = !old_name.as_ref().is_some_and(|old| old.eq_ignore_ascii_case(&account.name));
    let taken: Option<String> = db
        .query_row("SELECT player_id FROM accounts WHERE name = ?1", params![account.name], |r| r.get(0))
        .optional()
        .map_err(internal)?;
    if taken.is_some_and(|owner| owner != account.player_id) {
        return Err((StatusCode::CONFLICT, format!("someone already has the name {}", account.name)));
    }
    let held: Option<String> = db
        .query_row(
            "SELECT player_id FROM released_names WHERE name = ?1 AND released >= ?2",
            params![account.name, now - NAME_HELD_FOR_SECS],
            |r| r.get(0),
        )
        .optional()
        .map_err(internal)?;
    if held.is_some_and(|owner| owner != account.player_id) {
        return Err((
            StatusCode::CONFLICT,
            format!("{} was someone else's name until recently; it stays theirs for 30 days", account.name),
        ));
    }
    let email = account.email.as_deref().map(str::trim).filter(|e| !e.is_empty());
    if old_name.is_none() && mail.sends() && email.is_none() {
        return bad("a new account needs an email (update PlayLibre if it didn't ask for one)");
    }
    if let Some(email) = email.filter(|_| old_name.is_none())
        && !valid_email(email)
    {
        return bad("that doesn't look like an email address");
    }
    if old_name.is_none() && !security::work_done(&account.player_id, &account.name, account.work) {
        return bad("a new account needs the bot check worked out (update PlayLibre)");
    }
    if old_name.is_none() && !limits.take(Limit::NewAccount, &caller.0) {
        return Err(too_many("new accounts"));
    }
    if let Some(old) = old_name.filter(|_| renaming) {
        db.execute(
            "INSERT INTO released_names (name, player_id, released) VALUES (?1, ?2, ?3)
             ON CONFLICT(name) DO UPDATE SET player_id = ?2, released = ?3",
            params![old, account.player_id, now],
        )
        .map_err(internal)?;
    }
    db.execute("DELETE FROM released_names WHERE name = ?1", params![account.name]).map_err(internal)?;
    db.execute(
        "INSERT INTO accounts (player_id, name, about, look, signed_at, created, number)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT COALESCE(MAX(number), -1) + 1 FROM accounts))
         ON CONFLICT(player_id) DO UPDATE SET name = ?2, about = ?3, look = ?4, signed_at = ?5",
        params![account.player_id, account.name, about, look, account.time, now],
    )
    .map_err(internal)?;
    // A new account's email: kept, and a code mailed to prove it.
    match email.filter(|_| new_account) {
        Some(email) => Ok(email::keep(&db, mail, &account.player_id, email)?.1),
        None => Ok(None),
    }
}

async fn share_item(
    State(db): State<Db>,
    State(limits): State<Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    Json(item): Json<NewItem>,
) -> Result<Json<Item>, (StatusCode, String)> {
    if !limits.take(Limit::Share, &caller.0) {
        return Err(too_many("games and items shared"));
    }
    let bad = |why: &str| Err((StatusCode::BAD_REQUEST, why.to_owned()));
    if item.kind != "hat" {
        return bad("the catalog only takes hats for now");
    }
    let name = clip(&item.name);
    if name.is_empty() || name != item.name {
        return bad("an item needs a name, without spaces at the ends");
    }
    let data = item.data.to_string();
    if data.len() > MAX_ITEM || !item.data.get("parts").is_some_and(serde_json::Value::is_array) {
        return bad("that is not a hat the catalog can take");
    }
    if !verify(
        &item.creator_id,
        &item_message(&item.creator_id, &item.name, &item.kind, &item.data, item.time),
        &item.signature,
    ) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that creator".into()));
    }
    let db = db.lock().unwrap();
    if crate::owner::is_banned(&db, &item.creator_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from PlayLibre".into()));
    }
    if !owner::is_hat_maker(&db, &item.creator_id) {
        return Err((StatusCode::FORBIDDEN, "only Hat-Makers can add to the catalog for now".into()));
    }
    let item_id = new_game_id();
    db.execute(
        "INSERT INTO items (item_id, creator_id, name, kind, data, wears, created) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)",
        params![item_id, item.creator_id, name, item.kind, data, now()],
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let creator: Option<String> = db
        .query_row("SELECT name FROM accounts WHERE player_id = ?1", params![item.creator_id], |r| r.get(0))
        .optional()
        .ok()
        .flatten();
    Ok(Json(Item {
        item_id,
        name,
        kind: item.kind,
        creator: creator.unwrap_or_else(|| "Someone".into()),
        creator_id: item.creator_id,
        data: item.data,
        wears: 0,
        price: 0,
    }))
}

async fn list_items(State(db): State<Db>, Query(query): Query<ListQuery>) -> Json<Vec<Item>> {
    let db = db.lock().unwrap();
    let pattern = format!("%{}%", query.q.unwrap_or_default().trim().replace(['%', '_'], ""));
    let mut statement = db
        .prepare(&format!(
            "SELECT i.item_id, i.name, i.kind,
                    COALESCE(a.name, CASE i.creator_id WHEN 'playlibre' THEN 'PlayLibre' ELSE 'Someone' END),
                    i.creator_id, i.data, i.wears, i.price
             FROM items i LEFT JOIN accounts a ON a.player_id = i.creator_id
             WHERE (i.name LIKE ?1 OR COALESCE(a.name, '') LIKE ?1) AND NOT {} AND NOT {}
             ORDER BY i.wears DESC, i.created DESC LIMIT 200",
            hidden_sql("item", "i.item_id"),
            owner::banned_sql("i.creator_id")
        ))
        .expect("the catalog query is valid");
    let rows = statement
        .query_map(params![pattern], |r| {
            let data: String = r.get(5)?;
            Ok(Item {
                item_id: r.get(0)?,
                name: r.get(1)?,
                kind: r.get(2)?,
                creator: r.get(3)?,
                creator_id: r.get(4)?,
                data: serde_json::from_str(&data).unwrap_or(serde_json::Value::Null),
                wears: r.get::<_, i64>(6)? as u32,
                price: r.get::<_, i64>(7)? as u32,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default();
    Json(rows)
}

async fn wear_item(State(db): State<Db>, UrlPath(id): UrlPath<String>) -> StatusCode {
    let db = db.lock().unwrap();
    match db.execute("UPDATE items SET wears = wears + 1 WHERE item_id = ?1", params![id]) {
        Ok(0) => StatusCode::NOT_FOUND,
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn unlist_item(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Json(signed): Json<Secret>,
) -> StatusCode {
    let db = db.lock().unwrap();
    let creator: Option<String> = db
        .query_row("SELECT creator_id FROM items WHERE item_id = ?1", params![id], |r| r.get(0))
        .optional()
        .ok()
        .flatten();
    let Some(creator) = creator else { return StatusCode::NOT_FOUND };
    if !verify(&creator, &unlist_message(&id), &signed.secret) {
        return StatusCode::FORBIDDEN;
    }
    match db.execute("DELETE FROM items WHERE item_id = ?1", params![id]) {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn report(
    State(db): State<Db>,
    State(limits): State<Arc<Limits>>,
    Extension(caller): Extension<Caller>,
    Json(report): Json<Report>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !limits.take(Limit::Report, &caller.0) {
        return Err(too_many("reports"));
    }
    let bad = |why: &str| Err((StatusCode::BAD_REQUEST, why.to_owned()));
    if !["game", "item", "market", "comment", "message", "group", "post", "player"]
        .contains(&report.kind.as_str())
    {
        return bad(
            "only players, games, items, marketplace items, comments, messages, groups and posts can be reported",
        );
    }
    if report.reason.trim().is_empty() || report.reason.chars().count() > 300 {
        return bad("a report needs a short reason");
    }
    if report.evidence.chars().count() > MAX_EVIDENCE {
        return bad("that's too much to send with a report");
    }
    let message = report_message(
        &report.reporter_id,
        &report.kind,
        &report.target,
        &report.reason,
        &report.evidence,
        report.time,
    );
    if !verify(&report.reporter_id, &message, &report.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    let db = db.lock().unwrap();
    if crate::owner::is_banned(&db, &report.reporter_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from PlayLibre".into()));
    }
    let mut target = report.target.trim().to_owned();
    if report.kind == "player" {
        target = owner::resolve(&db, &target).map_err(|e| (StatusCode::NOT_FOUND, e))?;
        if target == report.reporter_id {
            return bad("you can't report yourself");
        }
    }
    let table = match report.kind.as_str() {
        "game" => "games WHERE game_id = ?1",
        "item" => "items WHERE item_id = ?1",
        "market" => "market WHERE asset_id = ?1",
        "comment" => "game_comments WHERE comment_id = ?1",
        "group" => "groups WHERE group_id = ?1",
        "post" => "group_posts WHERE post_id = ?1",
        "player" => "accounts WHERE player_id = ?1",
        _ => "messages WHERE message_id = ?1 AND to_id = ?2",
    };
    let exists: Option<i64> = db
        .query_row(
            &format!("SELECT 1 FROM {table} AND ?2 IS NOT NULL"),
            params![target, report.reporter_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if exists.is_none() {
        return Err((StatusCode::NOT_FOUND, "there is nothing with that id".into()));
    }
    let chat = moderate::chat_around(&db, &report.kind, &target, &report.reporter_id);
    db.execute(
        "INSERT INTO reports (kind, target, reporter_id, reason, time, evidence, chat)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(kind, target, reporter_id) DO UPDATE SET reason = ?4, time = ?5, evidence = ?6, chat = ?7",
        params![report.kind, target, report.reporter_id, report.reason.trim(), now(), report.evidence.trim(), chat],
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Everything reported, most reported first, for whoever runs the Hub.
pub fn reports(db: &Connection) -> Vec<Reported> {
    let mut statement = db
        .prepare(
            "SELECT r.kind, r.target, GROUP_CONCAT(r.reason, '\n'), COUNT(*),
                 EXISTS (SELECT 1 FROM cleared c WHERE c.kind = r.kind AND c.target = r.target),
                 CASE r.kind WHEN 'game' THEN (SELECT title FROM games WHERE game_id = r.target)
                             WHEN 'item' THEN (SELECT name FROM items WHERE item_id = r.target)
                             WHEN 'market' THEN (SELECT name FROM market WHERE asset_id = r.target)
                             WHEN 'comment' THEN (SELECT text FROM game_comments WHERE comment_id = r.target)
                             WHEN 'group' THEN (SELECT name FROM groups WHERE group_id = r.target)
                             WHEN 'post' THEN (SELECT text FROM group_posts WHERE post_id = r.target)
                             ELSE (SELECT a.name || ': ' || unseal(m.text) FROM messages m
                                   LEFT JOIN accounts a ON a.player_id = m.from_id WHERE m.message_id = r.target) END
             FROM reports r GROUP BY r.kind, r.target ORDER BY COUNT(*) DESC",
        )
        .expect("the reports query is valid");
    statement
        .query_map([], |r| {
            let reasons: String = r.get(2)?;
            let count: i64 = r.get(3)?;
            let cleared: bool = r.get(4)?;
            Ok(Reported {
                kind: r.get(0)?,
                target: r.get(1)?,
                name: r.get(5)?,
                reasons: reasons.lines().map(str::to_owned).collect(),
                hidden: count >= HIDE_AFTER_REPORTS && !cleared,
                cleared,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

/// Whoever runs the Hub says a reported game or item is fine: it's listed
/// again, whatever reports come.
pub fn clear_reports(db: &Connection, kind: &str, target: &str) -> Result<(), String> {
    db.execute("INSERT OR IGNORE INTO cleared (kind, target) VALUES (?1, ?2)", params![kind, target])
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Whoever runs the Hub takes a game or item down for good.
pub fn take_down(db: &Connection, kind: &str, target: &str) -> Result<(), String> {
    let sql = match kind {
        "game" => "DELETE FROM games WHERE game_id = ?1",
        "item" => "DELETE FROM items WHERE item_id = ?1",
        "comment" => "DELETE FROM game_comments WHERE comment_id = ?1",
        "message" => "DELETE FROM messages WHERE message_id = ?1",
        "post" => "DELETE FROM group_posts WHERE post_id = ?1",
        "group" | "market" => "",
        _ => return Err(format!("there are no {kind}s on a Hub")),
    };
    let gone = match kind {
        "group" => groups::delete_group(db, target),
        "market" => market::take_down(db, target),
        _ => db.execute(sql, params![target]),
    }
    .map_err(|e| e.to_string())?;
    if gone == 0 {
        return Err(format!("there is no {kind} {target}"));
    }
    if kind == "game" {
        manage::forget(db, target);
    }
    Ok(())
}

async fn find_people(State(db): State<Db>, Query(query): Query<ListQuery>) -> Json<Vec<Person>> {
    let db = db.lock().unwrap();
    let pattern = format!("%{}%", query.q.unwrap_or_default().trim().replace(['%', '_'], ""));
    let mut statement = db
        .prepare(&format!(
            "SELECT player_id, name, about, number FROM accounts WHERE name LIKE ?1 AND NOT {}
             ORDER BY name LIMIT 100",
            owner::banned_sql("accounts.player_id")
        ))
        .expect("the people query is valid");
    let rows = statement
        .query_map(params![pattern], |r| {
            Ok(Person {
                player_id: r.get(0)?,
                name: r.get(1)?,
                about: r.get(2)?,
                number: r.get::<_, i64>(3)? as u64,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default();
    Json(rows)
}

async fn profile(State(db): State<Db>, UrlPath(id): UrlPath<String>) -> Result<Json<Profile>, StatusCode> {
    let db = db.lock().unwrap();
    let now = now();
    // An account number (names always have a letter), a player id or a name.
    let number: Option<i64> = id.parse().ok();
    let found: Option<(String, i64, String, String, String, i64)> = db
        .query_row(
            &format!(
                "SELECT player_id, number, name, about, look, created FROM accounts
                 WHERE (player_id = ?1 OR name = ?1 OR number = ?2) AND NOT {}",
                owner::banned_sql("accounts.player_id")
            ),
            params![id, number],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let Some((player_id, number, name, about, look, created)) = found else {
        return Err(if recovery::was_moved(&db, &id) { StatusCode::GONE } else { StatusCode::NOT_FOUND });
    };
    let mut statement = db
        .prepare(&format!(
            "SELECT {GAME_COLUMNS} FROM {GAMES} WHERE g.author_id = ?1 AND g.private = 0
             ORDER BY g.plays DESC, g.updated DESC"
        ))
        .expect("the profile query is valid");
    let games = statement
        .query_map(params![player_id], |r| game_row(r, now))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default();
    let admin = owner::is_owner(&db, &player_id);
    let hat_maker = owner::is_hat_maker(&db, &player_id);
    let verified = db
        .query_row(&format!("SELECT {}", security::verified_sql("?1")), params![player_id], |r| r.get(0))
        .unwrap_or(false);
    let (groups, badge) = groups::badges(&db, &player_id);
    Ok(Json(Profile {
        player_id,
        number: number as u64,
        name,
        about,
        look: serde_json::from_str(&look).unwrap_or(serde_json::Value::Null),
        joined_secs_ago: now - created,
        admin,
        verified,
        hat_maker,
        games,
        groups,
        badge,
    }))
}

async fn unpublish(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Json(secret): Json<Secret>,
) -> StatusCode {
    let db = db.lock().unwrap();
    match db
        .execute("DELETE FROM games WHERE game_id = ?1 AND secret = pepper(?2)", params![id, secret.secret])
    {
        Ok(0) => StatusCode::NOT_FOUND,
        Ok(_) => {
            manage::forget(&db, &id);
            copies::forget(&db, &id);
            StatusCode::NO_CONTENT
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// The columns [`server_row`] reads, from `servers`.
const SERVER_COLUMNS: &str = "server_id, name, game, code, players, max_players, kind, last_seen, game_id,
    (SELECT a.name FROM accounts a WHERE a.player_id = servers.host_id), host_id,
    EXISTS (SELECT 1 FROM owners o WHERE o.player_id = servers.host_id),
    EXISTS (SELECT 1 FROM verified v WHERE v.player_id = servers.host_id),
    EXISTS (SELECT 1 FROM fleet f WHERE f.server_id = servers.server_id)";

fn server_row(r: &rusqlite::Row, now: i64) -> rusqlite::Result<ServerInfo> {
    Ok(ServerInfo {
        server_id: r.get(0)?,
        name: r.get(1)?,
        game: r.get(2)?,
        code: r.get(3)?,
        players: r.get::<_, i64>(4)? as u32,
        max_players: r.get::<_, i64>(5)? as u32,
        kind: HostKind::from_rank(r.get(6)?),
        seen_secs_ago: now - r.get::<_, i64>(7)?,
        game_id: r.get(8)?,
        host: r.get(9)?,
        host_id: r.get(10)?,
        admin_host: r.get(11)?,
        host_verified: r.get::<_, bool>(12)? || r.get::<_, bool>(11)?,
        hub_server: r.get(13)?,
    })
}

async fn list_servers(
    State(db): State<Db>,
    State(lists): State<Arc<ServerLists>>,
    Query(query): Query<ListQuery>,
) -> impl axum::response::IntoResponse {
    let json = [(axum::http::header::CONTENT_TYPE, "application/json")];
    let key = (query.q.clone().unwrap_or_default(), query.game_id.clone());
    let db = db.lock().unwrap();
    // (Read while the database is held, so no write can slip in between.)
    // The hook counts this connection's writes; data_version moves when
    // another connection (an admin tool banning someone) commits.
    let others: i64 = db
        .prepare_cached("PRAGMA data_version")
        .and_then(|mut s| s.query_row([], |r| r.get(0)))
        .unwrap_or(-1);
    let version = lists.changed.load(std::sync::atomic::Ordering::Relaxed) ^ (others as u64).rotate_left(32);
    if let Some((v, at, body)) = lists.kept.lock().unwrap().get(&key)
        && *v == version
        && at.elapsed() < LIST_KEPT_FOR
    {
        return (json, body.clone());
    }
    let rows = server_list(&db, query);
    drop(db);
    let body = axum::body::Bytes::from(serde_json::to_vec(&rows).unwrap_or_default());
    let mut kept = lists.kept.lock().unwrap();
    if kept.len() >= 256 {
        kept.clear();
    }
    kept.insert(key, (version, std::time::Instant::now(), body.clone()));
    (json, body)
}

/// The listed servers a search finds, best first.
fn server_list(db: &Connection, query: ListQuery) -> Vec<ServerInfo> {
    let now = now();
    let pattern = format!("%{}%", query.q.unwrap_or_default().trim().replace(['%', '_'], ""));
    let game_id = query.game_id.filter(|g| !g.trim().is_empty());
    let mut statement = db
        .prepare(&format!(
            "SELECT {SERVER_COLUMNS} FROM servers
             WHERE (name LIKE ?1 OR game LIKE ?1) AND (?2 IS NULL OR game_id = ?2) AND last_seen >= ?3
             ORDER BY kind ASC, players DESC, first_seen ASC LIMIT 200"
        ))
        .expect("the list query is valid");
    statement
        .query_map(params![pattern, game_id, now - EXPIRES_AFTER_SECS], |r| server_row(r, now))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

async fn heartbeat(
    State(db): State<Db>,
    Json(beat): Json<Heartbeat>,
) -> Result<StatusCode, (StatusCode, String)> {
    let bad = |why: &str| Err((StatusCode::BAD_REQUEST, why.to_owned()));
    if beat.server_id.trim().is_empty() || beat.secret.len() < 16 {
        return bad("a heartbeat needs a server_id and a secret of 16 or more characters");
    }
    if beat.code.trim().is_empty() {
        return bad("a heartbeat needs a join code");
    }
    let db = db.lock().unwrap();
    let now = now();
    let owner: Option<String> = db
        .query_row("SELECT secret FROM servers WHERE server_id = ?1", params![beat.server_id], |r| r.get(0))
        .optional()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if owner.is_some_and(|s| s != seal::pepper(&db, &beat.secret)) {
        return Err((StatusCode::FORBIDDEN, "another host owns that server_id".into()));
    }
    let host_id = match (&beat.host_id, &beat.host_signature) {
        (Some(id), Some(signature)) => {
            if !verify(id, &host_message(&beat.server_id, &beat.secret), signature) {
                return Err((StatusCode::FORBIDDEN, "the host's signature does not match".into()));
            }
            // The code must reach the computer that signed, or anyone could
            // list their own server under someone else's account.
            if code_host(&beat.code).as_deref() != Some(id.as_str()) {
                return Err((StatusCode::FORBIDDEN, "that join code reaches a different computer".into()));
            }
            Some(id.clone())
        }
        _ => None,
    };
    // Only a signed server goes on a game's own list, so nobody can put
    // their server in front of a popular game's players anonymously.
    if host_id.is_none() && beat.game_id.as_deref().is_some_and(|g| !g.trim().is_empty()) {
        return Err((
            StatusCode::FORBIDDEN,
            "a server that runs a listed game must be signed by its host".into(),
        ));
    }
    if host_id.as_deref().is_some_and(|id| owner::is_banned(&db, id)) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from PlayLibre".into()));
    }
    // Only the Hub's owners (and servers they trust) list Official servers.
    let kind = match beat.kind {
        HostKind::Official
            if !host_id.as_deref().is_some_and(|id| owner::is_owner(&db, id) || fleet::is_fleet(&db, id)) =>
        {
            HostKind::Community
        }
        kind => kind,
    };
    let name = if beat.name.trim().is_empty() { "Unnamed server".to_owned() } else { clip(&beat.name) };
    let game = if beat.game.trim().is_empty() { "Untitled game".to_owned() } else { clip(&beat.game) };
    db.execute(
        "INSERT INTO servers (server_id, secret, name, game, code, players, max_players, kind, first_seen, last_seen,
             game_id, host_id)
         VALUES (?1, pepper(?2), ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10, ?11)
         ON CONFLICT(server_id) DO UPDATE SET name = ?3, game = ?4, code = ?5, players = ?6,
             max_players = ?7, kind = ?8, last_seen = ?9, game_id = ?10, host_id = ?11",
        params![
            clip(&beat.server_id),
            beat.secret,
            name,
            game,
            clip(&beat.code),
            beat.players,
            manage::most_players(&db, beat.game_id.as_deref().map(str::trim), beat.max_players),
            kind.rank(),
            now,
            beat.game_id.as_deref().map(str::trim).filter(|g| !g.is_empty()).map(clip),
            host_id
        ],
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Play: finds or starts one of the Hub's own servers of a game (see
/// [`FleetOptions`]). A Hub that runs none answers 404, and the app falls
/// back to the server list.
async fn play(
    State(db): State<Db>,
    State(fleet): State<Option<Arc<fleet::Fleet>>>,
    Json(request): Json<PlayRequest>,
) -> Result<Json<PlayAnswer>, (StatusCode, String)> {
    let db = db.lock().unwrap();
    if let (Some(id), Some(signature)) = (&request.player_id, &request.signature)
        && verify(id, &play_message(id, &request.ticket), signature)
        && let Some(why) = family::playtime_up(&db, id)
    {
        return Err((StatusCode::FORBIDDEN, why));
    }
    let Some(fleet) = fleet else {
        return Err((StatusCode::NOT_FOUND, "this Hub doesn't run game servers".into()));
    };
    fleet.play(&db, &request).map(Json).map_err(|e| match e {
        fleet::PlayError::BadRequest(why) => (StatusCode::BAD_REQUEST, why.to_owned()),
        fleet::PlayError::UnknownGame => (StatusCode::NOT_FOUND, "the Hub doesn't know that game".into()),
        fleet::PlayError::Busy => {
            (StatusCode::SERVICE_UNAVAILABLE, "every game server is busy; try again in a minute".into())
        }
        fleet::PlayError::Failed => {
            (StatusCode::BAD_GATEWAY, "the Hub couldn't start a server of that game".into())
        }
    })
}

async fn remove_server(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Json(secret): Json<Secret>,
) -> StatusCode {
    let db = db.lock().unwrap();
    match db.execute(
        "DELETE FROM servers WHERE server_id = ?1 AND secret = pepper(?2)",
        params![id, secret.secret],
    ) {
        Ok(0) => StatusCode::NOT_FOUND,
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// A Hub running in the background. Dropping it stops the Hub.
pub struct RunningHub {
    addr: SocketAddr,
    _stop: tokio::sync::oneshot::Sender<()>,
}

impl RunningHub {
    /// Where the Hub listens.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The address clients use, such as `http://127.0.0.1:7430`.
    pub fn url(&self) -> String {
        let host =
            if self.addr.ip().is_unspecified() { "127.0.0.1".to_owned() } else { self.addr.ip().to_string() };
        format!("http://{host}:{}", self.addr.port())
    }
}

/// Starts a Hub on `addr` in a background thread (port 0 picks a free one).
pub fn start(addr: SocketAddr, db_path: Option<&Path>) -> Result<RunningHub, String> {
    start_with(addr, db_path, HubOptions::default())
}

/// [`start`], run as `options` say.
pub fn start_with(
    addr: SocketAddr,
    db_path: Option<&Path>,
    options: HubOptions,
) -> Result<RunningHub, String> {
    let db = open_db(db_path)?;
    let listener =
        std::net::TcpListener::bind(addr).map_err(|e| format!("could not start the Hub on {addr}: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let mut options = options;
    fleet_finds(&mut options, addr);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("playlibre-hub")
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    std::thread::Builder::new()
        .name("playlibre-hub".into())
        .spawn(move || {
            runtime.block_on(async move {
                let Ok(listener) = tokio::net::TcpListener::from_std(listener) else { return };
                let app = router_with(db, options).into_make_service_with_connect_info::<SocketAddr>();
                let _ = axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        let _ = stopped.await;
                    })
                    .await;
            });
        })
        .map_err(|e| e.to_string())?;
    Ok(RunningHub { addr, _stop: stop })
}

/// Tells the Hub's own game servers where to find it: on this computer, at
/// `addr`'s port.
fn fleet_finds(options: &mut HubOptions, addr: SocketAddr) {
    if let Some(fleet) = options.fleet.as_mut().filter(|f| f.hub_url.is_empty()) {
        fleet.hub_url = format!("http://127.0.0.1:{}", addr.port());
    }
}

/// Runs a Hub on `addr` until Ctrl+C, printing what it does.
pub fn run(addr: SocketAddr, db_path: &Path, options: HubOptions) -> Result<(), String> {
    let mut options = options;
    fleet_finds(&mut options, addr);
    let db = open_db(Some(db_path))?;
    if let Some(program) = options.starters.as_ref().or(options.fleet.as_ref().map(|f| &f.program)) {
        match official_from(program).and_then(|games| set_official(&db, &games).map(|_| games.len())) {
            Ok(n) => println!("{n} official games from {}", program.display()),
            Err(e) => println!("No official games from {}: {e}", program.display()),
        }
    }
    let runtime =
        tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("could not start the Hub on {addr}: {e}"))?;
        println!("PlayLibre Hub listening on http://{addr} (data in {})", db_path.display());
        if let Some(fleet) = &options.fleet {
            println!(
                "Running up to {} game servers of {} players with {} (UDP ports from {})",
                fleet.max_servers,
                fleet.players_per_server,
                fleet.program.display(),
                fleet.first_udp_port
            );
        }
        if options.behind_proxy {
            println!("Trusting X-Forwarded-For from a web server on this computer for who is calling.");
        }
        match &options.mail {
            Mail::Smtp(smtp) => {
                println!("Mailing email codes through {}:{}, from {}.", smtp.host, smtp.port, smtp.from)
            }
            _ => println!("No mail settings (PLAYLIBRE_SMTP_HOST): emails are kept, but no codes are sent."),
        }
        println!("Try it from an app on this computer with PLAYLIBRE_HUB=http://127.0.0.1:{}", addr.port());
        axum::serve(listener, router_with(db, options).into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
                println!("Hub stopped.");
            })
            .await
            .map_err(|e| e.to_string())
    })
}

// ---------------------------------------------------------------------------
// The client
// ---------------------------------------------------------------------------

/// Talks to one Hub. Every call blocks, so call it off the UI thread.
#[derive(Clone)]
pub struct HubClient {
    base: String,
    agent: ureq::Agent,
    /// For calls that carry a whole game and may take a while.
    slow: ureq::Agent,
}

impl HubClient {
    /// A client for the Hub at `url` (`http://host:port`; a bare host gets
    /// `http://` and the default port).
    pub fn new(url: &str) -> Self {
        let url = url.trim().trim_end_matches('/');
        let url = if url.contains("://") { url.to_owned() } else { format!("http://{url}") };
        let has_port = url.rsplit_once(':').is_some_and(|(head, port)| {
            !head.ends_with("http") && !head.ends_with("https") && port.parse::<u16>().is_ok()
        });
        // HTTPS uses its usual port: a Hub reached that way is behind a web server.
        let base =
            if has_port || url.starts_with("https://") { url } else { format!("{url}:{DEFAULT_PORT}") };
        let local = base.contains("://127.") || base.contains("://localhost") || base.contains("://[::1]");
        let agent = |timeout: u64| {
            let mut config = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(timeout)))
                .http_status_as_error(true);
            if local {
                // A Hub on this computer is never behind a proxy.
                config = config.proxy(None);
            }
            ureq::Agent::from(config.build())
        };
        Self { base, agent: agent(5), slow: agent(120) }
    }

    /// The Hub's address.
    pub fn url(&self) -> &str {
        &self.base
    }

    /// Whether a Hub answers, and how busy it is.
    pub fn status(&self) -> Result<HubStatus, String> {
        self.agent
            .get(&self.base)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    /// Live servers, community hosts first, matching `search` if given.
    pub fn servers(&self, search: &str) -> Result<Vec<ServerInfo>, String> {
        self.agent
            .get(format!("{}/servers", self.base))
            .query("q", search)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    /// The live servers running one game (see [`Heartbeat::game_id`]).
    pub fn game_servers(&self, game_id: &str) -> Result<Vec<ServerInfo>, String> {
        self.agent
            .get(format!("{}/servers", self.base))
            .query("game_id", game_id)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    /// Announces a host; repeat every [`HEARTBEAT_EVERY`].
    pub fn heartbeat(&self, beat: &Heartbeat) -> Result<(), String> {
        self.agent
            .post(format!("{}/servers/heartbeat", self.base))
            .send_json(beat)
            .map(|_| ())
            .map_err(|e| format!("the Hub did not take the announcement: {e}"))
    }

    /// Takes a host off the list straight away.
    pub fn remove(&self, server_id: &str, secret: &str) -> Result<(), String> {
        self.agent
            .delete(format!("{}/servers/{server_id}", self.base))
            .force_send_body()
            .send_json(serde_json::json!({ "secret": secret }))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

impl HubClient {
    /// Asks the Hub for one of its own servers of a game to play on (see
    /// [`PlayAnswer`]). `Ok(None)`: this Hub runs no game servers.
    pub fn play(&self, request: &PlayRequest) -> Result<Option<PlayAnswer>, String> {
        let mut response = self
            .agent
            .post(format!("{}/play", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(request)
            .map_err(|e| format!("could not reach the Hub: {e}"))?;
        if response.status().is_success() {
            return response.body_mut().read_json().map(Some).map_err(|e| e.to_string());
        }
        let status = response.status().as_u16();
        let why = response.body_mut().read_to_string().unwrap_or_default();
        // Hubs from before game servers don't know /play at all.
        if status == 404 && (why.trim().is_empty() || why.contains("doesn't run game servers")) {
            return Ok(None);
        }
        Err(if why.trim().is_empty() { format!("the Hub said {status}") } else { why })
    }

    /// Published games, most played first, matching `search` if given.
    pub fn games(&self, search: &str) -> Result<Vec<GameInfo>, String> {
        self.agent
            .get(format!("{}/games", self.base))
            .query("q", search)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    /// Publishes a game, or a new version of one.
    pub fn publish(&self, game: &Publish) -> Result<Published, String> {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        serde_json::to_writer(&mut gz, game).map_err(|e| e.to_string())?;
        let body = gz.finish().map_err(|e| e.to_string())?;
        self.slow
            .post(format!("{}/games", self.base))
            .header("content-type", "application/json")
            .header("content-encoding", "gzip")
            .config()
            .http_status_as_error(false)
            .build()
            .send(&body[..])
            .map_err(|e| format!("could not reach the Hub: {e}"))
            .and_then(|mut r| {
                if r.status().is_success() {
                    r.body_mut().read_json().map_err(|e| e.to_string())
                } else {
                    let why = r.body_mut().read_to_string().unwrap_or_default();
                    Err(format!("the Hub did not take it: {why}"))
                }
            })
    }

    /// One published game.
    pub fn game(&self, game_id: &str) -> Result<GameInfo, String> {
        self.agent
            .get(format!("{}/games/{game_id}", self.base))
            .call()
            .map_err(|e| format!("could not find the game: {e}"))?
            .body_mut()
            .read_json()
            .map_err(|e| e.to_string())
    }

    /// A published game's saved place.
    pub fn download(&self, game_id: &str, proof: Option<&DownloadProof>) -> Result<String, String> {
        let mut request = self.slow.get(format!("{}/games/{game_id}/place", self.base));
        if let Some(proof) = proof {
            request = request
                .header("x-libroxel-player", &proof.player_id)
                .header("x-libroxel-time", proof.time.to_string())
                .header("x-libroxel-signature", &proof.signature);
        }
        let mut response = request
            .config()
            .http_status_as_error(false)
            .build()
            .call()
            .map_err(|e| format!("could not download the game: {e}"))?;
        let ok = response.status().is_success();
        let text = response
            .body_mut()
            .with_config()
            .limit(MAX_PLACE as u64 + 1)
            .read_to_string()
            .map_err(|e| format!("could not download the game: {e}"))?;
        if ok { Ok(text) } else { Err(format!("could not download the game: {text}")) }
    }

    /// Claims or changes an account.
    pub fn update_account(&self, account: &AccountUpdate) -> Result<(), String> {
        self.agent
            .post(format!("{}/accounts", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(account)
            .map_err(|e| format!("could not reach the Hub: {e}"))
            .and_then(|mut r| {
                if r.status().is_success() {
                    Ok(())
                } else {
                    Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
                }
            })
    }

    /// Sets or changes this account's password (see [`seal_password`]).
    pub fn set_password(&self, set: &PasswordSet) -> Result<(), String> {
        self.agent
            .post(format!("{}/accounts/password", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(set)
            .map_err(|e| format!("could not reach the Hub: {e}"))
            .and_then(|mut r| {
                if r.status().is_success() {
                    Ok(())
                } else {
                    Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
                }
            })
    }

    /// Signs in to the account called `name` with its password, and gives
    /// back its identity key (32 bytes) to keep on this computer. The
    /// password is stretched here and never sent. Takes a moment.
    pub fn sign_in(&self, name: &str, password: &str, code: Option<&str>) -> Result<[u8; 32], String> {
        self.sign_in_on(name, password, code, None).map(|(key, _)| key)
    }

    /// [`Self::sign_in`], naming the device; gives back the key and the session.
    pub fn sign_in_on(
        &self,
        name: &str,
        password: &str,
        code: Option<&str>,
        device: Option<&str>,
    ) -> Result<([u8; 32], String), String> {
        let salt = self.salt_for(name)?;
        let time = social::now_ms();
        let (mut attempt, seal) = password::sign_in_with(name.trim(), password, &salt, time)?;
        attempt.code = code.map(str::to_owned);
        attempt.device = device.map(str::to_owned);
        let signed_in: SignedIn = self.post_signed("/accounts/sign-in", &attempt)?;
        let key = password::open_sealed(&seal, &signed_in)?;
        Ok((key, signed_in.session))
    }

    fn salt_for(&self, name: &str) -> Result<SignInSalt, String> {
        let name = name.trim();
        let answer = |r: Result<ureq::http::Response<ureq::Body>, ureq::Error>| {
            let mut r = r.map_err(|e| format!("could not reach the Hub: {e}"))?;
            if r.status().is_success() {
                Ok(r)
            } else {
                Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
            }
        };
        let salt: SignInSalt = answer(
            self.agent
                .get(format!("{}/accounts/sign-in/{name}", self.base))
                .config()
                .http_status_as_error(false)
                .build()
                .call(),
        )?
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())?;
        Ok(salt)
    }

    /// New recovery codes for an account with two-step sign-in.
    pub fn recovery_codes(&self, key: &[u8; 32]) -> Result<Vec<String>, String> {
        let player_id = player_id_of(key);
        let time = social::now_ms();
        let ask = RecoveryAsk { signature: sign(key, &recovery_message(&player_id, time)), player_id, time };
        self.post_signed::<_, RecoveryCodes>("/accounts/recovery-codes", &ask).map(|r| r.codes)
    }

    /// Names this device in the account's list, giving back the session.
    pub fn start_session(&self, key: &[u8; 32], device: &str) -> Result<String, String> {
        let player_id = player_id_of(key);
        let time = social::now_ms();
        let start = SessionStart {
            signature: sign(key, &session_message(&player_id, device, time)),
            player_id,
            device: device.to_owned(),
            time,
        };
        self.post_signed::<_, Session>("/accounts/session", &start).map(|s| s.session)
    }

    /// Mails a code for resetting the password of the account called `name`.
    pub fn ask_reset(&self, name: &str) -> Result<(), String> {
        self.post_raw("/accounts/reset", &ResetAsk { name: name.trim().to_owned() }).map(|_| ())
    }

    /// Resets the password with the mailed code. The account moves to a new
    /// key, given back with the session; every other device is signed out.
    pub fn finish_reset(
        &self,
        name: &str,
        code: &str,
        two_step: Option<&str>,
        new_password: &str,
        device: Option<&str>,
    ) -> Result<([u8; 32], String), String> {
        let key = password::new_key();
        let password = seal_password(&key, new_password, social::now_ms() / 1000)?;
        let reset = ResetFinish {
            name: name.trim().to_owned(),
            code: code.trim().to_owned(),
            two_step: two_step.map(str::to_owned),
            password,
            device: device.map(str::to_owned),
        };
        let session: Session = self.post_signed("/accounts/reset/finish", &reset)?;
        Ok((key, session.session))
    }

    /// Signs out every other device by moving the account to a new key,
    /// given back with the session. Needs the password (and a two-step code).
    pub fn sign_out_everywhere(
        &self,
        key: &[u8; 32],
        name: &str,
        password: &str,
        code: Option<&str>,
        device: Option<&str>,
    ) -> Result<([u8; 32], String), String> {
        let salt = self.salt_for(name)?;
        let login = password::login_key(password, &salt)?;
        let new = password::new_key();
        let new_password = seal_password(&new, password, social::now_ms() / 1000)?;
        let player_id = player_id_of(key);
        let time = social::now_ms();
        let message = sign_out_message(&player_id, &new_password, time);
        let out = SignOutEverywhere {
            signature: sign(key, &message),
            login_signature: sign(&login, &message),
            player_id,
            password: new_password,
            code: code.map(str::to_owned),
            device: device.map(str::to_owned),
            time,
        };
        let session: Session = self.post_signed("/accounts/sign-out-everywhere", &out)?;
        Ok((new, session.session))
    }

    /// Turns two-step sign-in on or off (see [`TwoStepSet`]).
    /// Turning it on gives back the recovery codes.
    pub fn set_two_step(&self, set: &TwoStepSet) -> Result<Vec<String>, String> {
        self.post_signed::<_, RecoveryCodes>("/accounts/two-step", set).map(|r| r.codes)
    }

    /// How a player's account is kept safe, with [`security_message`] for
    /// `time` (milliseconds) signed by that player.
    pub fn security(&self, player_id: &str, time: i64, signature: &str) -> Result<Security, String> {
        self.security_on(player_id, time, signature, None)
    }

    /// [`Self::security`], marking this device's `session`. A signed-out key gets [`SIGNED_OUT`].
    pub fn security_on(
        &self,
        player_id: &str,
        time: i64,
        signature: &str,
        session: Option<&str>,
    ) -> Result<Security, String> {
        let mut r = self
            .agent
            .get(format!("{}/accounts/{player_id}/security", self.base))
            .query("time", time.to_string())
            .query("signature", signature)
            .query("session", session.unwrap_or(""))
            .config()
            .http_status_as_error(false)
            .build()
            .call()
            .map_err(|e| format!("could not reach the Hub: {e}"))?;
        match r.status().as_u16() {
            410 => Err(SIGNED_OUT.into()),
            s if (200..300).contains(&s) => r.body_mut().read_json().map_err(|e| e.to_string()),
            _ => Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string())),
        }
    }

    /// Gives the account an email, which mails it a code (see [`Self::verify_email`]).
    pub fn set_email(&self, set: &EmailSet) -> Result<EmailSent, String> {
        self.agent
            .post(format!("{}/accounts/email", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(set)
            .map_err(|e| format!("could not reach the Hub: {e}"))
            .and_then(|mut r| {
                if r.status().is_success() {
                    r.body_mut().read_json().map_err(|e| e.to_string())
                } else {
                    Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
                }
            })
    }

    fn post_raw<T: Serialize>(&self, path: &str, body: &T) -> Result<String, String> {
        self.agent
            .post(format!("{}{path}", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(body)
            .map_err(|e| format!("could not reach the Hub: {e}"))
            .and_then(|mut r| {
                let text = r.body_mut().read_to_string().unwrap_or_default();
                if r.status().is_success() {
                    Ok(text)
                } else if text.is_empty() {
                    Err(r.status().to_string())
                } else {
                    Err(text)
                }
            })
    }

    fn post_signed<T: Serialize, R: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<R, String> {
        serde_json::from_str(&self.post_raw(path, body)?).map_err(|e| e.to_string())
    }

    pub fn set_birthday(&self, birthday: &Birthday) -> Result<Family, String> {
        self.post_signed("/accounts/birthday", birthday)
    }

    pub fn ask_parent(&self, ask: &ParentEmail) -> Result<(), String> {
        self.post_raw("/accounts/family/parent", ask).map(|_| ())
    }

    pub fn consent(&self, consent: &Consent) -> Result<Family, String> {
        self.post_signed("/accounts/family/consent", consent)
    }

    pub fn change_family(&self, change: &FamilyChange) -> Result<Family, String> {
        self.post_signed("/accounts/family", change)
    }

    pub fn family(&self, player_id: &str, time: i64, signature: &str) -> Result<Family, String> {
        self.agent
            .get(format!("{}/accounts/{player_id}/family", self.base))
            .query("time", time.to_string())
            .query("signature", signature)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not get parent settings from the Hub: {e}"))
    }

    pub fn chat_rule(&self, player_id: &str) -> Result<ChatRule, String> {
        self.agent
            .get(format!("{}/accounts/{player_id}/chat", self.base))
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    /// Proves the account's email with the code from the mail.
    pub fn verify_email(&self, code: &EmailCode) -> Result<(), String> {
        self.agent
            .post(format!("{}/accounts/email/verify", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(code)
            .map_err(|e| format!("could not reach the Hub: {e}"))
            .and_then(|mut r| {
                if r.status().is_success() {
                    Ok(())
                } else {
                    Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
                }
            })
    }

    /// A player's page, by player id or account name.
    pub fn profile(&self, who: &str) -> Result<Option<Profile>, String> {
        let result = self
            .agent
            .get(format!("{}/accounts/{who}", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .call()
            .map_err(|e| format!("could not reach the Hub: {e}"))?;
        let mut result = result;
        match result.status().as_u16() {
            404 => Ok(None),
            410 => Err(SIGNED_OUT.into()),
            s if (200..300).contains(&s) => {
                result.body_mut().read_json().map(Some).map_err(|e| e.to_string())
            }
            s => Err(format!("the Hub said {s}")),
        }
    }

    /// Shares something to wear in the catalog.
    pub fn share_item(&self, item: &NewItem) -> Result<Item, String> {
        self.agent
            .post(format!("{}/catalog", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(item)
            .map_err(|e| format!("could not reach the Hub: {e}"))
            .and_then(|mut r| {
                if r.status().is_success() {
                    r.body_mut().read_json().map_err(|e| e.to_string())
                } else {
                    Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
                }
            })
    }

    /// The catalog, most worn first, matching `search` if given.
    pub fn catalog(&self, search: &str) -> Result<Vec<Item>, String> {
        self.agent
            .get(format!("{}/catalog", self.base))
            .query("q", search)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    pub fn wallet(&self, player_id: &str, time: i64, signature: &str) -> Result<Wallet, String> {
        let request = self
            .agent
            .get(format!("{}/wallet/{player_id}", self.base))
            .query("time", time.to_string())
            .query("signature", signature)
            .config()
            .http_status_as_error(false)
            .build()
            .call();
        self.answer(request)
    }

    pub fn buy_item(&self, buy: &Buy) -> Result<Wallet, String> {
        self.post_signed("/shop/buy", buy)
    }

    pub fn set_price(&self, item_id: &str, price: &SetPrice) -> Result<(), String> {
        self.post_raw(&format!("/catalog/{item_id}/price"), price).map(|_| ())
    }

    pub fn earnings(&self, player_id: &str, time: i64, signature: &str) -> Result<Earnings, String> {
        let request = self
            .agent
            .get(format!("{}/earnings/{player_id}", self.base))
            .query("time", time.to_string())
            .query("signature", signature)
            .config()
            .http_status_as_error(false)
            .build()
            .call();
        self.answer(request)
    }

    pub fn cash_out(&self, cash_out: &CashOut) -> Result<Earnings, String> {
        self.post_signed("/earnings/cash-out", cash_out)
    }

    pub fn passes(&self, game_id: &str, player_id: Option<&str>) -> Result<Vec<Pass>, String> {
        let mut request = self.agent.get(format!("{}/games/{game_id}/passes", self.base));
        if let Some(player) = player_id {
            request = request.query("player", player);
        }
        self.answer(request.config().http_status_as_error(false).build().call())
    }

    pub fn change_pass(&self, game_id: &str, change: &PassChange) -> Result<Vec<Pass>, String> {
        self.post_signed(&format!("/games/{game_id}/passes"), change)
    }

    pub fn buy_pass(&self, buy: &BuyPass) -> Result<Pass, String> {
        self.post_signed("/passes/buy", buy)
    }

    pub fn owns_pass(&self, pass_id: i64, player_id: &str) -> Result<bool, String> {
        let request = self
            .agent
            .get(format!("{}/passes/{pass_id}/owners/{player_id}", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .call();
        self.answer(request)
    }

    pub fn market(
        &self,
        search: &str,
        kind: Option<AssetKind>,
        creator: Option<&str>,
    ) -> Result<Vec<Asset>, String> {
        let mut request = self.agent.get(format!("{}/market", self.base)).query("q", search);
        if let Some(kind) = kind {
            request = request.query("kind", kind.word());
        }
        if let Some(creator) = creator {
            request = request.query("creator", creator);
        }
        request
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    pub fn market_asset(&self, asset_id: &str) -> Result<Asset, String> {
        let request = self
            .agent
            .get(format!("{}/market/{}", self.base, assets::encode(asset_id)))
            .config()
            .http_status_as_error(false)
            .build()
            .call();
        self.answer(request)
    }

    pub fn market_data(&self, asset_id: &str) -> Result<AssetData, String> {
        let request = self
            .slow
            .get(format!("{}/market/{}/data", self.base, assets::encode(asset_id)))
            .config()
            .http_status_as_error(false)
            .build()
            .call();
        self.answer(request)
    }

    pub fn publish_asset(&self, asset: &NewAsset) -> Result<Asset, String> {
        let request = self
            .slow
            .post(format!("{}/market", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(asset);
        self.answer(request)
    }

    pub fn unmarket(&self, asset_id: &str, signed: &Unmarket) -> Result<(), String> {
        let mut r = self
            .agent
            .delete(format!("{}/market/{}", self.base, assets::encode(asset_id)))
            .config()
            .http_status_as_error(false)
            .build()
            .force_send_body()
            .send_json(signed)
            .map_err(|e| format!("could not reach the Hub: {e}"))?;
        if r.status().is_success() {
            Ok(())
        } else {
            Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
        }
    }

    /// Counts someone putting an item on.
    pub fn wore(&self, item_id: &str) -> Result<(), String> {
        self.agent
            .post(format!("{}/catalog/{item_id}/wear", self.base))
            .send_empty()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Takes an item out of the catalog, with [`unlist_message`] signed by its creator.
    pub fn unlist(&self, item_id: &str, signature: &str) -> Result<(), String> {
        self.agent
            .delete(format!("{}/catalog/{item_id}", self.base))
            .force_send_body()
            .send_json(serde_json::json!({ "secret": signature }))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn dashboard(&self, moderator_id: &str, time: i64, signature: &str) -> Result<Dashboard, String> {
        let request = self
            .agent
            .get(format!("{}/moderation/{moderator_id}", self.base))
            .query("time", time.to_string())
            .query("signature", signature)
            .config()
            .http_status_as_error(false)
            .build()
            .call();
        self.answer(request)
    }

    pub fn moderate(&self, change: &ModChange) -> Result<(), String> {
        let mut r = self
            .agent
            .post(format!("{}/moderation", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(change)
            .map_err(|e| format!("could not reach the Hub: {e}"))?;
        if r.status().is_success() {
            Ok(())
        } else {
            Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
        }
    }

    /// Reports a player, game or item as not OK.
    pub fn report(&self, report: &Report) -> Result<(), String> {
        let mut r = self
            .agent
            .post(format!("{}/reports", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(report)
            .map_err(|e| format!("could not reach the Hub: {e}"))?;
        if r.status().is_success() {
            Ok(())
        } else {
            Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
        }
    }

    /// Asks the Hub's owners to lift a ban, mute or warning.
    pub fn appeal(&self, ask: &AppealAsk) -> Result<(), String> {
        let mut r = self
            .agent
            .post(format!("{}/appeals", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(ask)
            .map_err(|e| format!("could not reach the Hub: {e}"))?;
        if r.status().is_success() {
            Ok(())
        } else {
            Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
        }
    }

    fn answer<T: serde::de::DeserializeOwned>(
        &self,
        request: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<T, String> {
        let mut r = request.map_err(|e| format!("could not reach the Hub: {e}"))?;
        if r.status().is_success() {
            r.body_mut().read_json().map_err(|e| e.to_string())
        } else {
            Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
        }
    }

    /// A game's likes and favorites, with what `player_id` gave it.
    pub fn ratings(&self, game_id: &str, player_id: Option<&str>) -> Result<Ratings, String> {
        let mut request = self.agent.get(format!("{}/games/{}/ratings", self.base, assets::encode(game_id)));
        if let Some(id) = player_id {
            request = request.query("player_id", id);
        }
        request
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    pub fn rate(&self, game_id: &str, rate: &Rate) -> Result<Ratings, String> {
        let request = self
            .agent
            .post(format!("{}/games/{}/ratings", self.base, assets::encode(game_id)))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(rate);
        self.answer(request)
    }

    pub fn comments(&self, game_id: &str) -> Result<Vec<Comment>, String> {
        self.agent
            .get(format!("{}/games/{}/comments", self.base, assets::encode(game_id)))
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    pub fn comment(&self, game_id: &str, comment: &NewComment) -> Result<Comment, String> {
        let request = self
            .agent
            .post(format!("{}/games/{}/comments", self.base, assets::encode(game_id)))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(comment);
        self.answer(request)
    }

    pub fn remove_comment(&self, comment_id: i64, remove: &RemoveComment) -> Result<(), String> {
        self.agent
            .delete(format!("{}/comments/{comment_id}", self.base))
            .force_send_body()
            .send_json(remove)
            .map(|_| ())
            .map_err(|e| format!("the Hub did not take the comment down: {e}"))
    }

    /// The games a player favorited, newest first.
    pub fn favorites(&self, player_id: &str) -> Result<Vec<String>, String> {
        self.agent
            .get(format!("{}/accounts/{player_id}/favorites", self.base))
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    /// Players whose names match `search`.
    pub fn people(&self, search: &str) -> Result<Vec<Person>, String> {
        self.agent
            .get(format!("{}/accounts", self.base))
            .query("q", search)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    pub fn groups(&self, search: &str) -> Result<Vec<GroupInfo>, String> {
        self.agent
            .get(format!("{}/groups", self.base))
            .query("q", search)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    pub fn group(&self, group_id: &str) -> Result<GroupPage, String> {
        self.agent
            .get(format!("{}/groups/{group_id}", self.base))
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not get that group: {e}"))
    }

    pub fn group_as(
        &self,
        group_id: &str,
        viewer: &str,
        time: i64,
        signature: &str,
    ) -> Result<GroupPage, String> {
        self.agent
            .get(format!("{}/groups/{group_id}", self.base))
            .query("viewer", viewer)
            .query("time", time.to_string())
            .query("signature", signature)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not get that group: {e}"))
    }

    pub fn group_invites(
        &self,
        player_id: &str,
        time: i64,
        signature: &str,
    ) -> Result<Vec<GroupInfo>, String> {
        self.agent
            .get(format!("{}/groups/invites/{player_id}", self.base))
            .query("time", time.to_string())
            .query("signature", signature)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not get your group invites: {e}"))
    }

    pub fn change_group(&self, change: &GroupChange) -> Result<GroupChanged, String> {
        self.agent
            .post(format!("{}/groups", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(change)
            .map_err(|e| format!("could not reach the Hub: {e}"))
            .and_then(|mut r| {
                if r.status().is_success() {
                    r.body_mut().read_json().map_err(|e| e.to_string())
                } else {
                    Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
                }
            })
    }

    /// Sends, accepts, declines or ends a friendship.
    pub fn change_friend(&self, change: &FriendChange) -> Result<FriendState, String> {
        self.agent
            .post(format!("{}/friends", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(change)
            .map_err(|e| format!("could not reach the Hub: {e}"))
            .and_then(|mut r| {
                if r.status().is_success() {
                    r.body_mut().read_json().map_err(|e| e.to_string())
                } else {
                    Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
                }
            })
    }

    /// A player's friends (with where they are) and requests, with
    /// [`friends_list_message`] for `time` signed by that player.
    pub fn friends(&self, player_id: &str, time: i64, signature: &str) -> Result<Friends, String> {
        self.agent
            .get(format!("{}/friends/{player_id}", self.base))
            .query("time", time.to_string())
            .query("signature", signature)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not get your friends from the Hub: {e}"))
    }

    pub fn change_party(&self, change: &PartyChange) -> Result<PartyView, String> {
        self.agent
            .post(format!("{}/parties", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(change)
            .map_err(|e| format!("could not reach the Hub: {e}"))
            .and_then(|mut r| {
                if r.status().is_success() {
                    r.body_mut().read_json().map_err(|e| e.to_string())
                } else {
                    Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
                }
            })
    }

    pub fn party(&self, player_id: &str, time: i64, signature: &str) -> Result<PartyView, String> {
        self.agent
            .get(format!("{}/parties/{player_id}", self.base))
            .query("time", time.to_string())
            .query("signature", signature)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not get your party from the Hub: {e}"))
    }

    pub fn badges(&self, who: &str) -> Result<Vec<Badge>, String> {
        self.agent
            .get(format!("{}/accounts/{who}/badges", self.base))
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not get badges from the Hub: {e}"))
    }

    pub fn game_badges(&self, game_id: &str) -> Result<Vec<Badge>, String> {
        self.agent
            .get(format!("{}/games/{}/badges", self.base, assets::encode(game_id)))
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not get badges from the Hub: {e}"))
    }

    pub fn change_badge(&self, game_id: &str, change: &BadgeChange) -> Result<Vec<Badge>, String> {
        self.agent
            .post(format!("{}/games/{}/badges", self.base, assets::encode(game_id)))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(change)
            .map_err(|e| format!("could not reach the Hub: {e}"))
            .and_then(|mut r| {
                if r.status().is_success() {
                    r.body_mut().read_json().map_err(|e| e.to_string())
                } else {
                    Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
                }
            })
    }

    pub fn award_badge(&self, award: &BadgeAward) -> Result<Awarded, String> {
        self.agent
            .post(format!("{}/badges/award", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(award)
            .map_err(|e| format!("could not reach the Hub: {e}"))
            .and_then(|mut r| {
                if r.status().is_success() {
                    r.body_mut().read_json().map_err(|e| e.to_string())
                } else {
                    Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
                }
            })
    }

    pub fn charts(&self) -> Result<Charts, String> {
        self.agent
            .get(format!("{}/charts", self.base))
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    pub fn for_you(&self, player_id: &str, time: i64, signature: &str) -> Result<ForYou, String> {
        self.agent
            .get(format!("{}/for-you/{player_id}", self.base))
            .query("time", time.to_string())
            .query("signature", signature)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    pub fn ping(&self, ping: &Ping) -> Result<(), String> {
        self.agent
            .post(format!("{}/stats/ping", self.base))
            .send_json(ping)
            .map(|_| ())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    pub fn send_crash(&self, crash: &Crash) -> Result<(), String> {
        self.agent
            .post(format!("{}/stats/crash", self.base))
            .send_json(crash)
            .map(|_| ())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    pub fn usage(&self, player_id: &str, time: i64, signature: &str, days: i64) -> Result<Usage, String> {
        self.agent
            .get(format!("{}/admin/{player_id}", self.base))
            .query("time", time.to_string())
            .query("signature", signature)
            .query("days", days.to_string())
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub: {e}"))
    }

    /// Says whether this player is online and what they are playing.
    pub fn presence(&self, update: &PresenceUpdate) -> Result<(), String> {
        self.agent
            .post(format!("{}/presence", self.base))
            .send_json(update)
            .map(|_| ())
            .map_err(|e| format!("the Hub did not take your status: {e}"))
    }

    /// Sends a message to a friend.
    pub fn send_message(&self, message: &NewMessage) -> Result<Message, String> {
        self.agent
            .post(format!("{}/messages", self.base))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(message)
            .map_err(|e| format!("could not reach the Hub: {e}"))
            .and_then(|mut r| {
                if r.status().is_success() {
                    r.body_mut().read_json().map_err(|e| e.to_string())
                } else {
                    Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
                }
            })
    }

    /// Everyone a player has messages with, newest first, with
    /// [`read_messages_message`] for `time` signed by that player.
    pub fn conversations(
        &self,
        player_id: &str,
        time: i64,
        signature: &str,
    ) -> Result<Vec<Conversation>, String> {
        self.agent
            .get(format!("{}/messages/{player_id}", self.base))
            .query("time", time.to_string())
            .query("signature", signature)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not get your messages from the Hub: {e}"))
    }

    /// The latest messages between a player and `other`, oldest first; the
    /// ones from `other` count as read.
    pub fn conversation(
        &self,
        player_id: &str,
        other: &str,
        time: i64,
        signature: &str,
    ) -> Result<Vec<Message>, String> {
        self.agent
            .get(format!("{}/messages/{player_id}/{other}", self.base))
            .query("time", time.to_string())
            .query("signature", signature)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not get your messages from the Hub: {e}"))
    }

    pub fn web_presence(&self, update: &WebPresence) -> Result<(), String> {
        self.agent
            .post(format!("{}/presence/web", self.base))
            .send_json(update)
            .map(|_| ())
            .map_err(|e| format!("the Hub did not take your status: {e}"))
    }

    /// Takes a published game down.
    pub fn unpublish(&self, game_id: &str, secret: &str) -> Result<(), String> {
        self.agent
            .delete(format!("{}/games/{game_id}", self.base))
            .force_send_body()
            .send_json(serde_json::json!({ "secret": secret }))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn my_games(&self, player_id: &str, time: i64, signature: &str) -> Result<Vec<GameInfo>, String> {
        let request = self
            .agent
            .get(format!("{}/creator/{}/games", self.base, assets::encode(player_id)))
            .query("time", time.to_string())
            .query("signature", signature)
            .config()
            .http_status_as_error(false)
            .build()
            .call();
        self.answer(request)
    }

    pub fn versions(
        &self,
        game_id: &str,
        player_id: &str,
        time: i64,
        signature: &str,
    ) -> Result<Vec<Version>, String> {
        let request = self
            .agent
            .get(format!("{}/games/{}/versions", self.base, assets::encode(game_id)))
            .query("player_id", player_id)
            .query("time", time.to_string())
            .query("signature", signature)
            .config()
            .http_status_as_error(false)
            .build()
            .call();
        self.answer(request)
    }

    pub fn manage(&self, game_id: &str, ask: &ManageGame) -> Result<(), String> {
        let mut r = self
            .agent
            .post(format!("{}/games/{}/manage", self.base, assets::encode(game_id)))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(ask)
            .map_err(|e| format!("could not reach the Hub: {e}"))?;
        if r.status().is_success() {
            Ok(())
        } else {
            Err(r.body_mut().read_to_string().unwrap_or_else(|_| r.status().to_string()))
        }
    }
}

/// A random secret for claiming a server's place on the list.
pub fn new_secret() -> String {
    let mut bytes = [0u8; 20];
    getrandom::fill(&mut bytes).expect("the system has a random number source");
    data_encoding::BASE32_NOPAD.encode(&bytes).to_ascii_lowercase()
}

// ---------------------------------------------------------------------------
// Announcing in the background
// ---------------------------------------------------------------------------

/// What an announcer reports about the game it announces.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    pub name: String,
    pub game: String,
    pub code: Option<String>,
    pub players: u32,
    pub max_players: u32,
    pub kind: HostKind,
    /// Which game it is (see [`Heartbeat::game_id`]).
    pub game_id: Option<String>,
}

/// Signs a message with a player's identity key, as [`sign`] does.
pub type SignWith = Arc<dyn Fn(&[u8]) -> String + Send + Sync>;

/// Signs a listing as a player's, so the list shows their account.
#[derive(Clone)]
pub struct HostSigner {
    pub player_id: String,
    /// Signs a message with that player's identity key, as [`sign`] does.
    pub sign: SignWith,
}

/// Keeps a hosted game on a Hub's list from a background thread. Update the
/// shared [`Listing`] as players come and go; dropping the announcer takes
/// the game off the list.
pub struct Announcer {
    listing: Arc<Mutex<Listing>>,
    status: Arc<Mutex<Option<String>>>,
    stop: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Announcer {
    /// Starts announcing to `hub`. Nothing is sent until the listing has a
    /// join code.
    pub fn start(hub: HubClient, server_id: String, listing: Listing) -> Self {
        Self::start_signed(hub, server_id, listing, None)
    }

    /// [`Announcer::start`], with the listing signed by `host` so the list
    /// shows who hosts it.
    pub fn start_signed(
        hub: HubClient,
        server_id: String,
        listing: Listing,
        host: Option<HostSigner>,
    ) -> Self {
        let listing = Arc::new(Mutex::new(listing));
        let status = Arc::new(Mutex::new(None));
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        let shared = (listing.clone(), status.clone());
        let thread = std::thread::Builder::new()
            .name("playlibre-announce".into())
            .spawn(move || {
                let (listing, status) = shared;
                let secret = new_secret();
                let host_signature = host.as_ref().map(|h| (h.sign)(&host_message(&server_id, &secret)));
                let mut sent = false;
                loop {
                    let now = listing.lock().unwrap().clone();
                    let wait = if let Some(code) = now.code {
                        let beat = Heartbeat {
                            server_id: server_id.clone(),
                            secret: secret.clone(),
                            name: now.name,
                            game: now.game,
                            code,
                            players: now.players,
                            max_players: now.max_players,
                            kind: now.kind,
                            game_id: now.game_id,
                            host_id: host.as_ref().map(|h| h.player_id.clone()),
                            host_signature: host_signature.clone(),
                        };
                        let result = hub.heartbeat(&beat);
                        sent |= result.is_ok();
                        *status.lock().unwrap() = Some(match result {
                            Ok(()) => "Listed on the Hub".to_owned(),
                            Err(e) => e,
                        });
                        HEARTBEAT_EVERY
                    } else {
                        Duration::from_millis(500)
                    };
                    match stopped.recv_timeout(wait) {
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        _ => break,
                    }
                }
                if sent {
                    let _ = hub.remove(&server_id, &secret);
                }
            })
            .ok();
        Self { listing, status, stop: Some(stop), thread }
    }

    /// Changes what the list shows; the next heartbeat carries it.
    pub fn update(&self, change: impl FnOnce(&mut Listing)) {
        change(&mut self.listing.lock().unwrap());
    }

    /// The last thing the Hub said, such as "Listed on the Hub".
    pub fn status(&self) -> Option<String> {
        self.status.lock().unwrap().clone()
    }
}

impl Drop for Announcer {
    fn drop(&mut self) {
        // The thread takes the game off the list on its own, so a slow Hub
        // never holds up whoever stops hosting.
        drop(self.stop.take());
        drop(self.thread.take());
    }
}
