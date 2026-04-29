mod imp;

use adw::subclass::prelude::*;
use glib::Object;
use gtk4::prelude::*;

use crate::model::FileObject;

extern crate gdk_pixbuf;

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
// Two-tier cache so zoom changes only need in-memory scaling (no disk I/O).
//
// PIXBUF_CACHE  – raw thumbnail pixbuf at the file's native size (≤256×256).
//                 Populated once per file on first access; survives zoom changes.
// TEXTURE_CACHE – cover-cropped, px-sized GdkTexture.  Cache hit = zero work.
//
// Keeping GdkTexture alive keeps the GL texture resident in VRAM, avoiding
// re-upload on every cell recycle.

use std::collections::VecDeque;
use std::path::PathBuf;

const PIXBUF_CACHE_MAX: usize = 512;
const TEXTURE_CACHE_MAX: usize = 512;

thread_local! {
    static PIXBUF_CACHE: std::cell::RefCell<VecDeque<(PathBuf, gdk_pixbuf::Pixbuf)>> =
        std::cell::RefCell::new(VecDeque::new());

    static TEXTURE_CACHE: std::cell::RefCell<VecDeque<(PathBuf, i32, gtk4::gdk::Texture)>> =
        std::cell::RefCell::new(VecDeque::new());
}

// Cover-crop: scale so shorter dimension = px, then crop center to px×px.
fn cover_from_raw(raw: &gdk_pixbuf::Pixbuf, px: i32) -> Option<gdk_pixbuf::Pixbuf> {
    let rw = raw.width();
    let rh = raw.height();
    let (tw, th) = if rw <= rh {
        (px, ((rh as f32 * px as f32 / rw as f32).round() as i32).max(px))
    } else {
        (((rw as f32 * px as f32 / rh as f32).round() as i32).max(px), px)
    };
    let scaled = raw.scale_simple(tw, th, gdk_pixbuf::InterpType::Bilinear)?;
    let x = (scaled.width() - px) / 2;
    let y = (scaled.height() - px) / 2;
    Some(scaled.new_subpixbuf(x, y, px, px))
}

fn cached_texture(path: &PathBuf, px: i32) -> Option<gtk4::gdk::Texture> {
    let hit = TEXTURE_CACHE.with(|c| {
        c.borrow().iter()
            .find(|(p, s, _)| p == path && *s == px)
            .map(|e| e.2.clone())
    });
    if let Some(tex) = hit {
        return Some(tex);
    }

    let raw = PIXBUF_CACHE.with(|c| {
        c.borrow().iter().find(|(p, _)| p == path).map(|e| e.1.clone())
    });

    let raw = match raw {
        Some(pb) => pb,
        None => {
            let pb = gdk_pixbuf::Pixbuf::from_file_at_scale(path, 256, 256, true).ok()?;
            PIXBUF_CACHE.with(|c| {
                let mut c = c.borrow_mut();
                if c.len() >= PIXBUF_CACHE_MAX {
                    let evict = c.len() / 4;
                    c.drain(..evict);
                }
                c.push_back((path.clone(), pb.clone()));
            });
            pb
        }
    };

    let pixbuf = cover_from_raw(&raw, px)?;
    let tex = gtk4::gdk::Texture::for_pixbuf(&pixbuf);

    TEXTURE_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= TEXTURE_CACHE_MAX {
            let evict = c.len() / 4;
            c.drain(..evict);
        }
        c.push_back((path.clone(), px, tex.clone()));
    });

    Some(tex)
}

pub fn clear_thumbnail_cache() {
    PIXBUF_CACHE.with(|c| c.borrow_mut().clear());
    TEXTURE_CACHE.with(|c| c.borrow_mut().clear());
}

fn strip_extension(name: &str) -> String {
    if name.starts_with('.') {
        return name.to_string();
    }
    match name.rfind('.') {
        Some(pos) if pos > 0 => name[..pos].to_string(),
        _ => name.to_string(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────

impl WrenFileCell {
    pub fn new() -> Self {
        Object::builder().build()
    }

    fn imp(&self) -> &imp::WrenFileCell {
        imp::WrenFileCell::from_obj(self)
    }

    // Render just the icon portion at a given size; label is unchanged.
    fn render_icon(&self, file_obj: &FileObject, px: i32) {
        let imp = self.imp();
        // pixel_size drives get_content_size() for ALL storage types (paintable
        // and icon alike), so gdk_paintable_snapshot is called with (px, px).
        imp.icon.set_pixel_size(px);

        if let Some(thumb_path) = file_obj.thumbnail_path() {
            if let Some(texture) = cached_texture(&thumb_path, px) {
                imp.icon.set_paintable(Some(&texture));
                return;
            }
        }

        if let Some(icon) = file_obj.icon() {
            imp.icon.set_from_gicon(&icon);
        } else if file_obj.is_directory() {
            imp.icon.set_icon_name(Some("folder-symbolic"));
        } else {
            imp.icon.set_icon_name(Some("text-x-generic-symbolic"));
        }
    }

    pub fn bind(&self, file_obj: &FileObject, icon_size: u32, show_extension: bool) {
        let imp = self.imp();
        let px = icon_size as i32;

        // Store so set_icon_size can re-render without a model signal.
        *imp.bound_file.borrow_mut() = Some(file_obj.clone());
        imp.icon_size.set(icon_size);

        let display_name = if show_extension {
            file_obj.name()
        } else {
            strip_extension(&file_obj.name())
        };
        imp.name.set_label(&display_name);
        self.render_icon(file_obj, px);
    }

    /// Called by WrenFileGrid on zoom — updates icon in-place, no factory replacement.
    pub fn set_icon_size(&self, px: u32) {
        let imp = self.imp();
        imp.icon_size.set(px);
        let bound = imp.bound_file.borrow();
        if let Some(file_obj) = bound.as_ref() {
            self.render_icon(file_obj, px as i32);
        }
        self.queue_resize();
    }

    pub fn unbind(&self) {
        let imp = self.imp();
        *imp.bound_file.borrow_mut() = None;
        imp.name.set_label("");
        imp.icon.set_pixel_size(64);
        imp.icon.clear();
    }
}
