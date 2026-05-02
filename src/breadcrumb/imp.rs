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

        self.path_entry.connect_activate(glib::clone!(
            #[weak]
            obj,
            move |entry| {
                let text = entry.text().to_string();
                obj.navigate_to_text(&text);
            }
        ));

        let key_ctrl = gtk4::EventControllerKey::new();
        key_ctrl.connect_key_pressed(glib::clone!(
            #[weak]
            obj,
            #[weak(rename_to = entry)]
            self.path_entry,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                if key == gtk4::gdk::Key::Escape {
                    obj.leave_edit_mode();
                    glib::Propagation::Stop
                } else if key == gtk4::gdk::Key::Tab {
                    super::complete_path(&entry);
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
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
        self.obj().first_child().map(|child| child.unparent());
    }
}

impl WidgetImpl for WrenBreadcrumbBar {}
