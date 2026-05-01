//! File-operation helpers and the `OpHandle` infrastructure that drives the
//! progress popover. Pure module — no `WrenWindow` references — so the
//! parent module can pull this in without circular dependencies. `op_start`
//! and `op_finish` (which need `self.imp()`) stay on `WrenWindow` itself.

use gtk4::prelude::*;

// ── Operation logging ─────────────────────────────────────────────────────
// Stderr output for every destructive file action so users running from a
// terminal can see exactly what is being done.

pub fn fmt_path(f: &gio::File) -> String {
    f.path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| f.uri().to_string())
}

pub fn log_op(action: &str, src: &gio::File, dest: Option<&gio::File>) {
    match dest {
        Some(d) => eprintln!("[wren] {action}: {} -> {}", fmt_path(src), fmt_path(d)),
        None => eprintln!("[wren] {action}: {}", fmt_path(src)),
    }
}

pub fn log_err(
    action: &str,
    src: &gio::File,
    dest: Option<&gio::File>,
    err: &impl std::fmt::Display,
) {
    match dest {
        Some(d) => eprintln!(
            "[wren] {action} failed: {} -> {}: {err}",
            fmt_path(src),
            fmt_path(d)
        ),
        None => eprintln!("[wren] {action} failed: {}: {err}", fmt_path(src)),
    }
}

// Returns a non-colliding child path under dest_dir. If `dest_dir/name` is
// free, returns that. Otherwise appends " (Copy)" / " (Copy 2)" / ... to the
// stem until an unused name is found.
pub fn unique_dest(dest_dir: &gio::File, name: &std::path::Path) -> gio::File {
    let dest = dest_dir.child(name);
    if !dest.query_exists(gio::Cancellable::NONE) {
        return dest;
    }
    let stem = name
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    let ext = name
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| format!(".{}", e))
        .unwrap_or_default();
    let candidate = dest_dir.child(&format!("{} (Copy){}", stem, ext));
    if !candidate.query_exists(gio::Cancellable::NONE) {
        return candidate;
    }
    let mut i = 2u32;
    loop {
        let c = dest_dir.child(&format!("{} (Copy {}){}", stem, i, ext));
        if !c.query_exists(gio::Cancellable::NONE) {
            return c;
        }
        i += 1;
    }
}

pub fn cancelled_err() -> glib::Error {
    glib::Error::new(gio::IOErrorEnum::Cancelled, "Operation cancelled")
}

/// Build a "From  /path/to/foo" row used inside an OpHandle. Returns the
/// outer Box and the value Label so the OpHandle can update / hide them.
fn make_path_row(prefix: &str) -> (gtk4::Box, gtk4::Label) {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let prefix_label = gtk4::Label::new(Some(prefix));
    prefix_label.set_xalign(0.0);
    prefix_label.set_width_chars(4);
    prefix_label.set_max_width_chars(4);
    prefix_label.add_css_class("caption");
    prefix_label.add_css_class("dim-label");
    let value = gtk4::Label::new(None);
    value.set_xalign(0.0);
    value.set_hexpand(true);
    value.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    // Both width-chars and max-width-chars set to lock the natural width.
    // Without this, GtkLabel's natural size grows with its content and the
    // popover keeps resizing every time the path updates.
    value.set_width_chars(40);
    value.set_max_width_chars(40);
    value.add_css_class("caption");
    value.add_css_class("monospace");
    value.set_selectable(true);
    row.append(&prefix_label);
    row.append(&value);
    (row, value)
}

#[derive(Clone, Copy, Debug)]
pub enum ConflictResolution {
    Skip,
    Replace,
    Rename,
    Cancel,
}

#[derive(Clone, Copy, Debug)]
pub enum OpKind {
    Copy,
    Move,
    Delete,
    Duplicate,
    Trash,
    Restore,
}

impl OpKind {
    fn icon_name(self) -> &'static str {
        match self {
            Self::Copy | Self::Duplicate => "edit-copy-symbolic",
            Self::Move => "edit-cut-symbolic",
            Self::Delete | Self::Trash => "user-trash-symbolic",
            Self::Restore => "edit-undo-symbolic",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Copy => "Copying",
            Self::Move => "Moving",
            Self::Delete => "Deleting",
            Self::Duplicate => "Duplicating",
            Self::Trash => "Moving to Trash",
            Self::Restore => "Restoring",
        }
    }
    pub fn done_title(self) -> &'static str {
        match self {
            Self::Copy => "Copy complete",
            Self::Move => "Move complete",
            Self::Delete => "Delete complete",
            Self::Duplicate => "Duplicate complete",
            Self::Trash => "Moved to Trash",
            Self::Restore => "Restored from Trash",
        }
    }
}

/// Mutable progress accounting for one in-flight op. Lives behind an
/// `Rc<RefCell<…>>` inside an OpHandle so multiple callbacks can update it.
#[derive(Debug)]
pub(super) struct OpState {
    pub items_done: u64,
    pub bytes_done: u64,
    pub total_items: u64,
    pub total_bytes: u64,
    pub start: std::time::Instant,
    /// Last time the stats line (speed / ETA / bytes) was redrawn. Updated
    /// at most once per `STATS_REFRESH` so the values don't jitter.
    pub last_stats_emit: std::time::Instant,
    /// Last byte rate that was actually written to the label, for jitter
    /// smoothing on the next emit.
    pub last_byte_rate: f64,
    /// Last ETA written to the label, in seconds, for jitter smoothing.
    pub last_eta_secs: u64,
    /// True when every item processed without error. op_finish reads this
    /// to decide whether to fire the "X complete" desktop notification.
    pub succeeded: bool,
}

const STATS_REFRESH: std::time::Duration = std::time::Duration::from_millis(1000);
/// Wait for at least this much activity before computing speed / ETA — earlier
/// numbers are dominated by ramp-up jitter and the first-file outlier.
const STATS_WARMUP: std::time::Duration = std::time::Duration::from_millis(1500);

/// A handle to one in-flight file operation. Holds its Cancellable and the
/// widgets that display its progress in the header-bar popover. Cloning is
/// cheap — every field is a reference-counted GObject (or a Cancellable, which
/// is also a GObject).
#[derive(Clone, Debug)]
pub struct OpHandle {
    pub cancellable: gio::Cancellable,
    pub kind: OpKind,
    pub row: gtk4::Box,
    item_label: gtk4::Label,
    elapsed_label: gtk4::Label,
    progress: gtk4::ProgressBar,
    stats_label: gtk4::Label,
    src_row: gtk4::Box,
    src_label: gtk4::Label,
    dest_row: gtk4::Box,
    dest_label: gtk4::Label,
    pub(super) state: std::rc::Rc<std::cell::RefCell<OpState>>,
}

impl OpHandle {
    pub fn build(kind: OpKind) -> Self {
        let cancellable = gio::Cancellable::new();
        let row = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        row.set_width_request(380);
        // libadwaita "card" gives a subtle rounded background + border so
        // each op row reads as its own visual unit when several stack up.
        row.add_css_class("card");
        row.add_css_class("wren-op-row");

        // ── Header: [icon] [Title] [0:23 elapsed] ⟶ [X cancel] ──────────────
        let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        let title_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        title_box.set_hexpand(true);
        let icon = gtk4::Image::from_icon_name(kind.icon_name());
        icon.set_pixel_size(16);
        let title_label = gtk4::Label::new(Some(kind.title()));
        title_label.set_xalign(0.0);
        title_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        title_label.add_css_class("heading");
        let elapsed_label = gtk4::Label::new(None);
        elapsed_label.set_xalign(0.0);
        elapsed_label.add_css_class("caption");
        elapsed_label.add_css_class("dim-label");
        title_box.append(&icon);
        title_box.append(&title_label);
        title_box.append(&elapsed_label);
        let cancel_btn = gtk4::Button::from_icon_name("window-close-symbolic");
        cancel_btn.set_tooltip_text(Some("Cancel"));
        cancel_btn.add_css_class("flat");
        cancel_btn.add_css_class("circular");
        let c_clone = cancellable.clone();
        cancel_btn.connect_clicked(move |_| c_clone.cancel());
        header.append(&title_box);
        header.append(&cancel_btn);

        // ── Current item: filename + counter (single emphasised line) ───────
        let item_label = gtk4::Label::new(Some("Preparing…"));
        item_label.set_xalign(0.0);
        item_label.set_hexpand(true);
        item_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        item_label.set_width_chars(40);
        item_label.set_max_width_chars(40);

        // ── Progress bar with overlay percent ───────────────────────────────
        let progress = gtk4::ProgressBar::new();
        progress.set_show_text(true);
        progress.set_text(Some("0%"));

        // ── Stats line: "1.2 GB / 5.3 GB · 23 MB/s · 12 s left" ─────────────
        let stats_label = gtk4::Label::new(None);
        stats_label.set_xalign(0.0);
        stats_label.set_hexpand(true);
        stats_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        stats_label.set_width_chars(40);
        stats_label.set_max_width_chars(40);
        stats_label.add_css_class("caption");
        stats_label.add_css_class("dim-label");

        // ── Path rows: From / To, always visible (no expander) ──────────────
        let (src_row, src_label) = make_path_row("From");
        let (dest_row, dest_label) = make_path_row("To");
        // Hidden until set_paths is first called so blank rows don't take up
        // space while the op is in the pre-walk phase.
        src_row.set_visible(false);
        dest_row.set_visible(false);

        row.append(&header);
        row.append(&item_label);
        row.append(&progress);
        row.append(&stats_label);
        row.append(&src_row);
        row.append(&dest_row);

        let now = std::time::Instant::now();
        let state = std::rc::Rc::new(std::cell::RefCell::new(OpState {
            items_done: 0,
            bytes_done: 0,
            total_items: 0,
            total_bytes: 0,
            start: now,
            last_stats_emit: now,
            last_byte_rate: 0.0,
            last_eta_secs: 0,
            succeeded: false,
        }));

        Self {
            cancellable,
            kind,
            row,
            item_label,
            elapsed_label,
            progress,
            stats_label,
            src_row,
            src_label,
            dest_row,
            dest_label,
            state,
        }
    }

    /// Set the totals (called by the caller once pre-walk is done).
    pub fn set_total(&self, items: u64, bytes: u64) {
        let mut s = self.state.borrow_mut();
        s.total_items = items;
        s.total_bytes = bytes;
    }

    /// Mark the op as having completed every item without error. Called by
    /// the caller after the loop body finishes naturally, NOT from error /
    /// cancel break paths. Read by op_finish to gate the success notification.
    pub fn mark_succeeded(&self) {
        self.state.borrow_mut().succeeded = true;
    }

    /// Set the current item line (filename + counter, e.g. "foo.txt (3 of 17)").
    pub fn set_item(&self, msg: &str) {
        self.item_label.set_text(msg);
    }

    /// Set the source / destination path rows. Pass `None` for `dest` on
    /// operations that have no destination (e.g. delete).
    pub fn set_paths(&self, src: &gio::File, dest: Option<&gio::File>) {
        self.src_label.set_text(&fmt_path(src));
        self.src_row.set_visible(true);
        match dest {
            Some(d) => {
                self.dest_label.set_text(&fmt_path(d));
                self.dest_row.set_visible(true);
            }
            None => {
                self.dest_label.set_text("");
                self.dest_row.set_visible(false);
            }
        }
    }

    /// Set the progress bar fraction (0.0 — 1.0) and its overlay text to a %.
    pub fn set_fraction(&self, fraction: f64) {
        let f = fraction.clamp(0.0, 1.0);
        self.progress.set_fraction(f);
        self.progress.set_text(Some(&format!("{}%", (f * 100.0).round() as u32)));
    }

    /// Build a per-sub-item callback for `copy_recursive`. The callback
    /// always increments items_done and bytes_done in the shared OpState,
    /// then at most ~25 times/sec recomputes rate / ETA and updates labels.
    pub fn copy_callback(&self) -> impl Fn(&gio::File, &gio::File, u64) + 'static {
        let h = self.clone();
        let last = std::rc::Rc::new(std::cell::Cell::new(std::time::Instant::now()));
        move |s, d, size| {
            let snapshot = {
                let mut state = h.state.borrow_mut();
                state.items_done += 1;
                state.bytes_done += size;
                (
                    state.items_done,
                    state.bytes_done,
                    state.total_items,
                    state.total_bytes,
                    state.start,
                )
            };
            let now = std::time::Instant::now();
            if now.duration_since(last.get()) < std::time::Duration::from_millis(40) {
                return;
            }
            last.set(now);
            h.set_paths(s, Some(d));
            h.update_progress_display(snapshot);
        }
    }

    /// Like `delete_callback` but doesn't tick the cumulative counter — only
    /// updates the path labels. Used by Replace's pre-delete in paste / drop,
    /// where the items being deleted aren't in the pre-walked total.
    pub fn paths_only_callback(&self) -> impl Fn(&gio::File, u64) + 'static {
        let h = self.clone();
        let last = std::rc::Rc::new(std::cell::Cell::new(std::time::Instant::now()));
        move |s, _size| {
            let now = std::time::Instant::now();
            if now.duration_since(last.get()) < std::time::Duration::from_millis(40) {
                return;
            }
            last.set(now);
            h.set_paths(s, None);
        }
    }

    /// Build a per-sub-item callback for `delete_recursive`.
    pub fn delete_callback(&self) -> impl Fn(&gio::File, u64) + 'static {
        let h = self.clone();
        let last = std::rc::Rc::new(std::cell::Cell::new(std::time::Instant::now()));
        move |s, size| {
            let snapshot = {
                let mut state = h.state.borrow_mut();
                state.items_done += 1;
                state.bytes_done += size;
                (
                    state.items_done,
                    state.bytes_done,
                    state.total_items,
                    state.total_bytes,
                    state.start,
                )
            };
            let now = std::time::Instant::now();
            if now.duration_since(last.get()) < std::time::Duration::from_millis(40) {
                return;
            }
            last.set(now);
            h.set_paths(s, None);
            h.update_progress_display(snapshot);
        }
    }

    fn update_progress_display(
        &self,
        (items, bytes, total_items, total_bytes, start): (
            u64,
            u64,
            u64,
            u64,
            std::time::Instant,
        ),
    ) {
        // Fraction prefers bytes when known; falls back to items. This is
        // cheap and runs at the callback's 40 ms tick rate.
        if total_bytes > 0 {
            self.set_fraction(bytes as f64 / total_bytes as f64);
        } else if total_items > 0 {
            self.set_fraction(items as f64 / total_items as f64);
        }

        // Stats text (speed, ETA, bytes) is much more visually disruptive
        // when it bounces — gate it behind a 1 s refresh window plus a warmup
        // before any rate is computed at all.
        let elapsed = start.elapsed();
        if elapsed < STATS_WARMUP {
            return;
        }
        let now = std::time::Instant::now();
        let (smoothed_byte_rate, smoothed_eta) = {
            let mut state = self.state.borrow_mut();
            if now.duration_since(state.last_stats_emit) < STATS_REFRESH {
                return;
            }
            state.last_stats_emit = now;

            let elapsed_s = elapsed.as_secs_f64();
            let raw_byte_rate = bytes as f64 / elapsed_s;
            // EMA: 70% of the previous emit, 30% of the new sample. Stops
            // the rate from snapping to zero on a stretch of small files
            // and back to peak on a big one.
            let smoothed_rate = if state.last_byte_rate == 0.0 {
                raw_byte_rate
            } else {
                0.7 * state.last_byte_rate + 0.3 * raw_byte_rate
            };
            state.last_byte_rate = smoothed_rate;

            let raw_eta = if total_bytes > 0 && smoothed_rate > 0.0 {
                let remaining = total_bytes.saturating_sub(bytes);
                (remaining as f64 / smoothed_rate) as u64
            } else if total_items > 0 && items > 0 {
                let remaining = total_items.saturating_sub(items);
                let item_rate = items as f64 / elapsed_s;
                if item_rate > 0.0 {
                    (remaining as f64 / item_rate) as u64
                } else {
                    0
                }
            } else {
                0
            };
            // Round ETA to a sensible granularity so it doesn't tick every
            // second visibly; keeps "12 s left" stable for the second it's
            // displayed instead of jumping 12 → 11 → 9 → 11 every refresh.
            let smoothed_eta = round_eta(raw_eta);
            state.last_eta_secs = smoothed_eta;
            (smoothed_rate, smoothed_eta)
        };

        let item_rate = items as f64 / elapsed.as_secs_f64();
        let mut parts: Vec<String> = Vec::new();
        if total_bytes > 0 {
            parts.push(format!(
                "{} of {}",
                format_file_size(bytes),
                format_file_size(total_bytes)
            ));
        }
        if smoothed_byte_rate >= 1024.0 {
            parts.push(format!("{}/s", format_file_size(smoothed_byte_rate as u64)));
        } else if item_rate > 0.0 {
            parts.push(format!("{:.0} items/s", item_rate));
        }
        if smoothed_eta > 0 {
            parts.push(format!("{} left", format_duration(smoothed_eta)));
        }
        self.stats_label.set_text(&parts.join(" · "));
        // Elapsed counter next to the title: "· 1m 23s elapsed" once warmup
        // hits, throttled to the same 1 s cadence as the stats line above.
        self.elapsed_label
            .set_text(&format!("· {} elapsed", format_duration(elapsed.as_secs())));
    }
}

/// Round an ETA in seconds to a granularity that doesn't visibly shift on
/// every refresh:
///   < 10 s  → 1 s steps
///   < 1 min → 5 s steps
///   < 10 min → 30 s steps
///   < 1 h   → 1 min steps
///   ≥ 1 h   → 5 min steps
fn round_eta(secs: u64) -> u64 {
    if secs < 10 { secs }
    else if secs < 60 { (secs + 2) / 5 * 5 }
    else if secs < 600 { (secs + 15) / 30 * 30 }
    else if secs < 3600 { (secs + 30) / 60 * 60 }
    else { (secs + 150) / 300 * 300 }
}

// ── Iterative file operation helpers ────────────────────────────────────────
//
// Iterative (non-recursive) implementations avoid the Box::pin overhead and
// potential stack issues with deeply-nested directory trees.

pub async fn copy_recursive(
    src: gio::File,
    dest: gio::File,
    cancellable: &gio::Cancellable,
    on_item: impl Fn(&gio::File, &gio::File, u64) + 'static,
) -> Result<(), glib::Error> {
    if cancellable.is_cancelled() {
        return Err(cancelled_err());
    }

    // Check whether the top-level source is a directory.
    let src_info = src
        .query_info_future(
            "standard::type,standard::size",
            gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
            glib::Priority::DEFAULT,
        )
        .await?;

    let src_size = src_info.size().max(0) as u64;
    if src_info.file_type() != gio::FileType::Directory {
        on_item(&src, &dest, src_size);
        let (fut, _) = src.copy_future(
            &dest,
            gio::FileCopyFlags::NOFOLLOW_SYMLINKS,
            glib::Priority::DEFAULT,
        );
        return fut.await;
    }

    // Top-level directory: tick once with size 0 (dirs don't contribute bytes).
    on_item(&src, &dest, 0);
    // BFS queue of (src_dir, dest_dir) pairs.
    dest.make_directory_future(glib::Priority::DEFAULT).await?;
    let mut queue = std::collections::VecDeque::new();
    queue.push_back((src, dest));

    while let Some((src_dir, dest_dir)) = queue.pop_front() {
        if cancellable.is_cancelled() {
            return Err(cancelled_err());
        }
        let enumerator = src_dir
            .enumerate_children_future(
                "standard::name,standard::type,standard::size",
                gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                glib::Priority::DEFAULT,
            )
            .await?;

        loop {
            if cancellable.is_cancelled() {
                return Err(cancelled_err());
            }
            let batch = enumerator
                .next_files_future(30, glib::Priority::DEFAULT)
                .await?;
            if batch.is_empty() {
                break;
            }
            for child_info in batch {
                if cancellable.is_cancelled() {
                    return Err(cancelled_err());
                }
                let name = child_info.name();
                let child_src = src_dir.child(&name);
                let child_dest = dest_dir.child(&name);
                let is_dir = child_info.file_type() == gio::FileType::Directory;
                let size = if is_dir { 0 } else { child_info.size().max(0) as u64 };
                on_item(&child_src, &child_dest, size);
                if is_dir {
                    child_dest.make_directory_future(glib::Priority::DEFAULT).await?;
                    queue.push_back((child_src, child_dest));
                } else {
                    let (fut, _) = child_src.copy_future(
                        &child_dest,
                        gio::FileCopyFlags::NOFOLLOW_SYMLINKS,
                        glib::Priority::DEFAULT,
                    );
                    fut.await?;
                }
            }
        }
    }
    Ok(())
}

pub async fn delete_recursive(
    file: gio::File,
    cancellable: &gio::Cancellable,
    on_item: impl Fn(&gio::File, u64) + 'static,
) -> Result<(), glib::Error> {
    if cancellable.is_cancelled() {
        return Err(cancelled_err());
    }
    // The gvfs trash backend rejects any write inside a trashed item with
    // "Items in the trash may not be modified" — so we can't enumerate +
    // delete child-by-child. delete_future on a trash:/// top-level entry
    // IS supported and recursively removes the trashed item atomically.
    if file.has_uri_scheme("trash") {
        let result = file.delete_future(glib::Priority::DEFAULT).await;
        if result.is_ok() {
            on_item(&file, 0);
        }
        return result;
    }
    let info = file
        .query_info_future(
            "standard::type,standard::size",
            gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
            glib::Priority::DEFAULT,
        )
        .await?;

    if info.file_type() != gio::FileType::Directory {
        on_item(&file, info.size().max(0) as u64);
        return file.delete_future(glib::Priority::DEFAULT).await;
    }

    // DFS: collect directories in traversal order, delete all files immediately.
    // Directories are deleted in reverse traversal order (deepest first).
    let mut dirs: Vec<gio::File> = Vec::new();
    let mut stack = vec![file];

    while let Some(dir) = stack.pop() {
        if cancellable.is_cancelled() {
            return Err(cancelled_err());
        }
        let enumerator = dir
            .enumerate_children_future(
                "standard::name,standard::type,standard::size",
                gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                glib::Priority::DEFAULT,
            )
            .await?;

        dirs.push(dir.clone());

        loop {
            if cancellable.is_cancelled() {
                return Err(cancelled_err());
            }
            let batch = enumerator
                .next_files_future(30, glib::Priority::DEFAULT)
                .await?;
            if batch.is_empty() {
                break;
            }
            for child_info in batch {
                if cancellable.is_cancelled() {
                    return Err(cancelled_err());
                }
                let child = dir.child(child_info.name());
                let is_dir = child_info.file_type() == gio::FileType::Directory;
                let size = if is_dir { 0 } else { child_info.size().max(0) as u64 };
                on_item(&child, size);
                if is_dir {
                    stack.push(child);
                } else {
                    child.delete_future(glib::Priority::DEFAULT).await?;
                }
            }
        }
    }

    // Delete directories deepest-first.
    for dir in dirs.into_iter().rev() {
        if cancellable.is_cancelled() {
            return Err(cancelled_err());
        }
        on_item(&dir, 0);
        dir.delete_future(glib::Priority::DEFAULT).await?;
    }
    Ok(())
}

/// Sum `count_items_and_bytes` over a slice of top-level entries. `double`
/// is for ops that touch every item twice (move = copy + delete) so the bar
/// can advance across both phases. Returns the totals at whatever point a
/// cancellation was observed.
pub async fn pre_walk_total(
    files: &[gio::File],
    double: bool,
    cancellable: &gio::Cancellable,
) -> (u64, u64) {
    let mut total_items: u64 = 0;
    let mut total_bytes: u64 = 0;
    for f in files {
        if cancellable.is_cancelled() {
            break;
        }
        let (n, b) = count_items_and_bytes(f.clone(), cancellable).await;
        if double {
            total_items += n * 2;
            total_bytes += b * 2;
        } else {
            total_items += n;
            total_bytes += b;
        }
    }
    (total_items, total_bytes)
}

/// Pre-walk a file (or directory tree) and return `(item_count, total_bytes)`.
/// Used by callers to drive the progress bar fraction and the bytes display.
/// Honours the cancellable. Errors inside the walk (e.g. permission denied on
/// a subdir) are silently skipped, so totals may slightly under-count.
pub async fn count_items_and_bytes(
    file: gio::File,
    cancellable: &gio::Cancellable,
) -> (u64, u64) {
    let info = match file
        .query_info_future(
            "standard::type,standard::size",
            gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
            glib::Priority::DEFAULT,
        )
        .await
    {
        Ok(i) => i,
        Err(_) => return (0, 0),
    };
    if info.file_type() != gio::FileType::Directory {
        return (1, info.size().max(0) as u64);
    }
    // Count the directory itself, plus every descendant. Directories don't
    // contribute to total_bytes (their inode size isn't user-meaningful).
    let mut count: u64 = 1;
    let mut bytes: u64 = 0;
    let mut stack = vec![file];
    while let Some(dir) = stack.pop() {
        if cancellable.is_cancelled() {
            return (count, bytes);
        }
        let enumerator = match dir
            .enumerate_children_future(
                "standard::name,standard::type,standard::size",
                gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                glib::Priority::DEFAULT,
            )
            .await
        {
            Ok(e) => e,
            Err(_) => continue,
        };
        loop {
            if cancellable.is_cancelled() {
                return (count, bytes);
            }
            let batch = match enumerator
                .next_files_future(50, glib::Priority::DEFAULT)
                .await
            {
                Ok(b) => b,
                Err(_) => break,
            };
            if batch.is_empty() {
                break;
            }
            for info in batch {
                count += 1;
                if info.file_type() == gio::FileType::Directory {
                    stack.push(dir.child(info.name()));
                } else {
                    bytes += info.size().max(0) as u64;
                }
            }
        }
    }
    (count, bytes)
}

/// Format a duration in human-readable form: "12 s", "3 min 4 s", "1 h 23 min".
pub fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs} s")
    } else if secs < 3600 {
        let m = secs / 60;
        let s = secs % 60;
        if s == 0 { format!("{m} min") } else { format!("{m} min {s} s") }
    } else {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        if m == 0 { format!("{h} h") } else { format!("{h} h {m} min") }
    }
}

/// Recursively walk a directory, accumulating total bytes and item count.
/// Calls `on_update` periodically so a UI label can show live progress.
/// Honours the cancellable; returns the partial total if cancelled.
pub async fn compute_dir_size(
    root: gio::File,
    cancellable: &gio::Cancellable,
    on_update: impl Fn(u64, u64),
) -> (u64, u64) {
    let mut total: u64 = 0;
    let mut count: u64 = 0;
    let mut stack = vec![root];
    let mut last_emit = std::time::Instant::now();
    let emit_every = std::time::Duration::from_millis(120);

    while let Some(dir) = stack.pop() {
        if cancellable.is_cancelled() {
            return (total, count);
        }
        let enumerator = match dir
            .enumerate_children_future(
                "standard::name,standard::type,standard::size",
                gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                glib::Priority::DEFAULT,
            )
            .await
        {
            Ok(e) => e,
            Err(_) => continue, // permission denied etc — skip this subdir
        };
        loop {
            if cancellable.is_cancelled() {
                return (total, count);
            }
            let batch = match enumerator
                .next_files_future(50, glib::Priority::DEFAULT)
                .await
            {
                Ok(b) => b,
                Err(_) => break,
            };
            if batch.is_empty() {
                break;
            }
            for info in batch {
                count += 1;
                let size = info.size().max(0) as u64;
                total += size;
                if info.file_type() == gio::FileType::Directory {
                    stack.push(dir.child(info.name()));
                }
                if last_emit.elapsed() >= emit_every {
                    on_update(total, count);
                    last_emit = std::time::Instant::now();
                }
            }
        }
    }
    (total, count)
}

pub fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
