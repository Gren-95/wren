use std::cell::{Cell, RefCell};

use adw::subclass::prelude::*;
use gtk4::prelude::*;

use crate::window::WrenWindow;

use super::TabPref;

#[derive(Debug)]
pub struct WrenApplication {
    pub terminal_cmd: RefCell<String>,
    pub show_hidden: Cell<bool>,
    pub show_extensions: Cell<bool>,
    pub zoom_level: Cell<i32>,
    pub view_mode: RefCell<String>,
    pub sort_key: RefCell<String>,
    pub sort_reversed: Cell<bool>,
    pub window_width: Cell<i32>,
    pub window_height: Cell<i32>,
    pub window_maximized: Cell<bool>,
    pub sidebar_visible: Cell<bool>,
    pub last_directory: RefCell<String>,
    pub tab_states: RefCell<Vec<TabPref>>,
    pub last_tab_index: Cell<i32>,
    pub color_scheme: RefCell<String>,
    pub animations_enabled: Cell<bool>,
    pub debug_logging: Cell<bool>,
    pub recent_uris: RefCell<Vec<String>>,
    pub recents_enabled: Cell<bool>,
    pub recents_cap: Cell<usize>,
    pub thumbnail_cache_size: Cell<usize>,
    pub thumbnail_policy: Cell<u8>,
    pub folder_count_policy: RefCell<String>,
    pub show_duplicate: Cell<bool>,
    pub show_create_link: Cell<bool>,
    pub show_add_bookmark: Cell<bool>,
    pub show_copy_location: Cell<bool>,
    pub bookmarks_enabled: Cell<bool>,
    pub folders_first: Cell<bool>,
    pub confirm_move_to_trash: Cell<bool>,
    pub show_full_path_in_title: Cell<bool>,
    pub new_tab_use_defaults: Cell<bool>,
    pub new_tab_default_path: RefCell<String>,
    pub new_tab_default_view: RefCell<String>,
    pub new_tab_default_sort_key: RefCell<String>,
    pub new_tab_default_sort_reversed: Cell<bool>,
    pub new_tab_default_zoom: Cell<i32>,
    pub show_col_type: Cell<bool>,
    pub show_col_size: Cell<bool>,
    pub show_col_modified: Cell<bool>,
    pub show_col_permissions: Cell<bool>,
    pub show_col_owner: Cell<bool>,
    pub show_col_group: Cell<bool>,
    pub show_col_accessed: Cell<bool>,
    /// Action to take when activating an executable text file.
    /// Stored as one of `"run"`, `"view"`, `"ask"`. Defaults to `"ask"`.
    pub executable_text_action: RefCell<String>,
}

impl Default for WrenApplication {
    fn default() -> Self {
        Self {
            terminal_cmd: RefCell::new(String::new()),
            show_hidden: Cell::new(false),
            show_extensions: Cell::new(true),
            zoom_level: Cell::new(3),
            view_mode: RefCell::new("grid".to_string()),
            sort_key: RefCell::new("name".to_string()),
            sort_reversed: Cell::new(false),
            window_width: Cell::new(1000),
            window_height: Cell::new(700),
            window_maximized: Cell::new(false),
            sidebar_visible: Cell::new(true),
            last_directory: RefCell::new(String::new()),
            tab_states: RefCell::new(Vec::new()),
            last_tab_index: Cell::new(0),
            color_scheme: RefCell::new("default".to_string()),
            animations_enabled: Cell::new(true),
            debug_logging: Cell::new(false),
            recent_uris: RefCell::new(Vec::new()),
            recents_enabled: Cell::new(false),
            recents_cap: Cell::new(super::RECENTS_DEFAULT),
            thumbnail_cache_size: Cell::new(super::THUMB_CACHE_DEFAULT),
            thumbnail_policy: Cell::new(super::THUMB_POLICY_ALWAYS),
            folder_count_policy: RefCell::new("always".to_string()),
            show_duplicate: Cell::new(true),
            show_create_link: Cell::new(true),
            show_add_bookmark: Cell::new(true),
            show_copy_location: Cell::new(true),
            bookmarks_enabled: Cell::new(true),
            folders_first: Cell::new(true),
            confirm_move_to_trash: Cell::new(true),
            show_full_path_in_title: Cell::new(false),
            new_tab_use_defaults: Cell::new(false),
            new_tab_default_path: RefCell::new(String::new()),
            new_tab_default_view: RefCell::new("grid".to_string()),
            new_tab_default_sort_key: RefCell::new("name".to_string()),
            new_tab_default_sort_reversed: Cell::new(false),
            new_tab_default_zoom: Cell::new(3),
            show_col_type: Cell::new(true),
            show_col_size: Cell::new(true),
            show_col_modified: Cell::new(true),
            show_col_permissions: Cell::new(false),
            show_col_owner: Cell::new(false),
            show_col_group: Cell::new(false),
            show_col_accessed: Cell::new(false),
            executable_text_action: RefCell::new("ask".to_string()),
        }
    }
}

impl WrenApplication {
    fn settings_path() -> std::path::PathBuf {
        let mut path = glib::user_config_dir();
        path.push("wren");
        path.push("settings.ini");
        path
    }

    fn load_settings(&self) {
        let path = Self::settings_path();
        let kf = glib::KeyFile::new();
        if kf.load_from_file(&path, glib::KeyFileFlags::NONE).is_ok() {
            if let Ok(v) = kf.string("General", "terminal") {
                *self.terminal_cmd.borrow_mut() = v.to_string();
            }
            if let Ok(v) = kf.boolean("View", "show_hidden") {
                self.show_hidden.set(v);
            }
            if let Ok(v) = kf.boolean("View", "show_extensions") {
                self.show_extensions.set(v);
            }
            if let Ok(v) = kf.integer("View", "zoom_level") {
                self.zoom_level.set(v.clamp(1, 5));
            }
            if let Ok(v) = kf.string("View", "view_mode") {
                let s = v.to_string();
                if s == "list" || s == "grid" {
                    *self.view_mode.borrow_mut() = s;
                }
            }
            if let Ok(v) = kf.string("Sort", "key") {
                *self.sort_key.borrow_mut() = v.to_string();
            }
            if let Ok(v) = kf.boolean("Sort", "reversed") {
                self.sort_reversed.set(v);
            }
            if let Ok(v) = kf.integer("Window", "width") {
                if v > 0 { self.window_width.set(v); }
            }
            if let Ok(v) = kf.integer("Window", "height") {
                if v > 0 { self.window_height.set(v); }
            }
            if let Ok(v) = kf.boolean("Window", "maximized") {
                self.window_maximized.set(v);
            }
            if let Ok(v) = kf.boolean("Window", "sidebar_visible") {
                self.sidebar_visible.set(v);
            }
            if let Ok(v) = kf.string("General", "last_directory") {
                *self.last_directory.borrow_mut() = v.to_string();
            }
            // Per-tab state lives under [Tabs] as `tabN=uri|sort_key|asc|grid`.
            // If [Tabs] is present, it wins. Otherwise we migrate the legacy
            // [General] last_tabs (a \t-joined URI list) and synthesise per-tab
            // entries using the global [Sort]/[View] defaults loaded above.
            // The legacy keys are left in the file untouched on save (KeyFile
            // preserves untouched groups); they just stop being read once the
            // new [Tabs] block exists.
            if let Ok(v) = kf.integer("General", "last_tab_index") {
                self.last_tab_index.set(v.max(0));
            }
            if kf.has_group("Tabs") {
                let count = kf.integer("Tabs", "count").unwrap_or(0).max(0) as usize;
                let mut tabs = Vec::with_capacity(count);
                for i in 0..count {
                    let key = format!("tab{i}");
                    let Ok(line) = kf.string("Tabs", &key) else { continue };
                    if let Some(pref) = TabPref::parse_line(line.as_str()) {
                        tabs.push(pref);
                    }
                }
                *self.tab_states.borrow_mut() = tabs;
                if let Ok(v) = kf.integer("Tabs", "active") {
                    self.last_tab_index.set(v.max(0));
                }
            } else if let Ok(joined) = kf.string("General", "last_tabs") {
                let s = joined.to_string();
                let uris: Vec<String> = if s.is_empty() {
                    Vec::new()
                } else {
                    s.split('\t').map(|s| s.to_string()).collect()
                };
                let view = self.view_mode.borrow().clone();
                let key = self.sort_key.borrow().clone();
                let reversed = self.sort_reversed.get();
                *self.tab_states.borrow_mut() = uris
                    .into_iter()
                    .map(|uri| TabPref {
                        uri,
                        sort_key: key.clone(),
                        reversed,
                        view_mode: view.clone(),
                    })
                    .collect();
            }
            if let Ok(v) = kf.boolean("Appearance", "animations") {
                self.animations_enabled.set(v);
            }
            if let Ok(v) = kf.boolean("General", "debug_logging") {
                self.debug_logging.set(v);
            }
            if let Ok(v) = kf.string("Appearance", "color_scheme") {
                let s = v.to_string();
                if matches!(s.as_str(), "default" | "light" | "dark") {
                    *self.color_scheme.borrow_mut() = s;
                }
            }
            if let Ok(v) = kf.boolean("Recents", "enabled") {
                self.recents_enabled.set(v);
            }
            if let Ok(v) = kf.integer("Recents", "max") {
                let clamped = (v as usize).clamp(super::RECENTS_MIN, super::RECENTS_MAX);
                self.recents_cap.set(clamped);
            }
            if let Ok(v) = kf.integer("Performance", "thumbnail_cache_size") {
                let clamped = (v as usize).clamp(super::THUMB_CACHE_MIN, super::THUMB_CACHE_MAX);
                self.thumbnail_cache_size.set(clamped);
            }
            if let Ok(v) = kf.string("Performance", "thumbnail_policy") {
                self.thumbnail_policy
                    .set(super::thumbnail_policy_from_str(v.as_str()));
            }
            if let Ok(v) = kf.string("Performance", "folder_count_policy") {
                let s = v.to_string();
                if matches!(s.as_str(), "always" | "local" | "never") {
                    *self.folder_count_policy.borrow_mut() = s;
                }
            }
            if let Ok(v) = kf.boolean("ContextMenu", "show_duplicate") {
                self.show_duplicate.set(v);
            }
            if let Ok(v) = kf.boolean("ContextMenu", "show_create_link") {
                self.show_create_link.set(v);
            }
            if let Ok(v) = kf.boolean("ContextMenu", "show_add_bookmark") {
                self.show_add_bookmark.set(v);
            }
            if let Ok(v) = kf.boolean("ContextMenu", "show_copy_location") {
                self.show_copy_location.set(v);
            }
            if let Ok(v) = kf.boolean("Sidebar", "bookmarks_enabled") {
                self.bookmarks_enabled.set(v);
            }
            if let Ok(v) = kf.boolean("Sort", "folders_first") {
                self.folders_first.set(v);
            }
            if let Ok(v) = kf.boolean("Trash", "confirm_move_to_trash") {
                self.confirm_move_to_trash.set(v);
            }
            if let Ok(v) = kf.boolean("Window", "show_full_path_in_title") {
                self.show_full_path_in_title.set(v);
            }
            if let Ok(v) = kf.boolean("NewTab", "use_defaults") {
                self.new_tab_use_defaults.set(v);
            }
            if let Ok(v) = kf.string("NewTab", "default_path") {
                *self.new_tab_default_path.borrow_mut() = v.to_string();
            }
            if let Ok(v) = kf.string("NewTab", "default_view") {
                let s = v.to_string();
                if s == "list" || s == "grid" {
                    *self.new_tab_default_view.borrow_mut() = s;
                }
            }
            if let Ok(v) = kf.string("NewTab", "default_sort_key") {
                let s = v.to_string();
                if matches!(s.as_str(), "name" | "size" | "date" | "type") {
                    *self.new_tab_default_sort_key.borrow_mut() = s;
                }
            }
            if let Ok(v) = kf.boolean("NewTab", "default_sort_reversed") {
                self.new_tab_default_sort_reversed.set(v);
            }
            if let Ok(v) = kf.integer("NewTab", "default_zoom") {
                self.new_tab_default_zoom.set(v.clamp(1, 5));
            }
            if let Ok(v) = kf.boolean("ListColumns", "type") {
                self.show_col_type.set(v);
            }
            if let Ok(v) = kf.boolean("ListColumns", "size") {
                self.show_col_size.set(v);
            }
            if let Ok(v) = kf.boolean("ListColumns", "modified") {
                self.show_col_modified.set(v);
            }
            if let Ok(v) = kf.boolean("ListColumns", "permissions") {
                self.show_col_permissions.set(v);
            }
            if let Ok(v) = kf.boolean("ListColumns", "owner") {
                self.show_col_owner.set(v);
            }
            if let Ok(v) = kf.boolean("ListColumns", "group") {
                self.show_col_group.set(v);
            }
            if let Ok(v) = kf.boolean("ListColumns", "accessed") {
                self.show_col_accessed.set(v);
            }
            if let Ok(v) = kf.string("Files", "executable_text_action") {
                let s = v.to_string();
                if matches!(s.as_str(), "run" | "view" | "ask") {
                    *self.executable_text_action.borrow_mut() = s;
                }
            }
            // Same \t-joined storage rationale as last_tabs above.
            if let Ok(joined) = kf.string("Recents", "uris") {
                let s = joined.to_string();
                let mut uris: Vec<String> = if s.is_empty() {
                    Vec::new()
                } else {
                    s.split('\t').map(|s| s.to_string()).collect()
                };
                uris.truncate(self.recents_cap.get());
                *self.recent_uris.borrow_mut() = uris;
            }
        }
    }

    pub fn save_settings(&self) {
        let path = Self::settings_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let kf = glib::KeyFile::new();
        let _ = kf.load_from_file(&path, glib::KeyFileFlags::NONE);
        kf.set_string("General", "terminal", &self.terminal_cmd.borrow());
        kf.set_boolean("View", "show_hidden", self.show_hidden.get());
        kf.set_boolean("View", "show_extensions", self.show_extensions.get());
        kf.set_integer("View", "zoom_level", self.zoom_level.get());
        kf.set_string("View", "view_mode", &self.view_mode.borrow());
        kf.set_string("Sort", "key", &self.sort_key.borrow());
        kf.set_boolean("Sort", "reversed", self.sort_reversed.get());
        kf.set_integer("Window", "width", self.window_width.get());
        kf.set_integer("Window", "height", self.window_height.get());
        kf.set_boolean("Window", "maximized", self.window_maximized.get());
        kf.set_boolean("Window", "sidebar_visible", self.sidebar_visible.get());
        kf.set_string("General", "last_directory", &self.last_directory.borrow());
        let tabs = self.tab_states.borrow();
        kf.set_integer("Tabs", "count", tabs.len() as i32);
        for (i, pref) in tabs.iter().enumerate() {
            kf.set_string("Tabs", &format!("tab{i}"), &pref.to_line());
        }
        kf.set_integer("Tabs", "active", self.last_tab_index.get());
        // Mirror to legacy [General] last_tab_index for forward-compat with
        // older builds reading a config we wrote.
        kf.set_integer("General", "last_tab_index", self.last_tab_index.get());
        drop(tabs);
        kf.set_string("Appearance", "color_scheme", &self.color_scheme.borrow());
        kf.set_boolean("Appearance", "animations", self.animations_enabled.get());
        kf.set_boolean("General", "debug_logging", self.debug_logging.get());
        kf.set_boolean("Recents", "enabled", self.recents_enabled.get());
        kf.set_integer("Recents", "max", self.recents_cap.get() as i32);
        kf.set_string("Recents", "uris", &self.recent_uris.borrow().join("\t"));
        kf.set_integer(
            "Performance",
            "thumbnail_cache_size",
            self.thumbnail_cache_size.get() as i32,
        );
        kf.set_string(
            "Performance",
            "thumbnail_policy",
            super::thumbnail_policy_to_str(self.thumbnail_policy.get()),
        );
        kf.set_string(
            "Performance",
            "folder_count_policy",
            &self.folder_count_policy.borrow(),
        );
        kf.set_boolean("ContextMenu", "show_duplicate", self.show_duplicate.get());
        kf.set_boolean("ContextMenu", "show_create_link", self.show_create_link.get());
        kf.set_boolean("ContextMenu", "show_add_bookmark", self.show_add_bookmark.get());
        kf.set_boolean("ContextMenu", "show_copy_location", self.show_copy_location.get());
        kf.set_boolean("Sidebar", "bookmarks_enabled", self.bookmarks_enabled.get());
        kf.set_boolean("Sort", "folders_first", self.folders_first.get());
        kf.set_boolean("Trash", "confirm_move_to_trash", self.confirm_move_to_trash.get());
        kf.set_boolean("Window", "show_full_path_in_title", self.show_full_path_in_title.get());
        kf.set_boolean("NewTab", "use_defaults", self.new_tab_use_defaults.get());
        kf.set_string("NewTab", "default_path", &self.new_tab_default_path.borrow());
        kf.set_string("NewTab", "default_view", &self.new_tab_default_view.borrow());
        kf.set_string("NewTab", "default_sort_key", &self.new_tab_default_sort_key.borrow());
        kf.set_boolean(
            "NewTab",
            "default_sort_reversed",
            self.new_tab_default_sort_reversed.get(),
        );
        kf.set_integer("NewTab", "default_zoom", self.new_tab_default_zoom.get());
        kf.set_boolean("ListColumns", "type", self.show_col_type.get());
        kf.set_boolean("ListColumns", "size", self.show_col_size.get());
        kf.set_boolean("ListColumns", "modified", self.show_col_modified.get());
        kf.set_boolean("ListColumns", "permissions", self.show_col_permissions.get());
        kf.set_boolean("ListColumns", "owner", self.show_col_owner.get());
        kf.set_boolean("ListColumns", "group", self.show_col_group.get());
        kf.set_boolean("ListColumns", "accessed", self.show_col_accessed.get());
        kf.set_string("Files", "executable_text_action", &self.executable_text_action.borrow());
        let data = kf.to_data();
        let _ = std::fs::write(&path, data.as_str());
    }
}

#[glib::object_subclass]
impl ObjectSubclass for WrenApplication {
    const NAME: &'static str = "WrenApplication";
    type Type = super::WrenApplication;
    type ParentType = adw::Application;
}

impl ObjectImpl for WrenApplication {}

impl ApplicationImpl for WrenApplication {
    fn activate(&self) {
        self.parent_activate();
        let app = self.obj();

        let window = if let Some(win) = app.active_window() {
            win
        } else {
            let win = WrenWindow::new(&app);
            win.upcast()
        };

        window.present();
    }

    // Invoked when the binary is launched with file arguments
    // (e.g. `wren .` or `wren /tmp`). HANDLES_OPEN must be set on
    // ApplicationFlags for GIO to route here instead of complaining.
    fn open(&self, files: &[gio::File], _hint: &str) {
        let app = self.obj();
        let window = if let Some(win) = app.active_window().and_downcast::<WrenWindow>() {
            win
        } else {
            WrenWindow::new(&app)
        };
        window.present();

        // Use the first directory argument; ignore the rest. Files
        // (non-directories) are treated as their parent directory so
        // `wren README.md` opens the containing folder.
        if let Some(first) = files.first() {
            let target = match first.query_file_type(gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE) {
                gio::FileType::Directory => first.clone(),
                _ => first.parent().unwrap_or_else(|| first.clone()),
            };
            window.navigate_to(target);
        }
    }

    fn startup(&self) {
        self.parent_startup();
        self.load_settings();
        crate::logging::set_enabled(self.debug_logging.get());
        crate::file_view::cell::set_thumbnail_cache_cap(self.thumbnail_cache_size.get());
        crate::file_view::cell::set_thumbnail_policy(self.thumbnail_policy.get());
        crate::file_view::row::set_folder_count_policy(&self.folder_count_policy.borrow());
        crate::model::directory_model::set_folders_first(self.folders_first.get());
        let app = self.obj();

        let scheme = match self.color_scheme.borrow().as_str() {
            "light" => adw::ColorScheme::ForceLight,
            "dark"  => adw::ColorScheme::ForceDark,
            _       => adw::ColorScheme::Default,
        };
        adw::StyleManager::default().set_color_scheme(scheme);

        // Honour the persisted animations preference on startup. GTK's
        // gtk-enable-animations governs every transition app-wide
        // (sidebar slide, popover fade, banner reveal, …).
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::Settings::for_display(&display)
                .set_gtk_enable_animations(self.animations_enabled.get());
        }

        let provider = gtk4::CssProvider::new();
        provider.load_from_resource("/io/github/wren/style/app.css");
        gtk4::style_context_add_provider_for_display(
            &gtk4::gdk::Display::default().expect("could not connect to display"),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        app.set_accels_for_action("win.navigate-back", &["<Alt>Left"]);
        app.set_accels_for_action("win.navigate-forward", &["<Alt>Right"]);
        app.set_accels_for_action("win.navigate-up", &["<Alt>Up"]);
        app.set_accels_for_action("win.toggle-search", &["<Primary>f"]);
        app.set_accels_for_action("win.new-tab", &["<Primary>t"]);
        app.set_accels_for_action("win.close-tab", &["<Primary>w"]);
        app.set_accels_for_action("win.copy", &["<Primary>c"]);
        app.set_accels_for_action("win.cut", &["<Primary>x"]);
        app.set_accels_for_action("win.paste", &["<Primary>v"]);
        app.set_accels_for_action("win.select-all", &["<Primary>a"]);
        app.set_accels_for_action("win.move-to-trash", &["Delete"]);
        app.set_accels_for_action("win.delete-permanently", &["<Shift>Delete"]);
        app.set_accels_for_action("win.rename", &["F2"]);
        app.set_accels_for_action("win.new-folder", &["<Primary><Shift>n"]);
        app.set_accels_for_action("win.toggle-hidden", &["<Primary>h"]);
        app.set_accels_for_action("win.open-with", &["<Primary><Shift>o"]);
        app.set_accels_for_action("win.copy-path", &["<Primary><Shift>c"]);
        app.set_accels_for_action("win.focus-location", &["<Primary>l"]);
        app.set_accels_for_action("win.open-in-terminal", &["<Primary><Shift>t"]);
        app.set_accels_for_action("win.zoom-in", &["<Primary>equal", "<Primary>plus"]);
        app.set_accels_for_action("win.zoom-out", &["<Primary>minus"]);
        app.set_accels_for_action("win.zoom-reset", &["<Primary>0"]);
        app.set_accels_for_action("win.properties", &["<Alt>Return"]);
        app.set_accels_for_action("win.undo", &["<Primary>z"]);
        app.set_accels_for_action("win.redo", &["<Primary><Shift>z"]);
        app.set_accels_for_action("win.add-bookmark", &["<Primary>d"]);
        app.set_accels_for_action("win.batch-rename", &["<Primary><Shift>r"]);
        app.set_accels_for_action("win.reload", &["F5"]);
        app.set_accels_for_action("win.navigate-home", &["<Alt>Home"]);
        app.set_accels_for_action("win.new-window", &["<Primary>n"]);
        app.set_accels_for_action("win.show-shortcuts", &["<Primary>question"]);
    }
}

impl GtkApplicationImpl for WrenApplication {}
impl AdwApplicationImpl for WrenApplication {}
