use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::Receiver;
use tauri::Emitter;

use super::{arpeggio_base_match, now_ms};
use crate::types::{KeyEvent, Settings};
use crate::EVT_COACHING_DISMISS;

impl super::Detector {
    /// Main loop: block on the channel up to the idle threshold; on timeout
    /// flush the pending buffer (idle boundary).
    pub(super) fn run(&mut self, rx: Receiver<KeyEvent>, stop: Arc<AtomicBool>) {
        loop {
            if stop.load(Ordering::SeqCst) {
                self.flush();
                self.close_session();
                return;
            }
            // Read just the one field we need (don't clone the whole Settings,
            // incl. its HashSet<char>, every loop iteration). `process()` does its
            // own single `cfg()` acquisition for the rest.
            let idle = self.settings.lock().new_word_threshold_s.max(0.1);
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
        // Debug-only: this fires on EVERY keystroke, so in release builds it would
        // be O(keystrokes) of formatting + log IO — a measurable drag over a long
        // typing session. The [FLUSH]/[COACH] lines below stay (per word/hint).
        #[cfg(debug_assertions)]
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

            // A single backspace deleting a TRAILING whitespace/punctuation char
            // is a punctuation edit, not a correction of the word. The common
            // case: the space→period fixup ("word " → "word.") emits a BS to
            // remove the auto-space. If we wiped timing + flagged a correction
            // here, a fully-chorded sentence-final word ("reason." etc.) would
            // lose its chord timing and flush as "manual". Detect this by the
            // char about to be removed and skip the resets below.
            let deleting_disallowed = !word_del
                && self
                    .word
                    .chars()
                    .last()
                    .map(|c| {
                        matches!(c, ' ' | '\t' | '\n' | '\r') || !cfg.allowed_chars.contains(c)
                    })
                    .unwrap_or(false);

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

            // Mark correction if we actually removed a word char from THIS token.
            if was_nonempty && !deleting_disallowed {
                self.current_had_correction = true;
                // Track when an attempt drains completely — used to attribute
                // preceding aborted chord tries to the next successful chord.
                if self.word.is_empty() {
                    self.last_aborted_ts = now_ms();
                    self.last_aborted_len = self.word_peak_len;
                    self.word_peak_len = 0;
                }
            }
            // Preserve completed-chord timing when only trailing punctuation was
            // removed; otherwise reset (a real letter deletion invalidates timing).
            if !deleting_disallowed {
                self.chars_since_last_bs = 0;
                self.avg_char_time_after_last_bs = None;
                self.max_inter_char_ms = 0.0;
            }

            // PRACTICE GATE: suppress ALL ambient error/deletion/confusion signal
            // writes while practice mode is active. These blocks call
            // `self.store.bump_chord_deletion` (an ambient write to chord_errors),
            // so they must not run during a drill — practice leaves ambient stats
            // byte-for-byte unchanged.
            if self.practice_active.load(Ordering::Relaxed) {
                return;
            }

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
                // Track position for the ends-in-space guard but don't update
                // timing stats. A slow trailing disallowed char (e.g. a manual
                // space pressed after the last chord of a session) would pollute
                // avg_char_time_after_last_bs and max_inter_char_ms, causing the
                // idle flush to misclassify a valid chord as manual.
                if is_key {
                    self.word.push_str(key);
                    self.word_peak_len = self.word_peak_len.max(self.word.len());
                    if self.word_start_time.is_none() {
                        self.word_start_time = Some(time_pressed);
                    }
                    self.word_end_time = Some(time_pressed);
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

            // [FLUSH] log line for threshold tuning (one line per flush). Debug
            // builds only — in release this fired a format!+mutex+write on every
            // word, a steady hot-path tax with no user value.
            #[cfg(debug_assertions)]
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

            // PRACTICE GATE: while practice mode is active, FULLY SUPPRESS every
            // ambient write + emit (chord/word logging, wpm samples, error/retype/
            // confusion/split signals, coaching hints, session bookkeeping). On a
            // chord fire we emit ONLY `practice_chord`; a manual flush emits
            // nothing. This is the separation guarantee: a drill leaves ambient
            // stats byte-for-byte unchanged.
            if self.practice_active.load(Ordering::Relaxed) {
                if is_chorded {
                    let target = self.practice_target.lock().clone();
                    let correct = target
                        .as_deref()
                        .map(|t| t.trim().eq_ignore_ascii_case(word.trim()))
                        .unwrap_or(false);
                    let payload = crate::types::PracticeChordEvent {
                        phrase: word.clone(),
                        fire_ms: time_ms as f64,
                        correct,
                    };
                    let _ = self.app.emit(crate::EVT_PRACTICE_CHORD, &payload);
                    crate::logging::log_line(&format!(
                        "[PRACTICE] phrase=\"{}\" fire_ms={} correct={}",
                        word, time_ms, correct
                    ));
                }
                // Reset buffer state and bail — no ambient writes, emits, error
                // tracking, coaching, or session updates run during practice.
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
                // Clear ambient error-tracking carryover so a post-practice flush
                // can't attribute stale pending state to a chord.
                self.pending_chord = None;
                self.last_chord_phrase = None;
                self.last_deleted_phrase = None;
                self.prev_flush_phrase = None;
                return;
            }

            if is_chorded {
                // log_chord returns the post-write (frequency, total_time) so
                // emit_chord doesn't re-read what we just wrote.
                let (freq, total_time) = self
                    .store
                    .log_chord(&word, ts, time_ms, chord_kind)
                    .unwrap_or((0, 0));
                // Stamp mastery on the FIRE path (not the manual gate): a mastered
                // chord is one the user fires, so the manual path rarely runs for
                // it. Conditional + idempotent (WHERE mastered_at IS NULL).
                let _ = self.store.maybe_stamp_mastered(&word, ts);
                self.emit_chord(&word, time_ms, chars, ts, chord_kind, freq, total_time);
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
                // log_word returns the post-write (frequency, total_time) so
                // emit_word doesn't re-read what we just wrote.
                let (freq, total_time) = self
                    .store
                    .log_word(&word, ts, time_ms, clean)
                    .unwrap_or((0, 0));
                // Bump chord_manual so proficiency tracks hand-typed rate even
                // when a chord exists (manual path only, same as before).
                let _ = self.store.bump_chord_manual(&word);
                self.emit_word(&word, time_ms, chars, ts, freq, total_time);

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
}
