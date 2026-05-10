//! Disk usage indicator for the status bar. Resolves the enclosing mount
//! for a `gio::File`, queries `filesystem::free` / `filesystem::size` /
//! `filesystem::type`, and formats the suffix that `update_status_bar`
//! appends to the per-tab status label.
//!
//! Two concerns drove the design:
//!  - `find_enclosing_mount` is synchronous-only in gtk-rs 0.11 and can be
//!    slow for non-local backends. Multiple directories often share a
//!    mount, so cache by resolved mount root path.
//!  - The status bar repaints on every selection change. The synchronous
//!    fast path returns whatever we have cached so the suffix doesn't
//!    flicker; a stale entry triggers a background refresh.
//!
//! Networked / virtual filesystems (`trash:///`, `recent:///`, `sftp://`)
//! often don't expose these attributes; in that case the helper returns
//! `None` and the caller leaves the existing `N items` text untouched.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gtk4::prelude::*;

use crate::window::file_ops::format_file_size;

const CACHE_TTL: Duration = Duration::from_secs(5);
const FS_ATTRS: &str = "filesystem::free,filesystem::size,filesystem::type";

#[derive(Clone)]
pub struct DiskUsage {
    pub free: u64,
    pub total: u64,
    pub fs_type: Option<String>,
    pub mount_root: Option<PathBuf>,
    pub mount_name: Option<String>,
}

/// Cache key — prefer the resolved mount root path; fall back to the
/// directory's own path when no mount is reachable (single-user file://
/// case is the common one and find_enclosing_mount can fail on some
/// systems even though querying filesystem info works fine).
#[derive(Clone, Eq, Hash, PartialEq)]
enum CacheKey {
    Mount(PathBuf),
    Dir(PathBuf),
}

thread_local! {
    static CACHE: RefCell<HashMap<CacheKey, (Instant, DiskUsage)>> = RefCell::new(HashMap::new());
}

fn cache_get(key: &CacheKey) -> Option<DiskUsage> {
    CACHE.with(|c| {
        let map = c.borrow();
        let (when, usage) = map.get(key)?;
        if when.elapsed() < CACHE_TTL {
            Some(usage.clone())
        } else {
            None
        }
    })
}

fn cache_put(key: CacheKey, usage: DiskUsage) {
    CACHE.with(|c| {
        c.borrow_mut().insert(key, (Instant::now(), usage));
    });
}

/// Drop any cached entry covering `file`. Called from `reload()` after a
/// write op so the next render shows updated free-space.
pub fn invalidate_for(file: &gio::File) {
    let mount = resolve_mount(file);
    if let Some(key) = cache_key_for(file, mount.as_ref()) {
        CACHE.with(|c| {
            c.borrow_mut().remove(&key);
        });
    }
}

/// Best-effort mount resolution. Returns `None` for virtual schemes that
/// don't have an enclosing mount.
fn resolve_mount(file: &gio::File) -> Option<gio::Mount> {
    file.find_enclosing_mount(gio::Cancellable::NONE).ok()
}

fn cache_key_for(file: &gio::File, mount: Option<&gio::Mount>) -> Option<CacheKey> {
    if let Some(m) = mount {
        if let Some(p) = m.root().path() {
            return Some(CacheKey::Mount(p));
        }
    }
    file.path().map(CacheKey::Dir)
}

/// Format the right-aligned suffix the status bar appends.
pub fn format_suffix(u: &DiskUsage) -> String {
    format!(
        " \u{00B7} {} free of {}",
        format_file_size(u.free),
        format_file_size(u.total)
    )
}

/// Format the tooltip shown on hover. Includes mount point + filesystem
/// type when available.
pub fn format_tooltip(u: &DiskUsage) -> String {
    let mount = u
        .mount_root
        .as_ref()
        .map(|p| p.display().to_string())
        .or_else(|| u.mount_name.clone())
        .unwrap_or_else(|| "(unknown)".to_string());
    let fs = u.fs_type.as_deref().unwrap_or("unknown");
    format!(
        "Mount: {}\nFilesystem: {}\n{} free of {}",
        mount,
        fs,
        format_file_size(u.free),
        format_file_size(u.total)
    )
}

/// Read filesystem stats for `file`'s enclosing mount. Async — uses the
/// `query_filesystem_info_future` GIO future. Returns `None` when the
/// backend doesn't expose size/free (typical for `trash:///`,
/// `recent:///`, some network mounts).
pub async fn fetch(file: gio::File) -> Option<DiskUsage> {
    let info = file
        .query_filesystem_info_future(FS_ATTRS, glib::Priority::DEFAULT)
        .await
        .ok()?;
    if !info.has_attribute("filesystem::size") || !info.has_attribute("filesystem::free") {
        return None;
    }
    let total = info.attribute_uint64("filesystem::size");
    let free = info.attribute_uint64("filesystem::free");
    if total == 0 {
        return None;
    }
    let fs_type = info.attribute_string("filesystem::type").map(|s| s.to_string());
    let mount = resolve_mount(&file);
    let mount_root = mount.as_ref().and_then(|m| m.root().path());
    let mount_name = mount.as_ref().map(|m| m.name().to_string());
    Some(DiskUsage {
        free,
        total,
        fs_type,
        mount_root,
        mount_name,
    })
}

/// Synchronous cached lookup. Returns a hit only if the entry exists
/// AND is fresh. Used by `update_status_bar` for an instantaneous render
/// without async churn.
pub fn cached_for(file: &gio::File) -> Option<DiskUsage> {
    let mount = resolve_mount(file);
    let key = cache_key_for(file, mount.as_ref())?;
    cache_get(&key)
}

/// Refresh the cache for `file` and apply the result to `label` if and
/// only if the label is still alive (weak ref) and has not been hijacked
/// by a newer call (the caller passes the base text and we re-set the
/// whole label so a stale future can't overwrite a fresher one — the
/// last write of update_status_bar always wins because it's reentrant
/// from the tab switch / selection path).
///
/// `base_text` is the `N items, M selected (...)` string already
/// computed by `update_status_bar`; we append the disk-usage suffix.
pub fn refresh_async(file: gio::File, label: gtk4::Label, base_text: String) {
    glib::spawn_future_local(async move {
        let Some(usage) = fetch(file.clone()).await else {
            return;
        };
        let mount = resolve_mount(&file);
        if let Some(key) = cache_key_for(&file, mount.as_ref()) {
            cache_put(key, usage.clone());
        }
        // Only apply if the label's current text matches what we
        // captured: avoids racing with another `update_status_bar` that
        // ran after we kicked off the future (e.g. selection changed
        // while the I/O was in flight). If it doesn't match, that other
        // call will have either set its own suffix from cache or
        // spawned its own refresh — either way our result is stale.
        if label.text() == base_text.as_str() {
            label.set_text(&format!("{}{}", base_text, format_suffix(&usage)));
            label.set_tooltip_text(Some(&format_tooltip(&usage)));
        }
    });
}
