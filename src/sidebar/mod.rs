mod imp;

use adw::subclass::prelude::*;
use glib::Object;
use gtk4::prelude::*;

glib::wrapper! {
    pub struct WrenSidebar(ObjectSubclass<imp::WrenSidebar>)
        @extends gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl Default for WrenSidebar {
    fn default() -> Self {
        Self::new()
    }
}

impl WrenSidebar {
    pub fn new() -> Self {
        Object::builder().build()
    }

    fn imp(&self) -> &imp::WrenSidebar {
        imp::WrenSidebar::from_obj(self)
    }

    pub fn populate_places(&self) {
        let imp = self.imp();
        let list = &imp.list_box;

        list.set_selection_mode(gtk4::SelectionMode::Single);

        let mut uris: Vec<String> = Vec::new();

        // ── Static places ────────────────────────────────────────────────────

        let places: &[(&str, &str, fn() -> String)] = &[
            ("Home", "user-home-symbolic", || {
                gio::File::for_path(glib::home_dir()).uri().to_string()
            }),
            ("Documents", "folder-documents-symbolic", || {
                glib::user_special_dir(glib::UserDirectory::Documents)
                    .map(|p| gio::File::for_path(p).uri().to_string())
                    .unwrap_or_default()
            }),
            ("Downloads", "folder-download-symbolic", || {
                glib::user_special_dir(glib::UserDirectory::Downloads)
                    .map(|p| gio::File::for_path(p).uri().to_string())
                    .unwrap_or_default()
            }),
            ("Music", "folder-music-symbolic", || {
                glib::user_special_dir(glib::UserDirectory::Music)
                    .map(|p| gio::File::for_path(p).uri().to_string())
                    .unwrap_or_default()
            }),
            ("Pictures", "folder-pictures-symbolic", || {
                glib::user_special_dir(glib::UserDirectory::Pictures)
                    .map(|p| gio::File::for_path(p).uri().to_string())
                    .unwrap_or_default()
            }),
            ("Videos", "folder-videos-symbolic", || {
                glib::user_special_dir(glib::UserDirectory::Videos)
                    .map(|p| gio::File::for_path(p).uri().to_string())
                    .unwrap_or_default()
            }),
            ("Trash", "user-trash-symbolic", || "trash:///".to_string()),
        ];

        for (label, icon, uri_fn) in places {
            let uri = uri_fn();
            let row = Self::build_place_row(label, icon);
            if !uri.is_empty() {
                Self::attach_sidebar_context_menu(&row, &uri, false);
            }
            list.append(&row);
            uris.push(uri);
        }

        imp.n_static_rows.set(uris.len() as i32);

        // ── User bookmarks ───────────────────────────────────────────────────

        let bookmarks = read_gtk_bookmarks();
        if !bookmarks.is_empty() {
            let header = Self::build_header_row("Bookmarks");
            list.append(&header);
            uris.push(String::new()); // non-navigable header

            for (uri, label) in &bookmarks {
                let display = if !label.is_empty() {
                    label.clone()
                } else {
                    gio::File::for_uri(uri)
                        .basename()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|| uri.clone())
                };
                list.append(&Self::build_place_row(&display, "folder-symbolic"));
                uris.push(uri.clone());
            }
        }

        *imp.place_uris.borrow_mut() = uris;

        list.connect_row_activated(glib::clone!(
            #[weak(rename_to = sidebar)]
            self,
            move |_, row| {
                let idx = row.index() as usize;
                let uri = sidebar.imp().place_uris.borrow().get(idx).cloned();
                if let Some(uri) = uri {
                    if uri.is_empty() {
                        return;
                    }
                    if let Some(win) = row
                        .root()
                        .and_then(|r| r.downcast::<crate::window::WrenWindow>().ok())
                    {
                        win.navigate_to(gio::File::for_uri(&uri));
                    }
                }
            }
        ));

        self.reload_volumes();
    }

    /// Rebuild the Devices section from currently mounted volumes.
    pub fn reload_volumes(&self) {
        let imp = self.imp();
        let list = &imp.list_box;

        // Remove everything after n_static_rows (bookmarks + old devices)
        let n_static = imp.n_static_rows.get();
        loop {
            match list.row_at_index(n_static) {
                Some(row) => list.remove(&row),
                None => break,
            }
        }

        // Trim place_uris back to static entries
        {
            let mut uris = imp.place_uris.borrow_mut();
            uris.truncate(n_static as usize);
        }

        self.append_bookmarks_section();
        self.append_volumes_section();
    }

    fn append_bookmarks_section(&self) {
        let imp = self.imp();
        let list = &imp.list_box;
        let bookmarks = read_gtk_bookmarks();
        if !bookmarks.is_empty() {
            list.append(&Self::build_header_row("Bookmarks"));
            imp.place_uris.borrow_mut().push(String::new());
            for (uri, label) in &bookmarks {
                let display = if !label.is_empty() {
                    label.clone()
                } else {
                    gio::File::for_uri(uri)
                        .basename()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|| uri.clone())
                };
                let row = Self::build_place_row(&display, "folder-symbolic");
                Self::attach_sidebar_context_menu(&row, uri, true);
                list.append(&row);
                imp.place_uris.borrow_mut().push(uri.clone());
            }
        }
    }

    fn attach_sidebar_context_menu(row: &gtk4::ListBoxRow, uri: &str, is_bookmark: bool) {
        let menu = gio::Menu::new();

        let open_section = gio::Menu::new();
        let tab_item = gio::MenuItem::new(Some("Open in New Tab"), None);
        tab_item.set_action_and_target_value(
            Some("win.open-tab-at"),
            Some(&uri.to_variant()),
        );
        open_section.append_item(&tab_item);
        let term_item = gio::MenuItem::new(Some("Open in Terminal"), None);
        term_item.set_action_and_target_value(
            Some("win.open-terminal-at"),
            Some(&uri.to_variant()),
        );
        open_section.append_item(&term_item);
        menu.append_section(None, &open_section);

        if is_bookmark {
            let bm_section = gio::Menu::new();
            let remove_item = gio::MenuItem::new(Some("Remove Bookmark"), None);
            remove_item.set_action_and_target_value(
                Some("win.remove-bookmark"),
                Some(&uri.to_variant()),
            );
            bm_section.append_item(&remove_item);
            menu.append_section(None, &bm_section);
        }

        let popover = gtk4::PopoverMenu::from_model(Some(&menu));
        popover.set_has_arrow(false);
        popover.set_parent(row);

        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3);
        gesture.connect_pressed(move |_, _, x, y| {
            popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
                x as i32, y as i32, 1, 1,
            )));
            popover.popup();
        });
        row.add_controller(gesture);
    }

    fn append_volumes_section(&self) {
        let imp = self.imp();
        let list = &imp.list_box;
        let monitor = gio::VolumeMonitor::get();
        let mounts: Vec<gio::Mount> = monitor.mounts();

        if !mounts.is_empty() {
            list.append(&Self::build_header_row("Devices"));
            imp.place_uris.borrow_mut().push(String::new());
            for mount in &mounts {
                let name = mount.name().to_string();
                let icon_name = mount
                    .icon()
                    .downcast::<gio::ThemedIcon>()
                    .ok()
                    .and_then(|ti| ti.names().into_iter().next())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "drive-harddisk-symbolic".to_string());
                let uri = mount.root().uri().to_string();
                let row = Self::build_place_row(&name, &icon_name);
                Self::attach_sidebar_context_menu(&row, &uri, false);
                list.append(&row);
                imp.place_uris.borrow_mut().push(uri);
            }
        }
    }

    /// Re-read bookmarks and volumes, rebuilding all dynamic sidebar rows.
    pub fn reload_bookmarks(&self) {
        self.reload_volumes();
    }

    /// Update sidebar highlight to match the current directory.
    /// Only exact matches are highlighted; navigating to a folder not in the
    /// sidebar deselects everything (no ancestor/prefix matching).
    pub fn set_location(&self, file: &gio::File) {
        let imp = self.imp();
        let uris = imp.place_uris.borrow();

        let match_idx = uris.iter().enumerate().find_map(|(i, uri)| {
            if uri.is_empty() {
                return None;
            }
            if file.equal(&gio::File::for_uri(uri)) {
                Some(i as i32)
            } else {
                None
            }
        });

        let row = match_idx.and_then(|idx| imp.list_box.row_at_index(idx));
        imp.list_box.select_row(row.as_ref());
    }

    fn build_place_row(label: &str, icon_name: &str) -> gtk4::ListBoxRow {
        let row = gtk4::ListBoxRow::new();
        let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        hbox.set_margin_top(6);
        hbox.set_margin_bottom(6);
        hbox.set_margin_start(8);
        hbox.set_margin_end(8);

        let icon = gtk4::Image::from_icon_name(icon_name);
        icon.set_pixel_size(16);

        let lbl = gtk4::Label::new(Some(label));
        lbl.set_xalign(0.0);
        lbl.set_hexpand(true);
        lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);

        hbox.append(&icon);
        hbox.append(&lbl);
        row.set_child(Some(&hbox));
        row
    }

    fn build_header_row(title: &str) -> gtk4::ListBoxRow {
        let row = gtk4::ListBoxRow::new();
        row.set_activatable(false);
        row.set_selectable(false);

        let lbl = gtk4::Label::new(Some(title));
        lbl.set_xalign(0.0);
        lbl.set_margin_top(10);
        lbl.set_margin_bottom(2);
        lbl.set_margin_start(8);
        lbl.set_margin_end(8);
        lbl.add_css_class("heading");
        lbl.add_css_class("dim-label");

        row.set_child(Some(&lbl));
        row
    }
}

fn read_gtk_bookmarks() -> Vec<(String, String)> {
    let mut path = glib::home_dir();
    path.push(".config");
    path.push("gtk-3.0");
    path.push("bookmarks");

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    content
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut parts = line.splitn(2, ' ');
            let uri = parts.next()?.to_string();
            if uri.is_empty() {
                return None;
            }
            let label = parts.next().unwrap_or("").trim().to_string();
            Some((uri, label))
        })
        .collect()
}
