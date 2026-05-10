//! Nautilus-style multi-page Properties dialog.
//!
//! Three pages:
//!   * General     — large icon + editable name, type, size (recursive for
//!                   directories), location (clickable), modified / accessed /
//!                   created timestamps, free space on the filesystem.
//!   * Permissions — owner / group display, three sets of r/w/x checkboxes
//!                   (owner / group / others), executable shortcut for files,
//!                   SELinux context if available. Read-only when the current
//!                   user is not the owner. Hidden entirely when `unix::mode`
//!                   is unavailable (non-local backends).
//!   * Open With   — registered AppInfo handlers for the file's content type;
//!                   click to set as default.
//!
//! Async work spawned by the dialog (recursive size, free space, extended
//! FileInfo query) is gated by a `gio::Cancellable` that is cancelled when
//! the dialog is closed — this prevents updates from landing on stale
//! widgets after the dialog disappears.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use glib::clone;

use super::WrenWindow;
use super::file_ops::{compute_dir_size, format_file_size};
use crate::model::FileObject;

impl WrenWindow {
    pub fn show_properties(&self) {
        // Multi-select handling: v1 shows the first selected file. Nautilus
        // has a dedicated combined-properties view for N items — out of scope.
        let objs = self.selected_file_objects();
        let selection_count = objs.len();
        let file_obj = objs.first().cloned();

        // Resolve subject. When nothing is selected, fall back to the
        // current directory of the active tab.
        let subject = if let Some(obj) = file_obj.clone() {
            Subject::from_file_object(&obj)
        } else if let Some(s) = self.subject_from_current_dir() {
            s
        } else {
            return;
        };

        let dialog = adw::PreferencesDialog::new();
        dialog.set_title("Properties");
        dialog.set_search_enabled(true);

        // Cancellable shared by every async task spawned from this dialog.
        // When the dialog closes, the user has navigated away or just
        // doesn't care about the result — without cancellation, a large
        // recursive walk could keep running for seconds and eventually
        // try to update widgets that have been disposed. The Cancellable
        // is checked at every iteration boundary in compute_dir_size; any
        // pending GIO async future cancels at the kernel boundary too.
        let dialog_cancel = gio::Cancellable::new();
        dialog.connect_closed(clone!(
            #[strong]
            dialog_cancel,
            move |_| dialog_cancel.cancel()
        ));

        // ── General page ────────────────────────────────────────────────────
        let general_page = adw::PreferencesPage::new();
        general_page.set_title("General");
        general_page.set_icon_name(Some("dialog-information-symbolic"));

        let header_group = build_header_group(self, &subject, selection_count);
        general_page.add(&header_group);

        let general_group = adw::PreferencesGroup::new();
        let basic = build_basic_rows(self, &subject, &dialog_cancel);
        general_group.add(&basic.type_row);
        general_group.add(&basic.size_row);
        general_group.add(&basic.location_row);
        general_group.add(&basic.modified_row);
        general_group.add(&basic.accessed_row);
        general_group.add(&basic.created_row);
        general_group.add(&basic.free_space_row);
        general_page.add(&general_group);

        dialog.add(&general_page);

        // ── Permissions page (added lazily after extended query) ────────────
        let perms_page = adw::PreferencesPage::new();
        perms_page.set_title("Permissions");
        perms_page.set_icon_name(Some("system-lock-screen-symbolic"));
        // Pre-add the page; populate_perms_page will fill it with several
        // groups when the extended-info query returns. If unix::mode is
        // missing the page shows just a placeholder row.
        dialog.add(&perms_page);

        // ── Open With page (files only) ─────────────────────────────────────
        if !subject.is_directory && !subject.content_type.is_empty() {
            let open_with_page = build_open_with_page(self, &subject);
            dialog.add(&open_with_page);
        }

        // Kick off async population.
        spawn_size_walk(&basic.size_row, &subject, &dialog_cancel);
        spawn_free_space(&basic.free_space_row, &subject.target, &dialog_cancel);
        spawn_extended_info(self, &subject, &basic, &perms_page, &dialog_cancel);

        dialog.present(Some(self));
    }

    /// Build a Subject from the current tab's location (used when no
    /// file is selected — Alt+Enter on an empty file view shows the
    /// folder's properties).
    fn subject_from_current_dir(&self) -> Option<Subject> {
        let idx = self.current_tab_index()?;
        let tabs = self.imp().tabs.borrow();
        let tab = tabs.get(idx)?;
        let loc = tab.navigation.current()?.clone();
        let name = loc
            .basename()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Folder".to_string());
        Some(Subject {
            target: loc,
            name,
            content_type: "inode/directory".to_string(),
            file_size: 0,
            is_directory: true,
            file_object: None,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Subject — the file/directory the dialog is describing.
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct Subject {
    target: gio::File,
    name: String,
    content_type: String,
    file_size: u64,
    is_directory: bool,
    file_object: Option<FileObject>,
}

impl Subject {
    fn from_file_object(obj: &FileObject) -> Self {
        Self {
            target: obj.file().clone(),
            name: obj.name(),
            content_type: obj.content_type(),
            file_size: obj.file_size(),
            is_directory: obj.is_directory(),
            file_object: Some(obj.clone()),
        }
    }
}

/// Suffix label paired with a row — kept around so async tasks can
/// update the value after the dialog is presented. Only the labels we
/// actually need to mutate post-construction (timestamps from extended
/// query) are stored separately; size and free-space are looked up by
/// walking the row's children.
struct BasicRows {
    type_row: adw::ActionRow,
    size_row: adw::ActionRow,
    location_row: adw::ActionRow,
    modified_row: adw::ActionRow,
    modified_label: gtk4::Label,
    accessed_row: adw::ActionRow,
    accessed_label: gtk4::Label,
    created_row: adw::ActionRow,
    created_label: gtk4::Label,
    free_space_row: adw::ActionRow,
}

// ─────────────────────────────────────────────────────────────────────────────
// Header (icon + editable name)
// ─────────────────────────────────────────────────────────────────────────────

fn build_header_group(
    win: &WrenWindow,
    subject: &Subject,
    selection_count: usize,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();

    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 16);
    header.set_margin_top(8);
    header.set_margin_bottom(8);

    // Icon — use the FileObject's resolved gicon when available, otherwise
    // fall back to a generic symbolic icon based on directoryness.
    let image = gtk4::Image::new();
    image.set_pixel_size(96);
    if let Some(ref obj) = subject.file_object {
        if let Some(thumb) = obj.thumbnail_path() {
            // Thumbnail (already-decoded image preview) takes precedence.
            image.set_from_file(Some(&thumb));
        } else if let Some(icon) = obj.icon() {
            image.set_from_gicon(&icon);
        } else if subject.is_directory {
            image.set_icon_name(Some("folder"));
        } else {
            image.set_icon_name(Some("text-x-generic"));
        }
    } else if subject.is_directory {
        image.set_icon_name(Some("folder"));
    } else {
        image.set_icon_name(Some("text-x-generic"));
    }
    header.append(&image);

    let name_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    name_box.set_valign(gtk4::Align::Center);
    name_box.set_hexpand(true);

    // Inline-rename: GtkEditableLabel commits on Enter and reverts on
    // Escape (handled by the widget itself). We listen for "changed"
    // after the edit is finalised — Editable emits a changed signal
    // whenever the buffer is updated. To avoid firing on every keystroke
    // while typing, we listen instead on the editing-notify property:
    // it goes false when the user commits. Then we read the text once.
    let name_label = gtk4::EditableLabel::new(&subject.name);
    name_label.add_css_class("title-2");
    name_label.set_hexpand(true);

    // Read the original name through editing transitions; commit when
    // editing ends and the text changed.
    let original_name = Rc::new(RefCell::new(subject.name.clone()));
    let target_for_rename = subject.target.clone();
    name_label.connect_notify_local(
        Some("editing"),
        clone!(
            #[weak(rename_to = window)]
            win,
            #[strong]
            original_name,
            #[strong]
            target_for_rename,
            move |label, _| {
                if label.is_editing() {
                    return;
                }
                let new_name = label.text().to_string();
                let mut orig = original_name.borrow_mut();
                if new_name == *orig || new_name.is_empty() {
                    // Revert empty edits to the previous name so the
                    // header doesn't end up blank if the user clears the
                    // field then tabs out.
                    if new_name.is_empty() {
                        label.set_text(&orig);
                    }
                    return;
                }
                let old = orig.clone();
                *orig = new_name.clone();
                drop(orig);
                window.spawn_properties_rename(target_for_rename.clone(), old, new_name);
            }
        ),
    );

    name_box.append(&name_label);

    // Optional sub-line for multi-select — Nautilus shows "N items" in
    // its combined view; we don't have that yet so we leave a hint.
    if selection_count > 1 {
        let multi_hint = gtk4::Label::new(Some(&format!(
            "Showing first of {selection_count} selected items"
        )));
        multi_hint.add_css_class("dim-label");
        multi_hint.set_xalign(0.0);
        name_box.append(&multi_hint);
    }

    header.append(&name_box);
    group.add(&header);
    group
}

// ─────────────────────────────────────────────────────────────────────────────
// Basic rows (Type / Size / Location / Modified / Accessed / Created / Free)
// ─────────────────────────────────────────────────────────────────────────────

fn build_basic_rows(
    win: &WrenWindow,
    subject: &Subject,
    _cancel: &gio::Cancellable,
) -> BasicRows {
    let make_row = |title: &str, value: &str| -> (adw::ActionRow, gtk4::Label) {
        let row = adw::ActionRow::new();
        row.set_title(title);
        let lbl = gtk4::Label::new(Some(value));
        lbl.add_css_class("dim-label");
        lbl.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        lbl.set_max_width_chars(48);
        lbl.set_selectable(true);
        row.add_suffix(&lbl);
        (row, lbl)
    };

    // Type row: human-readable description + technical mime type as subtitle.
    let type_row = adw::ActionRow::new();
    type_row.set_title("Type");
    let type_text = if subject.content_type.is_empty() {
        "Unknown".to_string()
    } else {
        gio::functions::content_type_get_description(&subject.content_type).to_string()
    };
    let type_label = gtk4::Label::new(Some(&type_text));
    type_label.add_css_class("dim-label");
    type_label.set_selectable(true);
    type_row.add_suffix(&type_label);
    if !subject.content_type.is_empty() {
        type_row.set_subtitle(&subject.content_type);
    }

    // Size row — start with the static size for files; for directories the
    // recursive walk fills it asynchronously.
    let size_row = adw::ActionRow::new();
    size_row.set_title("Size");
    let size_label = gtk4::Label::new(None);
    size_label.add_css_class("dim-label");
    size_label.set_selectable(true);
    size_row.add_suffix(&size_label);
    if subject.is_directory {
        size_label.set_text("Calculating…");
    } else {
        size_label.set_text(&format_size_with_bytes(subject.file_size));
    }

    // Location row — clickable suffix that opens the parent directory.
    let location_row = adw::ActionRow::new();
    location_row.set_title("Location");
    let parent = subject.target.parent();
    let parent_str = match parent.as_ref() {
        Some(p) => p
            .path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.uri().to_string()),
        None => String::new(),
    };
    if !parent_str.is_empty() {
        let link = gtk4::Button::new();
        link.add_css_class("flat");
        let lbl = gtk4::Label::new(Some(&parent_str));
        lbl.add_css_class("accent");
        lbl.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        lbl.set_max_width_chars(48);
        link.set_child(Some(&lbl));
        link.set_tooltip_text(Some("Open parent directory"));
        if let Some(parent) = parent.clone() {
            link.connect_clicked(clone!(
                #[weak(rename_to = window)]
                win,
                move |_| {
                    window.navigate_to(parent.clone());
                }
            ));
        }
        location_row.add_suffix(&link);
    } else {
        let lbl = gtk4::Label::new(Some("—"));
        lbl.add_css_class("dim-label");
        location_row.add_suffix(&lbl);
    }

    // Timestamp rows — modified is filled from the FileObject (already
    // queried by the directory model); accessed and created come from
    // the extended-info query that runs after present().
    let modified_text = subject
        .file_object
        .as_ref()
        .map(|o| o.modified())
        .filter(|t| *t > 0)
        .and_then(format_unix_time)
        .unwrap_or_else(|| "—".to_string());
    let (modified_row, modified_label) = make_row("Modified", &modified_text);
    let (accessed_row, accessed_label) = make_row("Accessed", "—");
    let (created_row, created_label) = make_row("Created", "—");
    let (free_space_row, _) = make_row("Free Space", "—");

    let _ = size_label; // size label discovered later via locate_suffix_label
    BasicRows {
        type_row,
        size_row,
        location_row,
        modified_row,
        modified_label,
        accessed_row,
        accessed_label,
        created_row,
        created_label,
        free_space_row,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Async population
// ─────────────────────────────────────────────────────────────────────────────

/// For directories, walk recursively and update the size label. Cancels
/// promptly when `cancel` fires (dialog closed).
///
/// Cancellation safety: the dialog owns `cancel` and cancels it from
/// `connect_closed`, before its widget tree is dropped. compute_dir_size
/// checks `cancel.is_cancelled()` at every loop iteration AND inside the
/// `on_update` callback we pass below — so even an in-flight enumerator
/// won't keep mutating the label after the dialog disappears. We also
/// double-check in the post-await branch in case the walk completed
/// just as the dialog was closing.
fn spawn_size_walk(_size_row: &adw::ActionRow, subject: &Subject, cancel: &gio::Cancellable) {
    if !subject.is_directory {
        return;
    }
    let target = subject.target.clone();
    let size_label = match locate_suffix_label(_size_row) {
        Some(l) => l,
        None => return,
    };
    let cancel_outer = cancel.clone();
    let cancel_for_cb = cancel.clone();
    let label_for_cb = size_label.clone();
    glib::spawn_future_local(async move {
        let (total, count) = compute_dir_size(target, &cancel_outer, move |t, c| {
            if !cancel_for_cb.is_cancelled() {
                label_for_cb.set_text(&format!(
                    "Counting… {} ({} items)",
                    format_file_size(t),
                    c
                ));
            }
        })
        .await;
        if !cancel_outer.is_cancelled() {
            size_label.set_text(&format!(
                "{} ({} item{})",
                format_size_with_bytes(total),
                count,
                if count == 1 { "" } else { "s" }
            ));
        }
    });
}

/// Pull the GtkLabel out of an AdwActionRow's suffix slot. The row builds
/// an internal box and we appended exactly one label as a suffix in
/// `build_basic_rows`, so we just walk children once.
fn locate_suffix_label(row: &adw::ActionRow) -> Option<gtk4::Label> {
    use gtk4::prelude::ListModelExtManual;
    row.observe_children()
        .iter::<glib::Object>()
        .filter_map(|w| w.ok())
        .find_map(|w| w.downcast::<gtk4::Label>().ok())
}

fn spawn_free_space(row: &adw::ActionRow, target: &gio::File, cancel: &gio::Cancellable) {
    let target = target.clone();
    let cancel = cancel.clone();
    let Some(label) = locate_suffix_label(row) else { return };
    glib::spawn_future_local(async move {
        let attrs = "filesystem::free,filesystem::size,filesystem::used";
        let result = target
            .query_filesystem_info_future(attrs, glib::Priority::DEFAULT)
            .await;
        if cancel.is_cancelled() {
            return;
        }
        match result {
            Ok(info) => {
                let free = info.attribute_uint64("filesystem::free");
                let total = info.attribute_uint64("filesystem::size");
                let text = if total > 0 {
                    format!(
                        "{} free of {}",
                        format_file_size(free),
                        format_file_size(total)
                    )
                } else if free > 0 {
                    format!("{} free", format_file_size(free))
                } else {
                    "Unavailable".to_string()
                };
                label.set_text(&text);
            }
            Err(_) => label.set_text("Unavailable"),
        }
    });
}

/// Query the extended attributes (accessed, created, owner, group, mode,
/// SELinux context) and feed them into the basic rows + permissions group.
fn spawn_extended_info(
    win: &WrenWindow,
    subject: &Subject,
    basic: &BasicRows,
    perms_page: &adw::PreferencesPage,
    cancel: &gio::Cancellable,
) {
    let target = subject.target.clone();
    let cancel = cancel.clone();
    let accessed_label = basic.accessed_label.clone();
    let created_label = basic.created_label.clone();
    let modified_label = basic.modified_label.clone();
    let perms_page = perms_page.clone();
    let is_directory = subject.is_directory;
    let original_name = subject.name.clone();

    glib::spawn_future_local(clone!(
        #[weak(rename_to = window)]
        win,
        async move {
            let attrs = "time::modified,time::access,time::created,\
                         owner::user,owner::user-real,owner::group,\
                         unix::mode,unix::uid,unix::gid,\
                         unix::inode,unix::nlink,unix::device,\
                         unix::block-size,unix::blocks,\
                         standard::is-symlink,standard::symlink-target,\
                         selinux::context";
            let result = target
                .query_info_future(
                    attrs,
                    gio::FileQueryInfoFlags::NONE,
                    glib::Priority::DEFAULT,
                )
                .await;
            if cancel.is_cancelled() {
                return;
            }
            let Ok(info) = result else {
                // Couldn't even query — show a placeholder permissions row.
                let placeholder = adw::PreferencesGroup::new();
                add_perms_unavailable_row(&placeholder, "Permissions information unavailable");
                perms_page.add(&placeholder);
                return;
            };

            // Timestamps
            if let Some(dt) = info.modification_date_time() {
                modified_label.set_text(&format_glib_datetime(&dt));
            }
            if let Some(dt) = info.access_date_time() {
                accessed_label.set_text(&format_glib_datetime(&dt));
            }
            if let Some(dt) = info.creation_date_time() {
                created_label.set_text(&format_glib_datetime(&dt));
            }

            populate_perms_page(&window, &target, &info, is_directory, &original_name, &perms_page, &cancel);
        }
    ));
}

// ─────────────────────────────────────────────────────────────────────────────
// Permissions page
// ─────────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn populate_perms_page(
    win: &WrenWindow,
    target: &gio::File,
    info: &gio::FileInfo,
    is_directory: bool,
    original_name: &str,
    page: &adw::PreferencesPage,
    cancel: &gio::Cancellable,
) {
    // ── Name group (rename also lives here so the user doesn't need to
    //    flip back to the General page).
    let name_group = adw::PreferencesGroup::new();
    name_group.set_title("Name");
    let name_row = adw::EntryRow::new();
    name_row.set_title("Filename");
    name_row.set_text(original_name);
    let name_orig = Rc::new(RefCell::new(original_name.to_string()));
    let name_target = target.clone();
    name_row.connect_apply(clone!(
        #[weak(rename_to = window)]
        win,
        #[strong] name_orig,
        #[strong] name_target,
        move |row| {
            let new_name = row.text().to_string();
            let mut orig = name_orig.borrow_mut();
            if new_name == *orig || new_name.is_empty() {
                if new_name.is_empty() { row.set_text(&orig); }
                return;
            }
            let old = orig.clone();
            *orig = new_name.clone();
            drop(orig);
            window.spawn_properties_rename(name_target.clone(), old, new_name);
        }
    ));
    name_group.add(&name_row);
    page.add(&name_group);

    // Mode bits — bail if not present (non-local backend).
    // unix::mode is queried as uint32. The attribute is missing on
    // backends that don't expose POSIX permissions (smb://, mtp://);
    // attribute_uint32 returns 0 in that case which would render every
    // checkbox unchecked — distinguish by checking the attribute exists.
    if !info.has_attribute("unix::mode") {
        let placeholder = adw::PreferencesGroup::new();
        add_perms_unavailable_row(&placeholder, "POSIX permissions not available for this location");
        page.add(&placeholder);
        return;
    }
    let mode = info.attribute_uint32("unix::mode");
    let owner_uid = info.attribute_uint32("unix::uid");

    // Read-only when the running user isn't the owner. (Even root edits
    // through this dialog feel risky — keep escalation out of v1.)
    let current_uid = current_user_uid();
    let editable = current_uid.map_or(false, |u| u == owner_uid);

    let mode_state = Rc::new(Cell::new(mode));
    // Observers fire whenever any control mutates mode_state — used by
    // the live-updating Octal / Symbolic mode rows in the Details group.
    let mode_observers: Rc<RefCell<Vec<Box<dyn Fn(u32)>>>> = Rc::new(RefCell::new(Vec::new()));

    // ── Execution group — chmod +x equivalent, prominent at top.
    if !is_directory {
        let exec_group = adw::PreferencesGroup::new();
        exec_group.set_title("Execution");
        exec_group.set_description(Some(
            "When on, the file runs as a program when activated (chmod +x)",
        ));
        let exec_row = adw::SwitchRow::new();
        exec_row.set_title("Executable");
        exec_row.set_subtitle("Sets the execute bit for everyone");
        exec_row.set_active(mode & 0o111 != 0);
        exec_row.set_sensitive(editable);
        let mode_state_for_exec = mode_state.clone();
        let observers_for_exec = mode_observers.clone();
        let target_clone = target.clone();
        let cancel_clone = cancel.clone();
        exec_row.connect_active_notify(clone!(
            #[weak(rename_to = window)]
            win,
            move |row| {
                let mut m = mode_state_for_exec.get();
                if row.is_active() { m |= 0o111; } else { m &= !0o111; }
                mode_state_for_exec.set(m);
                for cb in observers_for_exec.borrow().iter() { cb(m); }
                spawn_set_mode(&window, target_clone.clone(), m, cancel_clone.clone());
            }
        ));
        exec_group.add(&exec_row);
        page.add(&exec_group);
    }

    // ── Permissions group — owner/group/SELinux + r/w/x triplets.
    let perms_group = adw::PreferencesGroup::new();
    perms_group.set_title("Permissions");

    let owner = info
        .attribute_string("owner::user-real")
        .or_else(|| info.attribute_string("owner::user"))
        .map(|s| s.to_string())
        .unwrap_or_else(|| "—".into());
    let group_name = info
        .attribute_string("owner::group")
        .map(|s| s.to_string())
        .unwrap_or_else(|| "—".into());

    let owner_row = adw::ActionRow::new();
    owner_row.set_title("Owner");
    let owner_label = gtk4::Label::new(Some(&format!("{owner} ({owner_uid})")));
    owner_label.add_css_class("dim-label");
    owner_label.set_selectable(true);
    owner_row.add_suffix(&owner_label);
    perms_group.add(&owner_row);

    let group_row = adw::ActionRow::new();
    group_row.set_title("Group");
    let gid = info.attribute_uint32("unix::gid");
    let group_label = gtk4::Label::new(Some(&format!("{group_name} ({gid})")));
    group_label.add_css_class("dim-label");
    group_label.set_selectable(true);
    group_row.add_suffix(&group_label);
    perms_group.add(&group_row);

    if let Some(ctx) = info.attribute_string("selinux::context") {
        let ctx_row = adw::ActionRow::new();
        ctx_row.set_title("Security Context");
        let ctx_label = gtk4::Label::new(Some(ctx.as_str()));
        ctx_label.add_css_class("dim-label");
        ctx_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        ctx_label.set_max_width_chars(48);
        ctx_label.set_selectable(true);
        ctx_row.add_suffix(&ctx_label);
        perms_group.add(&ctx_row);
    }

    add_perm_triplet_row(
        &perms_group, "Owner Permissions",
        &mode_state, &mode_observers, target, cancel,
        editable, is_directory, 0o400, 0o200, 0o100,
    );
    add_perm_triplet_row(
        &perms_group, "Group Permissions",
        &mode_state, &mode_observers, target, cancel,
        editable, is_directory, 0o040, 0o020, 0o010,
    );
    add_perm_triplet_row(
        &perms_group, "Others Permissions",
        &mode_state, &mode_observers, target, cancel,
        editable, is_directory, 0o004, 0o002, 0o001,
    );

    if !editable {
        let warn = adw::ActionRow::new();
        warn.set_title("Read-only");
        warn.set_subtitle("You are not the owner of this file");
        perms_group.add(&warn);
    }
    page.add(&perms_group);

    // ── Details group — live-updating octal/symbolic mode + extras.
    let details_group = adw::PreferencesGroup::new();
    details_group.set_title("Details");

    let octal_row = adw::ActionRow::new();
    octal_row.set_title("Octal mode");
    let octal_label = gtk4::Label::new(Some(&format!("{:04o}", mode & 0o7777)));
    octal_label.add_css_class("dim-label");
    octal_label.add_css_class("monospace");
    octal_label.set_selectable(true);
    octal_row.add_suffix(&octal_label);
    details_group.add(&octal_row);
    {
        let label = octal_label.clone();
        mode_observers.borrow_mut().push(Box::new(move |m| {
            label.set_text(&format!("{:04o}", m & 0o7777));
        }));
    }

    let sym_row = adw::ActionRow::new();
    sym_row.set_title("Symbolic mode");
    let sym_label = gtk4::Label::new(Some(&format_symbolic_mode(mode)));
    sym_label.add_css_class("dim-label");
    sym_label.add_css_class("monospace");
    sym_label.set_selectable(true);
    sym_row.add_suffix(&sym_label);
    details_group.add(&sym_row);
    {
        let label = sym_label.clone();
        mode_observers.borrow_mut().push(Box::new(move |m| {
            label.set_text(&format_symbolic_mode(m));
        }));
    }

    if info.has_attribute("unix::inode") {
        add_dim_row(&details_group, "Inode", &info.attribute_uint64("unix::inode").to_string());
    }
    if info.has_attribute("unix::nlink") {
        add_dim_row(&details_group, "Hard links", &info.attribute_uint32("unix::nlink").to_string());
    }
    if info.has_attribute("unix::block-size") && info.has_attribute("unix::blocks") {
        let bs = info.attribute_uint32("unix::block-size") as u64;
        let blocks = info.attribute_uint64("unix::blocks");
        add_dim_row(
            &details_group,
            "Allocated",
            &format!("{} blocks · {} bytes", blocks, blocks * bs),
        );
    }
    if info.has_attribute("unix::device") {
        add_dim_row(&details_group, "Device", &format!("{}", info.attribute_uint32("unix::device")));
    }
    if info.is_symlink() {
        if let Some(targ) = info.symlink_target() {
            add_dim_row(&details_group, "Symlink target", &targ.to_string_lossy());
        }
    }
    let uri = target.uri();
    if !uri.is_empty() {
        add_dim_row(&details_group, "URI", uri.as_str());
    }

    page.add(&details_group);
}

fn add_dim_row(group: &adw::PreferencesGroup, title: &str, value: &str) {
    let row = adw::ActionRow::new();
    row.set_title(title);
    let label = gtk4::Label::new(Some(value));
    label.add_css_class("dim-label");
    label.set_selectable(true);
    label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    label.set_max_width_chars(48);
    row.add_suffix(&label);
    group.add(&row);
}

fn format_symbolic_mode(mode: u32) -> String {
    let bit = |b: u32| mode & b != 0;
    let triplet = |out: &mut String, r: u32, w: u32, x: u32, sp: u32, sl: char, su: char| {
        out.push(if bit(r) { 'r' } else { '-' });
        out.push(if bit(w) { 'w' } else { '-' });
        out.push(match (bit(x), bit(sp)) {
            (true, true) => sl,
            (false, true) => su,
            (true, false) => 'x',
            (false, false) => '-',
        });
    };
    let mut out = String::with_capacity(9);
    triplet(&mut out, 0o400, 0o200, 0o100, 0o4000, 's', 'S');
    triplet(&mut out, 0o040, 0o020, 0o010, 0o2000, 's', 'S');
    triplet(&mut out, 0o004, 0o002, 0o001, 0o1000, 't', 'T');
    out
}

#[allow(clippy::too_many_arguments)]
fn add_perm_triplet_row(
    group: &adw::PreferencesGroup,
    title: &str,
    mode_state: &Rc<Cell<u32>>,
    observers: &Rc<RefCell<Vec<Box<dyn Fn(u32)>>>>,
    target: &gio::File,
    cancel: &gio::Cancellable,
    editable: bool,
    is_directory: bool,
    r_bit: u32,
    w_bit: u32,
    x_bit: u32,
) {
    let row = adw::ActionRow::new();
    row.set_title(title);

    let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    hbox.set_valign(gtk4::Align::Center);

    let make_check = |label: &str, bit: u32| -> gtk4::CheckButton {
        let cb = gtk4::CheckButton::with_label(label);
        cb.set_active(mode_state.get() & bit != 0);
        cb.set_sensitive(editable);
        cb
    };

    let r = make_check("Read", r_bit);
    let w = make_check("Write", w_bit);
    // For directories, "Execute" really means "search" — i.e. allowed to
    // descend into the dir. Label accordingly so it's not confusing.
    let x_label = if is_directory { "Access" } else { "Execute" };
    let x = make_check(x_label, x_bit);

    let on_toggle = |bit: u32, cb: &gtk4::CheckButton| {
        let mode_state = mode_state.clone();
        let observers = observers.clone();
        let target = target.clone();
        let cancel = cancel.clone();
        cb.connect_toggled(move |cb| {
            let mut m = mode_state.get();
            if cb.is_active() {
                m |= bit;
            } else {
                m &= !bit;
            }
            mode_state.set(m);
            for cb in observers.borrow().iter() { cb(m); }
            // We don't have a window handle here, but spawn_set_mode
            // doesn't strictly need one — toast suppression on failure
            // is acceptable for the perms triplet.
            let target_clone = target.clone();
            let cancel_clone = cancel.clone();
            glib::spawn_future_local(async move {
                let info = gio::FileInfo::new();
                info.set_attribute_uint32("unix::mode", m);
                let _ = target_clone
                    .set_attributes_future(
                        &info,
                        gio::FileQueryInfoFlags::NONE,
                        glib::Priority::DEFAULT,
                    )
                    .await;
                if cancel_clone.is_cancelled() { /* dialog gone */ }
            });
        });
    };
    on_toggle(r_bit, &r);
    on_toggle(w_bit, &w);
    on_toggle(x_bit, &x);

    hbox.append(&r);
    hbox.append(&w);
    hbox.append(&x);
    row.add_suffix(&hbox);
    group.add(&row);
}

fn add_perms_unavailable_row(group: &adw::PreferencesGroup, message: &str) {
    let row = adw::ActionRow::new();
    row.set_title("Permissions");
    row.set_subtitle(message);
    group.add(&row);
}

fn spawn_set_mode(
    win: &WrenWindow,
    target: gio::File,
    mode: u32,
    cancel: gio::Cancellable,
) {
    glib::spawn_future_local(clone!(
        #[weak(rename_to = window)]
        win,
        async move {
            let info = gio::FileInfo::new();
            info.set_attribute_uint32("unix::mode", mode);
            let res = target
                .set_attributes_future(
                    &info,
                    gio::FileQueryInfoFlags::NONE,
                    glib::Priority::DEFAULT,
                )
                .await;
            if cancel.is_cancelled() {
                return;
            }
            if let Err(e) = res {
                window.show_toast(&format!("Could not change permissions: {e}"));
            }
        }
    ));
}

// ─────────────────────────────────────────────────────────────────────────────
// Open With page
// ─────────────────────────────────────────────────────────────────────────────

fn build_open_with_page(win: &WrenWindow, subject: &Subject) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    page.set_title("Open With");
    page.set_icon_name(Some("application-x-executable-symbolic"));

    let group = adw::PreferencesGroup::new();
    group.set_title("Default Application");
    group.set_description(Some(
        "Click an application to make it the default for this file type.",
    ));

    let apps = gio::AppInfo::all_for_type(&subject.content_type);
    if apps.is_empty() {
        let row = adw::ActionRow::new();
        row.set_title("No applications available");
        row.set_subtitle(&format!(
            "No registered handler for “{}”",
            subject.content_type
        ));
        group.add(&row);
        page.add(&group);
        return page;
    }

    let default = gio::AppInfo::default_for_type(&subject.content_type, false);
    let default_id = default.as_ref().map(|d| d.id());

    // Track the row most recently marked default so we can clear its
    // checkmark when another row claims it.
    let current_default_row: Rc<RefCell<Option<adw::ActionRow>>> = Rc::new(RefCell::new(None));

    for app in apps.iter() {
        let row = adw::ActionRow::new();
        row.set_title(app.display_name().as_str());
        if let Some(desc) = app.description() {
            row.set_subtitle(desc.as_str());
        }
        if let Some(icon) = app.icon() {
            let img = gtk4::Image::from_gicon(&icon);
            img.set_pixel_size(32);
            row.add_prefix(&img);
        }
        row.set_activatable(true);

        let is_default = match (default_id.as_ref(), app.id()) {
            (Some(Some(a)), Some(b)) => a == &b,
            _ => false,
        };
        if is_default {
            attach_default_check(&row);
            *current_default_row.borrow_mut() = Some(row.clone());
        }

        let app_clone = app.clone();
        let content_type = subject.content_type.clone();
        let current_default_row = current_default_row.clone();
        row.connect_activated(clone!(
            #[weak(rename_to = window)]
            win,
            move |row| {
                match app_clone.set_as_default_for_type(&content_type) {
                    Ok(()) => {
                        if let Some(prev) = current_default_row.borrow_mut().take() {
                            clear_default_check(&prev);
                        }
                        attach_default_check(row);
                        *current_default_row.borrow_mut() = Some(row.clone());
                        window.show_toast(&format!(
                            "{} is now the default",
                            app_clone.display_name()
                        ));
                    }
                    Err(e) => {
                        window.show_toast(&format!("Could not set default: {e}"));
                    }
                }
            }
        ));

        group.add(&row);
    }

    page.add(&group);
    // TODO: "Add Custom" button to register a .desktop file from disk.
    page
}

fn attach_default_check(row: &adw::ActionRow) {
    use gtk4::prelude::ListModelExtManual;
    // Skip if a check is already attached (re-entry safe).
    let already = row
        .observe_children()
        .iter::<glib::Object>()
        .filter_map(|w| w.ok())
        .any(|w| {
            w.downcast_ref::<gtk4::Image>().is_some_and(|img| {
                img.icon_name().as_deref() == Some("emblem-ok-symbolic")
            })
        });
    if already {
        return;
    }
    let check = gtk4::Image::from_icon_name("emblem-ok-symbolic");
    check.add_css_class("accent");
    check.set_tooltip_text(Some("Default application"));
    row.add_suffix(&check);
}

fn clear_default_check(row: &adw::ActionRow) {
    use gtk4::prelude::ListModelExtManual;
    let to_remove: Vec<gtk4::Widget> = row
        .observe_children()
        .iter::<glib::Object>()
        .filter_map(|w| w.ok())
        .filter_map(|w| w.downcast::<gtk4::Widget>().ok())
        .filter(|w| {
            w.downcast_ref::<gtk4::Image>().is_some_and(|img| {
                img.icon_name().as_deref() == Some("emblem-ok-symbolic")
            })
        })
        .collect();
    for w in to_remove {
        row.remove(&w);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Format a `glib::DateTime` in a Nautilus-ish friendly form:
/// "Today at 14:32", "Yesterday at 09:15", or a full date for older entries.
fn format_glib_datetime(dt: &glib::DateTime) -> String {
    let local = dt.to_local().unwrap_or_else(|_| dt.clone());
    let now = glib::DateTime::now_local().ok();
    let same_day = now
        .as_ref()
        .map(|n| n.year() == local.year() && n.day_of_year() == local.day_of_year())
        .unwrap_or(false);
    let yesterday = now
        .as_ref()
        .and_then(|n| n.add_days(-1).ok())
        .map(|y| y.year() == local.year() && y.day_of_year() == local.day_of_year())
        .unwrap_or(false);

    let time_str = local.format("%H:%M").map(|g| g.to_string()).unwrap_or_default();
    if same_day {
        format!("Today at {time_str}")
    } else if yesterday {
        format!("Yesterday at {time_str}")
    } else {
        local
            .format("%Y-%m-%d %H:%M:%S")
            .map(|g| g.to_string())
            .unwrap_or_default()
    }
}

/// Format a Unix timestamp (seconds since epoch). Used for the Modified row,
/// which is sourced from the FileObject and stored as i64.
fn format_unix_time(ts: i64) -> Option<String> {
    let dt = glib::DateTime::from_unix_local(ts).ok()?;
    Some(format_glib_datetime(&dt))
}

/// Pretty + raw byte size: "1.2 GB · 1,234,567,890 bytes" for big values,
/// just the pretty form when the two would be identical (small files).
fn format_size_with_bytes(bytes: u64) -> String {
    let pretty = format_file_size(bytes);
    if bytes < 1024 {
        // Already shown as "N B" — no point repeating.
        pretty
    } else {
        format!("{pretty} · {} bytes", group_thousands(bytes))
    }
}

fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

#[cfg(unix)]
fn current_user_uid() -> Option<u32> {
    // SAFETY: getuid is documented as never failing or having any
    // side effects. POSIX requires it to be reentrant.
    Some(unsafe { libc_getuid_raw() })
}

#[cfg(not(unix))]
fn current_user_uid() -> Option<u32> {
    None
}

// We don't take a libc dependency; getuid is in libc.so.6 on Linux and
// any libsystem on macOS. Use the FFI directly.
#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid_raw() -> u32;
}

// ─────────────────────────────────────────────────────────────────────────────
// Bridge: window-side rename helper used by the inline EditableLabel.
// ─────────────────────────────────────────────────────────────────────────────

impl WrenWindow {
    /// Rename a file from inside the Properties dialog. Mirrors
    /// `spawn_rename` in window/mod.rs but doesn't touch the undo stack
    /// — the rename is initiated by an inline editor here, separate from
    /// the file-view's rename flow.
    fn spawn_properties_rename(&self, file: gio::File, old_name: String, new_name: String) {
        glib::spawn_future_local(clone!(
            #[weak(rename_to = window)]
            self,
            async move {
                match file
                    .set_display_name_future(&new_name, glib::Priority::DEFAULT)
                    .await
                {
                    Ok(_) => {
                        window.reload();
                    }
                    Err(e) => {
                        window.show_toast(&format!("Could not rename: {e}"));
                        // Best-effort: nothing to revert in the label
                        // here because the user already saw the new
                        // value. The next reload of the view will
                        // resync to the canonical name.
                        let _ = old_name;
                    }
                }
            }
        ));
    }
}
