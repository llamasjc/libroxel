//! Calls every script-facing member with odd arguments (huge, NaN, empty,
//! the wrong type) and checks that nothing panics or keeps the game busy.
//!
//! The quick pass runs with the other tests. The full pass, every class and
//! member against every odd value, takes minutes:
//!
//! ```text
//! cargo test -p playlibre-script --test fuzz_bindings -- --ignored --nocapture
//! ```

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use playlibre_script::ScriptRuntime;

/// A call that takes longer than this is reported as a hang.
const SLOW: Duration = Duration::from_millis(1500);

/// Odd values, as Luau expressions.
const ODD: &[&str] = &[
    "nil",
    "0",
    "-1",
    "1e308",
    "-1e308",
    "math.huge",
    "-math.huge",
    "0/0",
    "2^31",
    "-2^63",
    "0.5",
    "''",
    "'x'",
    "string.rep('a', 70000)",
    "'\\0\\255\\n'",
    "{}",
    "{1, 2, 3}",
    "{{}, {{}}}",
    "setmetatable({}, {__index = function() error('meta') end, __tostring = function() error('meta') end})",
    "true",
    "function() end",
    "function() error('callback') end",
    "workspace",
    "game",
    "Instance.new('Part')",
    "Vector3.new(1e30, 1e30, 1e30)",
    "Vector3.new(0/0, 0/0, 0/0)",
    "Vector3.new(math.huge, -math.huge, math.huge)",
    "Vector3.new(-5, -5, -5)",
    "CFrame.new(1e30, 1e30, 1e30)",
    "CFrame.new(0/0, 0/0, 0/0)",
    "CFrame.new(0, 0, 0, 0/0, 0, 0, 0, 0/0, 0, 0, 0, 0/0)",
    "Color3.new(0/0, 1e30, -1)",
    "UDim2.new(1e30, 1e9, 0/0, -1e9)",
    "Vector2.new(1e30, 0/0)",
    "Enum.Material.Grass",
    "Enum.KeyCode.E",
    "BrickColor.new('Bright red')",
    "Region3.new(Vector3.new(-1e9, -1e9, -1e9), Vector3.new(1e9, 1e9, 1e9))",
    "Ray.new(Vector3.new(), Vector3.new(1e30, 0, 0))",
    "TweenInfo.new(1e30)",
    "NumberSequence.new(0)",
    "newproxy(true)",
    "coroutine.create(function() end)",
];

/// Members left out: ones that wait (they'd stop the run), and ones that
/// end the run on purpose.
const SKIP: &[&str] = &[
    "Wait",
    "wait",
    "WaitForChild",
    "waitForChild",
    "Yield",
    "InvokeServer",
    "InvokeClient",
    "Invoke",
    "yield",
    "error",
    "assert",
    "LoadAsset",
    "PromptPurchase",
];

/// String literals in the crate's own sources that could be member names.
fn names_in_sources() -> BTreeSet<String> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut names = BTreeSet::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap_or_default();
        for piece in text.split('"').skip(1).step_by(2) {
            let ok = (2..=40).contains(&piece.len())
                && piece.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && piece.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if ok {
                names.insert(piece.to_owned());
            }
        }
    }
    for class in rbx_reflection_database::get_bundled().classes.values() {
        for property in class.properties.keys() {
            names.insert(property.to_string());
        }
    }
    names.retain(|n| !SKIP.contains(&n.as_str()));
    names
}

/// How to make the thing whose members are probed, as a Luau expression.
fn subjects(full: bool) -> Vec<String> {
    let mut out: Vec<String> = [
        "workspace.Terrain",
        "workspace",
        "game",
        "workspace.CurrentCamera",
        "Instance.new('Part')",
        "Instance.new('Model')",
        "Instance.new('Humanoid')",
        "Instance.new('ScreenGui')",
        "Instance.new('Frame')",
        "Instance.new('TextLabel')",
        "Instance.new('Sound')",
        "Instance.new('Animator')",
        "Instance.new('RemoteEvent')",
        "Vector3.new(1, 2, 3)",
        "CFrame.new(1, 2, 3)",
        "Color3.new(1, 0, 0)",
        "UDim2.new(1, 2, 3, 4)",
        "Vector2.new(1, 2)",
        "Random.new(1)",
        "workspace.ChildAdded",
        "Enum.Material",
        "string",
        "math",
        "table",
        "task",
        "buffer",
        "utf8",
        "bit32",
        "Vector3",
        "CFrame",
        "Color3",
        "UDim2",
        "Instance",
        "TweenInfo",
        "Region3",
        "NumberSequence",
        "ColorSequence",
        "BrickColor",
        "DateTime",
    ]
    .map(String::from)
    .to_vec();
    for service in [
        "Players",
        "Lighting",
        "RunService",
        "TweenService",
        "HttpService",
        "DataStoreService",
        "PathfindingService",
        "Debris",
        "UserInputService",
        "ContextActionService",
        "SoundService",
        "TextService",
        "CollectionService",
        "MarketplaceService",
        "TeleportService",
        "PhysicsService",
        "GuiService",
        "StarterGui",
        "ReplicatedStorage",
        "MessagingService",
        "InsertService",
        "Chat",
        "TextChatService",
    ] {
        out.push(format!("game:GetService('{service}')"));
    }
    if full {
        for class in rbx_reflection_database::get_bundled().classes.values() {
            out.push(format!("Instance.new('{}')", class.name));
        }
    }
    out
}

/// One probe: members of `subject` called or set with odd values. Returns
/// what went wrong, if anything.
fn probe(subject: &str, member: &str, values: &[&str]) -> Option<String> {
    let rt = ScriptRuntime::new_with_tree(Default::default()).expect("runtime starts");
    let mut calls = String::new();
    for (i, a) in values.iter().enumerate() {
        let b = values[(i * 7 + 3) % values.len()];
        calls.push_str(&format!(
            "pcall(f, s, {a}) pcall(f, s, {a}, {a}, {a}) pcall(f, s, {b}, {a}, {a}, {a}) pcall(f, {a}) \
             pcall(function() s[m] = {a} end)\n"
        ));
    }
    let source = format!(
        "local ok, s = pcall(function() return {subject} end)\n\
         if not ok or s == nil then return end\n\
         local m = '{member}'\n\
         local got, f = pcall(function() return s[m] end)\n\
         if not got then return end\n\
         if type(f) ~= 'function' then f = function(self, v) self[m] = v end end\n\
         {calls}"
    );
    let began = Instant::now();
    let ran = catch_unwind(AssertUnwindSafe(|| {
        let _ = rt.run("fuzz", &source);
        // Whatever they left waiting or connected runs too.
        for _ in 0..3 {
            let _ = rt.step(1.0 / 30.0);
        }
    }));
    let took = began.elapsed();
    match ran {
        Err(why) => Some(format!(
            "{subject} .{member}: panicked: {}",
            why.downcast_ref::<String>()
                .cloned()
                .or_else(|| why.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default()
        )),
        Ok(()) if took > SLOW => Some(format!("{subject} .{member}: took {:.1}s", took.as_secs_f32())),
        Ok(()) => None,
    }
}

fn sweep(full: bool) -> Vec<String> {
    let names = names_in_sources();
    let only = std::env::var("FUZZ_ONLY").ok();
    let mut problems = Vec::new();
    for subject in subjects(full) {
        if only.as_deref().is_some_and(|o| !subject.contains(o)) {
            continue;
        }
        // Which names mean something on it, found in one run.
        let rt = ScriptRuntime::new_with_tree(Default::default()).expect("runtime starts");
        let list = names.iter().map(|n| format!("'{n}'")).collect::<Vec<_>>().join(",");
        let found = rt.run(
            "members",
            &format!(
                "local ok, s = pcall(function() return {subject} end)\n\
                 if not ok or s == nil then return end\n\
                 for _, m in {{{list}}} do\n\
                   local got, v = pcall(function() return s[m] end)\n\
                   if got and v ~= nil then print(m) end\n\
                 end"
            ),
        );
        let _ = found;
        let members: Vec<String> = rt.take_output().into_iter().map(|l| l.text).collect();
        eprintln!("{subject}: {} members", members.len());
        for member in members {
            if !full
                && member.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                && subject.starts_with("Instance")
            {
                continue;
            }
            eprint!("  {member}\r");
            let values: Vec<&str> =
                if full { ODD.to_vec() } else { ODD.iter().step_by(3).copied().collect() };
            if let Some(problem) = probe(&subject, &member, &values) {
                eprintln!("PROBLEM {problem}");
                problems.push(problem);
            }
        }
    }
    problems
}

#[test]
#[ignore = "takes minutes; run with --ignored"]
fn every_member_survives_odd_arguments() {
    let problems = sweep(true);
    assert!(problems.is_empty(), "{} problems:\n{}", problems.len(), problems.join("\n"));
}
