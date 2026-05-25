// TypeScript mirror of the Rust serde types in `src-tauri/src/types.rs`.
// Keep these in sync with the backend contract.

export interface Modifiers {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
}

export interface KeyEvent {
  code: string;
  key: string;
  pressed: boolean;
  modifiers: Modifiers;
  ts_ms: number;
}

export interface WordRecord {
  word: string;
  frequency: number;
  last_used: number;
  avg_speed_ms: number;
  score: number;
  /** clean_count / frequency — fraction of occurrences with zero backspace corrections */
  accuracy_rate: number;
}

export interface ChordRecord {
  phrase: string;
  frequency: number;
  last_used: number;
  avg_speed_ms: number;
  /** "chord" (simultaneous burst) | "arpeggio" (sequential burst, in chordmap) */
  kind: string;
}

/** source: "overall" | "chorded" | "manual" */
export interface WpmSample {
  t: number;
  wpm: number;
  source: string;
}

export interface WpmSummary {
  rolling: number;
  session: number;
  overall: number;
  chorded: number;
  manual: number;
}

export interface ChordCombo {
  /** "chord" or "compound" */
  kind: string;
  /** For chord: ["a + h + t"]. For compound: ["h + i + s", "a + l"]. */
  parts: string[];
  /** Existing chord phrases using the same key combination. */
  conflicts: string[];
}

export interface Suggestion {
  phrase: string;
  frequency: number;
  score: number;
  avg_manual_ms: number;
  projected_saving_ms: number;
  combos: ChordCombo[];
}

export interface Proficiency {
  phrase: string;
  usage_rate: number;
  fired_count: number;
  manual_count: number;
  avg_fire_ms: number;
  consistency: number;
  mastered: boolean;
  /** High-confidence errors: chord fired then same phrase manually retyped within 5s. */
  error_count: number;
  /** error_count / (fired_count + error_count) */
  error_rate: number;
  /** Lower-confidence: chord fired then N backstrokes deleted it within 3s. May include intentional edits. */
  deletion_count: number;
  /** deletion_count / (fired_count + deletion_count) */
  deletion_rate: number;
  /** Chord deleted then a different chord fired within the confusion window. */
  confusion_count: number;
  /** confusion_count / (fired_count + confusion_count) */
  confusion_rate: number;
  /** Human-readable key combinations, one per device_chords row. E.g. ["p + t"]. Empty if no mapping found. */
  combos: string[];
}

/** One candidate key combination in a coaching hint, with conflict info. */
export interface CoachingCombo {
  /** Display string for the combo, e.g. "w + o" or "h + i + s → a + l". */
  combo: string;
  /** Words that already occupy this key combination (empty = no conflict). */
  conflicts: string[];
}

/** Coaching overlay hint, emitted immediately on `manual` classification. */
export interface CoachingHint {
  id: number;
  phrase: string;
  primary_combo: string;
  alt_count: number;
  /** "device" | "suggested" */
  source: string;
  /** All candidate combos (primary first), with per-combo conflict lists. */
  combos: CoachingCombo[];
  /** Live settings snapshot at emit time (the overlay window can't see later
   *  Settings edits, so it reads these per-hint rather than once on mount). */
  persist: boolean;
  show_ms: number;
  fade_ms: number;
}

/** A screen rectangle in Tauri logical (NS, top-left origin) coords. */
export interface ScreenRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Coaching overlay caret position, emitted by the main-thread AX closure. */
export interface CoachingPosition {
  id: number;
  rect: ScreenRect;
}

export interface DeviceInfo {
  name: string;
  company: string;
  device: string;
  chipset: string;
  version: string;
  port: string;
  chord_count: number;
}

export interface SerialPortInfo {
  port: string;
  name: string;
}

export interface Settings {
  new_word_threshold_s: number;
  chord_char_threshold_ms: number;
  allowed_chars: string;
  /** Max ms between any two consecutive chars for a chordmap phrase to be classified as chorded (arpeggio gate). */
  arpeggio_threshold_ms: number;
  /** When true, chord_char_threshold_ms and arpeggio_threshold_ms are auto-derived from device settings on connect/refresh. */
  thresholds_auto: boolean;
  /** Time window (ms) after a chord deletion within which firing a different chord is logged as a confusion event. */
  chord_confusion_window_ms: number;
  /** Master toggle for the real-time chord coaching overlay. */
  coaching_enabled: boolean;
  /** How long (ms) the coaching overlay stays fully visible before fading. */
  coaching_show_ms: number;
  /** Fade-out duration (ms) of the coaching overlay. */
  coaching_fade_ms: number;
  /** Minimum manual word frequency before a suggested (chordless) combo is shown. */
  coaching_suggest_min_count: number;
  /** A previously-mastered chord whose usage_rate drops below this is re-surfaced. */
  coaching_resurface_rate: number;
  /** When true, the overlay stays until the next word (no auto-fade; clears on next word). */
  coaching_persist: boolean;
  /** When true, suppress reminders for already-mastered chords. Default false (show all). */
  coaching_hide_mastered: boolean;
}

/** Raw device settings read via VAR B1 queries. Fields are -1 when the query failed. */
export interface DeviceSettings {
  /** Keyboard output delay in µs (id 0x17). */
  output_delay_us: number;
  /** Arpeggiate timeout in ms (id 0x54). */
  arpeggiate_timeout_ms: number;
  arpeggiate_enabled: boolean;
  chord_press_tolerance_ms: number;
  chord_release_tolerance_ms: number;
  auto_delete_timeout_ms: number;
  chording_enabled: boolean;
  spurring_enabled: boolean;
}

export interface LoggingState {
  logging: boolean;
  db_unlocked: boolean;
}

export interface BanlistEntry {
  word: string;
  added: number;
}

/** One 5-minute activity block from get_recent_blocks. */
export interface ActivityBlock {
  t: number;
  wpm: number;
  manual_words: string[];
  chorded_words: string[];
  arpeggio_words: string[];
}
