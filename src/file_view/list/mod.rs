use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
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

type BoundRows = Rc<RefCell<HashMap<usize, glib::WeakRef<WrenFileRow>>>>;

fn make_row_factory(
    icon_size: Rc<Cell<u32>>,
    cut_uris: Rc<RefCell<HashSet<String>>>,
    show_extensions: bool,
    bound_rows: BoundRows,
) -> gtk4::SignalListItemFactory {
    let factory = gtk4::SignalListItemFactory::new();
    factory.connect_setup(|_, obj| {
        let list_item = obj.downcast_ref::<gtk4::ListItem>().unwrap();
        list_item.set_child(Some(&WrenFileRow::new()));
    });
    {
        let bound_rows = Rc::clone(&bound_rows);
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

            let key = row.as_ptr() as usize;
            bound_rows.borrow_mut().insert(key, row.downgrade());

            let is_cut = cut_uris.borrow().contains(&file_obj.file().uri().to_string());
            row.bind(&file_obj, icon_size.get(), show_extensions);
            if is_cut {
                row.set_opacity(0.5);
            }
            if file_obj.is_hidden() {
                row.add_css_class("wren-hidden-file");
            }
        });
    }
    {
        let bound_rows = Rc::clone(&bound_rows);
        factory.connect_unbind(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk4::ListItem>().unwrap();
            if let Some(row) = list_item.child().and_downcast::<WrenFileRow>() {
                let key = row.as_ptr() as usize;
                bound_rows.borrow_mut().remove(&key);
                row.set_opacity(1.0);
                row.remove_css_class("wren-hidden-file");
                row.unbind();
            }
        });
    }
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

    pub fn set_icon_size(&self, icon_size: u32) {
        let imp = imp::WrenFileList::from_obj(self);
        imp.icon_size.set(icon_size);
        imp.bound_rows.borrow_mut().retain(|_, weak| {
            if let Some(row) = weak.upgrade() {
                row.set_icon_size(icon_size);
                true
            } else {
                false
            }
        });
        // Keep header spacer aligned: icon_size + row's inter-column spacing (8px)
        imp.header_icon_spacer.set_size_request((icon_size + 8) as i32, -1);
        imp.list_view.queue_resize();
    }

    pub fn set_cut_uris(&self, uris: &[String]) {
        let imp = imp::WrenFileList::from_obj(self);
        let mut set = imp.cut_uris.borrow_mut();
        set.clear();
        set.extend(uris.iter().cloned());
        drop(set);
        imp.list_view.set_factory(Some(&make_row_factory(
            Rc::clone(&imp.icon_size),
            Rc::clone(&imp.cut_uris),
            imp.show_extensions.get(),
            Rc::clone(&imp.bound_rows),
        )));
    }

    pub fn set_show_extensions(&self, show: bool) {
        let imp = imp::WrenFileList::from_obj(self);
        imp.show_extensions.set(show);
        imp.list_view.set_factory(Some(&make_row_factory(
            Rc::clone(&imp.icon_size),
            Rc::clone(&imp.cut_uris),
            show,
            Rc::clone(&imp.bound_rows),
        )));
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

    pub fn setup_empty_area_click(&self) {
        let imp = imp::WrenFileList::from_obj(self);
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(1);
        gesture.connect_pressed(glib::clone!(
            #[weak(rename_to = lv)]
            imp.list_view,
            move |_, _, x, y| {
                let on_item = lv
                    .pick(x, y, gtk4::PickFlags::NON_TARGETABLE)
                    .map_or(false, |w| {
                        let mut cur: Option<gtk4::Widget> = Some(w);
                        while let Some(widget) = cur {
                            if widget.is::<WrenFileRow>() {
                                return true;
                            }
                            if widget.is::<gtk4::ListView>() {
                                return false;
                            }
                            cur = widget.parent();
                        }
                        false
                    });
                if !on_item {
                    if let Some(model) = lv.model().and_downcast::<gtk4::MultiSelection>() {
                        model.unselect_all();
                    }
                }
            }
        ));
        imp.list_view.add_controller(gesture);
    }

    pub fn scroll_to_top(&self) {
        let imp = imp::WrenFileList::from_obj(self);
        if let Some(adj) = imp.list_view.vadjustment() {
            adj.set_value(adj.lower());
        }
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

    pub fn set_sort_state(&self, key: &str, reversed: bool) {
        let imp = imp::WrenFileList::from_obj(self);
        let buttons = imp.sort_buttons.borrow();
        let col_keys = ["name", "type", "size", "date"];
        let col_labels = ["Name", "Type", "Size", "Modified"];
        for (i, (btn_key, btn_label)) in col_keys.iter().zip(col_labels.iter()).enumerate() {
            if let Some(btn) = buttons.get(i) {
                if *btn_key == key {
                    let arrow = if reversed { " ↑" } else { " ↓" };
                    btn.set_label(&format!("{}{}", btn_label, arrow));
                    btn.add_css_class("wren-sort-active");
                } else {
                    btn.set_label(btn_label);
                    btn.remove_css_class("wren-sort-active");
                }
            }
        }
    }
}

mod imp {
    use super::*;

    #[derive(Debug)]
    pub struct WrenFileList {
        pub list_view: gtk4::ListView,
        pub cut_uris: Rc<RefCell<HashSet<String>>>,
        pub show_extensions: Cell<bool>,
        pub sort_buttons: RefCell<Vec<gtk4::Button>>,
        pub icon_size: Rc<Cell<u32>>,
        pub bound_rows: BoundRows,
        pub header_icon_spacer: gtk4::Box,
    }

    impl Default for WrenFileList {
        fn default() -> Self {
            Self {
                list_view: Default::default(),
                cut_uris: Rc::new(RefCell::new(HashSet::new())),
                show_extensions: Cell::new(true),
                sort_buttons: RefCell::new(Vec::new()),
                icon_size: Rc::new(Cell::new(24)),
                bound_rows: Rc::new(RefCell::new(HashMap::new())),
                header_icon_spacer: gtk4::Box::new(gtk4::Orientation::Horizontal, 0),
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

            self.list_view.set_factory(Some(&super::make_row_factory(
                Rc::clone(&self.icon_size),
                Rc::clone(&self.cut_uris),
                true,
                Rc::clone(&self.bound_rows),
            )));
            self.list_view.set_enable_rubberband(true);
            self.list_view.set_vexpand(true);
            self.list_view.set_hexpand(true);
            self.list_view.set_overflow(gtk4::Overflow::Hidden);

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
                        if dy < 0.0 { win.zoom_in(); } else { win.zoom_out(); }
                    }
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
            self.list_view.add_controller(scroll);

            // ── Column header row ─────────────────────────────────────────
            let header_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
            header_box.add_css_class("wren-list-header");
            header_box.set_margin_start(8);
            header_box.set_margin_end(8);

            // Spacer width = icon_size + row's inter-column spacing (8px)
            self.header_icon_spacer.set_size_request(32, -1);
            header_box.append(&self.header_icon_spacer);

            // (label, sort_key, hexpand, width_request, right_align)
            let cols: &[(&str, &str, bool, i32, bool)] = &[
                ("Name",     "name", true,  -1,  false),
                ("Type",     "type", false, 120, true),
                ("Size",     "size", false, 80,  true),
                ("Modified", "date", false, 120, true),
            ];

            let mut sort_buttons = self.sort_buttons.borrow_mut();
            for &(label, key, expand, width, right_align) in cols {
                let btn = gtk4::Button::with_label(label);
                btn.add_css_class("flat");
                if expand {
                    btn.set_hexpand(true);
                } else {
                    btn.set_width_request(width);
                }
                if right_align {
                    if let Some(lbl) = btn.child().and_downcast::<gtk4::Label>() {
                        lbl.set_xalign(1.0);
                    }
                }
                let key = key.to_string();
                btn.connect_clicked(move |btn| {
                    let _ = btn.activate_action(
                        "win.set-sort-key",
                        Some(&key.to_variant()),
                    );
                });
                header_box.append(&btn);
                sort_buttons.push(btn);
            }
            drop(sort_buttons);

            let scrolled = gtk4::ScrolledWindow::new();
            scrolled.set_child(Some(&self.list_view));
            scrolled.set_vexpand(true);
            scrolled.set_hexpand(true);
            scrolled.set_kinetic_scrolling(true);
            scrolled.set_overflow(gtk4::Overflow::Hidden);

            let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
            vbox.append(&header_box);
            vbox.append(&scrolled);
            vbox.set_parent(&*self.obj());
        }

        fn dispose(&self) {
            self.obj().first_child().map(|child| child.unparent());
        }
    }

    impl WidgetImpl for WrenFileList {}
}
