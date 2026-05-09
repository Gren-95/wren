use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use glib::subclass::InitializingObject;
use gtk4::{CompositeTemplate, TemplateChild};

use crate::breadcrumb::WrenBreadcrumbBar;
use crate::sidebar::WrenSidebar;
use crate::window::tab::TabState;
use crate::window::undo::UndoOp;

#[derive(Debug, CompositeTemplate)]
#[template(resource = "/io/github/wren/ui/window.ui")]
pub struct WrenWindow {
    #[template_child]
    pub header_bar: TemplateChild<adw::HeaderBar>,
    #[template_child]
    pub back_button: TemplateChild<gtk4::Button>,
    #[template_child]
    pub forward_button: TemplateChild<gtk4::Button>,
    #[template_child]
    pub split_view: TemplateChild<adw::OverlaySplitView>,
    #[template_child]
    pub toast_overlay: TemplateChild<adw::ToastOverlay>,
    #[template_child]
    pub search_bar: TemplateChild<gtk4::SearchBar>,
    #[template_child]
    pub search_entry: TemplateChild<gtk4::SearchEntry>,
    #[template_child]
    pub search_button: TemplateChild<gtk4::ToggleButton>,
    #[template_child]
    pub tab_bar: TemplateChild<adw::TabBar>,
    #[template_child]
    pub tab_view: TemplateChild<adw::TabView>,
    #[template_child]
    pub sidebar: TemplateChild<WrenSidebar>,
    #[template_child]
    pub sidebar_button: TemplateChild<gtk4::ToggleButton>,
    #[template_child]
    pub menu_button: TemplateChild<gtk4::MenuButton>,
    #[template_child]
    pub view_button: TemplateChild<gtk4::MenuButton>,
    #[template_child]
    pub breadcrumb_bar: TemplateChild<WrenBreadcrumbBar>,
    #[template_child]
    pub op_button: TemplateChild<gtk4::MenuButton>,

    pub tabs: RefCell<Vec<TabState>>,
    pub clipboard_files: RefCell<Option<(Vec<gio::File>, bool)>>,
    pub show_hidden: Cell<bool>,
    pub show_extensions: Cell<bool>,
    pub zoom_level: Cell<i32>,
    pub zoom_adjustment: gtk4::Adjustment,
    pub undo_stack: RefCell<Vec<UndoOp>>,
    pub redo_stack: RefCell<Vec<UndoOp>>,
    /// Vertical Box inside the op_button's popover that holds one row per
    /// active operation.
    pub op_popover_box: gtk4::Box,
    /// Active operations (cancellable + the row widget that displays them).
    pub op_handles: RefCell<Vec<crate::window::OpHandle>>,
    /// Currently-shown Undo toast for trash. Dismissed before issuing
    /// a new one so the Undo button always refers to the most recent
    /// trash op (otherwise toasts queue and the user clicks Undo on
    /// the wrong one).
    pub active_undo_toast: RefCell<Option<adw::Toast>>,
}

impl Default for WrenWindow {
    fn default() -> Self {
        Self {
            header_bar: Default::default(),
            back_button: Default::default(),
            forward_button: Default::default(),
            split_view: Default::default(),
            toast_overlay: Default::default(),
            search_bar: Default::default(),
            search_entry: Default::default(),
            search_button: Default::default(),
            tab_bar: Default::default(),
            tab_view: Default::default(),
            sidebar: Default::default(),
            sidebar_button: Default::default(),
            menu_button: Default::default(),
            view_button: Default::default(),
            breadcrumb_bar: Default::default(),
            op_button: Default::default(),
            tabs: Default::default(),
            clipboard_files: Default::default(),
            show_hidden: Default::default(),
            show_extensions: Cell::new(true),
            zoom_level: Cell::new(3),
            zoom_adjustment: gtk4::Adjustment::new(3.0, 1.0, 5.0, 1.0, 1.0, 0.0),
            undo_stack: Default::default(),
            redo_stack: Default::default(),
            op_popover_box: gtk4::Box::new(gtk4::Orientation::Vertical, 4),
            op_handles: Default::default(),
            active_undo_toast: Default::default(),
        }
    }
}

#[glib::object_subclass]
impl ObjectSubclass for WrenWindow {
    const NAME: &'static str = "WrenWindow";
    type Type = super::WrenWindow;
    type ParentType = adw::ApplicationWindow;

    fn class_init(klass: &mut Self::Class) {
        WrenSidebar::ensure_type();
        WrenBreadcrumbBar::ensure_type();
        klass.bind_template();
        klass.bind_template_callbacks();

        klass.install_action("win.navigate-back", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.navigate_back();
        });
        klass.install_action("win.navigate-forward", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.navigate_forward();
        });
        klass.install_action("win.navigate-up", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.navigate_up();
        });
        klass.install_action("win.toggle-search", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.toggle_search();
        });
        klass.install_action("win.new-tab", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.new_tab();
        });
        klass.install_action("win.close-tab", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.close_tab();
        });
        klass.install_action("win.select-all", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.select_all();
        });
        klass.install_action("win.open-settings", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.open_settings();
        });
        klass.install_action("win.focus-location", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.focus_location();
        });
        klass.install_action("win.open-selection", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.open_selection();
        });
        klass.install_action("win.open-with", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.open_with();
        });
        klass.install_action("win.open-in-terminal", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.open_in_terminal();
        });
        klass.install_action("win.new-folder", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.new_folder();
        });
        klass.install_action("win.rename", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.rename_selection();
        });
        klass.install_action("win.move-to-trash", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.move_to_trash();
        });
        klass.install_action("win.delete-permanently", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.delete_permanently();
        });
        klass.install_action("win.empty-trash", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.empty_trash();
        });
        klass.install_action("win.restore-from-trash", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.restore_from_trash();
        });
        klass.install_action(
            "win.open-window-at",
            Some(glib::VariantTy::STRING),
            |win, action_name, param| {
                if let Some(uri) = param.and_then(|v| v.str()) {
                    crate::wren_log!("action: {action_name}({uri})");
                    win.open_window_at(uri);
                }
            },
        );
        klass.install_action(
            "win.copy-path-at",
            Some(glib::VariantTy::STRING),
            |win, action_name, param| {
                if let Some(uri) = param.and_then(|v| v.str()) {
                    crate::wren_log!("action: {action_name}({uri})");
                    win.copy_path_at(uri);
                }
            },
        );
        klass.install_action("win.copy", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.copy_selection();
        });
        klass.install_action("win.cut", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.cut_selection();
        });
        klass.install_action("win.paste", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.paste();
        });
        klass.install_action("win.zoom-in", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.zoom_in();
        });
        klass.install_action("win.zoom-out", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.zoom_out();
        });
        klass.install_action("win.zoom-reset", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.zoom_reset();
        });
        klass.install_action("win.properties", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.show_properties();
        });
        klass.install_action("win.create-link", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.create_link();
        });
        klass.install_action("win.add-bookmark", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.add_bookmark();
        });
        klass.install_action("win.undo", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.undo();
        });
        klass.install_action("win.redo", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.redo();
        });
        klass.install_action("win.batch-rename", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.batch_rename();
        });
        klass.install_action("win.duplicate", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.duplicate();
        });
        klass.install_action("win.about", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.show_about();
        });
        klass.install_action("win.reload", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.reload();
        });
        klass.install_action("win.toggle-sidebar", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            let imp = win.imp();
            let show = !imp.split_view.shows_sidebar();
            imp.split_view.set_show_sidebar(show);
        });
        klass.install_action("win.navigate-home", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.navigate_home();
        });
        klass.install_action("win.new-window", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.new_window();
        });
        klass.install_action("win.copy-path", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.copy_path();
        });
        klass.install_action("win.show-shortcuts", None, |win, action_name, _| {
            crate::wren_log!("action: {action_name}");
            win.show_shortcuts();
        });
        klass.install_action(
            "win.remove-bookmark",
            Some(glib::VariantTy::STRING),
            |win, action_name, param| {
                if let Some(uri) = param.and_then(|v| v.str()) {
                    crate::wren_log!("action: {action_name}({uri})");
                    win.remove_bookmark(uri);
                }
            },
        );
        klass.install_action(
            "win.open-tab-at",
            Some(glib::VariantTy::STRING),
            |win, action_name, param| {
                if let Some(uri) = param.and_then(|v| v.str()) {
                    crate::wren_log!("action: {action_name}({uri})");
                    win.add_tab(gio::File::for_uri(uri));
                }
            },
        );
        klass.install_action(
            "win.open-terminal-at",
            Some(glib::VariantTy::STRING),
            |win, action_name, param| {
                if let Some(uri) = param.and_then(|v| v.str()) {
                    crate::wren_log!("action: {action_name}({uri})");
                    win.open_terminal_at_uri(uri);
                }
            },
        );
    }

    fn instance_init(obj: &InitializingObject<Self>) {
        obj.init_template();
    }
}

impl ObjectImpl for WrenWindow {
    fn constructed(&self) {
        self.parent_constructed();
        let obj = self.obj();

        self.zoom_level.set(3);

        // Close-page: triggered for both Ctrl+W (via close_tab → close_page)
        // and the per-tab close button. Refuse to close the last tab so the
        // window doesn't end up empty, otherwise drop the matching TabState
        // (cancelling its monitor + dir_model load) before letting GTK
        // detach the page.
        self.tab_view.connect_close_page(glib::clone!(
            #[weak]
            obj,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |tab_view, page| {
                if tab_view.n_pages() <= 1 {
                    tab_view.close_page_finish(page, false);
                    return glib::Propagation::Stop;
                }
                let child = page.child();
                {
                    let mut tabs = obj.imp().tabs.borrow_mut();
                    if let Some(idx) = tabs.iter().position(|t| t.content_widget == child) {
                        tabs[idx].cancel_monitor();
                        if let Some(model) = tabs[idx].dir_model.as_ref() {
                            model.cancel();
                        }
                        tabs.remove(idx);
                    }
                }
                tab_view.close_page_finish(page, true);
                glib::Propagation::Stop
            }
        ));

        // When selected tab changes, update breadcrumb and nav buttons
        self.tab_view.connect_selected_page_notify(glib::clone!(
            #[weak]
            obj,
            move |_| {
                obj.on_tab_switched();
            }
        ));

        // Stateful toggle-extensions action
        let toggle_ext_action = gio::SimpleAction::new_stateful(
            "toggle-extensions",
            None,
            &true.to_variant(),
        );
        toggle_ext_action.connect_activate(glib::clone!(
            #[weak]
            obj,
            move |action, _| {
                let current = action
                    .state()
                    .and_then(|v| v.get::<bool>())
                    .unwrap_or(true);
                let new_val = !current;
                crate::wren_log!("action: win.toggle-extensions -> {new_val}");
                action.set_state(&new_val.to_variant());
                obj.imp().show_extensions.set(new_val);
                obj.apply_extensions_setting();
                if let Some(app) = obj
                    .application()
                    .and_downcast::<crate::application::WrenApplication>()
                {
                    app.set_show_extensions(new_val);
                }
            }
        ));
        obj.add_action(&toggle_ext_action);

        // Stateful toggle-hidden action (drives checkmark in hamburger menu)
        let toggle_hidden_action = gio::SimpleAction::new_stateful(
            "toggle-hidden",
            None,
            &false.to_variant(),
        );
        toggle_hidden_action.connect_activate(glib::clone!(
            #[weak]
            obj,
            move |action, _| {
                let current = action
                    .state()
                    .and_then(|v| v.get::<bool>())
                    .unwrap_or(false);
                let new_val = !current;
                crate::wren_log!("action: win.toggle-hidden -> {new_val}");
                action.set_state(&new_val.to_variant());
                obj.imp().show_hidden.set(new_val);
                obj.apply_hidden_filter();
                if let Some(app) = obj.application().and_downcast::<crate::application::WrenApplication>() {
                    app.set_show_hidden(new_val);
                }
            }
        ));
        obj.add_action(&toggle_hidden_action);

        // Stateful sort-key action (drives radio checkmarks in sort submenu)
        let sort_key_action = gio::SimpleAction::new_stateful(
            "set-sort-key",
            Some(glib::VariantTy::STRING),
            &"name".to_variant(),
        );
        sort_key_action.connect_activate(glib::clone!(
            #[weak]
            obj,
            move |action, param| {
                if let Some(key_str) = param.and_then(|v| v.str()) {
                    crate::wren_log!("action: win.set-sort-key({key_str})");
                    action.set_state(&key_str.to_variant());
                    obj.set_sort_key(key_str);
                    if let Some(app) = obj.application().and_downcast::<crate::application::WrenApplication>() {
                        app.set_sort_key_pref(key_str);
                    }
                }
            }
        ));
        obj.add_action(&sort_key_action);

        // Stateful sort-reversed action
        let sort_reversed_action = gio::SimpleAction::new_stateful(
            "toggle-sort-reversed",
            None,
            &false.to_variant(),
        );
        sort_reversed_action.connect_activate(glib::clone!(
            #[weak]
            obj,
            move |action, _| {
                let current = action
                    .state()
                    .and_then(|v| v.get::<bool>())
                    .unwrap_or(false);
                let new_val = !current;
                crate::wren_log!("action: win.toggle-sort-reversed -> {new_val}");
                action.set_state(&new_val.to_variant());
                obj.set_sort_reversed(new_val);
                if let Some(app) = obj.application().and_downcast::<crate::application::WrenApplication>() {
                    app.set_sort_reversed_pref(new_val);
                }
            }
        ));
        obj.add_action(&sort_reversed_action);

        // View mode dropdown
        let view_menu = gio::Menu::new();
        let grid_item = gio::MenuItem::new(Some("Grid"), None);
        grid_item.set_action_and_target_value(
            Some("win.set-view-mode"),
            Some(&"grid".to_variant()),
        );
        view_menu.append_item(&grid_item);
        let list_item = gio::MenuItem::new(Some("List"), None);
        list_item.set_action_and_target_value(
            Some("win.set-view-mode"),
            Some(&"list".to_variant()),
        );
        view_menu.append_item(&list_item);
        obj.imp().view_button.set_menu_model(Some(&view_menu));

        // Stateful view-mode action
        let view_mode_action = gio::SimpleAction::new_stateful(
            "set-view-mode",
            Some(glib::VariantTy::STRING),
            &"grid".to_variant(),
        );
        view_mode_action.connect_activate(glib::clone!(
            #[weak]
            obj,
            move |action, param| {
                if let Some(mode) = param.and_then(|v| v.str()) {
                    crate::wren_log!("action: win.set-view-mode({mode})");
                    action.set_state(&mode.to_variant());
                    obj.set_view_mode(mode);
                    if let Some(app) = obj.application().and_downcast::<crate::application::WrenApplication>() {
                        app.set_view_mode_pref(mode);
                    }
                }
            }
        ));
        obj.add_action(&view_mode_action);

        // Hamburger menu
        let hamburger = gio::Menu::new();

        // Custom zoom slider section (widget embedded via PopoverMenu::add_child)
        let zoom_section = gio::Menu::new();
        let zoom_item = gio::MenuItem::new(None, None);
        zoom_item.set_attribute_value("custom", Some(&"zoom-controls".to_variant()));
        zoom_section.append_item(&zoom_item);
        hamburger.append_section(None, &zoom_section);

        let view_section = gio::Menu::new();
        view_section.append(Some("Show Hidden Files"), Some("win.toggle-hidden"));
        view_section.append(Some("Show File Extensions"), Some("win.toggle-extensions"));
        hamburger.append_section(None, &view_section);

        // Sort submenu
        let sort_submenu = gio::Menu::new();
        let sort_keys_section = gio::Menu::new();
        for (label, key) in &[
            ("Name", "name"),
            ("Size", "size"),
            ("Date Modified", "date"),
            ("Type", "type"),
        ] {
            let item = gio::MenuItem::new(Some(label), None);
            item.set_action_and_target_value(
                Some("win.set-sort-key"),
                Some(&key.to_variant()),
            );
            sort_keys_section.append_item(&item);
        }
        sort_submenu.append_section(None, &sort_keys_section);
        let sort_options_section = gio::Menu::new();
        sort_options_section.append(Some("Reversed"), Some("win.toggle-sort-reversed"));
        sort_submenu.append_section(None, &sort_options_section);
        let sort_section = gio::Menu::new();
        sort_section.append_submenu(Some("Sort By"), &sort_submenu);
        hamburger.append_section(None, &sort_section);

        let edit_section = gio::Menu::new();
        edit_section.append(Some("Undo"), Some("win.undo"));
        edit_section.append(Some("Redo"), Some("win.redo"));
        hamburger.append_section(None, &edit_section);

        let settings_section = gio::Menu::new();
        settings_section.append(Some("Settings…"), Some("win.open-settings"));
        hamburger.append_section(None, &settings_section);

        let help_section = gio::Menu::new();
        help_section.append(Some("Keyboard Shortcuts"), Some("win.show-shortcuts"));
        help_section.append(Some("About Wren"), Some("win.about"));
        hamburger.append_section(None, &help_section);

        obj.imp().menu_button.set_menu_model(Some(&hamburger));

        // Embed the zoom slider into the hamburger PopoverMenu
        if let Some(popover) = obj
            .imp()
            .menu_button
            .popover()
            .and_downcast::<gtk4::PopoverMenu>()
        {
            let zoom_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
            zoom_box.set_margin_top(6);
            zoom_box.set_margin_bottom(6);
            zoom_box.set_margin_start(12);
            zoom_box.set_margin_end(12);

            let zoom_out = gtk4::Button::from_icon_name("zoom-out-symbolic");
            zoom_out.set_action_name(Some("win.zoom-out"));
            zoom_out.add_css_class("flat");
            zoom_out.add_css_class("circular");
            zoom_out.set_valign(gtk4::Align::Center);

            let zoom_scale = gtk4::Scale::new(
                gtk4::Orientation::Horizontal,
                Some(&self.zoom_adjustment),
            );
            zoom_scale.set_hexpand(true);
            zoom_scale.set_draw_value(false);
            zoom_scale.set_width_request(140);
            zoom_scale.set_round_digits(0);

            let zoom_in = gtk4::Button::from_icon_name("zoom-in-symbolic");
            zoom_in.set_action_name(Some("win.zoom-in"));
            zoom_in.add_css_class("flat");
            zoom_in.add_css_class("circular");
            zoom_in.set_valign(gtk4::Align::Center);

            zoom_box.append(&zoom_out);
            zoom_box.append(&zoom_scale);
            zoom_box.append(&zoom_in);

            self.zoom_adjustment.connect_value_changed(glib::clone!(
                #[weak]
                obj,
                move |adj| {
                    let level = adj.value().round() as i32;
                    if level != obj.imp().zoom_level.get() {
                        obj.imp().zoom_level.set(level);
                        obj.apply_zoom();
                        obj.save_zoom();
                    }
                }
            ));

            popover.add_child(&zoom_box, "zoom-controls");
        }

        // Restore persisted settings
        if let Some(app) = obj.application().and_downcast::<crate::application::WrenApplication>() {
            let (w, h) = app.window_size();
            obj.set_default_size(w, h);
            obj.imp().show_hidden.set(app.show_hidden());
            let show_ext = app.show_extensions();
            obj.imp().show_extensions.set(show_ext);
            if let Some(action) = obj
                .lookup_action("toggle-extensions")
                .and_downcast::<gio::SimpleAction>()
            {
                action.set_state(&show_ext.to_variant());
            }
            let zl = app.zoom_level();
            obj.imp().zoom_level.set(zl);
            obj.imp().zoom_adjustment.set_value(zl as f64);
        }

        // File-operation popover: a vertical Box inside the op_button's
        // Popover. One row per active op is appended/removed via op_start /
        // op_finish. The button itself uses an AdwSpinner so the user can see
        // at a glance that something is in progress.
        {
            let imp = obj.imp();
            imp.op_popover_box.set_spacing(10);
            imp.op_popover_box.set_margin_top(8);
            imp.op_popover_box.set_margin_bottom(8);
            imp.op_popover_box.set_margin_start(8);
            imp.op_popover_box.set_margin_end(8);
            let popover = gtk4::Popover::new();
            popover.set_child(Some(&imp.op_popover_box));
            imp.op_button.set_popover(Some(&popover));
            let spinner = adw::Spinner::new();
            imp.op_button.set_child(Some(&spinner));
        }

        // Sidebar toggle button — keep split_view and button in sync
        obj.imp().sidebar_button.connect_toggled(glib::clone!(
            #[weak]
            obj,
            move |btn| {
                obj.imp().split_view.set_show_sidebar(btn.is_active());
            }
        ));
        obj.imp().split_view.connect_show_sidebar_notify(glib::clone!(
            #[weak]
            obj,
            move |sv| {
                obj.imp().sidebar_button.set_active(sv.shows_sidebar());
            }
        ));

        // Auto-collapse sidebar on narrow windows
        let bp = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            700.0,
            adw::LengthUnit::Sp,
        ));
        bp.add_setter(&*obj.imp().split_view, "collapsed", Some(&true.to_value()));
        obj.add_breakpoint(bp);

        obj.imp().split_view.connect_collapsed_notify(move |sv| {
            if sv.is_collapsed() {
                sv.set_show_sidebar(false);
            }
        });

        // Window-level capture click: dismiss path entry when clicking outside breadcrumb bar
        let dismiss_gesture = gtk4::GestureClick::new();
        dismiss_gesture.set_button(0);
        dismiss_gesture.set_propagation_phase(gtk4::PropagationPhase::Capture);
        dismiss_gesture.connect_pressed(glib::clone!(
            #[weak]
            obj,
            move |_, _, x, y| {
                let imp = obj.imp();
                let bar = imp.breadcrumb_bar.upcast_ref::<gtk4::Widget>();
                let in_bar = obj
                    .pick(x, y, gtk4::PickFlags::DEFAULT)
                    .map_or(false, |w| w == *bar || w.is_ancestor(bar));
                if !in_bar {
                    imp.breadcrumb_bar.leave_edit_mode();
                }
            }
        ));
        obj.add_controller(dismiss_gesture);

        // Mouse back/forward button navigation (buttons 8 and 9)
        let mouse_nav = gtk4::GestureClick::new();
        mouse_nav.set_button(0);
        mouse_nav.set_propagation_phase(gtk4::PropagationPhase::Capture);
        mouse_nav.connect_pressed(glib::clone!(
            #[weak]
            obj,
            move |gesture, _, _, _| {
                let btn = gesture.current_button();
                if btn == 8 {
                    let _ = gtk4::prelude::WidgetExt::activate_action(&obj, "win.navigate-back", None);
                    gesture.set_state(gtk4::EventSequenceState::Claimed);
                } else if btn == 9 {
                    let _ = gtk4::prelude::WidgetExt::activate_action(&obj, "win.navigate-forward", None);
                    gesture.set_state(gtk4::EventSequenceState::Claimed);
                }
            }
        ));
        obj.add_controller(mouse_nav);

        obj.setup_typeahead();
        obj.setup_search();
        obj.setup_volume_monitor();
        // Now that the sidebar is rooted in the window we can render the
        // Recent section pulled from settings; populate_places ran during
        // template construction when `self.root()` was still None.
        obj.imp().sidebar.reload_recents();
        obj.update_selection_actions();
        obj.update_undo_actions();

        // Restore tabs from the previous session if any of them still
        // exist on disk; fall back to last_directory, then $HOME.
        let app = obj
            .application()
            .and_downcast::<crate::application::WrenApplication>();
        let saved_tabs: Vec<gio::File> = app
            .as_ref()
            .map(|a| a.last_tabs())
            .unwrap_or_default()
            .into_iter()
            .map(|uri| gio::File::for_uri(&uri))
            .filter(|f| f.query_exists(gio::Cancellable::NONE))
            .collect();

        if !saved_tabs.is_empty() {
            for file in &saved_tabs {
                obj.add_tab(file.clone());
            }
            // Restore which tab was active. Clamp because the saved
            // index may be out of range if some tabs disappeared.
            if let Some(a) = &app {
                let idx = a.last_tab_index().clamp(0, (saved_tabs.len() as i32) - 1);
                obj.activate_tab_at(idx as usize);
            }
        } else {
            let initial = app
                .map(|a| a.last_directory())
                .filter(|s| !s.is_empty())
                .map(|uri| gio::File::for_uri(&uri))
                .filter(|f| f.query_exists(gio::Cancellable::NONE))
                .unwrap_or_else(|| gio::File::for_path(glib::home_dir()));
            obj.add_tab(initial);
        }
    }
}

impl WidgetImpl for WrenWindow {}
impl WindowImpl for WrenWindow {}
impl ApplicationWindowImpl for WrenWindow {}
impl AdwApplicationWindowImpl for WrenWindow {}

#[gtk4::template_callbacks]
impl WrenWindow {}
