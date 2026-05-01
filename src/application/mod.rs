mod imp;

use adw::subclass::prelude::ObjectSubclassIsExt;
use glib::Object;

glib::wrapper! {
    pub struct WrenApplication(ObjectSubclass<imp::WrenApplication>)
        @extends adw::Application, gtk4::Application, gio::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl WrenApplication {
    pub fn new(app_id: &str) -> Self {
        Object::builder()
            .property("application-id", app_id)
            .property("flags", gio::ApplicationFlags::empty())
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
}
