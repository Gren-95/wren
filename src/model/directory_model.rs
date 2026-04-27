use std::cell::RefCell;
use std::rc::Rc;
use gtk4::prelude::*;

use crate::model::FileObject;

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

#[derive(Debug, Default)]
struct FilterState {
    search: String,
    show_hidden: bool,
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
                // Directories always first regardless of sort key
                match (a.is_directory(), b.is_directory()) {
                    (true, false) => return gtk4::Ordering::Smaller,
                    (false, true) => return gtk4::Ordering::Larger,
                    _ => {}
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

            loop {
                if cancellable.is_cancelled() {
                    break;
                }
                let infos = enumerator
                    .next_files_future(30, glib::Priority::DEFAULT)
                    .await?;
                if infos.is_empty() {
                    break;
                }
                for info in infos {
                    if cancellable.is_cancelled() {
                        return Ok(());
                    }
                    let child = location.child(info.name());
                    store.append(&FileObject::new(child, info));
                }
            }

            Ok(())
        }
    }
}
