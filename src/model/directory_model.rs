use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
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

/// Cap recursive search depth to avoid pathological symlink loops and
/// keep walks bounded even on very deep trees.
const SEARCH_MAX_DEPTH: u32 = 8;

/// Hard cap on visited entries (files + dirs). Prevents the walk from
/// chewing through e.g. an entire `~/` if the user types a very common
/// substring. When hit, `start_search` resolves with `truncated = true`.
const SEARCH_MAX_VISITED: u32 = 50_000;

/// Yield to the GTK main loop after this many appended matches so the UI
/// can paint while a long search is in flight.
const SEARCH_YIELD_EVERY: u32 = 50;

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

        // The display-time filter now only handles show_hidden. Search
        // filtering is performed during the recursive walk in
        // `start_search`, so the store only ever contains entries that
        // already match the query (or, in browse mode, all children of
        // the current dir).
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
                // Search-text filtering moved into start_search's recursive
                // walk; the store only ever holds entries that already
                // match the query, so no display-time check needed here.
                if !matches!(state.category, MimeCategory::All) && !file_obj.is_directory() {
                    if !state.category.matches(&file_obj.content_type()) {
                        return false;
                    }
                }
                true
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

    /// Sets the show-hidden gate. Search filtering is no longer driven
    /// through here — `start_search` handles that during the walk.
    pub fn set_filter(&self, _search: &str, show_hidden: bool) {
        {
            let mut state = self.filter_state.borrow_mut();
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
        // Cancel any in-flight load OR search. Critical for rapid
        // typing: each keystroke supersedes the prior walk, and the
        // old future must observe the cancellation before it touches
        // the now-shared store.
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

    /// Recursive name search starting from `self.location`.
    ///
    /// BFS walk; matches are appended to `store` as the walk proceeds
    /// so the UI can show partial results immediately. Honours
    /// `show_hidden`: when false, dotfiles are skipped and dotted dirs
    /// are not descended into.
    ///
    /// Resolves with `Ok(true)` when truncated (visited cap or depth
    /// cap hit) so the caller can surface a hint to the user.
    pub fn start_search(
        &self,
        query: &str,
        show_hidden: bool,
    ) -> impl std::future::Future<Output = Result<bool, glib::Error>> + 'static {
        // Cancel any in-flight load/search before clearing the store.
        // Without this, an older walk could continue to append into the
        // store after we've already moved on to a newer query.
        self.cancel();
        self.store.remove_all();

        let cancellable = gio::Cancellable::new();
        *self.cancellable.borrow_mut() = Some(cancellable.clone());

        let store = self.store.clone();
        let root = self.location.clone();
        let needle = query.to_lowercase();

        async move {
            // (dir, depth) — depth of the entries inside that dir.
            let mut queue: VecDeque<(gio::File, u32)> = VecDeque::new();
            queue.push_back((root, 1));

            let mut visited: u32 = 0;
            let mut since_yield: u32 = 0;
            let mut truncated = false;

            'outer: while let Some((dir, depth)) = queue.pop_front() {
                if cancellable.is_cancelled() {
                    return Ok(false);
                }

                let enumerator = match dir
                    .enumerate_children_future(
                        FileObject::QUERY_ATTRS,
                        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                        glib::Priority::LOW,
                    )
                    .await
                {
                    Ok(e) => e,
                    // Permission denied / vanished dir: skip without
                    // failing the whole search.
                    Err(_) => continue,
                };

                loop {
                    if cancellable.is_cancelled() {
                        return Ok(false);
                    }
                    let infos = match enumerator
                        .next_files_future(50, glib::Priority::LOW)
                        .await
                    {
                        Ok(infos) => infos,
                        Err(_) => break,
                    };
                    if infos.is_empty() {
                        break;
                    }

                    for info in infos {
                        // Cancellation safety: we check per-item, not
                        // just per-batch, because a fresh keystroke
                        // can cancel us between resolving a batch and
                        // finishing iteration. Without this, a
                        // superseded walk can append into the store
                        // AFTER the new walk has cleared it.
                        if cancellable.is_cancelled() {
                            return Ok(false);
                        }
                        visited += 1;
                        if visited > SEARCH_MAX_VISITED {
                            truncated = true;
                            break 'outer;
                        }

                        let name = info.name();
                        let display = info.display_name().to_string();
                        let is_hidden = display.starts_with('.');
                        if is_hidden && !show_hidden {
                            continue;
                        }

                        let child = dir.child(&name);
                        let is_dir = info.file_type() == gio::FileType::Directory;

                        if display.to_lowercase().contains(&needle) {
                            let fo = FileObject::new(child.clone(), info.clone());
                            store.append(&fo);
                            since_yield += 1;
                        }

                        if is_dir && depth < SEARCH_MAX_DEPTH {
                            queue.push_back((child, depth + 1));
                        }

                        if since_yield >= SEARCH_YIELD_EVERY {
                            since_yield = 0;
                            // Yield to the main loop so a fresh
                            // keystroke can deliver its cancel before
                            // we start the next batch.
                            glib::timeout_future_with_priority(
                                glib::Priority::LOW,
                                std::time::Duration::from_millis(0),
                            )
                            .await;
                            if cancellable.is_cancelled() {
                                return Ok(false);
                            }
                        }
                    }
                }
            }

            Ok(truncated)
        }
    }
}
