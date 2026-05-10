mod imp;

use std::cell::RefCell;
use std::collections::VecDeque;

use adw::subclass::prelude::*;
use glib::Object;
use gio::prelude::*;
use gtk4::prelude::*;

use crate::application::WrenApplication;
use crate::model::FileObject;

/// Visibility for each toggleable list-view column. Snapshot taken
/// at bind time so each row reflects the current preferences.
#[derive(Clone, Copy, Debug)]
pub struct ColumnVisibility {
    pub content_type: bool,
    pub size: bool,
    pub modified: bool,
    pub permissions: bool,
    pub owner: bool,
    pub group: bool,
    pub accessed: bool,
}

impl ColumnVisibility {
    pub fn from_application() -> Self {
        let app = gio::Application::default().and_downcast::<WrenApplication>();
        if let Some(app) = app {
            Self {
                content_type: app.show_col_type(),
                size: app.show_col_size(),
                modified: app.show_col_modified(),
                permissions: app.show_col_permissions(),
                owner: app.show_col_owner(),
                group: app.show_col_group(),
                accessed: app.show_col_accessed(),
            }
        } else {
            Self {
                content_type: true,
                size: true,
                modified: true,
                permissions: false,
                owner: false,
                group: false,
                accessed: false,
            }
        }
    }
}

glib::wrapper! {
    pub struct WrenFileRow(ObjectSubclass<imp::WrenFileRow>)
        @extends gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl Default for WrenFileRow {
    fn default() -> Self {
        Self::new()
    }
}

// ── Folder item-count cache ──────────────────────────────────────────────────
//
// GtkListView aggressively rebinds rows on scroll. A naive implementation would
// re-enumerate every directory on every rebind, so we cache the result keyed
// by URI + show_hidden (the count differs between hidden-on / hidden-off, and
// we'd otherwise bake stale numbers in when the user toggles).
//
// VecDeque eviction (oldest first) bounded at FOLDER_COUNT_CACHE_MAX. The cap
// is well above a typical viewport so users don't churn cache entries while
// scrolling within a single directory.

#[derive(Copy, Clone, PartialEq, Eq)]
enum FolderCountPolicy {
    Always,
    Local,
    Never,
}

const FOLDER_COUNT_CACHE_MAX: usize = 500;

thread_local! {
    static FOLDER_COUNT_CACHE: RefCell<VecDeque<(String, bool, u32)>> =
        RefCell::new(VecDeque::new());

    static FOLDER_COUNT_POLICY: std::cell::Cell<FolderCountPolicy> =
        std::cell::Cell::new(FolderCountPolicy::Always);
}

pub fn set_folder_count_policy(v: &str) {
    let p = match v {
        "never" => FolderCountPolicy::Never,
        "local" => FolderCountPolicy::Local,
        _ => FolderCountPolicy::Always,
    };
    FOLDER_COUNT_POLICY.with(|c| c.set(p));
}

fn current_policy() -> FolderCountPolicy {
    FOLDER_COUNT_POLICY.with(|c| c.get())
}

/// Drop every cached folder count. Called from `WrenWindow::reload()` so that
/// changes made to a folder's contents (or the show_hidden toggle) are picked
/// up on the next bind.
pub fn clear_folder_count_cache() {
    FOLDER_COUNT_CACHE.with(|c| c.borrow_mut().clear());
}

fn cache_lookup(uri: &str, show_hidden: bool) -> Option<u32> {
    FOLDER_COUNT_CACHE.with(|c| {
        c.borrow()
            .iter()
            .find(|(u, h, _)| u == uri && *h == show_hidden)
            .map(|(_, _, n)| *n)
    })
}

fn cache_store(uri: String, show_hidden: bool, count: u32) {
    FOLDER_COUNT_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        // Replace any existing entry first so we don't grow the deque
        // when a refresh recomputes a previously-cached folder.
        if let Some(pos) = c.iter().position(|(u, h, _)| u == &uri && *h == show_hidden) {
            c.remove(pos);
        }
        if c.len() >= FOLDER_COUNT_CACHE_MAX {
            // Drop the oldest 64 to amortise the eviction cost.
            let drop_n = (FOLDER_COUNT_CACHE_MAX / 8).max(1);
            c.drain(..drop_n);
        }
        c.push_back((uri, show_hidden, count));
    });
}

fn format_item_count(n: u32) -> String {
    match n {
        0 => "Empty".to_string(),
        1 => "1 item".to_string(),
        n => format!("{n} items"),
    }
}

impl WrenFileRow {
    pub fn new() -> Self {
        Object::builder().build()
    }

    fn imp(&self) -> &imp::WrenFileRow {
        imp::WrenFileRow::from_obj(self)
    }

    pub fn bound_file_object(&self) -> Option<FileObject> {
        self.imp().bound_file.borrow().clone()
    }

    pub fn bind(&self, file_obj: &FileObject, icon_size: u32, show_extension: bool, show_hidden: bool) {
        let imp = self.imp();
        *imp.bound_file.borrow_mut() = Some(file_obj.clone());
        imp.icon_size.set(icon_size);
        imp.icon.set_pixel_size(icon_size as i32);

        let display_name = if show_extension {
            file_obj.name()
        } else {
            strip_extension_name(&file_obj.name())
        };
        imp.name.set_label(&display_name);

        let cols = ColumnVisibility::from_application();

        if file_obj.is_directory() {
            imp.content_type.set_label("Folder");
            self.bind_folder_count(file_obj, show_hidden);
        } else {
            // Clear any pending-count stamp so an in-flight future from a
            // previously-bound directory bails on completion instead of
            // stomping on this file's size label.
            *imp.pending_count_uri.borrow_mut() = None;
            imp.content_type.set_label(&file_obj.content_type());
            imp.size.set_label(&format_size(file_obj.file_size()));
        }
        imp.content_type.set_visible(cols.content_type);
        imp.size.set_visible(cols.size);

        let ts = file_obj.modified();
        if ts > 0 {
            imp.modified.set_label(&format_modified(ts));
        } else {
            imp.modified.set_label("");
        }
        imp.modified.set_visible(cols.modified);

        // Optional columns. Display "—" when the backend doesn't expose
        // the underlying attribute (typical for trash://, smb://, mtp://).
        if cols.permissions {
            let label = file_obj
                .unix_mode()
                .map(format_permissions)
                .unwrap_or_else(|| "—".to_string());
            imp.permissions.set_label(&label);
        }
        imp.permissions.set_visible(cols.permissions);

        if cols.owner {
            imp.owner
                .set_label(&file_obj.owner_user().unwrap_or_else(|| "—".to_string()));
        }
        imp.owner.set_visible(cols.owner);

        if cols.group {
            imp.group
                .set_label(&file_obj.owner_group().unwrap_or_else(|| "—".to_string()));
        }
        imp.group.set_visible(cols.group);

        if cols.accessed {
            let at = file_obj.accessed();
            let label = if at > 0 {
                format_modified(at)
            } else {
                "—".to_string()
            };
            imp.accessed.set_label(&label);
        }
        imp.accessed.set_visible(cols.accessed);

        if let Some(icon) = file_obj.icon() {
            imp.icon.set_from_gicon(&icon);
        } else if file_obj.is_directory() {
            imp.icon.set_icon_name(Some("folder-symbolic"));
        } else {
            imp.icon.set_icon_name(Some("text-x-generic-symbolic"));
        }
        // See cell::bind for why we use a manual overlay badge
        // instead of EmblemedIcon.
        imp.symlink_badge.set_visible(file_obj.is_symlink());
        imp.unreadable_badge.set_visible(!file_obj.is_readable());

        // Hover tooltip = full path (URI fallback for non-local files).
        let tooltip = file_obj
            .file()
            .path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| file_obj.file().uri().to_string());
        self.set_tooltip_text(Some(&tooltip));
    }

    /// Decide what to show in the size column for a directory row, applying
    /// the user's folder_count_policy. The async enumeration writes back into
    /// the row only if the row is still bound to the same URI — see the
    /// inline comment in the spawned future.
    fn bind_folder_count(&self, file_obj: &FileObject, show_hidden: bool) {
        let imp = self.imp();
        let dir = file_obj.file().clone();
        let uri = dir.uri().to_string();

        // Stamp the URI on the row — the future compares against this on
        // completion to detect that GtkListView recycled the row mid-flight.
        *imp.pending_count_uri.borrow_mut() = Some(uri.clone());

        let policy = current_policy();
        let is_local = dir.uri_scheme().as_deref() == Some("file");

        match policy {
            FolderCountPolicy::Never => {
                imp.size.set_label("—");
                return;
            }
            FolderCountPolicy::Local if !is_local => {
                imp.size.set_label("—");
                return;
            }
            _ => {}
        }

        // Cache hit — synchronous path, no future spawn.
        if let Some(count) = cache_lookup(&uri, show_hidden) {
            imp.size.set_label(&format_item_count(count));
            return;
        }

        imp.size.set_label("…");

        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = row)]
            self,
            async move {
                let Ok(enumerator) = dir
                    .enumerate_children_future(
                        // Need is-hidden to honour show_hidden; name only would
                        // trip g_file_info_get_is_hidden's missing-attr critical.
                        "standard::name,standard::is-hidden",
                        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                        glib::Priority::LOW,
                    )
                    .await
                else {
                    if row_still_bound_to(&row, &uri) {
                        row.imp().size.set_label("—");
                    }
                    return;
                };

                let mut count: u32 = 0;
                loop {
                    // Re-check policy each batch so toggling Always→Never
                    // mid-enumeration aborts a long-running scan.
                    if matches!(current_policy(), FolderCountPolicy::Never) {
                        return;
                    }
                    if !row_still_bound_to(&row, &uri) {
                        return;
                    }
                    let Ok(infos) = enumerator
                        .next_files_future(100, glib::Priority::LOW)
                        .await
                    else {
                        if row_still_bound_to(&row, &uri) {
                            row.imp().size.set_label("—");
                        }
                        return;
                    };
                    if infos.is_empty() {
                        break;
                    }
                    for info in infos {
                        if !show_hidden && info.is_hidden() {
                            continue;
                        }
                        count += 1;
                    }
                }

                cache_store(uri.clone(), show_hidden, count);
                if row_still_bound_to(&row, &uri) {
                    row.imp().size.set_label(&format_item_count(count));
                }
            }
        ));
    }

    pub fn set_icon_size(&self, px: u32) {
        let imp = self.imp();
        imp.icon_size.set(px);
        imp.icon.set_pixel_size(px as i32);
        self.queue_resize();
    }

    pub fn unbind(&self) {
        let imp = self.imp();
        *imp.bound_file.borrow_mut() = None;
        *imp.pending_count_uri.borrow_mut() = None;
        imp.name.set_label("");
        imp.content_type.set_label("");
        imp.size.set_label("");
        imp.modified.set_label("");
        imp.permissions.set_label("");
        imp.owner.set_label("");
        imp.group.set_label("");
        imp.accessed.set_label("");
        imp.icon.set_pixel_size(24);
        imp.icon.clear();
    }
}

fn row_still_bound_to(row: &WrenFileRow, uri: &str) -> bool {
    row.imp()
        .pending_count_uri
        .borrow()
        .as_deref()
        .map_or(false, |u| u == uri)
}

/// Format a unix mode (permission bits + type) as `rwxr-xr-x`.
/// Special bits (setuid, setgid, sticky) are folded into the
/// corresponding execute slot using the standard `s`/`S`/`t`/`T` letters.
fn format_permissions(mode: u32) -> String {
    let mut out = String::with_capacity(9);

    let bit = |b: u32| -> bool { mode & b != 0 };

    let triplet = |out: &mut String, r: u32, w: u32, x: u32, sp: u32, sp_lc: char, sp_uc: char| {
        out.push(if bit(r) { 'r' } else { '-' });
        out.push(if bit(w) { 'w' } else { '-' });
        out.push(match (bit(x), bit(sp)) {
            (true, true) => sp_lc,
            (false, true) => sp_uc,
            (true, false) => 'x',
            (false, false) => '-',
        });
    };

    triplet(&mut out, 0o400, 0o200, 0o100, 0o4000, 's', 'S');
    triplet(&mut out, 0o040, 0o020, 0o010, 0o2000, 's', 'S');
    triplet(&mut out, 0o004, 0o002, 0o001, 0o1000, 't', 'T');
    out
}

fn strip_extension_name(name: &str) -> String {
    if name.starts_with('.') {
        return name.to_string();
    }
    match name.rfind('.') {
        Some(pos) if pos > 0 => name[..pos].to_string(),
        _ => name.to_string(),
    }
}

fn format_modified(ts: i64) -> String {
    let Ok(file_dt) = glib::DateTime::from_unix_local(ts) else {
        return String::new();
    };
    let now_unix = glib::DateTime::now_local()
        .map(|dt| dt.to_unix())
        .unwrap_or(ts);
    let age = now_unix.saturating_sub(ts);

    if age < 60 {
        "Just now".to_string()
    } else if age < 3600 {
        let m = age / 60;
        format!("{m} minute{} ago", if m == 1 { "" } else { "s" })
    } else if age < 86_400 {
        let h = age / 3600;
        format!("{h} hour{} ago", if h == 1 { "" } else { "s" })
    } else if age < 86_400 * 2 {
        "Yesterday".to_string()
    } else if age < 86_400 * 7 {
        let d = age / 86_400;
        format!("{d} days ago")
    } else {
        file_dt
            .format("%e %b %Y")
            .map(|s| s.to_string())
            .unwrap_or_default()
    }
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::{format_size, strip_extension_name, WrenFileRow};
    use crate::model::FileObject;
    use crate::test_helpers::run_on_gtk_thread;

    fn make_file_object(name: &str, kind: gio::FileType, size: i64) -> FileObject {
        let info = gio::FileInfo::new();
        info.set_name(std::path::Path::new(name));
        info.set_display_name(name);
        info.set_file_type(kind);
        info.set_size(size);
        info.set_attribute_uint64("standard::size", size as u64);
        info.set_content_type("text/plain");
        info.set_icon(&gio::ThemedIcon::new("text-x-generic"));
        info.set_is_hidden(name.starts_with('.'));
        info.set_is_symlink(false);
        let file = gio::File::for_path(format!("/tmp/wren-test/{name}"));
        FileObject::new(file, info)
    }

    #[test]
    fn row_bind_file_shows_size_and_type() {
        run_on_gtk_thread(|| {
            let row = WrenFileRow::new();
            let fo = make_file_object("data.bin", gio::FileType::Regular, 2048);
            row.bind(&fo, 24, true);
            // Inspect template children directly via imp().
            let imp = row.imp();
            assert_eq!(imp.name.label().as_str(), "data.bin");
            assert_eq!(imp.size.label().as_str(), "2 KB");
            assert_eq!(imp.content_type.label().as_str(), "text/plain");
        });
    }

    #[test]
    fn row_bind_directory_shows_folder_label_and_em_dash_size() {
        run_on_gtk_thread(|| {
            let row = WrenFileRow::new();
            let fo = make_file_object("subdir", gio::FileType::Directory, 0);
            row.bind(&fo, 24, true);
            let imp = row.imp();
            assert_eq!(imp.content_type.label().as_str(), "Folder");
            // Directories don't display a meaningful size — em-dash placeholder.
            assert_eq!(imp.size.label().as_str(), "—");
        });
    }

    #[test]
    fn row_bind_hides_extension_when_disabled() {
        run_on_gtk_thread(|| {
            let row = WrenFileRow::new();
            let fo = make_file_object("photo.jpg", gio::FileType::Regular, 100);
            row.bind(&fo, 24, false);
            assert_eq!(row.imp().name.label().as_str(), "photo");
        });
    }

    #[test]
    fn row_unbind_clears_all_columns() {
        run_on_gtk_thread(|| {
            let row = WrenFileRow::new();
            let fo = make_file_object("any.txt", gio::FileType::Regular, 50);
            row.bind(&fo, 24, true);
            row.unbind();
            let imp = row.imp();
            assert_eq!(imp.name.label().as_str(), "");
            assert_eq!(imp.size.label().as_str(), "");
            assert_eq!(imp.content_type.label().as_str(), "");
            assert_eq!(imp.modified.label().as_str(), "");
            assert!(row.bound_file_object().is_none());
        });
    }

    #[test]
    fn strip_extension_name_drops_simple_ext() {
        assert_eq!(strip_extension_name("README.md"), "README");
    }

    #[test]
    fn strip_extension_name_multi_dot_keeps_inner_extension() {
        assert_eq!(strip_extension_name("archive.tar.gz"), "archive.tar");
    }

    #[test]
    fn strip_extension_name_dotfile_intact() {
        assert_eq!(strip_extension_name(".bashrc"), ".bashrc");
    }

    #[test]
    fn strip_extension_name_apple_double_intact() {
        assert_eq!(strip_extension_name("._foo"), "._foo");
    }

    #[test]
    fn strip_extension_name_no_extension() {
        assert_eq!(strip_extension_name("Makefile"), "Makefile");
    }

    #[test]
    fn format_size_bytes_under_kilobyte() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1), "1 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn format_size_kilobytes() {
        assert_eq!(format_size(1024), "1 KB");
        // Rounded to 0 decimal places.
        assert_eq!(format_size(1536), "2 KB");
    }

    #[test]
    fn format_size_megabytes() {
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 * 5 / 2), "2.5 MB");
    }

    #[test]
    fn format_size_gigabytes() {
        assert_eq!(format_size(1024u64.pow(3)), "1.0 GB");
        assert_eq!(format_size(1024u64.pow(3) * 3), "3.0 GB");
    }

    #[test]
    fn format_size_threshold_boundary() {
        // Just below KB cutoff stays in bytes.
        assert_eq!(format_size(1023), "1023 B");
        // At KB boundary switches units.
        assert_eq!(format_size(1024), "1 KB");
    }
}
