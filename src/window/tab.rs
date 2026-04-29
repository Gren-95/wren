use std::cell::{Cell, RefCell};

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
    pub error_page: adw::StatusPage,
    pub navigation: NavigationModel,
    pub dir_model: Option<DirectoryModel>,
    pub dir_monitor: RefCell<Option<gio::FileMonitor>>,
    pub sort_key: SortKey,
    pub sort_reversed: bool,
    pub status_bar: gtk4::Label,
    pub load_gen: Cell<u64>,
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

        let error_page = adw::StatusPage::builder()
            .icon_name("dialog-error-symbolic")
            .title("Could not open folder")
            .build();
        let retry_button = gtk4::Button::builder()
            .label("Retry")
            .halign(gtk4::Align::Center)
            .action_name("win.reload")
            .build();
        retry_button.add_css_class("suggested-action");
        error_page.set_child(Some(&retry_button));

        let content_stack = gtk4::Stack::new();
        content_stack.set_vexpand(true);
        content_stack.add_named(&empty_page, Some("empty"));
        content_stack.add_named(&no_results_page, Some("no-results"));
        content_stack.add_named(&error_page, Some("error"));
        content_stack.add_named(&view_stack, Some("files"));
        content_stack.set_visible_child_name("files");

        let status_bar = gtk4::Label::new(None);
        status_bar.set_halign(gtk4::Align::Start);
        status_bar.set_margin_start(8);
        status_bar.set_margin_end(8);
        status_bar.set_margin_top(3);
        status_bar.set_margin_bottom(3);
        status_bar.add_css_class("dim-label");
        status_bar.add_css_class("caption");

        let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        outer.set_vexpand(true);
        outer.append(&content_stack);
        outer.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
        outer.append(&status_bar);

        let content_widget = outer.upcast::<gtk4::Widget>();

        Self {
            content_widget,
            content_stack,
            view_stack,
            file_grid: grid,
            file_list: list,
            error_page,
            navigation: NavigationModel::default(),
            dir_model: None,
            dir_monitor: RefCell::new(None),
            sort_key: SortKey::Name,
            sort_reversed: false,
            status_bar,
            load_gen: Cell::new(0),
        }
    }

    pub fn cancel_monitor(&self) {
        if let Some(m) = self.dir_monitor.borrow_mut().take() {
            m.cancel();
        }
    }
}
