//! The Properties dialog. Shows file/directory metadata; for directories
//! kicks off an async recursive size walk that updates the size label live.

use adw::prelude::*;

use super::WrenWindow;
use super::file_ops::{compute_dir_size, format_file_size};

impl WrenWindow {
    pub fn show_properties(&self) {
        let objs = self.selected_file_objects();
        let file_obj = objs.first().cloned();
        // Resolve a gio::File for the subject (selected item, or current dir).
        let (target, name, content_type, file_size, path_str, is_directory) =
            if let Some(ref obj) = file_obj {
                let path = obj
                    .file()
                    .path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (
                    obj.file().clone(),
                    obj.name(),
                    obj.content_type(),
                    obj.file_size(),
                    path,
                    obj.is_directory(),
                )
            } else {
                let Some(idx) = self.current_tab_index() else {
                    return;
                };
                let tabs = self.imp().tabs.borrow();
                let Some(tab) = tabs.get(idx) else { return };
                let Some(loc) = tab.navigation.current() else { return };
                let name = loc
                    .basename()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Folder".to_string());
                let path = loc
                    .path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (loc.clone(), name.into(), "inode/directory".into(), 0u64, path, true)
            };

        let dialog = adw::PreferencesDialog::new();
        dialog.set_title("Properties");

        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::new();
        group.set_title(&name);

        let add_row = |group: &adw::PreferencesGroup, title: &str, value: &str| {
            let row = adw::ActionRow::new();
            row.set_title(title);
            let lbl = gtk4::Label::new(Some(value));
            lbl.add_css_class("dim-label");
            lbl.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
            lbl.set_max_width_chars(40);
            row.add_suffix(&lbl);
            group.add(&row);
        };

        let type_str = if content_type.is_empty() {
            "Unknown".to_string()
        } else {
            content_type.to_string()
        };
        add_row(&group, "Type", &type_str);

        if is_directory {
            // Directory: kick off async recursive size calculation.
            let size_row = adw::ActionRow::new();
            size_row.set_title("Size");
            let size_label = gtk4::Label::new(Some("Calculating…"));
            size_label.add_css_class("dim-label");
            size_row.add_suffix(&size_label);
            group.add(&size_row);

            let cancellable = gio::Cancellable::new();
            // Cancel the walk if the dialog is closed before it finishes.
            dialog.connect_closed(glib::clone!(
                #[strong]
                cancellable,
                move |_| cancellable.cancel()
            ));
            glib::spawn_future_local(glib::clone!(
                #[weak]
                size_label,
                #[strong]
                cancellable,
                async move {
                    let (total, count) = compute_dir_size(target, &cancellable, |t, c| {
                        size_label.set_text(&format!(
                            "{} ({} items, calculating…)",
                            format_file_size(t),
                            c
                        ));
                    })
                    .await;
                    if !cancellable.is_cancelled() {
                        size_label.set_text(&format!(
                            "{} ({} item{})",
                            format_file_size(total),
                            count,
                            if count == 1 { "" } else { "s" }
                        ));
                    }
                }
            ));
        } else if file_size > 0 || !is_directory {
            add_row(&group, "Size", &format_file_size(file_size));
        }

        if !path_str.is_empty() {
            add_row(&group, "Location", &path_str);
        }

        page.add(&group);
        dialog.add(&page);
        dialog.present(Some(self));
    }
}
