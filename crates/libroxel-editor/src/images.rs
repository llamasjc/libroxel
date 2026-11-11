//! Pictures for image labels and buttons.
//!
//! An `Image` names a PNG file. A plain name is looked for beside the game
//! file, in an `images` folder beside it, then in the games folder's
//! `images` folder; a full path is used as it is. Names starting with
//! `libroxel://` are pictures built into Libroxel. Each picture is decoded
//! once and kept.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, Image, ImageSampler, ImageType};
use bevy::render::render_resource::TextureFormat;
use bevy_egui::egui;

/// The start of the names of built-in pictures.
pub const BUILT_IN: &str = "libroxel://";

/// Built-in pictures, by name after [`BUILT_IN`].
const PICTURES: &[(&str, &[u8])] = &[("coin", include_bytes!("../assets/coin.png"))];

thread_local! {
    static FOLDERS: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    static LOADED: RefCell<HashMap<String, Option<(egui::TextureHandle, egui::Vec2)>>> =
        RefCell::new(HashMap::new());
}

/// Where plain picture names are looked for, first to last.
pub fn set_folders(folders: Vec<PathBuf>) {
    FOLDERS.with(|f| {
        let mut f = f.borrow_mut();
        if *f != folders {
            *f = folders;
            // A different game: names may mean other files now.
            LOADED.with(|l| l.borrow_mut().clear());
        }
    });
}

/// A PNG's size and pixels (RGBA, 8 bits each, row by row).
pub fn decode_png(bytes: &[u8]) -> Option<([usize; 2], Vec<u8>)> {
    let image = Image::from_buffer(
        bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::default(),
    )
    .ok()?;
    let image = match image.texture_descriptor.format {
        TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm => image,
        _ => image.convert(TextureFormat::Rgba8UnormSrgb)?,
    };
    let size = [image.width() as usize, image.height() as usize];
    let data = image.data?;
    (data.len() == size[0] * size[1] * 4).then_some((size, data))
}

fn bytes_for(name: &str) -> Option<Vec<u8>> {
    if let Some(built_in) = name.strip_prefix(BUILT_IN) {
        return PICTURES.iter().find(|(n, _)| *n == built_in).map(|(_, b)| b.to_vec());
    }
    let path = PathBuf::from(name);
    if path.is_absolute() {
        return std::fs::read(path).ok();
    }
    FOLDERS.with(|folders| folders.borrow().iter().find_map(|dir| std::fs::read(dir.join(&path)).ok()))
}

/// The picture called `name`, ready to draw, and its size in pixels.
pub fn texture(ctx: &egui::Context, name: &str) -> Option<(egui::TextureId, egui::Vec2)> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    LOADED.with(|loaded| {
        let mut loaded = loaded.borrow_mut();
        let slot = loaded.entry(name.to_owned()).or_insert_with(|| {
            let (size, pixels) = decode_png(&bytes_for(name)?)?;
            let picture = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
            let handle = ctx.load_texture(format!("image:{name}"), picture, egui::TextureOptions::LINEAR);
            Some((handle, egui::vec2(size[0] as f32, size[1] as f32)))
        });
        slot.as_ref().map(|(handle, size)| (handle.id(), *size))
    })
}

/// Where a picture of `size` goes in `rect`, and which part of it shows,
/// for each way of scaling it.
pub fn placement(
    rect: egui::Rect,
    size: egui::Vec2,
    scale: libroxel_instance::ScaleType,
) -> (egui::Rect, egui::Rect) {
    use libroxel_instance::ScaleType;
    let full = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    if size.x <= 0.0 || size.y <= 0.0 || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return (rect, full);
    }
    let fit = (rect.width() / size.x).min(rect.height() / size.y);
    match scale {
        ScaleType::Stretch => (rect, full),
        ScaleType::Fit => (egui::Rect::from_center_size(rect.center(), size * fit), full),
        ScaleType::Crop => {
            // Cover the box; show the middle of the picture.
            let cover = (rect.width() / size.x).max(rect.height() / size.y);
            let shown = egui::vec2(rect.width() / (size.x * cover), rect.height() / (size.y * cover));
            let min = egui::pos2((1.0 - shown.x) / 2.0, (1.0 - shown.y) / 2.0);
            (rect, egui::Rect::from_min_size(min, shown))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libroxel_instance::ScaleType;

    #[test]
    fn the_built_in_coin_decodes() {
        let (size, pixels) = decode_png(PICTURES[0].1).expect("decodes");
        assert_eq!(size, [64, 64]);
        // Corners are see-through, the middle is gold.
        assert_eq!(pixels[3], 0);
        let middle = (32 * 64 + 40) * 4;
        assert!(pixels[middle] > 150 && pixels[middle + 3] == 255, "{:?}", &pixels[middle..middle + 4]);
    }

    #[test]
    fn pictures_fit_crop_and_stretch() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 100.0));
        let square = egui::vec2(64.0, 64.0);
        let (at, uv) = placement(rect, square, ScaleType::Fit);
        assert_eq!(at, egui::Rect::from_min_size(egui::pos2(50.0, 0.0), egui::vec2(100.0, 100.0)));
        assert_eq!(uv.size(), egui::vec2(1.0, 1.0));
        let (at, uv) = placement(rect, square, ScaleType::Crop);
        assert_eq!(at, rect);
        assert_eq!(uv, egui::Rect::from_min_max(egui::pos2(0.0, 0.25), egui::pos2(1.0, 0.75)));
        assert_eq!(placement(rect, square, ScaleType::Stretch).0, rect);
    }
}
