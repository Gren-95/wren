# Wren — CLAUDE.md

## Project Overview

GTK4/libadwaita file manager in Rust, modeled after GNOME Nautilus.
**Tech stack:** Rust, gtk4-rs 0.11, libadwaita 0.9, gio, glib  
**Architecture:** Single-process GTK4 app; all UI on the main thread; async I/O via `glib::spawn_future_local`  
**Primary language:** Rust

## Common Commands

```bash
cargo build          # compile
cargo run            # run
cargo check          # fast type-check without linking (use during editing)
```

System deps (Fedora/Nobara): `gtk4-devel libadwaita-devel`

## GObject Subclassing Pattern

Every widget follows the split `mod.rs` / `imp.rs` convention required by glib-rs:

- **`imp.rs`** — the inner struct (`pub struct Foo { ... }`), `ObjectSubclass` impl, `ObjectImpl`, `WidgetImpl`, template callbacks
- **`mod.rs`** — `glib::wrapper!` macro, `Default`, public API methods that delegate into `imp`
- Access imp from pub type: `imp::Foo::from_obj(self)` or `self.imp()` helper

`CompositeTemplate` widgets load their layout from a `.ui` file in `data/ui/` registered via GResource (`data/resources.gresource.xml`).

## Source Map

```
src/
├── main.rs                        Entry point: register GResource, create WrenApplication, run
├── application/
│   ├── mod.rs                     WrenApplication wrapper + terminal_cmd getter/setter
│   └── imp.rs                     startup (CSS, accels), activate; settings saved to
│                                  ~/.config/wren/settings.ini via glib::KeyFile
├── window/
│   ├── mod.rs                     ALL window logic: navigation, file ops, zoom, sort,
│   │                              undo/redo, properties, bookmarks, search, context menu
│   ├── imp.rs                     WrenWindow GObject fields, class_init (install_action calls),
│   │                              constructed (stateful actions, hamburger menu build)
│   ├── tab.rs                     TabState struct — one per tab, owns the widget tree,
│   │                              DirectoryModel, NavigationModel, sort state
│   └── undo.rs                    UndoOp enum (Rename, NewFolder)
├── breadcrumb/
│   ├── mod.rs                     set_location(file), enter_edit_mode(); builds crumb buttons
│   └── imp.rs                     CompositeTemplate, crumb_box / path_entry / edit_button
├── sidebar/
│   ├── mod.rs                     populate_places(), reload_bookmarks(), set_location(file)
│   │                              Reads ~/.config/gtk-3.0/bookmarks for user bookmarks section
│   └── imp.rs                     CompositeTemplate, place_uris Vec, n_static_rows counter
├── file_view/
│   ├── grid/mod.rs                WrenFileGrid: GridView in ScrolledWindow; make_cell_factory(icon_size)
│   │                              replaces factory to force rebind on zoom; Ctrl+scroll → zoom actions
│   ├── list/mod.rs                WrenFileList: ListView in ScrolledWindow; same structure as grid
│   ├── cell/
│   │   ├── mod.rs                 WrenFileCell bind(file_obj, icon_size) / unbind()
│   │   │                          Thread-local TEXTURE_CACHE (VecDeque, max 256) for thumbnail
│   │   │                          gdk::Texture reuse — avoids re-decode + VRAM re-upload on scroll
│   │   └── imp.rs                 CompositeTemplate: icon (GtkImage) + name (GtkLabel)
│   └── row/
│       ├── mod.rs                 WrenFileRow bind(file_obj) / unbind() for list view
│       └── imp.rs                 CompositeTemplate: icon, name, content_type, size, modified
├── model/
│   ├── file_object.rs             FileObject GObject wrapping gio::File + gio::FileInfo
│   │                              GObject properties: name, content-type, is-directory,
│   │                              file-size (u64), modified (i64)
│   │                              thumbnail_path() reads thumbnail::path attribute
│   │                              QUERY_ATTRS const lists all FileInfo attrs to fetch
│   └── directory_model.rs         DirectoryModel (plain Rust struct, not GObject):
│                                  store → FilterListModel → SortListModel → MultiSelection
│                                  set_filter(search, show_hidden), set_sort(key, reversed)
│                                  start_load() returns a Future for async enumerate
├── navigation/
│   └── navigation_model.rs        NavigationModel: back_stack / forward_stack / current
│                                  navigate_to() / navigate_back() / navigate_forward()
└── operations/                    Thin wrappers (mostly stubs); actual ops live in window/mod.rs
    copy.rs, delete.rs, move_.rs, new_folder.rs, rename.rs
```

## Data / UI Files

```
data/
├── resources.gresource.xml        GResource manifest — lists all .ui and .css files
├── style/app.css                  Application CSS (loaded in application/imp.rs startup)
└── ui/
    ├── window.ui                  AdwApplicationWindow template
    ├── sidebar.ui                 WrenSidebar template (GtkScrolledWindow > GtkListBox)
    ├── breadcrumb_bar.ui          WrenBreadcrumbBar template
    ├── file_cell.ui               WrenFileCell template (icon + label, vertical)
    └── file_row.ui                WrenFileRow template (icon + name + type + size + date)
```

## Key Architectural Decisions

### Model pipeline (per tab)
```
gio::ListStore
  → gtk4::FilterListModel   (set_filter → CustomFilter)
  → gtk4::SortListModel     (set_sort   → CustomSorter)
  → gtk4::MultiSelection    (wired to both GridView and ListView)
```
Both views share the same `MultiSelection`; switching views is just flipping the inner `gtk4::Stack`.

### Zoom
`WrenWindow.zoom_level: Cell<i32>` (1–5, default 3 = 64px).  
Zoom calls `tab.file_grid.set_icon_size(px)` which replaces the `SignalListItemFactory` entirely — the new factory captures the new `icon_size` in its `connect_bind` closure, forcing GTK to unbind/setup/bind every visible cell with the new size.  
Icon sizes: 1→32, 2→48, 3→64, 4→96, 5→128 px.

### Sort (per tab)
`TabState.sort_key: SortKey` + `sort_reversed: bool`.  
`DirectoryModel.set_sort()` updates a shared `Rc<RefCell<SortState>>` then calls `sorter.changed(SorterChange::Different)` to trigger a re-sort.  
`on_tab_switched()` syncs the `win.set-sort-key` and `win.toggle-sort-reversed` stateful action states so the hamburger menu checkmarks stay correct across tab switches.

### Sidebar location sync
After every navigation `load_location_for_tab()` calls `imp.sidebar.set_location(&location)`.  
`set_location` walks `place_uris` looking for an exact match (`file.equal()`) or the deepest ancestor (`file.has_prefix()`) and calls `list_box.select_row()`. Row is deselected immediately on activation to prevent stale highlight.

### Undo/Redo
`UndoOp` variants live in `window/undo.rs`.  
`rename_selection()` and `new_folder()` push ops after success.  
`undo()` / `redo()` are async (spawn_future_local) to use the same future-based GIO API.  
`update_undo_actions()` gates `win.undo` / `win.redo` sensitivity.

### Texture cache
`TEXTURE_CACHE` in `file_view/cell/mod.rs` is a `thread_local! RefCell<VecDeque<(PathBuf, gdk::Texture)>>`.  
The GSK GL renderer identifies textures by object identity — keeping the same `gdk::Texture` alive keeps its GL texture object in VRAM between bind cycles.  
Eviction: when len ≥ 256, drain the oldest 64 entries (front of deque).

## Actions Reference

Class actions (installed in `imp.rs` `class_init`):

| Action | Shortcut | Handler |
|--------|----------|---------|
| `win.navigate-back` | Alt+Left | `navigate_back()` |
| `win.navigate-forward` | Alt+Right | `navigate_forward()` |
| `win.navigate-up` | Alt+Up | `navigate_up()` |
| `win.toggle-search` | Ctrl+F | `toggle_search()` |
| `win.new-tab` | Ctrl+T | `new_tab()` |
| `win.close-tab` | Ctrl+W | `close_tab()` |
| `win.select-all` | Ctrl+A | `select_all()` |
| `win.focus-location` | Ctrl+L | `focus_location()` |
| `win.copy` | Ctrl+C | `copy_selection()` |
| `win.cut` | Ctrl+X | `cut_selection()` |
| `win.paste` | Ctrl+V | `paste()` |
| `win.rename` | F2 | `rename_selection()` |
| `win.move-to-trash` | Delete | `move_to_trash()` |
| `win.delete-permanently` | Shift+Delete | `delete_permanently()` |
| `win.new-folder` | Ctrl+Shift+N | `new_folder()` |
| `win.open-in-terminal` | Ctrl+Shift+T | `open_in_terminal()` |
| `win.zoom-in` | Ctrl+= | `zoom_in()` |
| `win.zoom-out` | Ctrl+- | `zoom_out()` |
| `win.zoom-reset` | Ctrl+0 | `zoom_reset()` |
| `win.properties` | Alt+Enter | `show_properties()` |
| `win.undo` | Ctrl+Z | `undo()` |
| `win.redo` | Ctrl+Shift+Z | `redo()` |
| `win.add-bookmark` | Ctrl+D | `add_bookmark()` |
| `win.batch-rename` | Ctrl+Shift+R | `batch_rename()` |
| `win.create-link` | — | `create_link()` |

Instance stateful actions (added in `constructed()`):

| Action | Type | Effect |
|--------|------|--------|
| `win.set-view-mode` | string | switches grid ↔ list stack child |
| `win.toggle-hidden` | bool | show/hide dotfiles |
| `win.set-sort-key` | string | per-tab sort key (name/size/date/type) |
| `win.toggle-sort-reversed` | bool | per-tab sort direction |

## CSS Class Reference

| Class | Widget | Notes |
|-------|--------|-------|
| `wren-file-cell` | WrenFileCell root | hover/selected bg, 120ms transition |
| `wren-path-row` | breadcrumb outer box | margin only |
| `wren-crumb-scroll` | ScrolledWindow in breadcrumb | transparent bg |
| `wren-crumb-box` | crumb GtkBox | pill background |
| `wren-breadcrumb` | ancestor crumb buttons | `button.wren-breadcrumb` selector |
| `wren-crumb-sep` | chevron GtkImage | `image.wren-crumb-sep` selector |
| `wren-current-dir` | current dir GtkLabel | bold, `label.wren-current-dir` |
| `wren-edit-location-btn` | pencil GtkButton | color-alpha dimming (not opacity) |
| `wren-path-entry` | Ctrl+L entry | pill shape |

`opacity` is intentionally avoided on frequently-painted widgets — it forces GSK offscreen compositing. Use `color: alpha(...)` instead.

## Common Gotchas

- **CSS selectors**: the class is ON the button (`button.wren-breadcrumb`), not on a parent (`.wren-breadcrumb button`). Getting this wrong means styles silently don't apply.
- **Stateful actions**: `gio::SimpleAction::new_stateful()` for checkmarks/radio buttons in menus. `action.set_state()` inside `connect_activate` updates the menu indicator.
- **Factory replacement for zoom**: calling `grid_view.set_factory(Some(&new_factory))` is the only reliable way to get all visible cells to rebind with a new `icon_size`, since `connect_bind` captures `icon_size` by value.
- **`sorter.changed(SorterChange::Different)`**: must be called after mutating `sort_state` or the sort model won't re-sort.
- **`kf.to_data()`** returns `GString`, not `Result<GString, _>` — don't use `if let Ok(...)`.
- **Borrow splitting**: `tabs.borrow_mut()` and then accessing fields that need immutable borrows of the same `tabs` will panic. Extract values before releasing the borrow.
- **`gio::File`** does not implement `Debug` — `UndoOp` needs a manual `impl Debug`.
