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
