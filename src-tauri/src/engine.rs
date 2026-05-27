// Word/chord detection engine — port of CharaChorder `nexus` Freqlog
// `_process_queue`.
//
// The `Detector` consumes `KeyEvent`s from the keylogger channel, builds a word
// buffer char-by-char, and on each flush classifies the buffer:
//   - avg inter-char interval > chord_char_threshold_ms  => typed by HUMAN => WORD
//   - otherwise the chars arrived faster than humanly possible => CHORD
// Flush triggers: whitespace/disallowed char, idle > new_word_threshold_s,
// non-char/modifier boundary, or stop. Only buffers of length >= 2 are logged.
//
// On flush the detector writes to its OWN sqlite Connection (WAL) and emits
// Tauri events (word_logged / chord_logged / wpm) via the AppHandle.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::Receiver;
use parking_lot::{Mutex, RwLock};
use tauri::AppHandle;

use crate::storage::{CachedChordMaps, Storage};
use crate::types::{DeviceInfo, KeyEvent, Settings};

mod coaching;
mod detector;
mod emit;
mod session;

/// Allocate the next coaching hint id from the shared, process-global sequence.
///
/// The sequence lives in `AppState` (not the `Detector`), so it keeps climbing
/// across detector respawns (stop/start logging). The `coaching_position`
/// listener keeps a monotonic high-water mark and drops any id below it; a
/// per-Detector counter would reset to 0 on respawn and have its positions
/// silently dropped until it climbed back past the watermark — the overlay
/// would fail to appear, then "recover" on its own once it caught up.
fn next_hint_id(seq: &AtomicI64) -> i64 {
    seq.fetch_add(1, Ordering::Relaxed) + 1
}

/// Lightweight settings holder kept in `AppState` (the live detection loop runs
/// on its own thread; see `spawn`). Retained so existing wiring/API stays valid.
pub struct Engine {
    settings: Settings,
}

impl Engine {
    pub fn new(settings: Settings) -> Self {
        Self { settings }
    }

    pub fn update_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    pub fn settings(&self) -> Settings {
        self.settings.clone()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(Settings::default())
    }
}

/// Handle to a running detector thread.
pub struct DetectorHandle {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl DetectorHandle {
    /// Signal the detector to stop and join its thread.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Spawn the detector loop on a dedicated thread. It opens its own Connection to
/// the same sqlite file (WAL allows concurrent reader on the command thread).
/// `settings` is shared so changes via `set_settings` take effect live.
/// `chord_phrases` is the normalized device chordmap set for arpeggio lookup.
pub fn spawn(
    rx: Receiver<KeyEvent>,
    settings: Arc<Mutex<Settings>>,
    chord_phrases: Arc<RwLock<HashSet<String>>>,
    device: Arc<Mutex<Option<DeviceInfo>>>,
    coaching_overlay_visible: Arc<AtomicBool>,
    hint_seq: Arc<AtomicI64>,
    chordmap_gen: Arc<AtomicI64>,
    practice_active: Arc<AtomicBool>,
    practice_target: Arc<Mutex<Option<String>>>,
    app: AppHandle,
) -> DetectorHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let handle = std::thread::Builder::new()
        .name("cadenza-detector".into())
        .spawn(move || {
            crate::logging::log_line("detector thread: started");
            // Detector owns its own connection. If it can't open, bail quietly.
            let store = match Storage::open() {
                Ok(c) => Storage::from_connection(c),
                Err(e) => {
                    crate::logging::log_line(&format!(
                        "detector thread: failed to open storage: {e}"
                    ));
                    return;
                }
            };
            let mut det = Detector::new(
                store,
                settings,
                chord_phrases,
                device,
                coaching_overlay_visible,
                hint_seq,
                chordmap_gen,
                practice_active,
                practice_target,
                app,
            );
            det.run(rx, stop_thread);
            crate::logging::log_line("detector thread: stopped");
        })
        .ok();
    DetectorHandle { stop, handle }
}

/// Per-buffer timing state mirroring nexus's `_process_queue` locals.
struct Detector {
    store: Storage,
    settings: Arc<Mutex<Settings>>,
    /// Shared read-only view of the normalized device chord phrase set.
    chord_phrases: Arc<RwLock<HashSet<String>>>,
    /// Live shared device info — read at emit time so device_id stays correct
    /// even when a device connects AFTER `start_logging`.
    device: Arc<Mutex<Option<DeviceInfo>>>,
    /// Set true while a coaching overlay is showing; gates `EVT_KEYSTROKE`
    /// emission in `process()` so it doesn't flood IPC in steady state.
    coaching_overlay_visible: Arc<AtomicBool>,
    /// Process-global monotonic coaching hint id source (shared from AppState).
    /// Kept here as a shared atomic — NOT a per-Detector counter — so ids keep
    /// climbing across detector respawns and never fall below the
    /// `coaching_position` listener's monotonic watermark.
    hint_seq: Arc<AtomicI64>,
    /// Shared snapshot of the latest hint id, read by the detached clear-timer
    /// thread so an older timer never clears the flag after a newer hint fired.
    latest_hint_id: Arc<AtomicI64>,
    /// Shared chordmap generation counter (bumped by `refresh_chordmap`). When it
    /// climbs past `cached_maps_gen` the cached chord maps are stale and rebuilt.
    chordmap_gen: Arc<AtomicI64>,
    /// Per-session cache of the phrase-independent coaching chord maps. Filled
    /// lazily on the first manual word and reused until the device_id changes or
    /// the chordmap generation bumps — avoids rebuilding from SQL every word.
    cached_maps: Option<CachedChordMaps>,
    /// The device_id the cached maps were built for (empty when None). A change
    /// (device connect/disconnect) invalidates the cache.
    cached_maps_device_id: String,
    /// The `chordmap_gen` value the cached maps were built at.
    cached_maps_gen: i64,
    /// When TRUE the detector is in practice mode: ALL ambient stat writes +
    /// emits are suppressed and a `practice_chord` event is emitted on each
    /// chord fire instead. Shared with AppState so it flips live.
    practice_active: Arc<AtomicBool>,
    /// The phrase the user is being asked to drill, used to set `correct` on the
    /// emitted `practice_chord` (case-insensitive, trimmed compare). Shared.
    practice_target: Arc<Mutex<Option<String>>>,
    app: AppHandle,

    word: String,
    word_start_time: Option<i64>,
    word_end_time: Option<i64>,
    chars_since_last_bs: i64,
    /// Average inter-char interval (ms) since the last backspace.
    avg_char_time_after_last_bs: Option<f64>,
    /// Largest single gap (ms) between consecutive chars since last backspace.
    max_inter_char_ms: f64,
    last_key_was_disallowed: bool,
    /// True if ANY backspace removed a char from the current token while building it.
    /// Set whenever we pop from a non-empty buffer. Reset on every flush.
    current_had_correction: bool,
    /// Phrase most recently flushed as chorded. Used to detect "fired then deleted"
    /// errors by counting subsequent backstrokes. Persists across flushes; cleared
    /// when the user starts typing new content or the time window expires.
    pending_chord: Option<String>,
    /// Timestamp (ms) when pending_chord was set.
    pending_chord_ts: i64,
    /// Number of BS keypresses received since pending_chord was set.
    pending_bs_count: i64,
    /// Last chorded phrase — kept after pending_chord clears so that a manual
    /// re-type of the same phrase within the window signals a retype error.
    last_chord_phrase: Option<String>,
    /// Timestamp of the last chord flush; pairs with last_chord_phrase.
    last_chord_ts: i64,
    /// Last manually-flushed phrase (for split-word detection).
    prev_flush_phrase: Option<String>,
    /// Timestamp of that flush.
    prev_flush_ts: i64,
    /// Most recently chord-deleted phrase (for chord confusion detection).
    last_deleted_phrase: Option<String>,
    /// Timestamp of the last chord deletion.
    last_deleted_ts: i64,
    /// Backstrokes received against an empty buffer since the last chord flush.
    /// Detects the CharaChorder "quickfix" burst after the buffer has already drained.
    empty_buf_bs_count: i64,
    /// Timestamp when the buffer last drained to empty via backspace (aborted attempt).
    /// Used to attribute a preceding failed chord attempt to the next successful chord.
    last_aborted_ts: i64,
    /// Number of chars in the buffer when it last drained to empty via backspace.
    /// Guards against attributing a short accidental BS to a completely unrelated chord.
    last_aborted_len: usize,
    /// High-watermark of buffer length since the last flush or drain — used to measure
    /// how long an aborted attempt was at its longest point.
    word_peak_len: usize,

    // Session tracking.
    session_id: i64,
    session_start: i64,
    session_last_activity: i64,
    session_char_count: i64,
    session_word_count: i64,
    /// Timestamp (ms) of the last session-row write. Throttles the periodic
    /// heartbeat UPDATE so we don't write the row on every single word.
    session_last_write_ts: i64,
    /// `session_word_count` value at the last write, for the every-N-words flush.
    session_last_write_word_count: i64,
}

impl Detector {
    fn new(
        store: Storage,
        settings: Arc<Mutex<Settings>>,
        chord_phrases: Arc<RwLock<HashSet<String>>>,
        device: Arc<Mutex<Option<DeviceInfo>>>,
        coaching_overlay_visible: Arc<AtomicBool>,
        hint_seq: Arc<AtomicI64>,
        chordmap_gen: Arc<AtomicI64>,
        practice_active: Arc<AtomicBool>,
        practice_target: Arc<Mutex<Option<String>>>,
        app: AppHandle,
    ) -> Self {
        Self {
            store,
            settings,
            chord_phrases,
            device,
            coaching_overlay_visible,
            hint_seq,
            latest_hint_id: Arc::new(AtomicI64::new(0)),
            chordmap_gen,
            cached_maps: None,
            cached_maps_device_id: String::new(),
            cached_maps_gen: -1,
            practice_active,
            practice_target,
            app,
            word: String::new(),
            word_start_time: None,
            word_end_time: None,
            chars_since_last_bs: 0,
            avg_char_time_after_last_bs: None,
            max_inter_char_ms: 0.0,
            last_key_was_disallowed: false,
            current_had_correction: false,
            pending_chord: None,
            pending_chord_ts: 0,
            pending_bs_count: 0,
            last_chord_phrase: None,
            last_chord_ts: 0,
            prev_flush_phrase: None,
            prev_flush_ts: 0,
            last_deleted_phrase: None,
            last_deleted_ts: 0,
            empty_buf_bs_count: 0,
            last_aborted_ts: 0,
            last_aborted_len: 0,
            word_peak_len: 0,
            session_id: 0,
            session_start: 0,
            session_last_activity: 0,
            session_char_count: 0,
            session_word_count: 0,
            session_last_write_ts: 0,
            session_last_write_word_count: 0,
        }
    }

    fn cfg(&self) -> Settings {
        self.settings.lock().clone()
    }

    /// Ensure the cached coaching chord maps are fresh, rebuilding from SQL only
    /// when stale (first use, device_id change, or chordmap generation bump). The
    /// maps are phrase-independent so one build serves every manual word in a
    /// session. Callers read `self.cached_maps` after this returns — kept separate
    /// so the field's immutable borrow doesn't collide with `self.store` reads.
    fn ensure_chord_maps(&mut self, device_id: Option<&str>) {
        let id = device_id.unwrap_or("");
        let gen = self.chordmap_gen.load(Ordering::Relaxed);
        let stale = self.cached_maps.is_none()
            || self.cached_maps_device_id != id
            || self.cached_maps_gen != gen;
        if stale {
            self.cached_maps = Some(self.store.build_cached_chord_maps(device_id));
            self.cached_maps_device_id = id.to_string();
            self.cached_maps_gen = gen;
        }
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Returns true if stripping a common English conjugation/inflection suffix from
/// `word` yields a phrase present in `map`. Longest suffixes checked first.
/// `word` must be ASCII (guaranteed by `is_ascii_text` in `flush_and_reset`).
fn arpeggio_base_match(map: &HashSet<String>, word: &str) -> bool {
    const SUFFIXES: &[&str] = &[
        "ing", "ied", "ers", "est", "ies", // 3-char
        "ed", "er", "es", "ly",            // 2-char
        "d", "s",                           // 1-char
    ];
    for suffix in SUFFIXES {
        if let Some(base) = word.strip_suffix(suffix) {
            if base.len() > 1 && map.contains(base) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: coaching hint ids must stay strictly monotonic ACROSS a
    /// detector respawn (stop/start logging). The id source is a shared
    /// AppState atomic, so a fresh `Detector` built against the same sequence
    /// must keep climbing — never resetting to 0. A reset would push new
    /// positions below the `coaching_position` listener's high-water mark,
    /// silently dropping them so the overlay can't appear until the counter
    /// caught back up.
    #[test]
    fn hint_ids_monotonic_across_respawn() {
        let seq = Arc::new(AtomicI64::new(0));

        // Session 1: a few hints fire.
        let s1: Vec<i64> = (0..5).map(|_| next_hint_id(&seq)).collect();
        assert_eq!(s1, vec![1, 2, 3, 4, 5]);

        // Detector respawns (stop/start logging). The Detector's local state is
        // gone, but the SHARED sequence is passed into the new one untouched.
        let seq_after_respawn = Arc::clone(&seq);

        // Session 2: ids continue climbing — they do NOT reset to 1.
        let s2: Vec<i64> = (0..3).map(|_| next_hint_id(&seq_after_respawn)).collect();
        assert_eq!(s2, vec![6, 7, 8]);
        assert!(
            *s2.first().unwrap() > *s1.last().unwrap(),
            "post-respawn ids must exceed pre-respawn ids"
        );
    }
}
