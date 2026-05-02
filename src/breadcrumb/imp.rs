use std::cell::RefCell;

use adw::subclass::prelude::*;
use glib::subclass::InitializingObject;
use gtk4::prelude::*;
use gtk4::{CompositeTemplate, TemplateChild};

#[derive(Debug, Default, CompositeTemplate)]
#[template(resource = "/io/github/wren/ui/breadcrumb_bar.ui")]
pub struct WrenBreadcrumbBar {
    #[template_child]
    pub crumb_box: TemplateChild<gtk4::Box>,
    #[template_child]
    pub mode_stack: TemplateChild<gtk4::Stack>,
    #[template_child]
    pub path_entry: TemplateChild<gtk4::Entry>,
    pub current_location: RefCell<Option<gio::File>>,
    /// Suggestions popover lazily created in `constructed`. Shows
    /// matching entries from the parent dir as the user types in the
    /// path entry.
    pub suggest_popover: RefCell<Option<gtk4::Popover>>,
    pub suggest_list: RefCell<Option<gtk4::ListBox>>,
}

#[glib::object_subclass]
impl ObjectSubclass for WrenBreadcrumbBar {
    const NAME: &'static str = "WrenBreadcrumbBar";
    type Type = super::WrenBreadcrumbBar;
    type ParentType = gtk4::Widget;

    fn class_init(klass: &mut Self::Class) {
        klass.bind_template();
        klass.set_layout_manager_type::<gtk4::BinLayout>();
    }

    fn instance_init(obj: &InitializingObject<Self>) {
        obj.init_template();
    }
}

impl ObjectImpl for WrenBreadcrumbBar {
    fn constructed(&self) {
        self.parent_constructed();
        let obj = self.obj();

        // Build the suggestions popover. Anchored below the entry,
        // non-autohiding so the entry keeps focus while the user
        // arrows through entries.
        let popover = gtk4::Popover::new();
        popover.set_autohide(false);
        popover.set_position(gtk4::PositionType::Bottom);
        popover.set_has_arrow(false);
        popover.add_css_class("wren-suggest-popover");

        let scroll = gtk4::ScrolledWindow::new();
        scroll.set_min_content_width(280);
        scroll.set_max_content_width(560);
        scroll.set_max_content_height(280);
        scroll.set_propagate_natural_width(true);
        scroll.set_propagate_natural_height(true);
        scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

        let list = gtk4::ListBox::new();
        list.set_selection_mode(gtk4::SelectionMode::Single);
        list.add_css_class("navigation-sidebar"); // borrowed sidebar styling
        scroll.set_child(Some(&list));
        popover.set_child(Some(&scroll));
        popover.set_parent(&*self.path_entry);

        // Click on a row → fill the entry with that name and continue.
        list.connect_row_activated(glib::clone!(
            #[weak(rename_to = entry)] self.path_entry,
            move |_, row| {
                let name = row.widget_name().to_string();
                if name.is_empty() { return; }
                if let Some(text) = super::apply_completion(&entry.text(), &name) {
                    entry.set_text(&text);
                    entry.set_position(-1);
                }
            }
        ));

        self.suggest_popover.replace(Some(popover.clone()));
        self.suggest_list.replace(Some(list.clone()));

        // Activate (Enter): if a suggestion is highlighted, accept it
        // and refresh; otherwise navigate to the typed path.
        self.path_entry.connect_activate(glib::clone!(
            #[weak]
            obj,
            #[weak(rename_to = imp_self)]
            self.obj(),
            move |entry| {
                let imp = imp_self.imp();
                let list = imp.suggest_list.borrow();
                let popover = imp.suggest_popover.borrow();
                if let (Some(list), Some(popover)) = (list.as_ref(), popover.as_ref()) {
                    if popover.is_visible() {
                        if let Some(row) = list.selected_row() {
                            let name = row.widget_name().to_string();
                            if !name.is_empty() {
                                if let Some(text) =
                                    super::apply_completion(&entry.text(), &name)
                                {
                                    entry.set_text(&text);
                                    entry.set_position(-1);
                                    return;
                                }
                            }
                        }
                    }
                }
                let text = entry.text().to_string();
                obj.navigate_to_text(&text);
            }
        ));

        // Live-update suggestions as the user types.
        self.path_entry.connect_changed(glib::clone!(
            #[weak(rename_to = imp_self)]
            self.obj(),
            move |entry| {
                imp_self.imp().refresh_suggestions(entry);
            }
        ));

        let key_ctrl = gtk4::EventControllerKey::new();
        // Capture phase so we get Tab/arrow keys before GtkEntry's
        // inner GtkText hands them off (Tab → focus traversal,
        // arrows → cursor movement).
        key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);
        key_ctrl.connect_key_pressed(glib::clone!(
            #[weak]
            obj,
            #[weak(rename_to = entry)]
            self.path_entry,
            #[weak(rename_to = imp_self)]
            self.obj(),
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                let imp = imp_self.imp();
                let popover_visible = imp
                    .suggest_popover
                    .borrow()
                    .as_ref()
                    .is_some_and(|p| p.is_visible());
                match key {
                    gtk4::gdk::Key::Escape => {
                        if popover_visible {
                            imp.hide_suggestions();
                            glib::Propagation::Stop
                        } else {
                            obj.leave_edit_mode();
                            glib::Propagation::Stop
                        }
                    }
                    gtk4::gdk::Key::Tab => {
                        super::complete_path(&entry);
                        glib::Propagation::Stop
                    }
                    gtk4::gdk::Key::Down if popover_visible => {
                        imp.move_selection(1);
                        glib::Propagation::Stop
                    }
                    gtk4::gdk::Key::Up if popover_visible => {
                        imp.move_selection(-1);
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            }
        ));
        self.path_entry.add_controller(key_ctrl);

        // While the path entry has focus, disable the window-level clipboard
        // actions so that Ctrl+C/X/V work natively in the entry rather than
        // triggering file copy/cut/paste operations.
        let focus_ctrl = gtk4::EventControllerFocus::new();
        focus_ctrl.connect_enter(glib::clone!(
            #[weak]
            obj,
            move |_| {
                if let Some(win) = obj
                    .root()
                    .and_then(|r| r.downcast::<crate::window::WrenWindow>().ok())
                {
                    win.action_set_enabled("win.copy", false);
                    win.action_set_enabled("win.cut", false);
                    win.action_set_enabled("win.paste", false);
                    win.action_set_enabled("win.select-all", false);
                }
            }
        ));
        focus_ctrl.connect_leave(glib::clone!(
            #[weak]
            obj,
            move |_| {
                obj.leave_edit_mode();
                if let Some(win) = obj
                    .root()
                    .and_then(|r| r.downcast::<crate::window::WrenWindow>().ok())
                {
                    win.action_set_enabled("win.copy", true);
                    win.action_set_enabled("win.cut", true);
                    win.action_set_enabled("win.paste", true);
                    win.action_set_enabled("win.select-all", true);
                    win.update_selection_actions();
                }
            }
        ));
        self.path_entry.add_controller(focus_ctrl);
    }

    fn dispose(&self) {
        if let Some(popover) = self.suggest_popover.take() {
            popover.unparent();
        }
        self.obj().first_child().map(|child| child.unparent());
    }
}

impl WrenBreadcrumbBar {
    pub fn refresh_suggestions(&self, entry: &gtk4::Entry) {
        let Some(list) = self.suggest_list.borrow().clone() else { return };
        let Some(popover) = self.suggest_popover.borrow().clone() else { return };

        let raw = entry.text().to_string();
        let matches = super::list_completions(&raw, 50);

        // Clear previous rows.
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }

        if matches.is_empty() {
            popover.popdown();
            return;
        }

        for (name, is_dir) in &matches {
            let row = gtk4::ListBoxRow::new();
            // Stash the basename on widget-name so the activation
            // handler can read it back without juggling closures.
            row.set_widget_name(name);
            let row_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            row_box.set_margin_start(6);
            row_box.set_margin_end(6);
            row_box.set_margin_top(4);
            row_box.set_margin_bottom(4);
            let icon = gtk4::Image::from_icon_name(if *is_dir {
                "folder-symbolic"
            } else {
                "text-x-generic-symbolic"
            });
            icon.set_pixel_size(16);
            let label = gtk4::Label::new(Some(name));
            label.set_xalign(0.0);
            label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
            label.set_hexpand(true);
            row_box.append(&icon);
            row_box.append(&label);
            row.set_child(Some(&row_box));
            list.append(&row);
        }

        // Pre-select the first row so Enter / arrow keys feel responsive.
        if let Some(first) = list.row_at_index(0) {
            list.select_row(Some(&first));
        }
        if !popover.is_visible() {
            popover.popup();
        }
    }

    pub fn hide_suggestions(&self) {
        if let Some(popover) = self.suggest_popover.borrow().clone() {
            popover.popdown();
        }
    }

    pub fn move_selection(&self, delta: i32) {
        let Some(list) = self.suggest_list.borrow().clone() else { return };
        let n = {
            let mut count = 0;
            let mut child = list.first_child();
            while let Some(c) = child {
                count += 1;
                child = c.next_sibling();
            }
            count
        };
        if n == 0 { return };
        let cur = list.selected_row().map(|r| r.index()).unwrap_or(-1);
        let next = (cur + delta).rem_euclid(n);
        if let Some(row) = list.row_at_index(next) {
            list.select_row(Some(&row));
            // Scroll the row into view.
            row.grab_focus();
            // grab_focus shifts focus away from the entry; redirect
            // back so typing keeps reaching the path entry.
            self.path_entry.grab_focus();
        }
    }
}

impl WidgetImpl for WrenBreadcrumbBar {}
