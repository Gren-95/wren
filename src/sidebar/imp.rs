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
        self.obj().populate_places();
    }

    fn dispose(&self) {
        self.obj().first_child().map(|child| child.unparent());
    }
}

impl WidgetImpl for WrenSidebar {}

#[gtk4::template_callbacks]
impl WrenSidebar {}
