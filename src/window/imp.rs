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

    pub tabs: RefCell<Vec<TabState>>,
    pub clipboard_files: RefCell<Option<(Vec<gio::File>, bool)>>,
    pub show_hidden: Cell<bool>,
    pub show_extensions: Cell<bool>,
    pub zoom_level: Cell<i32>,
    pub zoom_adjustment: gtk4::Adjustment,
    pub undo_stack: RefCell<Vec<UndoOp>>,
    pub redo_stack: RefCell<Vec<UndoOp>>,
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
            tabs: Default::default(),
            clipboard_files: Default::default(),
            show_hidden: Default::default(),
            show_extensions: Cell::new(true),
            zoom_level: Cell::new(3),
            zoom_adjustment: gtk4::Adjustment::new(3.0, 1.0, 5.0, 1.0, 1.0, 0.0),
            undo_stack: Default::default(),
            redo_stack: Default::default(),
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

        klass.install_action("win.navigate-back", None, |win, _, _| {
            win.navigate_back();
        });
        klass.install_action("win.navigate-forward", None, |win, _, _| {
            win.navigate_forward();
        });
        klass.install_action("win.navigate-up", None, |win, _, _| {
            win.navigate_up();
        });
        klass.install_action("win.toggle-search", None, |win, _, _| {
            win.toggle_search();
        });
        klass.install_action("win.new-tab", None, |win, _, _| {
            win.new_tab();
        });
        klass.install_action("win.close-tab", None, |win, _, _| {
            win.close_tab();
        });
        klass.install_action("win.select-all", None, |win, _, _| {
            win.select_all();
        });
        klass.install_action("win.open-settings", None, |win, _, _| {
            win.open_settings();
        });
        klass.install_action("win.focus-location", None, |win, _, _| {
            win.focus_location();
        });
        klass.install_action("win.open-selection", None, |win, _, _| {
            win.open_selection();
        });
        klass.install_action("win.open-with", None, |win, _, _| {
            win.open_with();
        });
        klass.install_action("win.open-in-terminal", None, |win, _, _| {
            win.open_in_terminal();
        });
        klass.install_action("win.new-folder", None, |win, _, _| {
            win.new_folder();
        });
        klass.install_action("win.rename", None, |win, _, _| {
            win.rename_selection();
        });
        klass.install_action("win.move-to-trash", None, |win, _, _| {
            win.move_to_trash();
        });
        klass.install_action("win.delete-permanently", None, |win, _, _| {
            win.delete_permanently();
        });
        klass.install_action("win.copy", None, |win, _, _| {
            win.copy_selection();
        });
        klass.install_action("win.cut", None, |win, _, _| {
            win.cut_selection();
        });
        klass.install_action("win.paste", None, |win, _, _| {
            win.paste();
        });
        klass.install_action("win.zoom-in", None, |win, _, _| {
            win.zoom_in();
        });
        klass.install_action("win.zoom-out", None, |win, _, _| {
            win.zoom_out();
        });
        klass.install_action("win.zoom-reset", None, |win, _, _| {
            win.zoom_reset();
        });
        klass.install_action("win.properties", None, |win, _, _| {
            win.show_properties();
        });
        klass.install_action("win.create-link", None, |win, _, _| {
            win.create_link();
        });
        klass.install_action("win.add-bookmark", None, |win, _, _| {
            win.add_bookmark();
        });
        klass.install_action("win.undo", None, |win, _, _| {
            win.undo();
        });
        klass.install_action("win.redo", None, |win, _, _| {
            win.redo();
        });
        klass.install_action("win.batch-rename", None, |win, _, _| {
            win.batch_rename();
        });
        klass.install_action("win.duplicate", None, |win, _, _| {
            win.duplicate();
        });
        klass.install_action("win.about", None, |win, _, _| {
            win.show_about();
        });
        klass.install_action("win.reload", None, |win, _, _| {
            win.reload();
        });
        klass.install_action("win.toggle-sidebar", None, |win, _, _| {
            let imp = win.imp();
            let show = !imp.split_view.shows_sidebar();
            imp.split_view.set_show_sidebar(show);
        });
        klass.install_action("win.navigate-home", None, |win, _, _| {
            win.navigate_home();
        });
        klass.install_action("win.new-window", None, |win, _, _| {
            win.new_window();
        });
        klass.install_action("win.copy-path", None, |win, _, _| {
            win.copy_path();
        });
        klass.install_action("win.show-shortcuts", None, |win, _, _| {
            win.show_shortcuts();
        });
        klass.install_action(
            "win.remove-bookmark",
            Some(glib::VariantTy::STRING),
            |win, _, param| {
                if let Some(uri) = param.and_then(|v| v.str()) {
                    win.remove_bookmark(uri);
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

        // Close-page: always confirm (we guard against closing the last tab in close_tab)
        self.tab_view.connect_close_page(|tab_view, page| {
            tab_view.close_page_finish(page, true);
            glib::Propagation::Stop
        });

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

        obj.setup_search();
        obj.setup_volume_monitor();
        obj.update_selection_actions();
        obj.update_undo_actions();

        // Open first tab at home
        let home = gio::File::for_path(glib::home_dir());
        obj.add_tab(home);
    }
}

impl WidgetImpl for WrenWindow {}
impl WindowImpl for WrenWindow {}
impl ApplicationWindowImpl for WrenWindow {}
impl AdwApplicationWindowImpl for WrenWindow {}

#[gtk4::template_callbacks]
impl WrenWindow {}
