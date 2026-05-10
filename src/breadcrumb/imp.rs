// GtkEntryCompletion + GtkListStore + GtkCellRendererText are
// deprecated in GTK 4.10 but Nautilus still uses them on `main`
// (late 2025), and there's no maintained native replacement for
// the input-with-attached-completion pattern. Allowing the
// deprecation warnings module-wide for the same reason.
#![allow(deprecated)]

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
    /// GtkListStore feeding the entry's GtkEntryCompletion. Single
    /// G_TYPE_STRING column holding pre-built full path completions
    /// — same shape as Nautilus' priv->completions_store.
    pub completion_store: RefCell<Option<gtk4::ListStore>>,
    /// The cell renderer; we need a handle so refresh_completions can
    /// update its `attributes` PangoAttrList for prefix dimming.
    pub completion_cell: RefCell<Option<gtk4::CellRendererText>>,
    /// Up/Down navigation state through the persisted path history.
    /// `None` means the user hasn't started navigating yet — first Up
    /// captures the current entry text into `history_pending` and shows
    /// history[0]. Resets to None on any non-arrow key.
    pub history_index: RefCell<Option<usize>>,
    /// Text the user had typed before pressing Up; restored when they
    /// walk past the most-recent entry with Down.
    pub history_pending: RefCell<String>,
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

        // ── GtkEntryCompletion (Nautilus pattern) ────────────────────
        // Verbatim port of nautilus_location_entry_init's completion
        // setup (src/nautilus-location-entry.c). GtkEntryCompletion
        // is deprecated in GTK 4.10 but Nautilus still uses it on
        // main, so it's the closest thing to a stable native pattern
        // for "input with attached completion popup".
        #[allow(deprecated)]
        {
            let store = gtk4::ListStore::new(&[String::static_type()]);
            let completion = gtk4::EntryCompletion::new();
            completion.set_model(Some(&store));
            completion.set_text_column(0);
            completion.set_inline_completion(false);
            completion.set_inline_selection(true);
            completion.set_popup_single_match(true);

            let cell = gtk4::CellRendererText::new();
            cell.set_property("xpad", 6_u32);
            completion.pack_start(&cell, false);
            completion.add_attribute(&cell, "text", 0);

            self.path_entry.set_completion(Some(&completion));
            self.completion_store.replace(Some(store));
            self.completion_cell.replace(Some(cell));
        }

        // Update completion model + dimming on every text change.
        // User typing also resets the history navigation index — only
        // Up/Down (which set the text via set_text after stashing the
        // index) should leave it set. We do that reset in the key
        // controller below by gating which keys we consider "typing".
        self.path_entry.connect_changed(glib::clone!(
            #[weak(rename_to = imp_self)] self.obj(),
            move |entry| {
                imp_self.imp().refresh_completions(entry);
            }
        ));

        // Enter without a selection committed → navigate to typed path.
        // GtkEntryCompletion's match-selected handles Enter when a row
        // is highlighted; we just handle the no-selection case here.
        self.path_entry.connect_activate(glib::clone!(
            #[weak] obj,
            move |entry| {
                let text = entry.text().to_string();
                obj.navigate_to_text(&text);
            }
        ));

        // Escape leaves edit mode (when the popup is open the
        // completion machinery eats Escape itself; this fires only
        // when the popup is closed). Tab triggers path-component
        // completion (see handle_tab_completion). Up/Down walk the
        // persisted MRU history (pending-buffer pattern: the first
        // Up stashes whatever the user had typed; Down past index 0
        // restores it).
        let key_ctrl = gtk4::EventControllerKey::new();
        key_ctrl.connect_key_pressed(glib::clone!(
            #[weak] obj,
            #[upgrade_or] glib::Propagation::Proceed,
            move |_, key, _, _| {
                if key == gtk4::gdk::Key::Escape {
                    obj.imp().history_index.replace(None);
                    obj.leave_edit_mode();
                    glib::Propagation::Stop
                } else if key == gtk4::gdk::Key::Tab {
                    obj.imp().history_index.replace(None);
                    obj.imp().handle_tab_completion();
                    glib::Propagation::Stop
                } else if key == gtk4::gdk::Key::Up {
                    obj.imp().history_step(-1);
                    glib::Propagation::Stop
                } else if key == gtk4::gdk::Key::Down {
                    obj.imp().history_step(1);
                    glib::Propagation::Stop
                } else {
                    // Any other key is "typing" — fall back to default
                    // entry handling and reset history navigation so
                    // the next Up starts from a fresh pending buffer.
                    obj.imp().history_index.replace(None);
                    glib::Propagation::Proceed
                }
            }
        ));
        self.path_entry.add_controller(key_ctrl);

        // While the path entry has focus, disable the window-level
        // clipboard actions so Ctrl+C/X/V hit the entry natively.
        let focus_ctrl = gtk4::EventControllerFocus::new();
        focus_ctrl.connect_enter(glib::clone!(
            #[weak] obj,
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
            #[weak] obj,
            move |_| {
                obj.imp().history_index.replace(None);
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
        self.obj().first_child().map(|child| child.unparent());
    }
}

impl WrenBreadcrumbBar {
    /// Walk the persisted path-bar history. `delta` is -1 for Up
    /// (older), +1 for Down (newer). Implements the pending-buffer
    /// pattern: the first Up stashes the current entry text into
    /// `history_pending` and shows history[0]; subsequent Ups walk
    /// back; Down walks forward; one step past index 0 restores the
    /// pending text and clears the navigation state.
    pub fn history_step(&self, delta: i32) {
        let Some(win) = self.obj()
            .root()
            .and_then(|r| r.downcast::<crate::window::WrenWindow>().ok())
        else { return };
        let Some(app) = win
            .application()
            .and_downcast::<crate::application::WrenApplication>()
        else { return };
        let history = app.path_history();
        if history.is_empty() {
            return;
        }

        let entry = &*self.path_entry;
        let current = self.history_index.borrow().clone();

        let new_index: Option<i32> = match (current, delta) {
            (None, -1) => {
                self.history_pending.replace(entry.text().to_string());
                Some(0)
            }
            (None, _) => return,
            (Some(i), -1) => {
                let next = (i as i32 + 1).min(history.len() as i32 - 1);
                Some(next)
            }
            (Some(i), 1) => {
                let next = i as i32 - 1;
                if next < 0 { None } else { Some(next) }
            }
            _ => return,
        };

        match new_index {
            Some(i) => {
                self.history_index.replace(Some(i as usize));
                entry.set_text(&history[i as usize]);
            }
            None => {
                self.history_index.replace(None);
                let pending = self.history_pending.borrow().clone();
                entry.set_text(&pending);
            }
        }
        entry.set_position(-1);
    }

    /// Repopulate the completion model from the current entry text and
    /// re-apply prefix dimming. Mirrors the body of Nautilus'
    /// `completer_get_completions_thread` + `set_prefix_dimming`,
    /// minus the GTask threading (sync `std::fs::read_dir` is fast
    /// enough for typical directory sizes).
    pub fn refresh_completions(&self, entry: &gtk4::Entry) {
        #[allow(deprecated)]
        let Some(store) = self.completion_store.borrow().clone() else { return };
        let Some(cell) = self.completion_cell.borrow().clone() else { return };

        let raw = entry.text().to_string();
        let typed_path = super::typed_path_for_completion(&raw);
        let matches = super::list_completions(&raw, 200);

        // 1. Rebuild the store. Each row holds the FULL path
        //    (typed_path + basename + trailing / for dirs) — Nautilus'
        //    `completion = g_strconcat(typed_path, name_slash, NULL)`.
        store.clear();
        for (name, is_dir) in &matches {
            let mut full = typed_path.clone();
            full.push_str(name);
            if *is_dir {
                full.push('/');
            }
            store.set(&store.append(), &[(0, &full)]);
        }

        // 2. Prefix dimming. Pango alpha attribute spans bytes
        //    [0, typed_path.len()) — same trick as
        //    nautilus' set_prefix_dimming (alpha 36045 = 55%).
        let attrs = gtk4::pango::AttrList::new();
        let mut dim = gtk4::pango::AttrInt::new_foreground_alpha(36045);
        dim.set_start_index(0);
        dim.set_end_index(typed_path.len() as u32);
        attrs.insert(dim);
        cell.set_property("attributes", &attrs);
    }

    /// Tab-key handler for the path entry. Completes the trailing
    /// path component to the longest unambiguous directory name; on
    /// multiple matches it also asks the completion popup to surface.
    /// Tab is always swallowed (Propagation::Stop in the caller) — a
    /// 0-match Tab must not move focus out of the entry.
    pub fn handle_tab_completion(&self) {
        let entry = &*self.path_entry;
        let raw = entry.text().to_string();
        let typed_path = super::typed_path_for_completion(&raw);
        let partial = &raw[typed_path.len()..];

        let matches: Vec<(String, bool)> = super::list_completions(&raw, 1000)
            .into_iter()
            .filter(|(_, is_dir)| *is_dir)
            .collect();

        if matches.is_empty() {
            return;
        }

        if matches.len() == 1 {
            let mut full = typed_path;
            full.push_str(&matches[0].0);
            full.push('/');
            entry.set_text(&full);
            entry.set_position(-1);
            return;
        }

        let lcp = longest_common_prefix(matches.iter().map(|(n, _)| n.as_str()));
        if lcp.chars().count() > partial.chars().count() {
            let mut full = typed_path;
            full.push_str(&lcp);
            entry.set_text(&full);
            entry.set_position(-1);
        }

        #[allow(deprecated)]
        if let Some(completion) = entry.completion() {
            completion.complete();
        }
    }
}

/// Longest common prefix of an iterator of strings, computed at char
/// boundaries (so we never split a multi-byte UTF-8 codepoint). Empty
/// input → empty string.
fn longest_common_prefix<'a, I: IntoIterator<Item = &'a str>>(iter: I) -> String {
    let mut iter = iter.into_iter();
    let Some(first) = iter.next() else { return String::new() };
    let mut prefix: String = first.to_string();
    for s in iter {
        let new_len = prefix
            .chars()
            .zip(s.chars())
            .take_while(|(a, b)| a == b)
            .count();
        let byte_len: usize = prefix.chars().take(new_len).map(|c| c.len_utf8()).sum();
        prefix.truncate(byte_len);
        if prefix.is_empty() {
            break;
        }
    }
    prefix
}

impl WidgetImpl for WrenBreadcrumbBar {}
