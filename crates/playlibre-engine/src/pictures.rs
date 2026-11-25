//! Picture files, for image labels, decals and textures.
//!
//! A picture name is a PNG file. A plain name is looked for in each folder
//! given to [`set_folders`] (the editor gives the game file's folder, an
//! `images` folder beside it and the games folder's `images` folder); a full
//! path is used only when it is inside one of those folders. A game is
//! someone else's code, so its names never reach the rest of this computer:
//! no `..`, no network shares (which would make Windows sign in to someone
//! else's computer), and only ordinary files no bigger than [`MAX_FILE`]. Names starting with `libroxel://` are pictures
//! built into PlayLibre: `coin`, `smile`, `star`, `arrow`, `checker`, and
//! chess pieces named `chess-` then `w` or `b` then `k`, `q`, `r`, `b`, `n` or `p`,
//! cards named `card-` then `a`, `2` to `10`, `j`, `q` or `k` then `c`, `d`,
//! `h` or `s` (and `card-back`, `baize`), Connect 4 tiles `c4-red`, `c4-green`,
//! `c4-hole` and `c4-top`, and draughts pieces `draughts-white`,
//! `draughts-black`, `draughts-white-king` and `draughts-black-king`. The
//! chess and draughts pieces also come carved in wood: `chess-wood-wk`,
//! `draughts-wood-white-king` and so on.
//!
//! Games brought in from the reference editor name their files by asset number
//! (`rbxassetid://123`, or a web address ending `?id=123`). Those are
//! looked for as `123.png` (or `.jpg`, or `123` with no ending) in an `assets` folder
//! inside each of the folders, so a creator can drop in their own files.

use std::io::Read;
use std::path::{Component, Path, PathBuf};
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
    ("chess-wood-wk", include_bytes!("../assets/chess-wood-wk.png")),
    ("chess-wood-wq", include_bytes!("../assets/chess-wood-wq.png")),
    ("chess-wood-wr", include_bytes!("../assets/chess-wood-wr.png")),
    ("chess-wood-wb", include_bytes!("../assets/chess-wood-wb.png")),
    ("chess-wood-wn", include_bytes!("../assets/chess-wood-wn.png")),
    ("chess-wood-wp", include_bytes!("../assets/chess-wood-wp.png")),
    ("chess-wood-bk", include_bytes!("../assets/chess-wood-bk.png")),
    ("chess-wood-bq", include_bytes!("../assets/chess-wood-bq.png")),
    ("chess-wood-br", include_bytes!("../assets/chess-wood-br.png")),
    ("chess-wood-bb", include_bytes!("../assets/chess-wood-bb.png")),
    ("chess-wood-bn", include_bytes!("../assets/chess-wood-bn.png")),
    ("chess-wood-bp", include_bytes!("../assets/chess-wood-bp.png")),
    ("draughts-wood-white", include_bytes!("../assets/draughts/draughts-wood-white.png")),
    ("draughts-wood-white-king", include_bytes!("../assets/draughts/draughts-wood-white-king.png")),
    ("draughts-wood-black", include_bytes!("../assets/draughts/draughts-wood-black.png")),
    ("draughts-wood-black-king", include_bytes!("../assets/draughts/draughts-wood-black-king.png")),
    ("draughts-black", include_bytes!("../assets/draughts/draughts-black.png")),
    ("draughts-white-king", include_bytes!("../assets/draughts/draughts-white-king.png")),
    ("draughts-white", include_bytes!("../assets/draughts/draughts-white.png")),
];

static FOLDERS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// The biggest picture, sound or model file read.
pub const MAX_FILE: u64 = 64 * 1024 * 1024;

/// Whether `name` names a network share or a device (`\\server\share`,
/// `//server/share`, `\\?\`, `\\.\`): never opened, since just looking
/// makes Windows hand the other computer the user's sign-in.
fn remote_or_device(name: &str) -> bool {
    let name = name.trim_start();
    let slashes = |s: &str| {
        s.starts_with("\\\\") || s.starts_with("//") || s.starts_with("\\/") || s.starts_with("/\\")
    };
    slashes(name)
        || Path::new(name)
            .components()
            .any(|c| matches!(c, Component::Prefix(p) if !matches!(p.kind(), std::path::Prefix::Disk(_))))
}

/// `relative` if it stays inside whatever folder it is joined to.
fn inside_relative(relative: &str) -> Option<&Path> {
    if relative.is_empty() || remote_or_device(relative) {
        return None;
    }
    if relative.split(['/', '\\']).any(|part| part == "..") {
        return None;
    }
    let path = Path::new(relative);
    path.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir)).then_some(path)
}

/// Reads an ordinary file of at most [`MAX_FILE`] bytes.
fn read_file(path: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path).ok()?.take(MAX_FILE + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_FILE).then_some(bytes)
}

/// Reads `relative` inside `dir`, if it really is inside (not through a
/// link to somewhere else).
fn read_in(dir: &Path, relative: &str) -> Option<Vec<u8>> {
    let path = dir.join(inside_relative(relative)?);
    let (Ok(real), Ok(root)) = (path.canonicalize(), dir.canonicalize()) else { return None };
    if !real.starts_with(&root) {
        return None;
    }
    read_file(&real)
}

/// Reads a full path, if it is inside one of `folders`.
fn read_absolute(path: &Path, folders: &[PathBuf]) -> Option<Vec<u8>> {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return None;
    }
    let real = path.canonicalize().ok()?;
    folders
        .iter()
        .filter_map(|f| f.canonicalize().ok())
        .any(|f| real.starts_with(f))
        .then(|| read_file(&real))?
}
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
    if let Some(bytes) = read_file(&assets.dir.join(&hash)) {
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
                folders.iter().find_map(|dir| read_in(dir, &format!("{id}.{e}")).map(|b| (b, e.to_string())))
            })
            .or_else(|| {
                folders.iter().find_map(|dir| read_in(dir, &id)).map(|b| {
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
    if remote_or_device(name) {
        return None;
    }
    let folders = FOLDERS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let path = Path::new(name);
    if path.is_absolute() || path.has_root() {
        let mut allowed = folders.clone();
        allowed.extend(asset_folders());
        return read_absolute(path, &allowed);
    }
    folders.iter().find_map(|dir| read_in(dir, name))
}

/// The asset number in an address such as `rbxassetid://123` or
/// `https://assets.example/asset/?id=123`, if `name` is one.
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
        .find_map(|e| folders.iter().find_map(|dir| read_in(dir, &format!("{id}.{e}"))))
        .or_else(|| folders.iter().find_map(|dir| read_in(dir, id)))
}

/// A path relative to the folders or their `assets` folders.
fn find_file(relative: &str) -> Option<Vec<u8>> {
    let relative = relative.trim_start_matches('/');
    let folders = FOLDERS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    folders.iter().chain(&asset_folders()).find_map(|dir| read_in(dir, relative))
}

/// The file a name means, trying `endings` for asset numbers.
pub fn bytes_for_kind(name: &str, endings: &[&str]) -> Option<Vec<u8>> {
    match asset_id(name) {
        Some(id) => find_asset(&id, endings),
        None => bytes_for(name),
    }
}

/// Decodes a PNG (or JPEG) into an image with 8-bit RGBA pixels.
/// The widest or tallest picture decoded, and the most pixels in one. A
/// small file can say it holds a vast picture; this keeps one from taking
/// gigabytes when it is unpacked (a phone would just close the app).
pub const MAX_PICTURE_SIDE: u32 = 8192;
pub const MAX_PICTURE_PIXELS: u64 = 4096 * 4096;

/// The width and height a PNG or JPEG file says it has, read from its
/// header without unpacking it.
pub fn picture_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        // The first chunk is IHDR: width then height, big-endian.
        let word = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
        return (bytes.get(12..16) == Some(b"IHDR")).then(|| Some((word(16)?, word(20)?)))?;
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        // Walk the markers to the first start-of-frame, which holds the size.
        let mut at = 2;
        while at + 4 <= bytes.len() {
            if bytes[at] != 0xFF {
                return None;
            }
            let marker = bytes[at + 1];
            if marker == 0xFF {
                at += 1;
                continue;
            }
            let len = u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]) as usize;
            let frame = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
            if frame {
                let b = bytes.get(at + 5..at + 9)?;
                let height = u16::from_be_bytes([b[0], b[1]]) as u32;
                let width = u16::from_be_bytes([b[2], b[3]]) as u32;
                return Some((width, height));
            }
            at += 2 + len;
        }
    }
    None
}

/// Whether a picture of this size may be decoded.
fn size_ok((width, height): (u32, u32)) -> bool {
    width > 0
        && height > 0
        && width <= MAX_PICTURE_SIDE
        && height <= MAX_PICTURE_SIDE
        && u64::from(width) * u64::from(height) <= MAX_PICTURE_PIXELS
}

pub fn decode(bytes: &[u8], sampler: ImageSampler) -> Option<Image> {
    if !picture_size(bytes).is_some_and(size_ok) {
        return None;
    }
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

/// The widest a picture on a part is drawn, in pixels: anything bigger is
/// scaled down when it's read. A phone or tablet has far less graphics
/// memory, and a part rarely covers more of its screen than this.
pub const MAX_SIDE: u32 = if cfg!(any(target_os = "android", target_os = "ios")) { 1024 } else { 2048 };

/// Decodes a picture for drawing on parts in the 3D view: no bigger than
/// [`MAX_SIDE`], with every smaller size made up front (so far-off and
/// slanted parts read a small one, which is quicker and doesn't shimmer),
/// and kept only on the graphics side once it's there, instead of a second
/// copy in memory.
pub fn decode_for_view(bytes: &[u8], sampler: ImageSampler) -> Option<Image> {
    let image = decode(bytes, ImageSampler::Default)?;
    let (mut width, mut height) = (image.width(), image.height());
    let mut level = image.data?;
    if level.len() != (width * height * 4) as usize {
        return None;
    }
    while width.max(height) > MAX_SIDE {
        (level, width, height) = halve(&level, width, height);
    }
    let mut data = level.clone();
    let mut levels = 1;
    let (mut w, mut h) = (width, height);
    while w > 1 || h > 1 {
        (level, w, h) = halve(&level, w, h);
        data.extend_from_slice(&level);
        levels += 1;
    }
    let mut image = Image::new_uninit(
        bevy::render::render_resource::Extent3d { width, height, depth_or_array_layers: 1 },
        bevy::render::render_resource::TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = levels;
    image.data = Some(data);
    image.sampler = sampler;
    Some(image)
}

/// Brightness of each sRGB byte as light (pictures are averaged as light,
/// or they'd darken as they shrink).
fn to_light() -> &'static [f32; 256] {
    static TABLE: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        std::array::from_fn(|i| {
            let c = i as f32 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        })
    })
}

/// The sRGB byte for an amount of light (0 to 1), from a table of 4096 steps.
fn from_light(light: f32) -> u8 {
    static TABLE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        (0..4096)
            .map(|i| {
                let l = i as f32 / 4095.0;
                let c = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
                (c * 255.0).round().clamp(0.0, 255.0) as u8
            })
            .collect()
    });
    table[(light.clamp(0.0, 1.0) * 4095.0).round() as usize]
}

/// RGBA pixels at half the size each way (an odd last row or column is
/// folded into the one before), each the average of the up to four it covers.
fn halve(pixels: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let light = to_light();
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let rows =
            if height == 1 { 0..1 } else { y * 2..(y * 2 + 2 + u32::from(y + 1 == h && height % 2 == 1)) };
        for x in 0..w {
            let cols =
                if width == 1 { 0..1 } else { x * 2..(x * 2 + 2 + u32::from(x + 1 == w && width % 2 == 1)) };
            let mut sum = [0.0f32; 4];
            let mut n = 0.0;
            for py in rows.clone() {
                for px in cols.clone() {
                    let i = ((py * width + px) * 4) as usize;
                    let p = &pixels[i..i + 4];
                    // Weighted by how solid each is, so see-through pixels'
                    // colour doesn't bleed into the edge.
                    let a = f32::from(p[3]) / 255.0;
                    for c in 0..3 {
                        sum[c] += light[p[c] as usize] * a;
                    }
                    sum[3] += a;
                    n += 1.0;
                }
            }
            let a = sum[3] / n;
            for c in sum.iter().take(3) {
                out.push(if sum[3] > 0.0 { from_light(c / sum[3]) } else { 0 });
            }
            out.push((a * 255.0).round() as u8);
        }
    }
    (out, w, h)
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
    fn pictures_for_parts_come_with_every_smaller_size() {
        let bytes = bytes_for(&format!("{BUILT_IN}coin")).unwrap();
        let image = decode_for_view(&bytes, ImageSampler::linear()).unwrap();
        // 64, 32, 16, 8, 4, 2 and 1 pixels wide.
        assert_eq!(image.texture_descriptor.mip_level_count, 7);
        let total: u32 = (0..7).map(|l| (64u32 >> l).pow(2) * 4).sum();
        assert_eq!(image.data.as_ref().map(Vec::len), Some(total as usize));
        assert_eq!(image.asset_usage, RenderAssetUsages::RENDER_WORLD);
        // The smallest is the whole coin averaged: partly see-through, gold.
        let last = &image.data.as_ref().unwrap()[total as usize - 4..];
        assert!(last[3] > 0 && last[3] < 255, "{last:?}");
        assert!(last[0] > last[2], "{last:?}");
    }

    #[test]
    fn halving_keeps_flat_colour_and_odd_sizes() {
        let grey = [[200u8, 100, 50, 255]; 15].concat();
        let (half, w, h) = halve(&grey, 5, 3);
        assert_eq!((w, h), (2, 1));
        assert_eq!(&half[..4], &[200, 100, 50, 255]);
        assert_eq!(&half[4..], &[200, 100, 50, 255]);
        let (one, w, h) = halve(&[10, 20, 30, 255], 1, 1);
        assert_eq!((w, h, one), (1, 1, vec![10, 20, 30, 255]));
    }

    #[test]
    fn big_pictures_are_scaled_down() {
        let side = MAX_SIDE * 2;
        let (half, w, h) = halve(&vec![255; (side * 8 * 4) as usize], side, 8);
        assert_eq!((w, h, half.len()), (MAX_SIDE, 4, (MAX_SIDE * 4 * 4) as usize));
    }

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
        assert_eq!(asset_id("http://assets.example/asset/?id=987").as_deref(), Some("987"));
        assert_eq!(asset_id("https://assets.example/asset?id=5&v=2").as_deref(), Some("5"));
        assert_eq!(asset_id("coin.png"), None);
        assert_eq!(asset_id("rbxassetid://"), None);
        assert!(find_file("../secret").is_none());
    }

    #[test]
    fn pictures_that_claim_to_be_huge_are_not_unpacked() {
        let (_, bytes) = PICTURES[0];
        assert_eq!(picture_size(bytes), Some((64, 64)));
        assert_eq!(picture_size(include_bytes!("../assets/cards/baize.jpg")), Some((512, 512)));
        // A real PNG header that says 60000 by 60000 pixels.
        let mut bomb = bytes.to_vec();
        bomb[16..20].copy_from_slice(&60_000u32.to_be_bytes());
        bomb[20..24].copy_from_slice(&60_000u32.to_be_bytes());
        assert_eq!(picture_size(&bomb), Some((60_000, 60_000)));
        assert!(decode(&bomb, ImageSampler::Default).is_none());
        assert!(decode(b"not a picture", ImageSampler::Default).is_none());
    }

    #[test]
    fn a_game_cant_read_files_outside_its_folders() {
        let root = std::env::temp_dir().join(format!("playlibre-pictures-{}", std::process::id()));
        let game = root.join("game");
        let outside = root.join("private");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(game.join("ok.png"), b"inside").unwrap();
        std::fs::write(outside.join("secret.png"), b"secret").unwrap();
        let folders = vec![game.clone()];
        assert_eq!(read_in(&game, "ok.png").as_deref(), Some(&b"inside"[..]));
        assert!(read_in(&game, "../private/secret.png").is_none());
        assert!(read_in(&game, "..\\private\\secret.png").is_none());
        assert_eq!(read_absolute(&game.join("ok.png"), &folders).as_deref(), Some(&b"inside"[..]));
        assert!(read_absolute(&outside.join("secret.png"), &folders).is_none());
        assert!(read_absolute(&game.join("../private/secret.png"), &folders).is_none());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.join("secret.png"), game.join("link.png")).unwrap();
            assert!(read_in(&game, "link.png").is_none(), "a link out of the folder is not followed");
            assert!(read_absolute(Path::new("/dev/zero"), &[PathBuf::from("/dev")]).is_none());
        }
        for share in ["\\\\evil\\share\\x.png", "//evil/share/x.png", "\\\\?\\C:\\x.png", "\\\\.\\pipe\\x"] {
            assert!(remote_or_device(share), "{share}");
            assert!(inside_relative(share).is_none(), "{share}");
        }
        assert!(!remote_or_device("images/coin.png"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
