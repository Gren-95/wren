mod disk_usage;
mod file_ops;
mod imp;
mod operations;
mod properties;
pub mod tab;
mod templates;
mod trash;
pub mod undo;

use adw::prelude::*;
use adw::subclass::prelude::*;
use glib::Object;

use crate::application::{TabPref, WrenApplication};
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
                // Snapshot every open tab's current URI plus its sort/view
                // so the next launch can fully rebuild the session.
                // last_directory is kept up-to-date as a fallback for older
                // configs that read only [General].
                let imp = w.imp();
                let tabs = imp.tabs.borrow();
                let prefs: Vec<TabPref> = tabs
                    .iter()
                    .filter_map(|t| {
                        let uri = t.navigation.current().map(|f| f.uri().to_string())?;
                        let view = t
                            .view_stack
                            .visible_child_name()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "grid".to_string());
                        Some(TabPref {
                            uri,
                            sort_key: t.sort_key.as_str().to_string(),
                            reversed: t.sort_reversed,
                            view_mode: view,
                        })
                    })
                    .collect();
                let active = w.current_tab_index().unwrap_or(0) as i32;
                drop(tabs);
                if !prefs.is_empty() {
                    let i = active.max(0) as usize;
                    if let Some(p) = prefs.get(i) {
                        app.set_last_directory(&p.uri);
                    }
                }
                app.set_tab_states(prefs, active);
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
        self.add_tab_with_state(location, None);
    }

    /// Add a tab and restore explicit per-tab state (sort key, direction,
    /// view mode). When `pref` is `None`, the global app prefs are used —
    /// the path taken for "New Tab" and the legacy single-tab fallback.
    pub fn add_tab_with_state(&self, location: gio::File, pref: Option<&TabPref>) {
        let imp = self.imp();
        let mut tab = TabState::new();

        tab.file_grid.connect_item_activated(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |file_obj| {
                if file_obj.is_directory() {
                    window.navigate_to(window.target_for_activation(file_obj));
                } else {
                    window.activate_file_object(file_obj);
                }
            }
        ));
        tab.file_list.connect_item_activated(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |file_obj| {
                if file_obj.is_directory() {
                    window.navigate_to(window.target_for_activation(file_obj));
                } else {
                    window.activate_file_object(file_obj);
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
                    window.add_tab(window.target_for_activation(obj));
                } else {
                    window.activate_file_object(obj);
                }
            }
        );
        tab.file_grid.connect_open_in_tab(new_tab_handler.clone());
        tab.file_list.connect_open_in_tab(new_tab_handler);

        let builder_grid = glib::clone!(
            #[weak(rename_to = window)] self,
            #[upgrade_or_else] || gio::Menu::new().upcast::<gio::MenuModel>(),
            move || window.context_menu_model()
        );
        let builder_list = glib::clone!(
            #[weak(rename_to = window)] self,
            #[upgrade_or_else] || gio::Menu::new().upcast::<gio::MenuModel>(),
            move || window.context_menu_model()
        );
        tab.file_grid.setup_context_menu(builder_grid);
        tab.file_list.setup_context_menu(builder_list);
        tab.file_grid.setup_drop_target();
        tab.file_list.setup_drop_target();
        tab.file_grid.setup_empty_area_click();
        tab.file_list.setup_empty_area_click();
        tab.file_grid.set_show_extensions(imp.show_extensions.get());
        tab.file_list.set_show_extensions(imp.show_extensions.get());
        tab.file_list.set_show_hidden(imp.show_hidden.get());

        // Restore per-tab state when supplied (session restore), otherwise
        // fall back to the global app prefs (used for new tabs and legacy).
        let app = self.application().and_downcast::<WrenApplication>();
        let (mode, sort_key_str, reversed) = if let Some(p) = pref {
            (p.view_mode.clone(), p.sort_key.clone(), p.reversed)
        } else if let Some(a) = &app {
            (a.view_mode(), a.sort_key(), a.sort_reversed())
        } else {
            ("grid".to_string(), "name".to_string(), false)
        };
        tab.view_stack.set_visible_child_name(&mode);
        let sort_key = crate::model::SortKey::from_str(&sort_key_str);
        tab.sort_key = sort_key;
        tab.sort_reversed = reversed;
        tab.file_list.set_sort_state(sort_key.as_str(), reversed);

        let page = imp.tab_view.append(&tab.content_widget);
        page.set_title("Home");

        imp.tabs.borrow_mut().push(tab);
        imp.tab_view.set_selected_page(&page);

        self.navigate_to(location);
    }

    pub fn new_tab(&self) {
        let app = self.application().and_downcast::<WrenApplication>();
        if let Some(app) = app.as_ref() {
            if app.new_tab_use_defaults() {
                let path = app.new_tab_default_path();
                let location = if path.is_empty() {
                    gio::File::for_path(glib::home_dir())
                } else if path.contains("://") {
                    gio::File::for_uri(&path)
                } else {
                    gio::File::for_path(&path)
                };
                let pref = TabPref {
                    uri: location.uri().to_string(),
                    sort_key: app.new_tab_default_sort_key(),
                    reversed: app.new_tab_default_sort_reversed(),
                    view_mode: app.new_tab_default_view(),
                };
                let zoom = app.new_tab_default_zoom().clamp(1, 5);
                self.imp().zoom_level.set(zoom);
                self.imp().zoom_adjustment.set_value(zoom as f64);
                self.add_tab_with_state(location, Some(&pref));
                return;
            }
        }
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
                .map(|loc| self.title_for_location(loc))
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

        // Refresh the banner so it reflects the newly-active tab's
        // current directory writability.
        let pair = {
            let tabs = self.imp().tabs.borrow();
            tabs.get(idx)
                .and_then(|t| t.navigation.current().cloned().map(|l| (l, t.load_gen.get())))
        };
        if let Some((loc, load_gen)) = pair {
            self.refresh_readonly_banner(idx, load_gen, loc);
        } else {
            self.hide_banner();
        }
    }

    /// Render `location` as a window-title string. When the user has
    /// "Show full path in title" enabled this returns the absolute path
    /// (with `$HOME` collapsed to `~`) for `file://` URIs and the raw
    /// URI for non-local locations; otherwise the basename only.
    fn title_for_location(&self, location: &gio::File) -> String {
        let full = self
            .application()
            .and_downcast::<WrenApplication>()
            .map_or(false, |a| a.show_full_path_in_title());
        if full {
            if let Some(path) = location.path() {
                let s = path.to_string_lossy().into_owned();
                let home = glib::home_dir();
                let home_s = home.to_string_lossy();
                if !home_s.is_empty() && s == home_s.as_ref() {
                    return "~".to_string();
                }
                if !home_s.is_empty() {
                    let prefix = format!("{}/", home_s);
                    if let Some(rest) = s.strip_prefix(&prefix) {
                        return format!("~/{rest}");
                    }
                }
                return s;
            }
            return location.uri().to_string();
        }
        location
            .basename()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Files".to_string())
    }

    /// Re-apply `title_for_location` to the active tab. Used when the
    /// "Show full path in title" pref toggles so the change is visible
    /// immediately without re-navigating.
    pub fn refresh_title(&self) {
        let Some(idx) = self.current_tab_index() else { return };
        let loc = {
            let tabs = self.imp().tabs.borrow();
            tabs.get(idx).and_then(|t| t.navigation.current().cloned())
        };
        if let Some(loc) = loc {
            self.set_title(Some(&self.title_for_location(&loc)));
        }
    }

    // ── Navigation ───────────────────────────────────────────────────────────

    /// Resolve the navigation target for an activated entry. For symlinked
    /// directories we re-anchor onto the current tab's location so the
    /// breadcrumb shows the path the user clicked through (e.g. `~/projects`)
    /// rather than the symlink's resolved target (`/mnt/data/projects`).
    pub fn target_for_activation(&self, file_obj: &FileObject) -> gio::File {
        if file_obj.is_symlink() && file_obj.is_directory() {
            let parent = self.current_tab_index().and_then(|idx| {
                self.imp()
                    .tabs
                    .borrow()
                    .get(idx)
                    .and_then(|t| t.navigation.current().cloned())
            });
            if let Some(parent) = parent {
                return parent.child(file_obj.file_info().name());
            }
        }
        file_obj.file().clone()
    }

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
                let tab_title = location
                    .basename()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Files".to_string());
                page.set_title(&tab_title);
                self.set_title(Some(&self.title_for_location(&location)));
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
                        window.refresh_readonly_banner(tab_idx, load_gen, location.clone());
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
                        window.hide_banner();
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

    /// Force every tab's list view to rebind its visible rows. Used by the
    /// folder-count policy change handler so the new policy takes effect
    /// without requiring a scroll or reload.
    pub fn refresh_visible_list_rows(&self) {
        let tabs = self.imp().tabs.borrow();
        for tab in tabs.iter() {
            tab.file_list.rebind_visible_rows();
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
            // Drop any cached folder counts under this directory so that
            // external changes (files added/removed in a child folder)
            // surface on the next bind. Cheaper to clear everything than
            // to walk the cache for prefix matches.
            crate::file_view::row::clear_folder_count_cache();
            // Likewise drop the cached free/total for this filesystem —
            // a reload usually follows a write op (paste, delete, new
            // folder), so the next status-bar paint should show the
            // post-op numbers, not the 5-second-old ones.
            disk_usage::invalidate_for(&loc);
            self.load_location_for_tab(idx, loc);
        }
    }

    /// Reload every open tab. Used when a global setting flips the way
    /// cells render (currently only thumbnail policy).
    pub fn reload_all_tabs(&self) {
        let targets: Vec<(usize, gio::File)> = {
            let tabs = self.imp().tabs.borrow();
            tabs.iter()
                .enumerate()
                .filter_map(|(idx, t)| t.navigation.current().cloned().map(|f| (idx, f)))
                .collect()
        };
        for (idx, loc) in targets {
            self.load_location_for_tab(idx, loc);
        }
    }

    /// Re-run the sort on every tab's directory model. Used when a global
    /// sort preference (e.g. folders-first) changes — the comparator picks
    /// up the new value automatically; we just need to invalidate cached
    /// orderings.
    pub fn refresh_all_sorts(&self) {
        let tabs = self.imp().tabs.borrow();
        for tab in tabs.iter() {
            if let Some(model) = tab.dir_model.as_ref() {
                model.refresh_sort();
            }
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

    /// Push the current column-visibility prefs to every open tab's
    /// list view. The view replaces its row factory so visible rows
    /// rebind and pick up the new visibility flags.
    pub fn refresh_list_columns(&self) {
        let tabs = self.imp().tabs.borrow();
        for tab in tabs.iter() {
            tab.file_list.refresh_columns();
        }
    }

    pub fn apply_hidden_filter(&self) {
        let imp = self.imp();
        let show_hidden = imp.show_hidden.get();
        let current_idx = self.current_tab_index();
        // Folder counts include/exclude hidden children based on the same flag,
        // so any cached counts are now stale.
        crate::file_view::row::clear_folder_count_cache();
        let tabs = imp.tabs.borrow();
        for (i, tab) in tabs.iter().enumerate() {
            tab.file_list.set_show_hidden(show_hidden);
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
        // Wrap ~/Templates entries in a submenu so a populated Templates
        // dir doesn't blow up the file_section's vertical extent. List
        // is rebuilt every right-click (see setup_context_menu) so
        // adding/removing templates while wren is open Just Works.
        let templates = templates::list_templates();
        if !templates.is_empty() {
            let templates_menu = gio::Menu::new();
            for (name, _path) in &templates {
                let label = std::path::Path::new(name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(name);
                let item = gio::MenuItem::new(Some(label), None);
                item.set_action_and_target_value(
                    Some("win.new-from-template"),
                    Some(&name.to_variant()),
                );
                templates_menu.append_item(&item);
            }
            file_section.append_submenu(Some("New Document"), &templates_menu);
        }
        let app = self.application().and_downcast::<WrenApplication>();
        if app.as_ref().map_or(true, |a| a.show_duplicate()) {
            file_section.append(Some("Duplicate"), Some("win.duplicate"));
        }
        file_section.append(Some("Rename"), Some("win.rename"));
        if app.as_ref().map_or(true, |a| a.show_create_link()) {
            file_section.append(Some("Create Link"), Some("win.create-link"));
        }
        if app.as_ref().map_or(true, |a| a.show_add_bookmark()) {
            file_section.append(Some("Add to Bookmarks"), Some("win.add-bookmark"));
        }
        if app.as_ref().map_or(true, |a| a.show_copy_location()) {
            file_section.append(Some("Copy Location"), Some("win.copy-path"));
        }
        file_section.append(Some("Move to Trash"), Some("win.move-to-trash"));
        menu.append_section(None, &file_section);

        if self.current_location_is_trash() {
            let trash_section = gio::Menu::new();
            trash_section.append(Some("Restore From Trash"), Some("win.restore-from-trash"));
            trash_section.append(Some("Empty Trash"), Some("win.empty-trash"));
            menu.append_section(None, &trash_section);
        }

        let info_section = gio::Menu::new();
        info_section.append(Some("Properties"), Some("win.properties"));
        menu.append_section(None, &info_section);

        menu.upcast()
    }

    /// Menu model for the header-bar AdwSplitButton dropdown. Rebuilt fresh
    /// on every popup so adding/removing ~/Templates files while wren is
    /// open Just Works (same strategy as the empty-area context menu).
    pub fn new_folder_split_menu() -> gio::MenuModel {
        let menu = gio::Menu::new();
        menu.append(Some("New Folder"), Some("win.new-folder"));
        let templates = templates::list_templates();
        if !templates.is_empty() {
            let templates_section = gio::Menu::new();
            for (name, _path) in &templates {
                let label = std::path::Path::new(name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(name);
                let item = gio::MenuItem::new(Some(label), None);
                item.set_action_and_target_value(
                    Some("win.new-from-template"),
                    Some(&name.to_variant()),
                );
                templates_section.append_item(&item);
            }
            menu.append_section(Some("Templates"), &templates_section);
        }
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

    /// Copy a file from ~/Templates into the current directory and
    /// trigger inline rename on the new file (mirroring the New Folder
    /// flow). `template_name` is the basename inside ~/Templates — we
    /// resolve the source path here rather than embedding it in the
    /// menu item, so a freshly-added template shows up on the next
    /// right-click without restart.
    pub fn new_from_template(&self, template_name: &str) {
        let Some(parent) = self
            .current_tab_index()
            .and_then(|idx| {
                let tabs = self.imp().tabs.borrow();
                tabs.get(idx).and_then(|t| t.navigation.current().cloned())
            })
        else {
            return;
        };

        let src_path = match templates::list_templates()
            .into_iter()
            .find(|(n, _)| n == template_name)
        {
            Some((_, p)) => p,
            None => {
                self.show_toast(&format!("Template not found: {template_name}"));
                return;
            }
        };
        let src = gio::File::for_path(&src_path);

        let Some(dest) = file_ops::unique_dest(&parent, std::path::Path::new(template_name)) else {
            self.show_toast("Could not pick a unique destination name");
            return;
        };

        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = window)] self,
            async move {
                log_op("template", &src, Some(&dest));
                let (fut, _) = src.copy_future(
                    &dest,
                    gio::FileCopyFlags::NONE,
                    glib::Priority::DEFAULT,
                );
                match fut.await {
                    Ok(()) => {
                        window.reload();
                        // The reload spawns an async enumerate; poll
                        // briefly until the new file appears in the
                        // model, then anchor inline rename. Falls back
                        // to the centered dialog if the cell never
                        // realises (cap iterations to avoid spinning).
                        for _ in 0..20 {
                            glib::timeout_future(std::time::Duration::from_millis(50)).await;
                            if window.find_in_current_model(&dest).is_some() {
                                break;
                            }
                        }
                        window.start_inline_rename_for(&dest);
                    }
                    Err(e) => {
                        log_err("template", &src, Some(&dest), &e);
                        window.show_toast(&format!(
                            "Could not create from template: {e}"
                        ));
                    }
                }
            }
        ));
    }

    /// Position of `file` in the current tab's filtered+sorted
    /// selection model, or None if absent (e.g. reload not yet done).
    fn find_in_current_model(&self, file: &gio::File) -> Option<u32> {
        let idx = self.current_tab_index()?;
        let tabs = self.imp().tabs.borrow();
        let tab = tabs.get(idx)?;
        let model = tab.dir_model.as_ref()?;
        let n = model.selection.n_items();
        for i in 0..n {
            if let Some(obj) = model.selection.item(i).and_downcast::<FileObject>() {
                if obj.file().equal(file) {
                    return Some(i);
                }
            }
        }
        None
    }

    /// Select `file` in the current tab's model and pop the inline
    /// rename popover over its cell. Falls back to the centered dialog
    /// if the cell isn't realised yet (e.g. reload still in flight).
    fn start_inline_rename_for(&self, file: &gio::File) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let tabs = self.imp().tabs.borrow();
        let Some(tab) = tabs.get(idx) else { return };
        let Some(model) = tab.dir_model.as_ref() else { return };

        // Find the new file in the (filtered, sorted) selection model.
        let n = model.selection.n_items();
        let mut found_pos: Option<u32> = None;
        for i in 0..n {
            if let Some(obj) = model.selection.item(i).and_downcast::<FileObject>() {
                if obj.file().equal(file) {
                    found_pos = Some(i);
                    break;
                }
            }
        }
        if let Some(pos) = found_pos {
            model.selection.select_item(pos, true);
            tab.file_grid.scroll_to(pos, gtk4::ListScrollFlags::FOCUS);
            tab.file_list.scroll_to(pos, gtk4::ListScrollFlags::FOCUS);
        }
        let current_name = file
            .basename()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let anchor: Option<gtk4::Widget> = tab
            .file_grid
            .cell_for_file(file)
            .map(|c| c.upcast::<gtk4::Widget>())
            .or_else(|| {
                tab.file_list
                    .row_for_file(file)
                    .map(|r| r.upcast::<gtk4::Widget>())
            });
        drop(tabs);

        let file = file.clone();
        if let Some(anchor) = anchor {
            self.rename_selection_inline(file, current_name, &anchor);
        } else {
            self.rename_selection_dialog(file, current_name);
        }
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
            let mut p = glib::user_config_dir();
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
        for obj in self.selected_file_objects() {
            if obj.is_directory() {
                self.navigate_to(self.target_for_activation(&obj));
                return;
            }
            self.activate_file_object(&obj);
        }
    }

    /// Open a non-directory `FileObject` honoring the executable-text-action
    /// preference. For executable text (a script with the execute bit set
    /// whose content type is `text/*` or a known scripting MIME) the user's
    /// configured action — Run, View, or Ask — decides between running the
    /// script and opening it in the default editor. All other files (plain
    /// text, binaries, documents, …) are routed through the default app.
    pub fn activate_file_object(&self, obj: &FileObject) {
        if is_executable_text(obj.file_info()) {
            let policy = self
                .application()
                .and_downcast::<WrenApplication>()
                .map(|a| a.executable_text_action())
                .unwrap_or_else(|| "ask".to_string());
            match policy.as_str() {
                "run" => {
                    self.run_text_file(obj.file());
                    return;
                }
                "view" => {
                    self.launch_default_for_object(obj);
                    return;
                }
                _ => {
                    self.ask_executable_text(obj);
                    return;
                }
            }
        }
        self.launch_default_for_object(obj);
    }

    fn launch_default_for_object(&self, obj: &FileObject) {
        let uri = obj.file().uri();
        if let Err(e) = gio::AppInfo::launch_default_for_uri(
            uri.as_str(),
            gio::AppLaunchContext::NONE,
        ) {
            self.show_toast(&format!("Cannot open: {e}"));
        }
    }

    /// Execute a script directly via `gio::Subprocess::newv`. Runs the file
    /// detached; failures surface as a toast. Local files only — for
    /// non-local URIs we fall back to a toast since execve needs a real path.
    pub fn run_text_file(&self, file: &gio::File) {
        let Some(path) = file.path() else {
            self.show_toast("Cannot run: not a local file");
            return;
        };
        let argv: [&std::ffi::OsStr; 1] = [path.as_os_str()];
        match gio::Subprocess::newv(&argv, gio::SubprocessFlags::NONE) {
            Ok(_proc) => {
                let name = file
                    .basename()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                self.show_toast(&format!("Running {name}"));
            }
            Err(e) => self.show_toast(&format!("Cannot run: {e}")),
        }
    }

    fn ask_executable_text(&self, obj: &FileObject) {
        let name = obj
            .file()
            .basename()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| obj.name());
        let body = format!("What would you like to do with \u{201C}{name}\u{201D}?");
        let dialog = adw::AlertDialog::new(Some("Run executable text file?"), Some(&body));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("view", "Display");
        dialog.add_response("run", "Run");
        dialog.set_response_appearance("view", adw::ResponseAppearance::Suggested);
        dialog.set_response_appearance("run", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let file = obj.file().clone();
        let obj_clone = obj.clone();
        dialog.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, response| match response {
                    "run" => window.run_text_file(&file),
                    "view" => window.launch_default_for_object(&obj_clone),
                    _ => {}
                }
            ),
        );
        dialog.present(Some(self));
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

        let general_page = adw::PreferencesPage::new();
        general_page.set_title("General");
        general_page.set_icon_name(Some("preferences-other-symbolic"));

        let files_page = adw::PreferencesPage::new();
        files_page.set_title("Files");
        files_page.set_icon_name(Some("folder-symbolic"));

        let views_page = adw::PreferencesPage::new();
        views_page.set_title("Views");
        views_page.set_icon_name(Some("view-grid-symbolic"));

        let sidebar_page = adw::PreferencesPage::new();
        sidebar_page.set_title("Sidebar");
        sidebar_page.set_icon_name(Some("sidebar-show-symbolic"));

        let performance_page = adw::PreferencesPage::new();
        performance_page.set_title("Performance");
        performance_page.set_icon_name(Some("emblem-system-symbolic"));

        let context_page = adw::PreferencesPage::new();
        context_page.set_title("Context Menu");
        context_page.set_icon_name(Some("view-list-symbolic"));

        let advanced_page = adw::PreferencesPage::new();
        advanced_page.set_title("Advanced");
        advanced_page.set_icon_name(Some("applications-engineering-symbolic"));

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
        general_page.add(&appearance_group);

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
        advanced_page.add(&group);

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
        performance_page.add(&cache_group);

        // Performance group
        let performance_group = adw::PreferencesGroup::new();
        performance_group.set_title("Performance");

        let thumb_cache_row = adw::SpinRow::with_range(
            crate::application::THUMB_CACHE_MIN as f64,
            crate::application::THUMB_CACHE_MAX as f64,
            64.0,
        );
        thumb_cache_row.set_title("Thumbnail cache size");
        thumb_cache_row.set_subtitle("Number of scaled thumbnails kept in memory");
        let initial_thumb_cap = self
            .application()
            .and_downcast::<WrenApplication>()
            .map(|a| a.thumbnail_cache_size())
            .unwrap_or(crate::application::THUMB_CACHE_DEFAULT);
        thumb_cache_row.set_value(initial_thumb_cap as f64);
        thumb_cache_row.connect_value_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_thumbnail_cache_size(row.value() as usize);
                }
            }
        ));
        performance_group.add(&thumb_cache_row);

        let policy_row = adw::ComboRow::new();
        policy_row.set_title("Show thumbnails");
        policy_row.set_subtitle("Skip generating image previews for remote files");
        let policy_model = gtk4::StringList::new(&["Always", "Local files only", "Never"]);
        policy_row.set_model(Some(&policy_model));
        let initial_policy = self
            .application()
            .and_downcast::<WrenApplication>()
            .map(|a| a.thumbnail_policy())
            .unwrap_or(crate::application::THUMB_POLICY_ALWAYS);
        policy_row.set_selected(initial_policy as u32);
        policy_row.connect_selected_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                let Some(app) = window.application().and_downcast::<WrenApplication>() else {
                    return;
                };
                let new_policy = match row.selected() {
                    1 => crate::application::THUMB_POLICY_LOCAL,
                    2 => crate::application::THUMB_POLICY_NEVER,
                    _ => crate::application::THUMB_POLICY_ALWAYS,
                };
                if new_policy == app.thumbnail_policy() {
                    return;
                }
                app.set_thumbnail_policy(new_policy);
                // Stale textures may now be wrong (e.g. a file flipped
                // from "shown" to "hidden") — drop them and rebind every
                // open tab so the icon falls back / re-renders.
                crate::file_view::cell::clear_thumbnail_cache();
                window.reload_all_tabs();
            }
        ));
        performance_group.add(&policy_row);

        // Folder item count policy
        let folder_count_options = gtk4::StringList::new(&["Always", "On this computer only", "Never"]);
        let folder_count_row = adw::ComboRow::new();
        folder_count_row.set_title("Show folder item count");
        folder_count_row.set_subtitle("Counting items in folders can be slow on network or removable drives");
        folder_count_row.set_model(Some(&folder_count_options));
        let initial_fc_policy = self
            .application()
            .and_downcast::<WrenApplication>()
            .map(|a| a.folder_count_policy())
            .unwrap_or_else(|| "always".to_string());
        let initial_fc_idx: u32 = match initial_fc_policy.as_str() {
            "local" => 1,
            "never" => 2,
            _ => 0,
        };
        folder_count_row.set_selected(initial_fc_idx);
        folder_count_row.connect_selected_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                let policy = match row.selected() {
                    1 => "local",
                    2 => "never",
                    _ => "always",
                };
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_folder_count_policy(policy);
                }
                // Drop existing counts and rebind visible rows so the new
                // policy is reflected immediately (e.g. Always→Never blanks
                // the column without waiting for the user to scroll).
                crate::file_view::row::clear_folder_count_cache();
                window.refresh_visible_list_rows();
            }
        ));
        performance_group.add(&folder_count_row);
        performance_page.add(&performance_group);

        // Context menu group
        let context_group = adw::PreferencesGroup::new();
        context_group.set_title("Context Menu");
        context_group.set_description(Some("Hide actions you don't use"));

        let duplicate_row = adw::SwitchRow::new();
        duplicate_row.set_title("Duplicate");
        let initial_dup = self
            .application()
            .and_downcast::<WrenApplication>()
            .map_or(true, |a| a.show_duplicate());
        duplicate_row.set_active(initial_dup);
        duplicate_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_show_duplicate(row.is_active());
                }
            }
        ));
        context_group.add(&duplicate_row);

        let create_link_row = adw::SwitchRow::new();
        create_link_row.set_title("Create Link");
        let initial_link = self
            .application()
            .and_downcast::<WrenApplication>()
            .map_or(true, |a| a.show_create_link());
        create_link_row.set_active(initial_link);
        create_link_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_show_create_link(row.is_active());
                }
            }
        ));
        context_group.add(&create_link_row);

        let bookmark_row = adw::SwitchRow::new();
        bookmark_row.set_title("Add to Bookmarks");
        let initial_bm = self
            .application()
            .and_downcast::<WrenApplication>()
            .map_or(true, |a| a.show_add_bookmark());
        bookmark_row.set_active(initial_bm);
        bookmark_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_show_add_bookmark(row.is_active());
                }
            }
        ));
        context_group.add(&bookmark_row);

        let copy_loc_row = adw::SwitchRow::new();
        copy_loc_row.set_title("Copy Location");
        let initial_cl = self
            .application()
            .and_downcast::<WrenApplication>()
            .map_or(true, |a| a.show_copy_location());
        copy_loc_row.set_active(initial_cl);
        copy_loc_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_show_copy_location(row.is_active());
                }
            }
        ));
        context_group.add(&copy_loc_row);
        context_page.add(&context_group);

        // Window group
        let window_group = adw::PreferencesGroup::new();
        window_group.set_title("Window");

        let full_path_row = adw::SwitchRow::new();
        full_path_row.set_title("Show full path in title");
        full_path_row.set_subtitle("Display the active tab's full filesystem path in the window title");
        let initial_fp = self
            .application()
            .and_downcast::<WrenApplication>()
            .map_or(false, |a| a.show_full_path_in_title());
        full_path_row.set_active(initial_fp);
        full_path_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_show_full_path_in_title(row.is_active());
                }
                window.refresh_title();
            }
        ));
        window_group.add(&full_path_row);
        general_page.add(&window_group);

        // Files group (display options that affect every view)
        let files_group = adw::PreferencesGroup::new();
        files_group.set_title("Files");

        let hidden_row = adw::SwitchRow::new();
        hidden_row.set_title("Show hidden files");
        hidden_row.set_subtitle("Display dotfiles and folders starting with a period");
        let initial_hidden = self.imp().show_hidden.get();
        hidden_row.set_active(initial_hidden);
        hidden_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                let v = row.is_active();
                window.imp().show_hidden.set(v);
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_show_hidden(v);
                }
                if let Some(action) = window
                    .lookup_action("toggle-hidden")
                    .and_downcast::<gio::SimpleAction>()
                {
                    action.set_state(&v.to_variant());
                }
                window.apply_hidden_filter();
            }
        ));
        files_group.add(&hidden_row);
        files_page.add(&files_group);

        // Trash group
        let trash_group = adw::PreferencesGroup::new();
        trash_group.set_title("Trash");

        let confirm_trash_row = adw::SwitchRow::new();
        confirm_trash_row.set_title("Confirm before moving to Trash");
        confirm_trash_row.set_subtitle("Ask for confirmation when pressing Delete");
        let initial_ct = self
            .application()
            .and_downcast::<WrenApplication>()
            .map_or(true, |a| a.confirm_move_to_trash());
        confirm_trash_row.set_active(initial_ct);
        confirm_trash_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_confirm_move_to_trash(row.is_active());
                }
            }
        ));
        trash_group.add(&confirm_trash_row);
        files_page.add(&trash_group);

        // Sorting group
        let sorting_group = adw::PreferencesGroup::new();
        sorting_group.set_title("Sorting");

        let folders_first_row = adw::SwitchRow::new();
        folders_first_row.set_title("Folders before files");
        folders_first_row.set_subtitle("Group directories at the top regardless of sort order");
        let initial_ff = self
            .application()
            .and_downcast::<WrenApplication>()
            .map_or(true, |a| a.folders_first());
        folders_first_row.set_active(initial_ff);
        folders_first_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_folders_first(row.is_active());
                }
                window.refresh_all_sorts();
            }
        ));
        sorting_group.add(&folders_first_row);
        general_page.add(&sorting_group);

        // New Tab group
        self.build_new_tab_settings_group(&views_page);

        // List view columns group
        let columns_group = adw::PreferencesGroup::new();
        columns_group.set_title("List View Columns");
        columns_group.set_description(Some("Choose which columns appear in list view"));

        // (title, getter, action_name) — toggling routes through the
        // stateful action so the right-click header menu's checkmark
        // stays in sync with this SwitchRow.
        type GetFn = fn(&WrenApplication) -> bool;
        let column_rows: &[(&str, GetFn, &str)] = &[
            ("Type",        WrenApplication::show_col_type,        "toggle-col-type"),
            ("Size",        WrenApplication::show_col_size,        "toggle-col-size"),
            ("Modified",    WrenApplication::show_col_modified,    "toggle-col-modified"),
            ("Permissions", WrenApplication::show_col_permissions, "toggle-col-permissions"),
            ("Owner",       WrenApplication::show_col_owner,       "toggle-col-owner"),
            ("Group",       WrenApplication::show_col_group,       "toggle-col-group"),
            ("Accessed",    WrenApplication::show_col_accessed,    "toggle-col-accessed"),
        ];
        for &(title, getter, action_name) in column_rows {
            let row = adw::SwitchRow::new();
            row.set_title(title);
            let initial = self
                .application()
                .and_downcast::<WrenApplication>()
                .map(|a| getter(&a))
                .unwrap_or(false);
            row.set_active(initial);
            row.connect_active_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |row| {
                    let current = window
                        .application()
                        .and_downcast::<WrenApplication>()
                        .map(|a| getter(&a))
                        .unwrap_or(false);
                    if current != row.is_active() {
                        WidgetExt::activate_action(&window, action_name, None).ok();
                    }
                }
            ));
            columns_group.add(&row);
        }
        views_page.add(&columns_group);

        // Executable text files action — shoehorned into the existing Files
        // group built by the prefs-cluster (which already owns the Show
        // Hidden Files toggle). If that group's row is "show_hidden_row",
        // we stuff this combo row in beside it.
        let files_group = adw::PreferencesGroup::new();
        files_group.set_title("Executable Text Files");

        let exec_text_row = adw::ComboRow::new();
        exec_text_row.set_title("Executable text files");
        exec_text_row.set_subtitle("How to handle executable scripts when activated");
        let exec_model = gtk4::StringList::new(&["Run", "Display", "Ask each time"]);
        exec_text_row.set_model(Some(&exec_model));
        let initial_exec = self
            .application()
            .and_downcast::<WrenApplication>()
            .map(|a| a.executable_text_action())
            .unwrap_or_else(|| "ask".to_string());
        let initial_exec_idx: u32 = match initial_exec.as_str() {
            "run" => 0,
            "view" => 1,
            _ => 2,
        };
        exec_text_row.set_selected(initial_exec_idx);
        exec_text_row.connect_selected_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                let value = match row.selected() {
                    0 => "run",
                    1 => "view",
                    _ => "ask",
                };
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_executable_text_action(value);
                }
            }
        ));
        files_group.add(&exec_text_row);
        files_page.add(&files_group);

        // Sidebar group
        let sidebar_group = adw::PreferencesGroup::new();
        sidebar_group.set_title("Sidebar");

        let recents_row = adw::SwitchRow::new();
        recents_row.set_title("Recent locations");
        recents_row.set_subtitle("Track and show recently visited folders in the sidebar");
        let initial_recents = self
            .application()
            .and_downcast::<WrenApplication>()
            .map(|a| a.recents_enabled())
            .unwrap_or(false);
        recents_row.set_active(initial_recents);
        let recents_max_row = adw::SpinRow::with_range(
            crate::application::RECENTS_MIN as f64,
            crate::application::RECENTS_MAX as f64,
            1.0,
        );
        recents_max_row.set_title("Recent locations to keep");
        recents_max_row.set_subtitle("Maximum entries shown under Recent in the sidebar");
        let initial_cap = self
            .application()
            .and_downcast::<WrenApplication>()
            .map(|a| a.recents_cap())
            .unwrap_or(crate::application::RECENTS_DEFAULT);
        recents_max_row.set_value(initial_cap as f64);
        recents_max_row.set_sensitive(initial_recents);

        recents_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[weak] recents_max_row,
            move |row| {
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_recents_enabled(row.is_active());
                }
                recents_max_row.set_sensitive(row.is_active());
                window.imp().sidebar.reload_recents();
            }
        ));
        recents_max_row.connect_value_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    if app.set_recents_cap(row.value() as usize) {
                        window.imp().sidebar.reload_recents();
                    }
                }
            }
        ));
        sidebar_group.add(&recents_row);
        sidebar_group.add(&recents_max_row);

        let bookmarks_row = adw::SwitchRow::new();
        bookmarks_row.set_title("Bookmarks");
        bookmarks_row.set_subtitle("Show the Bookmarks section in the sidebar");
        let initial_bookmarks = self
            .application()
            .and_downcast::<WrenApplication>()
            .map_or(true, |a| a.bookmarks_enabled());
        bookmarks_row.set_active(initial_bookmarks);
        bookmarks_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |row| {
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_bookmarks_enabled(row.is_active());
                }
                window.imp().sidebar.reload_bookmarks();
            }
        ));
        sidebar_group.add(&bookmarks_row);
        sidebar_page.add(&sidebar_group);

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
        advanced_page.add(&advanced_group);

        dialog.add(&general_page);
        dialog.add(&files_page);
        dialog.add(&views_page);
        dialog.add(&sidebar_page);
        dialog.add(&performance_page);
        dialog.add(&context_page);
        dialog.add(&advanced_page);
        dialog.present(Some(self));
    }

    /// Build the "New Tab" settings group (use-defaults toggle + per-default
    /// rows). Split out of `open_settings` because it owns several inter-row
    /// sensitivity links that would clutter the dispatch site.
    fn build_new_tab_settings_group(&self, page: &adw::PreferencesPage) {
        let app = self.application().and_downcast::<WrenApplication>();

        let group = adw::PreferencesGroup::new();
        group.set_title("New Tab");
        group.set_description(Some(
            "Override the folder, view, sort, and zoom Ctrl+T uses",
        ));

        let use_defaults = app.as_ref().map_or(false, |a| a.new_tab_use_defaults());

        let use_row = adw::SwitchRow::new();
        use_row.set_title("Use defaults for new tabs");
        use_row.set_subtitle("When off, new tabs open the home folder with global view/sort");
        use_row.set_active(use_defaults);

        let path_row = adw::EntryRow::new();
        path_row.set_title("Default folder");
        let initial_path = app.as_ref().map_or(String::new(), |a| a.new_tab_default_path());
        let display_path = if initial_path.is_empty() {
            glib::home_dir().to_string_lossy().into_owned()
        } else if initial_path.starts_with("file://") {
            gio::File::for_uri(&initial_path)
                .path()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or(initial_path.clone())
        } else {
            initial_path.clone()
        };
        path_row.set_text(&display_path);

        let view_row = adw::ActionRow::new();
        view_row.set_title("Default view");
        let view_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        view_box.add_css_class("linked");
        view_box.set_valign(gtk4::Align::Center);
        let grid_btn = gtk4::ToggleButton::with_label("Grid");
        let list_btn = gtk4::ToggleButton::with_label("List");
        list_btn.set_group(Some(&grid_btn));
        let initial_view = app
            .as_ref()
            .map_or("grid".to_string(), |a| a.new_tab_default_view());
        if initial_view == "list" {
            list_btn.set_active(true);
        } else {
            grid_btn.set_active(true);
        }
        view_box.append(&grid_btn);
        view_box.append(&list_btn);
        view_row.add_suffix(&view_box);

        let sort_row = adw::ComboRow::new();
        sort_row.set_title("Default sort");
        let sort_keys = ["name", "size", "date", "type"];
        let sort_labels = ["Name", "Size", "Date Modified", "Type"];
        let sort_model = gtk4::StringList::new(&sort_labels);
        sort_row.set_model(Some(&sort_model));
        let initial_sort = app
            .as_ref()
            .map_or("name".to_string(), |a| a.new_tab_default_sort_key());
        let sort_idx = sort_keys
            .iter()
            .position(|k| *k == initial_sort.as_str())
            .unwrap_or(0) as u32;
        sort_row.set_selected(sort_idx);

        let dir_row = adw::ActionRow::new();
        dir_row.set_title("Default sort direction");
        let dir_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        dir_box.add_css_class("linked");
        dir_box.set_valign(gtk4::Align::Center);
        let asc_btn = gtk4::ToggleButton::with_label("Ascending");
        let desc_btn = gtk4::ToggleButton::with_label("Descending");
        desc_btn.set_group(Some(&asc_btn));
        let initial_reversed = app.as_ref().map_or(false, |a| a.new_tab_default_sort_reversed());
        if initial_reversed {
            desc_btn.set_active(true);
        } else {
            asc_btn.set_active(true);
        }
        dir_box.append(&asc_btn);
        dir_box.append(&desc_btn);
        dir_row.add_suffix(&dir_box);

        let zoom_row = adw::SpinRow::with_range(1.0, 5.0, 1.0);
        zoom_row.set_title("Default zoom");
        zoom_row.set_subtitle("Icon size: 1 (smallest) to 5 (largest)");
        let initial_zoom = app.as_ref().map_or(3, |a| a.new_tab_default_zoom());
        zoom_row.set_value(initial_zoom as f64);

        // Path row is gated by the use-defaults toggle; the per-default
        // rows (view/sort/dir/zoom) are always editable per task spec.
        path_row.set_sensitive(use_defaults);

        use_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = window)] self,
            #[weak] path_row,
            move |row| {
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_new_tab_use_defaults(row.is_active());
                }
                path_row.set_sensitive(row.is_active());
            }
        ));
        path_row.connect_changed(glib::clone!(
            #[weak(rename_to = window)] self,
            move |entry| {
                let text = entry.text().to_string();
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    let stored = if text.is_empty() {
                        String::new()
                    } else if text.contains("://") {
                        text
                    } else {
                        gio::File::for_path(&text).uri().to_string()
                    };
                    app.set_new_tab_default_path(&stored);
                }
            }
        ));
        let connect_view = |btn: &gtk4::ToggleButton, value: &'static str| {
            btn.connect_toggled(glib::clone!(
                #[weak(rename_to = window)] self,
                move |btn| {
                    if !btn.is_active() { return; }
                    if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                        app.set_new_tab_default_view(value);
                    }
                }
            ));
        };
        connect_view(&grid_btn, "grid");
        connect_view(&list_btn, "list");
        sort_row.connect_selected_notify(glib::clone!(
            #[weak(rename_to = window)] self,
            move |row| {
                let idx = row.selected() as usize;
                let key = ["name", "size", "date", "type"]
                    .get(idx)
                    .copied()
                    .unwrap_or("name");
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_new_tab_default_sort_key(key);
                }
            }
        ));
        let connect_dir = |btn: &gtk4::ToggleButton, reversed: bool| {
            btn.connect_toggled(glib::clone!(
                #[weak(rename_to = window)] self,
                move |btn| {
                    if !btn.is_active() { return; }
                    if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                        app.set_new_tab_default_sort_reversed(reversed);
                    }
                }
            ));
        };
        connect_dir(&asc_btn, false);
        connect_dir(&desc_btn, true);
        zoom_row.connect_value_notify(glib::clone!(
            #[weak(rename_to = window)] self,
            move |row| {
                if let Some(app) = window.application().and_downcast::<WrenApplication>() {
                    app.set_new_tab_default_zoom(row.value() as i32);
                }
            }
        ));

        group.add(&use_row);
        group.add(&path_row);
        group.add(&view_row);
        group.add(&sort_row);
        group.add(&dir_row);
        group.add(&zoom_row);
        page.add(&group);
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
        self.action_set_enabled("win.new-from-template", !in_trash);
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
        let (n_total, n_selected, selected_bytes, label, location) = {
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
            let location = tab.navigation.current().cloned();
            (n_total, n_selected, bytes, tab.status_bar.clone(), location)
        };
        let base = if n_selected == 0 {
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

        // Append free/total disk space when we already have a fresh cached
        // entry; otherwise paint the base text now and refresh in the
        // background. Skip the indicator entirely for virtual schemes that
        // can't expose filesystem info — leave the existing text alone.
        if let Some(loc) = location {
            if let Some(usage) = disk_usage::cached_for(&loc) {
                label.set_text(&format!("{}{}", base, disk_usage::format_suffix(&usage)));
                label.set_tooltip_text(Some(&disk_usage::format_tooltip(&usage)));
            } else {
                label.set_text(&base);
                label.set_tooltip_text(None);
                disk_usage::refresh_async(loc, label, base);
            }
        } else {
            label.set_text(&base);
            label.set_tooltip_text(None);
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

    /// Show the shared in-window banner with `text`. If `button_label` is
    /// `Some`, a button with that label is shown and `on_click` is invoked
    /// when pressed. If `button_label` is `None`, no button is shown and
    /// `on_click` is ignored. Replaces any previously-set callback.
    pub fn show_banner<F: Fn(&Self) + 'static>(
        &self,
        text: &str,
        button_label: Option<&str>,
        on_click: Option<F>,
    ) {
        let imp = self.imp();
        let banner: &adw::Banner = &imp.banner;
        banner.set_title(text);
        banner.set_button_label(button_label);
        if let Some(old) = imp.banner_handler.borrow_mut().take() {
            banner.disconnect(old);
        }
        if let (Some(_), Some(cb)) = (button_label, on_click) {
            let id = banner.connect_button_clicked(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| {
                    cb(&window);
                }
            ));
            *imp.banner_handler.borrow_mut() = Some(id);
        }
        banner.set_revealed(true);
    }

    pub fn hide_banner(&self) {
        let imp = self.imp();
        imp.banner.set_revealed(false);
        if let Some(old) = imp.banner_handler.borrow_mut().take() {
            imp.banner.disconnect(old);
        }
        imp.banner.set_button_label(None);
    }

    /// Asynchronously query the writability of `location` and toggle the
    /// read-only banner accordingly. Called after every successful load
    /// in `load_location_for_tab`.
    fn refresh_readonly_banner(&self, tab_idx: usize, load_gen: u64, location: gio::File) {
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = window)]
            self,
            async move {
                let info = location
                    .query_info_future(
                        "access::can-write",
                        gio::FileQueryInfoFlags::NONE,
                        glib::Priority::DEFAULT,
                    )
                    .await;

                // Only mutate UI if this load wasn't superseded and the
                // tab is still the foreground tab.
                let is_current = {
                    let tabs = window.imp().tabs.borrow();
                    tabs.get(tab_idx)
                        .map_or(false, |t| t.load_gen.get() == load_gen)
                };
                if !is_current {
                    return;
                }
                if window.current_tab_index() != Some(tab_idx) {
                    return;
                }

                match info {
                    Ok(info) if !info.boolean(gio::FILE_ATTRIBUTE_ACCESS_CAN_WRITE) => {
                        window.show_banner::<fn(&Self)>(
                            "This folder is read-only",
                            None,
                            None,
                        );
                    }
                    _ => window.hide_banner(),
                }
            }
        ));
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

    /// After a successful unmount/eject, navigate any tab whose current
    /// location lives under `unmounted_root` back to home. Returns true
    /// if at least one tab was redirected.
    pub fn leave_unmounted_root(&self, unmounted_root: &gio::File) -> bool {
        let imp = self.imp();
        let affected: Vec<usize> = {
            let tabs = imp.tabs.borrow();
            tabs.iter()
                .enumerate()
                .filter_map(|(i, t)| {
                    let cur = t.navigation.current()?;
                    if cur.equal(unmounted_root) || cur.has_prefix(unmounted_root) {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect()
        };
        if affected.is_empty() {
            return false;
        }
        let home = gio::File::for_path(glib::home_dir());
        for idx in affected {
            let maybe_loc = {
                let mut tabs = imp.tabs.borrow_mut();
                tabs.get_mut(idx)
                    .and_then(|t| t.navigation.navigate_to(home.clone()))
            };
            if let Some(loc) = maybe_loc {
                self.load_location_for_tab(idx, loc);
            }
        }
        self.update_nav_buttons();
        true
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
            let mut p = glib::user_config_dir();
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

// True when the file is executable text — a script the user might want to
// run rather than open in an editor. Requires BOTH the executable bit set
// (`access::can-execute`) AND a content type that's plain text or a known
// scripting MIME (shell, python, perl, ruby, lua, php, awk, sed, tcl,
// node/javascript, R). Plain text without the execute bit is just text;
// executable binaries (no `text/*` ancestry) fall through to the OS via
// the FileLauncher / desktop-registration path.
fn is_executable_text(info: &gio::FileInfo) -> bool {
    if !info.has_attribute(gio::FILE_ATTRIBUTE_ACCESS_CAN_EXECUTE)
        || !info.boolean(gio::FILE_ATTRIBUTE_ACCESS_CAN_EXECUTE)
    {
        return false;
    }
    let Some(ct) = info.content_type() else {
        return false;
    };
    let ct = ct.as_str();
    if gio::content_type_is_a(ct, "text/plain") {
        return true;
    }
    matches!(
        ct,
        "application/x-shellscript"
            | "application/x-sh"
            | "application/x-csh"
            | "application/x-python"
            | "application/x-python3"
            | "application/x-perl"
            | "application/x-ruby"
            | "application/x-lua"
            | "application/x-php"
            | "application/x-awk"
            | "application/x-sed"
            | "application/x-tcl"
            | "application/javascript"
            | "application/x-javascript"
            | "application/x-r"
    )
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
