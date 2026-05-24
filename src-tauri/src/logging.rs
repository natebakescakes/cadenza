// Lightweight durable logging for Cadenza.
//
// Appends timestamped lines to `<data_dir>/cadenza.log` (macOS:
// `~/Library/Application Support/Cadenza/cadenza.log`) and mirrors them to
// stderr. Deliberately tiny: no logging framework, no async, never panics.
// Used to diagnose crashes/permission issues that only reproduce on the user's
// machine (keylogger tap install, detector thread lifecycle, caught errors).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use crate::storage::Storage;

/// Path to the rolling log file: `<data_dir>/cadenza.log`.
pub fn log_path() -> PathBuf {
    let mut p = Storage::data_dir();
    p.push("cadenza.log");
    p
}

/// Append a timestamped line to the log file and echo to stderr. Best-effort:
/// any IO error is swallowed (we never want logging to break the app).
pub fn log_line(msg: &str) {
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
    let line = format!("[{ts}] {msg}");
    eprintln!("{line}");

    // Best-effort append; ensure the directory exists first.
    let dir = Storage::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        let _ = writeln!(f, "{line}");
    }
}

/// Install a global panic hook that appends panic info (+ backtrace if the
/// `RUST_BACKTRACE` env enables it) to the log file, then still prints to
/// stderr. Call once, early in `run()`.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".to_string()
        };
        let bt = std::backtrace::Backtrace::force_capture();
        log_line(&format!(
            "PANIC at {location}: {payload}\nbacktrace:\n{bt}"
        ));
        // Preserve default behavior (prints to stderr / aborts as configured).
        default(info);
    }));
}
