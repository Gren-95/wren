use adw::subclass::prelude::*;
use glib::subclass::InitializingObject;
use gtk4::prelude::*;
use gtk4::{CompositeTemplate, TemplateChild};

#[derive(Debug, Default, CompositeTemplate)]
#[template(resource = "/io/github/wren/ui/file_row.ui")]
pub struct WrenFileRow {
    #[template_child]
    pub icon: TemplateChild<gtk4::Image>,
    #[template_child]
    pub symlink_badge: TemplateChild<gtk4::Image>,
    #[template_child]
    pub name: TemplateChild<gtk4::Label>,
    #[template_child]
    pub content_type: TemplateChild<gtk4::Label>,
    #[template_child]
    pub size: TemplateChild<gtk4::Label>,
    #[template_child]
    pub modified: TemplateChild<gtk4::Label>,

    pub icon_size: std::cell::Cell<u32>,
    pub bound_file: std::cell::RefCell<Option<crate::model::FileObject>>,
}

#[glib::object_subclass]
impl ObjectSubclass for WrenFileRow {
    const NAME: &'static str = "WrenFileRow";
    type Type = super::WrenFileRow;
    type ParentType = gtk4::Widget;

    fn class_init(klass: &mut Self::Class) {
        klass.bind_template();
        klass.set_layout_manager_type::<gtk4::BinLayout>();
    }

    fn instance_init(obj: &InitializingObject<Self>) {
        obj.init_template();
    }
}

impl ObjectImpl for WrenFileRow {
    fn dispose(&self) {
        self.obj().first_child().map(|child| child.unparent());
    }
}

impl WidgetImpl for WrenFileRow {}
