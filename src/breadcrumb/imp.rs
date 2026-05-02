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
    /// Suggestions list. Lives inside the WrenWindow's overlay panel
    /// (set up by WrenWindow::setup_path_suggestions). We keep a
    /// reference here so refresh_suggestions can rebuild rows.
    pub suggest_list: RefCell<Option<gtk4::ListBox>>,
    /// The Box that floats over the file view (a child of the
    /// window's GtkOverlay). Toggled visible/invisible from
    /// refresh_suggestions / hide_suggestions.
    pub suggest_panel: RefCell<Option<gtk4::Box>>,
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

        // No GtkPopover — the suggestions list lives in the WrenWindow's
        // root_overlay so we control positioning, sizing, and chrome
        // entirely through CSS on a regular GtkBox. The list itself is
        // built lazily in attach_suggestions_to_overlay (called by
        // WrenWindow once both widgets exist in the same hierarchy).
        let list = gtk4::ListBox::new();
        list.set_selection_mode(gtk4::SelectionMode::Single);
        list.set_can_focus(false);

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
                let panel = imp.suggest_panel.borrow();
                if let (Some(list), Some(panel)) = (list.as_ref(), panel.as_ref()) {
                    if panel.is_visible() {
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
                let panel_visible = imp
                    .suggest_panel
                    .borrow()
                    .as_ref()
                    .is_some_and(|p| p.is_visible());
                match key {
                    gtk4::gdk::Key::Escape => {
                        if panel_visible {
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
                    gtk4::gdk::Key::Down if panel_visible => {
                        imp.move_selection(1);
                        glib::Propagation::Stop
                    }
                    gtk4::gdk::Key::Up if panel_visible => {
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
        // The suggest list / panel are owned by the WrenWindow's
        // root_overlay, not by us — nothing to unparent here.
        self.obj().first_child().map(|child| child.unparent());
    }
}

impl WrenBreadcrumbBar {
    pub fn refresh_suggestions(&self, entry: &gtk4::Entry) {
        let Some(list) = self.suggest_list.borrow().clone() else { return };
        let Some(panel) = self.suggest_panel.borrow().clone() else { return };

        let raw = entry.text().to_string();
        let matches = super::list_completions(&raw, 50);

        // Length of the user's typed prefix in the *expanded* form —
        // this is what the matched paths begin with after tilde
        // expansion, so it's the right offset for prefix-dimming.
        let typed_len = super::expanded_typed_len(&raw);

        // Capture the previously-highlighted name so we can restore the
        // user's selection after rebuild — without this, every
        // keystroke snaps the highlight back to row 0 even when the
        // user just arrowed down.
        let prev = list
            .selected_row()
            .map(|r| r.widget_name().to_string());

        while let Some(child) = list.first_child() {
            list.remove(&child);
        }

        if matches.is_empty() {
            panel.set_visible(false);
            return;
        }

        for (name, is_dir) in &matches {
            let row = gtk4::ListBoxRow::new();
            row.set_widget_name(name);
            row.set_can_focus(false);
            // Nautilus-style inline completion: each row shows the
            // *full* path that this match would expand to, with the
            // already-typed prefix dimmed and the completion bright.
            // No icon, no extra chrome — just one line of text.
            let label = gtk4::Label::new(None);
            label.set_xalign(0.0);
            label.set_ellipsize(gtk4::pango::EllipsizeMode::Start);
            label.set_hexpand(true);
            label.set_use_markup(true);
            label.set_margin_start(12);
            label.set_margin_end(12);
            label.set_margin_top(6);
            label.set_margin_bottom(6);
            let full = super::full_completion_path(&raw, name, *is_dir)
                .unwrap_or_else(|| name.clone());
            label.set_markup(&format_completion_markup(&full, typed_len));
            row.set_child(Some(&label));
            list.append(&row);
        }

        // Restore prior highlight if its row still exists, else pick
        // the first row so Enter feels responsive.
        let mut restored = false;
        if let Some(prev_name) = prev {
            let mut child = list.first_child();
            while let Some(c) = child.clone() {
                if let Some(r) = c.downcast_ref::<gtk4::ListBoxRow>() {
                    if r.widget_name().as_str() == prev_name {
                        list.select_row(Some(r));
                        restored = true;
                        break;
                    }
                }
                child = c.next_sibling();
            }
        }
        if !restored {
            if let Some(first) = list.row_at_index(0) {
                list.select_row(Some(&first));
            }
        }
        if !panel.is_visible() {
            panel.set_visible(true);
        }
        // Trigger an overlay re-allocation so the panel re-positions
        // to the entry's current bounds (window resize, scroll, etc.).
        if let Some(parent) = panel.parent() {
            parent.queue_resize();
        }
    }

    pub fn hide_suggestions(&self) {
        if let Some(panel) = self.suggest_panel.borrow().clone() {
            panel.set_visible(false);
        }
    }

    pub fn move_selection(&self, delta: i32) {
        let Some(list) = self.suggest_list.borrow().clone() else { return };
        let mut n = 0i32;
        let mut child = list.first_child();
        while let Some(c) = child.clone() {
            n += 1;
            child = c.next_sibling();
        }
        if n == 0 { return; }
        let cur = list.selected_row().map(|r| r.index()).unwrap_or(-1);
        let next = (cur + delta).rem_euclid(n);
        if let Some(row) = list.row_at_index(next) {
            list.select_row(Some(&row));
            // ListBoxRow inherits set_can_focus(false) from the rows
            // we built above, so this scrolls without yanking focus
            // off the entry.
            row.grab_focus();
        }
    }
}

impl WidgetImpl for WrenBreadcrumbBar {}

// Build Pango markup for a single completion suggestion: dim the
// already-typed prefix, leave the rest at full opacity. `typed_len`
// is in bytes (Pango works on bytes for indices).
fn format_completion_markup(full: &str, typed_len: usize) -> String {
    let split = typed_len.min(full.len());
    let (prefix, suffix) = full.split_at(split);
    format!(
        "<span alpha='55%'>{}</span>{}",
        glib::markup_escape_text(prefix),
        glib::markup_escape_text(suffix),
    )
}
