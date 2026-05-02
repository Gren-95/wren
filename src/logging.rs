//! Debug logging — gated by a single atomic bool driven from the Settings
//! "Debug logging" switch row. When the toggle is off the macro reduces to
//! a single relaxed atomic load + branch, so leaving log calls in hot
//! paths costs essentially nothing.

use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn set_enabled(v: bool) {
    ENABLED.store(v, Ordering::Relaxed);
}

pub fn log_args(args: std::fmt::Arguments<'_>) {
    eprintln!("[wren {}] {}", ts(), args);
}

fn ts() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let total_ms = now.as_millis();
    let s = (total_ms / 1000) % 86400;
    let h = (s / 3600) as u32;
    let m = ((s / 60) % 60) as u32;
    let sec = (s % 60) as u32;
    let ms = (total_ms % 1000) as u32;
    format!("{:02}:{:02}:{:02}.{:03}", h, m, sec, ms)
}

#[macro_export]
macro_rules! wren_log {
    ($($arg:tt)*) => {
        if $crate::logging::is_enabled() {
            $crate::logging::log_args(format_args!($($arg)*));
        }
    };
}
