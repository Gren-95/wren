//! File operations: delete (permanent), copy/cut/paste, duplicate, drop,
//! create symlink. The conflict-resolution dialog also lives here since
//! paste and drop_files are its only callers.

use adw::prelude::*;

use super::WrenWindow;
use super::file_ops::{
    ConflictResolution, OpKind, copy_recursive, count_items_and_bytes, delete_recursive, log_err,
    log_op, pre_walk_total, unique_dest,
};

impl WrenWindow {
    pub fn delete_permanently(&self) {
        let files = self.selected_files();
        if files.is_empty() {
            return;
        }
        let count = files.len();
        let body = if count == 1 {
            "This action cannot be undone.".to_string()
        } else {
            format!("Deleting {count} items. This action cannot be undone.")
        };
        let dialog = adw::AlertDialog::new(Some("Delete Permanently?"), Some(body.as_str()));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("delete", "Delete");
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let files_clone = files.clone();
        dialog.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, response| {
                    if response != "delete" {
                        return;
                    }
                    let files = files_clone.clone();
                    let total = files.len();
                    let handle = window.op_start(OpKind::Delete);
                    handle.set_item("Counting items…");
                    glib::spawn_future_local(glib::clone!(
                        #[weak]
                        window,
                        #[strong]
                        handle,
                        async move {
                            let (total_items, total_bytes) =
                                pre_walk_total(&files, false, &handle.cancellable).await;
                            handle.set_total(total_items, total_bytes);

                            let mut last_ui = std::time::Instant::now()
                                .checked_sub(std::time::Duration::from_secs(1))
                                .unwrap_or_else(std::time::Instant::now);
                            let succeeded = 'op: {
                            for (idx, file) in files.iter().enumerate() {
                                if handle.cancellable.is_cancelled() {
                                    break 'op false;
                                }
                                log_op("delete", file, None);
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
                                if let Err(e) = delete_recursive(
                                    file.clone(),
                                    &handle.cancellable,
                                    handle.delete_callback(),
                                )
                                .await
                                {
                                    if !e.matches(gio::IOErrorEnum::Cancelled) {
                                        log_err("delete", file, None, &e);
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

    pub fn copy_selection(&self) {
        let files = self.selected_files();
        if files.is_empty() {
            return;
        }
        let uris: Vec<String> = files.iter().map(|f| f.uri().to_string()).collect();
        self.imp()
            .clipboard_files
            .replace(Some((files, false)));
        self.clipboard().set_text(&uris.join("\r\n"));
        self.update_cut_indicator(&[]);
        self.update_selection_actions();
        self.show_toast("Copied");
    }

    pub fn cut_selection(&self) {
        let files = self.selected_files();
        if files.is_empty() {
            return;
        }
        let uris: Vec<String> = files.iter().map(|f| f.uri().to_string()).collect();
        self.imp()
            .clipboard_files
            .replace(Some((files, true)));
        self.clipboard().set_text(&uris.join("\r\n"));
        self.update_cut_indicator(&uris);
        self.update_selection_actions();
        self.show_toast("Cut");
    }

    fn update_cut_indicator(&self, uris: &[String]) {
        let Some(idx) = self.current_tab_index() else {
            return;
        };
        let tabs = self.imp().tabs.borrow();
        if let Some(tab) = tabs.get(idx) {
            tab.file_grid.set_cut_uris(uris);
            tab.file_list.set_cut_uris(uris);
        }
    }

    pub fn paste(&self) {
        let dest_dir = {
            let Some(idx) = self.current_tab_index() else {
                return;
            };
            let tabs = self.imp().tabs.borrow();
            match tabs.get(idx).and_then(|t| t.navigation.current().cloned()) {
                Some(d) => d,
                None => return,
            }
        };
        let clipboard_data = self.imp().clipboard_files.borrow().clone();
        let Some((files, is_cut)) = clipboard_data else {
            self.show_toast("Nothing to paste");
            return;
        };
        let kind = if is_cut { OpKind::Move } else { OpKind::Copy };
        let total = files.len();
        let handle = self.op_start(kind);
        handle.set_item("Counting items…");
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[strong]
            handle,
            async move {
                // For move (cut+paste), every item is touched twice — copy
                // then post-move delete — so the totals double.
                let (total_items, total_bytes) =
                    pre_walk_total(&files, is_cut, &handle.cancellable).await;
                handle.set_total(total_items, total_bytes);

                let mut policy: Option<ConflictResolution> = None;
                let succeeded = 'op: {
                for (idx, file) in files.iter().enumerate() {
                    if handle.cancellable.is_cancelled() {
                        break 'op false;
                    }
                    let Some(name) = file.basename() else {
                        continue;
                    };
                    // Cut + paste back into the source dir is a no-op.
                    if is_cut && dest_dir.child(&name).equal(file) {
                        continue;
                    }
                    let action = if is_cut { "move" } else { "copy" };
                    let display_name = name.to_string_lossy();
                    handle.set_item(&format!("{display_name} ({} of {total})", idx + 1));
                    handle.set_paths(file, Some(&dest_dir.child(&name)));

                    let dest_initial = dest_dir.child(&name);
                    let collides = !dest_initial.equal(file)
                        && dest_initial.query_exists(gio::Cancellable::NONE);
                    let resolution = if collides {
                        match policy {
                            Some(r) => r,
                            None => {
                                let (r, apply) = window.resolve_conflict(&display_name).await;
                                if apply {
                                    policy = Some(r);
                                }
                                r
                            }
                        }
                    } else {
                        ConflictResolution::Rename
                    };
                    let dest = match resolution {
                        ConflictResolution::Skip => continue,
                        ConflictResolution::Cancel => break 'op false,
                        ConflictResolution::Replace => {
                            // Replace's pre-delete: show path activity but
                            // don't tick the main counter (these items aren't
                            // in the pre-walked total).
                            if let Err(e) = delete_recursive(
                                dest_initial.clone(),
                                &handle.cancellable,
                                handle.paths_only_callback(),
                            )
                            .await
                            {
                                if !e.matches(gio::IOErrorEnum::Cancelled) {
                                    log_err("replace (delete existing)", &dest_initial, None, &e);
                                    window.show_toast(&format!("Could not replace: {e}"));
                                }
                                break 'op false;
                            }
                            dest_initial
                        }
                        ConflictResolution::Rename => match unique_dest(&dest_dir, &name) {
                            Some(d) => d,
                            None => {
                                window.show_toast(&format!(
                                    "Could not find a free name for {display_name}"
                                ));
                                continue;
                            }
                        },
                    };
                    log_op(action, file, Some(&dest));

                    if let Err(e) = copy_recursive(
                        file.clone(),
                        dest.clone(),
                        &handle.cancellable,
                        handle.copy_callback(),
                    )
                    .await
                    {
                        if !e.matches(gio::IOErrorEnum::Cancelled) {
                            log_err(action, file, Some(&dest), &e);
                            window.show_toast(&format!("Could not paste: {e}"));
                        }
                        break 'op false;
                    }
                    if is_cut {
                        log_op("delete (post-move)", file, None);
                        if let Err(e) = delete_recursive(
                            file.clone(),
                            &handle.cancellable,
                            handle.delete_callback(),
                        )
                        .await
                        {
                            if !e.matches(gio::IOErrorEnum::Cancelled) {
                                log_err("delete (post-move)", file, None, &e);
                                window.show_toast(&format!("Could not move: {e}"));
                            }
                            break 'op false;
                        }
                    }
                }
                true
                };
                if succeeded { handle.mark_succeeded(); }
                handle.set_fraction(1.0);
                if is_cut {
                    window.imp().clipboard_files.replace(None);
                    window.update_cut_indicator(&[]);
                    window.update_selection_actions();
                }
                window.op_finish(&handle);
                window.reload();
            }
        ));
    }

    pub fn create_link(&self) {
        let Some(file) = self.selected_files().into_iter().next() else {
            return;
        };
        let dest_dir = {
            let Some(idx) = self.current_tab_index() else {
                return;
            };
            let tabs = self.imp().tabs.borrow();
            tabs.get(idx).and_then(|t| t.navigation.current().cloned())
        };
        let Some(dest_dir) = dest_dir else { return };

        let Some(target_path) = file.path() else {
            self.show_toast("Cannot create link: not a local file");
            return;
        };
        // Explicitly require a local dest dir; unwrap_or_default() would give an
        // empty PathBuf and create the symlink in the process working directory.
        let Some(dest_dir_path) = dest_dir.path() else {
            self.show_toast("Cannot create link: current directory is not local");
            return;
        };
        let Some(name) = file.basename() else { return };

        // Place "(link)" before the extension: "photo (link).jpg" not "photo.jpg (link)"
        let stem = name.file_stem().and_then(|s| s.to_str()).unwrap_or("link");
        let ext  = name.extension().and_then(|s| s.to_str());

        let make_name = |suffix: &str| match ext {
            Some(e) => format!("{stem}{suffix}.{e}"),
            None    => format!("{stem}{suffix}"),
        };

        // Find a non-colliding link path. Cap at 1000 attempts so a
        // pathological directory doesn't spin forever.
        let link_path = {
            let first = dest_dir_path.join(make_name(" (link)"));
            if !first.exists() {
                first
            } else {
                let found = (2u32..=1000).find_map(|i| {
                    let p = dest_dir_path.join(make_name(&format!(" (link {i})")));
                    (!p.exists()).then_some(p)
                });
                match found {
                    Some(p) => p,
                    None => {
                        self.show_toast("Could not find a free name for the link");
                        return;
                    }
                }
            }
        };

        crate::wren_log!(
            "symlink: {} -> {}",
            link_path.display(),
            target_path.display()
        );
        match std::os::unix::fs::symlink(&target_path, &link_path) {
            Ok(()) => self.reload(),
            Err(e) => {
                crate::wren_log!(
                    "symlink failed: {} -> {}: {e}",
                    link_path.display(),
                    target_path.display()
                );
                self.show_toast(&format!("Could not create link: {e}"))
            }
        }
    }

    async fn resolve_conflict(&self, name: &str) -> (ConflictResolution, bool) {
        let dialog = adw::AlertDialog::new(
            Some("File already exists"),
            Some(&format!("\"{name}\" already exists in the destination.")),
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("skip", "Skip");
        dialog.add_response("rename", "Rename");
        dialog.add_response("replace", "Replace");
        dialog.set_response_appearance("replace", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("rename"));
        dialog.set_close_response("cancel");

        let apply_to_all = gtk4::CheckButton::with_label("Apply to all conflicts in this operation");
        dialog.set_extra_child(Some(&apply_to_all));

        let response = dialog.choose_future(Some(self)).await;
        let apply = apply_to_all.is_active();
        let res = match response.as_str() {
            "skip" => ConflictResolution::Skip,
            "replace" => ConflictResolution::Replace,
            "rename" => ConflictResolution::Rename,
            _ => ConflictResolution::Cancel,
        };
        (res, apply)
    }

    // ── About ────────────────────────────────────────────────────────────────

    pub fn duplicate(&self) {
        let current_dir = {
            let Some(idx) = self.current_tab_index() else { return };
            let tabs = self.imp().tabs.borrow();
            tabs.get(idx).and_then(|t| t.navigation.current().cloned())
        };
        let Some(dest_dir) = current_dir else { return };
        let Some(dest_dir_path) = dest_dir.path() else {
            self.show_toast("Cannot duplicate: current directory is not local");
            return;
        };

        let files = self.selected_files();
        if files.is_empty() { return; }

        for file in files {
            let Some(src_path) = file.path() else { continue };
            let name = src_path.file_name().unwrap_or_default();
            let stem = std::path::Path::new(name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("copy");
            let ext = std::path::Path::new(name)
                .extension()
                .and_then(|s| s.to_str());
            let make_name = |suffix: &str| match ext {
                Some(e) => format!("{stem}{suffix}.{e}"),
                None => format!("{stem}{suffix}"),
            };
            let dest_path = {
                let first = dest_dir_path.join(make_name(" (copy)"));
                if !first.exists() {
                    first
                } else {
                    match (2u32..=1000).find_map(|i| {
                        let p = dest_dir_path.join(make_name(&format!(" (copy {i})")));
                        (!p.exists()).then_some(p)
                    }) {
                        Some(p) => p,
                        None => {
                            self.show_toast(&format!(
                                "Could not find a free name for {}",
                                name.to_string_lossy()
                            ));
                            continue;
                        }
                    }
                }
            };
            let dest_file = gio::File::for_path(&dest_path);
            let display_name = file
                .basename()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let handle = self.op_start(OpKind::Duplicate);
            handle.set_item(&display_name);
            handle.set_paths(&file, Some(&dest_file));
            glib::spawn_future_local(glib::clone!(
                #[weak(rename_to = window)]
                self,
                #[strong]
                handle,
                async move {
                    log_op("duplicate", &file, Some(&dest_file));
                    let (total_items, total_bytes) = count_items_and_bytes(file.clone(), &handle.cancellable).await;
                    handle.set_total(total_items, total_bytes);
                    match copy_recursive(
                        file.clone(),
                        dest_file.clone(),
                        &handle.cancellable,
                        handle.copy_callback(),
                    )
                    .await
                    {
                        Ok(()) => {
                            handle.mark_succeeded();
                            handle.set_fraction(1.0);
                            window.op_finish(&handle);
                            window.reload();
                        }
                        Err(e) => {
                            window.op_finish(&handle);
                            if !e.matches(gio::IOErrorEnum::Cancelled) {
                                log_err("duplicate", &file, Some(&dest_file), &e);
                                window.show_toast(&format!("Could not duplicate: {e}"));
                            }
                        }
                    }
                }
            ));
        }
    }

    pub fn drop_files(&self, files: Vec<gio::File>, dest: Option<gio::File>, is_move: bool) {
        let dest_dir = if let Some(d) = dest {
            d
        } else {
            let Some(idx) = self.current_tab_index() else { return };
            let tabs = self.imp().tabs.borrow();
            match tabs.get(idx).and_then(|t| t.navigation.current().cloned()) {
                Some(d) => d,
                None => return,
            }
        };
        let kind = if is_move { OpKind::Move } else { OpKind::Copy };
        let total = files.len();
        let handle = self.op_start(kind);
        handle.set_item("Counting items…");
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[strong]
            handle,
            async move {
                let (total_items, total_bytes) =
                    pre_walk_total(&files, is_move, &handle.cancellable).await;
                handle.set_total(total_items, total_bytes);

                let mut policy: Option<ConflictResolution> = None;
                let succeeded = 'op: {
                for (idx, file) in files.iter().enumerate() {
                    if handle.cancellable.is_cancelled() {
                        break 'op false;
                    }
                    let Some(name) = file.basename() else { continue };
                    // Dropping a file onto its own parent dir is a no-op.
                    if dest_dir.child(&name).equal(file) { continue; }
                    let action = if is_move { "drop-move" } else { "drop-copy" };
                    let display_name = name.to_string_lossy();
                    handle.set_item(&format!("{display_name} ({} of {total})", idx + 1));
                    handle.set_paths(file, Some(&dest_dir.child(&name)));

                    let dest_initial = dest_dir.child(&name);
                    let collides = !dest_initial.equal(file)
                        && dest_initial.query_exists(gio::Cancellable::NONE);
                    let resolution = if collides {
                        match policy {
                            Some(r) => r,
                            None => {
                                let (r, apply) = window.resolve_conflict(&display_name).await;
                                if apply {
                                    policy = Some(r);
                                }
                                r
                            }
                        }
                    } else {
                        ConflictResolution::Rename
                    };
                    let dest = match resolution {
                        ConflictResolution::Skip => continue,
                        ConflictResolution::Cancel => break 'op false,
                        ConflictResolution::Replace => {
                            // Replace's pre-delete: show path activity but
                            // don't tick the main counter (these items aren't
                            // in the pre-walked total).
                            if let Err(e) = delete_recursive(
                                dest_initial.clone(),
                                &handle.cancellable,
                                handle.paths_only_callback(),
                            )
                            .await
                            {
                                if !e.matches(gio::IOErrorEnum::Cancelled) {
                                    log_err("replace (delete existing)", &dest_initial, None, &e);
                                    window.show_toast(&format!("Could not replace: {e}"));
                                }
                                break 'op false;
                            }
                            dest_initial
                        }
                        ConflictResolution::Rename => match unique_dest(&dest_dir, &name) {
                            Some(d) => d,
                            None => {
                                window.show_toast(&format!(
                                    "Could not find a free name for {display_name}"
                                ));
                                continue;
                            }
                        },
                    };
                    log_op(action, file, Some(&dest));

                    if let Err(e) = copy_recursive(
                        file.clone(),
                        dest.clone(),
                        &handle.cancellable,
                        handle.copy_callback(),
                    )
                    .await
                    {
                        if !e.matches(gio::IOErrorEnum::Cancelled) {
                            log_err(action, file, Some(&dest), &e);
                            window.show_toast(&format!("Could not copy: {e}"));
                        }
                        break 'op false;
                    }
                    if is_move {
                        log_op("delete (post-move)", file, None);
                        if let Err(e) = delete_recursive(
                            file.clone(),
                            &handle.cancellable,
                            handle.delete_callback(),
                        )
                        .await
                        {
                            if !e.matches(gio::IOErrorEnum::Cancelled) {
                                log_err("delete (post-move)", file, None, &e);
                                window.show_toast(&format!("Could not remove source: {e}"));
                            }
                            break 'op false;
                        }
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

}
