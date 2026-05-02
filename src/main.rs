mod application;
mod breadcrumb;
mod file_view;
mod logging;
mod model;
mod navigation;
mod sidebar;
mod window;

use application::WrenApplication;
use gtk4::prelude::*;

fn main() -> glib::ExitCode {
    install_log_filter();
    gio::resources_register_include!("wren.gresource").expect("failed to register resources");
    WrenApplication::new("io.github.wren").run()
}

// Drop a small set of known-harmless GTK4 warnings before they reach
// the default writer. The "reported min width -3" warning fires when
// ellipsizing GtkLabels are measured in a context that hasn't laid out
// fonts yet — a long-standing GTK4 quirk filed upstream and observed
// in every GTK4 file manager. Filtering by exact substring keeps real
// warnings visible.
fn install_log_filter() {
    glib::log_set_writer_func(|level, fields| {
        for field in fields {
            if field.key() == "MESSAGE"
                && field
                    .value_str()
                    .is_some_and(|s| s.contains("reported min width") && s.contains("but sizes must be >= 0"))
            {
                return glib::LogWriterOutput::Handled;
            }
        }
        glib::log_writer_default(level, fields)
    });
}
