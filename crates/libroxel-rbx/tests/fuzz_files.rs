//! Opens damaged copies of the sample files (bytes flipped, cut short,
//! repeated) and checks that each one either opens or is turned down, never
//! crashing or hanging. A game file is anything a player downloads, so it
//! is untrusted.
//!
//! The quick pass runs with the other tests; more rounds:
//!
//! ```text
//! FUZZ_ROUNDS=20000 cargo test -p libroxel-rbx --release --test fuzz_files -- --nocapture
//! ```

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::time::{Duration, Instant};

use libroxel_instance::{InstanceTree, place};

const SLOW: Duration = Duration::from_secs(3);

/// A small, repeatable random source.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// One damaged copy of `bytes`.
fn damage(rng: &mut Rng, bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    for _ in 0..=rng.below(4) {
        if out.is_empty() {
            break;
        }
        let at = rng.below(out.len());
        match rng.below(8) {
            0 => out[at] ^= 1 << rng.below(8),
            1 => out[at] = [0x00, 0xFF, 0x7F, 0x80][rng.below(4)],
            2 => out.truncate(at),
            3 => {
                // A run of the same byte, as a damaged length field.
                let n = rng.below(8).min(out.len() - at);
                out[at..at + n].fill(0xFF);
            }
            4 => {
                let end = (at + rng.below(64)).min(out.len());
                let copy = out[at..end].to_vec();
                out.splice(at..at, copy);
            }
            5 => {
                let end = (at + rng.below(64)).min(out.len());
                out.drain(at..end);
            }
            6 => {
                // Digits in a text file, made huge or strange.
                let word: &[u8] =
                    [&b"1e39"[..], b"-0", b"nan", b"inf", b"99999999999", b"-2147483649"][rng.below(6)];
                out.splice(at..at, word.iter().copied());
            }
            _ => {
                let n = (rng.below(4) + 1).min(out.len() - at);
                for b in &mut out[at..at + n] {
                    *b = rng.next() as u8;
                }
            }
        }
    }
    out
}

/// Runs `open` on `bytes`; says what went wrong if it panicked or hung.
fn check(what: &str, bytes: &[u8], open: impl FnOnce(&[u8])) -> Option<String> {
    let began = Instant::now();
    let ran = catch_unwind(AssertUnwindSafe(|| open(bytes)));
    let took = began.elapsed();
    match ran {
        Err(why) => {
            let why = why
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| why.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            let path = std::env::temp_dir().join(format!("libroxel-fuzz-{what}-{}", bytes.len()));
            let _ = std::fs::write(&path, bytes);
            Some(format!("{what}: panicked ({why}); saved as {}", path.display()))
        }
        Ok(()) if took > SLOW => Some(format!("{what}: took {:.1}s", took.as_secs_f32())),
        Ok(()) => None,
    }
}

fn open_studio_file(bytes: &[u8]) {
    if let Ok(dom) = libroxel_rbx::read_dom(bytes) {
        let mut tree = InstanceTree::new();
        let _ = libroxel_rbx::import_place(&mut tree, &dom);
    }
}

fn open_lrx(bytes: &[u8]) {
    if let Ok(text) = std::str::from_utf8(bytes) {
        let _ = place::load(text);
    }
}

#[test]
fn damaged_files_never_crash() {
    let rounds: usize = std::env::var("FUZZ_ROUNDS").ok().and_then(|n| n.parse().ok()).unwrap_or(150);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/files");
    let mut samples: Vec<(String, Vec<u8>)> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| libroxel_rbx::is_place_file(p))
        .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap()))
        .collect();
    samples.sort();
    // Libroxel's own format too, made from one of them.
    let (tree, _) = libroxel_rbx::open(&dir.join("baseplate-566-xml.rbxlx")).unwrap();
    let lrx = place::save(&tree).unwrap().into_bytes();

    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut problems = Vec::new();
    let mut rng = Rng(0x5eed_1234_abcd_0001);
    for _ in 0..rounds {
        for (name, bytes) in &samples {
            let damaged = damage(&mut rng, bytes);
            problems.extend(check(name, &damaged, open_studio_file));
        }
        let damaged = damage(&mut rng, &lrx);
        problems.extend(check("baseplate.lrx", &damaged, open_lrx));
    }
    std::panic::set_hook(quiet);
    problems.sort();
    problems.dedup();
    assert!(problems.is_empty(), "{} problems:\n{}", problems.len(), problems.join("\n"));
}

/// Folders inside folders a hundred thousand deep, which a reader that
/// calls itself for each level would run out of stack on.
#[test]
fn deeply_nested_files_are_turned_down() {
    let mut xml = String::from("<roblox version=\"4\">");
    for _ in 0..100_000 {
        xml.push_str("<Item class=\"Folder\"><Properties><string name=\"Name\">F</string></Properties>");
    }
    for _ in 0..100_000 {
        xml.push_str("</Item>");
    }
    xml.push_str("</roblox>");
    assert!(matches!(libroxel_rbx::read_dom(xml.as_bytes()), Err(libroxel_rbx::ImportError::TooDeep)));

    // A binary file names each instance's parent in a list, so it can say
    // the same in a few bytes.
    let mut dom = rbx_dom_weak::WeakDom::new(rbx_dom_weak::InstanceBuilder::new("DataModel"));
    let mut at = dom.root_ref();
    for _ in 0..5000 {
        at = dom.insert(at, rbx_dom_weak::InstanceBuilder::new("Folder"));
    }
    let mut bytes = Vec::new();
    rbx_binary::to_writer(&mut bytes, &dom, dom.root().children()).unwrap();
    assert!(matches!(libroxel_rbx::read_dom(&bytes), Err(libroxel_rbx::ImportError::TooDeep)));

    // Ordinary nesting still opens.
    let mut dom = rbx_dom_weak::WeakDom::new(rbx_dom_weak::InstanceBuilder::new("DataModel"));
    let mut at = dom.root_ref();
    for _ in 0..50 {
        at = dom.insert(at, rbx_dom_weak::InstanceBuilder::new("Folder"));
    }
    let mut bytes = Vec::new();
    rbx_binary::to_writer(&mut bytes, &dom, dom.root().children()).unwrap();
    assert!(libroxel_rbx::read_dom(&bytes).is_ok());
}
