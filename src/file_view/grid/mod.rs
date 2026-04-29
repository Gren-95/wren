use std::cell::{Cell, RefCell};
use std::collections::HashMap;
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

// (path, pixel_size) → WrenFileCell; cleaned up on unbind.
type BoundCells = Rc<RefCell<HashMap<usize, glib::WeakRef<WrenFileCell>>>>;

fn make_cell_factory(
    icon_size: Rc<Cell<u32>>,
    cut_uris: Rc<RefCell<std::collections::HashSet<String>>>,
    show_extensions: bool,
    bound_cells: BoundCells,
) -> gtk4::SignalListItemFactory {
    let factory = gtk4::SignalListItemFactory::new();

    factory.connect_setup(|_, obj| {
        let list_item = obj.downcast_ref::<gtk4::ListItem>().unwrap();
        list_item.set_child(Some(&WrenFileCell::new()));
    });

    {
        let bound_cells = Rc::clone(&bound_cells);
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

            let key = cell.as_ptr() as usize;
            bound_cells.borrow_mut().insert(key, cell.downgrade());

            let px = icon_size.get();
            let is_cut = cut_uris.borrow().contains(&file_obj.file().uri().to_string());
            cell.bind(&file_obj, px, show_extensions);
            if is_cut {
                cell.set_opacity(0.5);
            }
            if file_obj.is_hidden() {
                cell.add_css_class("wren-hidden-file");
            }
        });
    }

    {
        let bound_cells = Rc::clone(&bound_cells);
        factory.connect_unbind(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk4::ListItem>().unwrap();
            if let Some(cell) = list_item.child().and_downcast::<WrenFileCell>() {
                let key = cell.as_ptr() as usize;
                bound_cells.borrow_mut().remove(&key);
                cell.set_opacity(1.0);
                cell.remove_css_class("wren-hidden-file");
                cell.unbind();
            }
        });
    }

    factory
}

impl WrenFileGrid {
    pub fn new() -> Self {
        Object::builder().build()
    }

    pub fn set_model(&self, model: &gtk4::MultiSelection) {
        imp::WrenFileGrid::from_obj(self).grid_view.set_model(Some(model));
    }

    /// Zoom: update icon size in all currently-bound cells directly.
    /// No factory replacement — avoids creating/destroying hundreds of widgets.
    pub fn set_icon_size(&self, icon_size: u32) {
        let imp = imp::WrenFileGrid::from_obj(self);
        imp.icon_size.set(icon_size);
        imp.bound_cells.borrow_mut().retain(|_, weak| {
            if let Some(cell) = weak.upgrade() {
                cell.set_icon_size(icon_size);
                true
            } else {
                false
            }
        });
        imp.grid_view.queue_resize();
    }

    pub fn set_cut_uris(&self, uris: &[String]) {
        let imp = imp::WrenFileGrid::from_obj(self);
        let mut set = imp.cut_uris.borrow_mut();
        set.clear();
        set.extend(uris.iter().cloned());
        drop(set);
        imp.grid_view.set_factory(Some(&make_cell_factory(
            Rc::clone(&imp.icon_size),
            Rc::clone(&imp.cut_uris),
            imp.show_extensions.get(),
            Rc::clone(&imp.bound_cells),
        )));
    }

    pub fn set_show_extensions(&self, show: bool) {
        let imp = imp::WrenFileGrid::from_obj(self);
        imp.show_extensions.set(show);
        imp.grid_view.set_factory(Some(&make_cell_factory(
            Rc::clone(&imp.icon_size),
            Rc::clone(&imp.cut_uris),
            show,
            Rc::clone(&imp.bound_cells),
        )));
    }

    pub fn setup_drag_source(&self) {
        let imp = imp::WrenFileGrid::from_obj(self);
        let drag = gtk4::DragSource::new();
        drag.set_actions(gdk::DragAction::COPY | gdk::DragAction::MOVE);
        drag.connect_prepare(move |drag_src, _, _| {
            let view = drag_src.widget()?.downcast::<gtk4::GridView>().ok()?;
            let model = view.model()?.downcast::<gtk4::MultiSelection>().ok()?;
            let bitset = model.selection();
            let files: Vec<gio::File> = (0..bitset.size())
                .filter_map(|i| {
                    let pos = bitset.nth(i as u32);
                    model
                        .item(pos)
                        .and_downcast::<FileObject>()
                        .map(|obj| obj.file().clone())
                })
                .collect();
            if files.is_empty() {
                return None;
            }
            let uri_list = files.iter()
                .map(|f| f.uri().to_string())
                .collect::<Vec<_>>()
                .join("\r\n") + "\r\n";
            let bytes_provider = gdk::ContentProvider::for_bytes(
                "text/uri-list",
                &glib::Bytes::from(uri_list.as_bytes()),
            );
            let file_list = gdk::FileList::from_array(&files);
            let filelist_provider = gdk::ContentProvider::for_value(&file_list.to_value());
            Some(gdk::ContentProvider::new_union(&[bytes_provider, filelist_provider]))
        });
        imp.grid_view.add_controller(drag);
    }

    pub fn setup_empty_area_click(&self) {
        let imp = imp::WrenFileGrid::from_obj(self);
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(1);
        gesture.connect_pressed(glib::clone!(
            #[weak(rename_to = gv)]
            imp.grid_view,
            move |_, _, x, y| {
                let on_item = gv
                    .pick(x, y, gtk4::PickFlags::NON_TARGETABLE)
                    .map_or(false, |w| {
                        let mut cur: Option<gtk4::Widget> = Some(w);
                        while let Some(widget) = cur {
                            if widget.is::<WrenFileCell>() { return true; }
                            if widget.is::<gtk4::GridView>() { return false; }
                            cur = widget.parent();
                        }
                        false
                    });
                if !on_item {
                    if let Some(model) = gv.model().and_downcast::<gtk4::MultiSelection>() {
                        model.unselect_all();
                    }
                }
            }
        ));
        imp.grid_view.add_controller(gesture);
    }

    pub fn setup_drop_target(&self) {
        let imp = imp::WrenFileGrid::from_obj(self);
        let drop = gtk4::DropTarget::new(
            gdk::FileList::static_type(),
            gdk::DragAction::COPY | gdk::DragAction::MOVE,
        );
        drop.connect_drop(glib::clone!(
            #[weak(rename_to = gv)]
            imp.grid_view,
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
                if let Some(win) = gv.root().and_downcast::<crate::window::WrenWindow>() {
                    win.drop_files(files, is_move);
                }
                true
            }
        ));
        imp.grid_view.add_controller(drop);
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

    pub fn scroll_to_top(&self) {
        let imp = imp::WrenFileGrid::from_obj(self);
        if let Some(adj) = imp.grid_view.vadjustment() {
            adj.set_value(adj.lower());
        }
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
        pub icon_size: Rc<Cell<u32>>,
        pub cut_uris: Rc<RefCell<std::collections::HashSet<String>>>,
        pub show_extensions: Cell<bool>,
        pub bound_cells: BoundCells,
    }

    impl Default for WrenFileGrid {
        fn default() -> Self {
            Self {
                grid_view: Default::default(),
                icon_size: Rc::new(Cell::new(64)),
                cut_uris: Rc::new(RefCell::new(std::collections::HashSet::new())),
                show_extensions: Cell::new(true),
                bound_cells: Rc::new(RefCell::new(HashMap::new())),
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
                Rc::clone(&self.icon_size),
                Rc::clone(&self.cut_uris),
                true,
                Rc::clone(&self.bound_cells),
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
