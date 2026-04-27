mod imp;
pub mod tab;
pub mod undo;

use adw::prelude::*;
use adw::subclass::prelude::*;
use glib::Object;

use crate::application::WrenApplication;
use crate::model::{DirectoryModel, FileObject, SortKey};
use crate::window::tab::TabState;

glib::wrapper! {
    pub struct WrenWindow(ObjectSubclass<imp::WrenWindow>)
        @extends adw::ApplicationWindow, gtk4::ApplicationWindow, gtk4::Window, gtk4::Widget,
        @implements gio::ActionGroup, gio::ActionMap,
                    gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget,
                    gtk4::Native, gtk4::Root, gtk4::ShortcutManager;
}

impl WrenWindow {
    pub fn new(app: &WrenApplication) -> Self {
        Object::builder().property("application", app).build()
    }

    fn imp(&self) -> &imp::WrenWindow {
        imp::WrenWindow::from_obj(self)
    }

    // ── Tab helpers ──────────────────────────────────────────────────────────

    fn current_tab_index(&self) -> Option<usize> {
        let imp = self.imp();
        let page = imp.tab_view.selected_page()?;
        let child = page.child();
        imp.tabs
            .borrow()
            .iter()
            .position(|t| t.content_widget == child)
    }

    pub fn add_tab(&self, location: gio::File) {
        let imp = self.imp();
        let tab = TabState::new();

        tab.file_grid.connect_item_activated(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |file_obj| {
                if file_obj.is_directory() {
                    window.navigate_to(file_obj.file().clone());
                } else {
                    let uri = file_obj.file().uri();
                    if let Err(e) = gio::AppInfo::launch_default_for_uri(
                        uri.as_str(),
                        gio::AppLaunchContext::NONE,
                    ) {
                        window.show_toast(&format!("Cannot open: {e}"));
                    }
                }
            }
        ));
        tab.file_list.connect_item_activated(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |file_obj| {
                if file_obj.is_directory() {
                    window.navigate_to(file_obj.file().clone());
                } else {
                    let uri = file_obj.file().uri();
                    if let Err(e) = gio::AppInfo::launch_default_for_uri(
                        uri.as_str(),
                        gio::AppLaunchContext::NONE,
                    ) {
                        window.show_toast(&format!("Cannot open: {e}"));
                    }
                }
            }
        ));

        let menu = self.context_menu_model();
        tab.file_grid.setup_context_menu(&menu);
        tab.file_list.setup_context_menu(&menu);
        tab.file_grid.setup_drag_source();
        tab.file_list.setup_drag_source();

        let page = imp.tab_view.append(&tab.content_widget);
        page.set_title("Home");

        imp.tabs.borrow_mut().push(tab);
        imp.tab_view.set_selected_page(&page);

        self.navigate_to(location);
    }

    pub fn new_tab(&self) {
        self.add_tab(gio::File::for_path(glib::home_dir()));
    }

    pub fn close_tab(&self) {
        let imp = self.imp();
        if imp.tab_view.n_pages() <= 1 {
            return;
        }
        let Some(page) = imp.tab_view.selected_page() else {
            return;
        };
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        {
            let mut tabs = imp.tabs.borrow_mut();
            if idx < tabs.len() {
                tabs[idx].cancel_monitor();
                tabs.remove(idx);
            }
        }
        imp.tab_view.close_page(&page);
    }

    pub fn on_tab_switched(&self) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let (mode, sort_key, sort_reversed);
        {
            let imp = self.imp();
            let tabs = imp.tabs.borrow();
            let tab = match tabs.get(idx) {
                Some(t) => t,
                None => return,
            };
            mode = tab
                .view_stack
                .visible_child_name()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "grid".to_string());
            sort_key = tab.sort_key;
            sort_reversed = tab.sort_reversed;
            if let Some(loc) = tab.navigation.current() {
                imp.breadcrumb_bar.set_location(loc);
            }
        }
        // Sync view-mode action state
        if let Some(a) = self.lookup_action("set-view-mode") {
            if let Ok(a) = a.downcast::<gio::SimpleAction>() {
                a.set_state(&mode.to_variant());
            }
        }
        let icon = if mode == "list" {
            "view-list-symbolic"
        } else {
            "view-grid-symbolic"
        };
        self.imp().view_button.set_icon_name(icon);

        // Sync sort action states
        if let Some(a) = self.lookup_action("set-sort-key") {
            if let Ok(a) = a.downcast::<gio::SimpleAction>() {
                a.set_state(&sort_key.as_str().to_variant());
            }
        }
        if let Some(a) = self.lookup_action("toggle-sort-reversed") {
            if let Ok(a) = a.downcast::<gio::SimpleAction>() {
                a.set_state(&sort_reversed.to_variant());
            }
        }

        // Apply current zoom to the newly-selected tab's grid
        self.apply_zoom();

        self.update_nav_buttons();
        self.update_selection_actions();
    }

    // ── Navigation ───────────────────────────────────────────────────────────

    pub fn navigate_to(&self, location: gio::File) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let maybe_loc = {
            let mut tabs = self.imp().tabs.borrow_mut();
            tabs.get_mut(idx)
                .and_then(|t| t.navigation.navigate_to(location))
        };
        if let Some(loc) = maybe_loc {
            self.update_nav_buttons();
            self.load_location_for_tab(idx, loc);
        }
    }

    pub fn navigate_back(&self) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let maybe_loc = {
            let mut tabs = self.imp().tabs.borrow_mut();
            tabs.get_mut(idx).and_then(|t| t.navigation.navigate_back())
        };
        if let Some(loc) = maybe_loc {
            self.update_nav_buttons();
            self.load_location_for_tab(idx, loc);
        }
    }

    pub fn navigate_forward(&self) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let maybe_loc = {
            let mut tabs = self.imp().tabs.borrow_mut();
            tabs.get_mut(idx)
                .and_then(|t| t.navigation.navigate_forward())
        };
        if let Some(loc) = maybe_loc {
            self.update_nav_buttons();
            self.load_location_for_tab(idx, loc);
        }
    }

    pub fn navigate_up(&self) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let current = {
            let tabs = self.imp().tabs.borrow();
            tabs.get(idx).and_then(|t| t.navigation.current().cloned())
        };
        if let Some(loc) = current.and_then(|f| f.parent()) {
            self.navigate_to(loc);
        }
    }

    fn update_nav_buttons(&self) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let tabs = self.imp().tabs.borrow();
        if let Some(tab) = tabs.get(idx) {
            self.action_set_enabled("win.navigate-back", tab.navigation.can_go_back());
            self.action_set_enabled("win.navigate-forward", tab.navigation.can_go_forward());
        }
    }

    fn load_location_for_tab(&self, tab_idx: usize, location: gio::File) {
        let imp = self.imp();

        imp.breadcrumb_bar.set_location(&location);
        imp.sidebar.set_location(&location);

        {
            let tabs = imp.tabs.borrow();
            if let Some(tab) = tabs.get(tab_idx) {
                tab.cancel_monitor();
            }
        }

        let dir_model = DirectoryModel::new(location.clone());
        let search_text = imp.search_entry.text().to_lowercase();
        let show_hidden = imp.show_hidden.get();
        dir_model.set_filter(&search_text, show_hidden);

        // Apply the current tab's sort state to the new model
        let (sort_key, sort_reversed) = {
            let tabs = imp.tabs.borrow();
            tabs.get(tab_idx)
                .map(|t| (t.sort_key, t.sort_reversed))
                .unwrap_or((SortKey::Name, false))
        };
        dir_model.set_sort(sort_key, sort_reversed);

        let load_future;
        let store;
        let filter_model;
        let selection;
        let content_stack;
        {
            let mut tabs = imp.tabs.borrow_mut();
            let Some(tab) = tabs.get_mut(tab_idx) else {
                return;
            };
            tab.file_grid.set_model(&dir_model.selection);
            tab.file_list.set_model(&dir_model.selection);
            if let Some(old) = tab.dir_model.as_ref() {
                old.cancel();
            }
            store = dir_model.store.clone();
            filter_model = dir_model.filter_model.clone();
            selection = dir_model.selection.clone();
            content_stack = tab.content_stack.clone();
            load_future = dir_model.start_load();
            tab.dir_model = Some(dir_model);
        }

        // Apply current zoom to this tab's grid
        let icon_size = self.icon_size_for_zoom(imp.zoom_level.get());
        {
            let tabs = imp.tabs.borrow();
            if let Some(tab) = tabs.get(tab_idx) {
                tab.file_grid.set_icon_size(icon_size);
            }
        }

        selection.connect_selection_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _, _| {
                window.update_selection_actions();
            }
        ));

        {
            let tabs = imp.tabs.borrow();
            if let Some(tab) = tabs.get(tab_idx) {
                let page = imp.tab_view.page(&tab.content_widget);
                let title = location
                    .basename()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Files".to_string());
                page.set_title(&title);
            }
        }

        content_stack.set_visible_child_name("loading");

        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = window)]
            self,
            async move {
                match load_future.await {
                    Ok(()) => {
                        let search_text = window.imp().search_entry.text();
                        if store.n_items() == 0 {
                            content_stack.set_visible_child_name("empty");
                        } else if filter_model.n_items() == 0 && !search_text.is_empty() {
                            content_stack.set_visible_child_name("no-results");
                        } else {
                            content_stack.set_visible_child_name("files");
                        }
                        window.update_selection_actions();
                        window.start_dir_monitor(tab_idx, &location);
                    }
                    Err(e) => {
                        window.show_toast(&format!("Could not load folder: {e}"));
                        content_stack.set_visible_child_name("empty");
                    }
                }
            }
        ));
    }

    fn start_dir_monitor(&self, tab_idx: usize, location: &gio::File) {
        match location.monitor_directory(
            gio::FileMonitorFlags::WATCH_MOVES,
            gio::Cancellable::NONE,
        ) {
            Ok(monitor) => {
                monitor.connect_changed(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    move |_, _, _, event| {
                        use gio::FileMonitorEvent as E;
                        if matches!(
                            event,
                            E::Created | E::Deleted | E::Renamed | E::MovedIn | E::MovedOut
                        ) {
                            window.reload();
                        }
                    }
                ));
                let tabs = self.imp().tabs.borrow();
                if let Some(tab) = tabs.get(tab_idx) {
                    *tab.dir_monitor.borrow_mut() = Some(monitor);
                }
            }
            Err(e) => eprintln!("Cannot watch directory: {e}"),
        }
    }

    pub fn reload(&self) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let current = {
            let tabs = self.imp().tabs.borrow();
            tabs.get(idx).and_then(|t| t.navigation.current().cloned())
        };
        if let Some(loc) = current {
            self.load_location_for_tab(idx, loc);
        }
    }

    // ── Zoom ─────────────────────────────────────────────────────────────────

    pub fn zoom_in(&self) {
        let imp = self.imp();
        let current = imp.zoom_level.get();
        if current < 5 {
            imp.zoom_level.set(current + 1);
            self.apply_zoom();
        }
    }

    pub fn zoom_out(&self) {
        let imp = self.imp();
        let current = imp.zoom_level.get();
        if current > 1 {
            imp.zoom_level.set(current - 1);
            self.apply_zoom();
        }
    }

    pub fn zoom_reset(&self) {
        self.imp().zoom_level.set(3);
        self.apply_zoom();
    }

    fn apply_zoom(&self) {
        let icon_size = self.icon_size_for_zoom(self.imp().zoom_level.get());
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let tabs = self.imp().tabs.borrow();
        if let Some(tab) = tabs.get(idx) {
            tab.file_grid.set_icon_size(icon_size);
        }
    }

    fn icon_size_for_zoom(&self, level: i32) -> u32 {
        match level {
            1 => 32,
            2 => 48,
            4 => 96,
            5 => 128,
            _ => 64,
        }
    }

    // ── Sort ─────────────────────────────────────────────────────────────────

    pub fn set_sort_key(&self, key_str: &str) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let key = SortKey::from_str(key_str);
        {
            let mut tabs = self.imp().tabs.borrow_mut();
            if let Some(tab) = tabs.get_mut(idx) {
                tab.sort_key = key;
            }
        }
        self.apply_sort();
    }

    pub fn set_sort_reversed(&self, reversed: bool) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        {
            let mut tabs = self.imp().tabs.borrow_mut();
            if let Some(tab) = tabs.get_mut(idx) {
                tab.sort_reversed = reversed;
            }
        }
        self.apply_sort();
    }

    fn apply_sort(&self) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let tabs = self.imp().tabs.borrow();
        if let Some(tab) = tabs.get(idx) {
            if let Some(model) = tab.dir_model.as_ref() {
                model.set_sort(tab.sort_key, tab.sort_reversed);
            }
        }
    }

    // ── Search ───────────────────────────────────────────────────────────────

    pub fn toggle_search(&self) {
        let imp = self.imp();
        let active = !imp.search_bar.is_search_mode();
        imp.search_bar.set_search_mode(active);
        imp.search_button.set_active(active);
        if active {
            imp.search_entry.grab_focus();
        }
    }

    pub fn setup_search(&self) {
        let imp = self.imp();

        imp.search_bar
            .connect_search_mode_enabled_notify(glib::clone!(
                #[weak(rename_to = button)]
                imp.search_button,
                move |bar| {
                    button.set_active(bar.is_search_mode());
                }
            ));

        imp.search_entry.connect_search_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |entry| {
                let text = entry.text().to_lowercase();
                let show_hidden = window.imp().show_hidden.get();
                let Some(idx) = window.current_tab_index() else {
                    return;
                };
                let tabs = window.imp().tabs.borrow();
                if let Some(tab) = tabs.get(idx) {
                    if let Some(model) = tab.dir_model.as_ref() {
                        model.set_filter(&text, show_hidden);
                        if model.store.n_items() > 0 {
                            if model.filter_model.n_items() == 0 && !text.is_empty() {
                                tab.content_stack.set_visible_child_name("no-results");
                            } else {
                                tab.content_stack.set_visible_child_name("files");
                            }
                        }
                    }
                }
            }
        ));
    }

    pub fn apply_hidden_filter(&self) {
        let imp = self.imp();
        let show_hidden = imp.show_hidden.get();
        let text = imp.search_entry.text().to_lowercase();
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let tabs = imp.tabs.borrow();
        if let Some(tab) = tabs.get(idx) {
            if let Some(model) = tab.dir_model.as_ref() {
                model.set_filter(&text, show_hidden);
                if model.store.n_items() == 0 {
                    tab.content_stack.set_visible_child_name("empty");
                } else if model.filter_model.n_items() == 0 && !text.is_empty() {
                    tab.content_stack.set_visible_child_name("no-results");
                } else {
                    tab.content_stack.set_visible_child_name("files");
                }
            }
        }
    }

    // ── Context menu ─────────────────────────────────────────────────────────

    fn context_menu_model(&self) -> gio::MenuModel {
        let menu = gio::Menu::new();

        let open_section = gio::Menu::new();
        open_section.append(Some("Open"), Some("win.open-selection"));
        open_section.append(Some("Open With…"), Some("win.open-with"));
        open_section.append(Some("Open in Terminal"), Some("win.open-in-terminal"));
        menu.append_section(None, &open_section);

        let edit_section = gio::Menu::new();
        edit_section.append(Some("Cut"), Some("win.cut"));
        edit_section.append(Some("Copy"), Some("win.copy"));
        edit_section.append(Some("Paste"), Some("win.paste"));
        menu.append_section(None, &edit_section);

        let file_section = gio::Menu::new();
        file_section.append(Some("New Folder"), Some("win.new-folder"));
        file_section.append(Some("Rename"), Some("win.rename"));
        file_section.append(Some("Create Link"), Some("win.create-link"));
        file_section.append(Some("Add to Bookmarks"), Some("win.add-bookmark"));
        file_section.append(Some("Move to Trash"), Some("win.move-to-trash"));
        menu.append_section(None, &file_section);

        let info_section = gio::Menu::new();
        info_section.append(Some("Properties"), Some("win.properties"));
        menu.append_section(None, &info_section);

        menu.upcast()
    }

    // ── Selection helpers ────────────────────────────────────────────────────

    fn selected_file_objects(&self) -> Vec<FileObject> {
        let Some(idx) = self.current_tab_index() else {
            return vec![];
        };
        let tabs = self.imp().tabs.borrow();
        let Some(tab) = tabs.get(idx) else {
            return vec![];
        };
        let Some(model) = tab.dir_model.as_ref() else {
            return vec![];
        };
        let bitset = model.selection.selection();
        (0..bitset.size())
            .filter_map(|i| {
                let pos = bitset.nth(i as u32);
                model.selection.item(pos).and_downcast::<FileObject>()
            })
            .collect()
    }

    fn selected_files(&self) -> Vec<gio::File> {
        self.selected_file_objects()
            .iter()
            .map(|o| o.file().clone())
            .collect()
    }

    pub fn select_all(&self) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let tabs = self.imp().tabs.borrow();
        if let Some(tab) = tabs.get(idx) {
            if let Some(model) = tab.dir_model.as_ref() {
                model.selection.select_all();
            }
        }
    }

    pub fn set_view_mode(&self, mode: &str) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let tabs = self.imp().tabs.borrow();
        if let Some(tab) = tabs.get(idx) {
            tab.view_stack.set_visible_child_name(mode);
        }
        let icon = if mode == "list" {
            "view-list-symbolic"
        } else {
            "view-grid-symbolic"
        };
        self.imp().view_button.set_icon_name(icon);
    }

    // ── File operations ──────────────────────────────────────────────────────

    pub fn new_folder(&self) {
        let current = {
            let Some(idx) = self.current_tab_index() else {
                return;
            };
            let tabs = self.imp().tabs.borrow();
            tabs.get(idx).and_then(|t| t.navigation.current().cloned())
        };
        let Some(parent) = current else { return };

        let dialog = adw::AlertDialog::new(Some("New Folder"), None::<&str>);
        let entry = gtk4::Entry::new();
        entry.set_placeholder_text(Some("Folder name"));
        entry.set_activates_default(true);
        dialog.set_extra_child(Some(&entry));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("create", "Create");
        dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("create"));
        dialog.set_close_response("cancel");

        dialog.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                #[weak]
                entry,
                move |_, response| {
                    if response != "create" {
                        return;
                    }
                    let name = entry.text().to_string();
                    if name.is_empty() {
                        return;
                    }
                    let new_dir = parent.child(&name);
                    glib::spawn_future_local(glib::clone!(
                        #[weak]
                        window,
                        async move {
                            match new_dir
                                .make_directory_future(glib::Priority::DEFAULT)
                                .await
                            {
                                Ok(()) => {
                                    window.imp().undo_stack.borrow_mut().push(
                                        undo::UndoOp::NewFolder { dir: new_dir },
                                    );
                                    window.update_undo_actions();
                                    window.reload();
                                }
                                Err(e) => {
                                    window.show_toast(&format!("Could not create folder: {e}"))
                                }
                            }
                        }
                    ));
                }
            ),
        );
        dialog.present(Some(self));
    }

    pub fn rename_selection(&self) {
        let Some(file) = self.selected_files().into_iter().next() else {
            return;
        };
        let current_name = file
            .basename()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        let dialog = adw::AlertDialog::new(Some("Rename"), None::<&str>);
        let entry = gtk4::Entry::new();
        entry.set_text(&current_name);
        entry.select_region(0, -1);
        entry.set_activates_default(true);
        dialog.set_extra_child(Some(&entry));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("rename", "Rename");
        dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("rename"));
        dialog.set_close_response("cancel");

        dialog.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                #[weak]
                entry,
                move |_, response| {
                    if response != "rename" {
                        return;
                    }
                    let new_name = entry.text().to_string();
                    if new_name.is_empty() {
                        return;
                    }
                    let old_name = current_name.clone();
                    let file = file.clone();
                    glib::spawn_future_local(glib::clone!(
                        #[weak]
                        window,
                        async move {
                            match file
                                .set_display_name_future(&new_name, glib::Priority::DEFAULT)
                                .await
                            {
                                Ok(new_file) => {
                                    window.imp().undo_stack.borrow_mut().push(
                                        undo::UndoOp::Rename {
                                            file: new_file,
                                            old_name,
                                            new_name,
                                        },
                                    );
                                    window.update_undo_actions();
                                    window.reload();
                                }
                                Err(e) => window.show_toast(&format!("Could not rename: {e}")),
                            }
                        }
                    ));
                }
            ),
        );
        dialog.present(Some(self));
    }

    pub fn batch_rename(&self) {
        let files = self.selected_files();
        if files.len() < 2 {
            self.show_toast("Select multiple files to batch rename");
            return;
        }

        let dialog = adw::AlertDialog::new(
            Some("Batch Rename"),
            Some(&format!("Rename {} selected files", files.len())),
        );

        let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        let find_entry = gtk4::Entry::new();
        find_entry.set_placeholder_text(Some("Find…"));
        let replace_entry = gtk4::Entry::new();
        replace_entry.set_placeholder_text(Some("Replace with…"));
        replace_entry.set_activates_default(true);
        vbox.append(&find_entry);
        vbox.append(&replace_entry);

        dialog.set_extra_child(Some(&vbox));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("rename", "Rename");
        dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("rename"));
        dialog.set_close_response("cancel");

        dialog.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                #[weak]
                find_entry,
                #[weak]
                replace_entry,
                move |_, response| {
                    if response != "rename" {
                        return;
                    }
                    let find = find_entry.text().to_string();
                    let replace = replace_entry.text().to_string();
                    if find.is_empty() {
                        return;
                    }
                    let files = files.clone();
                    glib::spawn_future_local(glib::clone!(
                        #[weak]
                        window,
                        async move {
                            let mut errors = 0usize;
                            for file in &files {
                                let Some(old_name) = file
                                    .basename()
                                    .map(|p| p.to_string_lossy().into_owned())
                                else {
                                    continue;
                                };
                                let new_name = old_name.replace(&find, &replace);
                                if new_name == old_name {
                                    continue;
                                }
                                if let Err(_) = file
                                    .set_display_name_future(&new_name, glib::Priority::DEFAULT)
                                    .await
                                {
                                    errors += 1;
                                }
                            }
                            if errors > 0 {
                                window.show_toast(&format!(
                                    "Batch rename: {errors} file(s) could not be renamed"
                                ));
                            }
                            window.reload();
                        }
                    ));
                }
            ),
        );
        dialog.present(Some(self));
    }

    pub fn move_to_trash(&self) {
        let files = self.selected_files();
        if files.is_empty() {
            return;
        }
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = window)]
            self,
            async move {
                for file in &files {
                    if let Err(e) = file.trash_future(glib::Priority::DEFAULT).await {
                        window.show_toast(&format!("Could not trash: {e}"));
                        return;
                    }
                }
                window.reload();
            }
        ));
    }

    pub fn delete_permanently(&self) {
        let files = self.selected_files();
        if files.is_empty() {
            return;
        }
        let count = files.len();
        let body = if count == 1 {
            "This action cannot be undone.".to_string()
        } else {
            format!("Deleting {count} items. This action cannot be undone.")
        };
        let dialog = adw::AlertDialog::new(Some("Delete Permanently?"), Some(body.as_str()));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("delete", "Delete");
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let files_clone = files.clone();
        dialog.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, response| {
                    if response != "delete" {
                        return;
                    }
                    let files = files_clone.clone();
                    glib::spawn_future_local(glib::clone!(
                        #[weak]
                        window,
                        async move {
                            for file in &files {
                                if let Err(e) =
                                    file.delete_future(glib::Priority::DEFAULT).await
                                {
                                    window.show_toast(&format!("Could not delete: {e}"));
                                    return;
                                }
                            }
                            window.reload();
                        }
                    ));
                }
            ),
        );
        dialog.present(Some(self));
    }

    pub fn copy_selection(&self) {
        let files = self.selected_files();
        if files.is_empty() {
            return;
        }
        let uris: String = files
            .iter()
            .map(|f| f.uri().to_string())
            .collect::<Vec<_>>()
            .join("\r\n");
        self.imp().clipboard_files.replace(Some((files, false)));
        self.clipboard().set_text(&uris);
        self.show_toast("Copied");
    }

    pub fn cut_selection(&self) {
        let files = self.selected_files();
        if files.is_empty() {
            return;
        }
        let uris: String = files
            .iter()
            .map(|f| f.uri().to_string())
            .collect::<Vec<_>>()
            .join("\r\n");
        self.imp().clipboard_files.replace(Some((files, true)));
        self.clipboard().set_text(&uris);
        self.show_toast("Cut");
    }

    pub fn paste(&self) {
        let dest_dir = {
            let Some(idx) = self.current_tab_index() else {
                return;
            };
            let tabs = self.imp().tabs.borrow();
            match tabs.get(idx).and_then(|t| t.navigation.current().cloned()) {
                Some(d) => d,
                None => return,
            }
        };
        let clipboard_data = self.imp().clipboard_files.borrow().clone();
        let Some((files, is_cut)) = clipboard_data else {
            self.show_toast("Nothing to paste");
            return;
        };
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = window)]
            self,
            async move {
                for file in &files {
                    let Some(name) = file.basename() else {
                        continue;
                    };

                    let dest = dest_dir.child(&name);

                    if is_cut && dest.equal(file) {
                        continue;
                    }

                    let dest = if !is_cut && dest.equal(file) {
                        let stem = name
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or_default()
                            .to_string();
                        let ext = name
                            .extension()
                            .and_then(|s| s.to_str())
                            .map(|e| format!(".{}", e))
                            .unwrap_or_default();
                        let candidate = dest_dir.child(&format!("{} (Copy){}", stem, ext));
                        if candidate.query_exists(gio::Cancellable::NONE) {
                            let mut i = 2u32;
                            loop {
                                let c = dest_dir
                                    .child(&format!("{} (Copy {}){}", stem, i, ext));
                                if !c.query_exists(gio::Cancellable::NONE) {
                                    break c;
                                }
                                i += 1;
                            }
                        } else {
                            candidate
                        }
                    } else {
                        dest
                    };

                    let (copy_fut, _) = file.copy_future(
                        &dest,
                        gio::FileCopyFlags::NONE,
                        glib::Priority::DEFAULT,
                    );
                    if let Err(e) = copy_fut.await {
                        window.show_toast(&format!("Could not paste: {e}"));
                        return;
                    }
                    if is_cut {
                        if let Err(e) = file.trash_future(glib::Priority::DEFAULT).await {
                            window.show_toast(&format!("Could not move: {e}"));
                            return;
                        }
                    }
                }
                if is_cut {
                    window.imp().clipboard_files.replace(None);
                }
                window.reload();
            }
        ));
    }

    pub fn create_link(&self) {
        let Some(file) = self.selected_files().into_iter().next() else {
            return;
        };
        let dest_dir = {
            let Some(idx) = self.current_tab_index() else {
                return;
            };
            let tabs = self.imp().tabs.borrow();
            tabs.get(idx).and_then(|t| t.navigation.current().cloned())
        };
        let Some(dest_dir) = dest_dir else { return };

        let Some(target_path) = file.path() else {
            self.show_toast("Cannot create link: not a local file");
            return;
        };
        // Explicitly require a local dest dir; unwrap_or_default() would give an
        // empty PathBuf and create the symlink in the process working directory.
        let Some(dest_dir_path) = dest_dir.path() else {
            self.show_toast("Cannot create link: current directory is not local");
            return;
        };
        let Some(name) = file.basename() else { return };

        // Place "(link)" before the extension: "photo (link).jpg" not "photo.jpg (link)"
        let stem = name.file_stem().and_then(|s| s.to_str()).unwrap_or("link");
        let ext  = name.extension().and_then(|s| s.to_str());

        let make_name = |suffix: &str| match ext {
            Some(e) => format!("{stem}{suffix}.{e}"),
            None    => format!("{stem}{suffix}"),
        };

        // Find a non-colliding link path
        let link_path = {
            let first = dest_dir_path.join(make_name(" (link)"));
            if !first.exists() {
                first
            } else {
                (2u32..).find_map(|i| {
                    let p = dest_dir_path.join(make_name(&format!(" (link {i})")));
                    (!p.exists()).then_some(p)
                }).expect("will eventually find a free name")
            }
        };

        match std::os::unix::fs::symlink(&target_path, &link_path) {
            Ok(()) => self.reload(),
            Err(e) => self.show_toast(&format!("Could not create link: {e}")),
        }
    }

    pub fn add_bookmark(&self) {
        let file = self
            .selected_files()
            .into_iter()
            .find(|f| {
                f.query_file_type(gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE)
                    == gio::FileType::Directory
            })
            .or_else(|| {
                let Some(idx) = self.current_tab_index() else {
                    return None;
                };
                let tabs = self.imp().tabs.borrow();
                tabs.get(idx).and_then(|t| t.navigation.current().cloned())
            });
        let Some(file) = file else { return };

        let uri = file.uri().to_string();
        let bookmarks_path = {
            let mut p = glib::home_dir();
            p.push(".config");
            p.push("gtk-3.0");
            p.push("bookmarks");
            p
        };

        let content = std::fs::read_to_string(&bookmarks_path).unwrap_or_default();
        if content
            .lines()
            .any(|line| line.split_whitespace().next() == Some(uri.as_str()))
        {
            self.show_toast("Already bookmarked");
            return;
        }

        let new_content = if content.ends_with('\n') || content.is_empty() {
            format!("{}{}\n", content, uri)
        } else {
            format!("{}\n{}\n", content, uri)
        };

        if let Some(parent) = bookmarks_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&bookmarks_path, new_content) {
            Ok(()) => {
                self.show_toast("Bookmark added");
                self.imp().sidebar.reload_bookmarks();
            }
            Err(e) => self.show_toast(&format!("Could not save bookmark: {e}")),
        }
    }

    pub fn open_selection(&self) {
        for file in self.selected_files() {
            let ftype =
                file.query_file_type(gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE);
            if ftype == gio::FileType::Directory {
                self.navigate_to(file);
                return;
            }
            let uri = file.uri();
            if let Err(e) = gio::AppInfo::launch_default_for_uri(
                uri.as_str(),
                gio::AppLaunchContext::NONE,
            ) {
                self.show_toast(&format!("Cannot open: {e}"));
            }
        }
    }

    #[allow(deprecated)]
    pub fn open_with(&self) {
        let objs = self.selected_file_objects();
        let Some(obj) = objs.first() else { return };
        if obj.is_directory() {
            return;
        }
        let content_type = obj.content_type();
        if content_type.is_empty() {
            return;
        }

        let dialog = gtk4::AppChooserDialog::for_content_type(
            Some(self),
            gtk4::DialogFlags::MODAL | gtk4::DialogFlags::DESTROY_WITH_PARENT,
            &content_type,
        );
        let files: Vec<gio::File> = objs.iter().map(|o| o.file().clone()).collect();
        dialog.connect_response(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |dialog, response| {
                if response == gtk4::ResponseType::Ok {
                    if let Some(app_info) = dialog.app_info() {
                        let uris: Vec<_> = files.iter().map(|f| f.uri()).collect();
                        let uri_strs: Vec<&str> = uris.iter().map(|u| u.as_str()).collect();
                        if let Err(e) =
                            app_info.launch_uris(&uri_strs, gio::AppLaunchContext::NONE)
                        {
                            window.show_toast(&format!("Cannot open: {e}"));
                        }
                    }
                }
                dialog.close();
            }
        ));
        dialog.present();
    }

    pub fn focus_location(&self) {
        self.imp().breadcrumb_bar.enter_edit_mode();
    }

    pub fn show_properties(&self) {
        let objs = self.selected_file_objects();
        // Show current directory properties if nothing selected
        let file_obj = objs.first().cloned();
        let (name, content_type, file_size, path_str) = if let Some(ref obj) = file_obj {
            let path = obj
                .file()
                .path()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            (obj.name(), obj.content_type(), obj.file_size(), path)
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
            (name.into(), "inode/directory".into(), 0u64, path)
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

        if file_size > 0 || file_obj.as_ref().map(|o| !o.is_directory()).unwrap_or(false) {
            add_row(&group, "Size", &format_file_size(file_size));
        }

        if !path_str.is_empty() {
            add_row(&group, "Location", &path_str);
        }

        page.add(&group);
        dialog.add(&page);
        dialog.present(Some(self));
    }

    pub fn open_settings(&self) {
        let dialog = adw::PreferencesDialog::new();
        dialog.set_title("Settings");

        let page = adw::PreferencesPage::new();
        page.set_title("General");
        page.set_icon_name(Some("preferences-other-symbolic"));

        let group = adw::PreferencesGroup::new();
        group.set_title("Terminal");
        group.set_description(Some(
            "Leave blank to auto-detect a terminal. Example: kitty",
        ));

        let row = adw::EntryRow::new();
        row.set_title("Terminal command (e.g. kitty, alacritty)");

        let current_cmd = self
            .application()
            .and_downcast::<WrenApplication>()
            .map(|a| a.terminal_cmd())
            .unwrap_or_default();
        row.set_text(&current_cmd);

        row.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |entry| {
                let cmd = entry.text().to_string();
                if let Some(app) = window
                    .application()
                    .and_downcast::<WrenApplication>()
                {
                    app.set_terminal_cmd(&cmd);
                }
            }
        ));

        group.add(&row);
        page.add(&group);
        dialog.add(&page);
        dialog.present(Some(self));
    }

    pub fn open_in_terminal(&self) {
        let target = self
            .selected_file_objects()
            .into_iter()
            .find(|o| o.is_directory())
            .map(|o| o.file().clone())
            .or_else(|| {
                let Some(idx) = self.current_tab_index() else {
                    return None;
                };
                let tabs = self.imp().tabs.borrow();
                tabs.get(idx).and_then(|t| t.navigation.current().cloned())
            });

        let Some(dir) = target else { return };
        let Some(path) = dir.path() else {
            self.show_toast("Cannot open terminal: not a local path");
            return;
        };

        let known: &[(&str, &[&str])] = &[
            ("kgx", &["--working-directory"]),
            ("gnome-terminal", &["--working-directory"]),
            ("konsole", &["--workdir"]),
            ("xfce4-terminal", &["--working-directory"]),
            ("alacritty", &["--working-directory"]),
            ("kitty", &["--directory"]),
            ("xterm", &[]),
        ];

        let try_terminal = |cmd: &str| -> bool {
            for (k, wd_args) in known {
                if cmd == *k {
                    let mut c = std::process::Command::new(cmd);
                    for arg in *wd_args {
                        c.arg(arg);
                    }
                    c.arg(&path);
                    return c.spawn().is_ok();
                }
            }
            let mut c = std::process::Command::new(cmd);
            c.arg("--working-directory").arg(&path);
            if c.spawn().is_ok() {
                return true;
            }
            std::process::Command::new(cmd)
                .arg(&path)
                .spawn()
                .is_ok()
        };

        let custom = self
            .application()
            .and_downcast::<WrenApplication>()
            .map(|a| a.terminal_cmd())
            .unwrap_or_default();
        if !custom.is_empty() && try_terminal(&custom) {
            return;
        }

        for (cmd, wd_args) in known {
            let mut c = std::process::Command::new(cmd);
            for arg in *wd_args {
                c.arg(arg);
            }
            c.arg(&path);
            if c.spawn().is_ok() {
                return;
            }
        }
        self.show_toast("No terminal application found");
    }

    // ── Undo / Redo ──────────────────────────────────────────────────────────

    pub fn undo(&self) {
        let op = self.imp().undo_stack.borrow_mut().pop();
        self.update_undo_actions();
        let Some(op) = op else { return };
        match op {
            undo::UndoOp::Rename {
                file,
                old_name,
                new_name,
            } => {
                glib::spawn_future_local(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    async move {
                        match file
                            .set_display_name_future(&old_name, glib::Priority::DEFAULT)
                            .await
                        {
                            Ok(restored_file) => {
                                window.imp().redo_stack.borrow_mut().push(
                                    undo::UndoOp::Rename {
                                        file: restored_file,
                                        old_name: new_name,
                                        new_name: old_name,
                                    },
                                );
                                window.update_undo_actions();
                                window.reload();
                            }
                            Err(e) => window.show_toast(&format!("Undo failed: {e}")),
                        }
                    }
                ));
            }
            undo::UndoOp::NewFolder { dir } => {
                glib::spawn_future_local(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    async move {
                        match dir.trash_future(glib::Priority::DEFAULT).await {
                            Ok(()) => {
                                window.update_undo_actions();
                                window.reload();
                            }
                            Err(e) => window.show_toast(&format!("Undo failed: {e}")),
                        }
                    }
                ));
            }
        }
    }

    pub fn redo(&self) {
        let op = self.imp().redo_stack.borrow_mut().pop();
        self.update_undo_actions();
        let Some(op) = op else { return };
        match op {
            undo::UndoOp::Rename {
                file,
                old_name: _,
                new_name,
            } => {
                glib::spawn_future_local(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    async move {
                        match file
                            .set_display_name_future(&new_name, glib::Priority::DEFAULT)
                            .await
                        {
                            Ok(_) => {
                                window.update_undo_actions();
                                window.reload();
                            }
                            Err(e) => window.show_toast(&format!("Redo failed: {e}")),
                        }
                    }
                ));
            }
            undo::UndoOp::NewFolder { dir } => {
                glib::spawn_future_local(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    async move {
                        match dir
                            .make_directory_future(glib::Priority::DEFAULT)
                            .await
                        {
                            Ok(()) => {
                                window.update_undo_actions();
                                window.reload();
                            }
                            Err(e) => window.show_toast(&format!("Redo failed: {e}")),
                        }
                    }
                ));
            }
        }
    }

    // ── Action sensitivity ───────────────────────────────────────────────────

    pub fn update_selection_actions(&self) {
        let has_selection = !self.selected_files().is_empty();
        for action in &[
            "win.open-selection",
            "win.open-with",
            "win.rename",
            "win.move-to-trash",
            "win.delete-permanently",
            "win.cut",
            "win.copy",
            "win.create-link",
            "win.batch-rename",
        ] {
            self.action_set_enabled(action, has_selection);
        }
    }

    pub fn update_undo_actions(&self) {
        let imp = self.imp();
        self.action_set_enabled("win.undo", !imp.undo_stack.borrow().is_empty());
        self.action_set_enabled("win.redo", !imp.redo_stack.borrow().is_empty());
    }

    pub fn show_toast(&self, message: &str) {
        self.imp().toast_overlay.add_toast(adw::Toast::new(message));
    }
}

fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
