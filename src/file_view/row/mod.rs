mod imp;

use adw::subclass::prelude::*;
use glib::Object;

use crate::model::FileObject;

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

impl WrenFileRow {
    pub fn new() -> Self {
        Object::builder().build()
    }

    fn imp(&self) -> &imp::WrenFileRow {
        imp::WrenFileRow::from_obj(self)
    }

    pub fn bind(&self, file_obj: &FileObject) {
        let imp = self.imp();
        imp.name.set_label(&file_obj.name());

        if file_obj.is_directory() {
            imp.content_type.set_label("Folder");
            imp.size.set_label("—");
        } else {
            imp.content_type.set_label(&file_obj.content_type());
            imp.size.set_label(&format_size(file_obj.file_size()));
        }

        let ts = file_obj.modified();
        if ts > 0 {
            if let Ok(dt) = glib::DateTime::from_unix_local(ts) {
                imp.modified.set_label(&dt.format("%e %b %Y").unwrap_or_default());
            }
        }

        if let Some(icon) = file_obj.icon() {
            imp.icon.set_from_gicon(&icon);
        } else if file_obj.is_directory() {
            imp.icon.set_icon_name(Some("folder-symbolic"));
        } else {
            imp.icon.set_icon_name(Some("text-x-generic-symbolic"));
        }
    }

    pub fn unbind(&self) {
        let imp = self.imp();
        imp.name.set_label("");
        imp.content_type.set_label("");
        imp.size.set_label("");
        imp.modified.set_label("");
        imp.icon.clear();
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
