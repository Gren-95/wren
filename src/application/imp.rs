use std::cell::{Cell, RefCell};

use adw::subclass::prelude::*;
use gtk4::prelude::*;

use crate::window::WrenWindow;

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
    pub last_tabs: RefCell<Vec<String>>,
    pub last_tab_index: Cell<i32>,
    pub color_scheme: RefCell<String>,
    pub animations_enabled: Cell<bool>,
    pub debug_logging: Cell<bool>,
    pub recent_uris: RefCell<Vec<String>>,
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
            last_tabs: RefCell::new(Vec::new()),
            last_tab_index: Cell::new(0),
            color_scheme: RefCell::new("default".to_string()),
            animations_enabled: Cell::new(true),
            debug_logging: Cell::new(false),
            recent_uris: RefCell::new(Vec::new()),
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
            // Stored as a single \t-joined string. \t can't appear in a
            // URI (RFC 3986 reserves only printable chars), so it's safe
            // as a separator without escaping. Splitting an empty string
            // would yield [""], hence the explicit empty check.
            if let Ok(joined) = kf.string("General", "last_tabs") {
                let s = joined.to_string();
                *self.last_tabs.borrow_mut() = if s.is_empty() {
                    Vec::new()
                } else {
                    s.split('\t').map(|s| s.to_string()).collect()
                };
            }
            if let Ok(v) = kf.integer("General", "last_tab_index") {
                self.last_tab_index.set(v.max(0));
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
            // Same \t-joined storage rationale as last_tabs above.
            if let Ok(joined) = kf.string("Recents", "uris") {
                let s = joined.to_string();
                let mut uris: Vec<String> = if s.is_empty() {
                    Vec::new()
                } else {
                    s.split('\t').map(|s| s.to_string()).collect()
                };
                uris.truncate(super::RECENTS_MAX);
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
        kf.set_string(
            "General",
            "last_tabs",
            &self.last_tabs.borrow().join("\t"),
        );
        kf.set_integer("General", "last_tab_index", self.last_tab_index.get());
        kf.set_string("Appearance", "color_scheme", &self.color_scheme.borrow());
        kf.set_boolean("Appearance", "animations", self.animations_enabled.get());
        kf.set_boolean("General", "debug_logging", self.debug_logging.get());
        kf.set_string("Recents", "uris", &self.recent_uris.borrow().join("\t"));
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
