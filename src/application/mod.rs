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

/// Thumbnail policy values. u8 instead of an enum so `Cell<u8>` works
/// with `Copy` semantics matching the existing prefs.
pub const THUMB_POLICY_ALWAYS: u8 = 0;
pub const THUMB_POLICY_LOCAL: u8 = 1;
pub const THUMB_POLICY_NEVER: u8 = 2;

pub fn thumbnail_policy_to_str(v: u8) -> &'static str {
    match v {
        THUMB_POLICY_LOCAL => "local",
        THUMB_POLICY_NEVER => "never",
        _ => "always",
    }
}

pub fn thumbnail_policy_from_str(s: &str) -> u8 {
    match s {
        "local" => THUMB_POLICY_LOCAL,
        "never" => THUMB_POLICY_NEVER,
        _ => THUMB_POLICY_ALWAYS,
    }
}

/// Maximum number of typed paths remembered for the Ctrl+L Up/Down history.
pub const PATH_HISTORY_MAX: usize = 50;

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

    pub fn single_click(&self) -> bool { self.imp().single_click.get() }
    pub fn set_single_click(&self, v: bool) {
        self.imp().single_click.set(v);
        self.imp().save_settings();
    }

    pub fn settings_flat(&self) -> bool { self.imp().settings_flat.get() }
    pub fn set_settings_flat(&self, v: bool) {
        self.imp().settings_flat.set(v);
        self.imp().save_settings();
    }

    pub fn notifications_enabled(&self) -> bool { self.imp().notifications_enabled.get() }
    pub fn set_notifications_enabled(&self, v: bool) {
        self.imp().notifications_enabled.set(v);
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

    pub fn thumbnail_policy(&self) -> u8 {
        self.imp().thumbnail_policy.get()
    }
    /// Set the thumbnail policy (always / local / never). Pushes the
    /// new value to the cell module's thread-local gate, then persists.
    /// Caller is responsible for refreshing visible tabs.
    pub fn set_thumbnail_policy(&self, v: u8) {
        let v = match v {
            THUMB_POLICY_LOCAL | THUMB_POLICY_NEVER => v,
            _ => THUMB_POLICY_ALWAYS,
        };
        self.imp().thumbnail_policy.set(v);
        crate::file_view::cell::set_thumbnail_policy(v);
        self.imp().save_settings();
    }

    pub fn folder_count_policy(&self) -> String {
        self.imp().folder_count_policy.borrow().clone()
    }
    /// Set the folder item-count policy. Accepts "always" | "local" | "never".
    /// Pushes the new value to the row module's thread-local so in-flight
    /// enumerations can observe the change at their next checkpoint.
    pub fn set_folder_count_policy(&self, v: &str) {
        let v = match v {
            "local" | "never" => v,
            _ => "always",
        };
        *self.imp().folder_count_policy.borrow_mut() = v.to_string();
        crate::file_view::row::set_folder_count_policy(v);
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

    pub fn confirm_move_to_trash(&self) -> bool { self.imp().confirm_move_to_trash.get() }
    pub fn set_confirm_move_to_trash(&self, v: bool) {
        self.imp().confirm_move_to_trash.set(v);
        self.imp().save_settings();
    }

    pub fn show_full_path_in_title(&self) -> bool { self.imp().show_full_path_in_title.get() }
    pub fn set_show_full_path_in_title(&self, v: bool) {
        self.imp().show_full_path_in_title.set(v);
        self.imp().save_settings();
    }

    pub fn new_tab_use_defaults(&self) -> bool { self.imp().new_tab_use_defaults.get() }
    pub fn set_new_tab_use_defaults(&self, v: bool) {
        self.imp().new_tab_use_defaults.set(v);
        self.imp().save_settings();
    }

    pub fn new_tab_default_path(&self) -> String { self.imp().new_tab_default_path.borrow().clone() }
    pub fn set_new_tab_default_path(&self, v: &str) {
        *self.imp().new_tab_default_path.borrow_mut() = v.to_string();
        self.imp().save_settings();
    }

    pub fn new_tab_default_view(&self) -> String { self.imp().new_tab_default_view.borrow().clone() }
    pub fn set_new_tab_default_view(&self, v: &str) {
        *self.imp().new_tab_default_view.borrow_mut() = v.to_string();
        self.imp().save_settings();
    }

    pub fn new_tab_default_sort_key(&self) -> String { self.imp().new_tab_default_sort_key.borrow().clone() }
    pub fn set_new_tab_default_sort_key(&self, v: &str) {
        *self.imp().new_tab_default_sort_key.borrow_mut() = v.to_string();
        self.imp().save_settings();
    }

    pub fn new_tab_default_sort_reversed(&self) -> bool { self.imp().new_tab_default_sort_reversed.get() }
    pub fn set_new_tab_default_sort_reversed(&self, v: bool) {
        self.imp().new_tab_default_sort_reversed.set(v);
        self.imp().save_settings();
    }

    pub fn new_tab_default_zoom(&self) -> i32 { self.imp().new_tab_default_zoom.get() }
    pub fn set_new_tab_default_zoom(&self, v: i32) {
        self.imp().new_tab_default_zoom.set(v.clamp(1, 5));
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

    /// Returns the current executable-text-file action: `"run"`, `"view"`, or `"ask"`.
    pub fn executable_text_action(&self) -> String {
        self.imp().executable_text_action.borrow().clone()
    }
    pub fn set_executable_text_action(&self, v: &str) {
        let normalized = match v {
            "run" | "view" | "ask" => v,
            _ => "ask",
        };
        *self.imp().executable_text_action.borrow_mut() = normalized.to_string();
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

    pub fn path_history(&self) -> Vec<String> {
        self.imp().path_history.borrow().clone()
    }

    /// Push `text` to the front of the path-bar history (MRU), deduplicating
    /// any prior occurrence and capping at `PATH_HISTORY_MAX`. Persists.
    pub fn push_path_history(&self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        let mut list = self.imp().path_history.borrow_mut();
        if list.first().map_or(false, |s| s == trimmed) {
            return;
        }
        list.retain(|s| s != trimmed);
        list.insert(0, trimmed.to_string());
        list.truncate(PATH_HISTORY_MAX);
        drop(list);
        self.imp().save_settings();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    // GLib caches user_config_dir() on first call, so XDG_CONFIG_HOME
    // must be set BEFORE any glib calls and can never be changed for
    // the lifetime of the process. We point it at a single OnceLock
    // tempdir and clean settings.ini between tests to isolate state.
    static GTK_INIT: OnceLock<()> = OnceLock::new();
    static CONFIG_HOME: OnceLock<tempfile::TempDir> = OnceLock::new();

    fn ensure_test_config() -> &'static std::path::Path {
        let dir = CONFIG_HOME.get_or_init(|| {
            let dir = tempfile::tempdir().expect("tempdir");
            // SAFETY: set before any other thread or glib reads it.
            unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
            dir
        });
        dir.path()
    }

    fn ensure_gtk() {
        // Touch the config-home env var first so glib's first read
        // captures our tempdir.
        let _ = ensure_test_config();
        GTK_INIT.get_or_init(|| {
            unsafe { gtk4::ffi::gtk_init() };
        });
    }

    fn settings_ini_path() -> std::path::PathBuf {
        let mut p = ensure_test_config().to_path_buf();
        p.push("wren");
        p.push("settings.ini");
        p
    }

    fn fresh_app(suffix: &str) -> WrenApplication {
        // Wipe settings.ini so the new WrenApplication starts from defaults.
        let p = settings_ini_path();
        let _ = std::fs::remove_file(&p);
        WrenApplication::new(&format!("io.github.wren.test.{suffix}"))
    }

    #[test]
    #[serial_test::serial]
    fn push_recent_empty_string_no_op() {
        ensure_gtk();
        let app = fresh_app("empty");
        assert!(!app.push_recent_uri(""));
        assert!(app.recent_uris().is_empty());
    }

    #[test]
    #[serial_test::serial]
    fn push_recent_first_insert() {
        ensure_gtk();
        let app = fresh_app("first");
        assert!(app.push_recent_uri("file:///tmp/a"));
        assert_eq!(app.recent_uris(), vec!["file:///tmp/a"]);
    }

    #[test]
    #[serial_test::serial]
    fn push_recent_duplicate_at_head_returns_false() {
        ensure_gtk();
        let app = fresh_app("dup_head");
        assert!(app.push_recent_uri("file:///x"));
        // Pushing the same uri that's already at index 0 is a no-op.
        assert!(!app.push_recent_uri("file:///x"));
        assert_eq!(app.recent_uris(), vec!["file:///x"]);
    }

    #[test]
    #[serial_test::serial]
    fn push_recent_duplicate_elsewhere_moves_to_head() {
        ensure_gtk();
        let app = fresh_app("dup_mid");
        app.push_recent_uri("file:///a");
        app.push_recent_uri("file:///b");
        app.push_recent_uri("file:///c");
        // Re-pushing 'a' should move it back to head, dedup'ing the prior copy.
        assert!(app.push_recent_uri("file:///a"));
        assert_eq!(
            app.recent_uris(),
            vec![
                "file:///a".to_string(),
                "file:///c".to_string(),
                "file:///b".to_string()
            ]
        );
    }

    #[test]
    #[serial_test::serial]
    fn push_recent_caps_at_recents_max() {
        ensure_gtk();
        let app = fresh_app("cap");
        // Push RECENTS_MAX + 1 distinct uris.
        for i in 0..=RECENTS_MAX {
            assert!(app.push_recent_uri(&format!("file:///dir-{i}")));
        }
        let list = app.recent_uris();
        assert_eq!(list.len(), RECENTS_MAX);
        // Most recently pushed is at the head.
        assert_eq!(list[0], format!("file:///dir-{}", RECENTS_MAX));
        // Oldest entry (file:///dir-0) was dropped off the tail.
        assert!(!list.contains(&"file:///dir-0".to_string()));
    }

    #[test]
    #[serial_test::serial]
    fn settings_round_trip() {
        ensure_gtk();
        let app = fresh_app("rt");
        app.set_terminal_cmd("alacritty");
        app.set_show_hidden(true);
        app.set_show_extensions(false);
        app.set_zoom_level(5);
        app.set_view_mode_pref("list");
        app.set_sort_key_pref("size");
        app.set_sort_reversed_pref(true);
        app.set_window_size(1234, 567);
        app.set_window_maximized(true);
        app.set_sidebar_visible(false);
        app.set_last_directory("file:///home/foo");
        app.set_color_scheme_pref("dark");
        app.set_last_tabs(
            vec!["file:///a".to_string(), "file:///b".to_string()],
            1,
        );
        app.push_recent_uri("file:///recent-a");
        app.push_recent_uri("file:///recent-b");

        // Construct a second app instance and have it re-read settings.ini.
        let app2 = WrenApplication::new("io.github.wren.test.rt2");
        adw::subclass::prelude::ObjectSubclassIsExt::imp(&app2).reload_for_test();

        assert_eq!(app2.terminal_cmd(), "alacritty");
        assert!(app2.show_hidden());
        assert!(!app2.show_extensions());
        assert_eq!(app2.zoom_level(), 5);
        assert_eq!(app2.view_mode(), "list");
        assert_eq!(app2.sort_key(), "size");
        assert!(app2.sort_reversed());
        assert_eq!(app2.window_size(), (1234, 567));
        assert!(app2.window_maximized());
        assert!(!app2.sidebar_visible());
        assert_eq!(app2.last_directory(), "file:///home/foo");
        assert_eq!(app2.color_scheme(), "dark");
        assert_eq!(
            app2.last_tabs(),
            vec!["file:///a".to_string(), "file:///b".to_string()]
        );
        assert_eq!(app2.last_tab_index(), 1);
        assert_eq!(
            app2.recent_uris(),
            vec!["file:///recent-b".to_string(), "file:///recent-a".to_string()]
        );
    }

    #[test]
    #[serial_test::serial]
    fn settings_zoom_clamped_on_load() {
        // Out-of-range zoom values in settings.ini are clamped back
        // into [1, 5] when load_settings reads them.
        ensure_gtk();
        let app = fresh_app("clamp");
        app.set_zoom_level(5); // arbitrary valid value to populate file
        let p = settings_ini_path();
        let raw = std::fs::read_to_string(&p).unwrap();
        let raw = raw.replace("zoom_level=5", "zoom_level=12");
        std::fs::write(&p, raw).unwrap();

        let app2 = WrenApplication::new("io.github.wren.test.clamp2");
        adw::subclass::prelude::ObjectSubclassIsExt::imp(&app2).reload_for_test();
        assert_eq!(app2.zoom_level(), 5); // clamped from 12 → 5
    }

    #[test]
    #[serial_test::serial]
    fn settings_invalid_view_mode_keeps_default() {
        // Unknown view_mode strings are rejected in load_settings —
        // only "grid" and "list" are accepted; defaults to "grid".
        ensure_gtk();
        let app = fresh_app("badview");
        app.set_view_mode_pref("nonsense");
        // The setter accepts any string; on reload, the validator drops it
        // back to the in-memory default ("grid").
        let app2 = WrenApplication::new("io.github.wren.test.badview2");
        adw::subclass::prelude::ObjectSubclassIsExt::imp(&app2).reload_for_test();
        assert_eq!(app2.view_mode(), "grid");
    }

    #[test]
    #[serial_test::serial]
    fn settings_invalid_color_scheme_keeps_default() {
        // Same defensive parse for color_scheme: only default/light/dark allowed.
        ensure_gtk();
        let app = fresh_app("badscheme");
        app.set_color_scheme_pref("midnight");
        let app2 = WrenApplication::new("io.github.wren.test.badscheme2");
        adw::subclass::prelude::ObjectSubclassIsExt::imp(&app2).reload_for_test();
        assert_eq!(app2.color_scheme(), "default");
    }

    #[test]
    #[serial_test::serial]
    fn settings_recent_uris_truncated_on_load() {
        // A settings.ini with > RECENTS_MAX entries is truncated by load_settings.
        ensure_gtk();
        let app = fresh_app("trunc");
        app.set_zoom_level(3); // ensure save_settings runs and creates the file
        let p = settings_ini_path();

        // Build a tab-joined list of 20 entries — twice the cap.
        let many: Vec<String> = (0..20).map(|i| format!("file:///many-{i}")).collect();
        let joined = many.join("\t");
        let raw = std::fs::read_to_string(&p).unwrap();
        let mut new_lines: Vec<String> = raw.lines().map(|l| l.to_string()).collect();
        // Replace the [Recents] section's uris= line, or append it.
        let mut found = false;
        for line in new_lines.iter_mut() {
            if line.starts_with("uris=") {
                *line = format!("uris={joined}");
                found = true;
                break;
            }
        }
        if !found {
            new_lines.push("[Recents]".to_string());
            new_lines.push(format!("uris={joined}"));
        }
        std::fs::write(&p, new_lines.join("\n") + "\n").unwrap();

        let app2 = WrenApplication::new("io.github.wren.test.trunc2");
        adw::subclass::prelude::ObjectSubclassIsExt::imp(&app2).reload_for_test();
        assert_eq!(app2.recent_uris().len(), RECENTS_MAX);
        // The first RECENTS_MAX entries are the ones kept (load truncates the tail).
        assert_eq!(app2.recent_uris()[0], "file:///many-0");
    }
}
