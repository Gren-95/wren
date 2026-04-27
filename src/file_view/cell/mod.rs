mod imp;

use adw::subclass::prelude::*;
use glib::Object;
use gtk4::prelude::*;

use crate::model::FileObject;

glib::wrapper! {
    pub struct WrenFileCell(ObjectSubclass<imp::WrenFileCell>)
        @extends gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl Default for WrenFileCell {
    fn default() -> Self {
        Self::new()
    }
}

// ── Thumbnail texture cache ───────────────────────────────────────────────────
//
// gdk::Texture is a GObject reference type. Caching them keeps the decoded
// image in memory and — critically — keeps the GL texture resident in VRAM
// so the renderer can draw it without re-uploading on every cell recycle.
//
// The cache lives on the main thread (thread_local) so no locking is needed.
// When it fills past TEXTURE_CACHE_MAX we evict the oldest quarter by
// insertion order; this is good enough for the common case of scrolling back
// through a directory that was already visited.

use std::collections::VecDeque;
use std::path::PathBuf;

const TEXTURE_CACHE_MAX: usize = 256;

thread_local! {
    // (path, texture) in insertion order so we can evict the oldest entries.
    static TEXTURE_CACHE: std::cell::RefCell<VecDeque<(PathBuf, gtk4::gdk::Texture)>> =
        std::cell::RefCell::new(VecDeque::new());
}

fn cached_texture(path: &PathBuf) -> Option<gtk4::gdk::Texture> {
    TEXTURE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();

        // Fast path: already cached
        if let Some(entry) = cache.iter().find(|(p, _)| p == path) {
            return Some(entry.1.clone());
        }

        // Decode from disk
        let tex = gtk4::gdk::Texture::from_filename(path).ok()?;

        // Evict oldest 1/4 when at capacity to keep memory bounded
        if cache.len() >= TEXTURE_CACHE_MAX {
            let evict = cache.len() / 4;
            cache.drain(..evict);
        }

        cache.push_back((path.clone(), tex.clone()));
        Some(tex)
    })
}

// ─────────────────────────────────────────────────────────────────────────────

impl WrenFileCell {
    pub fn new() -> Self {
        Object::builder().build()
    }

    fn imp(&self) -> &imp::WrenFileCell {
        imp::WrenFileCell::from_obj(self)
    }

    pub fn bind(&self, file_obj: &FileObject, icon_size: u32) {
        let imp = self.imp();
        let px = icon_size as i32;
        imp.name.set_label(&file_obj.name());

        let mut thumb_loaded = false;
        if let Some(thumb_path) = file_obj.thumbnail_path() {
            if let Some(texture) = cached_texture(&thumb_path) {
                imp.icon.set_pixel_size(-1);
                imp.icon.set_size_request(px, px);
                imp.icon.set_paintable(Some(&texture));
                thumb_loaded = true;
            }
        }

        if !thumb_loaded {
            imp.icon.set_size_request(-1, -1);
            imp.icon.set_pixel_size(px);
            if let Some(icon) = file_obj.icon() {
                imp.icon.set_from_gicon(&icon);
            } else if file_obj.is_directory() {
                imp.icon.set_icon_name(Some("folder-symbolic"));
            } else {
                imp.icon.set_icon_name(Some("text-x-generic-symbolic"));
            }
        }
    }

    pub fn unbind(&self) {
        let imp = self.imp();
        imp.name.set_label("");
        imp.icon.set_size_request(-1, -1);
        imp.icon.set_pixel_size(64);
        imp.icon.clear();
    }
}
