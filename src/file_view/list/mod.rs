use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use adw::subclass::prelude::*;
use glib::Object;
use gtk4::gdk;
use gtk4::prelude::*;

use crate::file_view::row::WrenFileRow;
use crate::file_view::typeahead::TypeaheadState;
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
    show_hidden: Rc<Cell<bool>>,
    bound_rows: BoundRows,
) -> gtk4::SignalListItemFactory {
    let factory = gtk4::SignalListItemFactory::new();
    factory.connect_setup(|_, obj| {
        let list_item = obj.downcast_ref::<gtk4::ListItem>().unwrap();
        let row = WrenFileRow::new();

        let drag = gtk4::DragSource::new();
        drag.set_actions(gdk::DragAction::COPY | gdk::DragAction::MOVE);
        // Capture phase: claim the gesture before ListView's selection
        // gesture has a chance to mess with the selection on press.
        drag.set_propagation_phase(gtk4::PropagationPhase::Capture);
        drag.connect_prepare(|drag_src, _x, _y| {
            let row = drag_src.widget().and_downcast::<WrenFileRow>()?;
            let this_file = row.bound_file_object()?.file().clone();

            let mut w: Option<gtk4::Widget> = row.parent();
            let list_view = loop {
                match w {
                    Some(ref p) if p.is::<gtk4::ListView>() => {
                        break p.clone().downcast::<gtk4::ListView>().ok()?;
                    }
                    Some(ref p) => w = p.parent(),
                    None => return None,
                }
            };
            let model = list_view.model()?.downcast::<gtk4::MultiSelection>().ok()?;
            let bitset = model.selection();

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
        row.add_controller(drag);

        list_item.set_child(Some(&row));
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
            row.bind(&file_obj, icon_size.get(), show_extensions, show_hidden.get());
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
            Rc::clone(&imp.show_hidden),
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
            Rc::clone(&imp.show_hidden),
            Rc::clone(&imp.bound_rows),
        )));
    }

    /// Update the live show_hidden flag the row factory reads at bind
    /// time. Called by the window when the user toggles `win.toggle-hidden`.
    pub fn set_show_hidden(&self, show: bool) {
        let imp = imp::WrenFileList::from_obj(self);
        imp.show_hidden.set(show);
    }

    /// Force every visible row to rebind. Used after a global preference
    /// change (folder count policy) so the new behaviour shows up without
    /// scrolling.
    pub fn rebind_visible_rows(&self) {
        let imp = imp::WrenFileList::from_obj(self);
        imp.list_view.set_factory(Some(&make_row_factory(
            Rc::clone(&imp.icon_size),
            Rc::clone(&imp.cut_uris),
            imp.show_extensions.get(),
            Rc::clone(&imp.show_hidden),
            Rc::clone(&imp.bound_rows),
        )));
    }

    pub fn setup_drop_target(&self) {
        let imp = imp::WrenFileList::from_obj(self);
        let drop = gtk4::DropTarget::new(
            gdk::FileList::static_type(),
            gdk::DragAction::COPY | gdk::DragAction::MOVE,
        );

        let highlighted: Rc<RefCell<Option<WrenFileRow>>> = Rc::new(RefCell::new(None));

        let clear_highlight = {
            let highlighted = Rc::clone(&highlighted);
            move || {
                if let Some(row) = highlighted.borrow_mut().take() {
                    row.remove_css_class("wren-drop-hover");
                }
            }
        };

        drop.connect_motion(glib::clone!(
            #[weak(rename_to = lv)]
            imp.list_view,
            #[strong]
            highlighted,
            #[upgrade_or]
            gdk::DragAction::empty(),
            move |_, x, y| {
                let folder_row = lv
                    .pick(x, y, gtk4::PickFlags::NON_TARGETABLE)
                    .and_then(|w| {
                        let mut cur: Option<gtk4::Widget> = Some(w);
                        while let Some(widget) = cur {
                            if let Some(r) = widget.downcast_ref::<WrenFileRow>() {
                                return r.bound_file_object()
                                    .filter(|f| f.is_directory())
                                    .map(|_| r.clone());
                            }
                            if widget.is::<gtk4::ListView>() { return None; }
                            cur = widget.parent();
                        }
                        None
                    });
                let mut prev = highlighted.borrow_mut();
                let same = match (prev.as_ref(), folder_row.as_ref()) {
                    (Some(a), Some(b)) => a.as_ptr() == b.as_ptr(),
                    (None, None) => true,
                    _ => false,
                };
                if !same {
                    if let Some(old) = prev.take() {
                        old.remove_css_class("wren-drop-hover");
                    }
                    if let Some(ref new_row) = folder_row {
                        new_row.add_css_class("wren-drop-hover");
                    }
                    *prev = folder_row;
                }
                gdk::DragAction::COPY | gdk::DragAction::MOVE
            }
        ));

        drop.connect_leave(move |_| clear_highlight());

        drop.connect_drop(glib::clone!(
            #[weak(rename_to = lv)]
            imp.list_view,
            #[strong]
            highlighted,
            #[upgrade_or]
            false,
            move |drop_target, value, x, y| {
                if let Some(row) = highlighted.borrow_mut().take() {
                    row.remove_css_class("wren-drop-hover");
                }
                let Ok(file_list) = value.get::<gdk::FileList>() else {
                    return false;
                };
                let files = file_list.files();
                if files.is_empty() {
                    return false;
                }
                let folder_dest = lv
                    .pick(x, y, gtk4::PickFlags::NON_TARGETABLE)
                    .and_then(|w| {
                        let mut cur: Option<gtk4::Widget> = Some(w);
                        while let Some(widget) = cur {
                            if let Some(row) = widget.downcast_ref::<WrenFileRow>() {
                                return row.bound_file_object().and_then(|f| {
                                    if f.is_directory() { Some(f.file().clone()) } else { None }
                                });
                            }
                            if widget.is::<gtk4::ListView>() { return None; }
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
                if let Some(win) = lv.root().and_downcast::<crate::window::WrenWindow>() {
                    win.drop_files(files, folder_dest, is_move);
                }
                true
            }
        ));
        imp.list_view.add_controller(drop);
    }

    pub fn setup_context_menu<F: Fn() -> gio::MenuModel + 'static>(&self, builder: F) {
        let imp = imp::WrenFileList::from_obj(self);
        // See WrenFileGrid::setup_context_menu for rationale on the
        // rebuild-on-each-click pattern.
        imp.context_menu_builder.replace(Some(Box::new(builder)));
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3);
        gesture.connect_pressed(glib::clone!(
            #[weak(rename_to = view)] self,
            move |_, _, x, y| view.popup_context_menu(x, y)
        ));
        imp.list_view.add_controller(gesture);
    }

    fn popup_context_menu(&self, x: f64, y: f64) {
        let imp = imp::WrenFileList::from_obj(self);
        let model = match imp.context_menu_builder.borrow().as_ref() {
            Some(b) => b(),
            None => return,
        };
        if let Some(old) = imp.context_popover.take() {
            old.unparent();
        }
        let popover = gtk4::PopoverMenu::from_model(Some(&model));
        popover.set_has_arrow(false);
        popover.set_parent(self);
        let p = imp
            .list_view
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
        crate::file_view::popover::lock_vertical_only(&popover);
        imp.context_popover.replace(Some(popover));
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

    /// Scroll-and-focus a position in the underlying ListView. Used by
    /// type-ahead select to bring the matched item into view.
    pub fn scroll_to(&self, pos: u32, flags: gtk4::ListScrollFlags) {
        let imp = imp::WrenFileList::from_obj(self);
        imp.list_view.scroll_to(pos, flags, None);
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

    /// Find a currently-bound (visible) row whose backing FileObject
    /// points at `target`. See WrenFileGrid::cell_for_file for usage.
    pub fn row_for_file(&self, target: &gio::File) -> Option<WrenFileRow> {
        let imp = imp::WrenFileList::from_obj(self);
        let map = imp.bound_rows.borrow();
        for weak in map.values() {
            if let Some(row) = weak.upgrade() {
                if let Some(obj) = row.bound_file_object() {
                    if obj.file().equal(target) {
                        return Some(row);
                    }
                }
            }
        }
        None
    }

    /// Wire middle-click to open the targeted item in a new tab.
    /// Ctrl+left-click stays bound to GTK's default multi-select.
    pub fn connect_open_in_tab<F: Fn(&FileObject) + 'static>(&self, f: F) {
        let imp = imp::WrenFileList::from_obj(self);
        let f = std::rc::Rc::new(f);
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(2);
        gesture.connect_pressed(glib::clone!(
            #[weak(rename_to = lv)] imp.list_view,
            #[strong] f,
            move |gesture, _, x, y| {
                let Some(picked) = lv.pick(x, y, gtk4::PickFlags::NON_TARGETABLE) else {
                    return;
                };
                let mut cur: Option<gtk4::Widget> = Some(picked);
                while let Some(w) = cur {
                    if let Some(row) = w.downcast_ref::<WrenFileRow>() {
                        if let Some(obj) = row.bound_file_object() {
                            f(&obj);
                            gesture.set_state(gtk4::EventSequenceState::Claimed);
                        }
                        return;
                    }
                    if w.is::<gtk4::ListView>() { return; }
                    cur = w.parent();
                }
            }
        ));
        imp.list_view.add_controller(gesture);
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

    /// Sync the column header buttons / labels to the current
    /// app-level prefs. Called once at construction and again
    /// whenever the user toggles a column in Settings.
    pub fn apply_header_visibility(&self) {
        let imp = imp::WrenFileList::from_obj(self);
        let cols = crate::file_view::row::ColumnVisibility::from_application();
        imp.header_type.set_visible(cols.content_type);
        imp.header_size.set_visible(cols.size);
        imp.header_modified.set_visible(cols.modified);
        imp.header_permissions.set_visible(cols.permissions);
        imp.header_owner.set_visible(cols.owner);
        imp.header_group.set_visible(cols.group);
        imp.header_accessed.set_visible(cols.accessed);
    }

    /// Force every visible row to rebind so it picks up the latest
    /// column-visibility prefs. Replaces the factory — same trick
    /// used by zoom/extensions changes.
    pub fn refresh_columns(&self) {
        let imp = imp::WrenFileList::from_obj(self);
        self.apply_header_visibility();
        imp.list_view.set_factory(Some(&make_row_factory(
            Rc::clone(&imp.icon_size),
            Rc::clone(&imp.cut_uris),
            imp.show_extensions.get(),
            Rc::clone(&imp.show_hidden),
            Rc::clone(&imp.bound_rows),
        )));
    }
}

mod imp {
    use super::*;

    pub struct WrenFileList {
        pub list_view: gtk4::ListView,
        pub cut_uris: Rc<RefCell<HashSet<String>>>,
        pub show_extensions: Cell<bool>,
        pub show_hidden: Rc<Cell<bool>>,
        pub sort_buttons: RefCell<Vec<gtk4::Button>>,
        pub icon_size: Rc<Cell<u32>>,
        pub bound_rows: BoundRows,
        pub header_icon_spacer: gtk4::Box,
        pub context_menu_builder: RefCell<Option<Box<dyn Fn() -> gio::MenuModel>>>,
        pub context_popover: RefCell<Option<gtk4::PopoverMenu>>,
        pub typeahead: Rc<TypeaheadState>,
        // Header widgets for toggleable columns. Indexed by column key:
        //   type, size, date, permissions, owner, group, accessed
        pub header_type: gtk4::Button,
        pub header_size: gtk4::Button,
        pub header_modified: gtk4::Button,
        pub header_permissions: gtk4::Label,
        pub header_owner: gtk4::Label,
        pub header_group: gtk4::Label,
        pub header_accessed: gtk4::Label,
    }

    impl Default for WrenFileList {
        fn default() -> Self {
            Self {
                list_view: Default::default(),
                cut_uris: Rc::new(RefCell::new(HashSet::new())),
                show_extensions: Cell::new(true),
                show_hidden: Rc::new(Cell::new(false)),
                sort_buttons: RefCell::new(Vec::new()),
                icon_size: Rc::new(Cell::new(24)),
                bound_rows: Rc::new(RefCell::new(HashMap::new())),
                header_icon_spacer: gtk4::Box::new(gtk4::Orientation::Horizontal, 0),
                context_menu_builder: RefCell::new(None),
                context_popover: RefCell::new(None),
                typeahead: Rc::new(TypeaheadState::default()),
                header_type: gtk4::Button::new(),
                header_size: gtk4::Button::new(),
                header_modified: gtk4::Button::new(),
                header_permissions: gtk4::Label::new(None),
                header_owner: gtk4::Label::new(None),
                header_group: gtk4::Label::new(None),
                header_accessed: gtk4::Label::new(None),
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
                Rc::clone(&self.show_hidden),
                Rc::clone(&self.bound_rows),
            )));
            // Rubber-band selection in list view: GTK4 routes drag-from-row
            // through the DragSource controller without firing rubberband, so
            // the selection rect only appears when dragging from empty space —
            // exactly what users expect.
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

            let lv = self.list_view.clone();
            crate::file_view::typeahead::attach(
                &self.list_view,
                Rc::clone(&self.typeahead),
                move |pos| {
                    lv.scroll_to(pos, gtk4::ListScrollFlags::FOCUS, None);
                },
            );

            // ── Column header row ─────────────────────────────────────────
            let header_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
            header_box.add_css_class("wren-list-header");
            header_box.set_margin_start(8);
            header_box.set_margin_end(8);

            // Spacer width = icon_size + row's inter-column spacing (8px)
            self.header_icon_spacer.set_size_request(32, -1);
            header_box.append(&self.header_icon_spacer);

            // Name column (always visible, sortable, hexpand).
            let name_btn = gtk4::Button::with_label("Name");
            name_btn.add_css_class("flat");
            name_btn.set_hexpand(true);
            name_btn.connect_clicked(|btn| {
                let _ = btn.activate_action(
                    "win.set-sort-key",
                    Some(&"name".to_variant()),
                );
            });
            header_box.append(&name_btn);

            // Sortable toggleable headers (Type, Size, Modified).
            let mut sort_buttons = self.sort_buttons.borrow_mut();
            sort_buttons.push(name_btn);

            let setup_sort_btn = |btn: &gtk4::Button, label: &str, key: &'static str, width: i32| {
                btn.set_label(label);
                btn.add_css_class("flat");
                btn.set_width_request(width);
                if let Some(lbl) = btn.child().and_downcast::<gtk4::Label>() {
                    lbl.set_xalign(1.0);
                }
                btn.connect_clicked(move |btn| {
                    let _ = btn.activate_action(
                        "win.set-sort-key",
                        Some(&key.to_variant()),
                    );
                });
            };
            setup_sort_btn(&self.header_type, "Type", "type", 120);
            setup_sort_btn(&self.header_size, "Size", "size", 80);
            setup_sort_btn(&self.header_modified, "Modified", "date", 120);
            header_box.append(&self.header_type);
            header_box.append(&self.header_size);
            header_box.append(&self.header_modified);
            sort_buttons.push(self.header_type.clone());
            sort_buttons.push(self.header_size.clone());
            sort_buttons.push(self.header_modified.clone());
            drop(sort_buttons);

            // Non-sortable optional headers (Permissions / Owner / Group / Accessed).
            let setup_label_hdr = |lbl: &gtk4::Label, text: &str, width: i32| {
                lbl.set_label(text);
                lbl.set_width_request(width);
                lbl.set_xalign(1.0);
                lbl.add_css_class("dim-label");
                lbl.set_visible(false);
            };
            setup_label_hdr(&self.header_permissions, "Permissions", 100);
            setup_label_hdr(&self.header_owner, "Owner", 100);
            setup_label_hdr(&self.header_group, "Group", 100);
            setup_label_hdr(&self.header_accessed, "Accessed", 120);
            header_box.append(&self.header_permissions);
            header_box.append(&self.header_owner);
            header_box.append(&self.header_group);
            header_box.append(&self.header_accessed);

            // Apply the persisted column-visibility prefs to header
            // widgets. Row widget visibility is handled at bind-time.
            self.obj().apply_header_visibility();

            // Right-click on the header bar pops a menu to toggle
            // column visibility. Stateful actions on the window drive
            // the checkmarks; they're registered in window/imp.rs.
            let header_click = gtk4::GestureClick::new();
            header_click.set_button(gtk4::gdk::BUTTON_SECONDARY);
            let header_box_weak = header_box.downgrade();
            header_click.connect_pressed(move |gesture, _n, x, y| {
                let Some(header_box) = header_box_weak.upgrade() else { return };
                gesture.set_state(gtk4::EventSequenceState::Claimed);

                let menu = gio::Menu::new();
                let entries: &[(&str, &str)] = &[
                    ("Type", "win.toggle-col-type"),
                    ("Size", "win.toggle-col-size"),
                    ("Modified", "win.toggle-col-modified"),
                    ("Permissions", "win.toggle-col-permissions"),
                    ("Owner", "win.toggle-col-owner"),
                    ("Group", "win.toggle-col-group"),
                    ("Accessed", "win.toggle-col-accessed"),
                ];
                for &(label, action) in entries {
                    menu.append(Some(label), Some(action));
                }

                let popover = gtk4::PopoverMenu::from_model(Some(&menu));
                popover.set_has_arrow(false);
                popover.set_parent(&header_box);
                popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
                    x as i32, y as i32, 1, 1,
                )));
                popover.connect_closed(|p| p.unparent());
                popover.popup();
                crate::file_view::popover::lock_vertical_only(&popover);
            });
            header_box.add_controller(header_click);

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
            if let Some(popover) = self.context_popover.take() {
                popover.unparent();
            }
            self.obj().first_child().map(|child| child.unparent());
        }
    }

    impl WidgetImpl for WrenFileList {}
}
