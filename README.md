# Wren

A GTK4/libadwaita file manager written in Rust, inspired by GNOME Nautilus.

## Features

- **Icon grid and list views** — switchable per tab, with virtual scrolling for large directories
- **Thumbnails** — pre-generated from the system cache, GPU-resident texture caching keeps them in VRAM across scroll
- **Zoom** — Ctrl+scroll or Ctrl+±/0 (32 → 48 → 64 → 96 → 128 px)
- **Tabs** — each with independent navigation history, sort state, and view mode
- **Breadcrumb bar** — click any ancestor to jump; Ctrl+L for direct path entry (`~` and any URI scheme accepted)
- **Sidebar** — Places, GTK bookmarks (`~/.config/gtk-3.0/bookmarks`), and mounted volumes; right-click for Open in New Tab / Window / Terminal, Copy Location, Empty Trash
- **Drag and drop** — into folder cells, onto sidebar bookmarks / volumes / Trash, onto breadcrumb ancestors; folders highlight on hover; cell snapshot used as drag cursor
- **Trash management** — Empty Trash from anywhere, Restore From Trash via `trash::orig-path` xattr
- **File operation progress** — header-bar popover with per-op cards: progress bar, current file, source / destination paths, MB/s, ETA, elapsed timer, per-op cancel; desktop notification for ops over 30 s
- **Conflict resolution** — Skip / Replace / Rename / Cancel dialog with "apply to all" on paste and drop collisions
- **Sort** — by name, size, date modified, or type; reversible; per-tab state
- **Search** — Ctrl+F filters by filename; toggle hidden files with Ctrl+H
- **File operations** — copy, cut, paste, duplicate, rename, batch rename, move to trash, permanent delete, new folder, create symlink; symlinks preserved as symlinks across copy
- **Properties** — file type, location, size; live recursive size walk for directories with cancel
- **Undo / redo** — for rename and new folder (Ctrl+Z / Ctrl+Shift+Z)
- **Open in Terminal** — auto-detects kitty, GNOME Terminal, Konsole, Alacritty, and others; custom command in Settings
- **Open With** — application picker for any file type
- **Live monitoring** — folder contents update automatically on external changes; multiple tabs at the same folder stay in sync
- **Persistence** — window size + maximized state, sidebar visibility, last visited directory, theme, view mode, sort, zoom, hidden-files toggle all survive restart
- **stderr op logging** — every destructive operation prints to stderr (run from terminal to see what's happening)
- **Keyboard-driven** — Nautilus-compatible shortcuts throughout

## Screenshots

![Wren main window showing the home directory in grid view](data/screenshots/main-window.png)

## Requirements

- GTK 4.12+
- libadwaita 1.6+
- Rust 2024 edition (stable)

### Fedora / Nobara

```bash
sudo dnf install gtk4-devel libadwaita-devel
```

### Ubuntu / Debian

```bash
sudo apt install libgtk-4-dev libadwaita-1-dev
```

### Arch

```bash
sudo pacman -S gtk4 libadwaita
```

## Building

```bash
git clone https://github.com/Gren-95/wren
cd wren
cargo build --release
```

## Running

```bash
cargo run --release
```

Or after `cargo install --path .`:

```bash
wren
```

## Keyboard Shortcuts

| Action | Shortcut |
|--------|----------|
| Navigate back / forward | Alt+Left / Alt+Right |
| Navigate up | Alt+Up |
| Navigate home | Alt+Home |
| Reload | F5 |
| New tab / new window | Ctrl+T / Ctrl+N |
| Close tab | Ctrl+W |
| Focus path bar | Ctrl+L |
| Toggle search | Ctrl+F |
| Toggle hidden files | Ctrl+H |
| Select all | Ctrl+A |
| Open in terminal | Ctrl+Shift+T |
| New folder | Ctrl+Shift+N |
| Rename | F2 |
| Batch rename | Ctrl+Shift+R |
| Copy / Cut / Paste | Ctrl+C / X / V |
| Move to trash | Delete |
| Delete permanently | Shift+Delete |
| Undo / Redo | Ctrl+Z / Ctrl+Shift+Z |
| Zoom in / out / reset | Ctrl+= / Ctrl+- / Ctrl+0 |
| Properties | Alt+Enter |
| Add bookmark | Ctrl+D |
| Open with | Ctrl+Shift+O |
| Show shortcuts | Ctrl+? |
| Duplicate / Create link | *(context menu)* |

## License

GNU General Public License v3.0 or later (GPL-3.0-or-later). See [LICENSE](LICENSE).
