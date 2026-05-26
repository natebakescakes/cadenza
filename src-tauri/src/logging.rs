// Lightweight durable logging for Cadenza.
//
// Appends timestamped lines to `<data_dir>/cadenza.log` (macOS:
// `~/Library/Application Support/Cadenza/cadenza.log`) and mirrors them to
// stderr. Deliberately tiny: no logging framework, no async, never panics.
// Used to diagnose crashes/permission issues that only reproduce on the user's
// machine (keylogger tap install, detector thread lifecycle, caught errors).

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::storage::Storage;

/// Rotate the log once it passes this size, keeping a single `.1` backup. Caps
/// total log footprint so a long session can't grow the file without bound.
const MAX_LOG_BYTES: u64 = 8 * 1024 * 1024;

/// Path to the rolling log file: `<data_dir>/cadenza.log`.
pub fn log_path() -> PathBuf {
    let mut p = Storage::data_dir();
    p.push("cadenza.log");
    p
}

/// Open log handle held for the app's lifetime. `log_line` is called on the
/// detector hot path (per word/hint), so we open the file ONCE and reuse the
/// handle rather than create_dir_all + open + close on every call — that
/// per-call syscall churn was an O(keystrokes) drag during long sessions.
struct LogFile {
    file: File,
    written: u64,
}

static LOG: OnceLock<Mutex<Option<LogFile>>> = OnceLock::new();

/// Open (or create) the log file in append mode, seeding the written counter
/// from its current size so rotation accounts for pre-existing content.
fn open_log() -> Option<LogFile> {
    let dir = Storage::data_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
        .ok()?;
    let written = file.metadata().map(|m| m.len()).unwrap_or(0);
    Some(LogFile { file, written })
}

/// Append a timestamped line to the log file (and echo to stderr in debug
/// builds). Best-effort: any IO error is swallowed — logging must never break
/// the app. Reuses one open handle and rotates past `MAX_LOG_BYTES`.
pub fn log_line(msg: &str) {
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
    let line = format!("[{ts}] {msg}");
    // Stderr mirror is dev-only: in release it's pure overhead on the hot path
    // (and there's usually no attached terminal anyway).
    #[cfg(debug_assertions)]
    eprintln!("{line}");

    let slot = LOG.get_or_init(|| Mutex::new(open_log()));
    let Ok(mut guard) = slot.lock() else { return };
    // Retry opening if a previous attempt failed (e.g. dir not ready at boot).
    if guard.is_none() {
        *guard = open_log();
    }
    let Some(state) = guard.as_mut() else { return };

    // Rotate before the file outgrows the cap: keep a single `.1` backup.
    if state.written >= MAX_LOG_BYTES {
        let _ = state.file.flush();
        let _ = std::fs::rename(log_path(), log_path().with_file_name("cadenza.log.1"));
        match open_log() {
            Some(fresh) => *state = fresh,
            None => return,
        }
    }

    if writeln!(state.file, "{line}").is_ok() {
        state.written += line.len() as u64 + 1;
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
