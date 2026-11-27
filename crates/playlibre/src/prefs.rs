use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value as Json, json};

const RECENT: usize = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct Prefs {
    pub recent: Vec<PathBuf>,
    pub last_game: Option<String>,
    pub sign_in: String,
    pub explorer: bool,
    pub properties: bool,
    pub output: bool,
    pub toolbox: bool,
    pub in_play: bool,
    pub grid: bool,
    pub snap: f32,
    pub angle_snap: f32,
    pub toolbox_size: f32,
    pub side_size: f32,
    pub explorer_size: Option<f32>,
    pub output_size: f32,
    pub window: Option<(u32, u32)>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            recent: Vec::new(),
            last_game: None,
            sign_in: String::new(),
            explorer: true,
            properties: true,
            output: true,
            toolbox: true,
            in_play: true,
            grid: true,
            snap: 1.0,
            angle_snap: 15.0,
            toolbox_size: 190.0,
            side_size: 300.0,
            explorer_size: None,
            output_size: 170.0,
            window: None,
        }
    }
}

impl Prefs {
    pub fn touch(&mut self, file: &Path) {
        if self.recent.first().is_some_and(|f| f == file) {
            return;
        }
        self.recent.retain(|f| f != file);
        self.recent.insert(0, file.to_path_buf());
        self.recent.truncate(RECENT);
    }

    pub fn forget(&mut self, file: &Path) {
        self.recent.retain(|f| f != file);
    }
}

pub fn path() -> PathBuf {
    crate::server::data_dir().join("studio.json")
}

pub fn read(path: &Path) -> Prefs {
    let j: Json =
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let d = Prefs::default();
    let flag = |k: &str, or: bool| j[k].as_bool().unwrap_or(or);
    let num = |k: &str, or: f32| j[k].as_f64().map_or(or, |n| n as f32);
    Prefs {
        recent: j["recent"]
            .as_array()
            .map(|a| a.iter().filter_map(Json::as_str).map(PathBuf::from).take(RECENT).collect())
            .unwrap_or_default(),
        last_game: j["last_game"].as_str().map(str::to_owned),
        sign_in: j["sign_in"].as_str().unwrap_or_default().to_owned(),
        explorer: flag("explorer", d.explorer),
        properties: flag("properties", d.properties),
        output: flag("output", d.output),
        toolbox: flag("toolbox", d.toolbox),
        in_play: flag("in_play", d.in_play),
        grid: flag("grid", d.grid),
        snap: num("snap", d.snap),
        angle_snap: num("angle_snap", d.angle_snap),
        toolbox_size: num("toolbox_size", d.toolbox_size),
        side_size: num("side_size", d.side_size),
        explorer_size: j["explorer_size"].as_f64().map(|n| n as f32),
        output_size: num("output_size", d.output_size),
        window: j["window"]
            .as_array()
            .and_then(|a| Some((a.first()?.as_u64()? as u32, a.get(1)?.as_u64()? as u32))),
    }
}

fn to_json(p: &Prefs) -> Json {
    json!({
        "recent": p.recent.iter().map(|f| f.to_string_lossy()).collect::<Vec<_>>(),
        "last_game": p.last_game,
        "sign_in": p.sign_in,
        "explorer": p.explorer,
        "properties": p.properties,
        "output": p.output,
        "toolbox": p.toolbox,
        "in_play": p.in_play,
        "grid": p.grid,
        "snap": p.snap,
        "angle_snap": p.angle_snap,
        "toolbox_size": p.toolbox_size,
        "side_size": p.side_size,
        "explorer_size": p.explorer_size,
        "output_size": p.output_size,
        "window": p.window.map(|(w, h)| [w, h]),
    })
}

pub fn write(path: &Path, prefs: &Prefs) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(&to_json(prefs)) {
        let _ = std::fs::write(path, text);
    }
}

#[derive(Default)]
pub struct Saver {
    written: Option<Prefs>,
    at: Option<Instant>,
}

impl Saver {
    pub fn new(prefs: &Prefs) -> Self {
        Self { written: Some(prefs.clone()), at: None }
    }

    pub fn tick(&mut self, path: &Path, prefs: &Prefs) {
        if self.written.as_ref().is_none_or(|w| w == prefs)
            || self.at.is_some_and(|at| at.elapsed() < Duration::from_secs(1))
        {
            return;
        }
        write(path, prefs);
        self.written = Some(prefs.clone());
        self.at = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_places_come_first_once() {
        let mut prefs = Prefs::default();
        for name in ["a", "b", "a", "c"] {
            prefs.touch(Path::new(name));
        }
        assert_eq!(prefs.recent, [PathBuf::from("c"), "a".into(), "b".into()]);
        for i in 0..20 {
            prefs.touch(&PathBuf::from(i.to_string()));
        }
        assert_eq!(prefs.recent.len(), RECENT);
        prefs.forget(Path::new("19"));
        assert_eq!(prefs.recent[0], PathBuf::from("18"));
    }

    #[test]
    fn kept_and_read_back() {
        let dir = std::env::temp_dir().join(format!("playlibre-prefs-{}", std::process::id()));
        let path = dir.join("studio.json");
        assert_eq!(read(&path), Prefs::default());
        let mut prefs = Prefs { snap: 0.5, last_game: Some("g1".into()), ..Prefs::default() };
        prefs.touch(Path::new("x.lrx"));
        write(&path, &prefs);
        assert_eq!(read(&path), prefs);
        std::fs::write(&path, r#"{"snap": 2.0}"#).unwrap();
        assert_eq!(read(&path), Prefs { snap: 2.0, ..Prefs::default() });
        let _ = std::fs::remove_dir_all(dir);
    }
}
