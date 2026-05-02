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
// the default writer. Real problems still pass through.
//
//   "reported min width -3, but sizes must be >= 0"
//     Long-standing GTK4 quirk observed in every GTK4 file manager —
//     fires during initial layout of ellipsizing GtkLabels.
//
//   "still has children left: GtkPopoverMenu"
//     Fires at app shutdown from sidebar row popovers. Each sidebar
//     row owns its own context-menu PopoverMenu and they would all
//     need a deterministic unparent point that doesn't misfire on
//     mid-session sidebar rebuilds. The file-view popovers are now
//     cleaned up properly in dispose — this filter only covers the
//     sidebar case.
fn install_log_filter() {
    let dropped = [
        "reported min width",
        "still has children left: GtkPopoverMenu",
    ];
    glib::log_set_writer_func(move |level, fields| {
        for field in fields {
            if field.key() == "MESSAGE"
                && field
                    .value_str()
                    .is_some_and(|s| dropped.iter().any(|needle| s.contains(needle)))
            {
                return glib::LogWriterOutput::Handled;
            }
        }
        glib::log_writer_default(level, fields)
    });
}
