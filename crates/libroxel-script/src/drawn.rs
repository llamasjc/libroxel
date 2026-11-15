//! Pictures scripts draw pixel by pixel (EditableImage), shared with the
//! renderer by name ("editable://<key>", the value of the object's Image).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// One drawn picture as it was last flushed.
pub struct Drawn {
    /// Changes each time the pixels do, so the renderer knows to upload them.
    pub version: u64,
    /// Width and height in pixels.
    pub size: [usize; 2],
    /// Four bytes a pixel (red, green, blue, alpha), row by row.
    pub rgba: Vec<u8>,
}

static IMAGES: Mutex<Option<HashMap<String, Arc<Drawn>>>> = Mutex::new(None);
static VERSION: AtomicU64 = AtomicU64::new(1);

/// The prefix of the Image value that names a drawn picture.
pub const PREFIX: &str = "editable://";

/// The drawn picture named by an Image value, if there is one.
pub fn image(name: &str) -> Option<Arc<Drawn>> {
    let key = name.strip_prefix(PREFIX)?;
    IMAGES.lock().ok()?.as_ref()?.get(key).cloned()
}

/// A name no other drawn picture has.
pub(crate) fn new_key() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("e{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

pub(crate) fn store(key: &str, size: [usize; 2], rgba: Vec<u8>) {
    let version = VERSION.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut images) = IMAGES.lock() {
        images
            .get_or_insert_with(HashMap::new)
            .insert(key.to_owned(), Arc::new(Drawn { version, size, rgba }));
    }
}

pub(crate) fn forget(key: &str) {
    if let Ok(mut images) = IMAGES.lock()
        && let Some(images) = images.as_mut()
    {
        images.remove(key);
    }
}
