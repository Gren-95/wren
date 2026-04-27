use std::cell::RefCell;

use adw::prelude::*;

use crate::file_view::grid::WrenFileGrid;
use crate::file_view::list::WrenFileList;
use crate::model::{DirectoryModel, SortKey};
use crate::navigation::NavigationModel;

#[derive(Debug)]
pub struct TabState {
    pub content_widget: gtk4::Widget,
    pub content_stack: gtk4::Stack,
    pub view_stack: gtk4::Stack,
    pub file_grid: WrenFileGrid,
    pub file_list: WrenFileList,
    pub navigation: NavigationModel,
    pub dir_model: Option<DirectoryModel>,
    pub dir_monitor: RefCell<Option<gio::FileMonitor>>,
    pub sort_key: SortKey,
    pub sort_reversed: bool,
}

impl TabState {
    pub fn new() -> Self {
        let grid = WrenFileGrid::new();
        let list = WrenFileList::new();

        let view_stack = gtk4::Stack::new();
        view_stack.set_vexpand(true);
        view_stack.add_named(&grid, Some("grid"));
        view_stack.add_named(&list, Some("list"));
        view_stack.set_visible_child_name("grid");

        let spinner_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        spinner_box.set_halign(gtk4::Align::Center);
        spinner_box.set_valign(gtk4::Align::Center);
        spinner_box.set_vexpand(true);
        let spinner = gtk4::Spinner::builder()
            .spinning(true)
            .width_request(32)
            .height_request(32)
            .build();
        spinner_box.append(&spinner);

        let empty_page = adw::StatusPage::builder()
            .icon_name("folder-symbolic")
            .title("Empty Folder")
            .description("This folder contains no files.")
            .build();

        let no_results_page = adw::StatusPage::builder()
            .icon_name("edit-find-symbolic")
            .title("No Results")
            .description("Try a different search term.")
            .build();

        let content_stack = gtk4::Stack::new();
        content_stack.set_vexpand(true);
        content_stack.add_named(&spinner_box, Some("loading"));
        content_stack.add_named(&empty_page, Some("empty"));
        content_stack.add_named(&no_results_page, Some("no-results"));
        content_stack.add_named(&view_stack, Some("files"));

        let content_widget = content_stack.clone().upcast::<gtk4::Widget>();

        Self {
            content_widget,
            content_stack,
            view_stack,
            file_grid: grid,
            file_list: list,
            navigation: NavigationModel::default(),
            dir_model: None,
            dir_monitor: RefCell::new(None),
            sort_key: SortKey::Name,
            sort_reversed: false,
        }
    }

    pub fn cancel_monitor(&self) {
        if let Some(m) = self.dir_monitor.borrow_mut().take() {
            m.cancel();
        }
    }
}
