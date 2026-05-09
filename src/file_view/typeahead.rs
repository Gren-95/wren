// Type-ahead selection: typing printable characters while a file view
// has focus jumps the selection to the first item whose display name
// starts with the buffered prefix. The buffer resets after a brief
// idle period, matching Nautilus's behaviour.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gdk;
use gtk4::prelude::*;

use crate::model::FileObject;

// 750ms idle reset matches GtkTreeView's TYPEAHEAD_TIMEOUT and feels
// natural for pecking out a multi-character prefix without lingering.
const RESET_MS: u64 = 750;

/// Per-view type-ahead state. `prefix` accumulates lower-cased chars;
/// `timeout` is the active reset timer that we cancel-and-reschedule
/// on every keystroke.
#[derive(Debug, Default)]
pub struct TypeaheadState {
    pub prefix: RefCell<String>,
    pub timeout: RefCell<Option<glib::SourceId>>,
}

impl TypeaheadState {
    pub fn clear(&self) {
        self.prefix.borrow_mut().clear();
        if let Some(src) = self.timeout.borrow_mut().take() {
            src.remove();
        }
    }
}

// Characters worth feeding into the prefix buffer. Letters and digits
// are obvious; the punctuation set covers what real filenames actually
// contain. Anything else (control chars, isolated symbols) is left for
// the focus widget so accels and view shortcuts still fire.
fn is_typeahead_char(ch: char) -> bool {
    if ch.is_alphanumeric() {
        return true;
    }
    matches!(ch, '.' | '_' | '-' | ' ' | '(' | ')' | '\'' | ',')
}

/// Walk `selection` in display order and return the first position
/// whose item's lower-cased name starts with `prefix`.
fn find_match(selection: &gtk4::MultiSelection, prefix: &str) -> Option<u32> {
    let n = selection.n_items();
    for pos in 0..n {
        let Some(obj) = selection.item(pos).and_downcast::<FileObject>() else {
            continue;
        };
        if obj.name().to_lowercase().starts_with(prefix) {
            return Some(pos);
        }
    }
    None
}

/// Attach a key controller to a GridView or ListView. The controller
/// drives `state` (lives on the wrapper widget's imp), reads the
/// view's `MultiSelection` to pick a match, then calls `scroll_to`
/// with the matched position so the concrete view can bring the cell
/// or row into view.
///
/// `view` must be a GtkGridView or GtkListView whose model is a
/// MultiSelection; otherwise the controller is a no-op.
pub fn attach<V, F>(view: &V, state: Rc<TypeaheadState>, scroll_to: F)
where
    V: IsA<gtk4::Widget> + glib::clone::Downgrade,
    <V as glib::clone::Downgrade>::Weak: glib::clone::Upgrade<Strong = V>,
    F: Fn(u32) + 'static,
{
    let key = gtk4::EventControllerKey::new();
    let scroll_to = Rc::new(scroll_to);
    key.connect_key_pressed(glib::clone!(
        #[weak] view,
        #[strong] state,
        #[strong] scroll_to,
        #[upgrade_or] glib::Propagation::Proceed,
        move |_, keyval, _keycode, modifiers| {
            // Anything with Ctrl/Alt/Super is an accelerator; pass it
            // through so window actions still fire.
            let m = modifiers
                & (gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::ALT_MASK
                    | gdk::ModifierType::SUPER_MASK
                    | gdk::ModifierType::META_MASK);
            if !m.is_empty() {
                return glib::Propagation::Proceed;
            }

            // Escape clears the buffer immediately and propagates so
            // the window can still react (e.g. close search bar).
            if keyval == gdk::Key::Escape {
                state.clear();
                return glib::Propagation::Proceed;
            }

            // Backspace edits the prefix in place. Only consume the
            // key when there's actually a prefix to shorten.
            if keyval == gdk::Key::BackSpace {
                let mut p = state.prefix.borrow_mut();
                if p.is_empty() {
                    return glib::Propagation::Proceed;
                }
                p.pop();
                let new_prefix = p.clone();
                drop(p);
                if new_prefix.is_empty() {
                    state.clear();
                } else {
                    schedule_reset(&state);
                    apply_match(&view, &new_prefix, &scroll_to);
                }
                return glib::Propagation::Stop;
            }

            let Some(ch) = keyval.to_unicode() else {
                return glib::Propagation::Proceed;
            };
            if !is_typeahead_char(ch) {
                return glib::Propagation::Proceed;
            }

            state.prefix.borrow_mut().push(ch.to_ascii_lowercase());
            let prefix = state.prefix.borrow().clone();
            schedule_reset(&state);
            apply_match(&view, &prefix, &scroll_to);
            glib::Propagation::Stop
        }
    ));
    view.add_controller(key);
}

fn schedule_reset(state: &Rc<TypeaheadState>) {
    if let Some(old) = state.timeout.borrow_mut().take() {
        old.remove();
    }
    let state2 = Rc::clone(state);
    let id = glib::timeout_add_local_once(
        std::time::Duration::from_millis(RESET_MS),
        move || {
            state2.prefix.borrow_mut().clear();
            state2.timeout.borrow_mut().take();
        },
    );
    *state.timeout.borrow_mut() = Some(id);
}

fn apply_match<V, F>(view: &V, prefix: &str, scroll_to: &Rc<F>)
where
    V: IsA<gtk4::Widget>,
    F: Fn(u32) + ?Sized,
{
    let widget: &gtk4::Widget = view.upcast_ref();
    let selection = if let Some(gv) = widget.downcast_ref::<gtk4::GridView>() {
        gv.model().and_downcast::<gtk4::MultiSelection>()
    } else if let Some(lv) = widget.downcast_ref::<gtk4::ListView>() {
        lv.model().and_downcast::<gtk4::MultiSelection>()
    } else {
        None
    };
    let Some(selection) = selection else { return };
    if let Some(pos) = find_match(&selection, prefix) {
        selection.select_item(pos, true);
        scroll_to(pos);
    }
    // No match: keep the prefix so the user can recover by adding
    // chars or hitting Backspace. Nautilus does the same.
}
