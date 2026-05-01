mod imp;

use adw::subclass::prelude::*;
use glib::Object;
use gtk4::gdk;
use gtk4::prelude::*;

glib::wrapper! {
    pub struct WrenBreadcrumbBar(ObjectSubclass<imp::WrenBreadcrumbBar>)
        @extends gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl Default for WrenBreadcrumbBar {
    fn default() -> Self {
        Self::new()
    }
}

impl WrenBreadcrumbBar {
    pub fn new() -> Self {
        Object::builder().build()
    }

    fn imp(&self) -> &imp::WrenBreadcrumbBar {
        imp::WrenBreadcrumbBar::from_obj(self)
    }

    pub fn set_location(&self, file: &gio::File) {
        let imp = self.imp();
        imp.current_location.replace(Some(file.clone()));

        let crumb_box = &imp.crumb_box;
        while let Some(child) = crumb_box.first_child() {
            crumb_box.remove(&child);
        }

        let mut ancestors: Vec<gio::File> = Vec::new();
        let mut current = Some(file.clone());
        while let Some(f) = current {
            ancestors.push(f.clone());
            current = f.parent();
        }
        ancestors.reverse();

        let total = ancestors.len();
        for (i, ancestor) in ancestors.into_iter().enumerate() {
            let is_last = i == total - 1;

            let name = friendly_name_for(&ancestor);

            if is_last {
                // Current directory — accent chip, click enters edit mode
                let btn = gtk4::Button::with_label(&name);
                btn.add_css_class("flat");
                btn.add_css_class("wren-current-crumb");
                btn.connect_clicked(glib::clone!(
                    #[weak(rename_to = bar)]
                    self,
                    move |_| {
                        bar.enter_edit_mode();
                    }
                ));
                crumb_box.append(&btn);
            } else {
                let btn = gtk4::Button::with_label(&name);
                btn.add_css_class("flat");
                btn.add_css_class("wren-breadcrumb");

                let file_clone = ancestor.clone();
                btn.connect_clicked(move |btn| {
                    if let Some(win) = btn
                        .root()
                        .and_then(|r| r.downcast::<crate::window::WrenWindow>().ok())
                    {
                        win.navigate_to(file_clone.clone());
                    }
                });

                // Middle-click opens the segment location in a new tab
                let file_mid = ancestor.clone();
                let mid_gesture = gtk4::GestureClick::new();
                mid_gesture.set_button(2);
                mid_gesture.connect_pressed(move |gesture, _, _, _| {
                    gesture.set_state(gtk4::EventSequenceState::Claimed);
                    if let Some(win) = gesture
                        .widget()
                        .and_then(|w| w.root())
                        .and_then(|r| r.downcast::<crate::window::WrenWindow>().ok())
                    {
                        win.add_tab(file_mid.clone());
                    }
                });
                btn.add_controller(mid_gesture);

                // Drop target: dragging files onto an ancestor crumb moves/copies
                // them into that folder.
                let drop = gtk4::DropTarget::new(
                    gdk::FileList::static_type(),
                    gdk::DragAction::COPY | gdk::DragAction::MOVE,
                );
                let dest_file = ancestor.clone();
                drop.connect_drop(glib::clone!(
                    #[weak]
                    btn,
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
                        let action = drop_target
                            .current_drop()
                            .map(|d| d.actions())
                            .unwrap_or(gdk::DragAction::COPY);
                        let is_move = !action.contains(gdk::DragAction::COPY)
                            && action.contains(gdk::DragAction::MOVE);
                        if let Some(win) = btn
                            .root()
                            .and_downcast::<crate::window::WrenWindow>()
                        {
                            win.drop_files(files, Some(dest_file.clone()), is_move);
                        }
                        true
                    }
                ));
                btn.add_controller(drop);

                crumb_box.append(&btn);

                // Root already IS "/" — adding a separator would produce "//"
                if name != "/" {
                    let sep = gtk4::Label::new(Some("/"));
                    sep.add_css_class("wren-crumb-sep");
                    crumb_box.append(&sep);
                }
            }
        }

        // Scroll to the end so the current directory is always visible.
        // Connect once to the adjustment's changed signal — it fires after
        // GTK finishes measuring and allocating the new crumbs, at which
        // point upper and page-size have their final values.
        let scrolled = crumb_box
            .parent()
            .and_downcast::<gtk4::ScrolledWindow>();
        if let Some(sw) = scrolled {
            let adj = sw.hadjustment();
            let handler_id = std::rc::Rc::new(std::cell::Cell::new(
                None::<glib::SignalHandlerId>,
            ));
            let handler_id_clone = std::rc::Rc::clone(&handler_id);
            let adj_clone = adj.clone();
            let id = adj.connect_changed(move |a| {
                a.set_value(a.upper() - a.page_size());
                if let Some(id) = handler_id_clone.take() {
                    adj_clone.disconnect(id);
                }
            });
            handler_id.set(Some(id));
        }

        imp.mode_stack.set_visible_child_name("crumbs");
    }

    pub fn enter_edit_mode(&self) {
        let imp = self.imp();
        // Prefer the local path; for virtual filesystems (trash:///, recent:///,
        // sftp://…) f.path() is None — fall back to the full URI so the user
        // can see and copy the location.
        let path_text = imp
            .current_location
            .borrow()
            .as_ref()
            .map(|f| {
                f.path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| f.uri().to_string())
            })
            .unwrap_or_default();
        imp.path_entry.set_text(&path_text);
        imp.mode_stack.set_visible_child_name("entry");
        imp.path_entry.select_region(0, -1);
        imp.path_entry.grab_focus();
    }

    pub fn leave_edit_mode(&self) {
        self.imp().mode_stack.set_visible_child_name("crumbs");
    }

    pub fn navigate_to_text(&self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let file = if has_uri_scheme(text) {
            gio::File::for_uri(text)
        } else if let Some(rest) = text.strip_prefix("~/") {
            let mut p = glib::home_dir();
            p.push(rest);
            gio::File::for_path(p)
        } else if text == "~" {
            gio::File::for_path(glib::home_dir())
        } else {
            gio::File::for_path(text)
        };
        if let Some(win) = self
            .root()
            .and_then(|r| r.downcast::<crate::window::WrenWindow>().ok())
        {
            win.navigate_to(file);
        }
    }
}

// True for strings like "file://x", "trash:///", "recent:///", "smb://...".
// Excludes Windows-style drive letters by requiring at least 2 leading chars.
fn has_uri_scheme(text: &str) -> bool {
    let Some(idx) = text.find(':') else { return false };
    if idx < 2 {
        return false;
    }
    text[..idx]
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
}

fn friendly_name_for(file: &gio::File) -> String {
    let uri = file.uri();
    if uri == "trash:///" {
        return "Trash".to_string();
    }
    if uri == "network:///" {
        return "Network".to_string();
    }
    if uri == "recent:///" {
        return "Recent".to_string();
    }
    file.basename()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/".to_string())
}
