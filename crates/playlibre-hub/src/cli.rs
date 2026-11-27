//! The Hub's command line: running it, and looking after it from its own
//! computer, for the `playlibre-hub` program on the Hub's server.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// What `playlibre-hub help` prints.
pub const HUB_HELP: &str = "\
  (nothing)                    run the Hub
      --port <n>               Hub port (default 7430)
      --data <file>            where the Hub keeps its data
      --behind-proxy           a web server here (such as Caddy) passes players
                               on: listen only on this computer, and trust it
                               for their addresses
      --game-servers <program> run game servers here: Play on a game finds or
                               starts one (<program> is the playlibre app, run
                               as `playlibre server`)
      --max-game-servers <n>   at most this many at once (default 8)
      --players-per-server <n> players on each (default 12)
      --first-udp-port <n>     servers use UDP ports from here up (default 7500)
      --stop-empty-after <s>   a server stops once empty this long (default 120)
      --game-server-dir <dir>  their keys and logs (default: game-servers next
                               to --data)
      --starters <program>     the playlibre app to list the official games from
                               (default: the --game-servers one)
  reports                      list reported games, hats, comments and messages
  clear <kind> <id>            say a reported one is fine: list it again
  remove <kind> <id>           take one down for good (kind: game, item,
                               comment, message
                               or group)
  accounts [search]            list accounts
  owners                       list the Hub's owners (its admins)
  owner add|remove <name or player id>
                               owners are the Hub's admins and may list
                               Official servers
  hat-makers                   list the Hat-Makers: the only accounts (with
                               the owners) that add hats to the catalog
  hat-maker add|remove <name>  give or take away Hat-Maker
  ban <name> [reason]          ban an account; unban <name> lets it back
  verify|unverify <name>       give or take away a verified badge
  badge give|take <name> <badge>
                               give or take away a PlayLibre badge (such as
                               supporter)
  rename <name> <new name>     rename an account
  credits <name>               where an account's Credits came from
  credits free [take]          free Credits from daily rewards, badges and
                               sales paid with them; take takes them back
  earnings                     show the engagement pool and cash out settings
  earnings pool <credits>       Credits shared each day between creators by
                               playtime (0: off)
  payouts [all]                list cash outs waiting to be paid
  payouts on <provider> <cents per 1000 Credits> <minimum Credits>
  payouts off
  payouts paid <id> [reference]
  payouts refuse <id> <reason> gives the Credits back
";

struct Flags {
    rest: Vec<String>,
    pairs: Vec<(String, String)>,
    bare: Vec<String>,
}

impl Flags {
    fn parse(args: &[String], with_value: &[&str]) -> Result<Self, String> {
        let mut flags = Flags { rest: Vec::new(), pairs: Vec::new(), bare: Vec::new() };
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            if with_value.contains(&arg.as_str()) {
                let value = args.get(i + 1).ok_or(format!("{arg} needs a value"))?;
                flags.pairs.push((arg.clone(), value.clone()));
                i += 2;
            } else if arg.starts_with("--") {
                flags.bare.push(arg.clone());
                i += 1;
            } else {
                flags.rest.push(arg.clone());
                i += 1;
            }
        }
        Ok(flags)
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.pairs.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    fn has(&self, name: &str) -> bool {
        self.bare.iter().any(|b| b == name)
    }

    fn number<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T, String> {
        self.get(name)
            .map_or(Ok(default), |v| v.parse().map_err(|_| format!("{name} needs a number, not {v}")))
    }
}

/// Runs `playlibre-hub …` with `args` after the program's name. `default_data` is where the Hub keeps its data without `--data`.
pub fn cli(args: &[String], default_data: &Path) -> Result<(), String> {
    let flags = Flags::parse(
        args,
        &[
            "--port",
            "--data",
            "--game-servers",
            "--game-server-dir",
            "--max-game-servers",
            "--players-per-server",
            "--first-udp-port",
            "--stop-empty-after",
            "--starters",
        ],
    )?;
    let port = flags.number("--port", crate::DEFAULT_PORT)?;
    let data = flags.get("--data").map_or_else(|| default_data.to_path_buf(), PathBuf::from);
    let fleet = match flags.get("--game-servers") {
        None => None,
        Some(program) => {
            let dir = flags
                .get("--game-server-dir")
                .map_or_else(|| data.parent().unwrap_or(Path::new(".")).join("game-servers"), PathBuf::from);
            let usual = crate::FleetOptions::new(PathBuf::from(program), dir);
            Some(crate::FleetOptions {
                max_servers: flags.number("--max-game-servers", usual.max_servers)?,
                players_per_server: flags.number("--players-per-server", usual.players_per_server)?,
                first_udp_port: flags.number("--first-udp-port", usual.first_udp_port)?,
                stop_when_empty: std::time::Duration::from_secs(
                    flags.number("--stop-empty-after", usual.stop_when_empty.as_secs())?,
                ),
                ..usual
            })
        }
    };
    let options = crate::HubOptions {
        behind_proxy: flags.has("--behind-proxy"),
        mail: crate::Mail::from_env()?,
        fleet,
        starters: flags.get("--starters").map(PathBuf::from),
        moderation: crate::Moderation::from_env(),
        stripe: crate::Stripe::from_env(),
        ..Default::default()
    };
    let words: Vec<&str> = flags.rest.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] => {
            // Behind a web server, only it may reach the Hub; otherwise anyone on the network may.
            let ip = if options.behind_proxy { [127, 0, 0, 1] } else { [0, 0, 0, 0] };
            crate::run(SocketAddr::from((ip, port)), &data, options)
        }
        ["reports"] => {
            let db = crate::open_db(Some(&data))?;
            let reported = crate::reports(&db);
            if reported.is_empty() {
                println!("Nothing has been reported.");
            }
            for r in reported {
                let state = if r.hidden {
                    "HIDDEN"
                } else if r.cleared {
                    "cleared"
                } else {
                    "listed"
                };
                let name = r.name.as_deref().unwrap_or("(gone)");
                println!("{} {} \"{name}\" [{state}], {} report(s):", r.kind, r.target, r.reasons.len());
                for reason in r.reasons {
                    println!("    - {reason}");
                }
            }
            Ok(())
        }
        ["clear", kind, id] => {
            let db = crate::open_db(Some(&data))?;
            crate::clear_reports(&db, kind, id)?;
            crate::moderate::log(&db, "", crate::ModAction::Dismiss, kind, id, "", "", "", None, 0);
            println!("{kind} {id} is listed again.");
            Ok(())
        }
        ["remove", kind, id] => {
            let db = crate::open_db(Some(&data))?;
            crate::take_down(&db, kind, id)?;
            crate::moderate::log(&db, "", crate::ModAction::Remove, kind, id, "", "", "", None, 0);
            println!("{kind} {id} is gone.");
            Ok(())
        }
        ["accounts", search @ ..] => {
            let rows = crate::accounts(&crate::open_db(Some(&data))?, &search.join(" "));
            if rows.is_empty() {
                println!("No accounts match.");
            }
            for a in rows {
                let mut notes = Vec::new();
                if a.owner {
                    notes.push("owner".to_owned());
                }
                if a.has_password {
                    notes.push("password".to_owned());
                }
                if let Some(reason) = &a.banned {
                    notes.push(format!("BANNED: {reason}"));
                }
                let days = a.joined_secs_ago / 86_400;
                println!("{:<20} {}  joined {days} days ago  {}", a.name, a.player_id, notes.join(", "));
            }
            Ok(())
        }
        ["owners"] => {
            let owners = crate::owners(&crate::open_db(Some(&data))?);
            if owners.is_empty() {
                println!("No owners yet. Add your account: hub owner add <your name>");
            }
            for (id, name) in owners {
                println!("{:<20} {id}", name.as_deref().unwrap_or("(a server)"));
            }
            Ok(())
        }
        ["owner", "add", who] => {
            let id = crate::add_owner(&crate::open_db(Some(&data))?, who)?;
            println!("{who} ({id}) is an owner of the Hub.");
            Ok(())
        }
        ["owner", "remove", who] => {
            crate::remove_owner(&crate::open_db(Some(&data))?, who)?;
            println!("{who} is no longer an owner.");
            Ok(())
        }
        ["hat-makers"] => {
            let makers = crate::hat_makers(&crate::open_db(Some(&data))?);
            if makers.is_empty() {
                println!("No Hat-Makers besides the owners. Add one: hub hat-maker add <name>");
            }
            for (id, name) in makers {
                println!("{:<20} {id}", name.as_deref().unwrap_or("?"));
            }
            Ok(())
        }
        ["hat-maker", how @ ("add" | "remove"), who] => {
            crate::set_hat_maker(&crate::open_db(Some(&data))?, who, *how == "add")?;
            println!("{who} {}", if *how == "add" { "is a Hat-Maker." } else { "is no longer a Hat-Maker." });
            Ok(())
        }
        ["ban", who, reason @ ..] => {
            let reason = if reason.is_empty() { "no reason given".to_owned() } else { reason.join(" ") };
            let db = crate::open_db(Some(&data))?;
            let id = crate::ban(&db, who, &reason)?;
            crate::moderate::log(&db, "", crate::ModAction::Ban, "", "", &id, &reason, "", None, 0);
            println!("{who} is banned.");
            Ok(())
        }
        ["credits", "free", how @ ..] if how.is_empty() || how == ["take"] => {
            let db = crate::open_db(Some(&data))?;
            let list = if how.is_empty() { crate::free_to_take(&db) } else { crate::take_back_free(&db) }
                .map_err(|e| e.to_string())?;
            if list.is_empty() {
                println!("No free Credits left.");
            }
            let mut total = 0;
            for (id, credits) in &list {
                let name: String = db
                    .query_row("SELECT name FROM accounts WHERE player_id = ?1", [id], |r| r.get(0))
                    .unwrap_or_else(|_| id.clone());
                println!("{name:<20} {credits}");
                total += credits;
            }
            if !list.is_empty() {
                println!("{} Credits {}.", total, if how.is_empty() { "to take back" } else { "taken back" });
            }
            Ok(())
        }
        ["credits", who] => {
            let db = crate::open_db(Some(&data))?;
            let id = crate::owner::resolve(&db, who)?;
            for (amount, reason, reference, time) in crate::history(&db, &id) {
                println!("{amount:>7}  {reason:<12} {reference:<24} {} days ago", (crate::now() - time) / 86_400);
            }
            println!("{who} has {} Credits.", crate::balance(&db, &id));
            Ok(())
        }
        ["earnings"] => {
            let s = crate::earnings_settings(&crate::open_db(Some(&data))?);
            println!("Engagement pool: {} Credits a day", s.pool);
            if s.payouts_on() {
                println!("Cash out: on, through {}, {} cents per 1000 Credits, at least {} Credits", s.provider, s.cents_per_thousand, s.minimum);
            } else {
                println!("Cash out: off");
            }
            Ok(())
        }
        ["earnings", "pool", credits] => {
            let db = crate::open_db(Some(&data))?;
            let pool = credits.parse().map_err(|_| format!("{credits} is not a number"))?;
            crate::set_earnings_settings(&db, &crate::EarningsSettings { pool, ..crate::earnings_settings(&db) })?;
            println!("Creators share {pool} Credits a day.");
            Ok(())
        }
        ["payouts", "on", provider, cents, minimum] => {
            let db = crate::open_db(Some(&data))?;
            let number = |v: &str| v.parse::<u32>().map_err(|_| format!("{v} is not a number"));
            let s = crate::EarningsSettings {
                provider: (*provider).to_owned(),
                cents_per_thousand: number(cents)?,
                minimum: number(minimum)?,
                ..crate::earnings_settings(&db)
            };
            if !s.payouts_on() {
                return Err("cents per 1000 Credits must be more than 0".into());
            }
            crate::set_earnings_settings(&db, &s)?;
            println!("Cash out is on.");
            Ok(())
        }
        ["payouts", "off"] => {
            let db = crate::open_db(Some(&data))?;
            let s = crate::EarningsSettings { provider: String::new(), ..crate::earnings_settings(&db) };
            crate::set_earnings_settings(&db, &s)?;
            println!("Cash out is off.");
            Ok(())
        }
        ["payouts", "paid", id, note @ ..] => {
            let id = id.parse().map_err(|_| format!("{id} is not a cash out"))?;
            crate::finish_payout(&crate::open_db(Some(&data))?, id, true, &note.join(" "))?;
            println!("Cash out {id} is paid.");
            Ok(())
        }
        ["payouts", "refuse", id, reason @ ..] if !reason.is_empty() => {
            let id = id.parse().map_err(|_| format!("{id} is not a cash out"))?;
            crate::finish_payout(&crate::open_db(Some(&data))?, id, false, &reason.join(" "))?;
            println!("Cash out {id} is refused and its Credits are back.");
            Ok(())
        }
        ["payouts", which @ ..] if which.is_empty() || which == ["all"] => {
            let db = crate::open_db(Some(&data))?;
            let list = crate::payouts(&db, if which.is_empty() { "waiting" } else { "" });
            if list.is_empty() {
                println!("No cash outs.");
            }
            for p in list {
                let name: String = db
                    .query_row("SELECT name FROM accounts WHERE player_id = ?1", [&p.player_id], |r| r.get(0))
                    .unwrap_or_else(|_| p.player_id.clone());
                println!(
                    "{:<5} {:<20} {} Credits  ${}.{:02}  {}  {}  {} days ago  {}",
                    p.payout_id, name, p.credits, p.cents / 100, p.cents % 100, p.details, p.state, p.asked_secs_ago / 86_400, p.note
                );
            }
            Ok(())
        }
        ["rename", who, name] => {
            crate::rename(&crate::open_db(Some(&data))?, who, name)?;
            println!("{who} is now called {name}.");
            Ok(())
        }
        ["verify", who] => {
            crate::set_verified(&crate::open_db(Some(&data))?, who, true)?;
            println!("{who} is verified.");
            Ok(())
        }
        ["unverify", who] => {
            crate::set_verified(&crate::open_db(Some(&data))?, who, false)?;
            println!("{who} is no longer verified.");
            Ok(())
        }
        ["badge", how @ ("give" | "take"), who, id] => {
            let badge = crate::platform_badge(id).ok_or_else(|| {
                let ids: Vec<&str> = crate::PLATFORM_BADGES.iter().map(|b| b.id).collect();
                format!("there is no badge {id} (there are {})", ids.join(", "))
            })?;
            let db = crate::open_db(Some(&data))?;
            let player = crate::owner::resolve(&db, who)?;
            if *how == "give" {
                crate::badges::give(&db, &player, id);
                println!("{who} has the {} badge.", badge.name);
            } else {
                crate::badges::take(&db, &player, id);
                println!("{who} no longer has the {} badge.", badge.name);
            }
            Ok(())
        }
        ["unban", who] => {
            let db = crate::open_db(Some(&data))?;
            let id = crate::owner::resolve(&db, who)?;
            crate::unban(&db, who)?;
            crate::moderate::log(&db, "", crate::ModAction::Unban, "", "", &id, "", "", None, 0);
            println!("{who} can use PlayLibre again.");
            Ok(())
        }
        _ => {
            Err("usage: playlibre-hub [reports | clear <kind> <id> | remove <kind> <id> | accounts \
                  [search] | owners | owner add|remove <who> | ban <who> [reason] | unban <who> | rename <who> <name> | verify|unverify <who>]"
                .into())
        }
    }
}
