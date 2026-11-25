//! Times common script operations, in nanoseconds each:
//! `cargo run -p playlibre-script --example script_bench`.

use playlibre_script::ScriptRuntime;

/// Each case: a name, code run once before timing, and the code timed.
const CASES: &[(&str, &str, &str)] = &[
    ("empty loop", "", ""),
    ("part.Name", "", "local _ = part.Name"),
    ("part.Position", "", "local _ = part.Position"),
    ("part.Transparency", "", "local _ = part.Transparency"),
    ("part.Transparency = x", "", "part.Transparency = 0.5"),
    ("part.CFrame", "", "local _ = part.CFrame"),
    ("CFrame * CFrame", "local a, b = CFrame.new(1, 2, 3), CFrame.Angles(0, 1, 0)", "local _ = a * b"),
    ("CFrame * Vector3", "local a, v = CFrame.new(1, 2, 3), Vector3.new(1, 0, 0)", "local _ = a * v"),
    ("Color3.new", "", "local _ = Color3.new(1, 0, 0)"),
    ("part:IsA", "", "local _ = part:IsA(\"BasePart\")"),
    ("folder.ChildName", "", "local _ = folder.Child150"),
    ("FindFirstChild", "", "local _ = folder:FindFirstChild(\"Child150\")"),
    ("lenient member", "", "local _ = part.TopSurface"),
    ("GetChildren (200)", "", "local _ = folder:GetChildren()"),
    (
        "BindableEvent:Fire",
        "local e = Instance.new(\"BindableEvent\") e.Event:Connect(function() end)",
        "e:Fire()",
    ),
    ("task.spawn", "", "task.spawn(function() end)"),
    ("Instance.new+Destroy", "", "local p = Instance.new(\"Part\") p.Parent = folder p:Destroy()"),
];

fn main() {
    let reps: u32 = std::env::args().nth(1).and_then(|n| n.parse().ok()).unwrap_or(20000);
    let began = std::time::Instant::now();
    let rt = ScriptRuntime::new_with_tree(Default::default()).expect("runtime starts");
    println!("runtime start: {:.1} ms", began.elapsed().as_secs_f64() * 1000.0);
    rt.run(
        "setup",
        r#"
        _G.part = Instance.new("Part", workspace)
        _G.folder = Instance.new("Folder", workspace)
        for i = 1, 200 do Instance.new("Folder", _G.folder).Name = "Child" .. i end
    "#,
    )
    .expect("setup runs");
    // A second argument picks the cases whose names contain it.
    let only = std::env::args().nth(2).unwrap_or_default();
    // Starting copies of one script (a coin, a door), as games do by the hundred.
    let mut source = String::from("local Coin = {}\n");
    for i in 0..60 {
        source.push_str(&format!(
            "function Coin.f{i}(a, b)\n  local t = {{}}\n  for k = 1, a do t[k] = k * b + {i} end\n  return #t\nend\n"
        ));
    }
    source.push_str("return Coin\n");
    let copies = 200;
    let t = std::time::Instant::now();
    for _ in 0..copies {
        rt.run("Coin", &source).expect("the coin script runs");
    }
    println!(
        "{:>24}: {:>8.0} us",
        "start a script copy",
        t.elapsed().as_secs_f64() * 1e6 / f64::from(copies)
    );
    // A frame's scheduling work with many threads asleep (NPCs, spinners,
    // coins each in a `while task.wait(n)` loop).
    if only.is_empty() || only == "sleepers" {
        rt.run(
            "sleepers",
            "for i = 1, 10000 do task.spawn(function() while true do task.wait(5 + i % 7) end end) end",
        )
        .expect("sleepers start");
        let _ = rt.step(1.0 / 60.0);
        let frames = 600;
        let t = std::time::Instant::now();
        for _ in 0..frames {
            let _ = rt.step(1.0 / 60.0);
        }
        println!(
            "{:>24}: {:>8.1} us",
            "frame, 10k asleep",
            t.elapsed().as_secs_f64() * 1e6 / f64::from(frames)
        );
    }
    for (name, setup, body) in CASES.iter().filter(|c| c.0.contains(only.as_str())) {
        let src =
            format!("local part, folder = _G.part, _G.folder\n{setup}\nfor _ = 1, {reps} do\n{body}\nend");
        // Once to warm up, then the best of three.
        let mut best = f64::MAX;
        for _ in 0..4 {
            let t = std::time::Instant::now();
            if let Err(err) = rt.run(name, &src) {
                println!("{name}: failed: {err}");
                break;
            }
            best = best.min(t.elapsed().as_secs_f64());
        }
        println!("{name:>24}: {:>8.0} ns", best * 1e9 / f64::from(reps));
        let _ = rt.step(0.0);
    }
}
