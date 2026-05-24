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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::Receiver;
use parking_lot::{Mutex, RwLock};
use tauri::{AppHandle, Emitter};

use crate::storage::Storage;
use crate::types::{ChordRecord, KeyEvent, Settings, WordRecord, WpmSample};
use crate::{EVT_CHORD_LOGGED, EVT_WORD_LOGGED, EVT_WPM};

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
            let mut det = Detector::new(store, settings, chord_phrases, app);
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
    /// If the current buffer content looks like a chord candidate (in_chordmap +
    /// fast timing), we record the phrase here so we can detect a "fired then
    /// deleted" error when backspace removes it before flush.
    chord_candidate: Option<String>,

    // Session tracking.
    session_id: i64,
    session_start: i64,
    session_last_activity: i64,
    session_char_count: i64,
    session_word_count: i64,
}

impl Detector {
    fn new(
        store: Storage,
        settings: Arc<Mutex<Settings>>,
        chord_phrases: Arc<RwLock<HashSet<String>>>,
        app: AppHandle,
    ) -> Self {
        Self {
            store,
            settings,
            chord_phrases,
            app,
            word: String::new(),
            word_start_time: None,
            word_end_time: None,
            chars_since_last_bs: 0,
            avg_char_time_after_last_bs: None,
            max_inter_char_ms: 0.0,
            last_key_was_disallowed: false,
            current_had_correction: false,
            chord_candidate: None,
            session_id: 0,
            session_start: 0,
            session_last_activity: 0,
            session_char_count: 0,
            session_word_count: 0,
        }
    }

    fn cfg(&self) -> Settings {
        self.settings.lock().clone()
    }

    /// Main loop: block on the channel up to the idle threshold; on timeout
    /// flush the pending buffer (idle boundary).
    fn run(&mut self, rx: Receiver<KeyEvent>, stop: Arc<AtomicBool>) {
        loop {
            if stop.load(Ordering::SeqCst) {
                self.flush();
                self.close_session();
                return;
            }
            let idle = self.cfg().new_word_threshold_s.max(0.1);
            match rx.recv_timeout(Duration::from_secs_f64(idle)) {
                Ok(ev) => self.process(&ev),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // Idle longer than threshold => flush current buffer.
                    if !self.word.is_empty() {
                        self.flush();
                    }
                    self.maybe_close_session(now_ms());
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    self.flush();
                    self.close_session();
                    return;
                }
            }
        }
    }

    /// Feed one key event into the buffer + classify/flush (port of `_process_queue` body).
    fn process(&mut self, ev: &KeyEvent) {
        // Only act on key presses.
        if !ev.pressed {
            return;
        }
        let cfg = self.cfg();
        let key = ev.key.as_str();
        let time_pressed = ev.ts_ms;

        let is_key = !key.is_empty();

        // Backspace / Forward Delete: pop last char (or last word if Option/Alt held on macOS,
        // or Ctrl held on other platforms — mirrors Nexus Freqlog._process_queue logic).
        // \u{8} = BS (standard backspace), \u{7f} = DEL (sometimes used for forward-delete).
        if key == "\u{8}" || key == "\u{7f}" {
            // Option+Backspace (macOS) / Ctrl+Backspace (other) = delete last word in buffer.
            #[cfg(target_os = "macos")]
            let word_del = ev.modifiers.alt;
            #[cfg(not(target_os = "macos"))]
            let word_del = ev.modifiers.ctrl;

            // Capture the pre-deletion word for chord-error detection below.
            let pre_bs_word = self.word.clone();
            let was_nonempty = !self.word.is_empty();

            if word_del {
                // Remove everything back to the last whitespace boundary.
                if let Some(pos) = self.word.rfind(|c: char| c == ' ' || c == '\t' || c == '\n') {
                    self.word.truncate(pos);
                } else {
                    self.word.clear();
                }
            } else if !self.word.is_empty() {
                self.word.pop();
            }

            // Mark correction if we actually removed something from THIS token.
            if was_nonempty {
                self.current_had_correction = true;
            }
            self.chars_since_last_bs = 0;
            self.avg_char_time_after_last_bs = None;
            self.max_inter_char_ms = 0.0;

            // Chord-error detection: if the buffer just before this backspace
            // matched a chord candidate (in_chordmap + was the exact candidate
            // phrase we were tracking), record a chord error.
            if let Some(ref candidate) = self.chord_candidate.clone() {
                // Normalize what was in the buffer before deletion.
                let pre_norm: String = pre_bs_word
                    .trim()
                    .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-')
                    .to_lowercase();
                // An error is triggered when the candidate phrase was fully
                // present in the buffer and backspace(s) are now removing it.
                // We detect this by checking that the pre-bs word contained the
                // candidate and the post-bs word no longer does.
                let post_norm: String = self.word
                    .trim()
                    .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-')
                    .to_lowercase();
                if pre_norm == *candidate && post_norm != *candidate {
                    let ts = now_ms();
                    let _ = self.store.bump_chord_error(candidate, ts);
                    crate::logging::log_line(&format!(
                        "[CHORD_ERROR] phrase=\"{}\" ts={}",
                        candidate, ts
                    ));
                    self.chord_candidate = None;
                }
            }

            return;
        }

        // Whitespace or disallowed char.
        let is_whitespace = matches!(key, " " | "\t" | "\n" | "\r");
        let is_allowed = is_key && key.chars().count() == 1 && {
            let c = key.chars().next().unwrap();
            cfg.allowed_chars.contains(c)
        };

        if is_key && (is_whitespace || !is_allowed) {
            if !self.word.is_empty()
                && self
                    .avg_char_time_after_last_bs
                    .map(|a| a > cfg.chord_char_threshold_ms)
                    .unwrap_or(false)
            {
                // Clear human-typing boundary => flush.
                self.flush_and_reset(&cfg);
            } else {
                // Otherwise treat as part of an in-progress chord burst.
                if is_key {
                    self.append_char(key, time_pressed);
                }
                self.last_key_was_disallowed = true;
            }
            return;
        }

        // Non-key event (modifier-only / unmapped) => boundary, flush.
        if !is_key {
            if !self.word.is_empty() {
                self.flush_and_reset(&cfg);
            }
            return;
        }

        // A banned modifier (ctrl/alt/meta) means this is a shortcut, not text.
        let banned_modifier = ev.modifiers.ctrl || ev.modifiers.alt || ev.modifiers.meta;
        if banned_modifier {
            if !self.word.is_empty() {
                self.flush_and_reset(&cfg);
            }
            return;
        }

        // Normal allowed char. nexus's "ends-in-space chord" guard:
        if self.last_key_was_disallowed
            && !self.word.is_empty()
            && self
                .word_end_time
                .map(|end| (time_pressed - end) as f64 > cfg.chord_char_threshold_ms)
                .unwrap_or(false)
        {
            self.flush_and_reset(&cfg);
        }
        self.append_char(key, time_pressed);
        self.chars_since_last_bs += 1;
        // Clear the disallowed flag once a normal char is consumed. The
        // "ends-in-space chord" guard above must only fire on the FIRST char
        // after a disallowed/whitespace key — otherwise a leading auto-space
        // (chords often emit one) keeps the flag set and a later >threshold gap
        // splits the first letter off the burst (e.g. "device" → "d" + "evice").
        self.last_key_was_disallowed = false;
    }

    /// Append a char to the buffer and update timing (port of `_update_timing`).
    fn append_char(&mut self, key: &str, time_pressed: i64) {
        self.word.push_str(key);
        if self.word_start_time.is_none() {
            self.word_start_time = Some(time_pressed);
        } else if self.chars_since_last_bs > 1 {
            let end = self.word_end_time.unwrap_or(time_pressed);
            let delta = (time_pressed - end) as f64;
            // Track max gap for arpeggio classification.
            if delta > self.max_inter_char_ms {
                self.max_inter_char_ms = delta;
            }
            self.avg_char_time_after_last_bs = Some(match self.avg_char_time_after_last_bs {
                Some(avg) => {
                    let n = self.chars_since_last_bs as f64;
                    (avg * (n - 1.0) + delta) / n
                }
                None => delta,
            });
        }
        self.word_end_time = Some(time_pressed);
    }

    /// Force-flush the current buffer without resetting derived config.
    fn flush(&mut self) {
        let cfg = self.cfg();
        self.flush_and_reset(&cfg);
    }

    /// Classify + log the buffer (min length 2), emit events, then reset state.
    fn flush_and_reset(&mut self, cfg: &Settings) {
        // Strip leading/trailing punctuation and whitespace, then lowercase.
        // Internal hyphens and apostrophes are kept (contractions, hyphenated words).
        let raw = self.word.trim();
        let word: String = raw
            .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-')
            .to_lowercase();
        // Suppress single-character repeats ("jjjj", "kkk", "llll") — these are
        // almost always held keys or vim motions in normal mode, not typed words.
        let is_char_repeat = {
            let mut cs = word.chars();
            match cs.next() {
                Some(first) => word.chars().count() >= 2 && cs.all(|c| c == first),
                None => false,
            }
        };
        // Reject non-ASCII symbol noise (e.g. macOS Option-key output like
        // "π†∫ß" from ⌥p/⌥t/⌥b/⌥s) — only ASCII letters/digits plus '/- count
        // as real text. Note: this also drops accented words (café) — fine for
        // an English chording workflow.
        let is_ascii_text = word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '\'' || c == '-');
        if word.chars().count() >= 2
            && !is_char_repeat
            && is_ascii_text
            && !self.store.is_banned(&word)
        {
            let start = self.word_start_time.unwrap_or(0);
            let end = self.word_end_time.unwrap_or(start);
            let time_ms = (end - start).max(0);
            let chars = word.chars().count() as f64;
            let ts = now_ms();

            let avg_ms = self.avg_char_time_after_last_bs.unwrap_or(0.0);
            let max_ms = self.max_inter_char_ms;

            // Check device chordmap (normalized: lowercase+trim already applied).
            let in_chordmap = self.chord_phrases.read().contains(&word);

            // Three-way classification:
            // 1. avg < chord_char_threshold_ms  → pure simultaneous chord burst.
            // 2. in_chordmap && max < arpeggio_threshold_ms → arpeggio/compound chord.
            // 3. otherwise → manual (hand-typed); bump chord_manual if in_chordmap.
            let is_chorded = avg_ms < cfg.chord_char_threshold_ms
                || (in_chordmap && max_ms < cfg.arpeggio_threshold_ms);

            // Track the chord candidate so that a subsequent backspace before
            // the next flush can be identified as a chord error.
            if is_chorded && in_chordmap {
                self.chord_candidate = Some(word.clone());
            } else {
                self.chord_candidate = None;
            }

            // [FLUSH] log line for threshold tuning (one line per flush).
            crate::logging::log_line(&format!(
                "[FLUSH] phrase=\"{}\" chars={} avg_ms={:.1} max_ms={:.1} in_chordmap={} class={}",
                word,
                word.chars().count(),
                avg_ms,
                max_ms,
                in_chordmap,
                if is_chorded { "chorded" } else { "manual" },
            ));

            if is_chorded {
                let _ = self.store.log_chord(&word, ts, time_ms);
                self.emit_chord(&word, time_ms, chars, ts);
                // Chord path: correction flag doesn't apply (chords have error_rate instead).
            } else {
                let clean = !self.current_had_correction;
                let _ = self.store.log_word(&word, ts, time_ms, clean);
                // Bump chord_manual so proficiency tracks hand-typed rate even
                // when a chord exists (manual path only, same as before).
                let _ = self.store.bump_chord_manual(&word);
                self.emit_word(&word, time_ms, chars, ts);
            }

            // Session bookkeeping.
            self.update_session(ts, word.chars().count() as i64);
        }

        self.word.clear();
        self.word_start_time = None;
        self.word_end_time = None;
        self.chars_since_last_bs = 0;
        self.avg_char_time_after_last_bs = None;
        self.max_inter_char_ms = 0.0;
        self.last_key_was_disallowed = false;
        self.current_had_correction = false;
        self.chord_candidate = None;
    }

    fn emit_word(&self, word: &str, time_ms: i64, chars: f64, ts: i64) {
        let freq = self.lookup_freq("words", "word", word);
        let clean = self.store.scalar_i64(
            "SELECT COALESCE(clean_count,0) FROM words WHERE word = ?1",
            word,
        );
        let rec = WordRecord {
            word: word.to_string(),
            frequency: freq,
            last_used: ts,
            avg_speed_ms: if freq > 0 {
                self.total_time("words", "word", word) as f64 / freq as f64
            } else {
                time_ms as f64
            },
            score: word.chars().count() as i64 * freq,
            accuracy_rate: if freq > 0 { clean as f64 / freq as f64 } else { 1.0 },
        };
        let _ = self.app.emit(EVT_WORD_LOGGED, &rec);
        self.emit_wpm(time_ms, chars, ts, "manual");
    }

    fn emit_chord(&self, phrase: &str, time_ms: i64, chars: f64, ts: i64) {
        let freq = self.lookup_freq("chords", "phrase", phrase);
        let rec = ChordRecord {
            phrase: phrase.to_string(),
            frequency: freq,
            last_used: ts,
            avg_speed_ms: if freq > 0 {
                self.total_time("chords", "phrase", phrase) as f64 / freq as f64
            } else {
                time_ms as f64
            },
        };
        let _ = self.app.emit(EVT_CHORD_LOGGED, &rec);
        self.emit_wpm(time_ms, chars, ts, "chorded");
    }

    /// Record a logged unit (its real character count + flush time + source) and
    /// emit the live `wpm` event carrying the trailing-60s rolling speed computed
    /// from raw units. WPM is computed at query time, not from a per-burst rate.
    fn emit_wpm(&self, _time_ms: i64, chars: f64, ts: i64, source: &str) {
        if chars < 1.0 {
            return;
        }
        let _ = self.store.add_wpm_sample(ts, chars as i64, source);

        // Live number: rolling WPM over the trailing 60s wall-clock window.
        let rolling = self.store.rolling_wpm(ts);
        let sample = WpmSample {
            t: ts,
            wpm: rolling,
            source: "rolling".to_string(),
        };
        let _ = self.app.emit(EVT_WPM, &sample);
    }

    fn lookup_freq(&self, table: &str, col: &str, key: &str) -> i64 {
        self.store.scalar_i64(
            &format!("SELECT frequency FROM {table} WHERE {col} = ?1"),
            key,
        )
    }

    fn total_time(&self, table: &str, col: &str, key: &str) -> i64 {
        self.store.scalar_i64(
            &format!("SELECT total_time_ms FROM {table} WHERE {col} = ?1"),
            key,
        )
    }

    // --- Session tracking -------------------------------------------------

    fn update_session(&mut self, ts: i64, chars: i64) {
        if self.session_id == 0 {
            self.session_start = ts;
            self.session_char_count = 0;
            self.session_word_count = 0;
            self.session_id = self
                .store
                .upsert_session(0, ts, ts, 0, 0)
                .unwrap_or(0);
        }
        self.session_last_activity = ts;
        self.session_char_count += chars;
        self.session_word_count += 1;
        let _ = self.store.upsert_session(
            self.session_id,
            self.session_start,
            ts,
            self.session_char_count,
            self.session_word_count,
        );
    }

    /// Close the session if idle gap exceeds the new-word threshold.
    fn maybe_close_session(&mut self, now: i64) {
        if self.session_id == 0 {
            return;
        }
        let gap_ms = (self.cfg().new_word_threshold_s * 1000.0) as i64;
        if now - self.session_last_activity >= gap_ms {
            self.close_session();
        }
    }

    fn close_session(&mut self) {
        if self.session_id != 0 {
            let _ = self.store.upsert_session(
                self.session_id,
                self.session_start,
                self.session_last_activity,
                self.session_char_count,
                self.session_word_count,
            );
        }
        self.session_id = 0;
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
