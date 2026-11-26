//! Games' own icons, from the Hub, drawn where the made-up picture of a
//! game's blocks goes when it has none. Each icon is fetched once, in the
//! background, and kept.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bevy_egui::egui;
use playlibre_hub::{HubClient, MediaRef};

type Fetched = Arc<Mutex<HashMap<String, Option<(egui::Vec2, Vec<u8>)>>>>;

thread_local! {
    static TEXTURES: std::cell::RefCell<HashMap<String, Option<egui::TextureHandle>>> =
        std::cell::RefCell::new(HashMap::new());
}

fn fetched() -> &'static Fetched {
    static FETCHED: std::sync::OnceLock<Fetched> = std::sync::OnceLock::new();
    FETCHED.get_or_init(Fetched::default)
}

/// The icon for a game, once it has arrived.
fn icon(ctx: &egui::Context, game_id: &str, media: &[MediaRef]) -> Option<egui::TextureHandle> {
    let m = media.iter().find(|m| m.slot == 0)?;
    let key = format!("{game_id}/{}", m.version);
    if let Some(found) = TEXTURES.with(|t| t.borrow().get(&key).cloned()) {
        return found;
    }
    let mut all = fetched().lock().unwrap();
    match all.get(&key) {
        // Still on its way.
        None if all.contains_key(&key) => None,
        None => {
            all.insert(key.clone(), None);
            drop(all);
            let (id, m, key2) = (game_id.to_owned(), *m, key.clone());
            let ctx2 = ctx.clone();
            std::thread::spawn(move || {
                let hub = HubClient::new(&playlibre_hub::hub_address());
                let decoded =
                    hub.media(&id, &m).ok().and_then(|bytes| playlibre_engine::pictures::decode_png(&bytes));
                let mut all = fetched().lock().unwrap();
                match decoded {
                    Some((size, rgba)) => {
                        all.insert(key2, Some((egui::vec2(size[0] as f32, size[1] as f32), rgba)));
                    }
                    // Asked for again next time it is drawn.
                    None => {
                        all.remove(&key2);
                    }
                }
                ctx2.request_repaint();
            });
            None
        }
        Some(Some((size, rgba))) => {
            let image = egui::ColorImage::from_rgba_unmultiplied([size.x as usize, size.y as usize], rgba);
            let texture = ctx.load_texture(format!("game-icon:{key}"), image, egui::TextureOptions::LINEAR);
            TEXTURES.with(|t| t.borrow_mut().insert(key, Some(texture.clone())));
            Some(texture)
        }
        Some(None) => None,
    }
}

/// Paints a game's icon over `rect` (cut to fill it), or its blocks when it
/// has no icon (or it hasn't arrived yet).
pub fn paint(painter: &egui::Painter, rect: egui::Rect, game_id: &str, media: &[MediaRef]) {
    let Some(texture) = icon(painter.ctx(), game_id, media) else {
        crate::menu::paint_blocks(painter, rect, game_id);
        return;
    };
    // Cover: the middle of the square icon, as wide as the rect.
    let aspect = rect.width() / rect.height().max(1.0);
    let uv = if aspect >= 1.0 {
        let h = 1.0 / aspect;
        egui::Rect::from_min_max(egui::pos2(0.0, 0.5 - h / 2.0), egui::pos2(1.0, 0.5 + h / 2.0))
    } else {
        let w = aspect;
        egui::Rect::from_min_max(egui::pos2(0.5 - w / 2.0, 0.0), egui::pos2(0.5 + w / 2.0, 1.0))
    };
    painter.image(texture.id(), rect, uv, egui::Color32::WHITE);
}
