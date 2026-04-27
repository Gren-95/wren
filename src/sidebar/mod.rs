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
            list.append(&Self::build_place_row(label, icon));
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

        *imp.place_uris.borrow_mut() = uris.clone();

        list.connect_row_activated(move |list_box, row| {
            let idx = row.index() as usize;
            if let Some(uri) = uris.get(idx) {
                if uri.is_empty() {
                    return;
                }
                let file = gio::File::for_uri(uri);
                if let Some(window) = row
                    .root()
                    .and_then(|r| r.downcast::<crate::window::WrenWindow>().ok())
                {
                    list_box.select_row(None::<&gtk4::ListBoxRow>);
                    window.navigate_to(file);
                }
            }
        });
    }

    /// Re-read bookmarks from disk and update the rows below the static places.
    pub fn reload_bookmarks(&self) {
        let imp = self.imp();
        let list = &imp.list_box;

        // Remove all rows beyond the static places
        let n_static = imp.n_static_rows.get();
        loop {
            match list.row_at_index(n_static) {
                Some(row) => list.remove(&row),
                None => break,
            }
        }

        // Rebuild URI list from scratch (keeps static entries intact)
        let mut uris: Vec<String> = imp
            .place_uris
            .borrow()
            .iter()
            .take(n_static as usize)
            .cloned()
            .collect();

        let bookmarks = read_gtk_bookmarks();
        if !bookmarks.is_empty() {
            list.append(&Self::build_header_row("Bookmarks"));
            uris.push(String::new());

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
    }

    /// Update sidebar highlight to match the current directory.
    pub fn set_location(&self, file: &gio::File) {
        let imp = self.imp();
        let uris = imp.place_uris.borrow();

        let mut best_idx: Option<i32> = None;
        let mut best_depth: usize = 0;

        for (i, uri) in uris.iter().enumerate() {
            if uri.is_empty() {
                continue;
            }
            let bookmark = gio::File::for_uri(uri);
            if file.equal(&bookmark) {
                best_idx = Some(i as i32);
                break;
            }
            if file.has_prefix(&bookmark) {
                let depth = bookmark
                    .path()
                    .map(|p| p.components().count())
                    .unwrap_or(0);
                if depth > best_depth {
                    best_depth = depth;
                    best_idx = Some(i as i32);
                }
            }
        }

        match best_idx {
            Some(idx) => {
                let row = imp.list_box.row_at_index(idx);
                imp.list_box.select_row(row.as_ref());
            }
            None => {
                imp.list_box.select_row(None::<&gtk4::ListBoxRow>);
            }
        }
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
