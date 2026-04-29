use adw::subclass::prelude::*;
use glib::subclass::InitializingObject;
use gtk4::prelude::*;
use gtk4::{CompositeTemplate, TemplateChild};

use crate::model::FileObject;

#[derive(Debug, Default, CompositeTemplate)]
#[template(resource = "/io/github/wren/ui/file_cell.ui")]
pub struct WrenFileCell {
    #[template_child]
    pub icon: TemplateChild<gtk4::Image>,
    #[template_child]
    pub name: TemplateChild<gtk4::Label>,

    // Kept across unbind/bind so set_icon_size can re-render without a model signal.
    pub bound_file: std::cell::RefCell<Option<FileObject>>,
    pub icon_size: std::cell::Cell<u32>,
}

#[glib::object_subclass]
impl ObjectSubclass for WrenFileCell {
    const NAME: &'static str = "WrenFileCell";
    type Type = super::WrenFileCell;
    type ParentType = gtk4::Widget;

    fn class_init(klass: &mut Self::Class) {
        klass.bind_template();
        klass.set_layout_manager_type::<gtk4::BinLayout>();
    }

    fn instance_init(obj: &InitializingObject<Self>) {
        obj.init_template();
    }
}

impl ObjectImpl for WrenFileCell {
    fn dispose(&self) {
        self.obj().first_child().map(|child| child.unparent());
    }
}

impl WidgetImpl for WrenFileCell {}
