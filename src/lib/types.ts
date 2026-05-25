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
  /** Human-readable key combinations, one per device_chords row. E.g. ["p + t"]. Empty if no mapping found. */
  combos: string[];
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
