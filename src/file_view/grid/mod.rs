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
        let cell = WrenFileCell::new();

        // DragSource on the cell rather than the view so it fires before the
        // view's rubber-band gesture — pressing on a cell starts a drag;
        // pressing on empty space starts rubber-band selection.
        let drag = gtk4::DragSource::new();
        drag.set_actions(gdk::DragAction::COPY | gdk::DragAction::MOVE);
        // Capture phase: claim the gesture before GridView's selection
        // gesture has a chance to mess with the selection on press.
        drag.set_propagation_phase(gtk4::PropagationPhase::Capture);
        drag.connect_prepare(|drag_src, _x, _y| {
            let cell = drag_src.widget().and_downcast::<WrenFileCell>()?;
            let this_file = cell.bound_file_object()?.file().clone();

            let mut w: Option<gtk4::Widget> = cell.parent();
            let grid_view = loop {
                match w {
                    Some(ref p) if p.is::<gtk4::GridView>() => {
                        break p.clone().downcast::<gtk4::GridView>().ok()?;
                    }
                    Some(ref p) => w = p.parent(),
                    None => return None,
                }
            };
            let model = grid_view.model()?.downcast::<gtk4::MultiSelection>().ok()?;
            let bitset = model.selection();

            // Locate the clicked cell in the model.
            let n = model.n_items();
            let mut this_pos: Option<u32> = None;
            for i in 0..n {
                if let Some(obj) = model.item(i).and_downcast::<FileObject>() {
                    if obj.file().equal(&this_file) {
                        this_pos = Some(i);
                        break;
                    }
                }
            }

            // If the clicked cell is part of the current selection, drag the
            // whole selection. Otherwise drag just this one file (and update
            // the selection so it reflects what's being dragged).
            let files: Vec<gio::File> = match this_pos {
                Some(pos) if bitset.contains(pos) => (0..bitset.size())
                    .filter_map(|i| {
                        model.item(bitset.nth(i as u32))
                            .and_downcast::<FileObject>()
                            .map(|obj| obj.file().clone())
                    })
                    .collect(),
                Some(pos) => {
                    model.select_item(pos, true);
                    vec![this_file]
                }
                None => vec![this_file],
            };

            if files.is_empty() { return None; }
            let uri_list = files.iter()
                .map(|f| f.uri().to_string())
                .collect::<Vec<_>>()
                .join("\r\n") + "\r\n";
            let bytes = gdk::ContentProvider::for_bytes(
                "text/uri-list",
                &glib::Bytes::from(uri_list.as_bytes()),
            );
            let filelist = gdk::ContentProvider::for_value(
                &gdk::FileList::from_array(&files).to_value(),
            );
            Some(gdk::ContentProvider::new_union(&[bytes, filelist]))
        });
        drag.connect_drag_begin(|drag_src, _| {
            if let Some(widget) = drag_src.widget() {
                let paintable = gtk4::WidgetPaintable::new(Some(&widget));
                let w = widget.width();
                let h = widget.height();
                drag_src.set_icon(Some(&paintable), w / 2, h / 2);
            }
        });
        cell.add_controller(drag);

        list_item.set_child(Some(&cell));
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

        let highlighted: Rc<RefCell<Option<WrenFileCell>>> = Rc::new(RefCell::new(None));

        let clear_highlight = {
            let highlighted = Rc::clone(&highlighted);
            move || {
                if let Some(cell) = highlighted.borrow_mut().take() {
                    cell.remove_css_class("wren-drop-hover");
                }
            }
        };

        drop.connect_motion(glib::clone!(
            #[weak(rename_to = gv)]
            imp.grid_view,
            #[strong]
            highlighted,
            #[upgrade_or]
            gdk::DragAction::empty(),
            move |_, x, y| {
                let folder_cell = gv
                    .pick(x, y, gtk4::PickFlags::NON_TARGETABLE)
                    .and_then(|w| {
                        let mut cur: Option<gtk4::Widget> = Some(w);
                        while let Some(widget) = cur {
                            if let Some(c) = widget.downcast_ref::<WrenFileCell>() {
                                return c.bound_file_object()
                                    .filter(|f| f.is_directory())
                                    .map(|_| c.clone());
                            }
                            if widget.is::<gtk4::GridView>() { return None; }
                            cur = widget.parent();
                        }
                        None
                    });
                let mut prev = highlighted.borrow_mut();
                let same = match (prev.as_ref(), folder_cell.as_ref()) {
                    (Some(a), Some(b)) => a.as_ptr() == b.as_ptr(),
                    (None, None) => true,
                    _ => false,
                };
                if !same {
                    if let Some(old) = prev.take() {
                        old.remove_css_class("wren-drop-hover");
                    }
                    if let Some(ref new_cell) = folder_cell {
                        new_cell.add_css_class("wren-drop-hover");
                    }
                    *prev = folder_cell;
                }
                gdk::DragAction::COPY | gdk::DragAction::MOVE
            }
        ));

        drop.connect_leave(move |_| clear_highlight());

        drop.connect_drop(glib::clone!(
            #[weak(rename_to = gv)]
            imp.grid_view,
            #[strong]
            highlighted,
            #[upgrade_or]
            false,
            move |drop_target, value, x, y| {
                if let Some(cell) = highlighted.borrow_mut().take() {
                    cell.remove_css_class("wren-drop-hover");
                }
                let Ok(file_list) = value.get::<gdk::FileList>() else {
                    return false;
                };
                let files = file_list.files();
                if files.is_empty() {
                    return false;
                }
                // If the pointer is over a folder, drop into it; otherwise into
                // the current directory.
                let folder_dest = gv
                    .pick(x, y, gtk4::PickFlags::NON_TARGETABLE)
                    .and_then(|w| {
                        let mut cur: Option<gtk4::Widget> = Some(w);
                        while let Some(widget) = cur {
                            if let Some(cell) = widget.downcast_ref::<WrenFileCell>() {
                                return cell.bound_file_object().and_then(|f| {
                                    if f.is_directory() { Some(f.file().clone()) } else { None }
                                });
                            }
                            if widget.is::<gtk4::GridView>() { return None; }
                            cur = widget.parent();
                        }
                        None
                    });
                let action = drop_target
                    .current_drop()
                    .map(|d| d.actions())
                    .unwrap_or(gdk::DragAction::COPY);
                let is_move = !action.contains(gdk::DragAction::COPY)
                    && action.contains(gdk::DragAction::MOVE);
                if let Some(win) = gv.root().and_downcast::<crate::window::WrenWindow>() {
                    win.drop_files(files, folder_dest, is_move);
                }
                true
            }
        ));
        imp.grid_view.add_controller(drop);
    }

    pub fn setup_context_menu(&self, menu: &gio::MenuModel) {
        let imp = imp::WrenFileGrid::from_obj(self);
        // Stash the model for use in the gesture handler. We rebuild
        // the popover on every right-click — the Nautilus pattern —
        // because reusing a single PopoverMenu across right-clicks
        // sometimes shows stale submenu state, and parenting to the
        // outer composite widget (rather than the inner GridView) gives
        // the popover correct measurement context.
        imp.context_menu_model.replace(Some(menu.clone()));
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3);
        gesture.connect_pressed(glib::clone!(
            #[weak(rename_to = view)] self,
            move |_, _, x, y| view.popup_context_menu(x, y)
        ));
        imp.grid_view.add_controller(gesture);
    }

    fn popup_context_menu(&self, x: f64, y: f64) {
        let imp = imp::WrenFileGrid::from_obj(self);
        let Some(model) = imp.context_menu_model.borrow().clone() else { return };
        // Drop the previous popover via unparent before building the
        // new one. Mirrors `g_clear_pointer(&p, gtk_widget_unparent)`.
        if let Some(old) = imp.context_popover.take() {
            old.unparent();
        }
        let popover = gtk4::PopoverMenu::from_model(Some(&model));
        popover.set_has_arrow(false);
        popover.set_parent(self);
        // The gesture's x,y are in grid_view coordinates; translate
        // into our own coordinate space because that's what
        // set_pointing_to expects when parented on `self`.
        let p = imp
            .grid_view
            .compute_point(self, &gtk4::graphene::Point::new(x as f32, y as f32))
            .unwrap_or_else(|| gtk4::graphene::Point::new(x as f32, y as f32));
        let (px, py) = (p.x() as f64, p.y() as f64);
        popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
            px as i32,
            py as i32,
            1,
            1,
        )));
        popover.popup();
        imp.context_popover.replace(Some(popover));
    }

    pub fn scroll_to_top(&self) {
        let imp = imp::WrenFileGrid::from_obj(self);
        if let Some(adj) = imp.grid_view.vadjustment() {
            adj.set_value(adj.lower());
        }
    }

    /// Scroll-and-focus a position in the underlying GridView. Used by
    /// type-ahead select to bring the matched item into view.
    pub fn scroll_to(&self, pos: u32, flags: gtk4::ListScrollFlags) {
        let imp = imp::WrenFileGrid::from_obj(self);
        imp.grid_view.scroll_to(pos, flags, None);
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
        pub context_menu_model: RefCell<Option<gio::MenuModel>>,
        pub context_popover: RefCell<Option<gtk4::PopoverMenu>>,
    }

    impl Default for WrenFileGrid {
        fn default() -> Self {
            Self {
                grid_view: Default::default(),
                icon_size: Rc::new(Cell::new(64)),
                cut_uris: Rc::new(RefCell::new(std::collections::HashSet::new())),
                show_extensions: Cell::new(true),
                bound_cells: Rc::new(RefCell::new(HashMap::new())),
                context_menu_model: RefCell::new(None),
                context_popover: RefCell::new(None),
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
            // Rubber-band selection disabled: it competes with the per-cell
            // DragSource and shows a brief selection rect when starting a drag
            // from a selected cell. Multi-select via Ctrl/Shift+click.
            self.grid_view.set_enable_rubberband(false);
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
            if let Some(popover) = self.context_popover.take() {
                popover.unparent();
            }
            self.obj().first_child().map(|child| child.unparent());
        }
    }

    impl WidgetImpl for WrenFileGrid {}
}
