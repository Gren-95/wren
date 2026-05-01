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
        let win: Self = Object::builder().property("application", app).build();
        win.connect_close_request(|w| {
            w.save_window_size();
            glib::Propagation::Proceed
        });
        win
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
        let mut tab = TabState::new();

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
        tab.file_grid.setup_drop_target();
        tab.file_list.setup_drop_target();
        tab.file_grid.setup_empty_area_click();
        tab.file_list.setup_empty_area_click();
        tab.file_grid.set_show_extensions(imp.show_extensions.get());
        tab.file_list.set_show_extensions(imp.show_extensions.get());

        // Restore persisted view mode and sort for new tabs
        if let Some(app) = self.application().and_downcast::<WrenApplication>() {
            let mode = app.view_mode();
            tab.view_stack.set_visible_child_name(&mode);
            let sort_key = crate::model::SortKey::from_str(&app.sort_key());
            tab.sort_key = sort_key;
            tab.sort_reversed = app.sort_reversed();
            tab.file_list.set_sort_state(sort_key.as_str(), tab.sort_reversed);
        }

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
                if let Some(model) = tabs[idx].dir_model.as_ref() {
                    model.cancel();
                }
                tabs.remove(idx);
            }
        }
        imp.tab_view.close_page(&page);
    }

    pub fn on_tab_switched(&self) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let (mode, sort_key, sort_reversed, window_title);
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
            window_title = tab
                .navigation
                .current()
                .and_then(|loc| loc.basename())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Files".to_string());
            if let Some(loc) = tab.navigation.current() {
                imp.breadcrumb_bar.set_location(loc);
            }
        }
        self.set_title(Some(&window_title));
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
        self.update_list_sort_headers();

        // Close search and reset the new tab's filter
        {
            let imp = self.imp();
            imp.search_bar.set_search_mode(false);
            imp.search_entry.set_text("");
        }
        {
            let imp = self.imp();
            let show_hidden = imp.show_hidden.get();
            let tabs = imp.tabs.borrow();
            if let Some(tab) = tabs.get(idx) {
                if let Some(model) = tab.dir_model.as_ref() {
                    model.set_filter("", show_hidden);
                    if tab.content_stack.visible_child_name().as_deref() == Some("no-results") {
                        tab.content_stack.set_visible_child_name("files");
                    }
                }
            }
        }
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

    pub fn navigate_home(&self) {
        self.navigate_to(gio::File::for_path(glib::home_dir()));
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
        let status_bar;
        {
            let mut tabs = imp.tabs.borrow_mut();
            let Some(tab) = tabs.get_mut(tab_idx) else {
                return;
            };
            tab.file_grid.set_model(&dir_model.selection);
            tab.file_list.set_model(&dir_model.selection);
            tab.file_grid.scroll_to_top();
            tab.file_list.scroll_to_top();
            if let Some(old) = tab.dir_model.as_ref() {
                old.cancel();
            }
            store = dir_model.store.clone();
            filter_model = dir_model.filter_model.clone();
            selection = dir_model.selection.clone();
            content_stack = tab.content_stack.clone();
            status_bar = tab.status_bar.clone();
            load_future = dir_model.start_load();
            tab.dir_model = Some(dir_model);
        }

        let level = imp.zoom_level.get();
        let icon_size = self.icon_size_for_zoom(level);
        let list_icon_size = self.list_icon_size_for_zoom(level);
        let load_gen;
        {
            let tabs = imp.tabs.borrow();
            if let Some(tab) = tabs.get(tab_idx) {
                tab.file_grid.set_icon_size(icon_size);
                tab.file_list.set_icon_size(list_icon_size);
                let next = tab.load_gen.get() + 1;
                tab.load_gen.set(next);
                load_gen = next;
            } else {
                return;
            }
        }

        selection.connect_selection_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _, _| {
                window.update_selection_actions();
            }
        ));

        filter_model.connect_items_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _, _, _| {
                window.update_status_bar();
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
                self.set_title(Some(&title));
            }
        }

        status_bar.set_text("");
        content_stack.set_visible_child_name("files");

        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = window)]
            self,
            async move {
                match load_future.await {
                    Ok(()) => {
                        let is_current = {
                            let tabs = window.imp().tabs.borrow();
                            tabs.get(tab_idx)
                                .map_or(false, |t| t.load_gen.get() == load_gen)
                        };
                        if !is_current {
                            return;
                        }
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
                        {
                            let tabs = window.imp().tabs.borrow();
                            if let Some(tab) = tabs.get(tab_idx) {
                                tab.error_page.set_description(Some(&e.message().to_string()));
                            }
                        }
                        content_stack.set_visible_child_name("error");
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
            let new_level = current + 1;
            imp.zoom_level.set(new_level);
            imp.zoom_adjustment.set_value(new_level as f64);
            self.apply_zoom();
            self.save_zoom();
        }
    }

    pub fn zoom_out(&self) {
        let imp = self.imp();
        let current = imp.zoom_level.get();
        if current > 1 {
            let new_level = current - 1;
            imp.zoom_level.set(new_level);
            imp.zoom_adjustment.set_value(new_level as f64);
            self.apply_zoom();
            self.save_zoom();
        }
    }

    pub fn zoom_reset(&self) {
        let imp = self.imp();
        imp.zoom_level.set(3);
        imp.zoom_adjustment.set_value(3.0);
        self.apply_zoom();
        self.save_zoom();
    }

    fn save_zoom(&self) {
        if let Some(app) = self.application().and_downcast::<WrenApplication>() {
            app.set_zoom_level(self.imp().zoom_level.get());
        }
    }

    fn apply_zoom(&self) {
        let level = self.imp().zoom_level.get();
        let grid_size = self.icon_size_for_zoom(level);
        let list_size = self.list_icon_size_for_zoom(level);
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let tabs = self.imp().tabs.borrow();
        if let Some(tab) = tabs.get(idx) {
            tab.file_grid.set_icon_size(grid_size);
            tab.file_list.set_icon_size(list_size);
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

    fn list_icon_size_for_zoom(&self, level: i32) -> u32 {
        match level {
            1 => 16,
            2 => 20,
            4 => 32,
            5 => 40,
            _ => 24,
        }
    }

    // ── Sort ─────────────────────────────────────────────────────────────────

    pub fn set_sort_key(&self, key_str: &str) {
        let Some(idx) = self.current_tab_index() else { return };
        let key = SortKey::from_str(key_str);
        let new_reversed;
        {
            let mut tabs = self.imp().tabs.borrow_mut();
            let Some(tab) = tabs.get_mut(idx) else { return };
            if tab.sort_key == key {
                tab.sort_reversed = !tab.sort_reversed;
            } else {
                tab.sort_key = key;
                tab.sort_reversed = false;
            }
            new_reversed = tab.sort_reversed;
        }
        if let Some(a) = self
            .lookup_action("toggle-sort-reversed")
            .and_downcast::<gio::SimpleAction>()
        {
            a.set_state(&new_reversed.to_variant());
        }
        self.apply_sort();
        self.update_list_sort_headers();
        if let Some(app) = self.application().and_downcast::<WrenApplication>() {
            app.set_sort_key_pref(key_str);
            app.set_sort_reversed_pref(new_reversed);
        }
    }

    pub fn set_sort_reversed(&self, reversed: bool) {
        let Some(idx) = self.current_tab_index() else { return };
        {
            let mut tabs = self.imp().tabs.borrow_mut();
            if let Some(tab) = tabs.get_mut(idx) {
                tab.sort_reversed = reversed;
            }
        }
        if let Some(app) = self.application().and_downcast::<WrenApplication>() {
            app.set_sort_reversed_pref(reversed);
        }
        self.apply_sort();
        self.update_list_sort_headers();
    }

    fn update_list_sort_headers(&self) {
        let Some(idx) = self.current_tab_index() else { return };
        let tabs = self.imp().tabs.borrow();
        if let Some(tab) = tabs.get(idx) {
            tab.file_list.set_sort_state(tab.sort_key.as_str(), tab.sort_reversed);
        }
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

    pub fn apply_extensions_setting(&self) {
        let imp = self.imp();
        let show = imp.show_extensions.get();
        let tabs = imp.tabs.borrow();
        for tab in tabs.iter() {
            tab.file_grid.set_show_extensions(show);
            tab.file_list.set_show_extensions(show);
        }
    }

    pub fn apply_hidden_filter(&self) {
        let imp = self.imp();
        let show_hidden = imp.show_hidden.get();
        let current_idx = self.current_tab_index();
        let tabs = imp.tabs.borrow();
        for (i, tab) in tabs.iter().enumerate() {
            let Some(model) = tab.dir_model.as_ref() else { continue };
            // Background tabs have no live search; only the current tab's
            // search_entry text matters for content_stack state.
            let text = if Some(i) == current_idx {
                imp.search_entry.text().to_lowercase()
            } else {
                String::new()
            };
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
        file_section.append(Some("Duplicate"), Some("win.duplicate"));
        file_section.append(Some("Rename"), Some("win.rename"));
        file_section.append(Some("Create Link"), Some("win.create-link"));
        file_section.append(Some("Add to Bookmarks"), Some("win.add-bookmark"));
        file_section.append(Some("Copy Path"), Some("win.copy-path"));
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
                            log_op("mkdir", &new_dir, None);
                            match new_dir
                                .make_directory_future(glib::Priority::DEFAULT)
                                .await
                            {
                                Ok(()) => {
                                    let imp = window.imp();
                                    imp.undo_stack.borrow_mut().push(
                                        undo::UndoOp::NewFolder { dir: new_dir },
                                    );
                                    imp.redo_stack.borrow_mut().clear();
                                    window.update_undo_actions();
                                    window.reload();
                                }
                                Err(e) => {
                                    log_err("mkdir", &new_dir, None, &e);
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
                            eprintln!(
                                "[wren] rename: {} -> {}",
                                fmt_path(&file),
                                new_name
                            );
                            match file
                                .set_display_name_future(&new_name, glib::Priority::DEFAULT)
                                .await
                            {
                                Ok(new_file) => {
                                    let imp = window.imp();
                                    imp.undo_stack.borrow_mut().push(
                                        undo::UndoOp::Rename {
                                            file: new_file,
                                            old_name,
                                            new_name,
                                        },
                                    );
                                    imp.redo_stack.borrow_mut().clear();
                                    window.update_undo_actions();
                                    window.reload();
                                }
                                Err(e) => {
                                    eprintln!(
                                        "[wren] rename failed: {}: {e}",
                                        fmt_path(&file)
                                    );
                                    window.show_toast(&format!("Could not rename: {e}"))
                                }
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
        let n = files.len();
        let name = files
            .first()
            .and_then(|f| f.basename())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let body = if n == 1 {
            format!("\"{name}\" will be moved to the Trash.")
        } else {
            format!("{n} items will be moved to the Trash.")
        };
        let confirm = adw::AlertDialog::new(Some("Move to Trash?"), Some(&body));
        confirm.add_response("cancel", "Cancel");
        confirm.add_response("trash", "Move to Trash");
        confirm.set_response_appearance("trash", adw::ResponseAppearance::Destructive);
        confirm.set_default_response(Some("cancel"));
        confirm.set_close_response("cancel");
        let files = std::rc::Rc::new(files);
        confirm.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, response| {
                    if response != "trash" {
                        return;
                    }
                    let files = (*files).clone();
                    glib::spawn_future_local(glib::clone!(
                        #[weak]
                        window,
                        async move {
                            window.do_trash_files(files).await;
                        }
                    ));
                }
            ),
        );
        confirm.present(Some(self));
    }

    /// Trash a specific list of files (used by sidebar drop on Trash).
    pub fn trash_files(&self, files: Vec<gio::File>) {
        if files.is_empty() {
            return;
        }
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = window)]
            self,
            async move {
                window.do_trash_files(files).await;
            }
        ));
    }

    async fn do_trash_files(&self, files: Vec<gio::File>) {
        let mut not_supported: Vec<gio::File> = Vec::new();
        for file in &files {
            log_op("trash", file, None);
            match file.trash_future(glib::Priority::DEFAULT).await {
                Ok(()) => {}
                Err(e) if e.matches(gio::IOErrorEnum::NotSupported) => {
                    not_supported.push(file.clone());
                }
                Err(e) => {
                    log_err("trash", file, None, &e);
                    self.show_toast(&format!("Could not trash: {e}"));
                }
            }
        }
        self.reload();

        if not_supported.is_empty() {
            return;
        }

        let count = not_supported.len();
        let body = if count == 1 {
            "This location does not support trash. Delete permanently instead?".to_string()
        } else {
            format!(
                "{count} items are on a location that does not support trash. \
                 Delete them permanently instead?"
            )
        };
        let dialog = adw::AlertDialog::new(Some("Cannot Move to Trash"), Some(&body));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("delete", "Delete Permanently");
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        dialog.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, response| {
                    if response != "delete" {
                        return;
                    }
                    let to_delete = not_supported.clone();
                    let total = to_delete.len();
                    let handle = window.op_start("Deleting");
                    glib::spawn_future_local(glib::clone!(
                        #[weak]
                        window,
                        #[strong]
                        handle,
                        async move {
                            for (idx, f) in to_delete.iter().enumerate() {
                                if handle.cancellable.is_cancelled() {
                                    break;
                                }
                                log_op("delete (trash unsupported)", f, None);
                                let name = f
                                    .basename()
                                    .map(|p| p.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                handle.set_item(&format!("{name} ({} of {total})", idx + 1));
                                handle.set_fraction(idx as f64 / total as f64);
                                handle.set_paths(f, None);
                                if let Err(e) = delete_recursive(f.clone(), &handle.cancellable).await {
                                    if !e.matches(gio::IOErrorEnum::Cancelled) {
                                        log_err("delete (trash unsupported)", f, None, &e);
                                        window.show_toast(&format!("Could not delete: {e}"));
                                    }
                                    break;
                                }
                                handle.set_fraction((idx + 1) as f64 / total as f64);
                            }
                            window.op_finish(&handle);
                            window.reload();
                        }
                    ));
                }
            ),
        );
        dialog.present(Some(self));
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
                    let total = files.len();
                    let handle = window.op_start("Deleting");
                    glib::spawn_future_local(glib::clone!(
                        #[weak]
                        window,
                        #[strong]
                        handle,
                        async move {
                            for (idx, file) in files.iter().enumerate() {
                                if handle.cancellable.is_cancelled() {
                                    break;
                                }
                                log_op("delete", file, None);
                                let name = file
                                    .basename()
                                    .map(|p| p.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                handle.set_item(&format!("{name} ({} of {total})", idx + 1));
                                handle.set_fraction(idx as f64 / total as f64);
                                handle.set_paths(file, None);
                                if let Err(e) = delete_recursive(file.clone(), &handle.cancellable).await {
                                    if !e.matches(gio::IOErrorEnum::Cancelled) {
                                        log_err("delete", file, None, &e);
                                        window.show_toast(&format!("Could not delete: {e}"));
                                    }
                                    break;
                                }
                                handle.set_fraction((idx + 1) as f64 / total as f64);
                            }
                            window.op_finish(&handle);
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
        let uris: Vec<String> = files.iter().map(|f| f.uri().to_string()).collect();
        self.imp()
            .clipboard_files
            .replace(Some((files, false)));
        self.clipboard().set_text(&uris.join("\r\n"));
        self.update_cut_indicator(&[]);
        self.update_selection_actions();
        self.show_toast("Copied");
    }

    pub fn cut_selection(&self) {
        let files = self.selected_files();
        if files.is_empty() {
            return;
        }
        let uris: Vec<String> = files.iter().map(|f| f.uri().to_string()).collect();
        self.imp()
            .clipboard_files
            .replace(Some((files, true)));
        self.clipboard().set_text(&uris.join("\r\n"));
        self.update_cut_indicator(&uris);
        self.update_selection_actions();
        self.show_toast("Cut");
    }

    fn update_cut_indicator(&self, uris: &[String]) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let tabs = self.imp().tabs.borrow();
        if let Some(tab) = tabs.get(idx) {
            tab.file_grid.set_cut_uris(uris);
            tab.file_list.set_cut_uris(uris);
        }
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
        let action_label = if is_cut { "Moving" } else { "Copying" };
        let total = files.len();
        let handle = self.op_start(action_label);
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[strong]
            handle,
            async move {
                let mut policy: Option<ConflictResolution> = None;
                for (idx, file) in files.iter().enumerate() {
                    if handle.cancellable.is_cancelled() {
                        break;
                    }
                    let Some(name) = file.basename() else {
                        continue;
                    };
                    // Cut + paste back into the source dir is a no-op.
                    if is_cut && dest_dir.child(&name).equal(file) {
                        continue;
                    }
                    let action = if is_cut { "move" } else { "copy" };
                    let display_name = name.to_string_lossy();
                    handle.set_item(&format!("{display_name} ({} of {total})", idx + 1));
                    handle.set_fraction(idx as f64 / total as f64);
                    handle.set_paths(file, Some(&dest_dir.child(&name)));

                    let dest_initial = dest_dir.child(&name);
                    let collides = !dest_initial.equal(file)
                        && dest_initial.query_exists(gio::Cancellable::NONE);
                    let resolution = if collides {
                        match policy {
                            Some(r) => r,
                            None => {
                                let (r, apply) = window.resolve_conflict(&display_name).await;
                                if apply {
                                    policy = Some(r);
                                }
                                r
                            }
                        }
                    } else {
                        ConflictResolution::Rename
                    };
                    let dest = match resolution {
                        ConflictResolution::Skip => continue,
                        ConflictResolution::Cancel => break,
                        ConflictResolution::Replace => {
                            if let Err(e) = delete_recursive(dest_initial.clone(), &handle.cancellable).await {
                                if !e.matches(gio::IOErrorEnum::Cancelled) {
                                    log_err("replace (delete existing)", &dest_initial, None, &e);
                                    window.show_toast(&format!("Could not replace: {e}"));
                                }
                                break;
                            }
                            dest_initial
                        }
                        ConflictResolution::Rename => unique_dest(&dest_dir, &name),
                    };
                    log_op(action, file, Some(&dest));

                    if let Err(e) = copy_recursive(file.clone(), dest.clone(), &handle.cancellable).await {
                        if !e.matches(gio::IOErrorEnum::Cancelled) {
                            log_err(action, file, Some(&dest), &e);
                            window.show_toast(&format!("Could not paste: {e}"));
                        }
                        break;
                    }
                    if is_cut {
                        log_op("delete (post-move)", file, None);
                        if let Err(e) = delete_recursive(file.clone(), &handle.cancellable).await {
                            if !e.matches(gio::IOErrorEnum::Cancelled) {
                                log_err("delete (post-move)", file, None, &e);
                                window.show_toast(&format!("Could not move: {e}"));
                            }
                            break;
                        }
                    }
                    handle.set_fraction((idx + 1) as f64 / total as f64);
                }
                if is_cut {
                    window.imp().clipboard_files.replace(None);
                    window.update_cut_indicator(&[]);
                    window.update_selection_actions();
                }
                window.op_finish(&handle);
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

        eprintln!(
            "[wren] symlink: {} -> {}",
            link_path.display(),
            target_path.display()
        );
        match std::os::unix::fs::symlink(&target_path, &link_path) {
            Ok(()) => self.reload(),
            Err(e) => {
                eprintln!(
                    "[wren] symlink failed: {} -> {}: {e}",
                    link_path.display(),
                    target_path.display()
                );
                self.show_toast(&format!("Could not create link: {e}"))
            }
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

    pub fn open_settings(&self) {
        let dialog = adw::PreferencesDialog::new();
        dialog.set_title("Settings");

        let page = adw::PreferencesPage::new();
        page.set_title("General");
        page.set_icon_name(Some("preferences-other-symbolic"));

        // Appearance group
        let appearance_group = adw::PreferencesGroup::new();
        appearance_group.set_title("Appearance");

        let scheme_row = adw::ActionRow::new();
        scheme_row.set_title("Color Scheme");

        let btn_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        btn_box.add_css_class("linked");
        btn_box.set_valign(gtk4::Align::Center);

        let auto_btn  = gtk4::ToggleButton::with_label("Auto");
        let light_btn = gtk4::ToggleButton::with_label("Light");
        let dark_btn  = gtk4::ToggleButton::with_label("Dark");
        light_btn.set_group(Some(&auto_btn));
        dark_btn.set_group(Some(&auto_btn));

        let current_scheme = self
            .application()
            .and_downcast::<WrenApplication>()
            .map(|a| a.color_scheme())
            .unwrap_or_default();
        match current_scheme.as_str() {
            "light" => light_btn.set_active(true),
            "dark"  => dark_btn.set_active(true),
            _       => auto_btn.set_active(true),
        }

        let connect_scheme = |btn: &gtk4::ToggleButton, scheme_str: &'static str, adw_scheme: adw::ColorScheme| {
            btn.connect_toggled(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |btn| {
                    if btn.is_active() {
                        adw::StyleManager::default().set_color_scheme(adw_scheme);
                        if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                            app.set_color_scheme_pref(scheme_str);
                        }
                    }
                }
            ));
        };
        connect_scheme(&auto_btn,  "default", adw::ColorScheme::Default);
        connect_scheme(&light_btn, "light",   adw::ColorScheme::ForceLight);
        connect_scheme(&dark_btn,  "dark",    adw::ColorScheme::ForceDark);

        btn_box.append(&auto_btn);
        btn_box.append(&light_btn);
        btn_box.append(&dark_btn);
        scheme_row.add_suffix(&btn_box);
        appearance_group.add(&scheme_row);
        page.add(&appearance_group);

        // Terminal group
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

        // Cache group
        let cache_group = adw::PreferencesGroup::new();
        cache_group.set_title("Cache");
        cache_group.set_description(Some("Thumbnail cache speeds up browsing by keeping scaled images in memory."));

        let cache_row = adw::ActionRow::new();
        cache_row.set_title("Thumbnail Cache");
        cache_row.set_subtitle("Free memory used by cached thumbnails");

        let clear_btn = gtk4::Button::with_label("Clear Cache");
        clear_btn.add_css_class("destructive-action");
        clear_btn.set_valign(gtk4::Align::Center);
        clear_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                crate::file_view::cell::clear_thumbnail_cache();
                window.show_toast("Thumbnail cache cleared");
            }
        ));

        cache_row.add_suffix(&clear_btn);
        cache_group.add(&cache_row);
        page.add(&cache_group);

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
        if !self.launch_terminal_at(&path) {
            self.show_toast("No terminal application found");
        }
    }

    pub fn open_terminal_at_uri(&self, uri: &str) {
        let file = gio::File::for_uri(uri);
        let Some(path) = file.path() else {
            self.show_toast("Cannot open terminal: not a local path");
            return;
        };
        if !self.launch_terminal_at(&path) {
            self.show_toast("No terminal application found");
        }
    }

    fn launch_terminal_at(&self, path: &std::path::Path) -> bool {
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
                    c.arg(path);
                    return c.spawn().is_ok();
                }
            }
            let mut c = std::process::Command::new(cmd);
            c.arg("--working-directory").arg(path);
            if c.spawn().is_ok() {
                return true;
            }
            std::process::Command::new(cmd).arg(path).spawn().is_ok()
        };

        let custom = self
            .application()
            .and_downcast::<WrenApplication>()
            .map(|a| a.terminal_cmd())
            .unwrap_or_default();
        if !custom.is_empty() && try_terminal(&custom) {
            return true;
        }

        for (cmd, wd_args) in known {
            let mut c = std::process::Command::new(cmd);
            for arg in *wd_args {
                c.arg(arg);
            }
            c.arg(path);
            if c.spawn().is_ok() {
                return true;
            }
        }
        false
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
                eprintln!(
                    "[wren] undo rename: {} -> {}",
                    fmt_path(&file),
                    old_name
                );
                glib::spawn_future_local(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    async move {
                        match file
                            .set_display_name_future(&old_name, glib::Priority::DEFAULT)
                            .await
                        {
                            Ok(restored_file) => {
                                // file is now back at old_name; redo would
                                // re-apply: rename old_name → new_name.
                                window.imp().redo_stack.borrow_mut().push(
                                    undo::UndoOp::Rename {
                                        file: restored_file,
                                        old_name,
                                        new_name,
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
                log_op("undo mkdir (trash)", &dir, None);
                glib::spawn_future_local(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    async move {
                        match dir.trash_future(glib::Priority::DEFAULT).await {
                            Ok(()) => {
                                window.imp().redo_stack.borrow_mut().push(
                                    undo::UndoOp::NewFolder { dir },
                                );
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
                old_name,
                new_name,
            } => {
                eprintln!(
                    "[wren] redo rename: {} -> {}",
                    fmt_path(&file),
                    new_name
                );
                glib::spawn_future_local(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    async move {
                        match file
                            .set_display_name_future(&new_name, glib::Priority::DEFAULT)
                            .await
                        {
                            Ok(restored_file) => {
                                // file is now at new_name again; undo would
                                // revert to old_name.
                                window.imp().undo_stack.borrow_mut().push(
                                    undo::UndoOp::Rename {
                                        file: restored_file,
                                        old_name,
                                        new_name,
                                    },
                                );
                                window.update_undo_actions();
                                window.reload();
                            }
                            Err(e) => window.show_toast(&format!("Redo failed: {e}")),
                        }
                    }
                ));
            }
            undo::UndoOp::NewFolder { dir } => {
                log_op("redo mkdir", &dir, None);
                glib::spawn_future_local(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    async move {
                        match dir
                            .make_directory_future(glib::Priority::DEFAULT)
                            .await
                        {
                            Ok(()) => {
                                window.imp().undo_stack.borrow_mut().push(
                                    undo::UndoOp::NewFolder { dir },
                                );
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
            "win.duplicate",
            "win.batch-rename",
        ] {
            self.action_set_enabled(action, has_selection);
        }
        let has_clipboard = self.imp().clipboard_files.borrow().is_some();
        self.action_set_enabled("win.paste", has_clipboard);
        self.update_status_bar();
    }

    fn update_status_bar(&self) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let (n_total, n_selected, label) = {
            let tabs = self.imp().tabs.borrow();
            let Some(tab) = tabs.get(idx) else { return };
            let Some(model) = tab.dir_model.as_ref() else { return };
            let n_total = model.selection.n_items();
            let n_selected = model.selection.selection().size() as u32;
            (n_total, n_selected, tab.status_bar.clone())
        };
        let text = if n_selected == 0 {
            format!("{n_total} item{}", if n_total == 1 { "" } else { "s" })
        } else {
            format!(
                "{n_total} item{}, {n_selected} selected",
                if n_total == 1 { "" } else { "s" }
            )
        };
        label.set_text(&text);
    }

    pub fn update_undo_actions(&self) {
        let imp = self.imp();
        self.action_set_enabled("win.undo", !imp.undo_stack.borrow().is_empty());
        self.action_set_enabled("win.redo", !imp.redo_stack.borrow().is_empty());
    }

    pub fn show_toast(&self, message: &str) {
        self.imp().toast_overlay.add_toast(adw::Toast::new(message));
    }

    // ── File-operation progress + cancel ─────────────────────────────────────
    //
    // Each long-running op calls `op_start(title)` which returns an OpHandle
    // wrapping a Cancellable plus widgets in the header-bar popover (title
    // label, current-item label, ProgressBar, per-op Cancel button). Callers
    // update via methods on the handle and call `op_finish` when done.

    pub fn op_start(&self, title: &str) -> OpHandle {
        let imp = self.imp();
        let handle = OpHandle::build(title);
        imp.op_popover_box.append(&handle.row);
        imp.op_handles.borrow_mut().push(handle.clone());
        imp.op_button.set_visible(true);
        handle
    }

    pub fn op_finish(&self, handle: &OpHandle) {
        let imp = self.imp();
        imp.op_popover_box.remove(&handle.row);
        let mut active = imp.op_handles.borrow_mut();
        active.retain(|h| h.cancellable != handle.cancellable);
        if active.is_empty() {
            imp.op_button.set_visible(false);
            if let Some(p) = imp.op_button.popover() {
                p.popdown();
            }
        }
    }

    /// Ask the user how to resolve a name collision. Returns the resolution
    /// plus whether to apply it to subsequent collisions in the same batch.
    async fn resolve_conflict(&self, name: &str) -> (ConflictResolution, bool) {
        let dialog = adw::AlertDialog::new(
            Some("File already exists"),
            Some(&format!("\"{name}\" already exists in the destination.")),
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("skip", "Skip");
        dialog.add_response("rename", "Rename");
        dialog.add_response("replace", "Replace");
        dialog.set_response_appearance("replace", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("rename"));
        dialog.set_close_response("cancel");

        let apply_to_all = gtk4::CheckButton::with_label("Apply to all conflicts in this operation");
        dialog.set_extra_child(Some(&apply_to_all));

        let response = dialog.choose_future(Some(self)).await;
        let apply = apply_to_all.is_active();
        let res = match response.as_str() {
            "skip" => ConflictResolution::Skip,
            "replace" => ConflictResolution::Replace,
            "rename" => ConflictResolution::Rename,
            _ => ConflictResolution::Cancel,
        };
        (res, apply)
    }

    // ── About ────────────────────────────────────────────────────────────────

    pub fn show_about(&self) {
        let dialog = adw::AboutDialog::builder()
            .application_name("Wren")
            .version(env!("CARGO_PKG_VERSION"))
            .application_icon("system-file-manager")
            .developer_name("Wren contributors")
            .website("https://github.com/Gren-95/wren")
            .issue_url("https://github.com/Gren-95/wren/issues")
            .license_type(gtk4::License::MitX11)
            .build();
        dialog.present(Some(self));
    }

    // ── Duplicate ────────────────────────────────────────────────────────────

    pub fn duplicate(&self) {
        let current_dir = {
            let Some(idx) = self.current_tab_index() else { return };
            let tabs = self.imp().tabs.borrow();
            tabs.get(idx).and_then(|t| t.navigation.current().cloned())
        };
        let Some(dest_dir) = current_dir else { return };
        let Some(dest_dir_path) = dest_dir.path() else {
            self.show_toast("Cannot duplicate: current directory is not local");
            return;
        };

        let files = self.selected_files();
        if files.is_empty() { return; }

        for file in files {
            let Some(src_path) = file.path() else { continue };
            let name = src_path.file_name().unwrap_or_default();
            let stem = std::path::Path::new(name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("copy");
            let ext = std::path::Path::new(name)
                .extension()
                .and_then(|s| s.to_str());
            let make_name = |suffix: &str| match ext {
                Some(e) => format!("{stem}{suffix}.{e}"),
                None => format!("{stem}{suffix}"),
            };
            let dest_path = {
                let first = dest_dir_path.join(make_name(" (copy)"));
                if !first.exists() {
                    first
                } else {
                    (2u32..)
                        .find_map(|i| {
                            let p = dest_dir_path.join(make_name(&format!(" (copy {i})")));
                            (!p.exists()).then_some(p)
                        })
                        .expect("will eventually find a free name")
                }
            };
            let dest_file = gio::File::for_path(&dest_path);
            let display_name = file
                .basename()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let handle = self.op_start("Duplicating");
            handle.set_item(&display_name);
            handle.set_paths(&file, Some(&dest_file));
            glib::spawn_future_local(glib::clone!(
                #[weak(rename_to = window)]
                self,
                #[strong]
                handle,
                async move {
                    log_op("duplicate", &file, Some(&dest_file));
                    match copy_recursive(file.clone(), dest_file.clone(), &handle.cancellable).await {
                        Ok(()) => {
                            handle.set_fraction(1.0);
                            window.op_finish(&handle);
                            window.reload();
                        }
                        Err(e) => {
                            window.op_finish(&handle);
                            if !e.matches(gio::IOErrorEnum::Cancelled) {
                                log_err("duplicate", &file, Some(&dest_file), &e);
                                window.show_toast(&format!("Could not duplicate: {e}"));
                            }
                        }
                    }
                }
            ));
        }
    }

    pub fn drop_files(&self, files: Vec<gio::File>, dest: Option<gio::File>, is_move: bool) {
        let dest_dir = if let Some(d) = dest {
            d
        } else {
            let Some(idx) = self.current_tab_index() else { return };
            let tabs = self.imp().tabs.borrow();
            match tabs.get(idx).and_then(|t| t.navigation.current().cloned()) {
                Some(d) => d,
                None => return,
            }
        };
        let action_label = if is_move { "Moving" } else { "Copying" };
        let total = files.len();
        let handle = self.op_start(action_label);
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[strong]
            handle,
            async move {
                let mut policy: Option<ConflictResolution> = None;
                for (idx, file) in files.iter().enumerate() {
                    if handle.cancellable.is_cancelled() {
                        break;
                    }
                    let Some(name) = file.basename() else { continue };
                    // Dropping a file onto its own parent dir is a no-op.
                    if dest_dir.child(&name).equal(file) { continue; }
                    let action = if is_move { "drop-move" } else { "drop-copy" };
                    let display_name = name.to_string_lossy();
                    handle.set_item(&format!("{display_name} ({} of {total})", idx + 1));
                    handle.set_fraction(idx as f64 / total as f64);
                    handle.set_paths(file, Some(&dest_dir.child(&name)));

                    let dest_initial = dest_dir.child(&name);
                    let collides = !dest_initial.equal(file)
                        && dest_initial.query_exists(gio::Cancellable::NONE);
                    let resolution = if collides {
                        match policy {
                            Some(r) => r,
                            None => {
                                let (r, apply) = window.resolve_conflict(&display_name).await;
                                if apply {
                                    policy = Some(r);
                                }
                                r
                            }
                        }
                    } else {
                        ConflictResolution::Rename
                    };
                    let dest = match resolution {
                        ConflictResolution::Skip => continue,
                        ConflictResolution::Cancel => break,
                        ConflictResolution::Replace => {
                            if let Err(e) = delete_recursive(dest_initial.clone(), &handle.cancellable).await {
                                if !e.matches(gio::IOErrorEnum::Cancelled) {
                                    log_err("replace (delete existing)", &dest_initial, None, &e);
                                    window.show_toast(&format!("Could not replace: {e}"));
                                }
                                break;
                            }
                            dest_initial
                        }
                        ConflictResolution::Rename => unique_dest(&dest_dir, &name),
                    };
                    log_op(action, file, Some(&dest));

                    if let Err(e) = copy_recursive(file.clone(), dest.clone(), &handle.cancellable).await {
                        if !e.matches(gio::IOErrorEnum::Cancelled) {
                            log_err(action, file, Some(&dest), &e);
                            window.show_toast(&format!("Could not copy: {e}"));
                        }
                        break;
                    }
                    if is_move {
                        log_op("delete (post-move)", file, None);
                        if let Err(e) = delete_recursive(file.clone(), &handle.cancellable).await {
                            if !e.matches(gio::IOErrorEnum::Cancelled) {
                                log_err("delete (post-move)", file, None, &e);
                                window.show_toast(&format!("Could not remove source: {e}"));
                            }
                            break;
                        }
                    }
                    handle.set_fraction((idx + 1) as f64 / total as f64);
                }
                window.op_finish(&handle);
                window.reload();
            }
        ));
    }

    pub fn new_window(&self) {
        if let Some(app) = self.application().and_downcast::<WrenApplication>() {
            WrenWindow::new(&app).present();
        }
    }

    pub fn copy_path(&self) {
        let path = self
            .selected_files()
            .into_iter()
            .next()
            .and_then(|f| f.path())
            .map(|p| p.to_string_lossy().into_owned())
            .or_else(|| {
                let idx = self.current_tab_index()?;
                let tabs = self.imp().tabs.borrow();
                tabs.get(idx)?
                    .navigation
                    .current()?
                    .path()
                    .map(|p| p.to_string_lossy().into_owned())
            });
        if let Some(path) = path {
            self.clipboard().set_text(&path);
            self.show_toast("Path copied to clipboard");
        }
    }

    pub fn show_shortcuts(&self) {
        let dialog = adw::PreferencesDialog::new();
        dialog.set_title("Keyboard Shortcuts");

        let make_page = |title: &str, icon: &str, shortcuts: &[(&str, &str)]| {
            let page = adw::PreferencesPage::new();
            page.set_title(title);
            page.set_icon_name(Some(icon));
            let group = adw::PreferencesGroup::new();
            for (key, desc) in shortcuts {
                let row = adw::ActionRow::new();
                row.set_title(desc);
                let lbl = gtk4::Label::new(Some(key));
                lbl.add_css_class("dim-label");
                lbl.add_css_class("caption");
                lbl.add_css_class("wren-kbd");
                lbl.set_valign(gtk4::Align::Center);
                row.add_suffix(&lbl);
                group.add(&row);
            }
            page.add(&group);
            page
        };

        dialog.add(&make_page("Navigation", "go-next-symbolic", &[
            ("Alt + ←",          "Go Back"),
            ("Alt + →",          "Go Forward"),
            ("Alt + ↑",          "Go Up"),
            ("Alt + Home",       "Go to Home Folder"),
            ("Ctrl + L",         "Focus Path Bar"),
            ("Ctrl + T",         "New Tab"),
            ("Ctrl + W",         "Close Tab"),
            ("Ctrl + N",         "New Window"),
        ]));

        dialog.add(&make_page("View", "view-grid-symbolic", &[
            ("Ctrl + F",         "Search"),
            ("Ctrl + H",         "Show Hidden Files"),
            ("Ctrl + =",         "Zoom In"),
            ("Ctrl + -",         "Zoom Out"),
            ("Ctrl + 0",         "Reset Zoom"),
            ("F5",               "Reload"),
            ("Ctrl + ?",         "Keyboard Shortcuts"),
        ]));

        dialog.add(&make_page("File Operations", "document-edit-symbolic", &[
            ("Ctrl + C",         "Copy"),
            ("Ctrl + X",         "Cut"),
            ("Ctrl + V",         "Paste"),
            ("Ctrl + A",         "Select All"),
            ("F2",               "Rename"),
            ("Delete",           "Move to Trash"),
            ("Shift + Delete",   "Delete Permanently"),
            ("Ctrl + Shift + N", "New Folder"),
            ("Ctrl + D",         "Add Bookmark"),
            ("Ctrl + Shift + R", "Batch Rename"),
            ("Alt + Enter",      "Properties"),
            ("Ctrl + Z",         "Undo"),
            ("Ctrl + Shift + Z", "Redo"),
            ("Ctrl + Shift + T", "Open in Terminal"),
        ]));

        dialog.present(Some(self));
    }

    // ── Window size persistence ───────────────────────────────────────────────

    pub fn setup_volume_monitor(&self) {
        let monitor = gio::VolumeMonitor::get();
        monitor.connect_mount_added(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| {
                win.imp().sidebar.reload_volumes();
            }
        ));
        monitor.connect_mount_removed(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| {
                win.imp().sidebar.reload_volumes();
            }
        ));
    }

    pub fn remove_bookmark(&self, uri: &str) {
        let bookmarks_path = {
            let mut p = glib::home_dir();
            p.push(".config");
            p.push("gtk-3.0");
            p.push("bookmarks");
            p
        };
        let content = std::fs::read_to_string(&bookmarks_path).unwrap_or_default();
        let new_content: String = content
            .lines()
            .filter(|line| line.split_whitespace().next() != Some(uri))
            .flat_map(|line| [line, "\n"])
            .collect();
        if let Err(e) = std::fs::write(&bookmarks_path, &new_content) {
            self.show_toast(&format!("Could not remove bookmark: {e}"));
            return;
        }
        self.show_toast("Bookmark removed");
        self.imp().sidebar.reload_bookmarks();
    }

    pub fn save_window_size(&self) {
        let (w, h) = self.default_size();
        if let Some(app) = self.application().and_downcast::<WrenApplication>() {
            app.set_window_size(w, h);
        }
    }
}

// ── Iterative file operation helpers ────────────────────────────────────────
//
// Iterative (non-recursive) implementations avoid the Box::pin overhead and
// potential stack issues with deeply-nested directory trees.

// ── Operation logging ─────────────────────────────────────────────────────
// Stderr output for every destructive file action so users running from a
// terminal can see exactly what is being done.

fn fmt_path(f: &gio::File) -> String {
    f.path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| f.uri().to_string())
}

fn log_op(action: &str, src: &gio::File, dest: Option<&gio::File>) {
    match dest {
        Some(d) => eprintln!("[wren] {action}: {} -> {}", fmt_path(src), fmt_path(d)),
        None => eprintln!("[wren] {action}: {}", fmt_path(src)),
    }
}

fn log_err(action: &str, src: &gio::File, dest: Option<&gio::File>, err: &impl std::fmt::Display) {
    match dest {
        Some(d) => eprintln!("[wren] {action} failed: {} -> {}: {err}", fmt_path(src), fmt_path(d)),
        None => eprintln!("[wren] {action} failed: {}: {err}", fmt_path(src)),
    }
}

// Returns a non-colliding child path under dest_dir. If `dest_dir/name` is
// free, returns that. Otherwise appends " (Copy)" / " (Copy 2)" / ... to the
// stem until an unused name is found.
fn unique_dest(dest_dir: &gio::File, name: &std::path::Path) -> gio::File {
    let dest = dest_dir.child(name);
    if !dest.query_exists(gio::Cancellable::NONE) {
        return dest;
    }
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
    if !candidate.query_exists(gio::Cancellable::NONE) {
        return candidate;
    }
    let mut i = 2u32;
    loop {
        let c = dest_dir.child(&format!("{} (Copy {}){}", stem, i, ext));
        if !c.query_exists(gio::Cancellable::NONE) {
            return c;
        }
        i += 1;
    }
}

fn cancelled_err() -> glib::Error {
    glib::Error::new(gio::IOErrorEnum::Cancelled, "Operation cancelled")
}

#[derive(Clone, Copy, Debug)]
pub enum ConflictResolution {
    Skip,
    Replace,
    Rename,
    Cancel,
}

/// A handle to one in-flight file operation. Holds its Cancellable and the
/// widgets that display its progress in the header-bar popover. Cloning is
/// cheap — every field is a reference-counted GObject (or a Cancellable, which
/// is also a GObject).
#[derive(Clone, Debug)]
pub struct OpHandle {
    pub cancellable: gio::Cancellable,
    pub row: gtk4::Box,
    /// Expander whose label is the current item summary; expanding it reveals
    /// the verbose source / destination paths.
    expander: gtk4::Expander,
    src_label: gtk4::Label,
    dest_label: gtk4::Label,
    progress: gtk4::ProgressBar,
}

impl OpHandle {
    fn build(title: &str) -> Self {
        let cancellable = gio::Cancellable::new();
        let row = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        row.set_width_request(360);
        row.set_margin_top(4);
        row.set_margin_bottom(4);

        let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        let title_label = gtk4::Label::new(Some(title));
        title_label.set_xalign(0.0);
        title_label.set_hexpand(true);
        title_label.add_css_class("heading");
        let cancel_btn = gtk4::Button::from_icon_name("window-close-symbolic");
        cancel_btn.set_tooltip_text(Some("Cancel"));
        cancel_btn.add_css_class("flat");
        cancel_btn.add_css_class("circular");
        let c_clone = cancellable.clone();
        cancel_btn.connect_clicked(move |_| c_clone.cancel());
        header.append(&title_label);
        header.append(&cancel_btn);

        let progress = gtk4::ProgressBar::new();
        progress.set_show_text(true);
        progress.set_text(Some("0%"));

        // Expander: label is a brief item summary ("foo.txt (3 of 17)");
        // expanding it shows the full source and destination paths.
        let expander = gtk4::Expander::new(Some(""));
        expander.set_resize_toplevel(false);
        let details_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        details_box.set_margin_top(4);
        details_box.set_margin_start(18);

        let src_label = gtk4::Label::new(None);
        src_label.set_xalign(0.0);
        src_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        src_label.set_max_width_chars(48);
        src_label.add_css_class("caption");
        src_label.add_css_class("dim-label");
        src_label.set_selectable(true);

        let dest_label = gtk4::Label::new(None);
        dest_label.set_xalign(0.0);
        dest_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        dest_label.set_max_width_chars(48);
        dest_label.add_css_class("caption");
        dest_label.add_css_class("dim-label");
        dest_label.set_selectable(true);

        details_box.append(&src_label);
        details_box.append(&dest_label);
        expander.set_child(Some(&details_box));

        row.append(&header);
        row.append(&progress);
        row.append(&expander);

        Self {
            cancellable,
            row,
            expander,
            src_label,
            dest_label,
            progress,
        }
    }

    /// Set the brief item summary shown next to the disclosure arrow.
    pub fn set_item(&self, msg: &str) {
        self.expander.set_label(Some(msg));
    }

    /// Set the verbose source / destination paths shown when the row is
    /// expanded. Pass `None` for `dest` on operations that don't have one
    /// (e.g. delete).
    pub fn set_paths(&self, src: &gio::File, dest: Option<&gio::File>) {
        self.src_label.set_text(&format!("From: {}", fmt_path(src)));
        match dest {
            Some(d) => {
                self.dest_label.set_text(&format!("To: {}", fmt_path(d)));
                self.dest_label.set_visible(true);
            }
            None => {
                self.dest_label.set_text("");
                self.dest_label.set_visible(false);
            }
        }
    }

    /// Set the progress bar fraction (0.0 — 1.0) and its overlay text to a %.
    pub fn set_fraction(&self, fraction: f64) {
        let f = fraction.clamp(0.0, 1.0);
        self.progress.set_fraction(f);
        self.progress.set_text(Some(&format!("{}%", (f * 100.0).round() as u32)));
    }
}

async fn copy_recursive(
    src: gio::File,
    dest: gio::File,
    cancellable: &gio::Cancellable,
) -> Result<(), glib::Error> {
    if cancellable.is_cancelled() {
        return Err(cancelled_err());
    }

    // Check whether the top-level source is a directory.
    let src_info = src
        .query_info_future(
            "standard::type",
            gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
            glib::Priority::DEFAULT,
        )
        .await?;

    if src_info.file_type() != gio::FileType::Directory {
        let (fut, _) = src.copy_future(
            &dest,
            gio::FileCopyFlags::NOFOLLOW_SYMLINKS,
            glib::Priority::DEFAULT,
        );
        return fut.await;
    }

    // BFS queue of (src_dir, dest_dir) pairs.
    dest.make_directory_future(glib::Priority::DEFAULT).await?;
    let mut queue = std::collections::VecDeque::new();
    queue.push_back((src, dest));

    while let Some((src_dir, dest_dir)) = queue.pop_front() {
        if cancellable.is_cancelled() {
            return Err(cancelled_err());
        }
        let enumerator = src_dir
            .enumerate_children_future(
                "standard::name,standard::type",
                gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                glib::Priority::DEFAULT,
            )
            .await?;

        loop {
            if cancellable.is_cancelled() {
                return Err(cancelled_err());
            }
            let batch = enumerator
                .next_files_future(30, glib::Priority::DEFAULT)
                .await?;
            if batch.is_empty() {
                break;
            }
            for child_info in batch {
                if cancellable.is_cancelled() {
                    return Err(cancelled_err());
                }
                let name = child_info.name();
                let child_src = src_dir.child(&name);
                let child_dest = dest_dir.child(&name);
                if child_info.file_type() == gio::FileType::Directory {
                    child_dest.make_directory_future(glib::Priority::DEFAULT).await?;
                    queue.push_back((child_src, child_dest));
                } else {
                    let (fut, _) = child_src.copy_future(
                        &child_dest,
                        gio::FileCopyFlags::NOFOLLOW_SYMLINKS,
                        glib::Priority::DEFAULT,
                    );
                    fut.await?;
                }
            }
        }
    }
    Ok(())
}

async fn delete_recursive(
    file: gio::File,
    cancellable: &gio::Cancellable,
) -> Result<(), glib::Error> {
    if cancellable.is_cancelled() {
        return Err(cancelled_err());
    }
    let info = file
        .query_info_future(
            "standard::type",
            gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
            glib::Priority::DEFAULT,
        )
        .await?;

    if info.file_type() != gio::FileType::Directory {
        return file.delete_future(glib::Priority::DEFAULT).await;
    }

    // DFS: collect directories in traversal order, delete all files immediately.
    // Directories are deleted in reverse traversal order (deepest first).
    let mut dirs: Vec<gio::File> = Vec::new();
    let mut stack = vec![file];

    while let Some(dir) = stack.pop() {
        if cancellable.is_cancelled() {
            return Err(cancelled_err());
        }
        let enumerator = dir
            .enumerate_children_future(
                "standard::name,standard::type",
                gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                glib::Priority::DEFAULT,
            )
            .await?;

        dirs.push(dir.clone());

        loop {
            if cancellable.is_cancelled() {
                return Err(cancelled_err());
            }
            let batch = enumerator
                .next_files_future(30, glib::Priority::DEFAULT)
                .await?;
            if batch.is_empty() {
                break;
            }
            for child_info in batch {
                if cancellable.is_cancelled() {
                    return Err(cancelled_err());
                }
                let child = dir.child(child_info.name());
                if child_info.file_type() == gio::FileType::Directory {
                    stack.push(child);
                } else {
                    child.delete_future(glib::Priority::DEFAULT).await?;
                }
            }
        }
    }

    // Delete directories deepest-first.
    for dir in dirs.into_iter().rev() {
        if cancellable.is_cancelled() {
            return Err(cancelled_err());
        }
        dir.delete_future(glib::Priority::DEFAULT).await?;
    }
    Ok(())
}

/// Recursively walk a directory, accumulating total bytes and item count.
/// Calls `on_update` periodically so a UI label can show live progress.
/// Honours the cancellable; returns the partial total if cancelled.
async fn compute_dir_size(
    root: gio::File,
    cancellable: &gio::Cancellable,
    on_update: impl Fn(u64, u64),
) -> (u64, u64) {
    let mut total: u64 = 0;
    let mut count: u64 = 0;
    let mut stack = vec![root];
    let mut last_emit = std::time::Instant::now();
    let emit_every = std::time::Duration::from_millis(120);

    while let Some(dir) = stack.pop() {
        if cancellable.is_cancelled() {
            return (total, count);
        }
        let enumerator = match dir
            .enumerate_children_future(
                "standard::name,standard::type,standard::size",
                gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                glib::Priority::DEFAULT,
            )
            .await
        {
            Ok(e) => e,
            Err(_) => continue, // permission denied etc — skip this subdir
        };
        loop {
            if cancellable.is_cancelled() {
                return (total, count);
            }
            let batch = match enumerator
                .next_files_future(50, glib::Priority::DEFAULT)
                .await
            {
                Ok(b) => b,
                Err(_) => break,
            };
            if batch.is_empty() {
                break;
            }
            for info in batch {
                count += 1;
                let size = info.size().max(0) as u64;
                total += size;
                if info.file_type() == gio::FileType::Directory {
                    stack.push(dir.child(info.name()));
                }
                if last_emit.elapsed() >= emit_every {
                    on_update(total, count);
                    last_emit = std::time::Instant::now();
                }
            }
        }
    }
    (total, count)
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
