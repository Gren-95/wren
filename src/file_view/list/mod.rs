use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

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

fn make_row_factory(cut_uris: Rc<RefCell<HashSet<String>>>) -> gtk4::SignalListItemFactory {
    let factory = gtk4::SignalListItemFactory::new();
    factory.connect_setup(|_, obj| {
        let list_item = obj.downcast_ref::<gtk4::ListItem>().unwrap();
        list_item.set_child(Some(&WrenFileRow::new()));
    });
    factory.connect_bind(move |_, obj| {
        let list_item = obj.downcast_ref::<gtk4::ListItem>().unwrap();
        let file_obj = list_item
            .item()
            .and_downcast::<FileObject>()
            .expect("item must be FileObject");
        let row = list_item
            .child()
            .and_downcast::<WrenFileRow>()
            .expect("child must be WrenFileRow");
        let is_cut = cut_uris.borrow().contains(&file_obj.file().uri().to_string());
        row.bind(&file_obj);
        if is_cut {
            row.set_opacity(0.5);
        }
    });
    factory.connect_unbind(|_, obj| {
        let list_item = obj.downcast_ref::<gtk4::ListItem>().unwrap();
        if let Some(row) = list_item.child().and_downcast::<WrenFileRow>() {
            row.set_opacity(1.0);
            row.unbind();
        }
    });
    factory
}

impl WrenFileList {
    pub fn new() -> Self {
        Object::builder().build()
    }

    pub fn set_model(&self, model: &gtk4::MultiSelection) {
        let imp = imp::WrenFileList::from_obj(self);
        imp.list_view.set_model(Some(model));
    }

    pub fn set_cut_uris(&self, uris: &[String]) {
        let imp = imp::WrenFileList::from_obj(self);
        let mut set = imp.cut_uris.borrow_mut();
        set.clear();
        set.extend(uris.iter().cloned());
        drop(set);
        imp.list_view
            .set_factory(Some(&make_row_factory(Rc::clone(&imp.cut_uris))));
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

    #[derive(Debug)]
    pub struct WrenFileList {
        pub list_view: gtk4::ListView,
        pub cut_uris: Rc<RefCell<HashSet<String>>>,
    }

    impl Default for WrenFileList {
        fn default() -> Self {
            Self {
                list_view: Default::default(),
                cut_uris: Rc::new(RefCell::new(HashSet::new())),
            }
        }
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

            self.list_view
                .set_factory(Some(&super::make_row_factory(Rc::clone(&self.cut_uris))));
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
