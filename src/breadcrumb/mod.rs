mod imp;

use adw::subclass::prelude::*;
use glib::Object;
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

            let name = ancestor
                .basename()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "/".to_string());

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
                crumb_box.append(&btn);

                let sep = gtk4::Image::from_icon_name("go-next-symbolic");
                sep.set_pixel_size(12);
                sep.add_css_class("wren-crumb-sep");
                crumb_box.append(&sep);
            }
        }

        // Scroll to the end so the current directory is always visible
        let scrolled = crumb_box
            .parent()
            .and_downcast::<gtk4::ScrolledWindow>();
        if let Some(sw) = scrolled {
            glib::idle_add_local_once(move || {
                let adj = sw.hadjustment();
                adj.set_value(adj.upper() - adj.page_size());
            });
        }

        imp.mode_stack.set_visible_child_name("crumbs");
    }

    pub fn enter_edit_mode(&self) {
        let imp = self.imp();
        let path_text = imp
            .current_location
            .borrow()
            .as_ref()
            .and_then(|f| f.path())
            .map(|p| p.to_string_lossy().into_owned())
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
        let file = if text.starts_with("file://") {
            gio::File::for_uri(text)
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
