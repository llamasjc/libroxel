//! Pictures for image labels and buttons, drawn with egui. Finding and
//! decoding them is [`libroxel_engine::pictures`]; each one is decoded
//! once and kept.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;

use bevy_egui::egui;
use libroxel_engine::pictures;

thread_local! {
    static LOADED: RefCell<HashMap<String, Option<(egui::TextureHandle, egui::Vec2)>>> =
        RefCell::new(HashMap::new());
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// Where plain picture names are looked for, first to last.
pub fn set_folders(folders: Vec<PathBuf>) {
    pictures::set_folders(folders);
}

/// The picture called `name`, ready to draw, and its size in pixels.
pub fn texture(ctx: &egui::Context, name: &str) -> Option<(egui::TextureId, egui::Vec2)> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    LOADED.with(|loaded| {
        let mut loaded = loaded.borrow_mut();
        let generation = pictures::generation();
        if GENERATION.with(|g| g.replace(generation)) != generation {
            // A different game: names may mean other files now.
            loaded.clear();
        }
        let slot = loaded.entry(name.to_owned()).or_insert_with(|| {
            let (size, pixels) = pictures::decode_png(&pictures::bytes_for(name)?)?;
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
