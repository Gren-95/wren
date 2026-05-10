use std::cell::{Cell, RefCell};
use std::rc::Rc;
use gtk4::prelude::*;

use crate::model::FileObject;

thread_local! {
    static FOLDERS_FIRST: Cell<bool> = const { Cell::new(true) };
}

/// Update the global "directories before files" preference. The setting is
/// thread-local so the sort comparator (which fires on the main thread) can
/// read it without going through the app singleton each comparison. Caller
/// must invalidate any existing sorters separately — this only updates the
/// flag.
pub fn set_folders_first(v: bool) {
    FOLDERS_FIRST.with(|c| c.set(v));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    #[default]
    Name,
    Size,
    Date,
    Type,
}

impl SortKey {
    pub fn from_str(s: &str) -> Self {
        match s {
            "size" => Self::Size,
            "date" => Self::Date,
            "type" => Self::Type,
            _ => Self::Name,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Size => "size",
            Self::Date => "date",
            Self::Type => "type",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MimeCategory {
    #[default]
    All,
    Images,
    Videos,
    Audio,
    Documents,
    Archives,
}

impl MimeCategory {
    pub fn from_str(s: &str) -> Self {
        match s {
            "images" => Self::Images,
            "videos" => Self::Videos,
            "audio" => Self::Audio,
            "documents" => Self::Documents,
            "archives" => Self::Archives,
            _ => Self::All,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Images => "images",
            Self::Videos => "videos",
            Self::Audio => "audio",
            Self::Documents => "documents",
            Self::Archives => "archives",
        }
    }

    /// Match a content-type string against this category. `All` always
    /// matches. Empty/missing content-types match nothing except `All`.
    pub fn matches(self, content_type: &str) -> bool {
        if matches!(self, Self::All) {
            return true;
        }
        if content_type.is_empty() {
            return false;
        }
        match self {
            Self::All => true,
            Self::Images => content_type.starts_with("image/"),
            Self::Videos => content_type.starts_with("video/"),
            Self::Audio => content_type.starts_with("audio/"),
            Self::Documents => {
                content_type.starts_with("text/")
                    || content_type == "application/pdf"
                    || content_type == "application/rtf"
                    || content_type == "application/msword"
                    || content_type
                        .starts_with("application/vnd.openxmlformats-officedocument.")
                    || content_type.starts_with("application/vnd.oasis.opendocument.")
            }
            Self::Archives => matches!(
                content_type,
                "application/zip"
                    | "application/x-tar"
                    | "application/gzip"
                    | "application/x-7z-compressed"
                    | "application/x-bzip2"
                    | "application/zstd"
            ),
        }
    }
}

#[derive(Debug, Default)]
struct FilterState {
    search: String,
    show_hidden: bool,
    category: MimeCategory,
}

#[derive(Debug, Default)]
struct SortState {
    key: SortKey,
    reversed: bool,
}

#[derive(Debug)]
pub struct DirectoryModel {
    pub location: gio::File,
    pub store: gio::ListStore,
    filter: gtk4::CustomFilter,
    filter_state: Rc<RefCell<FilterState>>,
    pub filter_model: gtk4::FilterListModel,
    sort_state: Rc<RefCell<SortState>>,
    sorter: gtk4::CustomSorter,
    pub sort_model: gtk4::SortListModel,
    pub selection: gtk4::MultiSelection,
    cancellable: RefCell<Option<gio::Cancellable>>,
}

impl DirectoryModel {
    pub fn new(location: gio::File) -> Self {
        let store = gio::ListStore::new::<FileObject>();

        let filter_state = Rc::new(RefCell::new(FilterState::default()));
        let filter = gtk4::CustomFilter::new({
            let state = filter_state.clone();
            move |obj| {
                let state = state.borrow();
                let file_obj = obj.downcast_ref::<FileObject>().unwrap();
                if !state.show_hidden && file_obj.name().starts_with('.') {
                    return false;
                }
                // Mime category: directories always pass through so the
                // user can keep navigating. Files must match the category.
                if !matches!(state.category, MimeCategory::All) && !file_obj.is_directory() {
                    if !state.category.matches(&file_obj.content_type()) {
                        return false;
                    }
                }
                if state.search.is_empty() {
                    return true;
                }
                file_obj.name().to_lowercase().contains(&state.search)
            }
        });

        let filter_model = gtk4::FilterListModel::new(Some(store.clone()), Some(filter.clone()));

        let sort_state = Rc::new(RefCell::new(SortState::default()));
        let sorter = gtk4::CustomSorter::new({
            let sort_state = sort_state.clone();
            move |a, b| {
                let a = a.downcast_ref::<FileObject>().unwrap();
                let b = b.downcast_ref::<FileObject>().unwrap();
                // Directories first only when the user-configurable
                // preference is on (default). Off → fall through to the
                // selected sort key, mixing dirs and files.
                if FOLDERS_FIRST.with(|c| c.get()) {
                    match (a.is_directory(), b.is_directory()) {
                        (true, false) => return gtk4::Ordering::Smaller,
                        (false, true) => return gtk4::Ordering::Larger,
                        _ => {}
                    }
                }
                let state = sort_state.borrow();
                let ord = match state.key {
                    SortKey::Name => a.name().to_lowercase().cmp(&b.name().to_lowercase()),
                    SortKey::Size => a.file_size().cmp(&b.file_size()),
                    SortKey::Date => a.modified().cmp(&b.modified()),
                    SortKey::Type => {
                        let ta = a.content_type();
                        let tb = b.content_type();
                        ta.cmp(&tb).then_with(|| {
                            a.name().to_lowercase().cmp(&b.name().to_lowercase())
                        })
                    }
                };
                let ord = if state.reversed { ord.reverse() } else { ord };
                ord.into()
            }
        });

        let sort_model = gtk4::SortListModel::new(Some(filter_model.clone()), Some(sorter.clone()));
        let selection = gtk4::MultiSelection::new(Some(sort_model.clone()));

        Self {
            location,
            store,
            filter,
            filter_state,
            filter_model,
            sort_state,
            sorter,
            sort_model,
            selection,
            cancellable: RefCell::new(None),
        }
    }

    pub fn set_filter(&self, search: &str, show_hidden: bool) {
        {
            let mut state = self.filter_state.borrow_mut();
            state.search = search.to_lowercase();
            state.show_hidden = show_hidden;
        }
        self.filter.changed(gtk4::FilterChange::Different);
    }

    pub fn set_category(&self, category: MimeCategory) {
        {
            let mut state = self.filter_state.borrow_mut();
            state.category = category;
        }
        self.filter.changed(gtk4::FilterChange::Different);
    }

    /// Force a re-sort using the current `SortState` and the current
    /// thread-local `FOLDERS_FIRST` flag. Call after toggling that flag.
    pub fn refresh_sort(&self) {
        self.sorter.changed(gtk4::SorterChange::Different);
    }

    pub fn set_sort(&self, key: SortKey, reversed: bool) {
        {
            let mut state = self.sort_state.borrow_mut();
            state.key = key;
            state.reversed = reversed;
        }
        self.sorter.changed(gtk4::SorterChange::Different);
    }

    pub fn sort_key(&self) -> SortKey {
        self.sort_state.borrow().key
    }

    pub fn sort_reversed(&self) -> bool {
        self.sort_state.borrow().reversed
    }

    pub fn cancel(&self) {
        if let Some(c) = self.cancellable.borrow_mut().take() {
            c.cancel();
        }
    }

    pub fn start_load(
        &self,
    ) -> impl std::future::Future<Output = Result<(), glib::Error>> + 'static {
        self.cancel();
        self.store.remove_all();

        let cancellable = gio::Cancellable::new();
        *self.cancellable.borrow_mut() = Some(cancellable.clone());

        let store = self.store.clone();
        let location = self.location.clone();

        async move {
            let enumerator = location
                .enumerate_children_future(
                    FileObject::QUERY_ATTRS,
                    gio::FileQueryInfoFlags::NONE,
                    glib::Priority::DEFAULT,
                )
                .await?;

            let mut all_items: Vec<FileObject> = Vec::new();
            loop {
                if cancellable.is_cancelled() {
                    return Ok(());
                }
                let infos = enumerator
                    .next_files_future(30, glib::Priority::DEFAULT)
                    .await?;
                if infos.is_empty() {
                    break;
                }
                for info in infos {
                    let child = location.child(info.name());
                    all_items.push(FileObject::new(child, info));
                }
            }

            // Add all items in one splice so GTK fires a single items_changed.
            // This keeps the scroll position stable at 0 — progressive per-item
            // appends would shift the sorted-model positions and drift the viewport.
            store.splice(0, 0, &all_items);
            Ok(())
        }
    }
}
