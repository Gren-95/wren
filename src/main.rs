mod application;
mod breadcrumb;
mod file_view;
mod model;
mod navigation;
mod operations;
mod sidebar;
mod window;

use application::WrenApplication;
use gtk4::prelude::*;

fn main() -> glib::ExitCode {
    gio::resources_register_include!("wren.gresource").expect("failed to register resources");
    WrenApplication::new("io.github.wren").run()
}
