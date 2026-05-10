use std::cell::RefCell;

use adw::subclass::prelude::*;
use glib::subclass::InitializingObject;
use gtk4::prelude::*;
use gtk4::{CompositeTemplate, TemplateChild};

#[derive(Debug, Default, CompositeTemplate)]
#[template(resource = "/io/github/wren/ui/sidebar.ui")]
pub struct WrenSidebar {
    #[template_child]
    pub list_box: TemplateChild<gtk4::ListBox>,
    pub place_uris: RefCell<Vec<String>>,
    pub n_static_rows: std::cell::Cell<i32>,
    pub volume_monitor_handlers: RefCell<Vec<glib::SignalHandlerId>>,
}

#[glib::object_subclass]
impl ObjectSubclass for WrenSidebar {
    const NAME: &'static str = "WrenSidebar";
    type Type = super::WrenSidebar;
    type ParentType = gtk4::Widget;

    fn class_init(klass: &mut Self::Class) {
        klass.bind_template();
        klass.set_layout_manager_type::<gtk4::BinLayout>();
    }

    fn instance_init(obj: &InitializingObject<Self>) {
        obj.init_template();
    }
}

impl ObjectImpl for WrenSidebar {
    fn constructed(&self) {
        self.parent_constructed();
        let obj = self.obj();
        obj.populate_places();
        obj.connect_volume_monitor();
    }

    fn dispose(&self) {
        // Disconnect VolumeMonitor signal handlers before tearing down,
        // otherwise the singleton monitor may invoke a handler holding a
        // weak ref to a destroyed sidebar (handlers themselves use weak
        // refs, but it's still cleanest to remove them explicitly).
        let monitor = gio::VolumeMonitor::get();
        for id in self.volume_monitor_handlers.borrow_mut().drain(..) {
            monitor.disconnect(id);
        }
        self.obj().first_child().map(|child| child.unparent());
    }
}

impl WidgetImpl for WrenSidebar {}

#[gtk4::template_callbacks]
impl WrenSidebar {}
