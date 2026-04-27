use adw::subclass::prelude::*;
use glib::Object;
use gtk4::gdk;
use gtk4::prelude::*;

use crate::file_view::row::WrenFileRow;
use crate::model::FileObject;

glib::wrapper! {
    pub struct WrenFileList(ObjectSubclass<imp::WrenFileList>)
        @extends gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl Default for WrenFileList {
    fn default() -> Self {
        Self::new()
    }
}

impl WrenFileList {
    pub fn new() -> Self {
        Object::builder().build()
    }

    pub fn set_model(&self, model: &gtk4::MultiSelection) {
        let imp = imp::WrenFileList::from_obj(self);
        imp.list_view.set_model(Some(model));
    }

    pub fn setup_drag_source(&self) {
        let imp = imp::WrenFileList::from_obj(self);
        let drag = gtk4::DragSource::new();
        drag.set_actions(gdk::DragAction::COPY | gdk::DragAction::MOVE);
        drag.connect_prepare(move |drag_src, _, _| {
            let view = drag_src.widget()?.downcast::<gtk4::ListView>().ok()?;
            let model = view.model()?.downcast::<gtk4::MultiSelection>().ok()?;
            let bitset = model.selection();
            let uris: Vec<String> = (0..bitset.size())
                .filter_map(|i| {
                    let pos = bitset.nth(i as u32);
                    model
                        .item(pos)
                        .and_downcast::<FileObject>()
                        .map(|obj| obj.file().uri().to_string())
                })
                .collect();
            if uris.is_empty() {
                return None;
            }
            let uri_list = uris.join("\r\n") + "\r\n";
            Some(gdk::ContentProvider::for_bytes(
                "text/uri-list",
                &glib::Bytes::from(uri_list.as_bytes()),
            ))
        });
        imp.list_view.add_controller(drag);
    }

    pub fn setup_context_menu(&self, menu: &gio::MenuModel) {
        let imp = imp::WrenFileList::from_obj(self);
        let popover = gtk4::PopoverMenu::from_model(Some(menu));
        popover.set_has_arrow(false);
        popover.set_parent(&imp.list_view);
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3);
        gesture.connect_pressed(move |_, _, x, y| {
            popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
                x as i32, y as i32, 1, 1,
            )));
            popover.popup();
        });
        imp.list_view.add_controller(gesture);
    }

    pub fn connect_item_activated<F: Fn(&FileObject) + 'static>(&self, f: F) {
        let imp = imp::WrenFileList::from_obj(self);
        imp.list_view.connect_activate(move |list_view, pos| {
            if let Some(obj) = list_view
                .model()
                .and_then(|m| m.item(pos))
                .and_downcast::<FileObject>()
            {
                f(&obj);
            }
        });
    }
}

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct WrenFileList {
        pub list_view: gtk4::ListView,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WrenFileList {
        const NAME: &'static str = "WrenFileList";
        type Type = super::WrenFileList;
        type ParentType = gtk4::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_layout_manager_type::<gtk4::BinLayout>();
        }
    }

    impl ObjectImpl for WrenFileList {
        fn constructed(&self) {
            self.parent_constructed();

            let factory = gtk4::SignalListItemFactory::new();

            factory.connect_setup(|_, obj| {
                let list_item = obj.downcast_ref::<gtk4::ListItem>().unwrap();
                let row = WrenFileRow::new();
                list_item.set_child(Some(&row));
            });

            factory.connect_bind(|_, obj| {
                let list_item = obj.downcast_ref::<gtk4::ListItem>().unwrap();
                let file_obj = list_item
                    .item()
                    .and_downcast::<FileObject>()
                    .expect("item must be FileObject");
                let row = list_item
                    .child()
                    .and_downcast::<WrenFileRow>()
                    .expect("child must be WrenFileRow");
                row.bind(&file_obj);
            });

            factory.connect_unbind(|_, obj| {
                let list_item = obj.downcast_ref::<gtk4::ListItem>().unwrap();
                if let Some(row) = list_item.child().and_downcast::<WrenFileRow>() {
                    row.unbind();
                }
            });

            self.list_view.set_factory(Some(&factory));
            self.list_view.set_enable_rubberband(true);
            self.list_view.set_vexpand(true);
            self.list_view.set_hexpand(true);
            self.list_view.set_overflow(gtk4::Overflow::Hidden);

            let scrolled = gtk4::ScrolledWindow::new();
            scrolled.set_child(Some(&self.list_view));
            scrolled.set_vexpand(true);
            scrolled.set_hexpand(true);
            scrolled.set_kinetic_scrolling(true);
            scrolled.set_overflow(gtk4::Overflow::Hidden);
            scrolled.set_parent(&*self.obj());
        }

        fn dispose(&self) {
            self.obj().first_child().map(|child| child.unparent());
        }
    }

    impl WidgetImpl for WrenFileList {}
}
