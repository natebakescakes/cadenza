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
use std::time::Duration;

use crossbeam_channel::Receiver;
use parking_lot::{Mutex, RwLock};
use tauri::{AppHandle, Emitter};

use crate::storage::Storage;
use crate::types::{ChordRecord, CoachingHint, DeviceInfo, KeyEvent, Settings, WordRecord, WpmSample};
use crate::{
    EVT_CHORD_LOGGED, EVT_COACHING_DISMISS, EVT_COACHING_HINT, EVT_WORD_LOGGED, EVT_WPM,
};

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
    /// Monotonic coaching hint counter; lets a stale `coaching_position` /
    /// clear-timer be ignored once a newer hint has fired.
    hint_id: i64,
    /// Shared snapshot of the latest hint id, read by the detached clear-timer
    /// thread so an older timer never clears the flag after a newer hint fired.
    latest_hint_id: Arc<AtomicI64>,
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
}

impl Detector {
    fn new(
        store: Storage,
        settings: Arc<Mutex<Settings>>,
        chord_phrases: Arc<RwLock<HashSet<String>>>,
        device: Arc<Mutex<Option<DeviceInfo>>>,
        coaching_overlay_visible: Arc<AtomicBool>,
        app: AppHandle,
    ) -> Self {
        Self {
            store,
            settings,
            chord_phrases,
            device,
            coaching_overlay_visible,
            hint_id: 0,
            latest_hint_id: Arc::new(AtomicI64::new(0)),
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

        // Instant-dismiss producer: emit an EMPTY dismiss signal while a coaching
        // overlay is visible, so the frontend can dismiss without us shipping the
        // KeyEvent (PRIVACY: the literal typed char never reaches the overlay
        // webview — it only needs to know "dismiss"). Dismiss timing depends on
        // mode:
        //   - normal:  on the very next keystroke (quick, glance-and-go).
        //   - persist: keep it through the CURRENT word; clear only when the user
        //              starts a DIFFERENT word — i.e. the first word character on
        //              an empty buffer. (Edits/backspaces on the current word and
        //              repeated spaces don't dismiss it.)
        if self.coaching_overlay_visible.load(Ordering::Relaxed) {
            // A "word character" is a single printable, non-whitespace key.
            let is_word_char = is_key
                && key != " "
                && key != "\t"
                && key != "\n"
                && key != "\r"
                && key != "\u{8}"
                && key != "\u{7f}"
                && key.chars().count() == 1;
            let should_dismiss = if cfg.coaching_persist {
                self.word.is_empty() && is_word_char
            } else {
                true
            };
            if should_dismiss {
                crate::logging::log_line(&format!(
                    "[COACH] dismiss persist={} word_empty={} key={:?}",
                    cfg.coaching_persist,
                    self.word.is_empty(),
                    key
                ));
                let _ = self.app.emit(EVT_COACHING_DISMISS, ());
                // In persist mode there is no backend clear-timer, so clear the
                // visibility flag here too (keeps the position listener from
                // surfacing a stale panel and stops further dismiss emits).
                if cfg.coaching_persist {
                    self.coaching_overlay_visible.store(false, Ordering::Relaxed);
                }
            }
        }

        // Per-event trace for debugging avg_ms=? and unexpected flushes.
        // Shows raw key repr, buffer state, and timing counters on every keydown.
        {
            let key_repr = match key {
                "\u{8}" | "\u{7f}" => "BS".to_string(),
                " " => "SPC".to_string(),
                "\n" | "\r" => "RET".to_string(),
                "\t" => "TAB".to_string(),
                "" => format!("EMPTY({})", ev.code),
                s if s.chars().count() > 1 => format!("MULTI({},len={})", ev.code, s.chars().count()),
                s => format!("\"{}\"", s),
            };
            crate::logging::log_line(&format!(
                "[EV] key={} buf_len={} cs={} avg={} mods=ctrl:{}/alt:{}/meta:{}",
                key_repr,
                self.word.len(),
                self.chars_since_last_bs,
                self.avg_char_time_after_last_bs
                    .map(|a| format!("{:.1}", a))
                    .unwrap_or_else(|| "?".to_string()),
                ev.modifiers.ctrl as u8,
                ev.modifiers.alt as u8,
                ev.modifiers.meta as u8,
            ));
        }

        // Backspace / Forward Delete: pop last char (or last word if Option/Alt held on macOS,
        // or Ctrl held on other platforms — mirrors Nexus Freqlog._process_queue logic).
        // \u{8} = BS (standard backspace), \u{7f} = DEL (sometimes used for forward-delete).
        if key == "\u{8}" || key == "\u{7f}" {
            // Option+Backspace (macOS) / Ctrl+Backspace (other) = delete last word in buffer.
            #[cfg(target_os = "macos")]
            let word_del = ev.modifiers.alt;
            #[cfg(not(target_os = "macos"))]
            let word_del = ev.modifiers.ctrl;

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
                // Track when an attempt drains completely — used to attribute
                // preceding aborted chord tries to the next successful chord.
                if self.word.is_empty() {
                    self.last_aborted_ts = now_ms();
                    self.last_aborted_len = self.word_peak_len;
                    self.word_peak_len = 0;
                }
            }
            self.chars_since_last_bs = 0;
            self.avg_char_time_after_last_bs = None;
            self.max_inter_char_ms = 0.0;

            // Chord-error detection via BS-count: count backstrokes after a chord
            // flush. When the count reaches the phrase length within the time window,
            // the user deleted the entire chord output → record an error.
            if let Some(ref candidate) = self.pending_chord.clone() {
                let now = now_ms();
                if now - self.pending_chord_ts < 3_000 {
                    if word_del {
                        // Word-delete shortcut — assume the whole chord was removed.
                        let _ = self.store.bump_chord_deletion(candidate, now);
                        crate::logging::log_line(&format!(
                            "[CHORD_DEL] word-del phrase=\"{}\"",
                            candidate
                        ));
                        self.last_deleted_phrase = Some(candidate.clone());
                        self.last_deleted_ts = now;
                        self.pending_chord = None;
                        self.last_chord_phrase = None;
                    } else {
                        self.pending_bs_count += 1;
                        if self.pending_bs_count >= candidate.chars().count() as i64 {
                            let _ = self.store.bump_chord_deletion(candidate, now);
                            crate::logging::log_line(&format!(
                                "[CHORD_DEL] bs-count phrase=\"{}\" count={}",
                                candidate, self.pending_bs_count
                            ));
                            self.last_deleted_phrase = Some(candidate.clone());
                            self.last_deleted_ts = now;
                            self.pending_chord = None;
                            // Clear retype tracker too — error already logged.
                            self.last_chord_phrase = None;
                        }
                    }
                } else {
                    // Time window expired — intentional edit, not a chord error.
                    self.pending_chord = None;
                }
            }

            // Fallback "quickfix" detection: any BS on empty buffer within 1.5s of last chord.
            // CharaChorder quickfix arrives within milliseconds; 1.5s is tight enough to avoid
            // false positives from incidental BSes but covers re-output-then-BS sequences where
            // some BSes are consumed by partially re-output chars before hitting empty.
            if self.word.is_empty() {
                let now = now_ms();
                if let Some(ref candidate) = self.last_chord_phrase.clone() {
                    if now - self.last_chord_ts < 1_500 {
                        self.empty_buf_bs_count += 1;
                        let _ = self.store.bump_chord_deletion(candidate, now);
                        self.last_deleted_phrase = Some(candidate.clone());
                        self.last_deleted_ts = now;
                        crate::logging::log_line(&format!(
                            "[CHORD_DEL] quickfix phrase=\"{}\" empty-bs-count={} gap_ms={}",
                            candidate, self.empty_buf_bs_count, now - self.last_chord_ts
                        ));
                        self.last_chord_phrase = None;
                        self.empty_buf_bs_count = 0;
                    } else {
                        self.empty_buf_bs_count = 0;
                    }
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
        // If the buffer already has content, the user is typing new text after the
        // chord — they've accepted the chord output, so stop tracking pending errors.
        if !self.word.is_empty() {
            self.pending_chord = None;
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
        self.word_peak_len = self.word_peak_len.max(self.word.len());
        if self.word_start_time.is_none() {
            self.word_start_time = Some(time_pressed);
        } else if self.chars_since_last_bs > 0 {
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

            let has_timing = self.avg_char_time_after_last_bs.is_some();
            // unwrap_or(f64::MAX) means "no timing data → treat as very slow → not a chord burst".
            let avg_ms = self.avg_char_time_after_last_bs.unwrap_or(f64::MAX);
            let max_ms = self.max_inter_char_ms;

            // Check device chordmap (normalized: lowercase+trim already applied).
            // Also check suffix-stripped base forms for arpeggio conjugation detection
            // (e.g. "created" → base "create" in chordmap).
            let (in_chordmap, in_chordmap_base) = {
                let map = self.chord_phrases.read();
                let direct = map.contains(&word);
                let base = direct || arpeggio_base_match(&map, &word);
                (direct, base)
            };

            // Four-way classification:
            // 1. avg < chord_char_threshold_ms              → simultaneous burst ("chord").
            // 2. has_timing && in_chordmap_base && !burst && max < arpeggio_threshold
            //                                               → sequential arpeggio ("arpeggio").
            // 3. !has_timing && in_chordmap_base && !correction
            //                                               → inferred chorded: timing
            //    unavailable (chars arrived via disallowed/leading-space path, bypassing
            //    chars_since_last_bs increment), but chordmap match is high-confidence.
            // 4. otherwise → manual.
            let chord_by_timing = avg_ms < cfg.chord_char_threshold_ms;
            let arpeggio = has_timing
                && in_chordmap_base
                && !chord_by_timing
                && max_ms < cfg.arpeggio_threshold_ms;
            let inferred = !has_timing && in_chordmap_base && !self.current_had_correction;
            let is_chorded = chord_by_timing || arpeggio || inferred;
            let chord_kind = if arpeggio || (inferred && !in_chordmap) {
                "arpeggio"
            } else {
                "chord"
            };

            // [FLUSH] log line for threshold tuning (one line per flush).
            crate::logging::log_line(&format!(
                "[FLUSH] phrase=\"{}\" chars={} avg_ms={} max_ms={:.1} in_chordmap={} in_base={} class={} kind={}",
                word,
                word.chars().count(),
                if has_timing { format!("{:.1}", avg_ms) } else { "?".to_string() },
                max_ms,
                in_chordmap,
                in_chordmap_base,
                if is_chorded { "chorded" } else { "manual" },
                if is_chorded { chord_kind } else { "-" },
            ));

            if is_chorded {
                let _ = self.store.log_chord(&word, ts, time_ms, chord_kind);
                // Stamp mastery on the FIRE path (not the manual gate): a mastered
                // chord is one the user fires, so the manual path rarely runs for
                // it. Conditional + idempotent (WHERE mastered_at IS NULL).
                let _ = self.store.maybe_stamp_mastered(&word, ts);
                self.emit_chord(&word, time_ms, chars, ts, chord_kind);
                // Aborted-attempt signal: chord fired within 3s of a buffer that drained
                // to empty via BS, AND the aborted buffer peaked at ≥3 chars (guards against
                // attributing a short accidental BS to an unrelated short chord like "it").
                if self.last_aborted_ts > 0
                    && ts - self.last_aborted_ts < 3_000
                    && self.last_aborted_len >= 3
                {
                    let _ = self.store.bump_chord_deletion(&word, ts);
                    crate::logging::log_line(&format!(
                        "[CHORD_RETRY] phrase=\"{}\" gap_ms={}",
                        word, ts - self.last_aborted_ts
                    ));
                }
                self.last_aborted_ts = 0;
                // Set pending state for error detection (both mechanisms persist across flush).
                self.pending_chord = Some(word.clone());
                self.pending_chord_ts = ts;
                self.pending_bs_count = 0;
                self.last_chord_phrase = Some(word.clone());
                self.last_chord_ts = ts;
                // Chord confusion: chord fired shortly after deleting a different chord
                // → user likely confused two similar chords.
                if let Some(ref deleted) = self.last_deleted_phrase.take() {
                    if ts - self.last_deleted_ts < cfg.chord_confusion_window_ms as i64 && word != *deleted {
                        let _ = self.store.bump_chord_confusion(deleted, ts);
                        crate::logging::log_line(&format!(
                            "[CHORD_CONFUSION] deleted=\"{}\" new=\"{}\" gap_ms={}",
                            deleted, word, ts - self.last_deleted_ts
                        ));
                    }
                }
            } else {
                // Re-type signal: same phrase typed manually within 5s of a chord flush
                // → the chord likely misfired and the user corrected by retyping.
                if let Some(ref last) = self.last_chord_phrase.clone() {
                    if *last == word && ts - self.last_chord_ts < 5_000 {
                        let _ = self.store.bump_chord_error(&word, ts);
                        crate::logging::log_line(&format!(
                            "[CHORD_ERROR] retype phrase=\"{}\" gap_ms={}",
                            word,
                            ts - self.last_chord_ts
                        ));
                        self.last_chord_phrase = None;
                    }
                }
                let clean = !self.current_had_correction;
                let _ = self.store.log_word(&word, ts, time_ms, clean);
                // Bump chord_manual so proficiency tracks hand-typed rate even
                // when a chord exists (manual path only, same as before).
                let _ = self.store.bump_chord_manual(&word);
                self.emit_word(&word, time_ms, chars, ts);

                // Coaching overlay: on a manual word, look up its mapping and, if
                // the gate passes, fire the coaching hint + schedule the async
                // (Phase 2) caret locate. Non-blocking; never stalls the Detector.
                self.maybe_emit_coaching(&word, &cfg);

                // Split-word detection: consecutive manual flushes < 3s apart whose
                // concat is a known word or chord phrase → candidate for a new chord.
                if let Some(ref prev) = self.prev_flush_phrase.clone() {
                    if ts - self.prev_flush_ts < 3_000 {
                        let concat = format!("{}{}", prev, word);
                        let is_known_word = self.store.scalar_i64(
                            "SELECT COALESCE(frequency,0) FROM words WHERE LOWER(word)=LOWER(?1)",
                            &concat,
                        ) > 0;
                        let chord_phrases = self.chord_phrases.read();
                        let is_chord_phrase = chord_phrases.contains(&concat.to_lowercase());
                        let concat_spaced = format!("{} {}", prev, word);
                        let is_chord_phrase_spaced =
                            chord_phrases.contains(&concat_spaced.to_lowercase());
                        drop(chord_phrases);
                        if is_known_word || is_chord_phrase || is_chord_phrase_spaced {
                            let logged = if is_chord_phrase_spaced { &concat_spaced } else { &concat };
                            let _ = self.store.bump_split_phrase(logged, ts);
                            crate::logging::log_line(&format!(
                                "[SPLIT] \"{}\" + \"{}\" = \"{}\" gap_ms={}",
                                prev, word, logged, ts - self.prev_flush_ts
                            ));
                        }
                    }
                }
                self.prev_flush_phrase = Some(word.clone());
                self.prev_flush_ts = ts;
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
        self.empty_buf_bs_count = 0;
        self.last_aborted_ts = 0;
        self.word_peak_len = 0;
        // Note: pending_chord intentionally NOT cleared here — it must persist
        // after flush so the BS-count error detector can fire on the next BS event.
    }

    /// On a manual flush: resolve the coaching mapping + gate it, and if shown,
    /// emit `coaching_hint` immediately (fire-and-forget) then schedule the
    /// main-thread AX locate (Phase 2 stub) via GCD. Also arms the gated
    /// `EVT_KEYSTROKE` producer + a backend self-clearing timer.
    fn maybe_emit_coaching(&mut self, phrase: &str, cfg: &Settings) {
        if !cfg.coaching_enabled {
            return;
        }
        // Resolve device_id LIVE from shared state; clone to an owned Option and
        // DROP the guard before any dispatch.
        let device_id: Option<String> = {
            let guard = self.device.lock();
            guard.as_ref().map(|d| format!("{}-{}", d.name, d.version))
        };

        let mapping = match self.store.coaching_mapping(phrase, device_id.as_deref()) {
            Some(m) => m,
            None => return,
        };
        if !self
            .store
            .coaching_should_show(phrase, &mapping.source, cfg)
        {
            return;
        }

        // Bump the monotonic hint id and emit the hint immediately.
        self.hint_id += 1;
        let id = self.hint_id;
        let hint = CoachingHint {
            id,
            phrase: phrase.to_string(),
            primary_combo: mapping.primary,
            alt_count: mapping.alt_count,
            source: mapping.source,
            combos: mapping.combos,
            persist: cfg.coaching_persist,
            show_ms: cfg.coaching_show_ms,
            fade_ms: cfg.coaching_fade_ms,
        };
        self.coaching_overlay_visible.store(true, Ordering::Relaxed);
        crate::logging::log_line(&format!(
            "[COACH] show id={} phrase=\"{}\" source={} persist={}",
            id, phrase, hint.source, cfg.coaching_persist
        ));
        let _ = self.app.emit(EVT_COACHING_HINT, &hint);

        // Schedule the async caret locate on the main thread (where AX is legal).
        // Fire-and-forget: the Detector never awaits it. locate_caret runs the
        // tiered AX locator and returns None if no caret rect can be resolved.
        #[cfg(target_os = "macos")]
        {
            let app = self.app.clone();
            dispatch2::DispatchQueue::main().exec_async(move || {
                if let Some(hit) = crate::coaching::locate_caret() {
                    let pos = crate::types::CoachingPosition {
                        id,
                        rect: hit.rect,
                        centered: hit.centered,
                    };
                    let _ = app.emit(crate::EVT_COACHING_POSITION, &pos);
                }
            });
        }

        // Track the latest hint id so an in-flight timer can tell if a newer hint
        // superseded it.
        let captured_id = id;
        self.latest_hint_id.store(captured_id, Ordering::Relaxed);

        // Persist mode: no auto-dismiss. The overlay stays until the NEXT hint
        // replaces it (the frontend also skips its fade + keystroke-dismiss).
        if cfg.coaching_persist {
            return;
        }

        // Backend self-clearing timer (authoritative floor): clear the visible
        // flag after show+fade UNLESS a newer hint has fired in the meantime.
        // Uses tauri's async runtime + a tokio sleep instead of spawning a fresh
        // OS thread per hint (unbounded thread growth under fast typing).
        let flag = self.coaching_overlay_visible.clone();
        let dur_ms = (cfg.coaching_show_ms + cfg.coaching_fade_ms).max(0.0);
        let latest = self.latest_hint_id.clone();
        let timer_app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(dur_ms as u64)).await;
            // Only clear if no newer hint fired while we slept.
            if latest.load(Ordering::Relaxed) == captured_id {
                flag.store(false, Ordering::Relaxed);
                // Authoritative floor: tell the overlay to dismiss too. The
                // frontend's dismiss handler hides the React content AND calls
                // `hide_overlay`, so the NSPanel can't linger as an empty panel.
                // Idempotent: a frontend-driven hide may have already fired.
                let _ = timer_app.emit(EVT_COACHING_DISMISS, ());
            }
        });
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

    fn emit_chord(&self, phrase: &str, time_ms: i64, chars: f64, ts: i64, kind: &str) {
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
            kind: kind.to_string(),
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

        // Write stats for sketchybar widget (atomic tmp→rename, same FS).
        let json = format!("{{\"wpm\":{rolling:.1}}}\n");
        let data_dir = Storage::data_dir();
        let tmp = data_dir.join("sketchybar.json.tmp");
        let dest = data_dir.join("sketchybar.json");
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, &dest);
        }
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
