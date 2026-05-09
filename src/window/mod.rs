mod file_ops;
mod imp;
mod operations;
mod properties;
pub mod tab;
mod trash;
pub mod undo;

use adw::prelude::*;
use adw::subclass::prelude::*;
use glib::Object;

use crate::application::WrenApplication;
use crate::model::{DirectoryModel, FileObject, SortKey};
use crate::window::tab::TabState;
pub use file_ops::{OpHandle, OpKind};
use file_ops::{fmt_path, format_duration, log_err, log_op};

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
        // Restore window state (size + maximized + sidebar) before presenting.
        if app.window_maximized() {
            win.maximize();
        }
        win.imp().split_view.set_show_sidebar(app.sidebar_visible());
        win.connect_close_request(|w| {
            w.save_window_size();
            if let Some(app) = w.application().and_downcast::<WrenApplication>() {
                app.set_window_maximized(w.is_maximized());
                app.set_sidebar_visible(w.imp().split_view.shows_sidebar());
                // Snapshot every open tab's current URI so the next
                // launch can rebuild the same set. last_directory is
                // kept up-to-date as a fallback for older configs.
                let imp = w.imp();
                let tabs = imp.tabs.borrow();
                let uris: Vec<String> = tabs
                    .iter()
                    .filter_map(|t| t.navigation.current().map(|f| f.uri().to_string()))
                    .collect();
                let active = w.current_tab_index().unwrap_or(0) as i32;
                drop(tabs);
                if !uris.is_empty() {
                    app.set_last_directory(&uris[active.max(0) as usize]);
                }
                app.set_last_tabs(uris, active);
            }
            glib::Propagation::Proceed
        });
        win
    }

    fn imp(&self) -> &imp::WrenWindow {
        imp::WrenWindow::from_obj(self)
    }

    // ── Tab helpers ──────────────────────────────────────────────────────────

    /// Set which tab is the active (visible) one. Used at startup to
    /// restore the tab that was selected in the previous session.
    pub fn activate_tab_at(&self, idx: usize) {
        let imp = self.imp();
        let tabs = imp.tabs.borrow();
        let Some(tab) = tabs.get(idx) else { return };
        let widget = tab.content_widget.clone();
        drop(tabs);
        if let Some(page) = imp.tab_view.page(&widget).into() {
            imp.tab_view.set_selected_page(&page);
        }
    }

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

        // Middle-click and Ctrl+left-click on a directory open it in a
        // new tab; on a file they fall through to the default opener
        // (so Ctrl+click on a .pdf still launches the viewer).
        let new_tab_handler = glib::clone!(
            #[weak(rename_to = window)] self,
            move |obj: &FileObject| {
                if obj.is_directory() {
                    window.add_tab(obj.file().clone());
                } else {
                    let uri = obj.file().uri();
                    if let Err(e) = gio::AppInfo::launch_default_for_uri(
                        uri.as_str(),
                        gio::AppLaunchContext::NONE,
                    ) {
                        window.show_toast(&format!("Cannot open: {e}"));
                    }
                }
            }
        );
        tab.file_grid.connect_open_in_tab(new_tab_handler.clone());
        tab.file_list.connect_open_in_tab(new_tab_handler);

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
        // close_page triggers connect_close_page which removes the TabState
        // and cancels its monitor / dir_model, so we don't have to here.
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
                        window.track_recent_location(&location);
                    }
                    Err(e) => {
                        // Match the success-path guard: if this load was
                        // superseded by a newer one (or its tab was closed
                        // and the Vec shifted), don't paint the error onto
                        // whatever tab now sits at this index.
                        let is_current = {
                            let tabs = window.imp().tabs.borrow();
                            tabs.get(tab_idx)
                                .map_or(false, |t| t.load_gen.get() == load_gen)
                        };
                        if !is_current {
                            return;
                        }
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
                let watched = location.clone();
                monitor.connect_changed(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    move |_, _, _, event| {
                        use gio::FileMonitorEvent as E;
                        if matches!(
                            event,
                            E::Created | E::Deleted | E::Renamed | E::MovedIn | E::MovedOut
                        ) {
                            // Reload the tab(s) that actually watch this
                            // directory — NOT the foreground tab. External
                            // changes (terminal, other apps) to a background
                            // tab's folder must update that tab, not whatever
                            // the user happens to be looking at.
                            window.reload_tabs_at(&watched);
                        }
                    }
                ));
                let tabs = self.imp().tabs.borrow();
                if let Some(tab) = tabs.get(tab_idx) {
                    *tab.dir_monitor.borrow_mut() = Some(monitor);
                }
            }
            Err(e) => crate::wren_log!("Cannot watch directory: {e}"),
        }
    }

    /// Reload every tab whose current location equals `location`. Used by
    /// the per-tab file monitor so external changes refresh the right tab,
    /// and so multiple tabs viewing the same folder stay in sync.
    pub fn reload_tabs_at(&self, location: &gio::File) {
        let to_reload: Vec<usize> = {
            let tabs = self.imp().tabs.borrow();
            tabs.iter()
                .enumerate()
                .filter_map(|(idx, tab)| {
                    if tab
                        .navigation
                        .current()
                        .map_or(false, |c| c.equal(location))
                    {
                        Some(idx)
                    } else {
                        None
                    }
                })
                .collect()
        };
        for idx in to_reload {
            self.load_location_for_tab(idx, location.clone());
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
        file_section.append(Some("Copy Location"), Some("win.copy-path"));
        file_section.append(Some("Move to Trash"), Some("win.move-to-trash"));
        menu.append_section(None, &file_section);

        let trash_section = gio::Menu::new();
        trash_section.append(Some("Restore From Trash"), Some("win.restore-from-trash"));
        trash_section.append(Some("Empty Trash"), Some("win.empty-trash"));
        menu.append_section(None, &trash_section);

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

        // Try anchoring a popover-based inline rename to the cell.
        // Falls back to a centered AlertDialog if the cell isn't
        // realised (selection scrolled off-screen, or the file is on
        // a different tab).
        let Some(idx) = self.current_tab_index() else { return };
        let tabs = self.imp().tabs.borrow();
        let Some(tab) = tabs.get(idx) else { return };
        let anchor: Option<gtk4::Widget> = tab
            .file_grid
            .cell_for_file(&file)
            .map(|c| c.upcast::<gtk4::Widget>())
            .or_else(|| {
                tab.file_list
                    .row_for_file(&file)
                    .map(|r| r.upcast::<gtk4::Widget>())
            });
        drop(tabs);

        if let Some(anchor) = anchor {
            self.rename_selection_inline(file, current_name, &anchor);
        } else {
            self.rename_selection_dialog(file, current_name);
        }
    }

    /// Pop a small Popover with a text entry directly over the file's
    /// cell. Enter commits, Escape cancels, focus-loss cancels.
    fn rename_selection_inline(
        &self,
        file: gio::File,
        current_name: String,
        anchor: &gtk4::Widget,
    ) {
        let popover = gtk4::Popover::new();
        popover.set_autohide(true);
        // No `.menu` class — that strips the default padding and the
        // entry ends up flush against the rounded corners.

        let entry = gtk4::Entry::new();
        entry.set_text(&current_name);
        // Pre-select just the stem so the typical "fix the name, keep
        // the extension" workflow doesn't require a manual selection.
        let stem_len = std::path::Path::new(&current_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.chars().count() as i32)
            .unwrap_or(-1);
        entry.select_region(0, stem_len);
        entry.set_width_chars(20);

        popover.set_child(Some(&entry));
        popover.set_parent(anchor);

        let committed = std::rc::Rc::new(std::cell::Cell::new(false));

        let do_commit = glib::clone!(
            #[weak(rename_to = window)] self,
            #[weak] entry,
            #[weak] popover,
            #[strong] committed,
            #[strong] current_name,
            #[strong] file,
            move || {
                if committed.get() { return };
                committed.set(true);
                let new_name = entry.text().to_string();
                popover.popdown();
                if new_name.is_empty() || new_name == current_name {
                    return;
                }
                window.spawn_rename(file.clone(), current_name.clone(), new_name);
            }
        );

        entry.connect_activate(glib::clone!(
            #[strong] do_commit,
            move |_| do_commit()
        ));

        // Escape inside the entry → close popover without commit.
        let key_ctrl = gtk4::EventControllerKey::new();
        key_ctrl.connect_key_pressed(glib::clone!(
            #[weak] popover,
            #[strong] committed,
            #[upgrade_or] glib::Propagation::Proceed,
            move |_, key, _, _| {
                if key == gtk4::gdk::Key::Escape {
                    committed.set(true); // suppress focus-loss commit
                    popover.popdown();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        entry.add_controller(key_ctrl);

        // Closing the popover via tap-outside / Escape unparents it.
        popover.connect_closed(move |p| p.unparent());

        popover.popup();
        entry.grab_focus();
        // grab_focus selects all by default for a freshly-shown Entry;
        // override with our stem-only selection.
        entry.select_region(0, stem_len);
    }

    /// Fallback rename UI used when the file's cell isn't visible.
    fn rename_selection_dialog(&self, file: gio::File, current_name: String) {
        let dialog = adw::AlertDialog::new(Some("Rename"), None::<&str>);
        let entry = gtk4::Entry::new();
        entry.set_text(&current_name);
        let stem_len = std::path::Path::new(&current_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.chars().count() as i32)
            .unwrap_or(-1);
        entry.select_region(0, stem_len);
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
                #[weak(rename_to = window)] self,
                #[weak] entry,
                move |_, response| {
                    if response != "rename" { return };
                    let new_name = entry.text().to_string();
                    if new_name.is_empty() || new_name == current_name { return };
                    window.spawn_rename(file.clone(), current_name.clone(), new_name);
                }
            ),
        );
        dialog.present(Some(self));
    }

    /// Shared back-end for both the inline and dialog rename paths.
    /// Issues the GIO rename, pushes onto the undo stack on success,
    /// and surfaces errors via toast.
    fn spawn_rename(&self, file: gio::File, old_name: String, new_name: String) {
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = window)] self,
            async move {
                crate::wren_log!("rename: {} -> {}", fmt_path(&file), new_name);
                match file
                    .set_display_name_future(&new_name, glib::Priority::DEFAULT)
                    .await
                {
                    Ok(new_file) => {
                        let imp = window.imp();
                        imp.undo_stack.borrow_mut().push(undo::UndoOp::Rename {
                            file: new_file,
                            old_name,
                            new_name,
                        });
                        imp.redo_stack.borrow_mut().clear();
                        window.update_undo_actions();
                        window.reload();
                    }
                    Err(e) => {
                        crate::wren_log!("rename failed: {}: {e}", fmt_path(&file));
                        window.show_toast(&format!("Could not rename: {e}"));
                    }
                }
            }
        ));
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
                            // Collect (old_name, error_message) for every
                            // failure so the user can see which files
                            // didn't rename and why, rather than just a
                            // bare "N could not be renamed" toast.
                            let mut errors: Vec<(String, String)> = Vec::new();
                            let mut renamed = 0usize;
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
                                match file
                                    .set_display_name_future(&new_name, glib::Priority::DEFAULT)
                                    .await
                                {
                                    Ok(_) => renamed += 1,
                                    Err(e) => errors.push((old_name, e.to_string())),
                                }
                            }
                            window.reload();
                            if errors.is_empty() {
                                if renamed > 0 {
                                    window.show_toast(&format!("Renamed {renamed} file(s)"));
                                }
                            } else {
                                window.show_batch_rename_errors(renamed, errors);
                            }
                        }
                    ));
                }
            ),
        );
        dialog.present(Some(self));
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

        // Distinguish "file doesn't exist" (treat as empty) from a real I/O
        // error (don't proceed — we'd otherwise wipe every bookmark by
        // overwriting the file with just the new entry).
        let content = match std::fs::read_to_string(&bookmarks_path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                self.show_toast(&format!("Could not read bookmarks: {e}"));
                return;
            }
        };
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
        // Fall back to the current directory when nothing is selected so
        // right-clicking the empty area of the file view opens a picker
        // for "this folder" — useful for opening a project dir in an
        // editor, the current folder in a terminal, etc.
        let objs = self.selected_file_objects();
        let (files, content_type) = if let Some(obj) = objs.first() {
            // Folders use the synthetic "inode/directory" type; that's a
            // real mimetype with apps registered against it (file managers,
            // archive tools, editors that accept directories, …).
            let ct = if obj.is_directory() {
                "inode/directory".to_string()
            } else {
                obj.content_type()
            };
            let files: Vec<gio::File> = objs.iter().map(|o| o.file().clone()).collect();
            (files, ct)
        } else {
            let Some(idx) = self.current_tab_index() else { return };
            let tabs = self.imp().tabs.borrow();
            let Some(loc) = tabs.get(idx).and_then(|t| t.navigation.current().cloned()) else {
                return;
            };
            drop(tabs);
            (vec![loc], "inode/directory".to_string())
        };
        if content_type.is_empty() {
            self.show_toast("Unknown file type");
            return;
        }

        let apps = gio::AppInfo::all_for_type(&content_type);
        if apps.is_empty() {
            self.show_toast("No applications available for this file type");
            return;
        }
        let default = gio::AppInfo::default_for_type(&content_type, false);

        // Build the picker manually — GtkAppChooserDialog has been
        // deprecated since GTK 4.10 and is non-functional on 4.18+.
        let body = match files.len() {
            1 => format!(
                "Choose an application to open “{}”",
                files[0]
                    .basename()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| files[0].uri().to_string())
            ),
            n => format!("Choose an application to open the {n} files"),
        };
        let dialog = adw::AlertDialog::new(Some("Open With"), Some(&body));

        let list = gtk4::ListBox::new();
        list.add_css_class("boxed-list");
        list.set_selection_mode(gtk4::SelectionMode::Single);

        let mut default_idx: Option<i32> = None;
        for (i, app) in apps.iter().enumerate() {
            let row = adw::ActionRow::new();
            row.set_title(app.display_name().as_str());
            if let Some(desc) = app.description() {
                row.set_subtitle(desc.as_str());
            }
            if let Some(icon) = app.icon() {
                let img = gtk4::Image::from_gicon(&icon);
                img.set_pixel_size(32);
                row.add_prefix(&img);
            }
            row.set_activatable(true);
            list.append(&row);
            if let Some(d) = &default {
                if app.id() == d.id() {
                    default_idx = Some(i as i32);
                }
            }
        }
        if let Some(idx) = default_idx {
            if let Some(row) = list.row_at_index(idx) {
                list.select_row(Some(&row));
            }
        } else if let Some(row) = list.row_at_index(0) {
            list.select_row(Some(&row));
        }

        let scroll = gtk4::ScrolledWindow::new();
        scroll.set_min_content_height(280);
        scroll.set_max_content_height(420);
        scroll.set_propagate_natural_height(true);
        scroll.set_child(Some(&list));
        dialog.set_extra_child(Some(&scroll));

        dialog.add_response("cancel", "Cancel");
        dialog.add_response("open", "Open");
        dialog.set_response_appearance("open", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("open"));
        dialog.set_close_response("cancel");

        // The launcher takes a row index and runs the open. Shared by the
        // "Open" button (via connect_response) and double-click / Enter on
        // a row. AdwAlertDialog has no programmatic-response API, so the
        // row-activation path closes the dialog itself.
        let launch = {
            let apps = apps.clone();
            let files = files.clone();
            glib::clone!(
                #[weak(rename_to = window)] self,
                #[upgrade_or] (),
                move |idx: usize| {
                    let Some(app) = apps.get(idx) else { return };
                    // Terminal apps (Terminal=true in the .desktop file) need
                    // to be run inside a terminal emulator. glib's built-in
                    // launcher only knows a hardcoded list of terminals
                    // (gnome-terminal, xterm, …) so user-configured ones
                    // like kitty / alacritty / wezterm fail with "Unable to
                    // find terminal required for application". Detect this
                    // case and fall back to spawning through the terminal
                    // we already use for "Open in Terminal".
                    if window.app_needs_terminal(app) {
                        if window.launch_terminal_app(app, &files) {
                            return;
                        }
                        window.show_toast(&format!(
                            "Could not launch {}: no terminal configured",
                            app.display_name()
                        ));
                        return;
                    }
                    let uris: Vec<_> = files.iter().map(|f| f.uri()).collect();
                    let uri_strs: Vec<&str> = uris.iter().map(|u| u.as_str()).collect();
                    if let Err(e) = app.launch_uris(&uri_strs, gio::AppLaunchContext::NONE) {
                        window.show_toast(&format!("Cannot open: {e}"));
                    }
                }
            )
        };

        list.connect_row_activated(glib::clone!(
            #[weak] dialog,
            #[strong] launch,
            move |_, row| {
                launch(row.index() as usize);
                dialog.close();
            }
        ));

        dialog.connect_response(
            None,
            glib::clone!(
                #[weak] list,
                #[strong] launch,
                move |_, response| {
                    if response != "open" { return };
                    let Some(row) = list.selected_row() else { return };
                    launch(row.index() as usize);
                }
            ),
        );

        dialog.present(Some(self));
    }

    pub fn focus_location(&self) {
        self.imp().breadcrumb_bar.enter_edit_mode();
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

        // Animations toggle. Drives gtk-enable-animations app-wide:
        // sidebar slide, popover fade, banner reveal, etc.
        let anim_row = adw::SwitchRow::new();
        anim_row.set_title("Animations");
        anim_row.set_subtitle("Sidebar slide, popover fade, banner reveal");
        let initial_anim = self
            .application()
            .and_downcast::<WrenApplication>()
            .map(|a| a.animations_enabled())
            .unwrap_or(true);
        anim_row.set_active(initial_anim);
        anim_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_animations_enabled(row.is_active());
                }
            }
        ));
        appearance_group.add(&anim_row);
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

        // Advanced group
        let advanced_group = adw::PreferencesGroup::new();
        advanced_group.set_title("Advanced");

        let log_row = adw::SwitchRow::new();
        log_row.set_title("Debug logging");
        log_row.set_subtitle("Print every action and file operation to stderr");
        let initial_log = self
            .application()
            .and_downcast::<WrenApplication>()
            .map(|a| a.debug_logging())
            .unwrap_or(false);
        log_row.set_active(initial_log);
        log_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_debug_logging(row.is_active());
                }
            }
        ));
        advanced_group.add(&log_row);
        page.add(&advanced_group);

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

    /// True if `app`'s .desktop file declares `Terminal=true`. Reads the
    /// .desktop directly because gio-rs doesn't expose
    /// gio::DesktopAppInfo::needs_terminal.
    fn app_needs_terminal(&self, app: &gio::AppInfo) -> bool {
        let Some(id) = app.id() else { return false };
        let Some(path) = locate_desktop_file(&id) else { return false };
        let kf = glib::KeyFile::new();
        if kf.load_from_file(&path, glib::KeyFileFlags::NONE).is_err() {
            return false;
        }
        kf.boolean("Desktop Entry", "Terminal").unwrap_or(false)
    }

    /// Run a Terminal=true application by wrapping the .desktop Exec line
    /// in the user's preferred terminal. Returns false only when no
    /// terminal binary can be located.
    fn launch_terminal_app(&self, app: &gio::AppInfo, files: &[gio::File]) -> bool {
        // Strip Exec= placeholders (%f, %F, %u, %U, %i, %c, %k, %%) per
        // the FreeDesktop spec — we'll append actual paths ourselves.
        let raw_exec = app
            .commandline()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        if raw_exec.is_empty() { return false; }
        let exec = raw_exec
            .split_whitespace()
            .filter(|tok| !tok.starts_with('%'))
            .collect::<Vec<_>>()
            .join(" ");
        let mut command = exec;
        for f in files {
            let arg = f
                .path()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| f.uri().to_string());
            command.push(' ');
            command.push_str(&shell_escape(&arg));
        }

        // Mirror launch_terminal_at's terminal selection: prefer the
        // user's configured terminal_cmd, fall back to a known list.
        let custom = self
            .application()
            .and_downcast::<WrenApplication>()
            .map(|a| a.terminal_cmd())
            .unwrap_or_default();
        let candidates: Vec<&str> = if !custom.is_empty() {
            vec![custom.as_str()]
        } else {
            vec!["kitty", "alacritty", "wezterm", "kgx", "gnome-terminal", "konsole", "xterm"]
        };

        for term in candidates {
            // gnome-terminal needs `--`, most others use `-e`. Try -e
            // first since it's the broadly-supported flag, then `--`.
            for sep in ["-e", "--"] {
                if std::process::Command::new(term)
                    .arg(sep)
                    .arg("sh")
                    .arg("-c")
                    .arg(&command)
                    .spawn()
                    .is_ok()
                {
                    return true;
                }
            }
        }
        false
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
                crate::wren_log!(
                    "undo rename: {} -> {}",
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
            undo::UndoOp::Trash { originals } => {
                glib::spawn_future_local(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    async move {
                        match window.restore_from_trash_by_orig(&originals).await {
                            Ok(()) => {
                                window.imp().redo_stack.borrow_mut().push(
                                    undo::UndoOp::Trash { originals },
                                );
                                window.update_undo_actions();
                                window.reload();
                            }
                            Err(msg) => window.show_toast(&format!("Undo failed: {msg}")),
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
                crate::wren_log!(
                    "redo rename: {} -> {}",
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
            undo::UndoOp::Trash { originals } => {
                glib::spawn_future_local(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    async move {
                        for f in &originals {
                            log_op("redo trash", f, None);
                            let _ = f.trash_future(glib::Priority::DEFAULT).await;
                        }
                        window
                            .imp()
                            .undo_stack
                            .borrow_mut()
                            .push(undo::UndoOp::Trash { originals });
                        window.update_undo_actions();
                        window.reload();
                    }
                ));
            }
        }
    }

    // ── Action sensitivity ───────────────────────────────────────────────────

    pub fn update_selection_actions(&self) {
        let has_selection = !self.selected_files().is_empty();
        let in_trash = self.current_location_is_trash();
        // open / open-with / delete-permanently / move-to-trash all
        // apply inside trash. move-to-trash is rerouted to
        // delete_permanently() in the handler when in_trash, so the
        // Delete key naturally becomes "purge selected" — matching
        // Nautilus.
        self.action_set_enabled("win.open-selection", has_selection);
        // Open With falls back to the current directory when no file is
        // selected, so it stays available on the empty-area context menu.
        self.action_set_enabled("win.open-with", true);
        self.action_set_enabled("win.delete-permanently", has_selection);
        self.action_set_enabled("win.move-to-trash", has_selection);
        for action in &[
            "win.rename",
            "win.cut",
            "win.copy",
            "win.create-link",
            "win.duplicate",
            "win.batch-rename",
        ] {
            self.action_set_enabled(action, has_selection && !in_trash);
        }
        // Folder-mutating actions: also disabled when viewing trash.
        self.action_set_enabled("win.new-folder", !in_trash);
        let has_clipboard = self.imp().clipboard_files.borrow().is_some();
        self.action_set_enabled("win.paste", has_clipboard && !in_trash);
        // Restore needs in-trash AND a selection; Empty Trash is always
        // available — it lives in the hamburger menu and the sidebar Trash
        // row, with its own confirmation dialog before doing anything.
        self.action_set_enabled("win.restore-from-trash", in_trash && has_selection);
        self.action_set_enabled("win.empty-trash", true);
        self.update_status_bar();
    }

    fn update_status_bar(&self) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let (n_total, n_selected, selected_bytes, label) = {
            let tabs = self.imp().tabs.borrow();
            let Some(tab) = tabs.get(idx) else { return };
            let Some(model) = tab.dir_model.as_ref() else { return };
            let n_total = model.selection.n_items();
            let bitset = model.selection.selection();
            let n_selected = bitset.size() as u32;
            // Sum the byte size of selected non-directories so the user
            // can see "you've got 4 GB selected before you trash it".
            // Skip directories — recursive size walk is too expensive
            // here; properties dialog still computes those on demand.
            let mut bytes = 0u64;
            for i in 0..n_selected {
                let pos = bitset.nth(i);
                if let Some(obj) = model.selection.item(pos).and_downcast::<FileObject>() {
                    if !obj.is_directory() {
                        bytes = bytes.saturating_add(obj.file_size());
                    }
                }
            }
            (n_total, n_selected, bytes, tab.status_bar.clone())
        };
        let text = if n_selected == 0 {
            format!("{n_total} item{}", if n_total == 1 { "" } else { "s" })
        } else if selected_bytes > 0 {
            format!(
                "{n_total} item{}, {n_selected} selected ({})",
                if n_total == 1 { "" } else { "s" },
                file_ops::format_file_size(selected_bytes),
            )
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

    /// Per-file failure dialog used by batch rename. A toast can only
    /// say "N failed" — this dialog lists each (file, reason) so the
    /// user can act on the actual failures.
    pub fn show_batch_rename_errors(&self, renamed: usize, errors: Vec<(String, String)>) {
        let body = if renamed == 0 {
            format!("{} file(s) could not be renamed.", errors.len())
        } else {
            format!(
                "Renamed {renamed} file(s); {} could not be renamed.",
                errors.len()
            )
        };
        let dialog = adw::AlertDialog::new(Some("Batch Rename"), Some(&body));
        let list = gtk4::ScrolledWindow::new();
        list.set_min_content_height(160);
        list.set_max_content_height(320);
        list.set_propagate_natural_height(true);
        let lb = gtk4::ListBox::new();
        lb.add_css_class("boxed-list");
        for (name, err) in &errors {
            let row = adw::ActionRow::new();
            row.set_title(&glib::markup_escape_text(name));
            row.set_subtitle(&glib::markup_escape_text(err));
            lb.append(&row);
        }
        list.set_child(Some(&lb));
        dialog.set_extra_child(Some(&list));
        dialog.add_response("ok", "Close");
        dialog.set_default_response(Some("ok"));
        dialog.set_close_response("ok");
        dialog.present(Some(self));
    }

    /// Toast variant with an "Undo" button wired to `win.undo`.
    /// Used by move-to-trash so a single trashed item can be reversed
    /// without opening the trash view.
    ///
    /// Coalesces with previous undo toasts: the prior one is dismissed
    /// before the new one is shown, so the visible Undo button always
    /// refers to the latest trash op (matches the LIFO undo stack).
    pub fn show_undo_toast(&self, message: &str) {
        let imp = self.imp();
        if let Some(prev) = imp.active_undo_toast.borrow_mut().take() {
            prev.dismiss();
        }
        let toast = adw::Toast::new(message);
        toast.set_button_label(Some("Undo"));
        toast.set_action_name(Some("win.undo"));
        toast.connect_dismissed(glib::clone!(
            #[weak(rename_to = window)] self,
            move |t| {
                let imp = window.imp();
                let mut slot = imp.active_undo_toast.borrow_mut();
                if slot.as_ref().is_some_and(|cur| cur == t) {
                    *slot = None;
                }
            }
        ));
        imp.active_undo_toast.replace(Some(toast.clone()));
        imp.toast_overlay.add_toast(toast);
    }

    // ── File-operation progress + cancel ─────────────────────────────────────
    //
    // Each long-running op calls `op_start(title)` which returns an OpHandle
    // wrapping a Cancellable plus widgets in the header-bar popover (title
    // label, current-item label, ProgressBar, per-op Cancel button). Callers
    // update via methods on the handle and call `op_finish` when done.

    pub fn op_start(&self, kind: OpKind) -> OpHandle {
        let imp = self.imp();
        let handle = OpHandle::build(kind);
        imp.op_popover_box.append(&handle.row);
        imp.op_handles.borrow_mut().push(handle.clone());
        imp.op_button.set_visible(true);
        handle
    }

    pub fn op_finish(&self, handle: &OpHandle) {
        let imp = self.imp();
        // System notification for long, successful ops so the user knows
        // they can come back to the app — only worth it if the op fully
        // completed (mark_succeeded was called), wasn't cancelled, and ran
        // for at least 30 s of wall time. Errored ops would otherwise
        // contradict their toast with a "Copy complete" notification.
        let (elapsed, succeeded) = {
            let s = handle.state.borrow();
            (s.start.elapsed(), s.succeeded)
        };
        let was_cancelled = handle.cancellable.is_cancelled();
        if succeeded
            && !was_cancelled
            && elapsed >= std::time::Duration::from_secs(30)
        {
            if let Some(app) = self.application() {
                let notif = gio::Notification::new(handle.kind.done_title());
                notif.set_body(Some(&format!(
                    "Finished in {}",
                    format_duration(elapsed.as_secs())
                )));
                app.send_notification(Some("wren-op-done"), &notif);
            }
        }

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

    pub fn show_about(&self) {
        let dialog = adw::AboutDialog::builder()
            .application_name("Wren")
            .version(env!("CARGO_PKG_VERSION"))
            .application_icon("system-file-manager")
            .developer_name("Wren contributors")
            .website("https://github.com/Gren-95/wren")
            .issue_url("https://github.com/Gren-95/wren/issues")
            .license_type(gtk4::License::Gpl30)
            .build();
        dialog.present(Some(self));
    }

    // ── Duplicate ────────────────────────────────────────────────────────────

    pub fn new_window(&self) {
        if let Some(app) = self.application().and_downcast::<WrenApplication>() {
            WrenWindow::new(&app).present();
        }
    }

    /// Open a new top-level window already navigated to `uri`. Used by
    /// "Open in New Window" in the sidebar context menu.
    pub fn open_window_at(&self, uri: &str) {
        let Some(app) = self.application().and_downcast::<WrenApplication>() else {
            return;
        };
        let win = WrenWindow::new(&app);
        win.present();
        win.navigate_to(gio::File::for_uri(uri));
    }

    /// Copy a sidebar place's URI / local path to the clipboard. For local
    /// places, copies the path; for virtual ones (trash:///, recent:///,
    /// sftp://…), copies the URI.
    pub fn copy_path_at(&self, uri: &str) {
        let file = gio::File::for_uri(uri);
        let text = file
            .path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| uri.to_string());
        self.clipboard().set_text(&text);
        self.show_toast("Location copied");
    }

    pub fn copy_path(&self) {
        // Selected file's path → falling back to its URI for non-local
        // files (trash:///, sftp://, etc.) → falling back to the current
        // directory's path/URI when nothing is selected. Previously
        // skipped step 2 and silently copied the parent dir's path
        // when run on a trash entry.
        let location = |f: &gio::File| {
            f.path()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| f.uri().to_string())
        };
        let text = self
            .selected_files()
            .into_iter()
            .next()
            .map(|f| location(&f))
            .or_else(|| {
                let idx = self.current_tab_index()?;
                let tabs = self.imp().tabs.borrow();
                tabs.get(idx)?.navigation.current().map(location)
            });
        if let Some(text) = text {
            self.clipboard().set_text(&text);
            self.show_toast("Location copied to clipboard");
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
            ("Ctrl + Shift + O", "Open With…"),
            ("Ctrl + Shift + C", "Copy Location"),
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

    /// Push a successfully-navigated directory onto the Recents MRU list and
    /// refresh the sidebar if it changed. Only `file://` locations are
    /// tracked — virtual roots (`trash:///`, `recent:///`, search results,
    /// remote mounts without a local path) wouldn't round-trip cleanly
    /// through the URI list and aren't useful as quick-access entries.
    pub fn track_recent_location(&self, location: &gio::File) {
        if location.path().is_none() || location.uri_scheme().as_deref() != Some("file") {
            return;
        }
        let uri = location.uri().to_string();
        let Some(app) = self.application().and_downcast::<WrenApplication>() else {
            return;
        };
        if app.push_recent_uri(&uri) {
            self.imp().sidebar.reload_recents();
        }
    }

    pub fn remove_bookmark(&self, uri: &str) {
        let bookmarks_path = {
            let mut p = glib::home_dir();
            p.push(".config");
            p.push("gtk-3.0");
            p.push("bookmarks");
            p
        };
        // If we can't read the file, do NOT proceed — overwriting on a
        // transient I/O error would wipe every other bookmark too.
        let content = match std::fs::read_to_string(&bookmarks_path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                self.show_toast(&format!("Could not read bookmarks: {e}"));
                return;
            }
        };
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

// Look up a .desktop file by id (e.g. "ranger.desktop") in the
// standard XDG application directories. Returns the first match,
// matching gio's own resolution order.
fn locate_desktop_file(id: &str) -> Option<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    let user = glib::user_data_dir();
    dirs.push(user.join("applications"));
    for sys in glib::system_data_dirs() {
        dirs.push(sys.join("applications"));
    }
    for d in dirs {
        let candidate = d.join(id);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

// Single-quote a string for safe interpolation into a shell command.
// Used to wrap file paths when we shell out to a terminal emulator
// running `sh -c "<exec> <arg>…"`. Embedded single quotes are
// closed-out via `'\''`.
fn shell_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}
