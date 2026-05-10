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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::run_on_gtk_thread;

    fn make_file_info(
        name: &str,
        kind: gio::FileType,
        size: i64,
        content_type: &str,
        modified_unix: i64,
    ) -> gio::FileInfo {
        let info = gio::FileInfo::new();
        info.set_name(std::path::Path::new(name));
        info.set_display_name(name);
        info.set_file_type(kind);
        info.set_size(size);
        // GFileInfo emits criticals on getter calls if these standard
        // attributes are unset; populate explicitly so test output stays clean.
        info.set_attribute_uint64("standard::size", size as u64);
        info.set_content_type(content_type);
        info.set_icon(&gio::ThemedIcon::new("text-x-generic"));
        info.set_is_hidden(name.starts_with('.'));
        info.set_is_symlink(false);
        if modified_unix > 0 {
            if let Ok(dt) = glib::DateTime::from_unix_local(modified_unix) {
                info.set_modification_date_time(&dt);
            }
        }
        info
    }

    fn make_file_object(
        name: &str,
        kind: gio::FileType,
        size: i64,
        content_type: &str,
        modified_unix: i64,
    ) -> FileObject {
        let info = make_file_info(name, kind, size, content_type, modified_unix);
        let file = gio::File::for_path(format!("/tmp/wren-test/{name}"));
        FileObject::new(file, info)
    }

    fn names_in_selection(model: &DirectoryModel) -> Vec<String> {
        use gtk4::prelude::*;
        let sel = &model.selection;
        let mut out = Vec::new();
        for i in 0..sel.n_items() {
            let item = sel.item(i).unwrap();
            let fo = item.downcast::<FileObject>().unwrap();
            out.push(fo.name());
        }
        out
    }

    #[test]
    fn sort_key_round_trip_all_variants() {
        for key in [SortKey::Name, SortKey::Size, SortKey::Date, SortKey::Type] {
            assert_eq!(SortKey::from_str(key.as_str()), key);
        }
    }

    #[test]
    fn sort_key_unknown_string_defaults_to_name() {
        assert_eq!(SortKey::from_str(""), SortKey::Name);
        assert_eq!(SortKey::from_str("nope"), SortKey::Name);
        assert_eq!(SortKey::from_str("NAME"), SortKey::Name); // case sensitive
    }

    #[test]
    fn sort_key_default_is_name() {
        assert_eq!(SortKey::default(), SortKey::Name);
    }

    #[test]
    fn sort_key_as_str_returns_lowercase_token() {
        assert_eq!(SortKey::Name.as_str(), "name");
        assert_eq!(SortKey::Size.as_str(), "size");
        assert_eq!(SortKey::Date.as_str(), "date");
        assert_eq!(SortKey::Type.as_str(), "type");
    }

    #[test]
    fn pipeline_selection_reflects_store_after_splice() {
        run_on_gtk_thread(|| {
            let model = DirectoryModel::new(gio::File::for_path("/tmp"));
            let items = vec![
                make_file_object("alpha.txt", gio::FileType::Regular, 10, "text/plain", 1),
                make_file_object("beta.txt", gio::FileType::Regular, 20, "text/plain", 2),
                make_file_object("gamma.txt", gio::FileType::Regular, 30, "text/plain", 3),
            ];
            model.store.splice(0, 0, &items);
            // Default filter hides nothing (no hidden entries) and search="".
            // Default sort = Name ascending.
            assert_eq!(model.selection.n_items(), 3);
        });
    }

    #[test]
    fn filter_hides_hidden_files_by_default() {
        run_on_gtk_thread(|| {
            let model = DirectoryModel::new(gio::File::for_path("/tmp"));
            let items = vec![
                make_file_object("visible.txt", gio::FileType::Regular, 1, "text/plain", 1),
                make_file_object(".hidden", gio::FileType::Regular, 1, "text/plain", 1),
            ];
            model.store.splice(0, 0, &items);
            // show_hidden = false (default); .hidden filtered out.
            assert_eq!(model.selection.n_items(), 1);
            assert_eq!(names_in_selection(&model), vec!["visible.txt".to_string()]);
        });
    }

    #[test]
    fn filter_show_hidden_includes_dotfiles() {
        run_on_gtk_thread(|| {
            let model = DirectoryModel::new(gio::File::for_path("/tmp"));
            let items = vec![
                make_file_object("visible.txt", gio::FileType::Regular, 1, "text/plain", 1),
                make_file_object(".hidden", gio::FileType::Regular, 1, "text/plain", 1),
            ];
            model.store.splice(0, 0, &items);
            model.set_filter("", true);
            assert_eq!(model.selection.n_items(), 2);
        });
    }

    #[test]
    fn directories_sort_before_files() {
        run_on_gtk_thread(|| {
            let model = DirectoryModel::new(gio::File::for_path("/tmp"));
            // Even with a name-asc sort, "zfolder" precedes "afile".
            let items = vec![
                make_file_object("afile", gio::FileType::Regular, 0, "text/plain", 1),
                make_file_object(
                    "zfolder",
                    gio::FileType::Directory,
                    0,
                    "inode/directory",
                    1,
                ),
            ];
            model.store.splice(0, 0, &items);
            assert_eq!(
                names_in_selection(&model),
                vec!["zfolder".to_string(), "afile".to_string()]
            );
        });
    }

    #[test]
    fn sort_by_size_ascending_orders_smallest_first() {
        run_on_gtk_thread(|| {
            let model = DirectoryModel::new(gio::File::for_path("/tmp"));
            let items = vec![
                make_file_object("big", gio::FileType::Regular, 1000, "text/plain", 1),
                make_file_object("tiny", gio::FileType::Regular, 1, "text/plain", 1),
                make_file_object("med", gio::FileType::Regular, 100, "text/plain", 1),
            ];
            model.store.splice(0, 0, &items);
            model.set_sort(SortKey::Size, false);
            assert_eq!(
                names_in_selection(&model),
                vec!["tiny".to_string(), "med".to_string(), "big".to_string()]
            );
        });
    }

    #[test]
    fn sort_by_size_reversed_orders_largest_first() {
        run_on_gtk_thread(|| {
            let model = DirectoryModel::new(gio::File::for_path("/tmp"));
            let items = vec![
                make_file_object("med", gio::FileType::Regular, 100, "text/plain", 1),
                make_file_object("big", gio::FileType::Regular, 1000, "text/plain", 1),
                make_file_object("tiny", gio::FileType::Regular, 1, "text/plain", 1),
            ];
            model.store.splice(0, 0, &items);
            model.set_sort(SortKey::Size, true);
            assert_eq!(
                names_in_selection(&model),
                vec!["big".to_string(), "med".to_string(), "tiny".to_string()]
            );
        });
    }

    #[test]
    fn sort_by_name_reversed_z_to_a() {
        run_on_gtk_thread(|| {
            let model = DirectoryModel::new(gio::File::for_path("/tmp"));
            let items = vec![
                make_file_object("apple", gio::FileType::Regular, 1, "text/plain", 1),
                make_file_object("zebra", gio::FileType::Regular, 1, "text/plain", 1),
                make_file_object("mango", gio::FileType::Regular, 1, "text/plain", 1),
            ];
            model.store.splice(0, 0, &items);
            model.set_sort(SortKey::Name, true);
            assert_eq!(
                names_in_selection(&model),
                vec![
                    "zebra".to_string(),
                    "mango".to_string(),
                    "apple".to_string()
                ]
            );
        });
    }

    #[test]
    fn sort_by_date_orders_oldest_first_when_not_reversed() {
        run_on_gtk_thread(|| {
            let model = DirectoryModel::new(gio::File::for_path("/tmp"));
            let items = vec![
                make_file_object("recent", gio::FileType::Regular, 1, "text/plain", 3000),
                make_file_object("old", gio::FileType::Regular, 1, "text/plain", 1000),
                make_file_object("mid", gio::FileType::Regular, 1, "text/plain", 2000),
            ];
            model.store.splice(0, 0, &items);
            model.set_sort(SortKey::Date, false);
            assert_eq!(
                names_in_selection(&model),
                vec!["old".to_string(), "mid".to_string(), "recent".to_string()]
            );
        });
    }

    #[test]
    fn sort_state_round_trips_through_set_sort() {
        run_on_gtk_thread(|| {
            let model = DirectoryModel::new(gio::File::for_path("/tmp"));
            assert_eq!(model.sort_key(), SortKey::Name);
            assert!(!model.sort_reversed());
            model.set_sort(SortKey::Date, true);
            assert_eq!(model.sort_key(), SortKey::Date);
            assert!(model.sort_reversed());
        });
    }

    // --- MimeCategory ----------------------------------------------

    #[test]
    fn mime_category_round_trip_all_variants() {
        for cat in [
            MimeCategory::All,
            MimeCategory::Images,
            MimeCategory::Videos,
            MimeCategory::Audio,
            MimeCategory::Documents,
            MimeCategory::Archives,
        ] {
            assert_eq!(MimeCategory::from_str(cat.as_str()), cat);
        }
    }

    #[test]
    fn mime_category_unknown_string_defaults_to_all() {
        assert_eq!(MimeCategory::from_str("anything else"), MimeCategory::All);
        assert_eq!(MimeCategory::from_str(""), MimeCategory::All);
        assert_eq!(MimeCategory::from_str("Images"), MimeCategory::All); // case sensitive
    }

    #[test]
    fn mime_category_all_matches_anything() {
        assert!(MimeCategory::All.matches("image/png"));
        assert!(MimeCategory::All.matches("application/zip"));
        assert!(MimeCategory::All.matches(""));
        assert!(MimeCategory::All.matches("nonsense"));
    }

    #[test]
    fn mime_category_empty_content_type_only_matches_all() {
        assert!(MimeCategory::All.matches(""));
        assert!(!MimeCategory::Images.matches(""));
        assert!(!MimeCategory::Videos.matches(""));
        assert!(!MimeCategory::Audio.matches(""));
        assert!(!MimeCategory::Documents.matches(""));
        assert!(!MimeCategory::Archives.matches(""));
    }

    #[test]
    fn mime_category_images_match_image_subtree_only() {
        assert!(MimeCategory::Images.matches("image/png"));
        assert!(MimeCategory::Images.matches("image/jpeg"));
        assert!(!MimeCategory::Images.matches("text/plain"));
    }

    #[test]
    fn mime_category_documents_pdf_text_office_open_xml_and_open_document() {
        // The Documents bucket spans plaintext, PDF, RTF, .doc, modern
        // OOXML formats, and ODF.
        assert!(MimeCategory::Documents.matches("application/pdf"));
        assert!(MimeCategory::Documents.matches("text/plain"));
        assert!(MimeCategory::Documents.matches(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        ));
        assert!(MimeCategory::Documents.matches("application/vnd.oasis.opendocument.text"));
        // Negative: an image is not a document.
        assert!(!MimeCategory::Documents.matches("image/jpeg"));
    }

    #[test]
    fn mime_category_archives_match_known_compressed_formats() {
        assert!(MimeCategory::Archives.matches("application/zip"));
        assert!(MimeCategory::Archives.matches("application/x-tar"));
        assert!(MimeCategory::Archives.matches("application/gzip"));
        assert!(MimeCategory::Archives.matches("application/x-7z-compressed"));
        assert!(MimeCategory::Archives.matches("application/x-bzip2"));
        assert!(MimeCategory::Archives.matches("application/zstd"));
        assert!(!MimeCategory::Archives.matches("application/pdf"));
    }

    // --- folders-first thread-local --------------------------------

    #[test]
    #[serial_test::serial]
    fn folders_first_default_sorts_directories_above_files() {
        run_on_gtk_thread(|| {
            // Force the documented default explicitly so prior tests can't
            // leak a different value across the shared GTK worker.
            super::set_folders_first(true);
            let model = DirectoryModel::new(gio::File::for_path("/tmp"));
            // 'a' file precedes 'z' directory alphabetically — only the
            // folders-first rule can flip the order.
            let items = vec![
                make_file_object("alpha", gio::FileType::Regular, 0, "text/plain", 1),
                make_file_object(
                    "zeta",
                    gio::FileType::Directory,
                    0,
                    "inode/directory",
                    1,
                ),
            ];
            model.store.splice(0, 0, &items);
            assert_eq!(
                names_in_selection(&model),
                vec!["zeta".to_string(), "alpha".to_string()]
            );
            // Restore default for any subsequent serialised test.
            super::set_folders_first(true);
        });
    }

    #[test]
    #[serial_test::serial]
    fn folders_first_off_falls_through_to_name_sort() {
        run_on_gtk_thread(|| {
            super::set_folders_first(false);
            let model = DirectoryModel::new(gio::File::for_path("/tmp"));
            let items = vec![
                make_file_object("alpha", gio::FileType::Regular, 0, "text/plain", 1),
                make_file_object(
                    "zeta",
                    gio::FileType::Directory,
                    0,
                    "inode/directory",
                    1,
                ),
            ];
            model.store.splice(0, 0, &items);
            // With folders_first off, name asc — file 'alpha' wins.
            assert_eq!(
                names_in_selection(&model),
                vec!["alpha".to_string(), "zeta".to_string()]
            );
            // Restore the documented default.
            super::set_folders_first(true);
        });
    }

    // --- set_category ----------------------------------------------

    #[test]
    fn set_category_filters_files_by_mime_and_lets_directories_pass() {
        run_on_gtk_thread(|| {
            let model = DirectoryModel::new(gio::File::for_path("/tmp"));
            let items = vec![
                make_file_object("pic.png", gio::FileType::Regular, 1, "image/png", 1),
                make_file_object("notes.txt", gio::FileType::Regular, 1, "text/plain", 1),
                make_file_object(
                    "subdir",
                    gio::FileType::Directory,
                    0,
                    "inode/directory",
                    1,
                ),
            ];
            model.store.splice(0, 0, &items);

            // All: 3 visible.
            model.set_category(MimeCategory::All);
            assert_eq!(model.selection.n_items(), 3);

            // Images: directory + image.
            model.set_category(MimeCategory::Images);
            let names = names_in_selection(&model);
            assert!(names.contains(&"pic.png".to_string()));
            assert!(names.contains(&"subdir".to_string()));
            assert!(!names.contains(&"notes.txt".to_string()));

            // Documents: directory + text.
            model.set_category(MimeCategory::Documents);
            let names = names_in_selection(&model);
            assert!(names.contains(&"notes.txt".to_string()));
            assert!(names.contains(&"subdir".to_string()));
            assert!(!names.contains(&"pic.png".to_string()));
        });
    }
}
