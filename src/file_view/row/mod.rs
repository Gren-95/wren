mod imp;

use adw::subclass::prelude::*;
use glib::Object;
use gio::prelude::*;
use gtk4::prelude::*;

use crate::application::WrenApplication;
use crate::model::FileObject;

/// Visibility for each toggleable list-view column. Snapshot taken
/// at bind time so each row reflects the current preferences.
#[derive(Clone, Copy, Debug)]
pub struct ColumnVisibility {
    pub content_type: bool,
    pub size: bool,
    pub modified: bool,
    pub permissions: bool,
    pub owner: bool,
    pub group: bool,
    pub accessed: bool,
}

impl ColumnVisibility {
    pub fn from_application() -> Self {
        let app = gio::Application::default().and_downcast::<WrenApplication>();
        if let Some(app) = app {
            Self {
                content_type: app.show_col_type(),
                size: app.show_col_size(),
                modified: app.show_col_modified(),
                permissions: app.show_col_permissions(),
                owner: app.show_col_owner(),
                group: app.show_col_group(),
                accessed: app.show_col_accessed(),
            }
        } else {
            Self {
                content_type: true,
                size: true,
                modified: true,
                permissions: false,
                owner: false,
                group: false,
                accessed: false,
            }
        }
    }
}

glib::wrapper! {
    pub struct WrenFileRow(ObjectSubclass<imp::WrenFileRow>)
        @extends gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl Default for WrenFileRow {
    fn default() -> Self {
        Self::new()
    }
}

impl WrenFileRow {
    pub fn new() -> Self {
        Object::builder().build()
    }

    fn imp(&self) -> &imp::WrenFileRow {
        imp::WrenFileRow::from_obj(self)
    }

    pub fn bound_file_object(&self) -> Option<FileObject> {
        self.imp().bound_file.borrow().clone()
    }

    pub fn bind(&self, file_obj: &FileObject, icon_size: u32, show_extension: bool) {
        let imp = self.imp();
        *imp.bound_file.borrow_mut() = Some(file_obj.clone());
        imp.icon_size.set(icon_size);
        imp.icon.set_pixel_size(icon_size as i32);

        let display_name = if show_extension {
            file_obj.name()
        } else {
            strip_extension_name(&file_obj.name())
        };
        imp.name.set_label(&display_name);

        let cols = ColumnVisibility::from_application();

        if file_obj.is_directory() {
            imp.content_type.set_label("Folder");
            imp.size.set_label("—");
        } else {
            imp.content_type.set_label(&file_obj.content_type());
            imp.size.set_label(&format_size(file_obj.file_size()));
        }
        imp.content_type.set_visible(cols.content_type);
        imp.size.set_visible(cols.size);

        let ts = file_obj.modified();
        if ts > 0 {
            imp.modified.set_label(&format_modified(ts));
        } else {
            imp.modified.set_label("");
        }
        imp.modified.set_visible(cols.modified);

        // Optional columns. Display "—" when the backend doesn't expose
        // the underlying attribute (typical for trash://, smb://, mtp://).
        if cols.permissions {
            let label = file_obj
                .unix_mode()
                .map(format_permissions)
                .unwrap_or_else(|| "—".to_string());
            imp.permissions.set_label(&label);
        }
        imp.permissions.set_visible(cols.permissions);

        if cols.owner {
            imp.owner
                .set_label(&file_obj.owner_user().unwrap_or_else(|| "—".to_string()));
        }
        imp.owner.set_visible(cols.owner);

        if cols.group {
            imp.group
                .set_label(&file_obj.owner_group().unwrap_or_else(|| "—".to_string()));
        }
        imp.group.set_visible(cols.group);

        if cols.accessed {
            let at = file_obj.accessed();
            let label = if at > 0 {
                format_modified(at)
            } else {
                "—".to_string()
            };
            imp.accessed.set_label(&label);
        }
        imp.accessed.set_visible(cols.accessed);

        if let Some(icon) = file_obj.icon() {
            imp.icon.set_from_gicon(&icon);
        } else if file_obj.is_directory() {
            imp.icon.set_icon_name(Some("folder-symbolic"));
        } else {
            imp.icon.set_icon_name(Some("text-x-generic-symbolic"));
        }
        // See cell::bind for why we use a manual overlay badge
        // instead of EmblemedIcon.
        imp.symlink_badge.set_visible(file_obj.is_symlink());
        imp.unreadable_badge.set_visible(!file_obj.is_readable());

        // Hover tooltip = full path (URI fallback for non-local files).
        let tooltip = file_obj
            .file()
            .path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| file_obj.file().uri().to_string());
        self.set_tooltip_text(Some(&tooltip));
    }

    pub fn set_icon_size(&self, px: u32) {
        let imp = self.imp();
        imp.icon_size.set(px);
        imp.icon.set_pixel_size(px as i32);
        self.queue_resize();
    }

    pub fn unbind(&self) {
        let imp = self.imp();
        *imp.bound_file.borrow_mut() = None;
        imp.name.set_label("");
        imp.content_type.set_label("");
        imp.size.set_label("");
        imp.modified.set_label("");
        imp.permissions.set_label("");
        imp.owner.set_label("");
        imp.group.set_label("");
        imp.accessed.set_label("");
        imp.icon.set_pixel_size(24);
        imp.icon.clear();
    }
}

/// Format a unix mode (permission bits + type) as `rwxr-xr-x`.
/// Special bits (setuid, setgid, sticky) are folded into the
/// corresponding execute slot using the standard `s`/`S`/`t`/`T` letters.
fn format_permissions(mode: u32) -> String {
    let mut out = String::with_capacity(9);

    let bit = |b: u32| -> bool { mode & b != 0 };

    let triplet = |out: &mut String, r: u32, w: u32, x: u32, sp: u32, sp_lc: char, sp_uc: char| {
        out.push(if bit(r) { 'r' } else { '-' });
        out.push(if bit(w) { 'w' } else { '-' });
        out.push(match (bit(x), bit(sp)) {
            (true, true) => sp_lc,
            (false, true) => sp_uc,
            (true, false) => 'x',
            (false, false) => '-',
        });
    };

    triplet(&mut out, 0o400, 0o200, 0o100, 0o4000, 's', 'S');
    triplet(&mut out, 0o040, 0o020, 0o010, 0o2000, 's', 'S');
    triplet(&mut out, 0o004, 0o002, 0o001, 0o1000, 't', 'T');
    out
}

fn strip_extension_name(name: &str) -> String {
    if name.starts_with('.') {
        return name.to_string();
    }
    match name.rfind('.') {
        Some(pos) if pos > 0 => name[..pos].to_string(),
        _ => name.to_string(),
    }
}

fn format_modified(ts: i64) -> String {
    let Ok(file_dt) = glib::DateTime::from_unix_local(ts) else {
        return String::new();
    };
    let now_unix = glib::DateTime::now_local()
        .map(|dt| dt.to_unix())
        .unwrap_or(ts);
    let age = now_unix.saturating_sub(ts);

    if age < 60 {
        "Just now".to_string()
    } else if age < 3600 {
        let m = age / 60;
        format!("{m} minute{} ago", if m == 1 { "" } else { "s" })
    } else if age < 86_400 {
        let h = age / 3600;
        format!("{h} hour{} ago", if h == 1 { "" } else { "s" })
    } else if age < 86_400 * 2 {
        "Yesterday".to_string()
    } else if age < 86_400 * 7 {
        let d = age / 86_400;
        format!("{d} days ago")
    } else {
        file_dt
            .format("%e %b %Y")
            .map(|s| s.to_string())
            .unwrap_or_default()
    }
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
