use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use playlibre_engine::pictures;
use playlibre_hub::{ASSET_PREFIX, HubClient, OfficialGame, asset_hash};
use playlibre_instance::serde_json::{self, Value as Json};

use crate::server::data_dir;

struct Queue {
    hub: String,
    dir: PathBuf,
    waiting: VecDeque<String>,
    working: bool,
}

static QUEUE: Mutex<Queue> =
    Mutex::new(Queue { hub: String::new(), dir: PathBuf::new(), waiting: VecDeque::new(), working: false });

pub fn dir() -> PathBuf {
    data_dir().join("assets")
}

pub fn start(hub: &str) {
    start_at(hub, dir());
}

fn start_at(hub: &str, dir: PathBuf) {
    let mut queue = QUEUE.lock().unwrap();
    queue.hub = hub.to_owned();
    queue.dir = dir.clone();
    pictures::set_hub_assets(dir, fetch);
}

fn fetch(hash: String) {
    let mut queue = QUEUE.lock().unwrap();
    queue.waiting.push_back(hash);
    if !queue.working {
        queue.working = true;
        std::thread::spawn(work);
    }
}

fn work() {
    let (client, dir) = {
        let queue = QUEUE.lock().unwrap();
        (HubClient::new(&queue.hub), queue.dir.clone())
    };
    loop {
        let Some(hash) = ({
            let mut queue = QUEUE.lock().unwrap();
            let next = queue.waiting.pop_front();
            queue.working = next.is_some();
            next
        }) else {
            return;
        };
        if let Ok(bytes) = client.asset(&hash) {
            keep(&dir, &hash, &bytes);
        }
        if QUEUE.lock().unwrap().waiting.is_empty() {
            pictures::hub_asset_arrived();
        }
    }
}

fn keep(dir: &Path, hash: &str, bytes: &[u8]) {
    let part = dir.join(format!("{hash}.part"));
    let _ = std::fs::create_dir_all(dir)
        .and_then(|_| std::fs::write(&part, bytes))
        .and_then(|_| std::fs::rename(&part, dir.join(hash)));
}

fn each_name(node: &mut Json, f: &mut dyn FnMut(&mut String)) {
    for key in ["properties", "extras"] {
        if let Some(Json::Object(map)) = node.get_mut(key) {
            for value in map.values_mut() {
                if let Json::String(s) = value {
                    f(s);
                }
            }
        }
    }
    if let Some(Json::Array(children)) = node.get_mut("children") {
        for child in children {
            each_name(child, f);
        }
    }
}

pub fn upload(client: &HubClient, place: &str) -> Result<String, String> {
    let mut doc: Json = serde_json::from_str(place).map_err(|e| e.to_string())?;
    let mut files: HashMap<String, (String, String, Vec<u8>)> = HashMap::new();
    each_name(&mut doc, &mut |name| {
        if !files.contains_key(name.as_str())
            && let Some((bytes, ending)) = pictures::local_file(name)
        {
            files.insert(name.clone(), (asset_hash(&bytes), ending, bytes));
        }
    });
    if files.is_empty() {
        return Ok(doc.to_string());
    }
    let hashes: Vec<String> = files.values().map(|(h, ..)| h.clone()).collect();
    let missing = client.missing_assets(&hashes)?;
    for (hash, _, bytes) in files.values() {
        if missing.contains(hash) {
            client.put_asset(bytes)?;
        }
    }
    each_name(&mut doc, &mut |name| {
        if let Some((hash, ending, _)) = files.get(name.as_str()) {
            *name = format!("{ASSET_PREFIX}{hash}.{ending}");
        }
    });
    Ok(doc.to_string())
}

pub fn official_dir() -> PathBuf {
    data_dir().join("official")
}

pub fn official_file(name: &str) -> PathBuf {
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' { c } else { '_' }).collect();
    official_dir().join(format!("{}.lrx", safe.trim()))
}

pub fn cached_official(game: &OfficialGame) -> Option<PathBuf> {
    let path = official_file(&game.name);
    let text = std::fs::read(&path).ok()?;
    (asset_hash(&text) == game.hash).then_some(path)
}

pub fn get_official(client: &HubClient, game: &OfficialGame) -> Result<PathBuf, String> {
    if let Some(path) = cached_official(game) {
        return Ok(path);
    }
    let text = client.official_place(&game.name)?;
    let path = official_file(&game.name);
    write(&path, &text)?;
    Ok(path)
}

pub fn write(path: &Path, text: &str) -> Result<(), String> {
    let part = path.with_extension("part");
    path.parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|_| std::fs::write(&part, text))
        .and_then(|_| std::fs::rename(&part, path))
        .map_err(|e| format!("could not keep the game: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_files_go_to_the_hub_and_come_back() {
        let hub = playlibre_hub::start("127.0.0.1:0".parse().unwrap(), None).unwrap();
        let client = HubClient::new(&hub.url());
        let root = std::env::temp_dir().join(format!("playlibre-assets-{}", playlibre_hub::new_secret()));
        let game = root.join("game");
        std::fs::create_dir_all(game.join("assets")).unwrap();
        let picture = playlibre_engine::pictures::PICTURES[0].1;
        std::fs::write(game.join("logo.png"), picture).unwrap();
        std::fs::write(game.join("assets").join("77.png"), picture).unwrap();
        pictures::set_folders(vec![game.clone()]);
        let place = r#"{"children":[{"class":"ImageLabel","name":"logo.png","properties":{"image":"logo.png"},
            "children":[{"class":"Decal","name":"D","properties":{"texture":"rbxassetid://77"}}]}]}"#;
        let published = upload(&client, place).unwrap();
        let hash = asset_hash(picture);
        let name = format!("{ASSET_PREFIX}{hash}.png");
        assert_eq!(published.matches(&name).count(), 2, "{published}");
        assert!(published.contains(r#""name":"logo.png""#));
        assert_eq!(client.asset(&hash).unwrap(), picture);

        let cache = root.join("cache");
        start_at(&hub.url(), cache.clone());
        let before = pictures::generation();
        assert!(pictures::bytes_for(&name).is_none());
        let waited = std::time::Instant::now();
        while pictures::generation() == before && waited.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(pictures::bytes_for(&name).as_deref(), Some(picture));
        assert!(cache.join(&hash).exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
