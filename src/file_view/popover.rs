use gtk4::prelude::*;

/// Walk a `PopoverMenu`'s child tree and pin every inner `ScrolledWindow` to
/// vertical-only scrolling. GtkPopoverMenu wraps its contents in a
/// `ScrolledWindow` whose horizontal policy defaults to `Automatic` — long
/// menu item labels (e.g. "Move to Trash" plus a localised accelerator hint)
/// can trip it into showing a horizontal bar that the user has to scroll. We
/// never want horizontal scrolling here; the popover should grow as wide as
/// its widest item.
pub fn lock_vertical_only(widget: &impl IsA<gtk4::Widget>) {
    if let Some(sw) = widget.dynamic_cast_ref::<gtk4::ScrolledWindow>() {
        sw.set_hscrollbar_policy(gtk4::PolicyType::Never);
        sw.set_propagate_natural_width(true);
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        lock_vertical_only(&c);
        child = c.next_sibling();
    }
}
