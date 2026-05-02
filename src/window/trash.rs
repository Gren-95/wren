//! Trash-related WrenWindow methods: move-to-trash, empty trash, restore.

use adw::prelude::*;

use super::WrenWindow;
use super::file_ops::{
    OpKind, delete_recursive, log_err, log_op, pre_walk_total,
};
use gio::Cancellable;
use gio::prelude::FileExtManual;

impl WrenWindow {
    pub fn move_to_trash(&self) {
        let files = self.selected_files();
        if files.is_empty() {
            return;
        }
        let n = files.len();
        let name = files
            .first()
            .and_then(|f| f.basename())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let body = if n == 1 {
            format!("\"{name}\" will be moved to the Trash.")
        } else {
            format!("{n} items will be moved to the Trash.")
        };
        let confirm = adw::AlertDialog::new(Some("Move to Trash?"), Some(&body));
        confirm.add_response("cancel", "Cancel");
        confirm.add_response("trash", "Move to Trash");
        confirm.set_response_appearance("trash", adw::ResponseAppearance::Destructive);
        confirm.set_default_response(Some("cancel"));
        confirm.set_close_response("cancel");
        let files = std::rc::Rc::new(files);
        confirm.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, response| {
                    if response != "trash" {
                        return;
                    }
                    let files = (*files).clone();
                    glib::spawn_future_local(glib::clone!(
                        #[weak]
                        window,
                        async move {
                            window.do_trash_files(files).await;
                        }
                    ));
                }
            ),
        );
        confirm.present(Some(self));
    }

    /// Permanently delete every item in `trash:///` after confirmation.
    pub fn empty_trash(&self) {
        let dialog = adw::AlertDialog::new(
            Some("Empty Trash?"),
            Some("All items in the Trash will be permanently deleted. This cannot be undone."),
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("empty", "Empty Trash");
        dialog.set_response_appearance("empty", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        dialog.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, response| {
                    if response != "empty" { return; }
                    let trash = gio::File::for_uri("trash:///");
                    glib::spawn_future_local(glib::clone!(
                        #[weak] window,
                        async move {
                            // Enumerate trash:/// to gather every top-level item.
                            // The trash backend rejects writes to anything inside
                            // a trashed directory ("Items in the trash may not be
                            // modified"), so we DELETE EACH TOP-LEVEL ITEM atomically
                            // via delete_future — gvfs handles the recursion under
                            // the hood. Don't call delete_recursive here.
                            let mut to_delete: Vec<gio::File> = Vec::new();
                            if let Ok(en) = trash
                                .enumerate_children_future(
                                    "standard::name",
                                    gio::FileQueryInfoFlags::NONE,
                                    glib::Priority::DEFAULT,
                                )
                                .await
                            {
                                while let Ok(batch) = en
                                    .next_files_future(50, glib::Priority::DEFAULT)
                                    .await
                                {
                                    if batch.is_empty() { break; }
                                    for info in batch {
                                        to_delete.push(trash.child(info.name()));
                                    }
                                }
                            }

                            // Empty trash → just toast, don't open the popover.
                            if to_delete.is_empty() {
                                window.show_toast("Trash is already empty");
                                return;
                            }

                            let total = to_delete.len();
                            let handle = window.op_start(OpKind::Delete);
                            handle.set_total(total as u64, 0);
                            // For trash entries, delete_future returns in ~ms,
                            // so a 1000-item empty would whip the labels through
                            // 1000 names in a second — visually unpleasant /
                            // potentially photosensitive. Throttle the outer
                            // loop's label changes to ~10 Hz; the progress bar
                            // still ticks every iteration.
                            let mut last_ui = std::time::Instant::now()
                                .checked_sub(std::time::Duration::from_secs(1))
                                .unwrap_or_else(std::time::Instant::now);
                            let succeeded = 'op: {
                            for (idx, f) in to_delete.iter().enumerate() {
                                if handle.cancellable.is_cancelled() { break 'op false; }
                                log_op("empty trash", f, None);
                                let now = std::time::Instant::now();
                                if now.duration_since(last_ui)
                                    >= std::time::Duration::from_millis(100)
                                    || idx == 0
                                    || idx + 1 == total
                                {
                                    last_ui = now;
                                    let name = f
                                        .basename()
                                        .map(|p| p.to_string_lossy().into_owned())
                                        .unwrap_or_default();
                                    handle.set_item(&format!(
                                        "{name} ({} of {total})",
                                        idx + 1
                                    ));
                                    handle.set_paths(f, None);
                                }
                                if let Err(e) = delete_recursive(
                                    f.clone(),
                                    &handle.cancellable,
                                    handle.delete_callback(),
                                )
                                .await
                                {
                                    if !e.matches(gio::IOErrorEnum::Cancelled) {
                                        log_err("empty trash", f, None, &e);
                                        window.show_toast(&format!("Could not delete: {e}"));
                                    }
                                    break 'op false;
                                }
                            }
                            true
                            };
                            if succeeded { handle.mark_succeeded(); }
                            handle.set_fraction(1.0);
                            window.op_finish(&handle);
                            window.reload();
                        }
                    ));
                }
            ),
        );
        dialog.present(Some(self));
    }

    /// Restore selected trash items to their original locations.
    ///
    /// Uses `gio::File::move_future` directly: the gvfs trash backend
    /// implements native move from `trash:///` to `file:///`, so this is
    /// atomic and avoids the copy+delete round-trip the previous
    /// implementation needed (which silently failed on cross-device
    /// trash entries and on directories whose parents had been removed).
    pub fn restore_from_trash(&self) {
        let files = self.selected_files();
        if files.is_empty() { return; }
        let handle = self.op_start(OpKind::Restore);
        handle.set_item("Restoring…");
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = window)] self,
            #[strong] handle,
            async move {
                let total = files.len();
                let succeeded = 'op: {
                    for (idx, file) in files.iter().enumerate() {
                        if handle.cancellable.is_cancelled() { break 'op false; }

                        let info = match file
                            .query_info_future(
                                "trash::orig-path",
                                gio::FileQueryInfoFlags::NONE,
                                glib::Priority::DEFAULT,
                            )
                            .await
                        {
                            Ok(i) => i,
                            Err(e) => {
                                log_err("restore (query)", file, None, &e);
                                window.show_toast(&format!("Cannot read trash entry: {e}"));
                                continue;
                            }
                        };
                        let Some(orig_bytes) =
                            info.attribute_byte_string("trash::orig-path")
                        else {
                            window.show_toast("Original path unknown for this trash item");
                            continue;
                        };
                        let orig_str = std::str::from_utf8(orig_bytes.as_ref())
                            .unwrap_or_default();
                        let dest = gio::File::for_path(std::path::Path::new(orig_str));
                        let name = file
                            .basename()
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        handle.set_item(&format!("{name} ({} of {total})", idx + 1));
                        handle.set_paths(file, Some(&dest));
                        log_op("restore", file, Some(&dest));

                        if dest.query_exists(Cancellable::NONE) {
                            window.show_toast(&format!(
                                "Cannot restore {name}: destination already exists"
                            ));
                            continue;
                        }

                        // Recreate missing parent directories so files
                        // restored after their containing folder was
                        // removed land back at the original path.
                        if let Some(parent) = dest.parent() {
                            if !parent.query_exists(Cancellable::NONE) {
                                if let Err(e) = parent.make_directory_with_parents(Cancellable::NONE) {
                                    if !e.matches(gio::IOErrorEnum::Exists) {
                                        log_err("restore (mkdir)", file, Some(&dest), &e);
                                        window.show_toast(&format!(
                                            "Could not restore {name}: {e}"
                                        ));
                                        continue;
                                    }
                                }
                            }
                        }

                        let (fut, _progress) = file.move_future(
                            &dest,
                            gio::FileCopyFlags::NOFOLLOW_SYMLINKS,
                            glib::Priority::DEFAULT,
                        );
                        if let Err(e) = fut.await {
                            if !e.matches(gio::IOErrorEnum::Cancelled) {
                                log_err("restore", file, Some(&dest), &e);
                                window.show_toast(&format!(
                                    "Could not restore {name}: {e}"
                                ));
                            }
                            break 'op false;
                        }
                    }
                    true
                };
                if succeeded { handle.mark_succeeded(); }
                handle.set_fraction(1.0);
                window.op_finish(&handle);
                window.reload();
            }
        ));
    }

    /// Trash a specific list of files (used by sidebar drop on Trash).
    pub fn trash_files(&self, files: Vec<gio::File>) {
        if files.is_empty() {
            return;
        }
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = window)]
            self,
            async move {
                window.do_trash_files(files).await;
            }
        ));
    }

    pub(super) async fn do_trash_files(&self, files: Vec<gio::File>) {
        let total = files.len();
        let handle = self.op_start(OpKind::Trash);
        handle.set_item("Counting items…");
        // Pre-walk only for the count — trash_future doesn't expose progress
        // bytes, so we drive the bar by item count alone.
        let (total_items, _) = pre_walk_total(&files, false, &handle.cancellable).await;
        handle.set_total(total_items, 0);

        let mut not_supported: Vec<gio::File> = Vec::new();
        let mut last_ui = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(1))
            .unwrap_or_else(std::time::Instant::now);
        let succeeded = 'op: {
            for (idx, file) in files.iter().enumerate() {
                if handle.cancellable.is_cancelled() {
                    break 'op false;
                }
                log_op("trash", file, None);
                let now = std::time::Instant::now();
                if now.duration_since(last_ui)
                    >= std::time::Duration::from_millis(100)
                    || idx == 0
                    || idx + 1 == total
                {
                    last_ui = now;
                    let name = file
                        .basename()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    handle.set_item(&format!("{name} ({} of {total})", idx + 1));
                    handle.set_paths(file, None);
                }
                match file.trash_future(glib::Priority::DEFAULT).await {
                    Ok(()) => {
                        // Tick the per-item counter so the bar advances. The
                        // trash backend doesn't tell us how many sub-items it
                        // moved, so we assume the pre-walked count.
                        let mut s = handle.state.borrow_mut();
                        s.items_done = (idx as u64 + 1).min(s.total_items);
                        drop(s);
                        if total_items > 0 {
                            handle.set_fraction((idx + 1) as f64 / total as f64);
                        }
                    }
                    Err(e) if e.matches(gio::IOErrorEnum::NotSupported) => {
                        not_supported.push(file.clone());
                    }
                    Err(e) => {
                        log_err("trash", file, None, &e);
                        self.show_toast(&format!("Could not trash: {e}"));
                    }
                }
            }
            true
        };
        if succeeded {
            handle.mark_succeeded();
        }
        handle.set_fraction(1.0);
        self.op_finish(&handle);
        self.reload();

        if not_supported.is_empty() {
            return;
        }

        let count = not_supported.len();
        let body = if count == 1 {
            "This location does not support trash. Delete permanently instead?".to_string()
        } else {
            format!(
                "{count} items are on a location that does not support trash. \
                 Delete them permanently instead?"
            )
        };
        let dialog = adw::AlertDialog::new(Some("Cannot Move to Trash"), Some(&body));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("delete", "Delete Permanently");
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        dialog.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, response| {
                    if response != "delete" {
                        return;
                    }
                    let to_delete = not_supported.clone();
                    let total = to_delete.len();
                    let handle = window.op_start(OpKind::Delete);
                    glib::spawn_future_local(glib::clone!(
                        #[weak]
                        window,
                        #[strong]
                        handle,
                        async move {
                            let (total_items, total_bytes) =
                                pre_walk_total(&to_delete, false, &handle.cancellable).await;
                            handle.set_total(total_items, total_bytes);

                            let mut last_ui = std::time::Instant::now()
                                .checked_sub(std::time::Duration::from_secs(1))
                                .unwrap_or_else(std::time::Instant::now);
                            let succeeded = 'op: {
                            for (idx, f) in to_delete.iter().enumerate() {
                                if handle.cancellable.is_cancelled() {
                                    break 'op false;
                                }
                                log_op("delete (trash unsupported)", f, None);
                                let now = std::time::Instant::now();
                                if now.duration_since(last_ui)
                                    >= std::time::Duration::from_millis(100)
                                    || idx == 0
                                    || idx + 1 == total
                                {
                                    last_ui = now;
                                    let name = f
                                        .basename()
                                        .map(|p| p.to_string_lossy().into_owned())
                                        .unwrap_or_default();
                                    handle.set_item(&format!("{name} ({} of {total})", idx + 1));
                                    handle.set_paths(f, None);
                                }
                                if let Err(e) = delete_recursive(
                                    f.clone(),
                                    &handle.cancellable,
                                    handle.delete_callback(),
                                )
                                .await
                                {
                                    if !e.matches(gio::IOErrorEnum::Cancelled) {
                                        log_err("delete (trash unsupported)", f, None, &e);
                                        window.show_toast(&format!("Could not delete: {e}"));
                                    }
                                    break 'op false;
                                }
                            }
                            true
                            };
                            if succeeded { handle.mark_succeeded(); }
                            handle.set_fraction(1.0);
                            window.op_finish(&handle);
                            window.reload();
                        }
                    ));
                }
            ),
        );
        dialog.present(Some(self));
    }

    pub(super) fn current_location_is_trash(&self) -> bool {
        let Some(idx) = self.current_tab_index() else { return false };
        let tabs = self.imp().tabs.borrow();
        tabs.get(idx)
            .and_then(|t| t.navigation.current().cloned())
            .map(|f| f.has_uri_scheme("trash"))
            .unwrap_or(false)
    }
}
