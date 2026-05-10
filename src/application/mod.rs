mod imp;

use adw::subclass::prelude::ObjectSubclassIsExt;
use glib::Object;

/// Persisted per-tab UI state. Serialised as
/// `uri|sort_key|asc|grid` under `[Tabs] tabN=`. The pipe separator is
/// safe because `|` is reserved by RFC 3986 and never appears in a URI.
#[derive(Debug, Clone)]
pub struct TabPref {
    pub uri: String,
    pub sort_key: String,
    pub reversed: bool,
    pub view_mode: String,
}

impl TabPref {
    pub(crate) fn to_line(&self) -> String {
        let dir = if self.reversed { "desc" } else { "asc" };
        let view = if self.view_mode == "list" { "list" } else { "grid" };
        let key = match self.sort_key.as_str() {
            "size" | "date" | "type" => self.sort_key.as_str(),
            _ => "name",
        };
        format!("{}|{}|{}|{}", self.uri, key, dir, view)
    }

    pub(crate) fn parse_line(s: &str) -> Option<Self> {
        let mut parts = s.splitn(4, '|');
        let uri = parts.next()?.to_string();
        if uri.is_empty() {
            return None;
        }
        let key = match parts.next().unwrap_or("name") {
            "size" => "size",
            "date" => "date",
            "type" => "type",
            _ => "name",
        }
        .to_string();
        let reversed = matches!(parts.next().unwrap_or("asc"), "desc");
        let view = match parts.next().unwrap_or("grid") {
            "list" => "list",
            _ => "grid",
        }
        .to_string();
        Some(Self {
            uri,
            sort_key: key,
            reversed,
            view_mode: view,
        })
    }
}

/// Default number of locations kept in the sidebar's Recent section.
pub const RECENTS_DEFAULT: usize = 10;
/// Bounds for the user-configurable recents cap.
pub const RECENTS_MIN: usize = 1;
pub const RECENTS_MAX: usize = 50;

/// Default size (entries) of the thumbnail texture cache.
pub const THUMB_CACHE_DEFAULT: usize = 256;
/// Bounds for the user-configurable thumbnail cache size.
pub const THUMB_CACHE_MIN: usize = 64;
pub const THUMB_CACHE_MAX: usize = 1024;

glib::wrapper! {
    pub struct WrenApplication(ObjectSubclass<imp::WrenApplication>)
        @extends adw::Application, gtk4::Application, gio::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl WrenApplication {
    pub fn new(app_id: &str) -> Self {
        Object::builder()
            .property("application-id", app_id)
            .property("flags", gio::ApplicationFlags::HANDLES_OPEN)
            .build()
    }

    pub fn terminal_cmd(&self) -> String {
        self.imp().terminal_cmd.borrow().clone()
    }

    pub fn set_terminal_cmd(&self, cmd: &str) {
        *self.imp().terminal_cmd.borrow_mut() = cmd.to_string();
        self.imp().save_settings();
    }

    pub fn show_hidden(&self) -> bool { self.imp().show_hidden.get() }
    pub fn set_show_hidden(&self, v: bool) { self.imp().show_hidden.set(v); self.imp().save_settings(); }

    pub fn show_extensions(&self) -> bool { self.imp().show_extensions.get() }
    pub fn set_show_extensions(&self, v: bool) { self.imp().show_extensions.set(v); self.imp().save_settings(); }

    pub fn zoom_level(&self) -> i32 { self.imp().zoom_level.get() }
    pub fn set_zoom_level(&self, v: i32) { self.imp().zoom_level.set(v); self.imp().save_settings(); }

    pub fn view_mode(&self) -> String { self.imp().view_mode.borrow().clone() }
    pub fn set_view_mode_pref(&self, v: &str) { *self.imp().view_mode.borrow_mut() = v.to_string(); self.imp().save_settings(); }

    pub fn sort_key(&self) -> String { self.imp().sort_key.borrow().clone() }
    pub fn set_sort_key_pref(&self, v: &str) { *self.imp().sort_key.borrow_mut() = v.to_string(); self.imp().save_settings(); }

    pub fn sort_reversed(&self) -> bool { self.imp().sort_reversed.get() }
    pub fn set_sort_reversed_pref(&self, v: bool) { self.imp().sort_reversed.set(v); self.imp().save_settings(); }

    pub fn color_scheme(&self) -> String { self.imp().color_scheme.borrow().clone() }
    pub fn set_color_scheme_pref(&self, v: &str) {
        *self.imp().color_scheme.borrow_mut() = v.to_string();
        self.imp().save_settings();
    }

    pub fn window_size(&self) -> (i32, i32) {
        (self.imp().window_width.get(), self.imp().window_height.get())
    }
    pub fn set_window_size(&self, w: i32, h: i32) {
        self.imp().window_width.set(w);
        self.imp().window_height.set(h);
        self.imp().save_settings();
    }

    pub fn window_maximized(&self) -> bool { self.imp().window_maximized.get() }
    pub fn set_window_maximized(&self, v: bool) {
        self.imp().window_maximized.set(v);
        self.imp().save_settings();
    }

    pub fn sidebar_visible(&self) -> bool { self.imp().sidebar_visible.get() }
    pub fn set_sidebar_visible(&self, v: bool) {
        self.imp().sidebar_visible.set(v);
        self.imp().save_settings();
    }

    pub fn last_directory(&self) -> String { self.imp().last_directory.borrow().clone() }
    pub fn set_last_directory(&self, uri: &str) {
        *self.imp().last_directory.borrow_mut() = uri.to_string();
        self.imp().save_settings();
    }

    pub fn tab_states(&self) -> Vec<TabPref> { self.imp().tab_states.borrow().clone() }
    pub fn set_tab_states(&self, tabs: Vec<TabPref>, active_index: i32) {
        *self.imp().tab_states.borrow_mut() = tabs;
        self.imp().last_tab_index.set(active_index);
        self.imp().save_settings();
    }
    pub fn last_tab_index(&self) -> i32 { self.imp().last_tab_index.get() }

    pub fn animations_enabled(&self) -> bool { self.imp().animations_enabled.get() }
    pub fn set_animations_enabled(&self, v: bool) {
        self.imp().animations_enabled.set(v);
        // Apply to the running display immediately — the change is
        // visible on the very next animation (sidebar toggle, popover, …)
        // without restarting the app.
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::Settings::for_display(&display).set_gtk_enable_animations(v);
        }
        self.imp().save_settings();
    }

    pub fn debug_logging(&self) -> bool { self.imp().debug_logging.get() }
    pub fn set_debug_logging(&self, v: bool) {
        self.imp().debug_logging.set(v);
        crate::logging::set_enabled(v);
        self.imp().save_settings();
    }

    pub fn recent_uris(&self) -> Vec<String> {
        self.imp().recent_uris.borrow().clone()
    }

    pub fn recents_enabled(&self) -> bool { self.imp().recents_enabled.get() }
    pub fn set_recents_enabled(&self, v: bool) {
        self.imp().recents_enabled.set(v);
        self.imp().save_settings();
    }

    pub fn recents_cap(&self) -> usize { self.imp().recents_cap.get() }
    /// Set the maximum number of recents kept. Truncates the existing list
    /// if it exceeds the new cap. Returns true if the list was shortened
    /// (caller may want to refresh the sidebar).
    pub fn set_recents_cap(&self, v: usize) -> bool {
        let v = v.clamp(RECENTS_MIN, RECENTS_MAX);
        self.imp().recents_cap.set(v);
        let mut list = self.imp().recent_uris.borrow_mut();
        let shortened = list.len() > v;
        list.truncate(v);
        drop(list);
        self.imp().save_settings();
        shortened
    }

    pub fn thumbnail_cache_size(&self) -> usize {
        self.imp().thumbnail_cache_size.get()
    }
    /// Set the thumbnail texture cache size (entries). Clamps to the
    /// configured bounds, persists, and trims the live cache so the
    /// new limit is honoured immediately.
    pub fn set_thumbnail_cache_size(&self, v: usize) {
        let v = v.clamp(THUMB_CACHE_MIN, THUMB_CACHE_MAX);
        self.imp().thumbnail_cache_size.set(v);
        crate::file_view::cell::set_thumbnail_cache_cap(v);
        self.imp().save_settings();
    }

    pub fn show_duplicate(&self) -> bool { self.imp().show_duplicate.get() }
    pub fn set_show_duplicate(&self, v: bool) {
        self.imp().show_duplicate.set(v);
        self.imp().save_settings();
    }

    pub fn show_create_link(&self) -> bool { self.imp().show_create_link.get() }
    pub fn set_show_create_link(&self, v: bool) {
        self.imp().show_create_link.set(v);
        self.imp().save_settings();
    }

    pub fn show_add_bookmark(&self) -> bool { self.imp().show_add_bookmark.get() }
    pub fn set_show_add_bookmark(&self, v: bool) {
        self.imp().show_add_bookmark.set(v);
        self.imp().save_settings();
    }

    pub fn show_copy_location(&self) -> bool { self.imp().show_copy_location.get() }
    pub fn set_show_copy_location(&self, v: bool) {
        self.imp().show_copy_location.set(v);
        self.imp().save_settings();
    }

    pub fn bookmarks_enabled(&self) -> bool { self.imp().bookmarks_enabled.get() }
    pub fn set_bookmarks_enabled(&self, v: bool) {
        self.imp().bookmarks_enabled.set(v);
        self.imp().save_settings();
    }

    pub fn folders_first(&self) -> bool { self.imp().folders_first.get() }
    pub fn set_folders_first(&self, v: bool) {
        self.imp().folders_first.set(v);
        crate::model::directory_model::set_folders_first(v);
        self.imp().save_settings();
    }

    pub fn show_col_type(&self) -> bool { self.imp().show_col_type.get() }
    pub fn set_show_col_type(&self, v: bool) {
        self.imp().show_col_type.set(v);
        self.imp().save_settings();
    }

    pub fn show_col_size(&self) -> bool { self.imp().show_col_size.get() }
    pub fn set_show_col_size(&self, v: bool) {
        self.imp().show_col_size.set(v);
        self.imp().save_settings();
    }

    pub fn show_col_modified(&self) -> bool { self.imp().show_col_modified.get() }
    pub fn set_show_col_modified(&self, v: bool) {
        self.imp().show_col_modified.set(v);
        self.imp().save_settings();
    }

    pub fn show_col_permissions(&self) -> bool { self.imp().show_col_permissions.get() }
    pub fn set_show_col_permissions(&self, v: bool) {
        self.imp().show_col_permissions.set(v);
        self.imp().save_settings();
    }

    pub fn show_col_owner(&self) -> bool { self.imp().show_col_owner.get() }
    pub fn set_show_col_owner(&self, v: bool) {
        self.imp().show_col_owner.set(v);
        self.imp().save_settings();
    }

    pub fn show_col_group(&self) -> bool { self.imp().show_col_group.get() }
    pub fn set_show_col_group(&self, v: bool) {
        self.imp().show_col_group.set(v);
        self.imp().save_settings();
    }

    pub fn show_col_accessed(&self) -> bool { self.imp().show_col_accessed.get() }
    pub fn set_show_col_accessed(&self, v: bool) {
        self.imp().show_col_accessed.set(v);
        self.imp().save_settings();
    }

    /// Push `uri` to the front of the recents list (MRU), deduplicating any
    /// prior occurrence and capping at `RECENTS_MAX`. Returns true when the
    /// list changed (caller may want to refresh the sidebar).
    pub fn push_recent_uri(&self, uri: &str) -> bool {
        if !self.imp().recents_enabled.get() {
            return false;
        }
        if uri.is_empty() {
            return false;
        }
        let mut list = self.imp().recent_uris.borrow_mut();
        if list.first().map_or(false, |u| u == uri) {
            return false;
        }
        list.retain(|u| u != uri);
        list.insert(0, uri.to_string());
        list.truncate(self.imp().recents_cap.get());
        drop(list);
        self.imp().save_settings();
        true
    }
}
