//! Picture files, for image labels, decals and textures.
//!
//! A picture name is a PNG file. A plain name is looked for in each folder
//! given to [`set_folders`] (the editor gives the game file's folder, an
//! `images` folder beside it and the games folder's `images` folder); a full
//! path is used as it is. Names starting with `libroxel://` are pictures
//! built into Libroxel: `coin`, `smile`, `star`, `arrow`, `checker`, and
//! chess pieces named `chess-` then `w` or `b` then `k`, `q`, `r`, `b`, `n` or `p`,
//! cards named `card-` then `a`, `2` to `10`, `j`, `q` or `k` then `c`, `d`,
//! `h` or `s` (and `card-back`, `baize`), Connect 4 tiles `c4-red`, `c4-green`,
//! `c4-hole` and `c4-top`, and draughts pieces `draughts-white`,
//! `draughts-black`, `draughts-white-king` and `draughts-black-king`.
//!
//! Games brought in from Roblox Studio name their files by asset number
//! (`rbxassetid://123`, or a web address ending `?id=123`). Those are
//! looked for as `123.png` (or `.jpg`, or `123` with no ending) in an `assets` folder
//! inside each of the folders, so a creator can drop in their own files.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, Image, ImageSampler, ImageType};
use bevy::render::render_resource::TextureFormat;

/// The start of the names of built-in pictures.
pub const BUILT_IN: &str = "libroxel://";

/// Built-in pictures, by name after [`BUILT_IN`].
pub const PICTURES: &[(&str, &[u8])] = &[
    ("coin", include_bytes!("../assets/coin.png")),
    ("smile", include_bytes!("../assets/smile.png")),
    ("star", include_bytes!("../assets/star.png")),
    ("arrow", include_bytes!("../assets/arrow.png")),
    ("checker", include_bytes!("../assets/checker.png")),
    ("chess-wk", include_bytes!("../assets/chess-wk.png")),
    ("chess-wq", include_bytes!("../assets/chess-wq.png")),
    ("chess-wr", include_bytes!("../assets/chess-wr.png")),
    ("chess-wb", include_bytes!("../assets/chess-wb.png")),
    ("chess-wn", include_bytes!("../assets/chess-wn.png")),
    ("chess-wp", include_bytes!("../assets/chess-wp.png")),
    ("chess-bk", include_bytes!("../assets/chess-bk.png")),
    ("chess-bq", include_bytes!("../assets/chess-bq.png")),
    ("chess-br", include_bytes!("../assets/chess-br.png")),
    ("chess-bb", include_bytes!("../assets/chess-bb.png")),
    ("chess-bn", include_bytes!("../assets/chess-bn.png")),
    ("chess-bp", include_bytes!("../assets/chess-bp.png")),
    ("baize", include_bytes!("../assets/cards/baize.jpg")),
    ("card-10c", include_bytes!("../assets/cards/card-10c.png")),
    ("card-10d", include_bytes!("../assets/cards/card-10d.png")),
    ("card-10h", include_bytes!("../assets/cards/card-10h.png")),
    ("card-10s", include_bytes!("../assets/cards/card-10s.png")),
    ("card-2c", include_bytes!("../assets/cards/card-2c.png")),
    ("card-2d", include_bytes!("../assets/cards/card-2d.png")),
    ("card-2h", include_bytes!("../assets/cards/card-2h.png")),
    ("card-2s", include_bytes!("../assets/cards/card-2s.png")),
    ("card-3c", include_bytes!("../assets/cards/card-3c.png")),
    ("card-3d", include_bytes!("../assets/cards/card-3d.png")),
    ("card-3h", include_bytes!("../assets/cards/card-3h.png")),
    ("card-3s", include_bytes!("../assets/cards/card-3s.png")),
    ("card-4c", include_bytes!("../assets/cards/card-4c.png")),
    ("card-4d", include_bytes!("../assets/cards/card-4d.png")),
    ("card-4h", include_bytes!("../assets/cards/card-4h.png")),
    ("card-4s", include_bytes!("../assets/cards/card-4s.png")),
    ("card-5c", include_bytes!("../assets/cards/card-5c.png")),
    ("card-5d", include_bytes!("../assets/cards/card-5d.png")),
    ("card-5h", include_bytes!("../assets/cards/card-5h.png")),
    ("card-5s", include_bytes!("../assets/cards/card-5s.png")),
    ("card-6c", include_bytes!("../assets/cards/card-6c.png")),
    ("card-6d", include_bytes!("../assets/cards/card-6d.png")),
    ("card-6h", include_bytes!("../assets/cards/card-6h.png")),
    ("card-6s", include_bytes!("../assets/cards/card-6s.png")),
    ("card-7c", include_bytes!("../assets/cards/card-7c.png")),
    ("card-7d", include_bytes!("../assets/cards/card-7d.png")),
    ("card-7h", include_bytes!("../assets/cards/card-7h.png")),
    ("card-7s", include_bytes!("../assets/cards/card-7s.png")),
    ("card-8c", include_bytes!("../assets/cards/card-8c.png")),
    ("card-8d", include_bytes!("../assets/cards/card-8d.png")),
    ("card-8h", include_bytes!("../assets/cards/card-8h.png")),
    ("card-8s", include_bytes!("../assets/cards/card-8s.png")),
    ("card-9c", include_bytes!("../assets/cards/card-9c.png")),
    ("card-9d", include_bytes!("../assets/cards/card-9d.png")),
    ("card-9h", include_bytes!("../assets/cards/card-9h.png")),
    ("card-9s", include_bytes!("../assets/cards/card-9s.png")),
    ("card-ac", include_bytes!("../assets/cards/card-ac.png")),
    ("card-ad", include_bytes!("../assets/cards/card-ad.png")),
    ("card-ah", include_bytes!("../assets/cards/card-ah.png")),
    ("card-as", include_bytes!("../assets/cards/card-as.png")),
    ("card-back", include_bytes!("../assets/cards/card-back.png")),
    ("card-jc", include_bytes!("../assets/cards/card-jc.png")),
    ("card-jd", include_bytes!("../assets/cards/card-jd.png")),
    ("card-jh", include_bytes!("../assets/cards/card-jh.png")),
    ("card-js", include_bytes!("../assets/cards/card-js.png")),
    ("card-kc", include_bytes!("../assets/cards/card-kc.png")),
    ("card-kd", include_bytes!("../assets/cards/card-kd.png")),
    ("card-kh", include_bytes!("../assets/cards/card-kh.png")),
    ("card-ks", include_bytes!("../assets/cards/card-ks.png")),
    ("card-qc", include_bytes!("../assets/cards/card-qc.png")),
    ("card-qd", include_bytes!("../assets/cards/card-qd.png")),
    ("card-qh", include_bytes!("../assets/cards/card-qh.png")),
    ("card-qs", include_bytes!("../assets/cards/card-qs.png")),
    ("c4-green", include_bytes!("../assets/connect4/c4-green.png")),
    ("c4-hole", include_bytes!("../assets/connect4/c4-hole.png")),
    ("c4-red", include_bytes!("../assets/connect4/c4-red.png")),
    ("c4-top", include_bytes!("../assets/connect4/c4-top.png")),
    ("draughts-black-king", include_bytes!("../assets/draughts/draughts-black-king.png")),
    ("draughts-black", include_bytes!("../assets/draughts/draughts-black.png")),
    ("draughts-white-king", include_bytes!("../assets/draughts/draughts-white-king.png")),
    ("draughts-white", include_bytes!("../assets/draughts/draughts-white.png")),
];

static FOLDERS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub const HUB_ASSET: &str = "asset://";

pub const ENDINGS: &[&str] = &["png", "jpg", "jpeg", "mesh", "obj", "ogg", "wav", "mp3"];

struct HubAssets {
    dir: PathBuf,
    fetch: fn(String),
    asked: std::collections::HashSet<String>,
}

static HUB_ASSETS: Mutex<Option<HubAssets>> = Mutex::new(None);

pub fn set_hub_assets(dir: PathBuf, fetch: fn(String)) {
    *HUB_ASSETS.lock().unwrap_or_else(|e| e.into_inner()) =
        Some(HubAssets { dir, fetch, asked: Default::default() });
}

pub fn hub_asset_arrived() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

fn hub_asset(rest: &str) -> Option<Vec<u8>> {
    let hash = rest.split('.').next().unwrap_or_default();
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let hash = hash.to_ascii_lowercase();
    let mut assets = HUB_ASSETS.lock().unwrap_or_else(|e| e.into_inner());
    let assets = assets.as_mut()?;
    if let Ok(bytes) = std::fs::read(assets.dir.join(&hash)) {
        return Some(bytes);
    }
    if assets.asked.insert(hash.clone()) {
        (assets.fetch)(hash);
    }
    None
}

fn ending_of(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        "jpg"
    } else if bytes.starts_with(b"OggS") {
        "ogg"
    } else if bytes.starts_with(b"RIFF") {
        "wav"
    } else if bytes.starts_with(b"ID3") || bytes.starts_with(&[0xFF, 0xFB]) {
        "mp3"
    } else if bytes.starts_with(b"version ") {
        "mesh"
    } else {
        "obj"
    }
}

pub fn local_file(name: &str) -> Option<(Vec<u8>, String)> {
    let name = name.trim();
    let lower = name.to_ascii_lowercase();
    if name.is_empty() || name.starts_with(BUILT_IN) || lower.starts_with(HUB_ASSET) {
        return None;
    }
    if let Some(id) = asset_id(name) {
        let folders = asset_folders();
        return ENDINGS
            .iter()
            .find_map(|e| {
                folders.iter().find_map(|dir| {
                    std::fs::read(dir.join(format!("{id}.{e}"))).ok().map(|b| (b, e.to_string()))
                })
            })
            .or_else(|| {
                folders.iter().find_map(|dir| std::fs::read(dir.join(&id)).ok()).map(|b| {
                    let e = ending_of(&b).to_owned();
                    (b, e)
                })
            });
    }
    let ending = lower.rsplit_once('.').map(|(_, e)| e)?;
    let ending = *ENDINGS.iter().find(|e| **e == ending)?;
    bytes_for(name).map(|b| (b, ending.to_owned()))
}

/// Where plain picture names are looked for, first to last.
pub fn set_folders(folders: Vec<PathBuf>) {
    let mut current = FOLDERS.lock().unwrap_or_else(|e| e.into_inner());
    if *current != folders {
        *current = folders;
        // A different game: names may mean other files now.
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
}

/// Changes whenever the folders do, so caches know to forget pictures.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// The PNG file a picture name means, if it can be found.
pub fn bytes_for(name: &str) -> Option<Vec<u8>> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    if let Some(built_in) = name.strip_prefix(BUILT_IN) {
        return PICTURES.iter().find(|(n, _)| *n == built_in).map(|(_, b)| b.to_vec());
    }
    if let Some(rest) = name.strip_prefix(HUB_ASSET) {
        return hub_asset(rest);
    }
    if let Some(id) = asset_id(name) {
        return find_asset(&id, &["png", "jpg", "jpeg"]);
    }
    if let Some(rest) = name.strip_prefix("rbxasset://") {
        return find_file(rest);
    }
    let path = PathBuf::from(name);
    if path.is_absolute() {
        return std::fs::read(path).ok();
    }
    let folders = FOLDERS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    folders.iter().find_map(|dir| std::fs::read(dir.join(&path)).ok())
}

/// The asset number in an address such as `rbxassetid://123` or
/// `https://www.roblox.com/asset/?id=123`, if `name` is one.
pub fn asset_id(name: &str) -> Option<String> {
    let name = name.trim();
    let lower = name.to_ascii_lowercase();
    let rest = if let Some(rest) = lower.strip_prefix("rbxassetid://") {
        rest
    } else if lower.starts_with("http://") || lower.starts_with("https://") {
        let at = lower.find("id=")?;
        &lower[at + 3..]
    } else {
        return None;
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then_some(digits)
}

/// The folders asset files are looked for in: `assets` inside each folder.
fn asset_folders() -> Vec<PathBuf> {
    let folders = FOLDERS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    folders.iter().map(|f| f.join("assets")).collect()
}

/// The file for asset number `id`: `id.ending` for the first of `endings`
/// found, or else a file called just `id`.
pub fn find_asset(id: &str, endings: &[&str]) -> Option<Vec<u8>> {
    let folders = asset_folders();
    endings
        .iter()
        .find_map(|e| folders.iter().find_map(|dir| std::fs::read(dir.join(format!("{id}.{e}"))).ok()))
        .or_else(|| folders.iter().find_map(|dir| std::fs::read(dir.join(id)).ok()))
}

/// A path relative to the folders or their `assets` folders.
fn find_file(relative: &str) -> Option<Vec<u8>> {
    let relative = relative.trim_start_matches('/');
    if relative.split(['/', '\\']).any(|part| part == "..") {
        return None;
    }
    let folders = FOLDERS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    folders.iter().chain(&asset_folders()).find_map(|dir| std::fs::read(dir.join(relative)).ok())
}

/// The file a name means, trying `endings` for asset numbers.
pub fn bytes_for_kind(name: &str, endings: &[&str]) -> Option<Vec<u8>> {
    match asset_id(name) {
        Some(id) => find_asset(&id, endings),
        None => bytes_for(name),
    }
}

/// Decodes a PNG (or JPEG) into an image with 8-bit RGBA pixels.
pub fn decode(bytes: &[u8], sampler: ImageSampler) -> Option<Image> {
    let image = Image::from_buffer(
        bytes,
        // A creator's own asset files may be JPEGs.
        ImageType::Extension(if bytes.starts_with(&[0xFF, 0xD8]) { "jpg" } else { "png" }),
        CompressedImageFormats::NONE,
        true,
        sampler,
        RenderAssetUsages::default(),
    )
    .ok()?;
    match image.texture_descriptor.format {
        TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm => Some(image),
        _ => image.convert(TextureFormat::Rgba8UnormSrgb),
    }
}

/// A PNG's size and pixels (RGBA, 8 bits each, row by row).
pub fn decode_png(bytes: &[u8]) -> Option<([usize; 2], Vec<u8>)> {
    let image = decode(bytes, ImageSampler::Default)?;
    let size = [image.width() as usize, image.height() as usize];
    let data = image.data?;
    (data.len() == size[0] * size[1] * 4).then_some((size, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_pictures_decode() {
        for (name, _) in PICTURES {
            let bytes = bytes_for(&format!("{BUILT_IN}{name}")).expect("built in");
            let (size, pixels) = decode_png(&bytes).expect("decodes");
            let want = match *name {
                n if n.starts_with("card-") => [162, 252],
                "baize" => [512, 512],
                n if n.starts_with("chess-") || n.starts_with("c4-") || n.starts_with("draughts-") => {
                    [128, 128]
                }
                _ => [64, 64],
            };
            assert_eq!(size, want, "{name}");
            assert_eq!(pixels.len(), size[0] * size[1] * 4);
        }
        // The coin's corners are see-through and its middle is gold.
        let (_, pixels) = decode_png(PICTURES[0].1).unwrap();
        assert_eq!(pixels[3], 0);
        let middle = (32 * 64 + 40) * 4;
        assert!(pixels[middle] > 150 && pixels[middle + 3] == 255, "{:?}", &pixels[middle..middle + 4]);
        assert!(bytes_for("libroxel://nothing").is_none());
        assert!(bytes_for("  ").is_none());
    }

    #[test]
    fn asset_numbers_are_found_in_addresses() {
        assert_eq!(asset_id("rbxassetid://12345").as_deref(), Some("12345"));
        assert_eq!(asset_id(" rbxassetid://77/ ").as_deref(), Some("77"));
        assert_eq!(asset_id("http://www.roblox.com/asset/?id=987").as_deref(), Some("987"));
        assert_eq!(asset_id("https://www.roblox.com/asset?id=5&v=2").as_deref(), Some("5"));
        assert_eq!(asset_id("coin.png"), None);
        assert_eq!(asset_id("rbxassetid://"), None);
        assert!(find_file("../secret").is_none());
    }
}
