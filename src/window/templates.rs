//! XDG-spec ~/Templates support — files in the user's Templates directory
//! become "New from Template" entries in the empty-area context menu.

use std::path::PathBuf;

/// Returns the user's Templates directory if it's configured AND distinct
/// from the home directory. Some GNOME setups point Templates at $HOME,
/// in which case treating every dotfile-or-not in $HOME as a template is
/// nonsense — bail out.
pub fn templates_dir() -> Option<PathBuf> {
    let dir = glib::user_special_dir(glib::UserDirectory::Templates)?;
    if dir == glib::home_dir() {
        return None;
    }
    if !dir.is_dir() {
        return None;
    }
    Some(dir)
}

/// Enumerate regular non-hidden files in ~/Templates, sorted by name.
/// Returns (display_name, absolute_path) pairs. Synchronous std::fs is
/// fine here — Templates is conventionally small and we read it lazily
/// on each right-click.
pub fn list_templates() -> Vec<(String, PathBuf)> {
    let Some(dir) = templates_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let path = e.path();
            // Regular files only — skip directories and symlinks-to-dirs.
            // Use metadata() (follows symlinks) so a symlink to a real
            // template file is still usable.
            let md = std::fs::metadata(&path).ok()?;
            if !md.is_file() {
                return None;
            }
            Some((name, path))
        })
        .collect();
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out
}
