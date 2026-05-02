use std::cell::OnceCell;

use glib::prelude::*;
use glib::subclass::prelude::*;

mod imp {
    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::FileObject)]
    pub struct FileObject {
        pub file: OnceCell<gio::File>,
        pub file_info: OnceCell<gio::FileInfo>,

        #[property(get, set, construct_only)]
        pub name: std::cell::RefCell<String>,
        #[property(get, set, construct_only)]
        pub content_type: std::cell::RefCell<String>,
        #[property(get, set, construct_only)]
        pub is_directory: std::cell::Cell<bool>,
        #[property(get, set, construct_only)]
        pub file_size: std::cell::Cell<u64>,
        #[property(get, set, construct_only)]
        pub modified: std::cell::Cell<i64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FileObject {
        const NAME: &'static str = "WrenFileObject";
        type Type = super::FileObject;
    }

    #[glib::derived_properties]
    impl ObjectImpl for FileObject {}
}

glib::wrapper! {
    pub struct FileObject(ObjectSubclass<imp::FileObject>);
}

impl FileObject {
    pub const QUERY_ATTRS: &'static str =
        "standard::name,standard::display-name,standard::type,standard::icon,\
         standard::content-type,standard::size,standard::is-hidden,\
         standard::is-symlink,time::modified,access::can-delete,\
         access::can-rename,thumbnail::path";

    pub fn new(file: gio::File, info: gio::FileInfo) -> Self {
        let name = info.display_name().to_string();
        let content_type = info
            .content_type()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let is_directory = info.file_type() == gio::FileType::Directory;
        let file_size = info.size().max(0) as u64;
        let modified = info
            .modification_date_time()
            .map(|dt| dt.to_unix())
            .unwrap_or(0);

        let obj: Self = glib::Object::builder()
            .property("name", &name)
            .property("content-type", &content_type)
            .property("is-directory", is_directory)
            .property("file-size", file_size)
            .property("modified", modified)
            .build();

        obj.imp().file.set(file).ok();
        obj.imp().file_info.set(info).ok();
        obj
    }

    fn imp(&self) -> &imp::FileObject {
        imp::FileObject::from_obj(self)
    }

    pub fn file(&self) -> &gio::File {
        self.imp().file.get().expect("file set at construction")
    }

    pub fn file_info(&self) -> &gio::FileInfo {
        self.imp().file_info.get().expect("file_info set at construction")
    }

    pub fn is_hidden(&self) -> bool {
        self.file_info().is_hidden()
    }

    pub fn icon(&self) -> Option<gio::Icon> {
        self.file_info().icon()
    }

    /// Path to a pre-generated thumbnail, if one exists in the thumbnail cache.
    pub fn thumbnail_path(&self) -> Option<std::path::PathBuf> {
        self.file_info()
            .attribute_byte_string("thumbnail::path")
            .map(|gs| std::path::PathBuf::from(gs.as_str()))
    }

    /// True when the underlying file is a symlink. Driven by
    /// `standard::is-symlink` (queried via QUERY_ATTRS) — file-views
    /// use this to overlay a small arrow badge on the icon.
    pub fn is_symlink(&self) -> bool {
        self.file_info().is_symlink()
    }
}
