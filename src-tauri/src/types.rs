// Shared serde types for the Cadenza backend <-> frontend contract.
// These mirror `src/lib/types.ts` exactly. Keep them in sync.

use serde::{Deserialize, Serialize};

/// Keyboard modifier state captured alongside a key event.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

/// A single global key event captured by the keylogger.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KeyEvent {
    pub code: String,
    pub key: String,
    pub pressed: bool,
    pub modifiers: Modifiers,
    pub ts_ms: i64,
}

/// A manually-typed word the engine has logged.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WordRecord {
    pub word: String,
    pub frequency: i64,
    pub last_used: i64,
    pub avg_speed_ms: f64,
    pub score: i64,
    /// clean_count / frequency — fraction of occurrences typed with no corrections.
    pub accuracy_rate: f64,
}

/// A phrase that was fired as a chord by the device.
/// `kind` is "chord" (simultaneous burst, avg < chord_char_threshold_ms) or
/// "arpeggio" (in-chordmap sequential burst, max < arpeggio_threshold_ms).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChordRecord {
    pub phrase: String,
    pub frequency: i64,
    pub last_used: i64,
    pub avg_speed_ms: f64,
    pub kind: String,
}

/// A single WPM data point. `source` is one of: "overall" | "chorded" | "manual".
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WpmSample {
    pub t: i64,
    pub wpm: f64,
    pub source: String,
}

/// Aggregated WPM figures for the summary header.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct WpmSummary {
    pub rolling: f64,
    pub session: f64,
    pub overall: f64,
    pub chorded: f64,
    pub manual: f64,
}

/// One chord option for a suggestion — either a single chord or a compound sequence.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChordCombo {
    /// "chord" for a single simultaneous chord, "compound" for a sequence of chords.
    pub kind: String,
    /// For "chord": one element, the combo string e.g. "a + h + t".
    /// For "compound": one element per part e.g. ["h + i + s", "a + l"].
    pub parts: Vec<String>,
    /// Phrases of existing device chords whose key combination matches this combo
    /// (only populated for kind="chord"; compound parts are each checked separately).
    pub conflicts: Vec<String>,
}

/// A chord suggestion: a frequent phrase worth chording.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Suggestion {
    pub phrase: String,
    pub frequency: i64,
    pub score: i64,
    pub avg_manual_ms: f64,
    pub projected_saving_ms: f64,
    pub combos: Vec<ChordCombo>,
}

/// Proficiency stats for an existing chord.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Proficiency {
    pub phrase: String,
    pub usage_rate: f64,
    pub fired_count: i64,
    pub manual_count: i64,
    pub avg_fire_ms: f64,
    pub consistency: f64,
    pub mastered: bool,
    /// High-confidence errors: chord fired then user manually retyped same phrase within 5s.
    pub error_count: i64,
    /// error_count / (fired_count + error_count); 0.0 if never errored.
    pub error_rate: f64,
    /// Lower-confidence: chord fired then deleted (BS-count >= phrase length within 3s).
    /// May include intentional edits; useful as a secondary signal alongside error_count.
    pub deletion_count: i64,
    /// deletion_count / (fired_count + deletion_count); 0.0 if never deleted.
    pub deletion_rate: f64,
    /// Chord deleted then a different chord fired within the confusion window.
    /// Indicates the user confused this chord with another.
    pub confusion_count: i64,
    /// confusion_count / (fired_count + confusion_count); 0.0 if never confused.
    pub confusion_rate: f64,
    /// Human-readable key combinations for this chord, one string per
    /// device_chords row (a phrase may have multiple chord mappings).
    /// E.g. ["p + t"] for a chord whose actions decode to the keys 'p' and 't'.
    /// Empty when no device_chords row exists for the phrase.
    pub combos: Vec<String>,
}

/// One candidate chord combo for a coaching hint, with any conflicts.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CoachingCombo {
    /// Display string for the combo, e.g. "w + o" or "h + i + s → a + l".
    pub combo: String,
    /// Existing device-chord phrases whose key combination already matches this
    /// combo (i.e. the combo is "occupied"). Empty when unconflicted.
    pub conflicts: Vec<String>,
    /// When this combo is occupied, the holder word whose chord we'd suggest
    /// REASSIGNING to the current word (the weakest-used holder). `None` when the
    /// combo is free or no swap applies. Recommend-only: the app never writes to
    /// the device — this is advice for manual remapping.
    #[serde(default)]
    pub swap_target: Option<String>,
    /// Human-readable rationale for the swap, e.g.
    /// `you type "race" 12× · "rce" chord fires 1×`. Pairs with `swap_target`.
    #[serde(default)]
    pub swap_reason: Option<String>,
}

/// Coaching overlay hint, emitted immediately on `manual` classification.
/// Carries NO coordinates; `id` is a monotonic counter so a stale
/// `CoachingPosition` can be ignored if a newer hint has fired.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CoachingHint {
    pub id: i64,
    pub phrase: String,
    /// Primary combo display string (= `combos[0].combo`); kept for convenience.
    pub primary_combo: String,
    /// Number of ADDITIONAL combos beyond the primary (= `combos.len() - 1`).
    pub alt_count: i64,
    /// "device" | "suggested"
    pub source: String,
    /// All candidate combos (primary first), each with its conflicts. For
    /// "device" these are the existing mappings (no conflicts); for "suggested"
    /// these are generated options, some of which may collide with existing
    /// chords (see each entry's `conflicts`).
    pub combos: Vec<CoachingCombo>,
    /// Live settings snapshot at emit time, so the overlay webview (a long-lived
    /// separate window that can't see later Settings edits) always reflects the
    /// CURRENT values rather than whatever was read when it first mounted.
    pub persist: bool,
    pub show_ms: f64,
    pub fade_ms: f64,
}

/// A screen rectangle in Tauri logical (NS, top-left origin) coords.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Coaching overlay caret position, emitted by the main-thread AX closure once
/// it resolves the focused element rect.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CoachingPosition {
    pub id: i64,
    pub rect: ScreenRect,
    /// True when `rect` is a screen-centre fallback (no real caret/field found,
    /// e.g. Ghostty) — the panel is centred horizontally rather than left-anchored.
    pub centered: bool,
    /// `Some(app_name)` when the focused app is a Chromium browser with "Text
    /// Metrics" accessibility disabled (no caret geometry available). The overlay
    /// shows a prompt to enable it. `None` when a real caret was resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_metrics_app: Option<String>,
}

/// Information about a connected CharaChorder device.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeviceInfo {
    pub name: String,
    pub company: String,
    pub device: String,
    pub chipset: String,
    pub version: String,
    pub port: String,
    pub chord_count: i64,
}

/// A serial port discovered during a scan.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SerialPortInfo {
    pub port: String,
    pub name: String,
}

/// User-tunable detection settings.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub new_word_threshold_s: f64,
    pub chord_char_threshold_ms: f64,
    pub allowed_chars: String,
    /// Max ms between ANY two consecutive chars for a known-chordmap phrase to
    /// still be classified as chorded (arpeggio / compound chord gate).
    pub arpeggio_threshold_ms: f64,
    /// When true, chord_char_threshold_ms and arpeggio_threshold_ms are
    /// automatically re-derived from device settings on connect/refresh.
    /// Flips to false the moment the user manually edits either threshold,
    /// preventing auto-overwrite of their custom values.
    pub thresholds_auto: bool,
    /// Time window (ms) after a chord deletion within which firing a different
    /// chord is logged as a [CHORD_CONFUSION] event.
    pub chord_confusion_window_ms: f64,
    /// Master toggle for the real-time chord coaching overlay.
    pub coaching_enabled: bool,
    /// How long (ms) the coaching overlay stays fully visible before fading.
    pub coaching_show_ms: f64,
    /// Fade-out duration (ms) of the coaching overlay.
    pub coaching_fade_ms: f64,
    /// Minimum manual `words.frequency` before a SUGGESTED (chordless) combo
    /// is shown by the overlay.
    pub coaching_suggest_min_count: i64,
    /// Minimum phrase length (chars) before ANY coaching hint is shown (both
    /// suggested combos and device-chord reminders). Suppresses noise from very
    /// short tokens — notably 2-letter Mouseless grid labels — which barely
    /// benefit from a chord and collide with overlay navigation.
    pub coaching_suggest_min_len: i64,
    /// A previously-mastered chord whose usage_rate drops below this value is
    /// re-surfaced (reminded again).
    pub coaching_resurface_rate: f64,
    /// When true, the overlay stays visible until the next hint replaces it —
    /// no auto-fade timer and no dismiss-on-keystroke. Useful for inspecting
    /// placement/content; leave off for normal use.
    pub coaching_persist: bool,
    /// When true, suppress reminders for chords you've already mastered. Default
    /// false: show for every manually-typed chord (turn this on to reduce noise
    /// once the overlay is working as expected).
    pub coaching_hide_mastered: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            new_word_threshold_s: 5.0,
            chord_char_threshold_ms: 5.0,
            allowed_chars:
                "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'-".to_string(),
            arpeggio_threshold_ms: 15.0,
            thresholds_auto: true,
            chord_confusion_window_ms: 5_000.0,
            coaching_enabled: true,
            coaching_show_ms: 1500.0,
            coaching_fade_ms: 300.0,
            coaching_suggest_min_count: 1,
            coaching_suggest_min_len: 3,
            coaching_resurface_rate: 0.6,
            coaching_persist: true,
            coaching_hide_mastered: false,
        }
    }
}

/// Raw device settings read via VAR B1 queries, cached in AppState.
/// Fields are -1 when the query failed (device didn't respond or parse error).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DeviceSettings {
    /// Keyboard output delay in µs (id 0x17). Inter-character emission spacing
    /// within a chord burst as seen by the host keylogger.
    pub output_delay_us: i64,
    /// Arpeggiate timeout in ms (id 0x54). Max time to complete an arpeggiate
    /// modifier after the first chord output.
    pub arpeggiate_timeout_ms: i64,
    /// Arpeggiate feature enabled (id 0x51).
    pub arpeggiate_enabled: bool,
    /// Chord press tolerance in ms (id 0x34).
    pub chord_press_tolerance_ms: i64,
    /// Chord release tolerance in ms (id 0x35).
    pub chord_release_tolerance_ms: i64,
    /// Chord auto-delete timeout in ms (id 0x33).
    pub auto_delete_timeout_ms: i64,
    /// Chording enabled (id 0x31).
    pub chording_enabled: bool,
    /// Spurring enabled (id 0x41).
    pub spurring_enabled: bool,
}

/// Current logging / database state, surfaced to the UI.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LoggingState {
    pub logging: bool,
    pub db_unlocked: bool,
}

/// A banned word that should not be logged.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BanlistEntry {
    pub word: String,
    pub added: i64,
}

/// Payload for the `practice_chord` event, emitted on each chord fire WHILE
/// practice mode is active (in place of any ambient write/emit). `correct` is
/// true when the fired phrase matches the active practice target (case-
/// insensitive, trimmed).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PracticeChordEvent {
    pub phrase: String,
    pub fire_ms: f64,
    pub correct: bool,
}

/// A spaced-repetition practice card returned by the practice queue. Carries
/// enough for the UI to render a drill prompt: the phrase, its decoded device
/// combo(s), and the SM-2 card state. `is_new` is true for a weak-chord SEED
/// candidate that has no `practice_cards` row yet (its card fields are SM-2
/// defaults). Practice stats are isolated from ambient stats — these never
/// touch chords/words/wpm_samples.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PracticeCard {
    pub phrase: String,
    /// Human-readable device key combinations for this phrase (one per
    /// device_chords row). Empty when the phrase has no device chord mapping.
    pub combos: Vec<String>,
    pub ease: f64,
    pub interval_days: f64,
    pub due_at: i64,
    pub reps: i64,
    pub lapses: i64,
    pub last_reviewed: i64,
    /// True when this is a freshly-seeded weak chord with no card row yet.
    pub is_new: bool,
}

/// Per-card practice statistics for the detail view.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct PracticeCardStats {
    pub phrase: String,
    pub reps: i64,
    pub lapses: i64,
    pub ease: f64,
    pub interval_days: f64,
    pub due_at: i64,
    /// Mean fire_ms over the most recent attempts for this phrase (0 if none).
    pub recent_avg_fire_ms: f64,
    /// first_try-correct attempts / total attempts (0.0 if no attempts).
    pub first_try_accuracy: f64,
    /// Mean backspaces over the most recent attempts for this phrase (0.0 if none).
    pub avg_backspaces: f64,
    /// Fraction of attempts with no backspaces AND no corrections (0.0 if none).
    pub clean_rate: f64,
    /// Minimum fire_ms among correct attempts (0.0 if none).
    pub best_fire_ms: f64,
    /// Fraction of attempts where a hint was used (0.0 if none).
    pub hint_rate: f64,
}

/// Aggregate practice overview for the hub header.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct PracticeOverview {
    /// Total practice attempts logged across all sessions.
    pub total_reps: i64,
    /// Distinct phrases that have a practice_cards row.
    pub distinct_cards: i64,
    /// Consecutive days (ending today) with >=1 completed session.
    pub current_streak: i64,
    /// Cards currently due (existing due + brand-new seed candidates).
    pub due_count: i64,
}

/// A single logged practice attempt, returned by `practice_session_summary`
/// for a post-session recap. Mirrors the `practice_attempts` row (0/1 integer
/// flags decoded to bool).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PracticeAttemptSummary {
    pub phrase: String,
    pub correct: bool,
    pub first_try: bool,
    pub fire_ms: f64,
    pub backspaces: i64,
    pub corrections: i64,
    pub hint_used: bool,
    pub ts: i64,
}

/// One 5-minute activity block returned by `get_recent_blocks`.
/// `t` is the epoch-ms start of the bucket.
/// `wpm` is 0.0 if no data in the bucket.
/// `manual_words` = hand-typed; `chorded_words` = simultaneous chord bursts;
/// `arpeggio_words` = sequential-burst chords (in chordmap, max < arpeggio threshold).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActivityBlock {
    pub t: i64,
    pub wpm: f64,
    pub manual_words: Vec<String>,
    pub chorded_words: Vec<String>,
    pub arpeggio_words: Vec<String>,
}
