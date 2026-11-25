//! Opens every place or model file under the folders named on the command line and
//! totals what didn't come across, by property and by the kind of value
//! the file held, with how long reading and building took.
//!
//! cargo run -p playlibre-rbx --example rbx_audit -- path/to/rbx-test-files
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if playlibre_rbx::is_place_file(&p) {
            out.push(p);
        }
    }
}

fn main() {
    let mut files = Vec::new();
    for arg in std::env::args().skip(1) {
        let p = PathBuf::from(arg);
        if p.is_dir() { walk(&p, &mut files) } else { files.push(p) }
    }
    files.sort();
    let (mut ok, mut failed) = (0, Vec::new());
    let mut skipped: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut unknown: BTreeMap<String, usize> = BTreeMap::new();
    let mut notes: BTreeMap<String, usize> = BTreeMap::new();
    let (mut read_time, mut build_time) = (Duration::ZERO, Duration::ZERO);
    let (mut instances, mut properties) = (0, 0);
    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        let t = Instant::now();
        let dom = match playlibre_rbx::read_dom(&bytes) {
            Ok(d) => d,
            Err(e) => {
                failed.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        read_time += t.elapsed();
        let mut kinds: HashMap<String, String> = HashMap::new();
        for inst in dom.descendants() {
            for (k, v) in &inst.properties {
                kinds.insert(format!("{}.{}", inst.class, k), format!("{:?}", v.ty()));
            }
        }
        let t = Instant::now();
        let mut tree = playlibre_instance::InstanceTree::new();
        let report = playlibre_rbx::import_place(&mut tree, &dom);
        build_time += t.elapsed();
        ok += 1;
        instances += report.instances;
        properties += report.properties;
        for (k, n) in report.skipped_properties {
            let kind = kinds.get(&k).cloned().unwrap_or_else(|| "?".into());
            let e = skipped.entry(k).or_insert((0, kind));
            e.0 += n;
        }
        for (k, n) in report.unknown_classes {
            *unknown.entry(k).or_default() += n;
        }
        for n in report.notes {
            *notes.entry(n).or_default() += 1;
        }
    }
    println!("files: {} opened, {} failed", ok, failed.len());
    for f in &failed {
        println!("  FAILED {f}");
    }
    println!("instances {instances}, properties set {properties}");
    println!("read {:?}, build {:?}", read_time, build_time);
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for (n, kind) in skipped.values() {
        *by_kind.entry(kind.clone()).or_default() += n;
    }
    println!("\nskipped by value kind:");
    let mut v: Vec<_> = by_kind.into_iter().collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.1));
    for (k, n) in v {
        println!("  {n:6} {k}");
    }
    println!("\nskipped properties:");
    let mut v: Vec<_> = skipped.into_iter().collect();
    v.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(&b.0)));
    for (k, (n, kind)) in v {
        println!("  {n:6} {k} ({kind})");
    }
    println!("\nclasses kept as folders or as themselves:");
    let mut v: Vec<_> = unknown.into_iter().collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.1));
    for (k, n) in v {
        println!("  {n:6} {k}");
    }
    println!("\nnotes:");
    for (k, n) in notes {
        println!("  {n:6} {k}");
    }
}
