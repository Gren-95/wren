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
}
