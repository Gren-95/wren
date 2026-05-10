// Test-only helpers shared across modules. GTK pins its "main thread"
// to whichever thread first calls gtk::init(); subsequent calls from
// other threads panic. So all GTK-touching test bodies dispatch onto
// a single dedicated worker we own here.

#![cfg(test)]

use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};

type Job = Box<dyn FnOnce() + Send + 'static>;
static SENDER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();

/// Run `f` on the shared GTK-initialised worker thread and return its result.
/// First call spins up the thread, calls `gtk::init()`, and registers the
/// build-time-bundled GResource so CompositeTemplate widgets can resolve
/// their .ui files.
pub(crate) fn run_on_gtk_thread<R: Send + 'static>(
    f: impl FnOnce() -> R + Send + 'static,
) -> R {
    let tx = SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::spawn(move || {
            gtk4::init().expect("gtk::init on test worker thread");
            // Register the bundled GResource so widgets that load .ui via
            // /io/github/wren/... can find their templates.
            gio::resources_register_include!("wren.gresource")
                .expect("register wren.gresource");
            while let Ok(job) = rx.recv() {
                job();
            }
        });
        Mutex::new(tx)
    });

    let (rtx, rrx) = mpsc::channel::<R>();
    let job: Job = Box::new(move || {
        let _ = rtx.send(f());
    });
    tx.lock().unwrap().send(job).unwrap();
    rrx.recv().expect("GTK worker panicked")
}
