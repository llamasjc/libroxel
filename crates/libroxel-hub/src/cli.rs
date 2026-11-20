//! The Hub's command line: running it, and looking after it from its own
//! computer. `libroxel hub` and the Hub-only `libroxel-hub` program both
//! use it.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// What `libroxel hub help` prints.
pub const HUB_HELP: &str = "\
  hub                          run a Hub
      --port <n>               Hub port (default 7430)
      --data <file>            where the Hub keeps its data
      --behind-proxy           a web server here (such as Caddy) passes players
                               on: listen only on this computer, and trust it
                               for their addresses
  hub reports                  list reported games and hats (and which are hidden)
  hub clear <game|item> <id>   say a reported one is fine: list it again
  hub remove <game|item> <id>  take one down for good
  hub accounts [search]        list accounts
  hub owners                   list the Hub's owners (its admins)
  hub owner add|remove <name or player id>
                               owners may list Official servers and take names
                               such as admin
  hub ban <name> [reason]      ban an account; unban <name> lets it back
  hub verify|unverify <name>   give or take away a verified badge
  hub rename <name> <new name> rename an account (reserved names too)
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

/// Runs `libroxel hub …` (or `libroxel-hub …`) with `args` after the word
/// `hub`. `default_data` is where the Hub keeps its data without `--data`.
pub fn cli(args: &[String], default_data: &Path) -> Result<(), String> {
    let flags = Flags::parse(args, &["--port", "--data"])?;
    let options = crate::HubOptions { behind_proxy: flags.has("--behind-proxy") };
    let port = flags.number("--port", crate::DEFAULT_PORT)?;
    let data = flags.get("--data").map_or_else(|| default_data.to_path_buf(), PathBuf::from);
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
            crate::clear_reports(&crate::open_db(Some(&data))?, kind, id)?;
            println!("{kind} {id} is listed again.");
            Ok(())
        }
        ["remove", kind, id] => {
            crate::take_down(&crate::open_db(Some(&data))?, kind, id)?;
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
            println!("{who} ({id}) is an owner of this Hub.");
            Ok(())
        }
        ["owner", "remove", who] => {
            crate::remove_owner(&crate::open_db(Some(&data))?, who)?;
            println!("{who} is no longer an owner.");
            Ok(())
        }
        ["ban", who, reason @ ..] => {
            let reason = if reason.is_empty() { "no reason given".to_owned() } else { reason.join(" ") };
            crate::ban(&crate::open_db(Some(&data))?, who, &reason)?;
            println!("{who} is banned.");
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
        ["unban", who] => {
            crate::unban(&crate::open_db(Some(&data))?, who)?;
            println!("{who} can use this Hub again.");
            Ok(())
        }
        _ => {
            Err("usage: libroxel hub [reports | clear <game|item> <id> | remove <game|item> <id> | accounts \
                  [search] | owners | owner add|remove <who> | ban <who> [reason] | unban <who> | rename <who> <name> | verify|unverify <who>]"
                .into())
        }
    }
}
