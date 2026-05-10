mod imp;

use adw::subclass::prelude::*;
use glib::Object;
use gtk4::gdk;
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

        // libadwaita's standard sidebar look: pill-shaped rows, soft
        // selection accent, dim header rows. Same class Nautilus and
        // every other GNOME-pattern app uses for their place list.
        list.add_css_class("navigation-sidebar");
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
                Self::attach_drop_target(&row, &uri);
            }
            list.append(&row);
            uris.push(uri);
        }

        imp.n_static_rows.set(uris.len() as i32);

        // ── User bookmarks ───────────────────────────────────────────────────

        let bookmarks_enabled = gio::Application::default()
            .and_downcast::<crate::application::WrenApplication>()
            .map_or(true, |a| a.bookmarks_enabled());
        let bookmarks = if bookmarks_enabled {
            read_gtk_bookmarks()
        } else {
            Vec::new()
        };
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
                        // Unmounted volume rows carry an attached gio::Volume.
                        if let Some(volume) = Self::volume_for_row(row) {
                            Self::trigger_mount(row, &volume);
                        }
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

        self.append_recents_section();
        self.append_bookmarks_section();
        self.append_volumes_section();
    }

    fn append_recents_section(&self) {
        let imp = self.imp();
        let list = &imp.list_box;

        let Some(root) = self.root() else { return };
        let Some(win) = root.downcast_ref::<crate::window::WrenWindow>() else { return };
        let Some(app) = win
            .application()
            .and_downcast::<crate::application::WrenApplication>()
        else {
            return;
        };
        if !app.recents_enabled() {
            return;
        }
        let recents = app.recent_uris();
        if recents.is_empty() {
            return;
        }

        list.append(&Self::build_header_row("Recent"));
        imp.place_uris.borrow_mut().push(String::new());
        for uri in &recents {
            let display = gio::File::for_uri(uri)
                .basename()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| uri.clone());
            let row = Self::build_place_row(&display, "document-open-recent-symbolic");
            Self::attach_sidebar_context_menu(&row, uri, false);
            Self::attach_drop_target(&row, uri);
            list.append(&row);
            imp.place_uris.borrow_mut().push(uri.clone());
        }
    }

    fn append_bookmarks_section(&self) {
        let imp = self.imp();
        let list = &imp.list_box;
        let bookmarks_enabled = gio::Application::default()
            .and_downcast::<crate::application::WrenApplication>()
            .map_or(true, |a| a.bookmarks_enabled());
        if !bookmarks_enabled {
            return;
        }
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
                Self::attach_drop_target(&row, uri);
                list.append(&row);
                imp.place_uris.borrow_mut().push(uri.clone());
            }
        }
    }

    fn attach_sidebar_context_menu(row: &gtk4::ListBoxRow, uri: &str, is_bookmark: bool) {
        let menu = gio::Menu::new();

        // Helper: parameterised menu item with a string target.
        fn item_for(label: &str, action: &str, target: &str) -> gio::MenuItem {
            let it = gio::MenuItem::new(Some(label), None);
            it.set_action_and_target_value(Some(action), Some(&target.to_variant()));
            it
        }

        let open_section = gio::Menu::new();
        open_section.append_item(&item_for("Open in New Tab", "win.open-tab-at", uri));
        open_section.append_item(&item_for("Open in New Window", "win.open-window-at", uri));
        // Terminal only makes sense for local locations; trash:/// /
        // recent:/// have no path so the action would fail silently.
        if !uri.starts_with("trash://") && !uri.starts_with("recent://") {
            open_section.append_item(&item_for("Open in Terminal", "win.open-terminal-at", uri));
        }
        menu.append_section(None, &open_section);

        let info_section = gio::Menu::new();
        info_section.append_item(&item_for("Copy Location", "win.copy-path-at", uri));
        menu.append_section(None, &info_section);

        if uri == "trash:///" {
            let trash_section = gio::Menu::new();
            trash_section.append(Some("Empty Trash"), Some("win.empty-trash"));
            menu.append_section(None, &trash_section);
        }

        if is_bookmark {
            let bm_section = gio::Menu::new();
            bm_section.append_item(&item_for("Remove Bookmark", "win.remove-bookmark", uri));
            menu.append_section(None, &bm_section);
        }

        let popover = gtk4::PopoverMenu::from_model(Some(&menu));
        popover.set_has_arrow(false);
        popover.set_parent(row);
        // The shutdown "leftover children" warning is filtered in
        // main::install_log_filter; unparenting at unrealize would
        // misfire when the sidebar list rebuilds.
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
        let volumes: Vec<gio::Volume> = monitor.volumes();

        // Build a fingerprint of every already-mounted thing so unmounted
        // volumes that point at the same backing account/device can be
        // suppressed. GOA-backed cloud accounts (Google Drive, Nextcloud,
        // …) often surface as BOTH a gio::Mount (auto-mounted at startup)
        // AND a separate gio::Volume for the same account that reports
        // get_mount() == None — the naive filter then renders both.
        //
        // We dedupe by:
        //   (a) the volume that owns each mount, if any (mount.volume())
        //   (b) each mount's root URI, matched against
        //       volume.activation_root().uri() — covers cases where mount
        //       and volume aren't linked through gio::Volume directly
        //   (c) each mount's drive, matched against volume.drive()
        let mounted_volumes: std::collections::HashSet<String> = mounts
            .iter()
            .filter_map(|m| m.volume().map(|v| v.identifier(gio::VOLUME_IDENTIFIER_KIND_UUID)
                .or_else(|| v.identifier(gio::VOLUME_IDENTIFIER_KIND_LABEL))
                .map(|s| s.to_string())
                .unwrap_or_else(|| v.name().to_string())))
            .collect();
        let mounted_roots: std::collections::HashSet<String> = mounts
            .iter()
            .map(|m| m.root().uri().to_string())
            .collect();
        let mounted_drives: std::collections::HashSet<*mut gio::ffi::GDrive> = mounts
            .iter()
            .filter_map(|m| m.drive())
            .map(|d| {
                use glib::translate::ToGlibPtr;
                let p: *mut gio::ffi::GDrive = d.to_glib_none().0;
                p
            })
            .collect();

        let unmounted: Vec<gio::Volume> = volumes
            .into_iter()
            .filter(|v| v.get_mount().is_none())
            .filter(|v| {
                let id = v
                    .identifier(gio::VOLUME_IDENTIFIER_KIND_UUID)
                    .or_else(|| v.identifier(gio::VOLUME_IDENTIFIER_KIND_LABEL))
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| v.name().to_string());
                if mounted_volumes.contains(&id) {
                    return false;
                }
                if let Some(root) = v.activation_root() {
                    if mounted_roots.contains(root.uri().as_str()) {
                        return false;
                    }
                }
                if let Some(d) = v.drive() {
                    use glib::translate::ToGlibPtr;
                    let p: *mut gio::ffi::GDrive = d.to_glib_none().0;
                    if mounted_drives.contains(&p) {
                        return false;
                    }
                }
                true
            })
            .collect();

        if mounts.is_empty() && unmounted.is_empty() {
            return;
        }

        list.append(&Self::build_header_row("Devices"));
        imp.place_uris.borrow_mut().push(String::new());

        // Mounted devices first.
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
            let row = Self::build_volume_row(&name, &icon_name, false);
            Self::attach_volume_eject_button(&row, mount);
            Self::attach_volume_context_menu(&row, Some(mount.clone()), None);
            Self::attach_drop_target(&row, &uri);
            list.append(&row);
            imp.place_uris.borrow_mut().push(uri);
        }

        // Unmounted volumes — clicking activates them via mount_future.
        for volume in &unmounted {
            // Stronger visual cue than just dimming the icon (which doesn't
            // affect non-symbolic gicons GVFS hands out for cloud accounts):
            // append " (offline)" to the label, force the symbolic
            // network-offline-symbolic icon, and italicise via the CSS
            // class wired in build_volume_row.
            let name = format!("{} (offline)", volume.name());
            let row = Self::build_volume_row(&name, "network-offline-symbolic", true);
            Self::attach_volume_context_menu(&row, None, Some(volume.clone()));
            list.append(&row);
            // Empty URI = handled specially by row_activated; we still need
            // an entry so indices line up with list rows.
            imp.place_uris.borrow_mut().push(String::new());

            // Stash the gio::Volume on the row so the activation handler
            // can pick it back up (we're using the qdata escape hatch
            // because place_uris is an index→uri map only).
            unsafe {
                row.set_data::<gio::Volume>("wren-volume", volume.clone());
            }
        }
    }

    /// Connect to gio::VolumeMonitor signals so devices appearing or
    /// disappearing trigger an automatic sidebar refresh.
    pub(crate) fn connect_volume_monitor(&self) {
        let imp = self.imp();
        let monitor = gio::VolumeMonitor::get();
        let mut handlers = imp.volume_monitor_handlers.borrow_mut();
        if !handlers.is_empty() {
            return;
        }

        let h1 = monitor.connect_mount_added(glib::clone!(
            #[weak(rename_to = sidebar)]
            self,
            move |_, _| sidebar.reload_volumes()
        ));
        let h2 = monitor.connect_mount_removed(glib::clone!(
            #[weak(rename_to = sidebar)]
            self,
            move |_, _| sidebar.reload_volumes()
        ));
        let h3 = monitor.connect_volume_added(glib::clone!(
            #[weak(rename_to = sidebar)]
            self,
            move |_, _| sidebar.reload_volumes()
        ));
        let h4 = monitor.connect_volume_removed(glib::clone!(
            #[weak(rename_to = sidebar)]
            self,
            move |_, _| sidebar.reload_volumes()
        ));
        handlers.extend([h1, h2, h3, h4]);
    }

    /// Find the row index for an unmounted-volume row that the user
    /// activated. Used by the row_activated handler.
    pub(crate) fn volume_for_row(row: &gtk4::ListBoxRow) -> Option<gio::Volume> {
        unsafe { row.data::<gio::Volume>("wren-volume").map(|nn| nn.as_ref().clone()) }
    }

    /// Build a sidebar row that can host an inline eject button on the
    /// trailing edge. Same layout as `build_place_row` but exposes the
    /// inner `GtkBox` so callers can append controls.
    fn build_volume_row(label: &str, icon_name: &str, dim_icon: bool) -> gtk4::ListBoxRow {
        let row = gtk4::ListBoxRow::new();
        let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        hbox.set_margin_top(6);
        hbox.set_margin_bottom(6);
        hbox.set_margin_start(8);
        hbox.set_margin_end(8);

        let icon = gtk4::Image::from_icon_name(icon_name);
        icon.set_pixel_size(16);
        if dim_icon {
            icon.add_css_class("wren-volume-unmounted");
        }

        let lbl = gtk4::Label::new(Some(label));
        lbl.set_xalign(0.0);
        lbl.set_hexpand(true);
        lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        if dim_icon {
            lbl.add_css_class("wren-volume-unmounted");
        }

        hbox.append(&icon);
        hbox.append(&lbl);
        row.set_child(Some(&hbox));
        row
    }

    /// Add a `media-eject-symbolic` button to a mounted-volume row when
    /// the mount supports unmount or eject. Click triggers eject (preferred)
    /// or unmount, as appropriate.
    fn attach_volume_eject_button(row: &gtk4::ListBoxRow, mount: &gio::Mount) {
        let can_eject = mount.can_eject();
        let can_unmount = mount.can_unmount();
        if !can_eject && !can_unmount {
            return;
        }
        let Some(hbox) = row.child().and_downcast::<gtk4::Box>() else {
            return;
        };

        let btn = gtk4::Button::from_icon_name("media-eject-symbolic");
        btn.set_valign(gtk4::Align::Center);
        btn.add_css_class("flat");
        btn.add_css_class("wren-volume-eject");
        btn.set_tooltip_text(Some(if can_eject { "Eject" } else { "Unmount" }));

        let mount_clone = mount.clone();
        let prefer_eject = can_eject;
        btn.connect_clicked(glib::clone!(
            #[weak]
            row,
            move |_| {
                Self::trigger_unmount_or_eject(&row, &mount_clone, prefer_eject);
            }
        ));
        hbox.append(&btn);
    }

    /// Build a context menu for a volume row. Either a mounted volume
    /// (`mount` is Some) or a present-but-unmounted volume (`volume` is Some).
    fn attach_volume_context_menu(
        row: &gtk4::ListBoxRow,
        mount: Option<gio::Mount>,
        volume: Option<gio::Volume>,
    ) {
        let menu = gio::Menu::new();
        let actions = gio::SimpleActionGroup::new();

        if let Some(mount) = mount.as_ref() {
            let uri = mount.root().uri().to_string();

            let open_section = gio::Menu::new();
            let item = gio::MenuItem::new(Some("Open in New Tab"), None);
            item.set_action_and_target_value(
                Some("win.open-tab-at"),
                Some(&uri.to_variant()),
            );
            open_section.append_item(&item);
            let item = gio::MenuItem::new(Some("Open in New Window"), None);
            item.set_action_and_target_value(
                Some("win.open-window-at"),
                Some(&uri.to_variant()),
            );
            open_section.append_item(&item);
            menu.append_section(None, &open_section);

            let info_section = gio::Menu::new();
            let item = gio::MenuItem::new(Some("Copy Location"), None);
            item.set_action_and_target_value(
                Some("win.copy-path-at"),
                Some(&uri.to_variant()),
            );
            info_section.append_item(&item);
            menu.append_section(None, &info_section);

            let device_section = gio::Menu::new();
            if mount.can_unmount() {
                device_section.append(Some("Unmount"), Some("volume.unmount"));
                let unmount = gio::SimpleAction::new("unmount", None);
                let mount_clone = mount.clone();
                unmount.connect_activate(glib::clone!(
                    #[weak]
                    row,
                    move |_, _| {
                        Self::trigger_unmount_or_eject(&row, &mount_clone, false);
                    }
                ));
                actions.add_action(&unmount);
            }
            if mount.can_eject() {
                device_section.append(Some("Eject"), Some("volume.eject"));
                let eject = gio::SimpleAction::new("eject", None);
                let mount_clone = mount.clone();
                eject.connect_activate(glib::clone!(
                    #[weak]
                    row,
                    move |_, _| {
                        Self::trigger_unmount_or_eject(&row, &mount_clone, true);
                    }
                ));
                actions.add_action(&eject);
            }
            menu.append_section(None, &device_section);
        }

        if let Some(volume) = volume.as_ref() {
            let device_section = gio::Menu::new();
            device_section.append(Some("Mount"), Some("volume.mount"));
            let mount_act = gio::SimpleAction::new("mount", None);
            let volume_clone = volume.clone();
            mount_act.connect_activate(glib::clone!(
                #[weak]
                row,
                move |_, _| {
                    Self::trigger_mount(&row, &volume_clone);
                }
            ));
            actions.add_action(&mount_act);

            if volume.can_eject() {
                device_section.append(Some("Eject"), Some("volume.eject"));
                let eject = gio::SimpleAction::new("eject", None);
                let volume_clone = volume.clone();
                eject.connect_activate(glib::clone!(
                    #[weak]
                    row,
                    move |_, _| {
                        Self::trigger_volume_eject(&row, &volume_clone);
                    }
                ));
                actions.add_action(&eject);
            }
            menu.append_section(None, &device_section);
        }

        row.insert_action_group("volume", Some(&actions));

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

    /// Run `mount.eject_with_operation_future` (preferred) or
    /// `mount.unmount_with_operation_future`. On success, navigate any
    /// tab currently inside the unmounted root back to home.
    fn trigger_unmount_or_eject(row: &gtk4::ListBoxRow, mount: &gio::Mount, prefer_eject: bool) {
        let mount = mount.clone();
        let win = row
            .root()
            .and_downcast::<crate::window::WrenWindow>();
        let root = mount.root();
        let op = gio::MountOperation::new();
        let prefer_eject = prefer_eject && mount.can_eject();
        glib::spawn_future_local(async move {
            let result = if prefer_eject {
                mount
                    .eject_with_operation_future(gio::MountUnmountFlags::NONE, Some(&op))
                    .await
            } else {
                mount
                    .unmount_with_operation_future(gio::MountUnmountFlags::NONE, Some(&op))
                    .await
            };
            if let Some(win) = win {
                match result {
                    Ok(()) => {
                        let navigated = win.leave_unmounted_root(&root);
                        let action = if prefer_eject { "Ejected" } else { "Unmounted" };
                        if navigated {
                            win.show_toast(&format!("{action}; navigated to Home"));
                        } else {
                            win.show_toast(action);
                        }
                    }
                    Err(e) => {
                        let action = if prefer_eject { "eject" } else { "unmount" };
                        win.show_toast(&format!("Could not {action}: {e}"));
                    }
                }
            }
        });
    }

    /// Mount an unmounted volume. On success, navigate the active tab
    /// into its newly-attached root.
    fn trigger_mount(row: &gtk4::ListBoxRow, volume: &gio::Volume) {
        let volume = volume.clone();
        let win = row
            .root()
            .and_downcast::<crate::window::WrenWindow>();
        // gtk4::MountOperation extends gio::MountOperation and renders a
        // proper dialog for ask-password / ask-question signals — the
        // bare gio::MountOperation just errors when those fire, which
        // is why cloud-account remounts silently failed before.
        let op = gtk4::MountOperation::new(win.as_ref());
        let op_g: gio::MountOperation = op.upcast();
        glib::spawn_future_local(async move {
            match volume
                .mount_future(gio::MountMountFlags::NONE, Some(&op_g))
                .await
            {
                Ok(()) => {
                    if let (Some(win), Some(mount)) = (win, volume.get_mount()) {
                        win.navigate_to(mount.root());
                    }
                }
                Err(e) => {
                    if let Some(win) = win {
                        win.show_toast(&format!("Could not mount: {e}"));
                    }
                }
            }
        });
    }

    /// Eject a volume that has no current mount (some optical/removable
    /// devices expose eject directly on the volume).
    fn trigger_volume_eject(row: &gtk4::ListBoxRow, volume: &gio::Volume) {
        let volume = volume.clone();
        let win = row
            .root()
            .and_downcast::<crate::window::WrenWindow>();
        let op = gio::MountOperation::new();
        glib::spawn_future_local(async move {
            match volume
                .eject_with_operation_future(gio::MountUnmountFlags::NONE, Some(&op))
                .await
            {
                Ok(()) => {
                    if let Some(win) = win {
                        win.show_toast("Ejected");
                    }
                }
                Err(e) => {
                    if let Some(win) = win {
                        win.show_toast(&format!("Could not eject: {e}"));
                    }
                }
            }
        });
    }

    /// Re-read bookmarks and volumes, rebuilding all dynamic sidebar rows.
    pub fn reload_bookmarks(&self) {
        self.reload_volumes();
    }

    /// Rebuild the Recent section (in practice all dynamic rows, since
    /// they share the same n_static_rows-based truncation).
    pub fn reload_recents(&self) {
        self.reload_volumes();
    }

    /// Highlight the row whose URI matches `file` via the standard
    /// `:selected` pseudo (libadwaita's navigation-sidebar styling).
    /// Picks an exact `gio::File::equal` match if one exists, otherwise
    /// the deepest ancestor by prefix.
    pub fn set_location(&self, file: &gio::File) {
        let imp = self.imp();
        let uris = imp.place_uris.borrow();

        let mut best: Option<(usize, usize)> = None;
        for (i, uri) in uris.iter().enumerate() {
            if uri.is_empty() {
                continue;
            }
            let other = gio::File::for_uri(uri);
            if file.equal(&other) {
                best = Some((i, usize::MAX));
                break;
            }
            if file.has_prefix(&other) {
                let depth = other.uri().len();
                match best {
                    Some((_, d)) if d >= depth => {}
                    _ => best = Some((i, depth)),
                }
            }
        }

        match best {
            Some((idx, _)) => {
                if let Some(row) = imp.list_box.row_at_index(idx as i32) {
                    imp.list_box.select_row(Some(&row));
                }
            }
            None => imp.list_box.unselect_all(),
        }
    }

    /// Attach a DropTarget so dragging files onto the row moves/copies them
    /// into the URI. trash:/// trashes the dropped files; recent:/// and
    /// section headers (empty URI) are skipped.
    fn attach_drop_target(row: &gtk4::ListBoxRow, uri: &str) {
        if uri.is_empty() || uri == "recent:///" {
            return;
        }
        let drop = gtk4::DropTarget::new(
            gdk::FileList::static_type(),
            gdk::DragAction::COPY | gdk::DragAction::MOVE,
        );
        let uri_owned = uri.to_string();
        drop.connect_drop(glib::clone!(
            #[weak]
            row,
            #[upgrade_or]
            false,
            move |drop_target, value, _x, _y| {
                let Ok(file_list) = value.get::<gdk::FileList>() else {
                    return false;
                };
                let files = file_list.files();
                if files.is_empty() {
                    return false;
                }
                let Some(win) = row
                    .root()
                    .and_downcast::<crate::window::WrenWindow>()
                else {
                    return false;
                };
                if uri_owned == "trash:///" {
                    win.trash_files(files);
                    return true;
                }
                let action = drop_target
                    .current_drop()
                    .map(|d| d.actions())
                    .unwrap_or(gdk::DragAction::COPY);
                let is_move = !action.contains(gdk::DragAction::COPY)
                    && action.contains(gdk::DragAction::MOVE);
                let dest = gio::File::for_uri(&uri_owned);
                win.drop_files(files, Some(dest), is_move);
                true
            }
        ));
        row.add_controller(drop);
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
        lbl.set_margin_top(12);
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
    let mut path = glib::user_config_dir();
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
