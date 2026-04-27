use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use adw::subclass::prelude::*;
use glib::Object;
use gtk4::gdk;
use gtk4::prelude::*;

use crate::file_view::cell::WrenFileCell;
use crate::model::FileObject;

glib::wrapper! {
    pub struct WrenFileGrid(ObjectSubclass<imp::WrenFileGrid>)
        @extends gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl Default for WrenFileGrid {
    fn default() -> Self {
        Self::new()
    }
}

fn make_cell_factory(
    icon_size: u32,
    cut_uris: Rc<RefCell<HashSet<String>>>,
) -> gtk4::SignalListItemFactory {
    let factory = gtk4::SignalListItemFactory::new();
    factory.connect_setup(|_, obj| {
        let list_item = obj.downcast_ref::<gtk4::ListItem>().unwrap();
        list_item.set_child(Some(&WrenFileCell::new()));
    });
    factory.connect_bind(move |_, obj| {
        let list_item = obj.downcast_ref::<gtk4::ListItem>().unwrap();
        let file_obj = list_item
            .item()
            .and_downcast::<FileObject>()
            .expect("item must be FileObject");
        let cell = list_item
            .child()
            .and_downcast::<WrenFileCell>()
            .expect("child must be WrenFileCell");
        let is_cut = cut_uris.borrow().contains(&file_obj.file().uri().to_string());
        cell.bind(&file_obj, icon_size);
        if is_cut {
            cell.set_opacity(0.5);
        }
    });
    factory.connect_unbind(|_, obj| {
        let list_item = obj.downcast_ref::<gtk4::ListItem>().unwrap();
        if let Some(cell) = list_item.child().and_downcast::<WrenFileCell>() {
            cell.set_opacity(1.0);
            cell.unbind();
        }
    });
    factory
}

impl WrenFileGrid {
    pub fn new() -> Self {
        Object::builder().build()
    }

    pub fn set_model(&self, model: &gtk4::MultiSelection) {
        imp::WrenFileGrid::from_obj(self).grid_view.set_model(Some(model));
    }

    pub fn set_icon_size(&self, icon_size: u32) {
        let imp = imp::WrenFileGrid::from_obj(self);
        imp.current_icon_size.set(icon_size);
        imp.grid_view
            .set_factory(Some(&make_cell_factory(icon_size, Rc::clone(&imp.cut_uris))));
    }

    pub fn set_cut_uris(&self, uris: &[String]) {
        let imp = imp::WrenFileGrid::from_obj(self);
        let mut set = imp.cut_uris.borrow_mut();
        set.clear();
        set.extend(uris.iter().cloned());
        drop(set);
        let icon_size = imp.current_icon_size.get();
        imp.grid_view
            .set_factory(Some(&make_cell_factory(icon_size, Rc::clone(&imp.cut_uris))));
    }

    pub fn setup_drag_source(&self) {
        let imp = imp::WrenFileGrid::from_obj(self);
        let drag = gtk4::DragSource::new();
        drag.set_actions(gdk::DragAction::COPY | gdk::DragAction::MOVE);
        drag.connect_prepare(move |drag_src, _, _| {
            let view = drag_src.widget()?.downcast::<gtk4::GridView>().ok()?;
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
        imp.grid_view.add_controller(drag);
    }

    pub fn setup_context_menu(&self, menu: &gio::MenuModel) {
        let imp = imp::WrenFileGrid::from_obj(self);
        let popover = gtk4::PopoverMenu::from_model(Some(menu));
        popover.set_has_arrow(false);
        popover.set_parent(&imp.grid_view);
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3);
        gesture.connect_pressed(move |_, _, x, y| {
            popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
                x as i32, y as i32, 1, 1,
            )));
            popover.popup();
        });
        imp.grid_view.add_controller(gesture);
    }

    pub fn connect_item_activated<F: Fn(&FileObject) + 'static>(&self, f: F) {
        let imp = imp::WrenFileGrid::from_obj(self);
        imp.grid_view.connect_activate(move |grid_view, pos| {
            if let Some(obj) = grid_view
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
    pub struct WrenFileGrid {
        pub grid_view: gtk4::GridView,
        pub current_icon_size: Cell<u32>,
        pub cut_uris: Rc<RefCell<HashSet<String>>>,
    }

    impl Default for WrenFileGrid {
        fn default() -> Self {
            Self {
                grid_view: Default::default(),
                current_icon_size: Cell::new(64),
                cut_uris: Rc::new(RefCell::new(HashSet::new())),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WrenFileGrid {
        const NAME: &'static str = "WrenFileGrid";
        type Type = super::WrenFileGrid;
        type ParentType = gtk4::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_layout_manager_type::<gtk4::BinLayout>();
        }
    }

    impl ObjectImpl for WrenFileGrid {
        fn constructed(&self) {
            self.parent_constructed();

            self.grid_view.set_factory(Some(&super::make_cell_factory(
                64,
                Rc::clone(&self.cut_uris),
            )));
            self.grid_view.set_min_columns(2);
            self.grid_view.set_max_columns(16);
            self.grid_view.set_enable_rubberband(true);
            self.grid_view.set_vexpand(true);
            self.grid_view.set_hexpand(true);
            self.grid_view.set_overflow(gtk4::Overflow::Hidden);

            let scroll = gtk4::EventControllerScroll::new(
                gtk4::EventControllerScrollFlags::VERTICAL,
            );
            scroll.connect_scroll(move |ctrl, _dx, dy| {
                let mods = ctrl.current_event_state();
                if mods.contains(gdk::ModifierType::CONTROL_MASK) {
                    if let Some(win) = ctrl
                        .widget()
                        .and_then(|w| w.root())
                        .and_downcast::<crate::window::WrenWindow>()
                    {
                        if dy < 0.0 {
                            win.zoom_in();
                        } else {
                            win.zoom_out();
                        }
                    }
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
            self.grid_view.add_controller(scroll);

            let scrolled = gtk4::ScrolledWindow::new();
            scrolled.set_child(Some(&self.grid_view));
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

    impl WidgetImpl for WrenFileGrid {}
}
