use std::cell::RefCell;

use adw::subclass::prelude::*;
use gtk4::prelude::*;

use crate::window::WrenWindow;

#[derive(Debug, Default)]
pub struct WrenApplication {
    pub terminal_cmd: RefCell<String>,
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
        if kf
            .load_from_file(&path, glib::KeyFileFlags::NONE)
            .is_ok()
        {
            if let Ok(cmd) = kf.string("General", "terminal") {
                *self.terminal_cmd.borrow_mut() = cmd.to_string();
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
    }
}

impl GtkApplicationImpl for WrenApplication {}
impl AdwApplicationImpl for WrenApplication {}
