use std::cell::{Cell, RefCell};

use adw::subclass::prelude::*;
use gtk4::prelude::*;

use crate::window::WrenWindow;

#[derive(Debug)]
pub struct WrenApplication {
    pub terminal_cmd: RefCell<String>,
    pub show_hidden: Cell<bool>,
    pub zoom_level: Cell<i32>,
    pub view_mode: RefCell<String>,
    pub sort_key: RefCell<String>,
    pub sort_reversed: Cell<bool>,
    pub window_width: Cell<i32>,
    pub window_height: Cell<i32>,
}

impl Default for WrenApplication {
    fn default() -> Self {
        Self {
            terminal_cmd: RefCell::new(String::new()),
            show_hidden: Cell::new(false),
            zoom_level: Cell::new(3),
            view_mode: RefCell::new("grid".to_string()),
            sort_key: RefCell::new("name".to_string()),
            sort_reversed: Cell::new(false),
            window_width: Cell::new(1000),
            window_height: Cell::new(700),
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
        kf.set_integer("View", "zoom_level", self.zoom_level.get());
        kf.set_string("View", "view_mode", &self.view_mode.borrow());
        kf.set_string("Sort", "key", &self.sort_key.borrow());
        kf.set_boolean("Sort", "reversed", self.sort_reversed.get());
        kf.set_integer("Window", "width", self.window_width.get());
        kf.set_integer("Window", "height", self.window_height.get());
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

    fn startup(&self) {
        self.parent_startup();
        self.load_settings();
        let app = self.obj();

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
