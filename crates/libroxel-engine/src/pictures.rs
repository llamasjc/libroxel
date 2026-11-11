//! Picture files, for image labels, decals and textures.
//!
//! A picture name is a PNG file. A plain name is looked for in each folder
//! given to [`set_folders`] (the editor gives the game file's folder, an
//! `images` folder beside it and the games folder's `images` folder); a full
//! path is used as it is. Names starting with `libroxel://` are pictures
//! built into Libroxel: `coin`, `smile`, `star`, `arrow` and `checker`.

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
];

static FOLDERS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static GENERATION: AtomicU64 = AtomicU64::new(0);

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
    let path = PathBuf::from(name);
    if path.is_absolute() {
        return std::fs::read(path).ok();
    }
    let folders = FOLDERS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    folders.iter().find_map(|dir| std::fs::read(dir.join(&path)).ok())
}

/// Decodes a PNG into an image with 8-bit RGBA pixels.
pub fn decode(bytes: &[u8], sampler: ImageSampler) -> Option<Image> {
    let image = Image::from_buffer(
        bytes,
        ImageType::Extension("png"),
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
            assert_eq!(size, [64, 64], "{name}");
            assert_eq!(pixels.len(), 64 * 64 * 4);
        }
        // The coin's corners are see-through and its middle is gold.
        let (_, pixels) = decode_png(PICTURES[0].1).unwrap();
        assert_eq!(pixels[3], 0);
        let middle = (32 * 64 + 40) * 4;
        assert!(pixels[middle] > 150 && pixels[middle + 3] == 255, "{:?}", &pixels[middle..middle + 4]);
        assert!(bytes_for("libroxel://nothing").is_none());
        assert!(bytes_for("  ").is_none());
    }
}
