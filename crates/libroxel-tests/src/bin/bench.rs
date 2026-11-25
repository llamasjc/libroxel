//! Speed benchmarks for the engine's hot paths.
//!
//! ```text
//! bench run [--out results.json] [--repeat 3] [--only name,name]
//! bench compare --old a.json[,b.json] --new c.json[,d.json] [--threshold 1.3]
//! ```
//!
//! `run` times each workload `repeat` times and writes every time (in
//! seconds) to JSON. `compare` takes the fastest time of each workload on
//! each side (the least disturbed by a busy computer) and fails when the
//! new code is slower than `threshold` times the old.
//!
//! Times only mean something next to times from the same computer, so CI
//! builds the old and new commit on one runner and runs them in turn.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use libroxel_engine::{CharacterInput, PlaySession};
use libroxel_instance::{ClassKind, InstanceTree, Property, PropertyValue, Vec3, place};
use libroxel_tests::{DT, baseplate, corpus_games, play, start};

/// A workload: its name, a line about it, and the function that runs it
/// and returns the time of the part being measured.
struct Workload {
    name: &'static str,
    about: &'static str,
    run: fn() -> Duration,
}

const WORKLOADS: &[Workload] = &[
    Workload {
        name: "script_property_loop",
        about: "Luau reading and writing part properties",
        run: script_property_loop,
    },
    Workload {
        name: "script_pure_luau",
        about: "Luau maths, tables and strings with no engine calls",
        run: script_pure_luau,
    },
    Workload { name: "instance_churn", about: "scripts creating and destroying parts", run: instance_churn },
    Workload {
        name: "physics_falling_parts",
        about: "1,000 loose parts falling onto a floor",
        run: physics_falling_parts,
    },
    Workload {
        name: "place_save_load",
        about: "saving and loading a 40,000 part place",
        run: place_save_load,
    },
    Workload {
        name: "heavy_join",
        about: "joining a hosted 20,000 part game over the network",
        run: heavy_join,
    },
    Workload { name: "corpus_play", about: "playing the whole golden corpus", run: corpus_play },
];

fn script_session(source: &str) -> PlaySession {
    let mut tree = baseplate();
    let script = tree.create(ClassKind::Script, "Bench").unwrap();
    tree.set(script, Property::Source, PropertyValue::String(source.into())).unwrap();
    let sss = tree.service(ClassKind::ServerScriptService).unwrap();
    tree.set_parent(script, Some(sss)).unwrap();
    start(tree)
}

/// Runs a script that does its work in `bench()` once the session started,
/// and times only that call.
fn time_script(setup: &str, body: &str) -> Duration {
    let source = format!(
        "{setup}\nlocal bench_started = os.clock()\n{body}\nprint('bench-time ' .. (os.clock() - bench_started))"
    );
    let mut session = script_session(&source);
    let played = play(&mut session, 0.1);
    assert!(played.errors().is_empty(), "{}", played.dump());
    let line = played
        .log
        .iter()
        .find_map(|l| l.text.strip_prefix("bench-time "))
        .expect("the bench script finished");
    Duration::from_secs_f64(line.trim().parse().unwrap())
}

fn script_property_loop() -> Duration {
    time_script(
        r#"
        local parts = {}
        for i = 1, 500 do
            local p = Instance.new("Part")
            p.Anchored = true
            p.Parent = workspace
            parts[i] = p
        end"#,
        r#"
        for round = 1, 200 do
            for i, p in ipairs(parts) do
                local pos = p.Position
                p.Position = pos + Vector3.new(0, 0.01, 0)
                p.Transparency = (round % 10) / 10
                local _ = p.Name, p.Size.X, p:GetAttribute("x")
            end
        end"#,
    )
}

fn script_pure_luau() -> Duration {
    time_script(
        "",
        r#"
        local acc = 0
        local t = {}
        for i = 1, 300000 do
            t[i % 1000 + 1] = (t[i % 1000 + 1] or 0) + math.sqrt(i) * math.sin(i)
            acc += #tostring(i)
        end
        local words = {}
        for i = 1, 20000 do
            words[i] = string.format("w%05d", i):upper()
        end
        table.sort(words, function(a, b) return a > b end)
        local joined = table.concat(words, ",")
        local count = 0
        for _ in joined:gmatch("W%d+") do count += 1 end
        assert(count == 20000)"#,
    )
}

fn instance_churn() -> Duration {
    time_script(
        "",
        r#"
        for round = 1, 30 do
            local folder = Instance.new("Folder")
            folder.Parent = workspace
            for i = 1, 300 do
                local p = Instance.new("Part")
                p.Anchored = true
                p.Position = Vector3.new(i, round, 0)
                p:SetAttribute("Round", round)
                p.Parent = folder
            end
            local found = #folder:GetChildren()
            assert(found == 300)
            folder:Destroy()
        end"#,
    )
}

fn physics_falling_parts() -> Duration {
    let mut tree = baseplate();
    let ws = tree.workspace();
    for i in 0..1000 {
        let part = tree.create(ClassKind::Part, format!("Loose{i}")).unwrap();
        let (x, z, y) =
            ((i % 20) as f32 * 3.0 - 30.0, ((i / 20) % 20) as f32 * 3.0 - 30.0, 5.0 + (i / 400) as f32 * 3.0);
        tree.set(part, Property::Position, PropertyValue::Vec3(Vec3::new(x, y, z))).unwrap();
        tree.set(part, Property::Size, PropertyValue::Vec3(Vec3::new(2.0, 2.0, 2.0))).unwrap();
        tree.set_parent(part, Some(ws)).unwrap();
    }
    let mut session = start(tree);
    session.step(DT, CharacterInput::default());
    let began = Instant::now();
    for _ in 0..300 {
        session.step(DT, CharacterInput::default());
    }
    began.elapsed()
}

fn big_tree(models: usize, parts_each: usize) -> InstanceTree {
    let mut tree = baseplate();
    let ws = tree.workspace();
    for m in 0..models {
        let model = tree.create(ClassKind::Model, format!("Model{m}")).unwrap();
        tree.set_parent(model, Some(ws)).unwrap();
        for p in 0..parts_each {
            let part = tree.create(ClassKind::Part, format!("Part{p}")).unwrap();
            tree.set_parent(part, Some(model)).unwrap();
            let x = (m % 50) as f32 * 20.0 - 500.0;
            let z = (m / 50) as f32 * 20.0 - 500.0;
            tree.set(part, Property::Position, PropertyValue::Vec3(Vec3::new(x, 1.0 + p as f32, z))).unwrap();
            tree.set(part, Property::Anchored, PropertyValue::Bool(true)).unwrap();
        }
    }
    tree
}

fn place_save_load() -> Duration {
    let tree = big_tree(2000, 20);
    let began = Instant::now();
    let text = place::save(&tree).unwrap();
    let (loaded, _) = place::load(&text).unwrap();
    let took = began.elapsed();
    assert!(loaded.len() >= tree.len());
    took
}

fn heavy_join() -> Duration {
    let mut host = PlaySession::host(Rc::new(RefCell::new(big_tree(1000, 20))), 0, None, false).unwrap();
    let port = host.host_address().unwrap().port();
    host.step(DT, CharacterInput::default());
    let began = Instant::now();
    let mut guest =
        PlaySession::join(Rc::new(RefCell::new(InstanceTree::new())), &format!("127.0.0.1:{port}"), "Bench")
            .unwrap();
    loop {
        host.step(DT, CharacterInput::default());
        guest.step(DT, CharacterInput::default());
        assert!(!guest.disconnected(), "the guest was disconnected");
        if guest.joining().is_none() && guest.character().is_some() {
            break;
        }
        assert!(began.elapsed() < Duration::from_secs(120), "the join never finished");
    }
    began.elapsed()
}

fn corpus_play() -> Duration {
    let games = corpus_games();
    let began = Instant::now();
    for game in games {
        let mut session = start(game.tree);
        play(&mut session, game.seconds);
        session.stop();
    }
    began.elapsed()
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn run(args: &[String]) -> Result<(), String> {
    let repeat: usize =
        flag(args, "--repeat").map_or(Ok(3), str::parse).map_err(|_| "--repeat needs a number")?;
    let only: Option<Vec<&str>> = flag(args, "--only").map(|o| o.split(',').collect());
    let mut results: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    for workload in WORKLOADS.iter().filter(|w| only.as_ref().is_none_or(|o| o.contains(&w.name))) {
        // One untimed round fills caches and compiles the JIT's code.
        (workload.run)();
        let times: Vec<f64> = (0..repeat).map(|_| (workload.run)().as_secs_f64()).collect();
        let best = times.iter().copied().fold(f64::INFINITY, f64::min);
        eprintln!("{:<24} best {:>9.2} ms  ({})", workload.name, best * 1000.0, workload.about);
        results.insert(workload.name, times);
    }
    let json = serde_json::to_string_pretty(&results).unwrap();
    match flag(args, "--out") {
        Some(path) => std::fs::write(path, json).map_err(|e| format!("could not write {path}: {e}")),
        None => {
            println!("{json}");
            Ok(())
        }
    }
}

/// The fastest time of each workload across `files` (comma separated).
fn best_times(files: &str) -> Result<BTreeMap<String, f64>, String> {
    let mut best: BTreeMap<String, f64> = BTreeMap::new();
    for path in files.split(',').filter(|p| !p.is_empty()) {
        let text = std::fs::read_to_string(path).map_err(|e| format!("could not read {path}: {e}"))?;
        let results: BTreeMap<String, Vec<f64>> =
            serde_json::from_str(&text).map_err(|e| format!("{path} is not bench results: {e}"))?;
        for (name, times) in results {
            for t in times {
                let entry = best.entry(name.clone()).or_insert(f64::INFINITY);
                *entry = entry.min(t);
            }
        }
    }
    Ok(best)
}

fn compare(args: &[String]) -> Result<(), String> {
    let old = best_times(flag(args, "--old").ok_or("--old is needed")?)?;
    let new = best_times(flag(args, "--new").ok_or("--new is needed")?)?;
    let threshold: f64 =
        flag(args, "--threshold").map_or(Ok(1.3), str::parse).map_err(|_| "--threshold needs a number")?;
    // Differences this small are timer and scheduler noise, whatever the ratio.
    let floor = 0.002;
    let mut slower = Vec::new();
    println!("| workload | before | after | change |");
    println!("|---|---:|---:|---:|");
    for (name, &after) in &new {
        let Some(&before) = old.get(name) else {
            println!("| {name} | (new) | {:.1} ms | |", after * 1000.0);
            continue;
        };
        let ratio = after / before;
        let mark = if ratio > threshold && after - before > floor { " ⚠" } else { "" };
        println!(
            "| {name} | {:.1} ms | {:.1} ms | {:+.0}%{mark} |",
            before * 1000.0,
            after * 1000.0,
            (ratio - 1.0) * 100.0
        );
        if !mark.is_empty() {
            slower.push(format!("{name} is {:.0}% slower", (ratio - 1.0) * 100.0));
        }
    }
    if slower.is_empty() {
        Ok(())
    } else {
        Err(format!("slower than {threshold}x the previous commit: {}", slower.join(", ")))
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("run") => run(&args[1..]),
        Some("compare") => compare(&args[1..]),
        Some("list") => {
            for w in WORKLOADS {
                println!("{:<24} {}", w.name, w.about);
            }
            Ok(())
        }
        _ => Err("usage: bench run [--out file] [--repeat n] [--only a,b] | bench compare --old a.json --new b.json [--threshold 1.3] | bench list".into()),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
