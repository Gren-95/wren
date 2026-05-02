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
// the default writer. Filtering is purely by substring so genuine
// problems still surface.
//
//   "reported min width -3, but sizes must be >= 0"
//     Fired during initial layout of ellipsizing GtkLabels — a
//     long-standing GTK4 quirk observed in every GTK4 file manager.
//
//   "Finalizing ..., but it still has children left: GtkPopoverMenu"
//     Fires only at app dispose. PopoverMenus parented to file
//     views / sidebar rows would be unparented if we connected to
//     unrealize, but unrealize misfires on stack-page hides
//     (grid↔list view switch) and on sidebar rebuilds — detaching
//     mid-session and breaking subsequent right-clicks.
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
